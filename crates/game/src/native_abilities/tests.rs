use crate::{Peer, Rooms};
use halcyon_platform::{MatchHost, Reservation};
use tokio::sync::{mpsc, watch};
use uuid::Uuid;

use super::*;

#[test]
fn twister_writes_speed_before_destination_and_only_accepts_the_flat_aral_profile() {
    let mut m = aral_dash_message(
        3,
        Position {
            x: -5.930279,
            y: 4.824222,
        },
        21.875313,
    );
    assert!(recordable_dash(&m));
    assert_eq!(u32::from_be_bytes(m.body[0..4].try_into().unwrap()), 1502);
    assert_eq!(&m.body[4..8], &21.875313f32.to_be_bytes());
    assert_eq!(&m.body[8..12], &(-5.930279f32).to_be_bytes());
    assert_eq!(&m.body[12..16], &0x3d192e1fu32.to_be_bytes());
    assert_eq!(&m.body[16..20], &4.824222f32.to_be_bytes());
    m.body[20] = 1;
    assert!(!recordable_dash(&m));
}
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
            (crate::wire_opcode::RequestSceneSnapshot, [0; 6]),
            (
                crate::wire_opcode::ConfirmSceneOwner,
                [0, 0, 5, 220 + i as u8, 1, 0],
            ),
            (crate::wire_opcode::SceneReadyConfirm, [0; 6]),
        ] {
            seat.preamble
                .observe(&Message::new(opcode, body), 1500 + i as u32);
        }
        recipients.push(rx);
    }
    room.phase(crate::RoomPhase::Running);
    room.world.entities[0].type_id = IDRIS;
    room.world.entities[0].skill_points = 2;
    room.world.entities[0].position = Position { x: 8.0, y: 0.0 };

    let learn = Message::known(crate::Opcode::RequestUpgradeAbility, [1, 0, 0, 0, 0, 0]);
    let mut body = [0; 14];
    body[..4].copy_from_slice(&12f32.to_be_bytes());
    body[4..8].copy_from_slice(&(-0.0_f32).to_be_bytes());
    body[12] = NativeCast::IdrisB.selector();
    let cast = Message::known(crate::Opcode::RequestPointAbility, body);

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
    let cancel = |selector, shared| {
        Message::known(
            crate::Opcode::RequestCancelAbility,
            [selector, shared, 0, 0, 0, 0],
        )
    };
    assert!(room.receive(0, 0, cancel(2, 0)).unwrap().is_empty());
    assert!(room.receive(0, 0, cancel(3, 1)).unwrap().is_empty());
    assert!(room.receive(1, 0, cancel(3, 0)).unwrap().is_empty());
    assert_eq!(
        room.receive(0, 0, cancel(3, 0)).unwrap(),
        [
            cancel(3, 0),
            Message::known(
                crate::Opcode::CancelAbility,
                [0, 0, 5, 220, 0, 0, 0, 3, 0, 0]
            )
        ]
    );
    assert!(room.receive(0, 0, cancel(3, 0)).unwrap().is_empty());
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
    assert_eq!(issuer.len(), 4);
    assert_eq!(
        issuer[0],
        Message::ability_point_used(1500, [12., -0.0, 0.], 3)
    );
    assert_eq!(issuer[1], Message::position(1500, stopped.x, stopped.y));
    let buffs = [&issuer[2], &issuer[3]].map(|message| {
        let Ok(halcyon_protocol::Fields::BuffApplication(buff)) = message.fields() else {
            panic!("missing released Chakram application")
        };
        assert_eq!(
            (
                buff.prefix.source,
                buff.prefix.target,
                buff.prefix.duration_bits
            ),
            (1500, 1500, 0x4066)
        );
        assert!(buff.prefix.instance > 1505);
        assert_eq!(buff.suffix, [0; 6]);
        buff.prefix
    });
    assert_eq!(buffs.map(|b| b.definition), [61, 820]);
    assert_eq!(buffs[0].instance + 1, buffs[1].instance);
    assert_eq!(drain(&mut recipients[1]), issuer[..1]);
    let teammate = drain(&mut recipients[2]);
    let stop = chakram_motion::stop(1500, stopped).unwrap();
    assert!(recordable_stop(&stop));
    assert_eq!(
        teammate,
        [
            stop,
            issuer[0].clone(),
            issuer[1].clone(),
            issuer[2].clone(),
            issuer[3].clone()
        ]
    );
    assert!(room.receive(0, 0, cast).unwrap().is_empty());
    for _ in 0..12 {
        room.tick();
    }
    assert!(drain(&mut recipients[0]).is_empty());

    let point = room.world.entities[0].position;
    room.world.entities[1].position = point;
    room.world.command(1, Command::Attack(2)).unwrap();
    for recipient in &mut recipients {
        while recipient.try_recv().is_ok() {}
    }
    room.world.entities[0].type_id = EntityTemplate::Alpha.as_raw();
    let reboot = [Event::RebootStarted {
        actor: 1,
        source: 2,
        instance: 400,
        cooldown_seconds: 85.,
    }];
    room.visible[0].insert(1);
    room.visible[1].remove(&1);
    publish(&mut room, &reboot);
    let frames = recipients[2].try_recv().unwrap().into_records();
    assert_eq!(
        frames[0],
        Message::known(crate::Opcode::ActorUndead, 1500u32.to_be_bytes())
    );
    assert!(
        matches!(frames[1].fields(),Ok(halcyon_protocol::Fields::BuffApplication(b)) if b.prefix.definition==683 && b.prefix.duration_bits==0x4300 && b.prefix.instance==1899 && b.prefix.target==1500 && b.prefix.source==1500)
    );
    assert_eq!(recipients[0].try_recv().unwrap().into_records(), frames);
    assert!(recipients[1].try_recv().is_err());
    room.world.entities[0].type_id = IDRIS;
    let change = [Event::BasicTargetChanged {
        actor: 1,
        target: 2,
    }];
    room.visible[0].remove(&2);
    publish(&mut room, &change);
    assert!(drain(&mut recipients[0]).is_empty());
    let mut target = super::super::messages::spawn(&room.world.entities[1], &[], None);
    target.body[745] = 17;
    room.scene_views[0].declarations(&[target]);
    room.visible[0].insert(2);
    publish(&mut room, &change);
    assert_eq!(
        drain(&mut recipients[0]),
        [Message::known(
            crate::Opcode::ModifyBasicAttackTarget,
            1501u32.to_be_bytes()
        )]
    );
    assert!(drain(&mut recipients[1]).is_empty());
    assert!(drain(&mut recipients[2]).is_empty());
}

