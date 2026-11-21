use halcyon_data::EntityTemplate;
use halcyon_protocol::{EntitySpawnBody, EntityStatMask, HeroSpawnBody, Message};
use halcyon_sim::{Entity, Kind, World};

use crate::Pick;
use crate::replication::{messages, messages::wire_id, slots::point_move};

/// One actual ARAL tower or crystal, registered at its observed map slot.
pub(crate) fn structure(world: &World, entity: &Entity) -> Option<Message> {
    let slot = world.aral_structure_slot(entity.id)?;
    let (height, forward, profile_word84) =
        match (entity.kind, EntityTemplate::from_raw(entity.type_id)?, slot) {
            (Kind::Crystal, EntityTemplate::VainCrystalHome3v3, 1) => (0.0, [0.0, 0.0, 1.0], 12.0),
            (Kind::Crystal, EntityTemplate::VainCrystalAway3v3, 9) => {
                (0.1, [0.104_528_464, 0.0, 0.994_521_9], 12.0)
            }
            (Kind::Tower, EntityTemplate::Turret, 5) => {
                (0.0, [0.965_925_8, 0.0, 0.258_819_07], 1.0)
            }
            (Kind::Tower, EntityTemplate::Turret, 13) => {
                (0.0, [-0.965_925_8, 0.0, 0.258_819_07], 1.0)
            }
            (Kind::Tower, EntityTemplate::Turret, 6 | 7) => (0.0, [1.0, 0.0, 0.0], 1.0),
            (Kind::Tower, EntityTemplate::Turret, 14 | 15) => (0.0, [-1.0, 0.0, 0.0], 1.0),
            (Kind::Tower, EntityTemplate::VainTurret, 3 | 4 | 11 | 12) => {
                let x = if entity.team == 0 {
                    0.766_044_44
                } else {
                    -0.766_044_44
                };
                (0.0, [x, 0.0, -0.642_787_64], 1.0)
            }
            _ => return None,
        };
    if !sane(entity) {
        return None;
    }
    let mut body = base(entity, EntitySpawnBody::STRUCTURE_SKIN, slot);
    body.position = [entity.position.x, height, entity.position.y];
    body.forward = forward;
    body.hp = entity.hp;
    body.max_hp = entity.max_hp;
    body.word84 = profile_word84;
    if entity.kind == Kind::Tower {
        body.word64 = 1.0;
    }
    body.set_team_visibility(1, EntitySpawnBody::VISIBILITY_ACTIVE_MASK);
    body.level = 1;
    body.lane_index = EntitySpawnBody::NO_LANE_INDEX;
    Some(body.encode())
}

/// Fixed map scenery. These actors have no simulation, vision, or economic effect.
pub(crate) fn scenery() -> impl Iterator<Item = Message> {
    [0, 2, 8, 10]
        .into_iter()
        .filter_map(scenery_body)
        .map(|body| body.encode())
}

fn scenery_body(slot: u8) -> Option<EntitySpawnBody> {
    let (template, position, direction, hp) = match slot {
        0 => (
            EntityTemplate::NeutralJungleMinionStore,
            [-88.5, 0.89, 2.0],
            [0.766_044_44, 0.0, -0.642_787_64],
            868.0,
        ),
        8 => (
            EntityTemplate::NeutralJungleMinionStore,
            [88.565, 1.8, 0.51],
            [0.0, 0.0, 1.0],
            868.0,
        ),
        2 => (
            EntityTemplate::VisionBeacon,
            [-76.12, 0.0, 19.9],
            [0.0, 0.0, 1.0],
            9999.0,
        ),
        10 => (
            EntityTemplate::VisionBeacon,
            [76.12, 0.1, 19.9],
            [0.104_528_464, -0.0, 0.994_521_9],
            9999.0,
        ),
        _ => return None,
    };
    let team = slot / 8;
    let mut body = EntitySpawnBody::new(
        u32::from(template.as_raw()),
        EntitySpawnBody::STRUCTURE_SKIN,
        2000 + u32::from(slot),
        slot,
        team + 1,
    );
    body.position = position;
    body.forward = direction;
    body.hp = hp;
    body.max_hp = hp;
    body.word84 = 12.0;
    let mask_fill = if template == EntityTemplate::NeutralJungleMinionStore {
        body.radius = 2.5;
        body.word64 = 1.0;
        3
    } else {
        0
    };
    body.set_team_visibility(mask_fill, EntitySpawnBody::VISIBILITY_ACTIVE_MASK);
    Some(body)
}

