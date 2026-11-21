//! Tracked fire actions for measured native tower and minion profiles.
use halcyon_data::EntityTemplate;
use halcyon_protocol::{ProjectileAction, ProjectileFire};
use halcyon_sim::{Entity, Event, Kind, World};

use super::{Batch, compact, slots::tower_aggro::View};
use crate::transport::Delivery;
use crate::{Message, Room};

const TOWER_PROFILE: u32 = 403_962_522;
const MINION_PROFILE: u32 = 6_148_364;

pub(crate) struct Shot {
    wire_id: u32,
    source: u32,
    target: u32,
    profile: u32,
    scalar: f32,
    operand: u16,
}

fn profile(source: &Entity, target: &Entity) -> Option<(u32, f32, u16)> {
    let shooter = EntityTemplate::from_raw(source.type_id)?;
    let victim = EntityTemplate::from_raw(target.type_id);
    if source.kind == Kind::Tower
        && shooter == EntityTemplate::OuterTurret5v5
        && matches!(compact::structure_slot(source), Some(9 | 11 | 13))
        && target.kind == Kind::Minion
        && matches!(
            victim,
            Some(
                EntityTemplate::MinionRange5v5
                    | EntityTemplate::MinionMelee5v5
                    | EntityTemplate::MinionSiege5v5
            )
        )
    {
        return Some((TOWER_PROFILE, 0., 113));
    }
    let aral_minion =
        target.kind == Kind::Minion && victim.is_some_and(EntityTemplate::is_aral_lane_minion);
    if source.kind == Kind::Tower
        && matches!(shooter, EntityTemplate::Turret | EntityTemplate::VainTurret)
        && (target.kind == Kind::Hero || aral_minion)
    {
        return Some((TOWER_PROFILE, 0., 112));
    }
    if source.kind != Kind::Minion
        || !matches!(
            shooter,
            EntityTemplate::RangedMinion | EntityTemplate::LeadMinion
        )
        || source.level != 2
    {
        return None;
    }
    let scalar = if target.kind == Kind::Hero {
        if shooter == EntityTemplate::RangedMinion {
            14.
        } else {
            47.
        }
    } else if aral_minion {
        if shooter == EntityTemplate::RangedMinion {
            46.5
        } else {
            62.
        }
    } else if matches!(
        (target.kind, victim),
        (
            Kind::Tower,
            Some(EntityTemplate::Turret | EntityTemplate::VainTurret)
        ) | (
            Kind::Crystal,
            Some(EntityTemplate::VainCrystalHome3v3 | EntityTemplate::VainCrystalAway3v3)
        )
    ) {
        100.
    } else {
        return None;
    };
    Some((MINION_PROFILE, scalar, 79))
}

impl Shot {
    fn from_event(world: &World, event: &Event) -> Option<Self> {
        let Event::NpcProjectile { id, source, target } = *event else {
            return None;
        };
        let entity = |id| {
            world
                .entities
                .binary_search_by_key(&id, |e| e.id)
                .ok()
                .map(|i| &world.entities[i])
        };
        let a = entity(source)?;
        let b = entity(target)?;
        if a.team == b.team || a.hp <= 0. || b.hp <= 0. {
            return None;
        }
        let (profile, scalar, operand) = profile(a, b)?;
        Some(Self {
            wire_id: id.checked_add(1499)?,
            source,
            target,
            profile,
            scalar,
            operand,
        })
    }

    fn project(&self, view: &View, visible: &impl Fn(u32) -> bool) -> Option<Message> {
        if !visible(self.source) || !visible(self.target) {
            return None;
        }
        let a = view.slot(self.source)?;
        let b = view.slot(self.target)?;
        ProjectileAction::FireTracked(ProjectileFire {
            projectile: self.wire_id,
            profile_word: self.profile,
            values: [self.scalar],
            operand: self.operand,
            slots: [a, a, b],
            padding: &[0; 5],
        })
        .message()
    }
}

pub(crate) fn batch(shots: &[Shot], view: &View, visible: impl Fn(u32) -> bool) -> Batch {
    shots
        .iter()
        .filter_map(|s| s.project(view, &visible))
        .collect()
}

pub(crate) fn publish(room: &mut Room, events: &[Event]) {
    let shots: Vec<_> = events
        .iter()
        .filter_map(|e| Shot::from_event(&room.world, e))
        .collect();
    if shots.is_empty() {
        return;
    }
    for (i, seat) in room.seats.iter().enumerate() {
        if !seat.world_actor_written || !seat.preamble.confirmed() || seat.peers[1].is_some() {
            continue;
        }
        let Some(peer) = &seat.peers[0] else {
            continue;
        };
        let visible = &room.visible[usize::from(room.world.entities[i].team)];
        let out = batch(&shots, &room.scene_views[i], |id| visible.contains(&id));
        if !out.is_empty() {
            peer.send(Delivery::Private(out));
        }
    }
    if room.legacy_aral() {
        room.publish_observer_projectiles(&shots);
    }
}

