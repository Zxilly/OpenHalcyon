use halcyon_protocol::Message;
use halcyon_sim::{Entity, Kind};

use super::{Pick, compact, messages};

const FIRST_MINION_SLOT: u8 = 52;
const ACTOR_SLOTS: u8 = 200;
const FULL_SLOT_OFFSET: usize = 745;

pub(super) struct ActorSlots {
    heroes: [Option<u8>; 10],
    minions: [Option<u32>; (ACTOR_SLOTS - FIRST_MINION_SLOT) as usize],
    reported_full: bool,
}

fn set_full(message: &mut Message, slot: u8) {
    debug_assert_eq!(message.opcode, 0x03f3);
    message.body[FULL_SLOT_OFFSET] = slot;
}

impl ActorSlots {
    pub fn from_roster(entities: &[Entity]) -> Self {
        let mut heroes = [None; 10];
        let mut next_slot = [14, 19];
        for hero in entities.iter().filter(|entity| entity.kind == Kind::Hero) {
            let team = usize::from(hero.team);
            if (1..=10).contains(&hero.id) && team < 2 && next_slot[team] < [19, 24][team] {
                heroes[(hero.id - 1) as usize] = Some(next_slot[team]);
                next_slot[team] += 1;
            }
        }
        Self {
            heroes,
            minions: [None; (ACTOR_SLOTS - FIRST_MINION_SLOT) as usize],
            reported_full: false,
        }
    }

    fn fixed(&self, entity: &Entity) -> Option<u8> {
        match (entity.kind, entity.type_id, entity.team) {
            (Kind::Hero, _, _) if (1..=10).contains(&entity.id) => {
                self.heroes[(entity.id - 1) as usize]
            }
            (Kind::Crystal, 295, 0) => Some(6),
            (Kind::Crystal, 296, 1) => Some(7),
            (Kind::Tower, 340, 0) => Some(9),
            (Kind::Tower, 340, 1) => Some(12),
            _ => None,
        }
    }

    pub fn declaration(
        &mut self,
        tick: u64,
        entity: &Entity,
        picks: &[Pick],
        data: Option<&halcyon_data::GameData>,
    ) -> Option<Message> {
        if !(1..=u32::from(u16::MAX) - 1500).contains(&entity.id) {
            return None;
        }
        if entity.kind == Kind::Minion {
            if let Some(slot) = self.minion(entity.id) {
                return Some(compact::minion(entity, slot).unwrap_or_else(|| {
                    let mut message = messages::spawn(entity, picks, data);
                    set_full(&mut message, slot);
                    message
                }));
            }
            if !std::mem::replace(&mut self.reported_full, true) {
                tracing::warn!("native Solo actor slots exhausted; deferring minion declaration");
            }
            return None;
        }
        let slot = self.fixed(entity)?;
        Some(compact::structure(tick, entity).unwrap_or_else(|| {
            let mut message = messages::spawn(entity, picks, data);
            if entity.kind == Kind::Hero {
                messages::solo_hero_profile(&mut message, entity, tick);
            }
            set_full(&mut message, slot);
            message
        }))
    }

    fn minion(&mut self, id: u32) -> Option<u8> {
        let index = self
            .minions
            .iter()
            .position(|slot| *slot == Some(id))
            .or_else(|| self.minions.iter().position(Option::is_none))?;
        self.minions[index] = Some(id);
        Some(FIRST_MINION_SLOT + index as u8)
    }

    pub fn released(&mut self, id: u32) {
        if let Some(slot) = self.minions.iter_mut().find(|slot| **slot == Some(id)) {
            *slot = None;
        }
    }

