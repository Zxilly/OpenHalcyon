//! Each receiver can refer only to slots declared on its own native scene.
use halcyon_data::EntityTemplate;
use std::collections::HashMap;

use halcyon_protocol::{Message, TowerAggro};
use halcyon_sim::{Kind, World};

use super::Batch;

#[derive(Default)]
pub(super) struct View {
    solo_bot_paths: super::solo_bot_paths::Paths,
    pub(super) guard_links: super::aral_guard::State,
    pub(super) chakrams: [Option<u32>; 6],
    pub(super) infusion_effects: [super::aral_item_actions::InfusionWitness; 6],
    pub(super) reflex_effects: [Option<u32>; 6],
    pub(super) ringo_effects: [Option<u32>; 6],
    pub(super) shop_base_declared: bool,
    pub(super) shop_river_declared: bool,
    pub(super) shop_can_icon_sent: bool,
    pub(super) shop_allow_next_tick: Option<u64>,
    pub(super) shop_allow_active_instance: Option<u32>,
    pub(super) shop_allow_seq: u32,
    actors: HashMap<u32, u8>,
    slots: HashMap<u8, u32>,
    // Target identity distinguishes two successive occupants of a recycled slot.
    previous: HashMap<u32, Option<(u32, u8)>>,
    lane_paths: HashMap<u8, LanePath>,
    spawn_visibility: HashMap<u8, super::aral_spawn::InitialVisibility>,
}

struct LanePath {
    actor: u32,
    destination: halcyon_sim::Position,
    tick: u64,
}

impl View {
    pub fn slot(&self, id: u32) -> Option<u8> {
        self.actors.get(&id).copied()
    }
    pub(super) fn release_effect(&mut self, id: u32) -> bool {
        if let Some(slot) = self.actors.remove(&id) {
            self.slots.remove(&slot);
            self.lane_paths.remove(&slot);
            self.spawn_visibility.remove(&slot);
            self.previous.remove(&id);
            true
        } else {
            false
        }
    }
    pub fn append_solo_bot_paths(
        &mut self,
        world: &World,
        viewer: &halcyon_sim::Entity,
        visible: &std::collections::HashSet<u32>,
        events: &[halcyon_sim::Event],
        out: &mut Batch,
    ) {
        self.solo_bot_paths
            .append(world, viewer, visible, &self.actors, events, out);
    }
    pub fn knows(&self, actor: u32) -> bool {
        self.actors.contains_key(&actor)
    }

    pub fn registered(&self, actor: u32, slot: u8) -> bool {
        self.actors.get(&actor) == Some(&slot)
    }

    pub fn declarations(&mut self, batch: &[Message]) {
        for m in batch {
            let (slot, wire, is_vain_node) = match m.known_opcode() {
                Some(crate::Opcode::EntitySpawn) => {
                    let Ok(spawn) = halcyon_protocol::EntitySpawnBody::read_body(&m.body) else {
                        continue;
                    };
                    (
                        spawn.slot,
                        spawn.actor,
                        spawn.template == u32::from(EntityTemplate::VainNode.as_raw()),
                    )
                }
                Some(crate::Opcode::HeroSpawn) => {
                    let Ok(spawn) =
                        halcyon_protocol::HeroSpawnBody::read_unpadded_or_padded(&m.body)
                    else {
                        continue;
                    };
                    (
                        spawn.slot,
                        spawn.actor,
                        spawn.template as u16 == EntityTemplate::VainNode.as_raw(),
                    )
                }
                _ => continue,
            };
            if slot > 199 {
                continue;
            }
            // Full declarations may use zero as an unset slot. Only the Solo
            // template-297 armory has a verified real registration there.
            if slot == 0 && !(wire == 2000 && is_vain_node) {
                continue;
            }
            let Some(id) = wire.checked_sub(1499) else {
                continue;
            };
            if let Some(initial) = super::aral_spawn::InitialVisibility::read(m) {
                self.spawn_visibility.insert(slot, initial);
            } else {
                self.spawn_visibility.remove(&slot);
            }
            if let Some(old) = self.slots.insert(slot, id)
                && old != id
            {
                self.actors.remove(&old);
                self.previous.remove(&old);
            }
            if let Some(old) = self.actors.insert(id, slot)
                && old != slot
            {
                self.slots.remove(&old);
                self.spawn_visibility.remove(&old);
                self.previous.remove(&id);
            }
        }
    }