#[tokio::test]
#[ignore = "requires original delivery and supplemental combat data"]
async fn authenticated_amael_learns_and_confirms_the_registered_enemy_kick() {
    let data = halcyon_data::GameData::load_with_extra(
        std::env::var_os("HALCYON_DELIVERY").unwrap(),
        Some(std::path::Path::new(
            &std::env::var_os("HALCYON_DATA_EXTRA").unwrap(),
        )),
    )
    .unwrap();
    let rooms = Rooms::new(
        "127.0.0.1".parse().unwrap(),
        "127.0.0.1".into(),
        1,
        Some(std::sync::Arc::new(data)),
    );
    let id = Uuid::new_v4();
    rooms
        .reserve(Reservation {
            match_id: id,
            players: (0..6).map(|_| Uuid::new_v4()).collect(),
            teams: vec![],
            mode: "casual_aral".into(),
            difficulty: None,
            talent_levels: vec![],
        })
        .unwrap();
    let shared = rooms.room(id).unwrap();
    let mut room = shared.lock().unwrap();
    room.world.use_aral_lane().unwrap();
    room.world.entities[0].type_id = AMAEL;
    room.world.entities[0].level = 6;
    room.world.entities[0].skill_points = 1;
    room.world.entities[0].energy = 400.;
    room.world.entities[0].max_energy = 400.;
    room.world.entities[0].position = Position { x: 20., y: 1. };
    room.world.entities[1].position = Position { x: 23., y: 5. };
    let (tx, mut rx) = mpsc::channel(32);
    room.seats[0].peers[0] = Some(Peer {
        generation: 42,
        tx,
        stalled: watch::channel(false).0,
    });
    let learn = Message::known(crate::Opcode::RequestUpgradeAbility, [2, 0, 0, 0, 0, 0]);
    assert!(receive(&mut room, 0, 0, learn.clone()).is_empty());
    room.seats[0].world_actor_written = true;
    let mut hero = crate::messages::spawn(&room.world.entities[0], &room.picks, None);
    hero.body[745] = 16;
    let mut target = crate::messages::spawn(&room.world.entities[1], &room.picks, None);
    target.body[745] = 17;
    room.scene_views[0].declarations(&[hero]);
    assert_eq!(
        receive(&mut room, 0, 0, learn.clone()),
        [learn, Message::ability_learned(1500, 2)]
    );
    let request = Message::known(crate::Opcode::RequestTargetAbility, [0, 0, 5, 221, 2, 0]);
    assert!(receive(&mut room, 0, 0, request.clone()).is_empty());
    assert!(room.seats[0].native_abilities.cast.is_none());
    room.scene_views[0].declarations(&[target]);
    room.visible[0].insert(2);
    receive(&mut room, 0, 0, request);
    assert!(room.seats[0].native_abilities.cast.is_some());
    let mut hits = 0;
    for _ in 0..8 {
        let ev = room.world.step();
        hits += ev
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    Event::AmaelKick {
                        source: 1,
                        target: 2,
                        kind: halcyon_sim::KickKind::SuperKick,
                        ..
                    }
                )
            })
            .count();
        publish(&mut room, &ev);
    }
    assert_eq!(hits, 1);
    let release = rx.try_recv().unwrap().into_records();
    assert_eq!(release.len(), 2);
    assert_eq!(release[0], Message::ability_target_used(1500, 1501, 2));
    let motion = release[1].forced_move().unwrap();
    assert_eq!(motion.actor, 1500);
    assert_eq!(motion.mode, halcyon_protocol::ForcedMoveMode::Dash);
    assert!((motion.speed - 3.2 / 0.30).abs() < 0.001);
    assert!((motion.destination[0] - 21.92).abs() < 0.001);
    assert!((motion.destination[2] - 3.56).abs() < 0.001);
}