    /// A new scene gets a fresh client table. Preserve indices of living actors.
    pub fn retain_living(&mut self, entities: &[Entity]) {
        for slot in &mut self.minions {
            if slot.is_some_and(|id| !entities.iter().any(|e| e.id == id && e.hp > 0.0)) {
                *slot = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declaration_preserves_hero_and_live_minion_indices_across_scenes() {
        let mut world = halcyon_sim::World::new(7, 1);
        let mut slots = ActorSlots::from_roster(&world.entities);
        for entity in &world.entities[..10] {
            let hero = slots.declaration(0, entity, &[], None).unwrap();
            assert_eq!(hero.opcode, 0x03f3);
            let expected = [14, 19, 15, 20, 16, 21, 17, 22, 18, 23];
            assert_eq!(hero.body[745], expected[(entity.id - 1) as usize]);
        }
        while world.tick < 900 {
            world.step();
        }
        let indices: Vec<_> = world
            .entities
            .iter()
            .filter(|entity| entity.kind == Kind::Minion)
            .map(|entity| {
                let spawn = slots.declaration(world.tick, entity, &[], None).unwrap();
                assert_eq!((spawn.opcode, spawn.body.len()), (0x03f2, 122));
                assert_eq!(spawn.body[117], 0);
                assert_eq!(spawn.body[120], [1, 4][entity.team as usize]);
                assert_eq!(spawn.body[121], entity.team + 1);
                assert_eq!(spawn.body[36..40], entity.hp.to_be_bytes());
                assert_eq!(spawn.body[40..44], entity.max_hp.to_be_bytes());
                (entity.id, spawn.body[116])
            })
            .collect();
        assert_eq!(indices.len(), 8);
        let [(dead, released), (living, stable), ..] = indices.as_slice() else {
            panic!()
        };
        world
            .entities
            .iter_mut()
            .find(|e| e.id == *dead)
            .unwrap()
            .hp = 0.0;
        let living = world.entities.iter_mut().find(|e| e.id == *living).unwrap();
        living.hp -= 25.0;
        let living = living.clone();
        slots.retain_living(&world.entities);
        let snapshot = slots.declaration(world.tick, &living, &[], None).unwrap();
        assert_eq!(snapshot.body[116], *stable);
        assert_eq!(snapshot.body[36..40], living.hp.to_be_bytes());
        assert_eq!(snapshot.body[40..44], living.max_hp.to_be_bytes());
        assert_eq!(slots.minion(4000), Some(*released));
    }

    #[test]
    fn solo_hero_declarations_keep_identity_separate_from_initial_team_flags() {
        let world = halcyon_sim::World::new(7, 1);
        let mut slots = ActorSlots::from_roster(&world.entities);
        for actor in world.entities.iter().filter(|e| e.kind == Kind::Hero) {
            let first = slots.declaration(0, actor, &[], None).unwrap();
            let late = slots.declaration(90, actor, &[], None).unwrap();
            for m in [&first, &late] {
                let selector = u32::from_be_bytes(m.body[709..713].try_into().unwrap());
                assert_eq!(
                    selector,
                    if actor.id == 1 {
                        u32::MAX
                    } else {
                        u32::from(actor.team)
                    }
                );
                assert_eq!(&m.body[741..745], &[255; 4]);
                assert_eq!(m.body[745], 13 + actor.id as u8);
            }
            assert_eq!(&first.body[713..721], &[1; 8]);
            let mut flags = [0; 8];
            flags[1 + actor.team as usize] = 15;
            assert_eq!(&first.body[721..729], &flags);
            assert_eq!(&late.body[713..729], &[0; 16]);
        }
    }

    #[tokio::test]
    async fn replication_defers_unregistered_actors_until_a_slot_is_free() {
        use crate::{Peer, Rooms, messages::wire_id, transport::Delivery};
        use halcyon_platform::{MatchHost, Reservation};
        use halcyon_sim::Event;
        use tokio::sync::{mpsc, watch};
        use uuid::Uuid;

        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let match_id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                match_id,
                players: vec![Uuid::new_v4()],
                teams: vec![0],
                mode: "5v5_bots_solo".into(),
                difficulty: None,
            })
            .unwrap();
        let room = rooms.room(match_id).unwrap();
        let mut room = room.lock().unwrap();
        let (tx, mut rx) = mpsc::channel(8);
        room.seats[0].control_world = true;
        room.seats[0].peers[1] = Some(Peer {
            generation: 0,
            tx,
            stalled: watch::channel(false).0,
        });
        while room.world.tick < 900 {
            room.world.step();
        }
        let minion = room
            .world
            .entities
            .iter()
            .find(|e| e.team == 0 && e.kind == Kind::Minion)
            .unwrap();
        let id = minion.id;
        let move_event = Event::Move {
            id,
            position: minion.position,
        };
        for slot in FIRST_MINION_SLOT..ACTOR_SLOTS {
            assert_eq!(room.solo_slots.minion(1000 + u32::from(slot)), Some(slot));
        }
        room.replicate(std::slice::from_ref(&move_event));
        let Delivery::Private(batch) = rx.try_recv().unwrap() else {
            panic!()
        };
        assert!(
            batch
                .iter()
                .all(|m| m.opcode != 0x03f2 || m.body[116] < FIRST_MINION_SLOT)
        );
        assert!(batch.iter().all(|m| m.opcode != 0x042e));
        assert!(!room.visible[0].contains(&id));
        assert!(room.world.entities.iter().any(|e| e.id == id && e.hp > 0.0));

        room.solo_slots.released(1052);
        room.replicate(&[move_event]);
        let Delivery::Private(batch) = rx.try_recv().unwrap() else {
            panic!()
        };
        assert_eq!(
            batch.iter().map(|m| m.opcode).collect::<Vec<_>>(),
            [0x03f2, 0x042e]
        );
        assert_eq!(batch[0].body[116], FIRST_MINION_SLOT);
        assert_eq!(batch[0].body[8..12], u32::from(wire_id(id)).to_be_bytes());
        assert_eq!(batch[1].body[..4], u32::from(wire_id(id)).to_be_bytes());
    }

    #[test]
    fn stable_indices_and_explicit_exhaustion() {
        let mut slots = ActorSlots::from_roster(&[]);
        for slot in FIRST_MINION_SLOT..ACTOR_SLOTS {
            assert_eq!(slots.minion(u32::from(slot) + 1000), Some(slot));
        }
        assert_eq!(slots.minion(9999), None);
        assert_eq!(slots.minion(1052), Some(52));
        slots.released(1052);
        assert_eq!(slots.minion(9999), Some(52));
        assert_eq!(slots.minion(1053), Some(53));
    }
}
