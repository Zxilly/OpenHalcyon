use super::*;
use crate::Peer;
use crate::host::manager::Rooms;
use crate::replication::messages;
use halcyon_protocol::ce::KnownQueue;
use halcyon_types::{MatchHost, Reservation};
use std::collections::BTreeSet;
use tokio::sync::{mpsc, watch};
use uuid::Uuid;

fn drain_deliveries(rx: &mut mpsc::Receiver<crate::transport::Delivery>) -> Vec<Message> {
    let mut out = Vec::new();
    while let Ok(delivery) = rx.try_recv() {
        match delivery {
            crate::transport::Delivery::Private(batch) => out.extend(batch),
            crate::transport::Delivery::Team(batch) => out.extend_from_slice(batch.messages()),
        }
    }
    out
}

#[test]
fn catalog_covers_all_61_heroes_with_verified_cff0_learn_slots() {
    assert_eq!(HERO_SLOT_PROFILES.len(), 61);
    let mut seen_tids = BTreeSet::new();
    let mut seen_ids = BTreeSet::new();
    for profile in all_heroes() {
        assert!(
            seen_tids.insert(profile.type_id),
            "duplicate tid {} for {}",
            profile.type_id,
            profile.hero_id
        );
        assert!(
            seen_ids.insert(profile.hero_id),
            "duplicate hero_id {}",
            profile.hero_id
        );
        for (index, slot) in [
            (0, AbilitySlot::A),
            (1, AbilitySlot::B),
            (2, AbilitySlot::C),
        ] {
            let (mapped_slot, operand) = learn_request(profile.type_id, index).unwrap();
            assert_eq!(mapped_slot, slot);
            assert_eq!(operand, profile.learn_slots[usize::from(index)]);
            assert_eq!(learn_operand(profile.type_id, slot), Some(operand));
            let spec = selector_spec(profile.type_id, operand)
                .unwrap_or_else(|| panic!("missing primary selector for {}", profile.hero_id));
            assert_eq!(spec.role, CatalogSlotRole::Primary(slot));
        }
    }
    assert!(learn_request(242, 3).is_none());
    assert_eq!(learn_request(242, 0), Some((AbilitySlot::A, 1)));
    assert_eq!(learn_request(242, 1), Some((AbilitySlot::B, 2)));
    assert_eq!(learn_request(242, 2), Some((AbilitySlot::C, 3)));
    assert_eq!(learn_request(395, 0), Some((AbilitySlot::A, 1)));
    assert_eq!(learn_request(269, 1), Some((AbilitySlot::B, 2)));
    assert_eq!(learn_request(250, 0), Some((AbilitySlot::A, 1)));
    assert_eq!(learn_request(439, 2), Some((AbilitySlot::C, 3)));
    assert_eq!(learn_request(273, 1), Some((AbilitySlot::B, 3)));
    assert_eq!(learn_request(273, 2), Some((AbilitySlot::C, 4)));
    assert_eq!(learn_request(274, 2), Some((AbilitySlot::C, 8)));
    assert_eq!(learn_request(413, 2), Some((AbilitySlot::C, 8)));
    assert_eq!(learn_request(432, 0), Some((AbilitySlot::A, 2)));
    assert_eq!(learn_request(924, 2), Some((AbilitySlot::C, 3)));
    assert_eq!(learn_request(435, 2), Some((AbilitySlot::C, 3)));
    assert_eq!(malene_form_operands(AbilitySlot::A), [0, 1]);
    assert_eq!(malene_form_operands(AbilitySlot::B), [2, 3]);
    assert_eq!(malene_form_operands(AbilitySlot::C), [4, 5]);
}