#[tokio::test]
#[ignore = "requires original delivery and supplemental combat data"]
async fn amael_elbow_drop_delivers_owner_release_visible_observer_and_gates_hidden_victims() {
    use halcyon_protocol::{ACK_FRAME, MatchCipher, async_frame::FrameReader, hello};
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    use tokio::time::timeout;

    let data = halcyon_data::GameData::load_with_extra(
        std::env::var_os("HALCYON_DELIVERY").unwrap(),
        Some(std::path::Path::new(
            &std::env::var_os("HALCYON_DATA_EXTRA").unwrap(),
        )),
    )
    .unwrap();
    let rooms = Rooms::new(
        "127.0.0.1".parse().unwrap(),
        "127.0.0.1".into(),
        1,
        Some(std::sync::Arc::new(data)),
    );
    let id = Uuid::new_v4();
    rooms
        .reserve(Reservation {
            talent_levels: vec![],
            match_id: id,
            players: (0..6).map(|_| Uuid::new_v4()).collect(),
            teams: vec![],
            mode: "casual_aral".into(),
            difficulty: None,
        })
        .unwrap();
    let cipher = MatchCipher::new(&id.to_string());
    let shared = rooms.room(id).unwrap();
    {
        let mut room = shared.lock().unwrap();
        room.phase(crate::RoomPhase::Running);
        room.world.use_aral_lane().unwrap();
        room.world.entities[0].type_id = AMAEL;
        room.world.entities[2].type_id = EntityTemplate::Reim.as_raw();
        // Step until the first wave starts while heroes remain safely on their home pads.
        for _ in 0..15 * halcyon_sim::TICK_HZ {
            room.world.step();
        }
    }

    // Attach observer BEFORE spawning the next team-1 minion so the observer knows heroes 1..=6 but NOT the late minion.
    let (addr, grant) = rooms.observe(id, Uuid::new_v4()).unwrap();
    let mut obs_sock = TcpStream::connect(&addr).await.unwrap();
    obs_sock
        .write_all(&hello("127.0.0.1").unwrap())
        .await
        .unwrap();
    let mut ack = [0; 5];
    obs_sock.read_exact(&mut ack).await.unwrap();
    assert_eq!(ack, ACK_FRAME);
    let identity = Message::known(
        crate::Opcode::RegisterPlayerUuid,
        grant.to_string().as_bytes(),
    );
    obs_sock
        .write_all(&cipher.encode(&identity).unwrap())
        .await
        .unwrap();
    let mut obs_reader = FrameReader::new();
    while let Ok(Ok(Some(raw))) =
        timeout(Duration::from_millis(50), obs_reader.next(&mut obs_sock)).await
    {
        let _ = cipher.decode_owned(raw.into()).unwrap();
    }

    let (mut owner_rx, mut ally_rx, mut enemy_rx, minion_id) = {
        let mut room = shared.lock().unwrap();
        for _ in 0..halcyon_sim::TICK_HZ {
            room.world.step();
        }
        let minion_idx = room
            .world
            .entities
            .iter()
            .rposition(|e| e.kind == Kind::Minion && e.team == 1)
            .unwrap();
        let minion_id = room.world.entities[minion_idx].id;

        room.world.entities[0].level = 6;
        room.world.entities[0].skill_points = 1;
        room.world.entities[0].energy = 400.;
        room.world.entities[0].max_energy = 400.;
        room.world.entities[0].position = Position { x: 0., y: 0. };

        // Use ally Reim (seat 2) ChillWinds to root primary hero (id=2) via real simulation.
        room.world.entities[1].max_hp = 2000.;
        room.world.entities[1].hp = 2000.;
        room.world.entities[1].position = Position { x: 3., y: 4. };
        room.world.entities[2].level = 2;
        room.world.entities[2].skill_points = 1;
        room.world.entities[2].energy = 400.;
        room.world.entities[2].max_energy = 400.;
        room.world.entities[2].position = Position { x: 3., y: 3.5 };
        room.world
            .command(3, halcyon_sim::Command::Learn(AbilitySlot::B))
            .unwrap();
        room.world
            .command(
                3,
                halcyon_sim::Command::Cast {
                    slot: AbilitySlot::B,
                    target: CastTarget::SelfCast,
                },
            )
            .unwrap();
        room.world.step();

        // Place secondary visible hero (id=4), hidden hero (id=6), and late minion inside Elbow Drop radius at mid-lane.
        room.world.entities[3].position = Position { x: 3.5, y: 4.5 };
        room.world.entities[5].position = Position { x: 4., y: 4. };
        let minion_idx = room
            .world
            .entities
            .iter()
            .position(|e| e.id == minion_id)
            .unwrap();
        room.world.entities[minion_idx].position = Position { x: 2.5, y: 3.5 };

        let (tx0, rx0) = mpsc::channel(32);
        room.seats[0].peers[0] = Some(Peer {
            generation: 42,
            tx: tx0,
            stalled: watch::channel(false).0,
        });
        let (tx1, rx1) = mpsc::channel(32);
        room.seats[1].peers[1] = Some(Peer {
            generation: 1,
            tx: tx1,
            stalled: watch::channel(false).0,
        });
        let (tx2, rx2) = mpsc::channel(32);
        room.seats[2].peers[1] = Some(Peer {
            generation: 2,
            tx: tx2,
            stalled: watch::channel(false).0,
        });
        room.seats[0].world_actor_written = true;
        for (op, body) in [
            (crate::wire_opcode::RequestSceneSnapshot, [0; 6]),
            (crate::wire_opcode::ConfirmSceneOwner, [0, 0, 5, 220, 1, 0]),
            (crate::wire_opcode::SceneReadyConfirm, [0; 6]),
        ] {
            room.seats[0]
                .preamble
                .observe(&Message::new(op, body), 1500);
        }

        let mut decls = Vec::new();
        for (idx, slot) in [(0usize, 16u8), (1, 17), (3, 18), (5, 19)] {
            let mut m = crate::messages::spawn(&room.world.entities[idx], &room.picks, None);
            m.body[745] = slot;
            decls.push((idx, m));
        }
        let mut minion_decl = crate::messages::spawn(
            room.world
                .entities
                .iter()
                .find(|e| e.id == minion_id)
                .unwrap(),
            &room.picks,
            None,
        );
        minion_decl.body[745] = 22;

        // Team 0 (seats 0 and 2) knows & sees source(1), primary(2), hero(4), minion; hidden hero(6) is EXCLUDED.
        for seat_idx in [0, 2] {
            for (idx, msg) in &decls {
                if *idx != 5 {
                    room.scene_views[seat_idx].declarations(std::slice::from_ref(msg));
                }
            }
            room.scene_views[seat_idx].declarations(std::slice::from_ref(&minion_decl));
        }
        // Team 1 (seat 1) knows & sees everyone including hidden hero(6).
        for (_, msg) in &decls {
            room.scene_views[1].declarations(std::slice::from_ref(msg));
        }
        room.scene_views[1].declarations(std::slice::from_ref(&minion_decl));

        room.visible[0].extend([1, 2, 4, minion_id]);
        room.visible[0].remove(&6);
        room.visible[1].extend([1, 2, 4, 6, minion_id]);

        let learn = Message::known(crate::Opcode::RequestUpgradeAbility, [2, 0, 0, 0, 0, 0]);
        assert_eq!(
            receive(&mut room, 0, 0, learn.clone()),
            [learn, Message::ability_learned(1500, 2)]
        );
        let request = Message::known(crate::Opcode::RequestTargetAbility, [0, 0, 5, 221, 2, 0]);
        receive(&mut room, 0, 0, request);
        assert!(room.seats[0].native_abilities.cast.is_some());

        let mut elbow_hits = Vec::new();
        for _ in 0..8 {
            let ev = room.world.step();
            for e in &ev {
                if let Event::AmaelKick {
                    source: 1,
                    target,
                    kind: halcyon_sim::KickKind::ElbowDrop,
                    ..
                } = *e
                {
                    elbow_hits.push(target);
                }
            }
            publish_before_impacts(&mut room, &ev);
            publish(&mut room, &ev);
        }
        assert_eq!(elbow_hits, vec![2, 4, 6, minion_id]);
        (rx0, rx2, rx1, minion_id)
    };

    let drain = |rx: &mut mpsc::Receiver<Delivery>| -> Vec<Message> {
        let mut out = Vec::new();
        while let Ok(d) = rx.try_recv() {
            out.extend(d.into_records());
        }
        out
    };
    let owner_msgs = drain(&mut owner_rx);
    let ally_msgs = drain(&mut ally_rx);
    let enemy_msgs = drain(&mut enemy_rx);

    // Owner received 0415 + 0403 lunge release.
    assert_eq!(owner_msgs[0], Message::ability_target_used(1500, 1501, 2));
    assert!(owner_msgs[1].forced_move().is_some());

    // Bounce actor references per stream:
    let bounced_actors = |msgs: &[Message]| -> Vec<u32> {
        msgs.iter()
            .filter_map(|m| match m.fields() {
                Ok(halcyon_protocol::Fields::Live(halcyon_protocol::LiveFields::Motion {
                    reference,
                    values,
                    enabled,
                })) if m.is(crate::Opcode::AutoActorBounce)
                    && values == [6., 5.]
                    && enabled
                    && m.body[16..].iter().all(|&b| b == 0) =>
                {
                    Some(reference)
                }
                _ => None,
            })
            .collect()
    };
    let hidden_wire = u32::from(wire_id(6));
    let minion_wire = u32::from(wire_id(minion_id));
    assert_eq!(bounced_actors(&owner_msgs), vec![1501, 1503, minion_wire]);
    assert!(!bounced_actors(&owner_msgs).contains(&hidden_wire));
    assert_eq!(bounced_actors(&ally_msgs), vec![1501, 1503, minion_wire]);
    assert!(!bounced_actors(&ally_msgs).contains(&hidden_wire));
    assert_eq!(
        bounced_actors(&enemy_msgs),
        vec![1501, 1503, hidden_wire, minion_wire]
    );
    assert!(enemy_msgs.iter().any(|m| {
        m.forced_move()
            .is_some_and(|fm| fm.actor == 1500 && fm.mode == halcyon_protocol::ForcedMoveMode::Dash)
    }));

    // Observer receives release + bounces for declared heroes (2, 4, 6), zero frames for undeclared late minion.
    let mut obs_msgs = Vec::new();
    while let Ok(Ok(Some(raw))) =
        timeout(Duration::from_millis(50), obs_reader.next(&mut obs_sock)).await
    {
        obs_msgs.push(cipher.decode_owned(raw.into()).unwrap());
    }
    assert!(obs_msgs.iter().any(|m| {
        m.is(crate::Opcode::PlayTargetAbility)
            && m.body
                .starts_with(&Message::ability_target_used(1500, 1501, 2).body)
            && m.body[9..].iter().all(|&b| b == 0)
    }));
    assert_eq!(bounced_actors(&obs_msgs), vec![1501, 1503, hidden_wire]);
    assert!(!bounced_actors(&obs_msgs).contains(&minion_wire));
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
        (crate::wire_opcode::RequestSceneSnapshot, [0; 6]),
        (crate::wire_opcode::ConfirmSceneOwner, [0, 0, 5, 220, 1, 0]),
        (crate::wire_opcode::SceneReadyConfirm, [0; 6]),
    ] {
        room.seats[0]
            .preamble
            .observe(&Message::new(op, body), 1500);
    }
    room.phase(crate::RoomPhase::Running);
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
        let request = Message::known(
            crate::Opcode::RequestUpgradeAbility,
            [selector, 0, 0, 0, 0, 0],
        );
        assert_eq!(
            room.receive(0, 0, request.clone()).unwrap(),
            [request, Message::ability_learned(1500, selector.into())]
        );
        room.world.tick += 1;
        let packet = if selector < 2 {
            let mut body = [0; 14];
            body[..4].copy_from_slice(&position.x.to_be_bytes());
            body[12] = selector;
            Message::known(crate::Opcode::RequestPointAbility, body)
        } else {
            let mut body = [0; 6];
            body[..4].copy_from_slice(&u32::from(wire_id(ally)).to_be_bytes());
            body[4] = 2;
            Message::known(crate::Opcode::RequestTargetAbility, body)
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
            let Ok(halcyon_protocol::Fields::BuffApplication(effect)) = batch[1].fields() else {
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
async fn winter_spire_publishes_only_the_registered_visible_witness_form() {
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
    room.world.entities[0].type_id = EntityTemplate::Reim.as_raw();
    let (tx, mut rx) = mpsc::channel(8);
    room.seats[1].peers[1] = Some(Peer {
        generation: 1,
        tx,
        stalled: watch::channel(false).0,
    });
    room.visible[0].insert(1);
    let p = Position {
        x: -3.9156992,
        y: 7.9015503,
    };
    let event = Event::Cast {
        source: 1,
        slot: AbilitySlot::A,
        target: CastTarget::Point(p),
    };
    let events = [
        event,
        Event::WinterSpireProjectile {
            source: 1,
            id: 800,
            point: p,
        },
    ];
    publish(&mut room, &events);
    assert!(rx.try_recv().is_err());
    let mut hero = crate::messages::spawn(&room.world.entities[0], &room.picks, None);
    hero.body[745] = 16;
    room.scene_views[1].declarations(&[hero]);
    publish(&mut room, &events);
    assert_eq!(
        rx.try_recv().unwrap().into_records(),
        [Message::ability_point_used(1500, [p.x, 0., p.y], 0)]
    );
    let batch = rx.try_recv().unwrap().into_records();
    let shot = &batch[0];
    assert!(recordable_spell_projectile(shot));
    assert_eq!(&shot.body[0..4], &2299u32.to_be_bytes());
    assert_eq!(&shot.body[4..8], &1043493024u32.to_be_bytes());
    assert_eq!(&shot.body[8..12], &p.x.to_be_bytes());
    assert_eq!(&shot.body[12..16], &[0; 4]);
    assert_eq!(&shot.body[16..20], &p.y.to_be_bytes());
    assert_eq!(&shot.body[20..], &[0, 0, 0, 0, 0, 86, 16, 16]);
    room.visible[0].clear();
    publish(&mut room, &events);
    assert!(rx.try_recv().is_err());
    assert!(NativeCast::point_input(EntityTemplate::Reim.as_raw(), 0).is_none());
    assert!(native_learn_request(EntityTemplate::Reim.as_raw(), 0).is_none());
}

#[tokio::test]
async fn chill_winds_sends_the_witnessed_pair_and_a_registered_one_second_root() {
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
    room.world.entities[0].type_id = EntityTemplate::Reim.as_raw();
    let mut streams = Vec::new();
    let mut caster = crate::messages::spawn(&room.world.entities[0], &room.picks, None);
    caster.body[745] = 16;
    let mut target = crate::messages::spawn(&room.world.entities[2], &room.picks, None);
    target.body[745] = 18;
    for i in 0..3 {
        let (tx, rx) = mpsc::channel(8);
        room.seats[i].peers[1] = Some(Peer {
            generation: 1,
            tx,
            stalled: watch::channel(false).0,
        });
        streams.push(rx);
        room.scene_views[i].declarations(std::slice::from_ref(&caster));
    }
    for v in &mut room.visible {
        v.extend([1, 3]);
    }
    let p = Position { x: 4., y: 2. };
    let angle = Some(-2.4);
    let start = Event::ChillWinds {
        source: 1,
        position: p,
        heading: angle,
    };
    let root = Event::ChillWindsRoot {
        source: 1,
        target: 3,
        instance: 700,
    };
    publish_before_impacts(&mut room, &[start]);
    publish(&mut room, std::slice::from_ref(&root));
    let batch = streams[1].try_recv().unwrap().into_records();
    assert_eq!(batch[0], Message::ability_target_used(1500, u32::MAX, 1));
    let pfx = batch[1].play_pfx_at_location().unwrap();
    assert_eq!(pfx.position, [4., f32::from_bits(0x3d192e1f), 2.]);
    assert_eq!(pfx.unknown_word, (-2.4f32).to_bits());
    assert!(recordable_chill_winds(&batch[1]));
    for rx in &mut streams {
        assert!(rx.try_recv().is_err());
    }
    room.scene_views[1].declarations(&[target]);
    publish(&mut room, &[root]);
    let buff = streams[1].try_recv().unwrap().into_records();
    let halcyon_protocol::Fields::BuffApplication(application) = buff[0].fields().unwrap() else {
        panic!("expected root")
    };
    assert_eq!(
        (
            application.prefix.target,
            application.prefix.source,
            application.prefix.definition,
            application.prefix.duration()
        ),
        (1502, 1500, 654, 1.)
    );
    publish_before_impacts(
        &mut room,
        &[Event::ChillWinds {
            source: 1,
            position: p,
            heading: None,
        }],
    );
    assert_eq!(
        streams[1].try_recv().unwrap().into_records(),
        [Message::ability_target_used(1500, u32::MAX, 1)]
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
            (crate::wire_opcode::RequestSceneSnapshot, [0; 6]),
            (
                crate::wire_opcode::ConfirmSceneOwner,
                [0, 0, 5, 220 + index as u8, 1, 0],
            ),
            (crate::wire_opcode::SceneReadyConfirm, [0; 6]),
        ] {
            seat.preamble.observe(&Message::new(op, body), actor);
        }
        seat.world_actor_written = true;
        witnesses.push(rx);
    }
    room.phase(crate::RoomPhase::Running);
    room.world.entities[0].type_id = IDRIS;
    room.world.entities[0].level = 5;
    room.world.entities[0].skill_points = 2;

    let learn =
        |index| Message::known(crate::Opcode::RequestUpgradeAbility, [index, 0, 0, 0, 0, 0]);
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
        room.receive(
            0,
            0,
            Message::known(crate::Opcode::RequestPointAbility, unsupported_cast)
        )
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

#[tokio::test]
async fn solo_and_live_5v5_basic_target_changed_emit_modify_basic_attack_target() {
    assert_eq!(
        modify_basic_attack_target_message(None),
        Message::known(
            crate::Opcode::ModifyBasicAttackTarget,
            u32::MAX.to_be_bytes(),
        )
    );
    assert_eq!(
        modify_basic_attack_target_message(Some(2)),
        Message::known(
            crate::Opcode::ModifyBasicAttackTarget,
            1501u32.to_be_bytes()
        )
    );

    let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 2, None);
    for solo in [true, false] {
        let match_id = Uuid::new_v4();
        let (players, teams, mode) = if solo {
            (
                vec![Uuid::new_v4()],
                vec![0],
                halcyon_protocol::ce::KnownQueue::SoloBots.as_str().into(),
            )
        } else {
            (
                vec![Uuid::new_v4(), Uuid::new_v4()],
                vec![0, 1],
                halcyon_protocol::ce::KnownQueue::CasualFive.as_str().into(),
            )
        };
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id,
                players,
                teams,
                mode,
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(match_id).unwrap();
        let mut room = shared.lock().unwrap();
        let (tx, mut rx) = mpsc::channel(16);
        let peer = Peer {
            generation: 1,
            tx,
            stalled: watch::channel(false).0,
        };
        if solo {
            room.seats[0].peers[0] = Some(peer);
        } else {
            room.seats[0].peers[1] = Some(peer);
        }
        let hero_id = room.world.entities[0].id;
        let enemy_idx = room
            .world
            .entities
            .iter()
            .position(|entity| entity.team == 1)
            .unwrap();
        let enemy_id = room.world.entities[enemy_idx].id;
        room.phase(crate::RoomPhase::Running);
        room.world.entities[0].position = Position { x: 0.0, y: 0.0 };
        room.world.entities[enemy_idx].position = Position { x: 3.0, y: 0.0 };
        room.visible[0].insert(enemy_id);
        room.world
            .command(hero_id, halcyon_sim::Command::Attack(enemy_id))
            .unwrap();

        let event = [Event::BasicTargetChanged {
            actor: hero_id,
            target: enemy_id,
        }];
        if solo {
            publish(&mut room, &event);
            assert!(rx.try_recv().is_err());
            let mut confirm = [0, 0, 0, 0, 1, 0];
            confirm[..4].copy_from_slice(&u32::from(wire_id(hero_id)).to_be_bytes());
            for (op, body) in [
                (crate::wire_opcode::RequestSceneSnapshot, [0; 6]),
                (crate::wire_opcode::ConfirmSceneOwner, confirm),
                (crate::wire_opcode::SceneReadyConfirm, [0; 6]),
            ] {
                room.seats[0]
                    .preamble
                    .observe(&Message::new(op, body), u32::from(wire_id(hero_id)));
            }
            assert!(room.seats[0].preamble.confirmed());
            room.seats[0].world_actor_written = true;
            let enemy_decl = room
                .allocate_entity_declaration(enemy_idx, 0, true, false, false)
                .unwrap();
            room.scene_views[0].declarations(&[enemy_decl]);
            assert!(room.scene_views[0].knows(enemy_id));
        }

        publish(&mut room, &event);
        assert_eq!(
            rx.try_recv().unwrap().into_records(),
            [modify_basic_attack_target_message(Some(enemy_id))]
        );
        room.visible[0].remove(&enemy_id);
        publish(&mut room, &event);
        assert!(rx.try_recv().is_err());
    }
}

fn drain(receiver: &mut mpsc::Receiver<Delivery>) -> Vec<Message> {
    let mut replies = vec![];
    while let Ok(batch) = receiver.try_recv() {
        let records = batch.into_records();
        let release_batch = records
            .iter()
            .any(|m| m.is(crate::Opcode::PlayPointAbility));
        replies.extend(records.into_iter().filter(|m| {
            matches!(
                m.opcode,
                crate::wire_opcode::ModifyBasicAttackTarget
                    | crate::wire_opcode::StopActor
                    | crate::wire_opcode::PlayPointAbility
                    | crate::wire_opcode::ApplyBuffCompact
            ) || (release_batch && m.is(crate::Opcode::MoveToCorrection))
        }));
    }
    replies
}
