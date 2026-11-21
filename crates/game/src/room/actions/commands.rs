use crate::replication::{Batch, messages::wire_id};
use crate::room::Room;
use halcyon_protocol::{LiveFields, RecommendedBuildPath};
use halcyon_sim::{Command, Position};

/// Translate only gameplay requests whose input and authoritative behavior are known.
pub(crate) fn from_live(request: LiveFields<'_>) -> Option<Command> {
    match request {
        LiveFields::MovePoint { x, y } if x.is_finite() && y.is_finite() => {
            Some(Command::MoveTo(Position { x, y }))
        }
        LiveFields::AttackTarget { target, flag } if flag <= 1 && target < u32::from(u16::MAX) => {
            let actor = target.checked_sub(u32::from(wire_id(0)))?;
            (actor != 0).then_some(Command::Attack(actor))
        }
        // Observed casts name Lorelai or Idris slots. A guessed A/B/C
        // mapping would apply a different ability to the authenticated hero.
        _ => None,
    }
}

pub(crate) fn clears_attack(request: &LiveFields<'_>) -> bool {
    matches!(
        request,
        LiveFields::AttackTarget {
            target: u32::MAX,
            flag,
        } if *flag <= 1
    )
}

impl Room {
    pub(crate) fn recommended_paths(&self, viewer_team: Option<u8>) -> Batch {
        self.seats
            .iter()
            .zip(&self.world.entities)
            .filter(|(_, actor)| self.legacy_aral() || viewer_team.is_none_or(|t| t == actor.team))
            .filter_map(|(seat, actor)| {
                Some(seat.recommended_path?.applied_to(wire_id(actor.id).into()))
            })
            .collect()
    }