#[test]
fn all_61_heroes_wire_selectors_convert_0411_0412_to_0415_0416_043c() {
    let entities = [
        ActorTeamRef {
            id: 1,
            team: 0,
            pos: Position { x: 0.0, y: 0.0 },
            alive: true,
        },
        ActorTeamRef {
            id: 2,
            team: 0,
            pos: Position { x: 2.0, y: 0.0 },
            alive: true,
        },
        ActorTeamRef {
            id: 3,
            team: 1,
            pos: Position { x: 4.0, y: 0.0 },
            alive: true,
        },
    ];
    let enemy_wire = u32::from(wire_id(3));
    let ally_wire = u32::from(wire_id(2));

    for profile in all_heroes() {
        for entry in profile.selectors {
            if matches!(entry.role, CatalogSlotRole::Cancel(_)) {
                let mut body = [0u8; 6];
                body[..4].copy_from_slice(&u32::MAX.to_be_bytes());
                body[4] = entry.selector;
                let req = Message::known(Opcode::RequestTargetAbility, body);
                let intent = decode_cast_intent(profile.type_id, 1, 0, &entities, &req).unwrap();
                assert_eq!(
                    intent,
                    CatalogCastIntent::Cancel {
                        reply: cancel_message(1500, entry.selector, true),
                    },
                    "hero {} cancel selector {}",
                    profile.hero_id,
                    entry.selector
                );
                continue;
            }
            if entry.input.accepts_point() {
                let mut body = [0u8; 14];
                body[0..4].copy_from_slice(&(-0.909_949f32).to_be_bytes());
                body[4..8].copy_from_slice(&0.005_715f32.to_be_bytes());
                body[8..12].copy_from_slice(&4.366_973f32.to_be_bytes());
                body[12] = entry.selector;
                let req = Message::known(Opcode::RequestPointAbility, body);
                let intent = decode_cast_intent(profile.type_id, 1, 0, &entities, &req).unwrap();
                assert_eq!(
                    intent,
                    CatalogCastIntent::Cast {
                        slot: entry.role.slot(),
                        target: CastTarget::Point(Position {
                            x: -0.909_949,
                            y: 4.366_973,
                        }),
                        reply: Message::ability_point_used(
                            1500,
                            [-0.909_949, 0.005_715, 4.366_973],
                            entry.selector,
                        ),
                    },
                    "hero {} point selector {}",
                    profile.hero_id,
                    entry.selector
                );
            } else {
                let (target_wire, expected_cast_target, expected_reply_target) = match entry.input {
                    CatalogInputKind::TargetActor => {
                        (enemy_wire, CastTarget::Entity(3), enemy_wire)
                    }
                    CatalogInputKind::AllyOrSelf => (ally_wire, CastTarget::Entity(2), ally_wire),
                    CatalogInputKind::SelfOnly
                    | CatalogInputKind::PointOrSelf
                    | CatalogInputKind::PointOrTarget => (u32::MAX, CastTarget::SelfCast, u32::MAX),
                    CatalogInputKind::Point => unreachable!(),
                };
                let mut body = [0u8; 6];
                body[..4].copy_from_slice(&target_wire.to_be_bytes());
                body[4] = entry.selector;
                let req = Message::known(Opcode::RequestTargetAbility, body);
                let intent = decode_cast_intent(profile.type_id, 1, 0, &entities, &req).unwrap();
                assert_eq!(
                    intent,
                    CatalogCastIntent::Cast {
                        slot: entry.role.slot(),
                        target: expected_cast_target,
                        reply: Message::ability_target_used(
                            1500,
                            expected_reply_target,
                            entry.selector,
                        ),
                    },
                    "hero {} target selector {}",
                    profile.hero_id,
                    entry.selector
                );
            }
        }
    }
}

