use super::super::{Room, messages::wire_id, transport};
use halcyon_protocol::Message;
use halcyon_sim::{Event, Kind, World};

const NONE: u32 = u32::MAX;
const FIRST_WAVE: u32 = 0x140b_e4b6;
const FIRST_BLOOD: u32 = 0x4648_2c5d;

#[derive(Default)]
pub(crate) struct Announcements {
    wave_started: bool,
    hero_died: bool,
}

impl Announcements {
    fn events(&mut self, world: &World, events: &[Event]) -> Vec<Message> {
        let mut out = Vec::new();
        for event in events {
            match *event {
                Event::Spawn { id } if !self.wave_started => {
                    if world
                        .entities
                        .iter()
                        .any(|e| e.id == id && e.kind == Kind::Minion && e.type_id == 289)
                    {
                        self.wave_started = true;
                        append(&mut out, FIRST_WAVE, None);
                    }
                }
                Event::Death { id: 1..=6, killer } if !self.hero_died => {
                    self.hero_died = true;
                    if (1..=6).contains(&killer) {
                        append(&mut out, FIRST_BLOOD, Some(u32::from(wire_id(killer))));
                    }
                }
                _ => {}
            }
        }
        out
    }
}

fn append(out: &mut Vec<Message>, key: u32, killer: Option<u32>) {
    for recipient in (1500..=1505).rev().chain([NONE]) {
        let words = [
            recipient,
            killer.unwrap_or(recipient),
            key,
            if killer.is_some() { 125 } else { NONE },
            NONE,
            NONE,
        ];
        let mut body = Vec::with_capacity(24);
        for word in words {
            body.extend_from_slice(&word.to_be_bytes());
        }
        out.push(Message::new(0x435, body));
    }
}

pub(crate) fn recordable(body: &[u8]) -> bool {
    let Ok(words): Result<&[[u8; 4]; 6], _> = body.as_chunks::<4>().0.try_into() else {
        return false;
    };
    if body.len() != 24 {
        return false;
    }
    let [recipient, subject, key, arg, a, b] = words.map(u32::from_be_bytes);
    (recipient == NONE || (1500..=1505).contains(&recipient))
        && [a, b] == [NONE; 2]
        && match key {
            FIRST_WAVE => subject == recipient && arg == NONE,
            FIRST_BLOOD => (1500..=1505).contains(&subject) && arg == 125,
            _ => false,
        }
}

pub(crate) fn publish(room: &mut Room, events: &[Event]) {
    let out = room.aral_announcements.events(&room.world, events);
    if out.is_empty() {
        return;
    }
    room.publish_observers(&out);
    let batch = transport::TeamBatch::new(out);
    for seat in &room.seats {
        if seat.world_actor_written
            && seat.preamble.confirmed()
            && seat.peers[1].is_none()
            && let Some(peer) = &seat.peers[0]
        {
            peer.send(transport::Delivery::Team(batch.clone()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn registered_native_seats_share_the_batch_and_rejoin_skips_past_events() {
        use crate::{Peer, Rooms};
        use halcyon_platform::{MatchHost, Reservation};
        use tokio::sync::{mpsc, watch};
        use uuid::Uuid;

        let host = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        host.reserve(Reservation {
            match_id: id,
            players: (0..6).map(|_| Uuid::new_v4()).collect(),
            teams: vec![],
            talent_levels: vec![],
            mode: "casual_aral".into(),
            difficulty: None,
        })
        .unwrap();
        let shared = host.room(id).unwrap();
        let mut room = shared.lock().unwrap();
        let mut readers = Vec::new();
        for (i, seat) in room.seats.iter_mut().enumerate() {
            let actor = 1500 + i as u32;
            let mut enter = actor.to_be_bytes().to_vec();
            enter.extend([1, 0]);
            for msg in [
                Message::new(0x46e, [0; 6]),
                Message::new(0x471, enter),
                Message::new(0x46d, [0; 6]),
            ] {
                seat.preamble.observe(&msg, actor);
            }
            seat.world_actor_written = i < 2;
            let (tx, rx) = mpsc::channel(4);
            seat.peers[0] = Some(Peer {
                generation: i as u64 + 1,
                tx,
                stalled: watch::channel(false).0,
            });
            readers.push(rx);
        }
        let death = [Event::Death { id: 3, killer: 6 }];
        publish(&mut room, &death);
        let (transport::Delivery::Team(a), transport::Delivery::Team(b)) = (
            readers[0].try_recv().unwrap(),
            readers[1].try_recv().unwrap(),
        ) else {
            panic!("expected shared announcement");
        };
        assert!(std::sync::Arc::ptr_eq(&a, &b));
        assert!(readers[2..].iter_mut().all(|rx| rx.try_recv().is_err()));
        room.seats[2].world_actor_written = true;
        publish(&mut room, &death);
        assert!(readers.iter_mut().all(|rx| rx.try_recv().is_err()));
    }

    #[test]
    fn first_wave_and_hero_kill_follow_actual_events_once_per_room() {
        let mut world = World::new(7, 6);
        world.use_six_human_roster().unwrap();
        world.use_aral_lane().unwrap();
        let mut state = Announcements::default();
        for _ in 1..15 * halcyon_sim::TICK_HZ {
            let events = world.step();
            assert!(state.events(&world, &events).is_empty());
        }
        let spawn = world.step();
        let wave = state.events(&world, &spawn);
        assert_eq!(wave.len(), 7);
        for (row, recipient) in wave.iter().zip((1500u32..=1505).rev().chain([NONE])) {
            assert!(recordable(&row.body));
            assert_eq!(&row.body[..4], &recipient.to_be_bytes());
            assert_eq!(&row.body[..4], &row.body[4..8]);
        }
        assert!(state.events(&world, &spawn).is_empty());
        let death = Event::Death { id: 5, killer: 2 };
        let first = state.events(&world, std::slice::from_ref(&death));
        assert_eq!(first.len(), 7);
        assert!(
            first
                .iter()
                .all(|m| recordable(&m.body) && m.body[4..8] == 1501u32.to_be_bytes())
        );
        assert!(
            state
                .events(&world, std::slice::from_ref(&death))
                .is_empty()
        );
        let mut other = Announcements::default();
        assert!(
            other
                .events(&world, &[Event::Death { id: 1, killer: 502 }, death])
                .is_empty()
        );
    }
}