const FIRST_WAVE_SLOT: usize = 22;
const WAVE_SLOTS: usize = 200 - FIRST_WAVE_SLOT;

pub(crate) struct WaveSlots {
    slots: [Option<WaveActor>; WAVE_SLOTS],
    reported_full: bool,
}

#[derive(Clone, Copy)]
struct WaveActor {
    id: u32,
    exposed: u8,
}

impl Default for WaveSlots {
    fn default() -> Self {
        Self {
            slots: [None; WAVE_SLOTS],
            reported_full: false,
        }
    }
}

impl WaveSlots {
    pub fn declaration(
        &mut self,
        world: &World,
        entity: &Entity,
        picks: &[Pick],
        data: Option<&halcyon_data::GameData>,
        team: u8,
        fresh_wave: bool,
    ) -> Option<Message> {
        if !sane(entity) {
            return None;
        }
        if entity.kind == Kind::Hero {
            return Some(point_move::full_spawn(world, entity, picks, data, true));
        }
        if entity.kind != Kind::Minion {
            return structure(world, entity);
        }
        let slot = self.dynamic_slot(entity.id, team)?;
        let native_minion = EntityTemplate::from_raw(entity.type_id)
            .is_some_and(EntityTemplate::is_aral_lane_minion);
        if fresh_wave
            && native_minion
            && (2..=4).contains(&entity.level)
            && entity.hp == entity.max_hp
        {
            Some(pristine_minion(entity, slot))
        } else {
            let full = messages::spawn(entity, picks, data);
            let mut body =
                HeroSpawnBody::read_body(&full.body).expect("messages::spawn emits 750B HeroSpawn");
            if native_minion {
                body.skin = EntitySpawnBody::MINION_SKIN;
            }
            body.slot = slot;
            Some(body.encode())
        }
    }

    pub(crate) fn dynamic_slot(&mut self, id: u32, team: u8) -> Option<u8> {
        let existing = self
            .slots
            .iter()
            .position(|slot| slot.is_some_and(|old| old.id == id));
        let Some(index) = existing.or_else(|| self.slots.iter().position(Option::is_none)) else {
            if !std::mem::replace(&mut self.reported_full, true) {
                tracing::warn!("ARAL actor slots exhausted; deferring minion declaration");
            }
            return None;
        };
        let actor = self.slots[index].get_or_insert(WaveActor { id, exposed: 0 });
        actor.exposed |= 1 << team;
        Some((FIRST_WAVE_SLOT + index) as u8)
    }
    /// Reuse only after the delayed destroy for every team that saw the actor.
    pub fn destroy_sent(&mut self, id: u32, team: u8) {
        if let Some(slot) = self
            .slots
            .iter_mut()
            .find(|slot| slot.is_some_and(|actor| actor.id == id))
            && let Some(actor) = slot.as_mut()
        {
            actor.exposed &= !(1 << team);
            if actor.exposed == 0 {
                *slot = None;
            }
        }
    }
}

fn pristine_minion(entity: &Entity, slot: u8) -> Message {
    let mut body = base(entity, EntitySpawnBody::MINION_SKIN, slot);
    body.position = [entity.position.x, 0.0, entity.position.y];
    body.forward = [0.0, 0.0, 1.0];
    body.visibility_mask[1 + usize::from(entity.team)] = EntitySpawnBody::VISIBILITY_PRISTINE_MASK;
    body.set_stat_mask(EntityStatMask::Pristine);
    body.level = entity.level;
    body.lane_index = entity.team;
    body.encode()
}

/// Established visibility deltas for a newly declared, pristine ARAL wave actor.
/// The opposing index varies even for identical declarations.
#[derive(Clone, Copy)]
pub(crate) struct InitialVisibility {
    actor: u32,
    team: u8,
    changed: u8,
}

