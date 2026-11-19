use halcyon_protocol::LiveFields;
use halcyon_sim::{Command, Position};

use super::wire_id;

/// Translate only gameplay requests whose input and authoritative behavior are known.
pub(super) fn from_live(request: LiveFields<'_>) -> Option<Command> {
    match request {
        LiveFields::MovePoint { x, y } if x.is_finite() && y.is_finite() => {
            Some(Command::MoveTo(Position { x, y }))
        }
        LiveFields::AttackTarget { target, flag: 0 } if target < u32::from(u16::MAX) => {
            let actor = target.checked_sub(u32::from(wire_id(0)))?;
            (actor != 0).then_some(Command::Attack(actor))
        }
        // Observed casts name Lorelai or Idris slots. A guessed A/B/C
        // mapping would apply a different ability to the authenticated hero.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Message, Rooms};
    use halcyon_platform::{MatchHost, Reservation};
    use std::net::Ipv4Addr;
    use uuid::Uuid;

    fn attack(target: u32, flag: u8) -> Message {
        let mut body = target.to_be_bytes().to_vec();
        body.push(flag);
        Message::new(0x0424, body)
    }

    fn point(x: f32, y: f32) -> Message {
        Message::new(0x03f5, [x.to_be_bytes(), y.to_be_bytes()].concat())
    }

    #[test]
    fn maps_known_target_and_movement_requests_only() {
        assert_eq!(
            from_live(LiveFields::AttackTarget {
                target: 1501,
                flag: 0
            }),
            Some(Command::Attack(2))
        );
        for (target, flag) in [
            (1499, 0),
            (1500, 1),
            (u32::from(u16::MAX), 0),
            (0, 0),
            (0x10000, 0),
        ] {
            assert!(from_live(LiveFields::AttackTarget { target, flag }).is_none());
        }
        assert_eq!(
            from_live(LiveFields::MovePoint { x: 2., y: 1. }),
            Some(Command::MoveTo(Position { x: 2., y: 1. }))
        );
        assert!(from_live(LiveFields::MovePoint { x: f32::NAN, y: 0. }).is_none());
        assert!(
            from_live(LiveFields::AbilityTarget {
                target: 1501,
                selector: 2
            })
            .is_none()
        );
        assert!(
            from_live(LiveFields::AbilityPoint {
                vector: [2., 0., 4.],
                selector: 3
            })
            .is_none()
        );
        assert!(from_live(LiveFields::LearnSelection(0)).is_none());
    }

    #[tokio::test]
    async fn live_requests_use_own_seat_fog_and_simulation_validation() {
        let rooms = Rooms::new(Ipv4Addr::LOCALHOST.into(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                match_id: id,
                players: vec![Uuid::new_v4(), Uuid::new_v4()],
                teams: vec![0, 1],
                mode: "5v5_casual".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut room = shared.lock().unwrap();
        let hero_start = room.world.entities[0].position;
        room.phase("running");

        let before = room.world.state_digest();
        for (socket, msg) in [
            (0, point(4., 0.)), // Control connection cannot submit gameplay.
            (1, point(f32::NAN, 0.)),
            (1, attack(1501, 0)),  // Hidden enemy.
            (1, attack(1500, 0)),  // Own actor.
            (1, attack(64999, 0)), // Missing actor.
            (1, Message::new(0x0411, [0, 0, 5, 221, 2])),
            (1, Message::new(0x0412, [0; 13])),
        ] {
            assert!(room.receive(0, socket, msg).unwrap().is_empty());
            assert_eq!(room.world.state_digest(), before);
        }

        assert!(room.receive(0, 1, point(4., 0.)).unwrap().is_empty());
        room.world.step();
        assert!(room.world.entities[0].position.x > hero_start.x);
        room.world.entities[1].position = Position { x: 3., y: 0. };
        assert!(room.receive(0, 1, attack(1501, 0)).unwrap().is_empty());
        let hit = (0..70).any(|_| room.world.step().into_iter().any(|event| {
            matches!(event, halcyon_sim::Event::Damage { source: 1, target: 2, amount, .. } if amount > 0.)
        }));
        assert!(
            hit,
            "the authenticated human's attack applies simulation damage"
        );
    }
}