pub(crate) fn recordable(m: &Message) -> bool {
    let Ok(halcyon_protocol::Fields::Projectile(ProjectileAction::FireTracked(f))) = m.fields()
    else {
        return false;
    };
    let [a, owner, b] = f.slots;
    if f.padding != [0; 5] || a != owner || a == b {
        return false;
    }
    match (f.profile_word, f.operand) {
        (TOWER_PROFILE, 113) => {
            matches!(a, 9 | 11 | 13) && (52..200).contains(&b) && f.values[0].to_bits() == 0
        }
        (TOWER_PROFILE, 112) => {
            matches!(a,3..=7|11..=15) && (16..200).contains(&b) && f.values[0].to_bits() == 0
        }
        (MINION_PROFILE, 79) => {
            (22..200).contains(&a)
                && matches!(b,1|3..=7|9|11..=199)
                && [14., 47., 46.5, 62., 100.].contains(&f.values[0])
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn register(view: &mut View, id: u32, slot: u8) {
        let mut b = [0; 122];
        b[8..12].copy_from_slice(&(id + 1499).to_be_bytes());
        b[116] = slot;
        view.declarations(&[Message::known(crate::Opcode::EntitySpawn, b)]);
    }
    #[test]
    fn shot_uses_its_own_id_and_each_receivers_live_slots() {
        let mut world = World::new(7, 6);
        world.use_six_human_roster().unwrap();
        world.use_aral_lane().unwrap();
        let tower = world
            .entities
            .iter()
            .find(|e| world.aral_structure_slot(e.id) == Some(7))
            .unwrap();
        let target = world
            .entities
            .iter()
            .find(|e| e.team != tower.team && e.kind == Kind::Hero)
            .unwrap();
        let (a, b) = (tower.id, target.id);
        let event = Event::NpcProjectile {
            id: 3000,
            source: a,
            target: b,
        };
        let shot = Shot::from_event(&world, &event).unwrap();
        let mut view = View::default();
        register(&mut view, a, 7);
        assert!(shot.project(&view, &|_| true).is_none());
        register(&mut view, b, 17);
        assert!(shot.project(&view, &|id| id == a).is_none());
        assert!(shot.project(&view, &|id| id == b).is_none());
        let mut msg = shot.project(&view, &|_| true).unwrap();
        assert_eq!(msg.body[..4], 4499u32.to_be_bytes());
        assert_eq!(msg.body[14..17], [7, 7, 17]);
        assert!(recordable(&msg));
        msg.body[19] = 1;
        assert!(!recordable(&msg));
        register(&mut view, b + 1000, 17);
        assert!(shot.project(&view, &|_| true).is_none());
        register(&mut view, b, 18);
        assert_eq!(
            shot.project(&view, &|_| true).unwrap().body[14..17],
            [7, 7, 18]
        );
        assert!(
            Shot::from_event(
                &world,
                &Event::Attack {
                    source: a,
                    target: b
                }
            )
            .is_none()
        );
    }

    #[test]
    fn solo_tower_projectile_requires_the_declared_target_and_current_visibility() {
        let mut world = World::new(1, 10);
        world.use_solo_mid_lane().unwrap();
        world.tick = 45 * halcyon_sim::TICK_HZ - 1;
        world.step();
        let target = world
            .entities
            .iter()
            .find(|e| e.kind == Kind::Minion && matches!(e.type_id, 335..=337) && e.team == 1)
            .unwrap()
            .id;
        let shot = Shot::from_event(
            &world,
            &Event::NpcProjectile {
                id: 900,
                source: 510,
                target,
            },
        )
        .unwrap();
        let mut view = View::default();
        register(&mut view, 510, 9);
        assert!(shot.project(&view, &|_| true).is_none());
        register(&mut view, target, 53);
        assert!(shot.project(&view, &|a| a != target).is_none());
        let m = shot.project(&view, &|_| true).unwrap();
        assert_eq!(m.body[12..17], [0, 113, 9, 9, 53]);
        assert!(recordable(&m));
        register(&mut view, target + 3000, 53);
        assert!(shot.project(&view, &|_| true).is_none());
        assert!(
            Shot::from_event(
                &world,
                &Event::NpcProjectile {
                    id: 901,
                    source: 510,
                    target: 2
                }
            )
            .is_none()
        );
    }

    #[test]
    fn minion_scalar_is_selected_only_for_measured_combinations() {
        let mut world = World::new(2, 6);
        world.use_six_human_roster().unwrap();
        world.use_aral_lane().unwrap();
        world.tick = 15 * halcyon_sim::TICK_HZ - 1;
        world.step();
        let mut source = world
            .entities
            .iter()
            .find(|e| e.kind == Kind::Minion)
            .unwrap()
            .clone();
        let opposing_minion = world
            .entities
            .iter()
            .find(|e| e.kind == Kind::Minion && e.team != source.team)
            .unwrap();
        let hero = &world.entities[0];
        let structure = world
            .entities
            .iter()
            .find(|e| e.kind == Kind::Crystal)
            .unwrap();
        for (k, h, m) in [(287, 14., 46.5), (288, 47., 62.)] {
            source.type_id = k;
            for (target, val) in [(hero, h), (opposing_minion, m), (structure, 100.)] {
                assert_eq!(profile(&source, target), Some((MINION_PROFILE, val, 79)));
            }
        }
        source.level = 3;
        assert!(profile(&source, hero).is_none());
        source.level = 2;
        source.type_id = 289;
        assert!(profile(&source, hero).is_none());
    }
}
