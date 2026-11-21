use halcyon_protocol::{EntitySpawnBody, HeroSpawnBody, Message};
use halcyon_sim::{Entity, Kind, World};

use super::{Pick, compact, messages};

const FIRST_MINION_SLOT: u8 = 52;
const ACTOR_SLOTS: u8 = 200;

const RIVER_LEFT_SLOT_KEY: u32 = u32::MAX - 1;
const RIVER_RIGHT_SLOT_KEY: u32 = u32::MAX;

pub(super) struct ActorSlots {
    heroes: [Option<u8>; 10],
    minions: [Option<u32>; (ACTOR_SLOTS - FIRST_MINION_SLOT) as usize],
    awaiting_destroy: [bool; (ACTOR_SLOTS - FIRST_MINION_SLOT) as usize],
    reported_full: bool,
}

fn spawn_with_slot(message: &Message, slot: u8) -> Message {
    let mut body =
        HeroSpawnBody::read_body(&message.body).expect("messages::spawn emits 750B HeroSpawn");
    body.slot = slot;
    body.encode()
}

impl ActorSlots {
    pub fn from_roster(entities: &halcyon_sim::Actors<'_>) -> Self {
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
            awaiting_destroy: [false; (ACTOR_SLOTS - FIRST_MINION_SLOT) as usize],
            reported_full: false,
        }
    }

    pub(super) fn fixed(&self, entity: &Entity) -> Option<u8> {
        match (entity.kind, entity.type_id, entity.team) {
            (Kind::Hero, _, _) if (1..=10).contains(&entity.id) => {
                self.heroes[(entity.id - 1) as usize]
            }
            _ => compact::structure_slot(entity),
        }
    }

    pub fn declaration(
        &mut self,
        world: &World,
        entity: &Entity,
        picks: &[Pick],
        data: Option<&halcyon_data::GameData>,
    ) -> Option<Message> {
        self.declaration_at(world, entity, picks, data, false)
    }

    pub fn declaration_at(
        &mut self,
        world: &World,
        entity: &Entity,
        picks: &[Pick],
        data: Option<&halcyon_data::GameData>,
        fresh: bool,
    ) -> Option<Message> {
        if !(1..=u32::from(u16::MAX) - 1500).contains(&entity.id) {
            return None;
        }
        if matches!(entity.kind, Kind::Minion | Kind::Neutral) {
            if let Some(slot) = self.minion(entity.id) {
                if entity.kind == Kind::Neutral {
                    return compact::camp_unit(entity, slot);
                }
                return Some(compact::minion(entity, slot, fresh).unwrap_or_else(|| {
                    let message = messages::spawn(entity, picks, data);
                    spawn_with_slot(&message, slot)
                }));
            }
            if !std::mem::replace(&mut self.reported_full, true) {
                tracing::warn!("native Solo actor slots exhausted; deferring minion declaration");
            }
            return None;
        }
        let slot = self.fixed(entity)?;
        Some(compact::structure(world.tick, entity).unwrap_or_else(|| {
            let raw = messages::spawn(entity, picks, data);
            let mut body =
                HeroSpawnBody::read_body(&raw.body).expect("messages::spawn emits 750B HeroSpawn");
            if entity.kind == Kind::Tower {
                // Use the stationary tower skin and terrain height. Dynamic aim
                // and timer words from replay snapshots are not authoritative here.
                body.skin = EntitySpawnBody::STRUCTURE_SKIN;
                body.position[1] = f32::from_bits(EntitySpawnBody::GROUND_HEIGHT_BITS);
                body.forward[0] = if entity.team == 0 { 1. } else { -1. };
            }
            if entity.kind == Kind::Armory {
                let profile = halcyon_protocol::SoloStructure::at_slot(slot)
                    .expect("fixed Solo armory slot")
                    .spawn_body(u32::from(messages::wire_id(entity.id)));
                body.skin = profile.skin;
                body.position[1] = profile.position[1];
                body.forward = profile.forward;
            }
            if entity.kind == Kind::Hero {
                body.apply_solo_profile(entity.bot, entity.team, world.tick);
                super::hero_progress::set_spawn(world, &mut body, entity);
            }
            body.slot = slot;
            body.encode()
        }))
    }

    pub(super) fn alloc_dynamic(&mut self, id: u32) -> Option<u8> {
        if !(1..=u32::from(u16::MAX) - 1500).contains(&id) {
            return None;
        }
        let slot = self.minion(id);
        if slot.is_none() && !std::mem::replace(&mut self.reported_full, true) {
            tracing::warn!("native Solo actor slots exhausted; deferring minion declaration");
        }
        slot
    }

    pub(super) fn assigned_dynamic(&self, id: u32) -> Option<u8> {
        self.minions
            .iter()
            .position(|slot| *slot == Some(id))
            .map(|index| FIRST_MINION_SLOT + index as u8)
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

    pub fn river_shop_slot(&mut self, right: bool) -> Option<u8> {
        let key = if right {
            RIVER_RIGHT_SLOT_KEY
        } else {
            RIVER_LEFT_SLOT_KEY
        };
        self.minion(key)
    }

    pub fn defer_destroy(&mut self, id: u32) {
        if let Some(index) = self.minions.iter().position(|slot| *slot == Some(id)) {
            self.awaiting_destroy[index] = true;
        }
    }

    pub fn destroy_sent(&mut self, id: u32) {
        if let Some(index) = self.minions.iter().position(|slot| *slot == Some(id)) {
            self.minions[index] = None;
            self.awaiting_destroy[index] = false;
        }
    }

    pub fn released(&mut self, id: u32) {
        if let Some(index) = self.minions.iter().position(|slot| *slot == Some(id))
            && !self.awaiting_destroy[index]
        {
            self.minions[index] = None;
        }
    }

    /// A new scene gets a fresh client table. Preserve indices of living actors.
    pub fn retain_living(&mut self, entities: &halcyon_sim::Actors<'_>) {
        for (slot, awaiting) in self.minions.iter_mut().zip(&mut self.awaiting_destroy) {
            if slot.is_some_and(|id| !entities.iter().any(|e| e.id == id && e.hp > 0.0)) {
                *slot = None;
                *awaiting = false;
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
        for entity in world.entities.iter().take(10) {
            let hero = slots.declaration(&world, entity, &[], None).unwrap();
            assert_eq!(hero.opcode, crate::wire_opcode::HeroSpawn);
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
                let spawn = slots.declaration(&world, entity, &[], None).unwrap();
                assert_eq!(
                    (spawn.opcode, spawn.body.len()),
                    (crate::wire_opcode::EntitySpawn, 122)
                );
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
        world.entities.find_mut(|e| e.id == *dead).unwrap().hp = 0.0;
        let index = world.entities.iter().position(|e| e.id == *living).unwrap();
        let living = &mut world.entities[index];
        living.hp -= 25.0;
        let living = living.clone();
        slots.retain_living(&world.entities);
        let snapshot = slots.declaration(&world, &living, &[], None).unwrap();
        assert_eq!(snapshot.body[116], *stable);
        assert_eq!(snapshot.body[36..40], living.hp.to_be_bytes());
        assert_eq!(snapshot.body[40..44], living.max_hp.to_be_bytes());
        assert_eq!(slots.minion(4000), Some(*released));
    }

    #[test]
    fn solo_birth_uses_route_defaults_and_rejoin_keeps_slot_with_current_health() {
        let mut world = World::new(7, 10);
        world.use_solo_mid_lane().unwrap();
        let mut slots = ActorSlots::from_roster(&world.entities);
        for _ in 0..19 * halcyon_sim::TICK_HZ {
            world.step();
        }
        for e in world.entities.iter().filter(|e| e.kind == Kind::Minion) {
            let birth = slots.declaration_at(&world, e, &[], None, true).unwrap();
            let b = &birth.body;
            assert!(compact::recordable_minion_birth(b));
            assert_eq!(
                b[12..16],
                ((if e.solo_side == 0 { 70f32 } else { 75f32 })
                    * (if e.team == 0 { -1. } else { 1. }))
                .to_be_bytes()
            );
            assert_eq!(b[16..20], 0x3d75_be29u32.to_be_bytes());
            assert_eq!(b[24..36], [0, 0, 0, 0, 0, 0, 0, 0, 0x3f, 0x80, 0, 0]);
            assert_eq!(b[36..88], [0; 52]);
            assert_eq!(
                [b[117], b[119], b[120], b[121]],
                [1, 1, (3 * e.team as i8 + 1 + e.solo_side) as u8, e.team + 1]
            );
            let mut live = e.clone();
            live.hp -= 17.;
            for fresh in [false, true] {
                let snapshot = slots
                    .declaration_at(&world, &live, &[], None, fresh)
                    .unwrap();
                assert_eq!(snapshot.body[116], b[116]);
                assert_eq!(snapshot.body[117], 0);
                assert_eq!(snapshot.body[36..40], 558f32.to_be_bytes());
                assert_eq!(snapshot.body[40..44], 575f32.to_be_bytes());
                assert!(!compact::recordable_minion_birth(&snapshot.body));
            }
            live.hp = live.max_hp + 100.;
            live.max_hp = live.hp;
            assert_eq!(
                slots
                    .declaration_at(&world, &live, &[], None, true)
                    .unwrap()
                    .body[117],
                0
            );
        }
    }

    #[test]
    fn solo_hero_declarations_keep_identity_separate_from_initial_team_flags() {
        let world = halcyon_sim::World::new(7, 1);
        let mut slots = ActorSlots::from_roster(&world.entities);
        for actor in world.entities.iter().filter(|e| e.kind == Kind::Hero) {
            let first = slots.declaration(&world, actor, &[], None).unwrap();
            let mut later = World::new(0, 1);
            later.tick = 90;
            let late = slots.declaration(&later, actor, &[], None).unwrap();
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
                assert_eq!(m.body[745], first.body[745]);
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
                talent_levels: Vec::new(),
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
                .all(|m| !m.is(crate::Opcode::EntitySpawn) || m.body[116] < FIRST_MINION_SLOT)
        );
        assert!(batch.iter().all(|m| !m.is(crate::Opcode::MoveToCorrection)));
        assert!(!room.visible[0].contains(&id));
        assert!(room.world.entities.iter().any(|e| e.id == id && e.hp > 0.0));

        room.solo_slots.released(1052);
        room.replicate(&[move_event]);
        let Delivery::Private(batch) = rx.try_recv().unwrap() else {
            panic!()
        };
        assert_eq!(
            batch.iter().map(|m| m.opcode).collect::<Vec<_>>(),
            [
                crate::wire_opcode::EntitySpawn,
                crate::wire_opcode::MoveToCorrection
            ]
        );
        assert_eq!(batch[0].body[116], FIRST_MINION_SLOT);
        assert_eq!(batch[0].body[8..12], u32::from(wire_id(id)).to_be_bytes());
        assert_eq!(batch[1].body[..4], u32::from(wire_id(id)).to_be_bytes());
    }

    #[test]
    fn stable_indices_and_explicit_exhaustion() {
        let mut slots = ActorSlots::from_roster(&halcyon_sim::Actors::default());
        for slot in FIRST_MINION_SLOT..ACTOR_SLOTS {
            assert_eq!(slots.minion(u32::from(slot) + 1000), Some(slot));
        }
        assert_eq!(slots.minion(9999), None);
        assert_eq!(slots.minion(1052), Some(52));
        slots.released(1052);
        assert_eq!(slots.minion(9999), Some(52));
        assert_eq!(slots.minion(1053), Some(53));
    }

    #[tokio::test]
    async fn fog_and_disconnected_solo_deaths_recycle_minion_slots_without_exhaustion() {
        use crate::{Peer, Rooms};
        use halcyon_platform::{MatchHost, Reservation};
        use halcyon_sim::{Event, Position};
        use tokio::sync::{mpsc, watch};
        use uuid::Uuid;

        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let match_id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id,
                players: vec![Uuid::new_v4()],
                teams: vec![0],
                mode: "5v5_bots_solo".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(match_id).unwrap();
        let mut room = shared.lock().unwrap();
        room.world.use_solo_mid_lane().unwrap();
        room.seats[0].control_world = true;
        room.seats[0].world_actor_written = true;
        room.seats[0].preamble.observe(
            &Message::known(crate::Opcode::RequestSceneSnapshot, [0; 6]),
            1500,
        );
        room.seats[0].preamble.observe(
            &Message::known(crate::Opcode::ConfirmSceneOwner, [0, 0, 5, 220, 1, 0]),
            1500,
        );
        room.seats[0].preamble.observe(
            &Message::known(crate::Opcode::SceneReadyConfirm, [0; 6]),
            1500,
        );
        let (tx, mut rx) = mpsc::channel(32);
        room.seats[0].peers[0] = Some(Peer {
            generation: 1,
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
            .find(|e| e.kind == Kind::Minion && e.team == 1)
            .unwrap()
            .id;
        let home_pos = room.world.entities[0].position;
        // Move enemy minion into vision so `replicate` allocates a slot for it, then saturate
        // every remaining free slot in `solo_slots`.
        room.world
            .entities
            .find_mut(|e| e.id == minion)
            .unwrap()
            .position = home_pos;
        room.replicate(&[]);
        let _ = rx.try_recv().unwrap();
        let mut dummy = 50_000_u32;
        while room.solo_slots.minion(dummy).is_some() {
            dummy += 1;
        }
        assert!(dummy > 50_000);
        assert_eq!(room.solo_slots.minion(90_001), None);

        // Move the enemy minion into fog and kill it there (no 0x0430 emitted).
        room.world
            .entities
            .find_mut(|e| e.id == minion)
            .unwrap()
            .position = Position { x: 80.0, y: 80.0 };
        room.replicate(&[]);
        while rx.try_recv().is_ok() {}
        room.world.entities.find_mut(|e| e.id == minion).unwrap().hp = 0.0;
        room.replicate(&[Event::Death {
            id: minion,
            killer: 2,
        }]);
        if let Ok(batch) = rx.try_recv() {
            assert!(
                batch
                    .into_records()
                    .iter()
                    .all(|m| !m.is(crate::Opcode::ActorDeath)),
                "fog death must not emit 0x0430"
            );
        }
        let recycled = room
            .solo_slots
            .minion(90_001)
            .expect("fog death must release minion slot");
        room.solo_slots.released(90_001);

        // Allocate the same remaining slot and verify death releases it even with no gameplay peer
        // (disconnected or between solo_opening and SceneConfirmed).
        assert_eq!(room.solo_slots.minion(90_002), Some(recycled));
        room.seats[0].peers[0] = None;
        room.replicate(&[Event::Death {
            id: 90_002,
            killer: 1,
        }]);
        assert_eq!(room.solo_slots.minion(90_003), Some(recycled));
    }
}
