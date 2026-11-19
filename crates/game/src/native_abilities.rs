use halcyon_sim::{AbilitySlot, CastTarget, Command, Event, Kind, Position};

use super::{Batch, Message, Room, messages::wire_id, transport::Delivery};

const IDRIS: u16 = 273;
const SELECTOR: u8 = 3;

#[derive(Default)]
pub(super) struct NativeAbilities {
    learned_at: Option<u64>,
    cast: Option<PointCast>,
}

struct PointCast {
    generation: u64,
    coordinates: [f32; 3],
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
        || actor.type_id != IDRIS
    {
        return Vec::new();
    }
    let generation = peer.generation;
    let id = actor.id;
    let state = &mut room.seats[seat].native_abilities;
    match m.opcode {
        0x0436 if m.body == [1, 0, 0, 0, 0, 0] => {
            if state.learned_at == Some(room.world.tick)
                || room
                    .world
                    .command(id, Command::Learn(AbilitySlot::B))
                    .is_err()
            {
                return Vec::new();
            }
            state.learned_at = Some(room.world.tick);
            vec![
                m,
                Message::ability_learned(u32::from(wire_id(id)), u32::from(SELECTOR)),
            ]
        }
        0x0412 if m.body.len() == 14 && m.body[12..] == [SELECTOR, 0] => {
            let coordinates: [f32; 3] = std::array::from_fn(|i| {
                f32::from_be_bytes(m.body[i * 4..i * 4 + 4].try_into().unwrap())
            });
            let [x, height, y] = coordinates;
            if height != 0.0 || !x.is_finite() || !y.is_finite() {
                return Vec::new();
            }
            if room
                .world
                .command(
                    id,
                    Command::Cast {
                        slot: AbilitySlot::B,
                        target: CastTarget::Point(Position { x, y }),
                    },
                )
                .is_ok()
            {
                state.cast = Some(PointCast {
                    generation,
                    coordinates,
                });
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

pub(super) fn publish(room: &mut Room, events: &[Event]) {
    for event in events {
        let Event::Cast {
            source,
            slot: AbilitySlot::B,
            target: CastTarget::Point(position),
        } = event
        else {
            continue;
        };
        let Some(seat) = source
            .checked_sub(1)
            .and_then(|i| room.seats.get_mut(i as usize))
        else {
            continue;
        };
        let Some(pending) = seat.native_abilities.cast.take() else {
            continue;
        };
        let [x, _, y] = pending.coordinates;
        if position.x.to_bits() == x.to_bits()
            && position.y.to_bits() == y.to_bits()
            && seat.world_actor_written
            && let Some(peer) = seat.peers[0]
                .as_ref()
                .filter(|p| p.generation == pending.generation)
        {
            peer.send(Delivery::Private(vec![Message::ability_point_used(
                u32::from(wire_id(*source)),
                pending.coordinates,
                SELECTOR,
            )]));
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
        assert!(room.receive(0, 0, cast.clone()).unwrap().is_empty());
        assert!(drain(&mut recipients[0]).is_empty());
        for _ in 0..12 {
            room.tick();
        }
        assert_eq!(
            drain(&mut recipients[0]),
            [Message::ability_point_used(1500, [12., -0.0, 0.], 3)]
        );
        assert!(drain(&mut recipients[1]).is_empty());
        assert!(room.receive(0, 0, cast).unwrap().is_empty());
        for _ in 0..12 {
            room.tick();
        }
        assert!(drain(&mut recipients[0]).is_empty());
    }

    fn drain(receiver: &mut mpsc::Receiver<Delivery>) -> Vec<Message> {
        let mut replies = vec![];
        while let Ok(batch) = receiver.try_recv() {
            if let Delivery::Private(messages) = batch {
                replies.extend(messages.into_iter().filter(|m| m.opcode == 0x0416));
            }
        }
        replies
    }
}