#[test]
fn segment_zone_barrier_matches_0422_and_0423_wire_and_intersection() {
    let barrier = SegmentBarrier {
        zone_id: 9001,
        owner_wire: 1500,
        p0: Position { x: -4.0, y: 2.0 },
        p1: Position { x: 4.0, y: 2.0 },
    };
    let create = barrier.to_create_message().unwrap();
    assert_eq!(create.known_opcode(), Some(Opcode::CreateRectZoneOfControl));
    assert_eq!(create.body.len(), 24);
    assert_eq!(
        create.zone_action(),
        Some(ZoneAction::Rectangle {
            id: 9001,
            owner: 1500,
            p0_xz_bits: [(-4.0f32).to_bits(), 2.0f32.to_bits()],
            p1_xz_bits: [4.0f32.to_bits(), 2.0f32.to_bits()],
            suffix: &[],
        })
    );
    let destroy = SegmentBarrier::to_destroy_message(9001).unwrap();
    assert_eq!(destroy.known_opcode(), Some(Opcode::DestroyZoneOfControl));
    assert_eq!(
        destroy.zone_action(),
        Some(ZoneAction::Destroy {
            id: 9001,
            suffix: &[]
        })
    );

    assert!(segments_intersect(
        barrier.p0,
        barrier.p1,
        Position { x: 0.0, y: -1.0 },
        Position { x: 0.0, y: 5.0 }
    ));
    assert!(!segments_intersect(
        barrier.p0,
        barrier.p1,
        Position { x: 0.0, y: 3.0 },
        Position { x: 0.0, y: 7.0 }
    ));
    assert!(segments_intersect(
        barrier.p0,
        barrier.p1,
        Position { x: 2.0, y: 2.0 },
        Position { x: 6.0, y: 2.0 }
    ));
}

#[tokio::test]
async fn live_5v5_channel_1_silvernail_tripwires_emit_0422_and_0423() {
    let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
    let id = Uuid::new_v4();
    rooms
        .reserve(Reservation {
            match_id: id,
            players: vec![Uuid::new_v4(), Uuid::new_v4()],
            teams: vec![0, 1],
            talent_levels: Vec::new(),
            difficulty: None,
            mode: KnownQueue::CasualFive.as_str().into(),
        })
        .unwrap();
    let shared = rooms.room(id).unwrap();
    let (tx0, mut rx0) = mpsc::channel(32);
    let (tx1, mut rx1) = mpsc::channel(32);
    {
        let mut room = shared.lock().unwrap();
        room.phase(crate::RoomPhase::Running);
        for (seat_idx, tx) in [tx0, tx1].into_iter().enumerate() {
            room.seats[seat_idx].peers[1] = Some(Peer {
                generation: 1,
                tx,
                stalled: watch::channel(false).0,
            });
        }
        room.world.entities[0].type_id = SILVERNAIL_TYPE_ID;
        room.world.entities[0].level = 2;
        room.world.entities[0].skill_points = 1;
        room.world.entities[0].position = Position { x: 0.0, y: 0.0 };
        room.world.entities[1].position = Position { x: 3.0, y: 0.0 };
        let caster_decl = messages::spawn(&room.world.entities[0], &room.picks, None);
        let enemy_decl = messages::spawn(&room.world.entities[1], &room.picks, None);
        for seat in &mut room.scene_views {
            seat.declarations(&[caster_decl.clone(), enemy_decl.clone()]);
        }
        let hero_id = room.world.entities[0].id;
        let enemy_id = room.world.entities[1].id;
        room.visible[0].insert(enemy_id);
        room.visible[1].insert(hero_id);

        // Verify Silvernail learn mapping (index 0 -> AbilitySlot::A -> CFF0 selector 2)
        // and 0x043A grant + 0x0422/0x0423 tripwire lifecycle on Event::Cast.
        assert_eq!(
            learn_request(SILVERNAIL_TYPE_ID, 0),
            Some((AbilitySlot::A, 2))
        );
        room.world.entities[0].ability_ranks[0] = 1;
        publish_event(
            &mut room,
            &Event::Learned {
                source: hero_id,
                slot: AbilitySlot::A,
            },
        );
        assert_eq!(
            drain_deliveries(&mut rx1),
            vec![Message::ability_learned(1500, 2)]
        );

        // Stake 1: no wire until paired with stake 2.
        publish_event(
            &mut room,
            &Event::Cast {
                source: hero_id,
                slot: AbilitySlot::A,
                target: CastTarget::Point(Position { x: 1.0, y: 2.0 }),
            },
        );
        assert!(
            drain_deliveries(&mut rx0)
                .iter()
                .all(|m| m.known_opcode() != Some(Opcode::CreateRectZoneOfControl))
        );
        let _ = drain_deliveries(&mut rx1);

        // Stake 2 within 16u: emits 0x0422 CreateRectZoneOfControl to caster & witness.
        publish_event(
            &mut room,
            &Event::Cast {
                source: hero_id,
                slot: AbilitySlot::A,
                target: CastTarget::Point(Position { x: 5.0, y: 2.0 }),
            },
        );
        let caster_msgs = drain_deliveries(&mut rx0);
        let witness_msgs = drain_deliveries(&mut rx1);
        let zone1 = caster_msgs
            .iter()
            .find(|m| m.known_opcode() == Some(Opcode::CreateRectZoneOfControl))
            .expect("expected 0x0422 zone creation for caster");
        assert!(
            witness_msgs.contains(zone1),
            "expected identical 0x0422 for visible enemy peer"
        );
        let zone1_id = match zone1.zone_action() {
            Some(ZoneAction::Rectangle {
                id,
                owner,
                p0_xz_bits,
                p1_xz_bits,
                ..
            }) => {
                assert_eq!(owner, 1500);
                assert_eq!(p0_xz_bits, [1.0f32.to_bits(), 2.0f32.to_bits()]);
                assert_eq!(p1_xz_bits, [5.0f32.to_bits(), 2.0f32.to_bits()]);
                id
            }
            other => panic!("unexpected zone1 packet: {other:?}"),
        };

        // Stake 3 & 4 prune oldest wire -> 0x0423 DestroyZoneOfControl + new 0x0422.
        publish_event(
            &mut room,
            &Event::Cast {
                source: hero_id,
                slot: AbilitySlot::A,
                target: CastTarget::Point(Position { x: 6.0, y: -1.0 }),
            },
        );
        let _ = drain_deliveries(&mut rx0);
        let _ = drain_deliveries(&mut rx1);
        publish_event(
            &mut room,
            &Event::Cast {
                source: hero_id,
                slot: AbilitySlot::A,
                target: CastTarget::Point(Position { x: 8.0, y: -1.0 }),
            },
        );
        let prune_msgs = drain_deliveries(&mut rx0);
        assert!(prune_msgs.iter().any(|m| {
            m.known_opcode() == Some(Opcode::DestroyZoneOfControl)
                && matches!(m.zone_action(), Some(ZoneAction::Destroy { id, .. }) if id == zone1_id)
        }));
        let _ = drain_deliveries(&mut rx1);

        // Expiry tick cleanup emits 0x0423 for remaining active wires.
        room.world.tick += 200;
        tick_tripwires(&mut room);
        let expired_msgs = drain_deliveries(&mut rx0);
        assert!(
            expired_msgs
                .iter()
                .any(|m| m.known_opcode() == Some(Opcode::DestroyZoneOfControl))
        );
        assert!(
            drain_deliveries(&mut rx1)
                .iter()
                .any(|m| m.known_opcode() == Some(Opcode::DestroyZoneOfControl))
        );
    }
}