impl InitialVisibility {
    pub(crate) fn read(declaration: &Message) -> Option<Self> {
        if !declaration.is(crate::Opcode::EntitySpawn) {
            return None;
        }
        let spawn = EntitySpawnBody::read_body(&declaration.body).ok()?;
        if spawn.stat_mask() != EntityStatMask::Pristine || !is_recordable(&spawn) {
            return None;
        }
        let actor = spawn.actor;
        let team = spawn.team_index;
        let mut changed = 0;
        for index in 0..8u8 {
            if index == 3 - team {
                continue;
            }
            let i = usize::from(index);
            let mask = if index == team {
                EntitySpawnBody::VISIBILITY_ACTIVE_MASK
            } else {
                0
            };
            if [
                spawn.visibility_required[i],
                spawn.visibility_mask[i],
                spawn.visibility_aux[i],
            ] != [1, mask, 0]
            {
                changed |= 1 << index;
            }
        }
        (changed != 0).then_some(Self {
            actor,
            team,
            changed,
        })
    }
    pub(crate) fn rows(self) -> impl Iterator<Item = Message> {
        (0..8)
            .filter(move |&index| self.changed & (1 << index) != 0)
            .map(move |index| {
                let mask = if index == self.team {
                    EntitySpawnBody::VISIBILITY_ACTIVE_MASK
                } else {
                    0
                };
                Message::actor_visibility(self.actor, index, 1, [mask, 0])
            })
    }
}

pub(crate) fn recordable(body: &[u8]) -> bool {
    EntitySpawnBody::read_body(body)
        .ok()
        .is_some_and(|spawn| is_recordable(&spawn))
}

fn is_recordable(spawn: &EntitySpawnBody) -> bool {
    if spawn.word112 != u32::MAX {
        return false;
    }
    let Some(template) = u16::try_from(spawn.template)
        .ok()
        .and_then(EntityTemplate::from_raw)
    else {
        return false;
    };
    if matches!(
        template,
        EntityTemplate::VisionBeacon | EntityTemplate::NeutralJungleMinionStore
    ) {
        return scenery_body(spawn.slot).is_some_and(|scenery| spawn.bit_eq(&scenery));
    }
    match (
        template,
        [
            spawn.slot,
            spawn.flag117,
            spawn.flag118,
            spawn.level,
            spawn.lane_index,
            spawn.team_index,
        ],
    ) {
        (EntityTemplate::VainCrystalHome3v3, [1, 0, 0, 1, 255, 1])
        | (EntityTemplate::VainCrystalAway3v3, [9, 0, 0, 1, 255, 2])
        | (EntityTemplate::Turret, [5..=7, 0, 0, 1, 255, 1] | [13..=15, 0, 0, 1, 255, 2])
        | (EntityTemplate::VainTurret, [3..=4, 0, 0, 1, 255, 1] | [11..=12, 0, 0, 1, 255, 2]) => {
            spawn.skin == EntitySpawnBody::STRUCTURE_SKIN
                && spawn.actor == 2000 + u32::from(spawn.slot)
        }
        (kind, [22..=199, 1, 0, 2..=4, 0, 1] | [22..=199, 1, 0, 2..=4, 1, 2])
            if kind.is_aral_lane_minion() =>
        {
            spawn.skin == EntitySpawnBody::MINION_SKIN
                && spawn.hp.to_bits() == 0
                && spawn.max_hp.to_bits() == 0
                && spawn.reserved_44 == [0; 8]
                && spawn.radius.to_bits() == 0
                && spawn.reserved_56 == [0; 8]
                && spawn.word64.to_bits() == 0
                && spawn.reserved_68 == [0; 16]
                && spawn.word84.to_bits() == 0
        }
        _ => false,
    }
}

fn sane(entity: &Entity) -> bool {
    (1..=u32::from(u16::MAX) - 1500).contains(&entity.id)
        && entity.team < 2
        && entity.position.x.is_finite()
        && entity.position.y.is_finite()
        && entity.hp.is_finite()
        && entity.max_hp.is_finite()
        && entity.hp > 0.0
        && entity.hp <= entity.max_hp
}

