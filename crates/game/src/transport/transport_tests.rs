use super::*;
use crate::{Peer, Room};
use halcyon_sim::{Event, Position};
use halcyon_types::{MatchHost, Reservation};
use std::sync::atomic::AtomicBool;
use tokio::io::AsyncReadExt;

async fn identified(addr: &str, identity: Uuid, cipher: &MatchCipher) -> TcpStream {
    let mut sock = TcpStream::connect(addr).await.unwrap();
    let port = sock.peer_addr().unwrap().port();
    sock.write_all(&halcyon_protocol::hello_with_port("127.0.0.1", port).unwrap())
        .await
        .unwrap();
    let mut ack = [0; 5];
    sock.read_exact(&mut ack).await.unwrap();
    assert_eq!(ack, ACK_FRAME);
    sock.write_all(
        &cipher
            .encode(&Message::known(
                crate::Opcode::RegisterPlayerUuid,
                identity.to_string().as_bytes(),
            ))
            .unwrap(),
    )
    .await
    .unwrap();
    // Reading a complete initial frame confirms that this socket is registered.
    let _ = frame(&mut sock).await.unwrap();
    sock
}

async fn final_frames(mut stream: TcpStream, cipher: &MatchCipher) -> Vec<Message> {
    let mut received = Vec::new();
    let mut bytes = Vec::new();
    timeout(Duration::from_secs(2), stream.read_to_end(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    let mut slice = bytes.as_slice();
    while let Some(body) = halcyon_protocol::read_frame(&mut slice).unwrap() {
        received.push(cipher.decode(&body).unwrap());
    }
    received
}

#[tokio::test]
async fn grants_no_grace_on_forced_close_and_bounds_a_stalled_natural_flush() {
    let mut tasks = JoinSet::<()>::new();
    tasks.spawn(std::future::pending());
    timeout(Duration::from_millis(250), close_clients(&mut tasks, false))
        .await
        .unwrap();
    assert!(tasks.is_empty());
    tasks.spawn(std::future::pending());
    timeout(Duration::from_secs(3), close_clients(&mut tasks, true))
        .await
        .unwrap();
    assert!(tasks.is_empty());

    let (tx, _rx) = mpsc::channel(1);
    let (stalled, slow) = watch::channel(false);
    let peer = Peer {
        generation: 1,
        tx,
        stalled,
    };
    peer.send(Delivery::Private(vec![]));
    peer.send(Delivery::Private(vec![]));
    assert!(*slow.borrow());
}

#[tokio::test]
async fn admin_termination_writes_one_targeted_native_exit_before_eof() {
    let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
    let id = Uuid::new_v4();
    let players = [Uuid::new_v4(), Uuid::new_v4()];
    let hosted = rooms
        .reserve(Reservation {
            talent_levels: Vec::new(),
            match_id: id,
            players: players.to_vec(),
            teams: vec![0, 1],
            mode: "5v5_pvp_casual".into(),
            difficulty: None,
        })
        .unwrap();
    let cipher = MatchCipher::new(&id.to_string());
    let a = identified(&hosted.address, players[0], &cipher).await;
    let b = identified(&hosted.address, players[1], &cipher).await;
    rooms.terminate(id, "operator", "maintenance").unwrap();
    let (a, b) = tokio::join!(final_frames(a, &cipher), final_frames(b, &cipher));
    for (stream, id) in [a, b].iter().zip(players) {
        let mut kicks = stream.iter().filter(|m| m.is(crate::Opcode::KickPlayer));
        let m = kicks.next().unwrap();
        assert!(kicks.next().is_none());
        assert_eq!(stream.last(), Some(m));
        let Fields::Live(halcyon_protocol::LiveFields::KickPlayer {
            player_uuid,
            disconnect_code,
        }) = m.fields().unwrap()
        else {
            panic!("expected native exit")
        };
        assert_eq!(&player_uuid[..36], id.to_string().as_bytes());
        assert_eq!(&player_uuid[36..], &[0; 28]);
        assert_eq!(disconnect_code, 0);
    }
}

#[tokio::test]
async fn natural_crystal_death_is_written_before_player_and_observer_eof() {
    let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
    let player = Uuid::new_v4();
    let id = Uuid::new_v4();
    let hosted = rooms
        .reserve(Reservation {
            talent_levels: Vec::new(),
            match_id: id,
            players: vec![player],
            teams: vec![0],
            mode: "5v5_pvp_casual".into(),
            difficulty: None,
        })
        .unwrap();
    let room = rooms.room(id).unwrap();
    let cipher = MatchCipher::new(&id.to_string());
    let target = {
        let mut r = room.lock().unwrap();
        let crystal = r
            .world
            .entities
            .iter()
            .find(|e| e.kind == halcyon_sim::Kind::Crystal && e.team == 1)
            .unwrap();
        let (id, pos) = (crystal.id, crystal.position);
        r.world.entities[0].position = pos;
        r.seats[0].locked = true;
        r.phase(crate::RoomPhase::Running);
        id
    };
    let data = identified(&hosted.address, player, &cipher).await;
    let observer = rooms.spectate(id, Uuid::new_v4()).unwrap();
    let view = identified(&observer.address, observer.spectator_id, &cipher).await;
    let withdrawn = rooms.spectate(id, Uuid::new_v4()).unwrap();
    let revoked_view = identified(&withdrawn.address, withdrawn.spectator_id, &cipher).await;
    {
        let mut r = room.lock().unwrap();
        r.world.entities.find_mut(|e| e.id == target).unwrap().hp = 0.1;
        r.world.tick = 15;
        r.world
            .command(1, halcyon_sim::Command::Attack(target))
            .unwrap();
        for _ in 0..5 {
            r.tick();
        }
        let reason = r.reason;
        drop(r);
        assert_eq!(reason, Some(crate::RoomEndReason::Completed));
    }
    // The final batch is pending for this grant, but revocation takes priority.
    rooms.stop_spectating(id, withdrawn.spectator_id).unwrap();
    // The current-thread runtime cannot poll either writer between tick()'s
    // fanout and stop signal. The final snapshot is the last queued frame.
    let (player, observer) = tokio::join!(final_frames(data, &cipher), final_frames(view, &cipher));
    let revoked = final_frames(revoked_view, &cipher).await;
    assert!(!revoked.iter().any(|m| matches!(
        m.opcode,
        crate::wire_opcode::ActorDeath
            | crate::wire_opcode::PostMatchStats
            | crate::wire_opcode::EndMatch
    )));
    for stream in [&player, &observer] {
        let opcodes: Vec<_> = stream.iter().map(|m| m.opcode).collect();
        let death = opcodes
            .iter()
            .position(|&op| op == crate::wire_opcode::ActorDeath)
            .unwrap();
        assert!(opcodes[..death].contains(&crate::wire_opcode::HealthImpact));
        assert!(opcodes[death + 1..].contains(&crate::wire_opcode::DraftRosterFull));
        assert!(opcodes.ends_with(&[
            crate::wire_opcode::PostMatchStats,
            crate::wire_opcode::EndMatch
        ]));
    }
    let player_report = &player[player.len() - 2];
    assert_eq!(player_report, &observer[observer.len() - 2]);
    let halcyon_protocol::Fields::MatchStatistics(stats) = player_report.fields().unwrap() else {
        panic!("expected match statistics")
    };
    assert_eq!(stats.slot(0).unwrap().player, 1500);
    assert!(stats.slot(0).unwrap().damage_to_structures > 0.0);
    assert_eq!(stats.slot(10).unwrap().player, u32::MAX);
}

#[tokio::test]
async fn bot_inventory_is_stable_across_visibility_and_reconnect() {
    use halcyon_sim::BasicItem;
    let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
    let id = Uuid::new_v4();
    let players = [Uuid::new_v4(), Uuid::new_v4()];
    rooms
        .reserve(Reservation {
            talent_levels: Vec::new(),
            match_id: id,
            players: players.to_vec(),
            teams: vec![0, 1],
            mode: "5v5".into(),
            difficulty: None,
        })
        .unwrap();
    let shared = rooms.room(id).unwrap();
    let mut r = shared.lock().unwrap();
    r.phase(crate::RoomPhase::Running);
    r.world.tick = 1;
    for s in &mut r.seats {
        s.locked = true;
    }
    let attach = |r: &mut Room, id| {
        let (tx, rx) = mpsc::channel(16);
        let (_, ch, _, batch) = r.attach(id, tx, watch::channel(false).0).unwrap();
        assert_eq!(ch, 1);
        (rx, batch)
    };
    let (mut ally, _) = attach(&mut r, players[0]);
    let (mut enemy, _) = attach(&mut r, players[1]);
    let i = r
        .world
        .entities
        .iter()
        .position(|e| e.kind == halcyon_sim::Kind::Hero && e.bot && e.team == 0)
        .unwrap();
    let id = r.world.entities[i].id;
    let actor = u32::from(wire_id(id));
    let grants = |messages: &[Message]| {
        messages
            .iter()
            .filter_map(|m| match m.fields().unwrap() {
                Fields::ItemGrant {
                    actor: a, instance, ..
                } if a == actor => Some(instance),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    r.world.entities[i].gold = 2000;
    let first = r.world.purchase(id, BasicItem::WeaponBlade).unwrap();
    r.replicate(&[Event::Purchase(first.clone())]);
    let batch = ally.try_recv().unwrap().into_records();
    assert_eq!(
        batch.iter().map(|m| m.opcode).collect::<Vec<_>>(),
        [
            crate::wire_opcode::ModifyActorResource,
            crate::wire_opcode::ItemGrant,
            crate::wire_opcode::ModifyActorAttribute
        ]
    );
    assert_eq!(grants(&batch), [first.granted.instance]);
    assert!(enemy.try_recv().is_err());
    r.visible[0].remove(&id);
    let second = r.world.purchase(id, BasicItem::LightArmor).unwrap();
    let events = [
        Event::Purchase(second.clone()),
        Event::GoldAward { id, amount: 25 },
    ];
    r.replicate(&events);
    let batch = ally.try_recv().unwrap().into_records();
    let expected = [first.granted.instance, second.granted.instance];
    assert_eq!(grants(&batch), expected);
    assert_eq!(
        batch
            .iter()
            .filter_map(|m| match m.fields().unwrap() {
                Fields::EntityStat { stat: 6, value, .. } => Some(value),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [-300., 25.]
    );
    assert!(enemy.try_recv().is_err());
    assert!(grants(&r.spawn_view(Some(1))).is_empty());
    assert_eq!(grants(&r.spawn_view(None)), expected);
    let generation = r.seats[0].peers[1].as_ref().unwrap().generation;
    r.detach(0, 1, generation);
    let (_, reconnect) = attach(&mut r, players[0]);
    assert_eq!(grants(&reconnect), expected);
    let spawn = batch
        .iter()
        .find(|m| m.is(crate::Opcode::HeroSpawn))
        .unwrap();
    assert_eq!(
        f32::from_be_bytes(spawn.body[190..194].try_into().unwrap()),
        r.world.entities[i].armor()
    );
}

#[tokio::test]
async fn teammates_share_ciphertext_while_opponents_and_reconnects_keep_their_view() {
    let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
    let id = Uuid::new_v4();
    let players = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
    rooms
        .reserve(Reservation {
            talent_levels: Vec::new(),
            match_id: id,
            players: players.to_vec(),
            teams: vec![0, 0, 1],
            mode: "5v5".into(),
            difficulty: None,
        })
        .unwrap();
    let shared = rooms.room(id).unwrap();
    let mut room = shared.lock().unwrap();
    room.phase(crate::RoomPhase::Running);
    room.world.tick = 1;
    for idx in 0..room.world.entities.len() {
        let entity = &mut room.world.entities[idx];
        entity.position = Position {
            x: if entity.team == 0 { -80. } else { 80. },
            y: 0.,
        };
    }
    let position = room.world.entities[0].position;
    for seat in &mut room.seats {
        seat.locked = true;
    }
    let attach = |room: &mut Room, id| {
        let (tx, rx) = mpsc::channel(16);
        let (_, channel, generation, _) = room.attach(id, tx, watch::channel(false).0).unwrap();
        assert_eq!(channel, 1);
        (rx, generation)
    };
    let (mut a, generation) = attach(&mut room, players[0]);
    let (mut b, _) = attach(&mut room, players[1]);
    let (mut other, _) = attach(&mut room, players[2]);
    let movement = [Event::Move { id: 1, position }];
    room.replicate(&movement);
    let first = a.try_recv().unwrap();
    let second = b.try_recv().unwrap();
    let cipher = MatchCipher::new(&id.to_string());
    let (mut scratch_a, mut scratch_b) = (Vec::new(), Vec::new());
    let bytes_a = first.bytes(&cipher, &mut scratch_a).unwrap();
    let bytes_b = second.bytes(&cipher, &mut scratch_b).unwrap();
    assert!(std::ptr::eq(bytes_a, bytes_b));
    assert_eq!(
        bytes_a,
        cipher
            .encode(&Message::position(wire_id(1), position.x, position.y))
            .unwrap()
    );
    assert!(other.try_recv().is_err());

    room.detach(0, 1, generation);
    let (mut reconnect, _) = attach(&mut room, players[0]);
    room.replicate(&movement);
    assert_eq!(
        reconnect.try_recv().unwrap().into_records(),
        second.into_records()
    );
    assert!(other.try_recv().is_err());
}

#[tokio::test]
async fn stalled_control_peer_allows_authenticated_replacement_while_healthy_peer_rejects() {
    let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
    let id = Uuid::new_v4();
    let player = Uuid::new_v4();
    let hosted = rooms
        .reserve(Reservation {
            talent_levels: Vec::new(),
            match_id: id,
            players: vec![player],
            teams: vec![0],
            mode: "5v5_bots_solo".into(),
            difficulty: None,
        })
        .unwrap();
    let shared = rooms.room(id).unwrap();
    let cipher = MatchCipher::new(&id.to_string());

    let first = identified(&hosted.address, player, &cipher).await;
    // Healthy ch == 0 socket still rejects concurrent duplicate attaches.
    let mut rejected = TcpStream::connect(&hosted.address).await.unwrap();
    let port = rejected.peer_addr().unwrap().port();
    rejected
        .write_all(&halcyon_protocol::hello_with_port("127.0.0.1", port).unwrap())
        .await
        .unwrap();
    let mut ack = [0; 5];
    rejected.read_exact(&mut ack).await.unwrap();
    assert_eq!(ack, ACK_FRAME);
    rejected
        .write_all(
            &cipher
                .encode(&Message::known(
                    crate::Opcode::RegisterPlayerUuid,
                    player.to_string().as_bytes(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        timeout(Duration::from_secs(2), rejected.read(&mut [0; 1]))
            .await
            .unwrap()
            .unwrap(),
        0
    );

    let first_gen = {
        let room = shared.lock().unwrap();
        let peer = room.seats[0].peers[0].as_ref().unwrap();
        peer.stalled.send_replace(true);
        peer.generation
    };
    let replacement = identified(&hosted.address, player, &cipher).await;
    let replacement_gen = shared.lock().unwrap().seats[0].peers[0]
        .as_ref()
        .unwrap()
        .generation;
    assert!(replacement_gen > first_gen);
    // Late detach from the superseded generation must not drop the replacement peer.
    shared.lock().unwrap().detach(0, 0, first_gen);
    assert_eq!(
        shared.lock().unwrap().seats[0].peers[0]
            .as_ref()
            .unwrap()
            .generation,
        replacement_gen
    );
    drop(first);
    drop(replacement);
}

#[tokio::test]
async fn finish_handler_commits_before_client_tcp_eof() {
    let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
    let committed = Arc::new(AtomicBool::new(false));
    let flag = committed.clone();
    rooms.set_finish_handler(Arc::new(move |_id, _result| {
        std::thread::sleep(Duration::from_millis(80));
        flag.store(true, Ordering::Release);
    }));
    let player = Uuid::new_v4();
    let id = Uuid::new_v4();
    let hosted = rooms
        .reserve(Reservation {
            talent_levels: Vec::new(),
            match_id: id,
            players: vec![player],
            teams: vec![0],
            mode: "5v5_pvp_casual".into(),
            difficulty: None,
        })
        .unwrap();
    let room = rooms.room(id).unwrap();
    let cipher = MatchCipher::new(&id.to_string());
    let target = {
        let mut r = room.lock().unwrap();
        let crystal = r
            .world
            .entities
            .iter()
            .find(|e| e.kind == halcyon_sim::Kind::Crystal && e.team == 1)
            .unwrap();
        let (cid, pos) = (crystal.id, crystal.position);
        r.world.entities[0].position = pos;
        r.seats[0].locked = true;
        r.phase(crate::RoomPhase::Running);
        cid
    };
    let data = identified(&hosted.address, player, &cipher).await;
    {
        let mut r = room.lock().unwrap();
        r.world.entities.find_mut(|e| e.id == target).unwrap().hp = 0.1;
        r.world.tick = 15;
        r.world
            .command(1, halcyon_sim::Command::Attack(target))
            .unwrap();
        for _ in 0..5 {
            r.tick();
        }
        assert_eq!(r.reason, Some(crate::RoomEndReason::Completed));
    }
    let messages = final_frames(data, &cipher).await;
    assert!(
        committed.load(Ordering::Acquire),
        "client observed TCP EOF before finish handler committed"
    );
    let opcodes: Vec<_> = messages.iter().map(|m| m.opcode).collect();
    assert!(opcodes.ends_with(&[
        crate::wire_opcode::PostMatchStats,
        crate::wire_opcode::EndMatch
    ]));
}
