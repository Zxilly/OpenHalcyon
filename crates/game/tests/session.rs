use halcyon_game::{Rooms, TerminateError};
use halcyon_platform::{MatchHost, Reservation};
use halcyon_protocol::{ACK_FRAME, MatchCipher, Message, hello_with_port};
use std::{net::Ipv4Addr, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};
use uuid::Uuid;

async fn login(addr: &str, player: Uuid, cipher: &MatchCipher) -> TcpStream {
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(&hello_with_port("127.0.0.1", s.peer_addr().unwrap().port()).unwrap())
        .await
        .unwrap();
    let mut ack = [0; 5];
    s.read_exact(&mut ack).await.unwrap();
    assert_eq!(ack, ACK_FRAME);
    let mut id = vec![0; 70];
    id[..36].copy_from_slice(player.to_string().as_bytes());
    s.write_all(
        &cipher
            .encode(&Message::known(halcyon_protocol::Opcode::PlayerUuid, id))
            .unwrap(),
    )
    .await
    .unwrap();
    s
}
async fn next_matching(
    s: &mut TcpStream,
    c: &MatchCipher,
    accept: impl Fn(u16) -> bool,
) -> Message {
    timeout(Duration::from_secs(2), async {
        loop {
            let len = s.read_u16().await.unwrap();
            let mut b = vec![0; len as usize];
            s.read_exact(&mut b).await.unwrap();
            let m = c.decode(&b).unwrap();
            if accept(m.opcode) {
                return m;
            }
        }
    })
    .await
    .unwrap()
}
async fn until(s: &mut TcpStream, c: &MatchCipher, op: u16) -> Message {
    next_matching(s, c, |actual| actual == op).await
}