fn base(entity: &Entity, skin: u32, slot: u8) -> EntitySpawnBody {
    EntitySpawnBody {
        template: u32::from(entity.type_id),
        skin,
        actor: u32::from(wire_id(entity.id)),
        word112: u32::MAX,
        slot,
        team_index: entity.team + 1,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_scenery_encodes_only_the_observed_profiles() {
        let messages: Vec<_> = scenery().collect();
        assert_eq!(messages.len(), 4);
        for (message, (slot, template, position, hp)) in messages.into_iter().zip([
            (0, 314u32, [-88.5_f32, 0.89, 2.0], 868.0_f32),
            (2, 298, [-76.12, 0.0, 19.9], 9999.0),
            (8, 314, [88.565, 1.8, 0.51], 868.0),
            (10, 298, [76.12, 0.1, 19.9], 9999.0),
        ]) {
            let mut b = message.body;
            assert_eq!(message.opcode, crate::wire_opcode::EntitySpawn);
            assert!(recordable(&b));
            assert_eq!(b[..4], template.to_be_bytes());
            assert_eq!(b[8..12], (2000 + u32::from(slot)).to_be_bytes());
            assert_eq!(
                b[12..24],
                position
                    .into_iter()
                    .flat_map(f32::to_be_bytes)
                    .collect::<Vec<_>>()
            );
            assert_eq!(b[36..40], hp.to_be_bytes());
            assert_eq!(b[40..44], hp.to_be_bytes());
            assert_eq!(b[116..122], [slot, 0, 0, 1, 255, slot / 8 + 1]);
            b[11] ^= 1;
            assert!(!recordable(&b));
        }
    }

    #[test]
    fn pristine_wave_uses_compact_and_damaged_snapshot_keeps_the_same_slot() {
        let mut world = World::new(1, 6);
        world.use_six_human_roster().unwrap();
        world.assign_teams(&[]).unwrap();
        world.use_aral_lane().unwrap();
        for _ in 0..15 * halcyon_sim::TICK_HZ {
            world.step();
        }
        let mut wave = world.entities.iter().filter(|e| e.kind == Kind::Minion);
        let first = wave.next().unwrap().clone();
        let second = wave.next().unwrap().clone();
        assert_eq!(first.type_id, 289);
        assert_eq!(second.type_id, 289);
        let mut slots = WaveSlots::default();
        for entity in [&first, &second] {
            let fresh = slots
                .declaration(&world, entity, &[], None, 0, true)
                .unwrap();
            assert_eq!(fresh.opcode, crate::wire_opcode::EntitySpawn);
            assert!(recordable(&fresh.body));
            let team = fresh.body[121];
            let mut prefilled = fresh.clone();
            prefilled.body[88 + 6] = 1;
            let rows: Vec<_> = InitialVisibility::read(&prefilled)
                .unwrap()
                .rows()
                .collect();
            assert_eq!(rows.len(), 6);
            assert!(rows.iter().all(|m| m.body[4] != 6 && m.body[4] != 3 - team));
            assert!(rows.iter().any(|m| m.body[4..8] == [team, 1, 15, 0]));

            let mut wide_template = fresh.body.clone();
            wide_template[0] = 1;
            assert!(!recordable(&wide_template));
            let mut malformed = fresh.body.clone();
            malformed[120] ^= 1;
            assert!(!recordable(&malformed));
            assert_eq!(fresh.body[116], 22 + entity.team);
            assert_eq!(fresh.body[117], 1);
            assert_eq!(fresh.body[119], 2);
            assert_eq!(fresh.body[120], entity.team);
            assert_eq!(fresh.body[12..16], entity.position.x.to_be_bytes());
            assert_eq!(fresh.body[20..24], entity.position.y.to_be_bytes());
        }
        let mut damaged = first.clone();
        damaged.hp -= 14.0;
        let restored = slots
            .declaration(&world, &damaged, &[], None, 1, false)
            .unwrap();
        assert_eq!(restored.opcode, crate::wire_opcode::HeroSpawn);
        assert!(InitialVisibility::read(&restored).is_none());
        assert_eq!(restored.body[745], 22);
        assert_eq!(restored.body[42..46], damaged.hp.to_be_bytes());
        assert_eq!(restored.body[46..50], damaged.max_hp.to_be_bytes());

        let mut custom = first.clone();
        custom.type_id = u16::MAX;
        let custom_actor = slots
            .declaration(&world, &custom, &[], None, 0, true)
            .unwrap();
        assert!(custom_actor.is(crate::Opcode::HeroSpawn));
        assert_eq!(&custom_actor.body[2..4], &custom.type_id.to_be_bytes());
        slots.destroy_sent(first.id, 0);
        let mut later = first.clone();
        later.id += 100;
        let before_other_team = slots
            .declaration(&world, &later, &[], None, 0, true)
            .unwrap();
        assert_eq!(before_other_team.body[116], 24);
        slots.destroy_sent(first.id, 1);
        later.id += 1;
        let after_both_teams = slots
            .declaration(&world, &later, &[], None, 0, true)
            .unwrap();
        assert_eq!(after_both_teams.body[116], 22);
    }
}
