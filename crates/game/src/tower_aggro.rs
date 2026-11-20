//! Each receiver can refer only to slots declared on its own native scene.
use std::collections::HashMap;

use halcyon_protocol::{Message, TowerAggro};
use halcyon_sim::{Kind, World};

use super::Batch;

#[derive(Default)]
pub(super) struct View {
    actors: HashMap<u32, u8>,
    slots: HashMap<u8, u32>,
    // Target identity distinguishes two successive occupants of a recycled slot.
    previous: HashMap<u32, Option<(u32, u8)>>,
}

impl View {
    pub fn registered(&self, actor: u32, slot: u8) -> bool {
        self.actors.get(&actor) == Some(&slot)
    }

    pub fn declarations(&mut self, batch: &[Message]) {
        for m in batch {
            let slot = match m.opcode {
                0x03f2 => m.body.get(116),
                0x03f3 => m.body.get(745),
                _ => continue,
            };
            let Some(&slot @ 1..=199) = slot else {
                continue;
            };
            let wire = u32::from_be_bytes(m.body[8..12].try_into().unwrap());
            let Some(id) = wire.checked_sub(1499) else {
                continue;
            };
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
                self.previous.remove(&id);
            }
        }
    }

    pub fn append(&mut self, world: &World, visible: impl Fn(u32) -> bool, out: &mut Batch) {
        for tower in &world.entities {
            if tower.kind != Kind::Tower || !matches!(tower.type_id, 300 | 301 | 340) {
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
    ) {
        batch.retain_mut(|message| {
            if message.opcode != 0x03f3 {
                return true;
            }
            let wire = u32::from_be_bytes(message.body[8..12].try_into().unwrap());
            let Some(id) = wire.checked_sub(1499) else {
                return false;
            };
            let Ok(i) = world.entities.binary_search_by_key(&id, |e| e.id) else {
                return false;
            };
            let actor = &world.entities[i];
            let replacement = match self {
                Self::Aral(slots) => slots.declaration(world, actor, picks, data, 0, false),
                Self::Solo(slots) => slots.declaration(world, actor, picks, data),
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
            if !matches!(
                (message.opcode, &*self),
                (0x040b, Self::Aral(_)) | (0x0430, Self::Solo(_))
            ) {
                continue;
            }
            let Some(id) =
                u32::from_be_bytes(message.body[..4].try_into().unwrap()).checked_sub(1499)
            else {
                continue;
            };
            match self {
                Self::Aral(slots) => slots.destroy_sent(id, 0),
                Self::Solo(slots) => slots.released(id),
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
            .iter_mut()
            .find(|a| a.kind == Kind::Hero && a.team == 1)
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
        Message::new(0x03f2, body)
    }
    fn frames(view: &mut View, world: &World, visible: impl Fn(u32) -> bool) -> Vec<TowerAggro> {
        let mut out = Vec::new();
        view.append(world, visible, &mut out);
        out.iter().map(|m| TowerAggro::decode(m).unwrap()).collect()
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
    fn observer_snapshot_uses_its_own_declared_slots() {
        let (world, tower, hero) = test_world();
        let mut view = View::default();
        let mut scene = ObserverScene::Aral(Default::default());
        let picks: Vec<_> = world
            .entities
            .iter()
            .filter(|e| e.kind == Kind::Hero)
            .map(|_| super::super::Pick {
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
        scene.prepare(&world, &picks, None, &mut view, &mut batch);
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
        scene.prepare(&world, &picks, None, &mut view, &mut next);
        assert!(next.is_empty());
    }
}
