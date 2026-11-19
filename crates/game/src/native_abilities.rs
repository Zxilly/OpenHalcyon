use halcyon_sim::{AbilitySlot, CastTarget, Command, Event, Kind, Position};

use super::{Batch, Message, Room, messages::wire_id, transport::Delivery};

const IDRIS: u16 = 273;
const SELECTOR: u8 = 3;
const LORELAI: u16 = 409;

#[derive(Default)]
pub(super) struct NativeAbilities {
    learned_at: Option<u64>,
    cast: Option<PendingCast>,
}

struct PendingCast {
    slot: AbilitySlot,
    target: CastTarget,
    reply: Message,
    generation: u64,
}

pub(super) fn receive(room: &mut Room, seat: usize, channel: usize, m: Message) -> Batch {
    let actor = &room.world.entities[seat];
    let connection = &room.seats[seat];
    let Some(peer) = connection.peers[channel].as_ref() else {
        return Vec::new();
    };
    if channel != 0
        || !(room.solo_scene() || room.legacy_aral())
        || !connection.world_actor_written
        || actor.kind != Kind::Hero
        || actor.bot
        || !matches!(actor.type_id, IDRIS | LORELAI)
    {
        return Vec::new();
    }
    let generation = peer.generation;
    let id = actor.id;
    let hero = actor.type_id;
    let state = &mut room.seats[seat].native_abilities;
    match m.opcode {
        0x0436 if m.body.len() == 6 && m.body[1..] == [0; 5] => {
            let (slot, selector) = match (hero, m.body[0]) {
                (IDRIS, 1) => (AbilitySlot::B, SELECTOR),
                (LORELAI, 0) => (AbilitySlot::A, 0),
                (LORELAI, 1) => (AbilitySlot::B, 1),
                (LORELAI, 2) => (AbilitySlot::C, 2),
                _ => return Vec::new(),
            };
            if state.learned_at == Some(room.world.tick)
                || room.world.command(id, Command::Learn(slot)).is_err()
            {
                return Vec::new();
            }
            state.learned_at = Some(room.world.tick);
            let grant = Message::ability_learned(u32::from(wire_id(id)), u32::from(selector));
            witness(room, id, None, &grant);
            vec![m, grant]
        }
        0x0412 if m.body.len() == 14 && m.body[13] == 0 => {
            let selector = m.body[12];
            let slot = match (hero, selector) {
                (IDRIS, SELECTOR) | (LORELAI, 1) => AbilitySlot::B,
                (LORELAI, 0) => AbilitySlot::A,
                _ => return Vec::new(),
            };
            let coordinates: [f32; 3] = std::array::from_fn(|i| {
                f32::from_be_bytes(m.body[i * 4..i * 4 + 4].try_into().unwrap())
            });
            let [x, height, y] = coordinates;
            if height != 0.0 || !x.is_finite() || !y.is_finite() {
                return Vec::new();
            }
            let target = CastTarget::Point(Position { x, y });
            if room
                .world
                .command(id, Command::Cast { slot, target })
                .is_ok()
            {
                state.cast = Some(PendingCast {
                    generation,
                    slot,
                    target,
                    reply: Message::ability_point_used(
                        u32::from(wire_id(id)),
                        coordinates,
                        selector,
                    ),
                });
            }
            Vec::new()
        }
        0x0411 if hero == LORELAI && m.body.len() == 6 && m.body[4..] == [2, 0] => {
            let wire_target = u32::from_be_bytes(m.body[..4].try_into().unwrap());
            let Some(target_id) = wire_target.checked_sub(1499) else {
                return Vec::new();
            };
            if !room.world.entities.iter().any(|e| {
                e.id == target_id
                    && e.id != id
                    && e.team == actor.team
                    && e.kind == Kind::Hero
                    && e.hp > 0.0
            }) {
                return Vec::new();
            }
            let slot = AbilitySlot::C;
            let target = CastTarget::Entity(target_id);
            if room
                .world
                .command(id, Command::Cast { slot, target })
                .is_ok()
            {
                state.cast = Some(PendingCast {
                    generation,
                    slot,
                    target,
                    reply: Message::ability_target_used(u32::from(wire_id(id)), wire_target, 2),
                });
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn selector(hero: u16, slot: AbilitySlot) -> Option<u8> {
    match (hero, slot) {
        (IDRIS, AbilitySlot::B) => Some(SELECTOR),
        (LORELAI, AbilitySlot::A) => Some(0),
        (LORELAI, AbilitySlot::B) => Some(1),
        (LORELAI, AbilitySlot::C) => Some(2),
        _ => None,
    }
}

// The origin has a generation-bound confirmation on its issuing control socket.
// Every other seat receives one release on its current world stream.
fn witness(room: &mut Room, source: u32, target: Option<u32>, message: &Message) {
    let mut shared = None;
    for (seat, actor) in room.seats.iter().zip(&room.world.entities) {
        let visible = &room.visible[actor.team as usize];
        if actor.id != source
            && (seat.world_actor_written || seat.peers[1].is_some())
            && visible.contains(&source)
            && target.is_none_or(|id| visible.contains(&id))
            && let Some(peer) = seat.gameplay_peer()
        {
            let batch = shared
                .get_or_insert_with(|| super::transport::TeamBatch::new(vec![message.clone()]));
            peer.send(Delivery::Team(batch.clone()));
        }
    }
    room.publish_observers(std::slice::from_ref(message));
}

pub(super) fn publish(room: &mut Room, events: &[Event]) {
    if !(room.solo_scene() || room.legacy_aral()) {
        return;
    }
    for event in events {
        let (source, slot) = match *event {
            Event::Cast { source, slot, .. } | Event::Learned { source, slot } => (source, slot),
            _ => continue,
        };
        let Some(index) = source.checked_sub(1).map(|v| v as usize) else {
            continue;
        };
        let Some(actor) = room
            .world
            .entities
            .get(index)
            .filter(|e| e.id == source && e.kind == Kind::Hero)
        else {
            continue;
        };
        let Some(selector) = selector(actor.type_id, slot) else {
            continue;
        };
        let actor_wire = u32::from(wire_id(source));
        let Event::Cast { target, .. } = *event else {
            witness(
                room,
                source,
                None,
                &Message::ability_learned(actor_wire, selector.into()),
            );
            continue;
        };
        let (mut message, target_id) = match (actor.type_id, slot, target) {
            (IDRIS, AbilitySlot::B, CastTarget::Point(p))
            | (LORELAI, AbilitySlot::A | AbilitySlot::B, CastTarget::Point(p)) => (
                Message::ability_point_used(actor_wire, [p.x, 0., p.y], selector),
                None,
            ),
            (LORELAI, AbilitySlot::C, CastTarget::Entity(id)) => (
                Message::ability_target_used(actor_wire, u32::from(wire_id(id)), selector),
                Some(id),
            ),
            _ => continue,
        };
        if let Some(seat) = room.seats.get_mut(index)
            && let Some(pending) = seat.native_abilities.cast.take()
            && pending.slot == slot
            && pending.target == target
            && seat.world_actor_written
            && let Some(peer) = seat.peers[0]
                .as_ref()
                .filter(|p| p.generation == pending.generation)
        {
            message = pending.reply;
            peer.send(Delivery::Private(vec![message.clone()]));
        }
        witness(room, source, target_id, &message);
    }
}

#[cfg(test)]
mod tests {
    use crate::{Peer, Rooms};
    use halcyon_platform::{MatchHost, Reservation};
    use tokio::sync::{mpsc, watch};
    use uuid::Uuid;

    use super::*;

    #[tokio::test]
    async fn learns_once_and_only_confirms_the_registered_caster_after_release() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let match_id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                match_id,
                players: (0..6).map(|_| Uuid::new_v4()).collect(),
                teams: vec![],
                mode: "casual_aral".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(match_id).unwrap();
        let mut room = shared.lock().unwrap();
        let mut recipients = Vec::new();
        for i in 0..2 {
            let (tx, rx) = mpsc::channel(64);
            room.seats[i].peers[0] = Some(Peer {
                generation: 1 + i as u64,
                tx,
                stalled: watch::channel(false).0,
            });
            for (opcode, body) in [
                (0x046e, [0; 6]),
                (0x0471, [0, 0, 5, 220 + i as u8, 1, 0]),
                (0x046d, [0; 6]),
            ] {
                room.seats[i]
                    .preamble
                    .observe(&Message::new(opcode, body), 1500 + i as u32);
            }
            recipients.push(rx);
        }
        room.phase("running");
        room.world.entities[0].type_id = IDRIS;
        room.world.entities[0].skill_points = 2;
        room.world.entities[0].position = Position { x: 8.0, y: 0.0 };

        let learn = Message::new(0x0436, [1, 0, 0, 0, 0, 0]);
        let mut body = [0; 14];
        body[..4].copy_from_slice(&12f32.to_be_bytes());
        body[4..8].copy_from_slice(&(-0.0_f32).to_be_bytes());
        body[12] = SELECTOR;
        let cast = Message::new(0x0412, body);

        assert!(room.receive(0, 0, learn.clone()).unwrap().is_empty());
        room.seats[0].world_actor_written = true;
        room.seats[1].world_actor_written = true;
        assert!(room.receive(1, 0, learn.clone()).unwrap().is_empty());
        assert!(room.receive(0, 0, cast.clone()).unwrap().is_empty());
        room.tick();
        assert!(drain(&mut recipients[0]).is_empty());

        let learned = room.receive(0, 0, learn.clone()).unwrap();
        assert_eq!(learned, [learn.clone(), Message::ability_learned(1500, 3)]);
        assert!(room.receive(0, 0, learn).unwrap().is_empty());
        assert_eq!(room.world.entities[0].skill_points, 1);
        assert_eq!(room.world.entities[0].ability_ranks, [0, 1, 0]);
        let mut invalid = cast.clone();
        invalid.body[..4].copy_from_slice(&1000f32.to_be_bytes());
        assert!(room.receive(0, 0, invalid).unwrap().is_empty());
        room.tick();
        assert!(drain(&mut recipients[0]).is_empty());

        assert!(room.receive(0, 0, cast.clone()).unwrap().is_empty());
        room.world.command(1, Command::Stop).unwrap();
        for _ in 0..12 {
            room.tick();
        }
        assert!(drain(&mut recipients[0]).is_empty());
        assert!(drain(&mut recipients[1]).is_empty());
        assert!(room.receive(0, 0, cast.clone()).unwrap().is_empty());
        assert!(room.receive(0, 0, cast.clone()).unwrap().is_empty());
        assert!(drain(&mut recipients[0]).is_empty());
        for _ in 0..12 {
            room.tick();
        }
        assert_eq!(
            drain(&mut recipients[0]),
            [Message::ability_point_used(1500, [12., -0.0, 0.], 3)]
        );
        assert_eq!(
            drain(&mut recipients[1]),
            [Message::ability_point_used(1500, [12., -0.0, 0.], 3)]
        );
        assert!(room.receive(0, 0, cast).unwrap().is_empty());
        for _ in 0..12 {
            room.tick();
        }
        assert!(drain(&mut recipients[0]).is_empty());
    }

    #[tokio::test]
    async fn lorelai_learns_three_selectors_and_confirms_only_released_ally_or_point_casts() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let match_id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                match_id,
                players: (0..6).map(|_| Uuid::new_v4()).collect(),
                teams: vec![],
                mode: "casual_aral".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(match_id).unwrap();
        let mut room = shared.lock().unwrap();
        let (tx, mut rx) = mpsc::channel(128);
        room.seats[0].peers[0] = Some(Peer {
            generation: 1,
            tx,
            stalled: watch::channel(false).0,
        });
        room.seats[0].world_actor_written = true;
        room.world.entities[0].type_id = LORELAI;
        room.world.entities[0].level = 10;
        room.world.entities[0].max_energy = 1000.;
        room.world.entities[0].energy = 1000.;
        room.world.entities[0].skill_points = 3;
        let ally = room
            .world
            .entities
            .iter()
            .skip(1)
            .find(|e| e.kind == Kind::Hero && e.team == room.world.entities[0].team)
            .unwrap()
            .id;
        let position = Position { x: 1., y: 0. };
        room.world.entities[0].position = position;
        room.world.entities[(ally - 1) as usize].position = position;
        for selector in 0..3 {
            let request = Message::new(0x0436, [selector, 0, 0, 0, 0, 0]);
            assert_eq!(
                receive(&mut room, 0, 0, request.clone()),
                [request, Message::ability_learned(1500, selector.into())]
            );
            room.world.tick += 1;
            let packet = if selector < 2 {
                let mut body = [0; 14];
                body[..4].copy_from_slice(&position.x.to_be_bytes());
                body[12] = selector;
                Message::new(0x0412, body)
            } else {
                let mut body = [0; 6];
                body[..4].copy_from_slice(&u32::from(wire_id(ally)).to_be_bytes());
                body[4] = 2;
                Message::new(0x0411, body)
            };
            receive(&mut room, 0, 0, packet);
            assert!(
                room.seats[0].native_abilities.cast.is_some(),
                "selector {selector}"
            );
            assert!(rx.try_recv().is_err());
            let expected = if selector < 2 {
                Message::ability_point_used(1500, [position.x, 0., position.y], selector)
            } else {
                Message::ability_target_used(1500, u32::from(wire_id(ally)), 2)
            };
            for _ in 0..60 {
                let events = room.world.step();
                publish(&mut room, &events);
                if room.seats[0].native_abilities.cast.is_none() {
                    break;
                }
            }
            assert!(matches!(rx.try_recv(), Ok(Delivery::Private(batch)) if batch == [expected]));
        }
    }

    #[tokio::test]
    async fn witnessed_releases_require_visible_source_and_target_and_skip_replaced_origin() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                match_id: id,
                players: (0..6).map(|_| Uuid::new_v4()).collect(),
                mode: "casual_aral".into(),
                teams: vec![0, 0, 1, 0, 1, 1],
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut room = shared.lock().unwrap();
        let mut streams = vec![];
        for seat in &mut room.seats {
            let (tx, rx) = mpsc::channel(8);
            seat.peers[1] = Some(Peer {
                generation: 2,
                tx: tx.clone(),
                stalled: watch::channel(false).0,
            });
            seat.peers[0] = Some(Peer {
                generation: 2,
                tx,
                stalled: watch::channel(false).0,
            });
            seat.world_actor_written = true;
            streams.push(rx);
        }
        room.world.entities[0].type_id = LORELAI;
        let target = CastTarget::Entity(2);
        room.seats[0].native_abilities.cast = Some(PendingCast {
            slot: AbilitySlot::C,
            target,
            reply: Message::ability_target_used(1500, 1501, 2),
            generation: 1,
        });
        room.visible[0].extend([1, 2]);
        room.visible[1].insert(1);
        let cast = Event::Cast {
            source: 1,
            slot: AbilitySlot::C,
            target,
        };
        publish(&mut room, std::slice::from_ref(&cast));
        assert!(streams[0].try_recv().is_err());
        assert_eq!(
            streams[1].try_recv().unwrap().into_records(),
            [Message::ability_target_used(1500, 1501, 2)]
        );
        assert!(streams[2].try_recv().is_err());
        room.visible[1].clear();
        room.visible[1].insert(2);
        publish(&mut room, std::slice::from_ref(&cast));
        assert!(streams[2].try_recv().is_err());
        room.visible[1].insert(1);
        publish(&mut room, &[cast]);
        assert_eq!(
            streams[2].try_recv().unwrap().into_records(),
            [Message::ability_target_used(1500, 1501, 2)]
        );
    }

    #[tokio::test]
    async fn solo_hero_bot_successful_learn_reaches_registered_world_peer() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                match_id: id,
                players: vec![Uuid::new_v4()],
                teams: vec![0],
                mode: "solo".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut room = shared.lock().unwrap();
        let (tx, mut bot_view) = mpsc::channel(16);
        room.seats[0].peers[1] = Some(Peer {
            generation: 1,
            tx,
            stalled: watch::channel(false).0,
        });
        let bot = &mut room.world.entities[3];
        assert!(bot.bot);
        bot.type_id = IDRIS;
        bot.skill_points = 1;
        bot.ability_ranks = [0; 3];
        let bot_id = bot.id;
        room.visible[0].insert(bot_id);
        for _ in 0..5 {
            let events = room.world.step();
            publish(&mut room, &events);
        }
        assert_eq!(
            bot_view.try_recv().unwrap().into_records(),
            [Message::ability_learned(u32::from(wire_id(bot_id)), 3)]
        );
    }

    fn drain(receiver: &mut mpsc::Receiver<Delivery>) -> Vec<Message> {
        let mut replies = vec![];
        while let Ok(batch) = receiver.try_recv() {
            replies.extend(
                batch
                    .into_records()
                    .into_iter()
                    .filter(|m| m.opcode == 0x0416),
            );
        }
        replies
    }
}