    fn append_lane_path(
        &mut self,
        tick: u64,
        actor: &halcyon_sim::Entity,
        visible: &impl Fn(u32) -> bool,
        out: &mut Batch,
    ) {
        let Some(&slot) = self.actors.get(&actor.id) else {
            return;
        };
        if actor.hp <= 0. || !visible(actor.id) {
            self.spawn_visibility.remove(&slot);
            self.lane_paths.remove(&slot);
            return;
        }
        let Some((destination, followed)) = actor.lane_step() else {
            return;
        };
        if followed.is_some_and(|id| !visible(id) || !self.actors.contains_key(&id)) {
            self.lane_paths.remove(&slot);
            return;
        }
        if let Some(last) = self.lane_paths.get(&slot)
            && last.actor == actor.id
            && (last.destination == destination
                || tick.saturating_sub(last.tick) < halcyon_sim::TICK_HZ / 5)
        {
            return;
        }
        self.lane_paths.insert(
            slot,
            LanePath {
                actor: actor.id,
                destination,
                tick,
            },
        );
        out.push(super::point_move::destination(slot, destination));
        if let Some(initial) = self.spawn_visibility.remove(&slot) {
            out.extend(initial.rows());
        }
    }

    pub fn append(&mut self, world: &World, visible: impl Fn(u32) -> bool, out: &mut Batch) {
        for actor in &world.entities {
            if actor.kind == Kind::Minion
                && EntityTemplate::from_raw(actor.type_id)
                    .is_some_and(|t| t.is_aral_lane_minion() || t.is_solo_lane_minion())
            {
                self.append_lane_path(world.tick, actor, &visible, out);
                continue;
            }
            let tower = actor;
            if tower.kind != Kind::Tower
                || !matches!(
                    EntityTemplate::from_raw(tower.type_id),
                    Some(
                        EntityTemplate::Turret
                            | EntityTemplate::VainTurret
                            | EntityTemplate::OuterTurret5v5
                    )
                )
            {
                continue;
            }
            let Some(&slot) = self.actors.get(&tower.id) else {
                continue;
            };
            if tower.hp <= 0. || !visible(tower.id) {
                self.previous.remove(&tower.id);
                continue;
            }
            let target = tower.attack_target().and_then(|id| {
                let target =
                    &world.entities[world.entities.binary_search_by_key(&id, |e| e.id).ok()?];
                if target.hp <= 0.
                    || target.team == tower.team
                    || !matches!(target.kind, Kind::Hero | Kind::Minion)
                    || !visible(id)
                {
                    return None;
                }
                self.actors.get(&id).map(|&slot| (id, slot))
            });
            if self.previous.get(&tower.id) == Some(&target) {
                continue;
            }
            self.previous.insert(tower.id, target);
            out.push(
                TowerAggro {
                    tower_slot: slot,
                    target_slot: target.map(|(_, slot)| slot),
                }
                .message()
                .unwrap(),
            );
        }
    }
}

pub(super) enum ObserverScene {
    Aral(Box<super::aral_spawn::WaveSlots>),
    Solo(Box<super::actor_slots::ActorSlots>),
}

impl ObserverScene {
    pub fn new(room: &super::Room) -> Option<Self> {
        if room.legacy_aral() {
            Some(Self::Aral(Default::default()))
        } else if room.solo_scene() && room.seats[0].control_world {
            Some(Self::Solo(Box::new(
                super::actor_slots::ActorSlots::from_roster(&room.world.entities),
            )))
        } else {
            None
        }
    }

    pub fn prepare(
        &mut self,
        world: &World,
        picks: &[super::Pick],
        data: Option<&halcyon_data::GameData>,
        view: &mut View,
        batch: &mut Batch,
        fresh_spawns: bool,
    ) {
        batch.retain_mut(|message| {
            if !message.is(crate::Opcode::HeroSpawn) {
                return true;
            }
            let Ok(spawn) = halcyon_protocol::HeroSpawnBody::read_unpadded_or_padded(&message.body)
            else {
                return false;
            };
            let Some(id) = spawn.actor.checked_sub(1499) else {
                return false;
            };
            let Ok(i) = world.entities.binary_search_by_key(&id, |e| e.id) else {
                return false;
            };
            let actor = &world.entities[i];
            let replacement = match self {
                Self::Aral(slots) => slots.declaration(world, actor, picks, data, 0, fresh_spawns),
                Self::Solo(slots) => slots.declaration_at(world, actor, picks, data, fresh_spawns),
            };
            if let Some(m) = replacement {
                *message = m;
                true
            } else {
                false
            }
        });
        view.declarations(batch);
        self.release(batch);
        view.append(world, |_| true, batch);
    }