#[tokio::test]
async fn all_61_heroes_channel_1_learn_round_trip_covers_every_cff0_slot() {
    for profile in HERO_SLOT_PROFILES {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                match_id: id,
                players: vec![Uuid::new_v4(), Uuid::new_v4()],
                teams: vec![0, 1],
                talent_levels: Vec::new(),
                difficulty: None,
                mode: KnownQueue::CasualFive.as_str().into(),
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let (tx0, _rx0) = mpsc::channel(32);
        let (tx1, mut rx1) = mpsc::channel(32);
        let mut room = shared.lock().unwrap();
        room.phase(crate::RoomPhase::Running);
        for (seat_idx, tx) in [tx0, tx1].into_iter().enumerate() {
            room.seats[seat_idx].peers[1] = Some(Peer {
                generation: 1,
                tx,
                stalled: watch::channel(false).0,
            });
        }
        room.world.entities[0].type_id = profile.type_id;
        room.world.entities[0].level = 6;
        room.world.entities[0].skill_points = 3;
        let mut caster_decl = messages::spawn(&room.world.entities[0], &room.picks, None);
        caster_decl.body[745] = 1;
        for seat in &mut room.scene_views {
            seat.declarations(std::slice::from_ref(&caster_decl));
        }
        let hero_id = room.world.entities[0].id;
        let enemy_id = room.world.entities[1].id;
        room.visible[0].insert(hero_id);
        room.visible[0].insert(enemy_id);
        room.visible[1].insert(hero_id);
        room.visible[1].insert(enemy_id);

        for (slot_idx, &learn_index) in profile.learn_slots.iter().enumerate() {
            let slot = match slot_idx {
                0 => AbilitySlot::A,
                1 => AbilitySlot::B,
                _ => AbilitySlot::C,
            };
            let primary = profile
                .selectors
                .iter()
                .find(|s| s.role == CatalogSlotRole::Primary(slot))
                .unwrap_or_else(|| panic!("{} missing primary selector", profile.hero_id));
            assert_eq!(
                learn_request(profile.type_id, slot_idx as u8),
                Some((slot, primary.selector)),
                "hero {} learn slot_idx {} -> CFF0 {}",
                profile.hero_id,
                slot_idx,
                learn_index
            );
            publish_event(
                &mut room,
                &Event::Learned {
                    source: hero_id,
                    slot,
                },
            );
            assert_eq!(
                drain_deliveries(&mut rx1),
                vec![Message::ability_learned(1500, u32::from(primary.selector))],
                "witness learn failed for {}",
                profile.hero_id
            );

            let target = if primary.input.accepts_point() {
                CastTarget::Point(Position { x: 2.5, y: -1.0 })
            } else if matches!(
                primary.input,
                CatalogInputKind::AllyOrSelf | CatalogInputKind::SelfOnly
            ) {
                CastTarget::SelfCast
            } else {
                CastTarget::Entity(enemy_id)
            };
            publish_event(
                &mut room,
                &Event::Cast {
                    source: hero_id,
                    slot,
                    target,
                },
            );
            let msgs = drain_deliveries(&mut rx1);
            let expected = default_cast_message(1500, profile.type_id, slot, target)
                .expect("default cast packet");
            assert!(
                msgs.contains(&expected),
                "witness cast failed for {} {:?}",
                profile.hero_id,
                slot
            );
        }
    }
}