#[tokio::test]
async fn reservation_and_two_authenticated_sockets() {
    let rooms = Rooms::new(Ipv4Addr::LOCALHOST.into(), "127.0.0.1".into(), 1, None);
    let replay_temp = tempfile::tempdir().unwrap();
    let replay_root = replay_temp.path().join("private");
    rooms.set_replay_directory(&replay_root).unwrap();
    let id = Uuid::new_v4();
    let player = Uuid::new_v4();
    let req = Reservation {
        talent_levels: Vec::new(),
        match_id: id,
        players: vec![player],
        teams: vec![],
        mode: "5v5_bots_solo".into(),
        difficulty: None,
    };
    let addr = rooms.reserve(req.clone()).unwrap().address;
    assert_eq!(rooms.reserve(req).unwrap().address, addr);
    let c = MatchCipher::new(&id.to_string());
    let mut rejected = login(&addr, Uuid::new_v4(), &c).await;
    let mut byte = [0];
    assert_eq!(
        timeout(Duration::from_secs(1), rejected.read(&mut byte))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    let mut control = login(&addr, player, &c).await;
    until(&mut control, &c, halcyon_protocol::wire_opcode::HeroCatalog).await;
    let roster = until(&mut control, &c, halcyon_protocol::wire_opcode::Snapshot5V5).await;
    assert_eq!(&roster.body[105..141], player.to_string().as_bytes());
    for opcode in [
        halcyon_protocol::wire_opcode::SetupA,
        halcyon_protocol::wire_opcode::SetupC,
    ] {
        control
            .write_all(&c.encode(&Message::new(opcode, [0; 6])).unwrap())
            .await
            .unwrap();
        until(&mut control, &c, opcode).await;
    }
    let mut pick = vec![0; 14];
    pick[..4].copy_from_slice(&243u32.to_be_bytes());
    pick[4..8].copy_from_slice(&0xf240ebfeu32.to_be_bytes());
    control
        .write_all(
            &c.encode(&Message::known(
                halcyon_protocol::Opcode::HeroSelection,
                pick.clone(),
            ))
            .unwrap(),
        )
        .await
        .unwrap();
    let updated = until(&mut control, &c, halcyon_protocol::wire_opcode::Snapshot5V5).await;
    let halcyon_protocol::Fields::DraftRoster(typed) = updated.fields().unwrap() else {
        panic!("expected draft roster")
    };
    let slot = typed.slot(0).unwrap();
    assert_eq!(slot.entity, 1500);
    assert_eq!(slot.display_name, b"Guest");
    assert!(slot.selected);
    assert!(!slot.locked);
    assert!(typed.slot(16).is_none());
    assert_eq!(&updated.body[17..19], &243u16.to_be_bytes());
    assert_eq!(&updated.body[21..25], &0xf240ebfeu32.to_be_bytes());
    until(
        &mut control,
        &c,
        halcyon_protocol::wire_opcode::HeroSelection,
    )
    .await;
    let sync = until(&mut control, &c, halcyon_protocol::wire_opcode::TickSyncC).await;
    assert_eq!(sync.body, [0, 0, 0, 0, 8, 0]);
    let mut repeat = c
        .encode(&Message::known(
            halcyon_protocol::Opcode::HeroSelection,
            pick,
        ))
        .unwrap();
    repeat.extend(
        c.encode(&Message::known(halcyon_protocol::Opcode::SetupC, [0; 6]))
            .unwrap(),
    );
    control.write_all(&repeat).await.unwrap();
    until(
        &mut control,
        &c,
        halcyon_protocol::wire_opcode::HeroSelection,
    )
    .await;
    let next = next_matching(&mut control, &c, |opcode| {
        opcode != halcyon_protocol::wire_opcode::Snapshot5V5
    })
    .await;
    assert_eq!(next.opcode, halcyon_protocol::wire_opcode::SetupC);
    control
        .write_all(
            &c.encode(&Message::known(
                halcyon_protocol::Opcode::LockSelection,
                [0; 6],
            ))
            .unwrap(),
        )
        .await
        .unwrap();
    until(
        &mut control,
        &c,
        halcyon_protocol::wire_opcode::LockSelection,
    )
    .await;
    let mut game = login(&addr, player, &c).await;
    let info = until(&mut game, &c, halcyon_protocol::wire_opcode::PlayerInfo).await;
    assert_eq!(&info.body[160..164], &1500u32.to_be_bytes());
    let clock = until(
        &mut game,
        &c,
        halcyon_protocol::wire_opcode::OpeningWorldClock,
    )
    .await;
    assert_eq!(&clock.body[..4], &0_f32.to_be_bytes());
    let spawn = until(&mut game, &c, halcyon_protocol::wire_opcode::EntitySpawn).await;
    assert_eq!(&spawn.body[4..8], &0xf240ebfeu32.to_be_bytes());

    let info = rooms.detail(id).unwrap();
    assert_eq!(info["phase"], "running");
    assert_eq!(info["participants"][0]["controlConnected"], true);
    assert_eq!(info["participants"][0]["gameConnected"], true);
    game.write_all(&c.encode(&Message::movement(5., 2.)).unwrap())
        .await
        .unwrap();
    until(&mut game, &c, halcyon_protocol::wire_opcode::Position).await;
    drop(game);
    timeout(Duration::from_secs(2), async {
        while rooms.detail_typed(id).unwrap().participants.unwrap()[0].game_connected {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let mut game = login(&addr, player, &c).await;
    let clock = until(
        &mut game,
        &c,
        halcyon_protocol::wire_opcode::OpeningWorldClock,
    )
    .await;
    let seconds = f32::from_be_bytes(clock.body[..4].try_into().unwrap());
    assert!(
        seconds >= 1.0 / halcyon_sim::TICK_HZ as f32,
        "fresh data socket received {seconds}s after movement"
    );
    until(&mut game, &c, halcyon_protocol::wire_opcode::EntitySpawn).await;
    let viewer = Uuid::new_v4();
    let grant = rooms.spectate(id, viewer).unwrap();
    assert_eq!(
        rooms.spectate(id, viewer).unwrap().spectator_id,
        grant.spectator_id
    );
    assert_ne!(grant.address, addr);
    let mut watch = login(&grant.address, grant.spectator_id, &c).await;
    let roster = until(&mut watch, &c, halcyon_protocol::wire_opcode::Snapshot5V5).await;
    assert!(roster.body[105..141].iter().all(|&v| v == 0));
    watch
        .write_all(&c.encode(&Message::movement(3., 4.)).unwrap())
        .await
        .unwrap();
    timeout(Duration::from_secs(2), async {
        let mut bytes = [0; 4096];
        while watch.read(&mut bytes).await.unwrap_or(0) != 0 {}
    })
    .await
    .unwrap();
    let mut watch = login(&grant.address, grant.spectator_id, &c).await;
    until(&mut watch, &c, halcyon_protocol::wire_opcode::Snapshot5V5).await;
    rooms.stop_spectating(id, grant.spectator_id).unwrap();
    timeout(Duration::from_secs(2), async {
        let mut bytes = [0; 4096];
        while watch.read(&mut bytes).await.unwrap_or(0) != 0 {}
    })
    .await
    .unwrap();
    let (pending, record) = rooms.terminate(id, "test", "finished").unwrap();
    assert!(!pending);
    assert_eq!(
        rooms.terminate(id, "test", "different"),
        Err(TerminateError::Conflict)
    );
    timeout(Duration::from_secs(2), async {
        loop {
            if rooms.detail(id).unwrap()["phase"] == "ended" {
                break;
            }
            tokio::task::yield_now().await
        }
    })
    .await
    .unwrap();
    assert_eq!(
        rooms.terminate(id, "test", "finished").unwrap(),
        (true, record)
    );
    assert!(TcpStream::connect(addr).await.is_err());
    assert!(rooms.outbound_bytes() > 1000);
    // Finishing the room deliberately does not wait for the independent disk writer.
    let (recorded, ops) = timeout(Duration::from_secs(4), async {
        loop {
            let Ok(attempts) = std::fs::read_dir(replay_root.join(id.to_string())) else {
                tokio::time::sleep(Duration::from_millis(10)).await;
                continue;
            };
            for attempt in attempts {
                let attempt = attempt.unwrap();
                let path = attempt.path().join("00000000.vgr.partial");
                let Ok(bytes) = std::fs::read(path) else {
                    continue;
                };
                let mut reader = halcyon_protocol::ReplayReader::new(bytes.as_slice());
                let mut ops = Vec::new();
                loop {
                    match reader.next_record() {
                        Ok(Some(record)) => ops.push(record.message.opcode),
                        Ok(None) if ops.contains(&halcyon_protocol::wire_opcode::Position) => {
                            return (bytes, ops);
                        }
                        _ => break,
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(ops.contains(&halcyon_protocol::wire_opcode::EntitySpawn));
    assert!(ops.contains(&halcyon_protocol::wire_opcode::Position));
    assert!(!ops.contains(&halcyon_protocol::wire_opcode::PlayerUuid));
    assert!(!ops.contains(&halcyon_protocol::wire_opcode::HeroCatalog));
    assert!(
        !recorded
            .windows(36)
            .any(|b| b == player.to_string().as_bytes())
    );
    assert_eq!(rooms.events(id, 0, 1).unwrap()["hasMore"], true);
}

#[tokio::test]
async fn admission_precedes_authentication() {
    let rooms = Rooms::new(Ipv4Addr::LOCALHOST.into(), "127.0.0.1".into(), 1, None);
    let player = Uuid::new_v4();
    let id = Uuid::new_v4();
    let req = Reservation {
        talent_levels: Vec::new(),
        match_id: id,
        players: vec![player],
        teams: vec![],
        mode: "solo".into(),
        difficulty: None,
    };
    let addr = rooms.prepare(req.clone()).unwrap().address;
    let mut early = TcpStream::connect(&addr).await.unwrap();
    rooms.discard(id).unwrap();
    assert!(rooms.detail(id).is_none());
    let mut b = [0; 1];
    let _ = timeout(Duration::from_secs(1), early.read(&mut b))
        .await
        .unwrap();
    let addr = rooms.prepare(req).unwrap().address;
    let mut s = TcpStream::connect(&addr).await.unwrap();
    s.write_all(&hello_with_port("127.0.0.1", s.peer_addr().unwrap().port()).unwrap())
        .await
        .unwrap();
    let mut ack = [0; 5];
    assert!(
        timeout(Duration::from_millis(60), s.read(&mut ack))
            .await
            .is_err()
    );
    rooms.activate(id).unwrap();
    assert!(rooms.discard(id).is_err());
    timeout(Duration::from_secs(1), s.read_exact(&mut ack))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ack, ACK_FRAME);
    let c = MatchCipher::new(&id.to_string());
    let mut body = vec![0; 70];
    body[..36].copy_from_slice(player.to_string().as_bytes());
    s.write_all(
        &c.encode(&Message::known(halcyon_protocol::Opcode::PlayerUuid, body))
            .unwrap(),
    )
    .await
    .unwrap();
    until(&mut s, &c, halcyon_protocol::wire_opcode::GameSetup).await;
}