    pub(crate) fn set_recommended_path(&mut self, i: usize, path: RecommendedBuildPath) -> Batch {
        let actor = &self.world.entities[i];
        let response = vec![
            path.announcement(),
            path.applied_to(wire_id(actor.id).into()),
        ];
        if self.seats[i].recommended_path.replace(path) != Some(path) {
            for (j, seat) in self.seats.iter().enumerate() {
                if j != i
                    && (self.legacy_aral() || self.world.entities[j].team == actor.team)
                    && let Some(peer) = seat.gameplay_peer()
                {
                    peer.send(crate::transport::Delivery::Private(response.clone()));
                }
            }
            self.publish_observers(&response);
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Message;
    use crate::host::manager::Rooms;
    use halcyon_types::{MatchHost, Reservation};
    use std::net::Ipv4Addr;
    use uuid::Uuid;

    fn attack(target: u32, flag: u8) -> Message {
        let mut body = target.to_be_bytes().to_vec();
        body.push(flag);
        Message::known(crate::Opcode::RequestModifyBasicAttackTarget, body)
    }

    fn point(x: f32, y: f32) -> Message {
        Message::known(
            crate::Opcode::RequestMovePoint,
            [x.to_be_bytes(), y.to_be_bytes()].concat(),
        )
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
        assert_eq!(
            from_live(LiveFields::AttackTarget {
                target: 1501,
                flag: 1
            }),
            Some(Command::Attack(2))
        );
        assert!(clears_attack(&LiveFields::AttackTarget {
            target: u32::MAX,
            flag: 0
        }));
        assert!(clears_attack(&LiveFields::AttackTarget {
            target: u32::MAX,
            flag: 1
        }));
        assert_eq!(
            from_live(LiveFields::AttackTarget {
                target: 1500,
                flag: 1
            }),
            Some(Command::Attack(1))
        );
        for (target, flag) in [
            (1499, 0),
            (1500, 2),
            (1501, 2),
            (u32::from(u16::MAX), 0),
            (0, 0),
            (0x10000, 0),
        ] {
            assert!(from_live(LiveFields::AttackTarget { target, flag }).is_none());
            assert!(!clears_attack(&LiveFields::AttackTarget { target, flag }));
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
    async fn path_broadcast_and_reconnect_use_authenticated_actors_and_mode_scope() {
        for aral in [false, true] {
            let rooms = Rooms::new(Ipv4Addr::LOCALHOST.into(), "127.0.0.1".into(), 1, None);
            let players: Vec<_> = (0..6).map(|_| Uuid::new_v4()).collect();
            let id = Uuid::new_v4();
            rooms
                .reserve(Reservation {
                    talent_levels: Vec::new(),
                    match_id: id,
                    players: players.clone(),
                    teams: vec![1, 1, 1, 0, 0, 0],
                    mode: if aral { "casual_aral" } else { "5v5_casual" }.into(),
                    difficulty: None,
                })
                .unwrap();
            let shared = rooms.room(id).unwrap();
            let mut room = shared.lock().unwrap();
            let mut readers = Vec::new();
            for (seat, slot) in room.seats.iter_mut().enumerate() {
                let (tx, rx) = tokio::sync::mpsc::channel(8);
                let ch = usize::from(!aral);
                if aral {
                    let actor = u32::from(wire_id(seat as u32 + 1));
                    slot.preamble.observe(
                        &Message::known(crate::Opcode::RequestSceneSnapshot, [0; 6]),
                        actor,
                    );
                    let mut ready = actor.to_be_bytes().to_vec();
                    ready.extend([1, 0]);
                    slot.preamble.observe(
                        &Message::known(crate::Opcode::ConfirmSceneOwner, ready),
                        actor,
                    );
                    slot.preamble.observe(
                        &Message::known(crate::Opcode::SceneReadyConfirm, [0; 6]),
                        actor,
                    );
                }
                slot.peers[ch] = Some(crate::Peer {
                    generation: seat as u64,
                    tx,
                    stalled: tokio::sync::watch::channel(false).0,
                });
                readers.push(rx);
            }
            room.phase(crate::RoomPhase::Running);
            for value in [0xfe, 1] {
                let out = room
                    .receive(
                        2,
                        usize::from(!aral),
                        Message::known(crate::Opcode::RequestRecommendedBuildPath, [value]),
                    )
                    .unwrap();
                assert_eq!(out[1].body, [0, 0, 5, 222, value]);
                for (seat, rx) in readers.iter_mut().enumerate() {
                    if seat != 2 && (aral || seat < 3) {
                        let crate::transport::Delivery::Private(batch) = rx.try_recv().unwrap()
                        else {
                            panic!()
                        };
                        assert_eq!(batch, out);
                    } else {
                        assert!(rx.try_recv().is_err());
                    }
                }
                assert_eq!(
                    room.spawn_view(Some(1))
                        .iter()
                        .filter(|m| m.is(crate::Opcode::SetRecommendedBuildPath))
                        .count(),
                    1
                );
                assert_eq!(
                    room.spawn_view(Some(0))
                        .iter()
                        .filter(|m| m.is(crate::Opcode::SetRecommendedBuildPath))
                        .count(),
                    usize::from(aral)
                );
            }
        }
    }

    #[tokio::test]
    async fn live_requests_use_own_seat_fog_and_simulation_validation() {
        let rooms = Rooms::new(Ipv4Addr::LOCALHOST.into(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
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
        room.phase(crate::RoomPhase::Running);

        let before = room.world.state_digest();
        for (socket, msg) in [
            (0, point(4., 0.)), // Control connection cannot submit gameplay.
            (1, point(f32::NAN, 0.)),
            (1, attack(1501, 0)),  // Hidden enemy.
            (1, attack(1500, 0)),  // Own actor.
            (1, attack(64999, 0)), // Missing actor.
            (
                1,
                Message::known(crate::Opcode::RequestTargetAbility, [0, 0, 5, 221, 2]),
            ),
            (
                1,
                Message::known(crate::Opcode::RequestPointAbility, [0; 13]),
            ),
        ] {
            assert!(room.receive(0, socket, msg).unwrap().is_empty());
            assert_eq!(room.world.state_digest(), before);
        }

        assert!(room.receive(0, 1, point(4., 0.)).unwrap().is_empty());
        room.world.step();
        assert!(room.world.entities[0].position.x > hero_start.x);
        room.world.entities[1].position = Position { x: 3., y: 0. };
        assert_eq!(
            room.receive(0, 1, attack(1501, 0)).unwrap(),
            vec![crate::abilities::modify_basic_attack_target_message(Some(
                2
            ))]
        );
        let hit = (0..70).any(|_| room.world.step().into_iter().any(|event| {
            matches!(event, halcyon_sim::Event::Damage { source: 1, target: 2, amount, .. } if amount > 0.)
        }));
        assert!(
            hit,
            "the authenticated human's attack applies simulation damage"
        );
        assert_eq!(
            room.receive(0, 1, attack(u32::MAX, 0)).unwrap(),
            vec![crate::abilities::modify_basic_attack_target_message(None)]
        );
        assert!(room.world.entities[0].attack_target().is_none());
        let before_clear = room.world.entities[0].position.x;
        assert!(room.receive(0, 1, point(4., 0.)).unwrap().is_empty());
        assert!(room.receive(0, 1, attack(u32::MAX, 0)).unwrap().is_empty());
        room.world.step();
        assert!(
            room.world.entities[0].position.x > before_clear,
            "u32::MAX basic-target clear must not cancel active movement"
        );
    }
}