#[tokio::test]
async fn live_5v5_channel_1_learns_and_casts_non_standard_cff0_heroes() {
    for (type_id, index, selector, target_kind) in [
        (269u16, 0u8, 0u8, CatalogInputKind::Point),
        (273u16, 1u8, 3u8, CatalogInputKind::Point),
        (409u16, 0u8, 0u8, CatalogInputKind::Point),
        (409u16, 1u8, 1u8, CatalogInputKind::Point),
        (409u16, 2u8, 2u8, CatalogInputKind::AllyOrSelf),
        (243u16, 1u8, 1u8, CatalogInputKind::SelfOnly),
        (244u16, 0u8, 0u8, CatalogInputKind::AllyOrSelf),
    ] {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                match_id: id,
                players: vec![Uuid::new_v4(), Uuid::new_v4()],
                teams: vec![0, 1],
                talent_levels: Vec::new(),
                difficulty: None,
                mode: KnownQueue::CasualFive.as_str().into(),
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let (tx0, mut rx0) = mpsc::channel(32);
        let (tx1, mut rx1) = mpsc::channel(32);
        {
            let mut room = shared.lock().unwrap();
            room.phase(crate::RoomPhase::Running);
            for (seat_idx, tx) in [tx0, tx1].into_iter().enumerate() {
                room.seats[seat_idx].peers[1] = Some(Peer {
                    generation: 1,
                    tx,
                    stalled: watch::channel(false).0,
                });
            }
            room.world.entities[0].type_id = type_id;
            room.world.entities[0].level = 6;
            room.world.entities[0].skill_points = 1;
            room.world.entities[0].energy = 1000.0;
            room.world.entities[0].max_energy = 1000.0;
            room.world.entities[0].position = Position { x: 0.0, y: 0.0 };
            room.world.entities[1].position = Position { x: 3.0, y: 0.0 };
            let caster_decl = messages::spawn(&room.world.entities[0], &room.picks, None);
            let enemy_decl = messages::spawn(&room.world.entities[1], &room.picks, None);
            for seat in &mut room.scene_views {
                seat.declarations(&[caster_decl.clone(), enemy_decl.clone()]);
            }
            let hero_id = room.world.entities[0].id;
            let enemy_id = room.world.entities[1].id;
            room.visible[0].insert(enemy_id);
            room.visible[1].insert(hero_id);

            let learn = Message::known(Opcode::RequestUpgradeAbility, [index, 0, 0, 0, 0, 0]);
            let out = room.receive(0, 1, learn.clone()).unwrap();
            assert_eq!(
                out,
                vec![learn, Message::ability_learned(1500, u32::from(selector))]
            );
            assert_eq!(
                drain_deliveries(&mut rx1),
                vec![Message::ability_learned(1500, u32::from(selector))]
            );

            let (req, expected) = match target_kind {
                CatalogInputKind::Point => {
                    let mut body = [0u8; 14];
                    body[0..4].copy_from_slice(&3.5f32.to_be_bytes());
                    body[4..8].copy_from_slice(&0.005_715f32.to_be_bytes());
                    body[8..12].copy_from_slice(&(-1.25f32).to_be_bytes());
                    body[12] = selector;
                    (
                        Message::known(Opcode::RequestPointAbility, body),
                        Message::ability_point_used(1500, [3.5, 0.005_715, -1.25], selector),
                    )
                }
                CatalogInputKind::TargetActor => {
                    let enemy_wire = u32::from(wire_id(enemy_id));
                    let mut body = [0u8; 6];
                    body[..4].copy_from_slice(&enemy_wire.to_be_bytes());
                    body[4] = selector;
                    (
                        Message::known(Opcode::RequestTargetAbility, body),
                        Message::ability_target_used(1500, enemy_wire, selector),
                    )
                }
                _ => {
                    let mut body = [0u8; 6];
                    body[..4].copy_from_slice(&u32::MAX.to_be_bytes());
                    body[4] = selector;
                    (
                        Message::known(Opcode::RequestTargetAbility, body),
                        Message::ability_target_used(1500, u32::MAX, selector),
                    )
                }
            };
            assert!(room.receive(0, 1, req).unwrap().is_empty());
            assert!(
                room.seats[0].native_abilities.catalog_cast.is_some(),
                "cast failed for hero {type_id} selector {selector}"
            );

            let mut emitted = Vec::new();
            for _ in 0..40 {
                room.tick();
                emitted.extend(drain_deliveries(&mut rx0));
                if emitted.contains(&expected) {
                    break;
                }
            }
            assert!(
                emitted.contains(&expected),
                "hero {type_id} selector {selector} missing {expected:?} in {emitted:?}"
            );
        }
    }
}