    pub fn release(&mut self, batch: &[Message]) {
        for message in batch {
            if message.opcode != crate::wire_opcode::DestroyEntity {
                continue;
            }
            let Ok(destroy) = halcyon_protocol::DestroyEntityBody::read_body(&message.body) else {
                continue;
            };
            let Some(id) = destroy.actor.checked_sub(1499) else {
                continue;
            };
            match self {
                Self::Aral(slots) => slots.destroy_sent(id, 0),
                Self::Solo(slots) => slots.destroy_sent(id),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use halcyon_sim::Position;

    fn test_world() -> (World, u32, u32) {
        let mut world = World::new(1, 6);
        world.use_six_human_roster().unwrap();
        world.use_aral_lane().unwrap();
        let tower = world
            .entities
            .iter()
            .find(|a| world.aral_structure_slot(a.id) == Some(7))
            .unwrap();
        let (tid, pos) = (tower.id, tower.position);
        let hero = world
            .entities
            .find_mut(|a| a.kind == Kind::Hero && a.team == 1)
            .unwrap();
        let hid = hero.id;
        hero.position = Position {
            x: pos.x + 1.,
            y: pos.y,
        };
        for _ in 0..20 {
            world.step();
            if world
                .entities
                .iter()
                .find(|a| a.id == tid)
                .unwrap()
                .attack_target()
                == Some(hid)
            {
                return (world, tid, hid);
            }
        }
        panic!("tower never acquired nearby enemy hero")
    }

    fn declaration(id: u32, slot: u8) -> Message {
        let mut body = vec![0; 122];
        body[8..12].copy_from_slice(&(id + 1499).to_be_bytes());
        body[116] = slot;
        Message::known(crate::Opcode::EntitySpawn, body)
    }
    fn frames(view: &mut View, world: &World, visible: impl Fn(u32) -> bool) -> Vec<TowerAggro> {
        let mut out = Vec::new();
        view.append(world, visible, &mut out);
        out.iter().map(|m| TowerAggro::decode(m).unwrap()).collect()
    }

    #[test]
    fn native_slot_zero_is_reserved_for_the_declared_solo_armory() {
        let mut view = View::default();
        let mut armory = halcyon_protocol::SoloStructure::at_slot(0)
            .unwrap()
            .message(2000);
        view.declarations(std::slice::from_ref(&armory));
        assert_eq!(view.slot(501), Some(0));
        armory.body[8..12].copy_from_slice(&2001u32.to_be_bytes());
        view.declarations(&[armory, declaration(1, 0)]);
        assert_eq!(view.slot(501), Some(0));
        assert_eq!(view.slot(502), None);
        assert_eq!(view.slot(1), None);
    }

    #[test]
    fn live_target_requires_both_declarations_and_current_visibility() {
        let (world, tower, hero) = test_world();
        let mut view = View::default();
        assert!(frames(&mut view, &world, |_| true).is_empty());
        view.declarations(&[declaration(tower, 7)]);
        let state = |target_slot| TowerAggro {
            tower_slot: 7,
            target_slot,
        };
        assert_eq!(frames(&mut view, &world, |_| true), [state(None)]);
        view.declarations(&[declaration(hero, 17)]);
        assert_eq!(frames(&mut view, &world, |_| true), [state(Some(17))]);
        assert!(frames(&mut view, &world, |_| true).is_empty());
        assert_eq!(frames(&mut view, &world, |id| id != hero), [state(None)]);
        assert!(frames(&mut view, &world, |id| id != tower).is_empty());
        assert_eq!(frames(&mut view, &world, |_| true), [state(Some(17))]);
        view.declarations(&[declaration(10000, 17)]);
        assert_eq!(frames(&mut view, &world, |_| true), [state(None)]);
        let mut reconnect = View::default();
        reconnect.declarations(&[declaration(tower, 7), declaration(hero, 17)]);
        assert_eq!(frames(&mut reconnect, &world, |_| true), [state(Some(17))]);
    }

    #[test]
    fn moving_lane_minions_use_each_receivers_slot_and_current_destination() {
        let (mut world, _, _) = test_world();
        world.tick = 15 * halcyon_sim::TICK_HZ - 1;
        let mut view = View::default();
        let mut observer = View::default();
        let mut out = Vec::new();
        let id = loop {
            world.step();
            if let Some(e) = world.entities.iter().find(|e| {
                e.kind == Kind::Minion && e.team == 0 && matches!(e.lane_step(), Some((_, None)))
            }) {
                break e.id;
            }
            assert!(
                world.tick < 320,
                "expected the first wave's authoritative movement"
            );
        };
        let current = |w: &World| {
            w.entities
                .iter()
                .find(|e| e.id == id)
                .unwrap()
                .lane_step()
                .unwrap()
                .0
        };
        let first = current(&world);
        view.append(&world, |_| true, &mut out);
        assert!(out.is_empty());
        view.declarations(&[declaration(id, 22)]);
        observer.declarations(&[declaration(id, 73)]);
        view.append(&world, |_| true, &mut out);
        observer.append(&world, |_| true, &mut out);
        assert_eq!(
            out,
            [
                crate::point_move::destination(22, first),
                crate::point_move::destination(73, first)
            ]
        );
        out.clear();
        view.append(&world, |_| true, &mut out);
        assert!(out.is_empty());
        view.append(&world, |_| false, &mut out);
        view.append(&world, |_| true, &mut out);
        assert_eq!(out, [crate::point_move::destination(22, first)]);
        out.clear();
        let m = world.entities.iter().find(|e| e.id == id).unwrap().position;
        let enemy = world
            .entities
            .find_mut(|e| e.team == 1 && e.kind == Kind::Hero)
            .unwrap();
        let hero = enemy.id;
        enemy.position = Position {
            x: m.x + 6.,
            y: m.y,
        };
        for _ in 0..16 {
            world.step();
            if matches!(world.entities.iter().find(|e|e.id==id).unwrap().lane_step(),Some((_,Some(target))) if target==hero)
            {
                break;
            }
        }
        let minion = world.entities.iter().find(|e| e.id == id).unwrap();
        let (point, target) = minion.lane_step().expect("expected pursuit");
        assert_eq!(target, Some(hero));
        view.append(&world, |_| true, &mut out);
        assert!(out.is_empty(), "unregistered pursuit target");
        view.declarations(&[declaration(hero, 17)]);
        view.append(&world, |a| a != hero, &mut out);
        assert!(out.is_empty(), "hidden pursuit target");
        let t = world.tick + 4;
        view.append_lane_path(t, minion, &|_| true, &mut out);
        assert_eq!(out, [crate::point_move::destination(22, point)]);
        out.clear();
        let mut recycled = minion.clone();
        recycled.id += 10000;
        view.declarations(&[declaration(recycled.id, 22)]);
        view.append_lane_path(t, &recycled, &|_| true, &mut out);
        assert_eq!(out, [crate::point_move::destination(22, point)]);
        out.clear();
        view.append_lane_path(t, minion, &|_| true, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn observer_snapshot_uses_its_own_declared_slots() {
        let (world, tower, hero) = test_world();
        let mut view = View::default();
        let mut scene = ObserverScene::Aral(Default::default());
        let picks: Vec<_> = world
            .entities
            .iter()
            .filter(|e| e.kind == Kind::Hero)
            .map(|_| super::super::Pick {
                preferences: Default::default(),
                name: "Ringo".into(),
                skin: 0,
                selected: true,
                locked: true,
            })
            .collect();
        let mut batch: Vec<_> = [tower, hero]
            .iter()
            .map(|id| {
                let e = world.entities.iter().find(|e| e.id == *id).unwrap();
                super::super::messages::spawn(e, &picks, None)
            })
            .collect();
        scene.prepare(&world, &picks, None, &mut view, &mut batch, false);
        assert_eq!(batch[0].body[116], 7);
        let slot = batch[1].body[745];
        assert_eq!(
            TowerAggro::decode(batch.last().unwrap()),
            Some(TowerAggro {
                tower_slot: 7,
                target_slot: Some(slot)
            })
        );
        let mut next = Vec::new();
        scene.prepare(&world, &picks, None, &mut view, &mut next, false);
        assert!(next.is_empty());
    }
}
