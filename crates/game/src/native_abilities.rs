use halcyon_sim::{
    AbilitySlot, CastTarget, Command, Event, Kind, Position, native_learn_operand,
    native_learn_request,
};

use super::{Batch, Message, Room, messages::wire_id, transport::Delivery};

mod chakram_motion;
pub(super) use chakram_motion::recordable_stop;

const IDRIS: u16 = 273;
const SELECTOR: u8 = 3;
const LORELAI: u16 = 409;

#[derive(Default)]
pub(super) struct NativeAbilities {
    learned_at: Option<u64>,
    walked_at: Option<u64>,
    cast: Option<PendingCast>,
}

struct PendingCast {
    slot: AbilitySlot,
    target: CastTarget,
    reply: Message,
    generation: u64,
    interrupted_walk: Option<Position>,
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
    {
        return Vec::new();
    }
    let generation = peer.generation;
    let id = actor.id;
    let hero = actor.type_id;
    let position = actor.position;
    let aral = room.legacy_aral();
    let state = &mut room.seats[seat].native_abilities;
    match m.opcode {
        0x0436 if m.body.len() == 6 && m.body[1..] == [0; 5] => {
            let Some((slot, operand)) = native_learn_request(hero, m.body[0]) else {
                return Vec::new();
            };
            if state.learned_at == Some(room.world.tick)
                || room.world.command(id, Command::Learn(slot)).is_err()
            {
                return Vec::new();
            }
            state.learned_at = Some(room.world.tick);
            let grant = Message::ability_learned(u32::from(wire_id(id)), u32::from(operand));
            witness(room, id, None, &grant, None);
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
                    interrupted_walk: (aral
                        && hero == IDRIS
                        && state.walked_at == Some(room.world.tick))
                    .then_some(position),
                    reply: Message::ability_point_used(
                        u32::from(wire_id(id)),
                        coordinates,
                        selector,
                    ),
                });
                return vec![m];
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
                    interrupted_walk: None,
                });
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn cast_selector(hero: u16, slot: AbilitySlot) -> Option<u8> {
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
fn witness(
    room: &mut Room,
    source: u32,
    target: Option<u32>,
    message: &Message,
    chakram_stop: Option<(u8, [Message; 2])>,
) {
    let mut shared = None;
    let mut walking = None;
    for (index, (seat, actor)) in room.seats.iter().zip(&room.world.entities).enumerate() {
        let visible = &room.visible[actor.team as usize];
        if actor.id != source
            && (seat.world_actor_written || seat.peers[1].is_some())
            && visible.contains(&source)
            && target.is_none_or(|id| visible.contains(&id))
            && let Some(peer) = seat.gameplay_peer()
        {
            let batch = if let Some((team, [stop, correction])) = &chakram_stop
                && *team == actor.team
                && room.scene_views[index].knows(source)
            {
                walking.get_or_insert_with(|| {
                    super::transport::TeamBatch::new(vec![
                        stop.clone(),
                        message.clone(),
                        correction.clone(),
                    ])
                })
            } else {
                shared
                    .get_or_insert_with(|| super::transport::TeamBatch::new(vec![message.clone()]))
            };
            peer.send(Delivery::Team(batch.clone()));
        }
    }
    room.publish_observers(std::slice::from_ref(message));
}

pub(super) fn publish(room: &mut Room, events: &[Event]) {
    if !(room.solo_scene() || room.legacy_aral()) {
        return;
    }
    let aral = room.legacy_aral();
    for (event_index, event) in events.iter().enumerate() {
        if aral
            && let Event::Move { id, .. } = *event
            && let Some(index) = id.checked_sub(1).map(|v| v as usize)
            && let Some(seat) = room.seats.get_mut(index)
            && room.world.entities[index].type_id == IDRIS
        {
            seat.native_abilities.walked_at = Some(room.world.tick);
        }
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
        let hero = actor.type_id;
        let actor_wire = u32::from(wire_id(source));
        let Event::Cast { target, .. } = *event else {
            if let Some(operand) = native_learn_operand(hero, slot) {
                witness(
                    room,
                    source,
                    None,
                    &Message::ability_learned(actor_wire, operand.into()),
                    None,
                );
            }
            continue;
        };
        let Some(selector) = cast_selector(hero, slot) else {
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
        let chakram = if aral
            && hero == IDRIS
            && slot == AbilitySlot::B
            && let Some(Event::ChakramApplied { source: caster, id }) = events.get(event_index + 1)
            && *caster == source
            && let Some(instance) = id.checked_add(1499)
        {
            Some(
                halcyon_protocol::BuffApplication {
                    prefix: halcyon_protocol::BuffPrefix {
                        target: actor_wire,
                        source: actor_wire,
                        duration_bits: 0x4066,
                        instance,
                        definition: 820,
                    },
                    extras: None,
                    suffix: &[0; 6],
                }
                .to_message()
                .expect("fixed Chakram application"),
            )
        } else {
            None
        };
        let team = actor.team;
        let position = actor.position;
        let mut stop = None;
        if let Some(seat) = room.seats.get_mut(index)
            && let Some(pending) = seat.native_abilities.cast.take()
            && pending.slot == slot
            && pending.target == target
            && seat.world_actor_written
            && let Some(peer) = seat.peers[0]
                .as_ref()
                .filter(|p| p.generation == pending.generation)
        {
            if chakram.is_some() && pending.interrupted_walk == Some(position) {
                stop = chakram_motion::stop(actor_wire, position).map(|messages| (team, messages));
            }
            message = pending.reply;
            let mut batch = vec![message.clone()];
            if aral
                && hero == LORELAI
                && slot == AbilitySlot::C
                && let Some(target) = target_id
                && let Some(instance) = events.iter().find_map(|event| match *event {
                    Event::WaterwallApplied {
                        source: caster,
                        target: ally,
                        id,
                    } if caster == source && ally == target => id.checked_add(1499),
                    _ => None,
                })
            {
                let apply = halcyon_protocol::BuffApplication {
                    prefix: halcyon_protocol::BuffPrefix {
                        target: u32::from(wire_id(target)),
                        source: actor_wire,
                        duration_bits: 0x4400,
                        instance,
                        definition: 954,
                    },
                    extras: None,
                    suffix: &[0; 6],
                };
                batch.push(apply.to_message().expect("fixed Waterwall format"));
            }
            batch.extend(chakram.iter().cloned());
            peer.send(Delivery::Private(batch));
        }
        witness(room, source, target_id, &message, stop);
        if let Some(effect) = chakram {
            let mut shared = None;
            for (recipient, seat) in room.seats.iter().enumerate() {
                if recipient == index
                    || room.world.entities[recipient].team != team
                    || !room.visible[usize::from(team)].contains(&source)
                    || !room.scene_views[recipient].knows(source)
                    || !(seat.world_actor_written || seat.peers[1].is_some())
                {
                    continue;
                }
                if let Some(peer) = seat.gameplay_peer() {
                    peer.send(Delivery::Team(
                        shared
                            .get_or_insert_with(|| {
                                super::transport::TeamBatch::new(vec![effect.clone()])
                            })
                            .clone(),
                    ));
                }
            }
        }
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
                talent_levels: Vec::new(),
                match_id,
                players: (0..6).map(|_| Uuid::new_v4()).collect(),
                teams: vec![0, 1, 0, 1, 0, 1],
                mode: "casual_aral".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(match_id).unwrap();
        let mut room = shared.lock().unwrap();
        let mut recipients = Vec::new();
        for (i, seat) in room.seats.iter_mut().enumerate().take(3) {
            let (tx, rx) = mpsc::channel(64);
            seat.peers[0] = Some(Peer {
                generation: 1 + i as u64,
                tx,
                stalled: watch::channel(false).0,
            });
            for (opcode, body) in [
                (0x046e, [0; 6]),
                (0x0471, [0, 0, 5, 220 + i as u8, 1, 0]),
                (0x046d, [0; 6]),
            ] {
                seat.preamble
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
        room.seats[2].world_actor_written = true;
        let mut declared = super::super::messages::spawn(&room.world.entities[0], &[], None);
        declared.body[745] = 16;
        for view in &mut room.scene_views[..3] {
            view.declarations(std::slice::from_ref(&declared));
        }
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

        room.world
            .command(1, Command::MoveTo(Position { x: 9., y: 0. }))
            .unwrap();
        room.tick();
        assert_eq!(
            room.receive(0, 0, cast.clone()).unwrap().as_slice(),
            std::slice::from_ref(&cast)
        );
        assert!(
            room.seats[0]
                .native_abilities
                .cast
                .as_ref()
                .unwrap()
                .interrupted_walk
                .is_some()
        );
        room.world.command(1, Command::Stop).unwrap();
        for _ in 0..12 {
            room.tick();
        }
        assert!(drain(&mut recipients[0]).is_empty());
        assert!(drain(&mut recipients[1]).is_empty());
        assert!(drain(&mut recipients[2]).is_empty());
        room.world
            .command(1, Command::MoveTo(Position { x: 9., y: 0. }))
            .unwrap();
        room.tick();
        let stopped = room.world.entities[0].position;
        assert!(stopped.x > 8.);
        assert_eq!(
            room.receive(0, 0, cast.clone()).unwrap().as_slice(),
            std::slice::from_ref(&cast)
        );
        assert!(room.receive(0, 0, cast.clone()).unwrap().is_empty());
        assert!(drain(&mut recipients[0]).is_empty());
        for _ in 0..12 {
            room.tick();
        }
        let issuer = drain(&mut recipients[0]);
        assert_eq!(issuer.len(), 2);
        assert_eq!(
            issuer[0],
            Message::ability_point_used(1500, [12., -0.0, 0.], 3)
        );
        let Ok(halcyon_protocol::Fields::BuffApplication(buff)) = issuer[1].fields() else {
            panic!("missing released Chakram application")
        };
        assert_eq!(
            (
                buff.prefix.source,
                buff.prefix.target,
                buff.prefix.duration_bits,
                buff.prefix.definition
            ),
            (1500, 1500, 0x4066, 820)
        );
        assert!(buff.prefix.instance > 1505);
        assert_eq!(buff.suffix, [0; 6]);
        assert_eq!(drain(&mut recipients[1]), issuer[..1]);
        let teammate = drain(&mut recipients[2]);
        let [stop, correction] = chakram_motion::stop(1500, stopped).unwrap();
        assert!(recordable_stop(&stop));
        assert_eq!(
            teammate,
            [stop, issuer[0].clone(), correction, issuer[1].clone()]
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
                talent_levels: Vec::new(),
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
        for (op, body) in [
            (0x046e, [0; 6]),
            (0x0471, [0, 0, 5, 220, 1, 0]),
            (0x046d, [0; 6]),
        ] {
            room.seats[0]
                .preamble
                .observe(&Message::new(op, body), 1500);
        }
        room.phase("running");
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
                room.receive(0, 0, request.clone()).unwrap(),
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
            if selector == 2 {
                // The native report witnessed a base veil away from the earlier pools.
                let dry = Position { x: 9., y: 1. };
                room.world.entities[0].position = dry;
                room.world.entities[(ally - 1) as usize].position = dry;
            }
            let response = room.receive(0, 0, packet.clone()).unwrap();
            if selector < 2 {
                assert_eq!(response, [packet]);
            } else {
                assert!(response.is_empty());
            }
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
            let batch = rx.try_recv().unwrap().into_records();
            assert_eq!(batch[0], expected);
            if selector == 2 {
                assert_eq!(batch.len(), 2);
                let Ok(halcyon_protocol::Fields::BuffApplication(effect)) = batch[1].fields()
                else {
                    panic!("expected the targeted base Waterwall after its release");
                };
                assert_eq!(effect.prefix.source, 1500);
                assert_eq!(effect.prefix.target, u32::from(wire_id(ally)));
                assert_eq!(effect.prefix.definition, 954);
                assert_eq!(effect.prefix.duration(), 4.);
                assert!(effect.prefix.instance > 1505);
                assert_eq!(effect.suffix, [0; 6]);
            } else {
                assert_eq!(batch.len(), 1);
            }
        }
    }

    #[tokio::test]
    async fn witnessed_releases_require_visible_source_and_target_and_skip_replaced_origin() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
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
            interrupted_walk: None,
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
        publish(
            &mut room,
            &[
                cast.clone(),
                Event::WaterwallApplied {
                    source: 1,
                    target: 2,
                    id: 90,
                },
            ],
        );
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
                talent_levels: Vec::new(),
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

    #[tokio::test]
    async fn progression_only_grants_use_rank_gates_and_allow_a_later_retry() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let match_id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id,
                players: (0..6).map(|_| Uuid::new_v4()).collect(),
                teams: vec![],
                mode: "casual_aral".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(match_id).unwrap();
        let mut room = shared.lock().unwrap();
        let mut witnesses = Vec::new();
        for (index, seat) in room.seats.iter_mut().enumerate().take(2) {
            let (tx, rx) = mpsc::channel(16);
            seat.peers[0] = Some(Peer {
                generation: (index + 1) as u64,
                tx,
                stalled: watch::channel(false).0,
            });
            let actor = 1500 + index as u32;
            for (op, body) in [
                (0x046e, [0; 6]),
                (0x0471, [0, 0, 5, 220 + index as u8, 1, 0]),
                (0x046d, [0; 6]),
            ] {
                seat.preamble.observe(&Message::new(op, body), actor);
            }
            seat.world_actor_written = true;
            witnesses.push(rx);
        }
        room.phase("running");
        room.world.entities[0].type_id = IDRIS;
        room.world.entities[0].level = 5;
        room.world.entities[0].skill_points = 2;

        let learn = |index| Message::new(0x0436, [index, 0, 0, 0, 0, 0]);
        assert!(room.receive(0, 0, learn(2)).unwrap().is_empty());
        assert_eq!(room.world.entities[0].skill_points, 2);
        room.world.tick += 1;
        room.world.entities[0].level = 6;
        room.visible[1].insert(1);
        assert_eq!(
            room.receive(0, 0, learn(2)).unwrap(),
            [learn(2), Message::ability_learned(1500, 4)]
        );
        assert!(room.receive(0, 0, learn(2)).unwrap().is_empty());
        assert!(room.receive(0, 0, learn(0)).unwrap().is_empty());
        assert_eq!(room.world.entities[0].skill_points, 1);
        assert!(witnesses[0].try_recv().is_err());
        assert_eq!(
            witnesses[1].try_recv().unwrap().into_records(),
            [Message::ability_learned(1500, 4)]
        );
        assert!(witnesses[1].try_recv().is_err());

        let mut unsupported_cast = [0; 14];
        unsupported_cast[12] = 4;
        assert!(
            room.receive(0, 0, Message::new(0x0412, unsupported_cast))
                .unwrap()
                .is_empty()
        );
        room.world.tick += 1;
        room.visible[1].clear();
        assert_eq!(
            room.receive(0, 0, learn(0)).unwrap(),
            [learn(0), Message::ability_learned(1500, 0)]
        );
        assert!(witnesses[1].try_recv().is_err());
        assert_eq!(room.world.entities[0].skill_points, 0);
        assert_eq!(room.world.entities[0].ability_ranks, [1, 0, 1]);
    }

    fn drain(receiver: &mut mpsc::Receiver<Delivery>) -> Vec<Message> {
        let mut replies = vec![];
        while let Ok(batch) = receiver.try_recv() {
            let records = batch.into_records();
            let stop_batch = records.iter().any(|m| m.opcode == 0x03fb);
            replies.extend(records.into_iter().filter(|m| {
                matches!(m.opcode, 0x03fb | 0x0416 | 0x043e) || (stop_batch && m.opcode == 0x042e)
            }));
        }
        replies
    }
}