#[tokio::test]
async fn baron_out_of_range_0412_clamps_and_0411_0413_quick_cast_emit_valid_replies() {
    let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
    let id = Uuid::new_v4();
    rooms
        .reserve(Reservation {
            match_id: id,
            players: vec![Uuid::new_v4(), Uuid::new_v4()],
            teams: vec![0, 1],
            talent_levels: Vec::new(),
            difficulty: None,
            mode: KnownQueue::CasualFive.as_str().into(),
        })
        .unwrap();
    let shared = rooms.room(id).unwrap();
    let (tx0, mut rx0) = mpsc::channel(64);
    let (tx1, _rx1) = mpsc::channel(64);
    {
        let mut room = shared.lock().unwrap();
        room.phase(crate::RoomPhase::Running);
        for (seat_idx, tx) in [tx0, tx1].into_iter().enumerate() {
            room.seats[seat_idx].peers[1] = Some(Peer {
                generation: 1,
                tx,
                stalled: watch::channel(false).0,
            });
        }
        room.world.enable_builtin_hero_kits();
        room.world.entities[0].type_id = 261; // Baron (selectors 1, 2, 3)
        room.world.entities[0].level = 9;
        room.world.entities[0].skill_points = 9;
        room.world.entities[0].energy = 1000.0;
        room.world.entities[0].max_energy = 1000.0;
        room.world.entities[0].position = Position { x: 20.0, y: 0.0 };
        room.world.entities[1].position = Position { x: 25.0, y: 0.0 };
        let hero_id = room.world.entities[0].id;
        let enemy_id = room.world.entities[1].id;
        room.visible[0].insert(enemy_id);
        room.visible[1].insert(hero_id);

        // Learn Baron A (learn index 0 -> selector 1).
        room.receive(
            0,
            1,
            Message::known(Opcode::RequestUpgradeAbility, [0, 0, 0, 0, 0, 0]),
        )
        .unwrap();
        let _ = drain_deliveries(&mut rx0);

        // 1. Out-of-range 0x0412 (25m away) clamps inside Baron A's 8m range and survives 0x03F4 moves!
        let mut pt_body = [0u8; 14];
        pt_body[0..4].copy_from_slice(&(45.0f32).to_be_bytes());
        pt_body[4..8].copy_from_slice(&0.0f32.to_be_bytes());
        pt_body[8..12].copy_from_slice(&(0.0f32).to_be_bytes());
        pt_body[12] = 1;
        room.receive(0, 1, Message::known(Opcode::RequestPointAbility, pt_body))
            .unwrap();

        let mut emitted = Vec::new();
        for _ in 0..8 {
            let _ = room.receive(0, 1, Message::movement(15.0, 0.0));
            room.tick();
            emitted.extend(drain_deliveries(&mut rx0));
        }
        let clamped_reply = emitted
            .iter()
            .find(|m| m.is(Opcode::PlayPointAbility) && m.body.get(16) == Some(&1))
            .expect("Baron A out-of-range + joystick move must emit 0x0416");
        let x = f32::from_be_bytes(clamped_reply.body[4..8].try_into().unwrap());
        assert!(
            (27.0..=28.0).contains(&x),
            "expected clamped Baron A X in [27.0, 28.0], got {x}"
        );

        // 2. Joystick quick-cast 0x0411 targeting enemy actor 1501 converts cleanly to Baron A point cast.
        let mut tgt_body = [0u8; 6];
        tgt_body[0..4].copy_from_slice(&1501u32.to_be_bytes());
        tgt_body[4] = 1;
        room.receive(0, 1, Message::known(Opcode::RequestTargetAbility, tgt_body))
            .unwrap();
        let mut quick_emitted = Vec::new();
        for _ in 0..8 {
            room.tick();
            quick_emitted.extend(drain_deliveries(&mut rx0));
        }
        assert!(
            quick_emitted
                .iter()
                .any(|m| m.is(Opcode::PlayPointAbility) && m.body.get(16) == Some(&1)),
            "0x0411 quick-cast on Point ability must emit 0x0416"
        );

        // Allow a charge to regenerate before testing 0x0413 on Baron A.
        for _ in 0..105 {
            room.tick();
        }
        let _ = drain_deliveries(&mut rx0);

        // 3. 0x0413 vector/unseen ability packet decodes & emits PlayPointAbility for Baron A.
        let mut vec_body = Vec::with_capacity(17);
        vec_body.extend_from_slice(&u32::MAX.to_be_bytes());
        vec_body.extend_from_slice(&(23.0f32).to_be_bytes());
        vec_body.extend_from_slice(&0.0f32.to_be_bytes());
        vec_body.extend_from_slice(&(1.0f32).to_be_bytes());
        vec_body.push(1);
        room.receive(
            0,
            1,
            Message::known(Opcode::RequestActivateAbilityUnseen, vec_body),
        )
        .unwrap();
        let mut unseen_emitted = Vec::new();
        for _ in 0..8 {
            room.tick();
            unseen_emitted.extend(drain_deliveries(&mut rx0));
        }
        assert!(
            unseen_emitted
                .iter()
                .any(|m| m.is(Opcode::PlayPointAbility) && m.body.get(16) == Some(&1)),
            "0x0413 packet must emit 0x0416"
        );
    }
}
