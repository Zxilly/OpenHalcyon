use crate::native_rpc;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use halcyon_ce_platform::{Config as CeConfig, LobbyState, Platform, RpcMethod};
use halcyon_game::{RoomEndReason, Rooms};
use halcyon_platform::{AppState, PlatformConfig};
use halcyon_protocol::{
    ACK_FRAME, Fields, MatchCipher, Message, Opcode, hello_with_port, wire_opcode,
};
use native_rpc::{RpcEnvelope, android_guest};
use serde::Serialize;
use serde_json::Value;
use std::{net::Ipv4Addr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::{sleep, timeout},
};
use tower::ServiceExt;
use uuid::Uuid;

#[derive(Serialize)]
struct SoloLobbyJoin<'a> {
    lobby: &'a str,
    difficulty: &'a str,
}

async fn rpc_call(router: &Router, method: &str, args: impl Serialize) -> Value {
    let req = Request::post(format!("/JSONRpc/{method}"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(
            serde_json::to_string(&RpcEnvelope {
                method,
                params: args,
            })
            .unwrap(),
        ))
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    serde_json::from_slice(&to_bytes(res.into_body(), 1 << 20).await.unwrap()).unwrap()
}

async fn rpc(router: &Router, method: &str, args: impl Serialize) -> Value {
    let payload = rpc_call(router, method, args).await;
    assert_eq!(payload["code"], 0, "{method}: {payload}");
    payload
}

async fn login_game(addr: &str, player_id: Uuid, cipher: &MatchCipher) -> TcpStream {
    let mut sock = TcpStream::connect(addr).await.unwrap();
    sock.write_all(&hello_with_port("127.0.0.1", sock.peer_addr().unwrap().port()).unwrap())
        .await
        .unwrap();
    let mut ack = [0; 5];
    timeout(Duration::from_secs(4), sock.read_exact(&mut ack))
        .await
        .expect("game hello ACK timed out")
        .expect("game socket closed before ACK");
    assert_eq!(ack, ACK_FRAME);
    let mut body = [0; 70];
    body[..36].copy_from_slice(player_id.to_string().as_bytes());
    sock.write_all(
        &cipher
            .encode(&Message::new(wire_opcode::RegisterPlayerUuid, body))
            .unwrap(),
    )
    .await
    .unwrap();
    sock
}

async fn send_frame(sock: &mut TcpStream, cipher: &MatchCipher, message: &Message) {
    sock.write_all(&cipher.encode(message).unwrap())
        .await
        .unwrap();
}

async fn next_frame(sock: &mut TcpStream, cipher: &MatchCipher) -> Message {
    timeout(Duration::from_secs(4), async {
        let len = sock.read_u16().await.unwrap();
        let mut buf = vec![0; usize::from(len)];
        sock.read_exact(&mut buf).await.unwrap();
        cipher.decode_owned(buf).unwrap()
    })
    .await
    .unwrap()
}

async fn collect_until_match(
    sock: &mut TcpStream,
    cipher: &MatchCipher,
    label: &str,
    mut matches: impl FnMut(&Message) -> bool,
) -> Vec<Message> {
    timeout(Duration::from_secs(4), async {
        let mut out = Vec::new();
        loop {
            let msg = next_frame(sock, cipher).await;
            let done = matches(&msg);
            out.push(msg);
            if done {
                return out;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {label}"))
}

async fn collect_until(sock: &mut TcpStream, cipher: &MatchCipher, op: u16) -> Vec<Message> {
    collect_until_match(sock, cipher, &format!("{op:#06x}"), |m| m.opcode == op).await
}

fn state_upd_a(x: f32, y: f32) -> Message {
    let mut body = [0u8; 14];
    body[..4].copy_from_slice(&x.to_be_bytes());
    body[4..8].copy_from_slice(&y.to_be_bytes());
    Message::known(Opcode::RequestMovePoint, body)
}

#[tokio::test]
#[ignore = "slow soak; run with cargo nextest run --run-ignored all"]
async fn native_solo_single_socket_full_lifecycle_reconnect_and_post_match() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("solo-e2e.sqlite");
    let rooms = Rooms::new(Ipv4Addr::LOCALHOST.into(), "127.0.0.1".into(), 2, None);
    let app = AppState::open(
        PlatformConfig {
            database_path: db_path.clone(),
            public_address: "localhost:8443".into(),
            seats_per_match: 10,
        },
        Arc::new(rooms.clone()),
    )
    .unwrap();

    let completion = app.clone();
    rooms.set_finish_handler(Arc::new(move |id, result| {
        // Inject blocking work so any settlement-vs-EOF race would be caught immediately.
        std::thread::sleep(Duration::from_millis(80));
        let reason = result["reason"].as_str();
        let aborted = [RoomEndReason::AdminTerminated, RoomEndReason::Error]
            .into_iter()
            .any(|r| reason == Some(r.as_str()));
        let outcome = if aborted {
            completion.abort_match(id)
        } else {
            completion.finish_match(id, result)
        };
        outcome.expect("persist Solo match outcome");
    }));

    let platform = Platform::new(
        app.clone(),
        CeConfig {
            authority: "localhost:8443".into(),
            notify_authority: "localhost:2112".into(),
        },
        &[55; 32],
        &db_path,
        None,
    )
    .unwrap();
    let router = platform.rpc();

    // 1. CE Guest creation & session startup
    let guest = rpc(
        &router,
        RpcMethod::CreateGuestPlayer.as_str(),
        android_guest("solo-e2e-device", "0"),
    )
    .await;
    let guest_token = guest["returnValue"]["playerUUID"]
        .as_str()
        .unwrap()
        .to_owned();
    let session = rpc(
        &router,
        RpcMethod::StartSessionForPlayer.as_str(),
        (&guest_token,),
    )
    .await;
    let token = session["sessionToken"].as_str().unwrap().to_owned();
    let player_id: Uuid = session["returnValue"]["playerUUID"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();

    // 2. Solo queue -> offer -> Playing
    let join = rpc(
        &router,
        RpcMethod::JoinLobby.as_str(),
        (
            &token,
            SoloLobbyJoin {
                lobby: "5v5_bots_solo",
                difficulty: "medium",
            },
            "147219",
        ),
    )
    .await;
    assert_eq!(join["returnValue"]["success"], true);

    let pending = rpc(&router, RpcMethod::Update.as_str(), (&token,)).await;
    assert_eq!(
        pending["returnValue"]["state"],
        LobbyState::MatchPending.as_str()
    );

    let playing = rpc(&router, RpcMethod::Update.as_str(), (&token,)).await;
    assert_eq!(
        playing["returnValue"]["state"],
        LobbyState::Playing.as_str()
    );
    let match_id: Uuid = playing["returnValue"]["matchId"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let host = playing["returnValue"]["host"].as_str().unwrap();
    let port = playing["returnValue"]["port"].as_u64().unwrap();
    let address = format!("{host}:{port}");
    let cipher = MatchCipher::new(&match_id.to_string());

    // 3. Single-socket Draft -> solo_opening -> Running -> move
    let mut sock = login_game(&address, player_id, &cipher).await;
    collect_until(&mut sock, &cipher, wire_opcode::DraftRosterFull).await;

    let mut pick = vec![0; 14];
    pick[..4].copy_from_slice(&243u32.to_be_bytes());
    pick[4..8].copy_from_slice(&0xf240ebfeu32.to_be_bytes());
    send_frame(
        &mut sock,
        &cipher,
        &Message::known(Opcode::DraftSelectHero, pick),
    )
    .await;
    collect_until(&mut sock, &cipher, wire_opcode::DraftSelectHero).await;
    send_frame(
        &mut sock,
        &cipher,
        &Message::known(Opcode::DraftLockHero, [0; 6]),
    )
    .await;
    collect_until(&mut sock, &cipher, wire_opcode::DraftLockHero).await;

    rooms.advance_solo_loading(match_id);
    collect_until(&mut sock, &cipher, wire_opcode::DraftTransitionToLoading).await;

    send_frame(
        &mut sock,
        &cipher,
        &Message::known(Opcode::RequestSceneSnapshot, [0; 6]),
    )
    .await;
    send_frame(
        &mut sock,
        &cipher,
        &Message::known(Opcode::ConfirmSceneOwner, [0, 0, 5, 220, 1, 0]),
    )
    .await;
    let opening = collect_until(&mut sock, &cipher, wire_opcode::SceneReadinessRows).await;
    assert_eq!(opening[0].opcode, wire_opcode::SceneSettings);
    let hero_spawns: Vec<u32> = opening
        .iter()
        .filter(|m| m.is(Opcode::HeroSpawn))
        .map(|m| u32::from_be_bytes(m.body[8..12].try_into().unwrap()))
        .collect();
    assert_eq!(hero_spawns, (1500..1510).collect::<Vec<_>>());

    send_frame(&mut sock, &cipher, &Message::clock(0.0)).await;
    collect_until(&mut sock, &cipher, wire_opcode::ClockSync).await;
    send_frame(
        &mut sock,
        &cipher,
        &Message::known(Opcode::SceneReadyConfirm, [0; 6]),
    )
    .await;
    collect_until(&mut sock, &cipher, wire_opcode::StartMatch).await;
    assert_eq!(
        rooms.detail_typed(match_id).unwrap().phase,
        halcyon_game::RoomPhase::Running
    );

    send_frame(&mut sock, &cipher, &state_upd_a(-80.0, 2.0)).await;
    let ack = collect_until_match(&mut sock, &cipher, "slot 14 EntityFloat", |m| {
        m.is(Opcode::MoveTo) && m.body.first() == Some(&14)
    })
    .await;
    assert_eq!(ack.last().unwrap().body[0], 14);
    let move_pos = collect_until_match(&mut sock, &cipher, "actor 1500 Position", |m| {
        m.is(Opcode::MoveToCorrection) && m.body.get(..4) == Some(&1500u32.to_be_bytes())
    })
    .await;
    assert_eq!(
        u32::from_be_bytes(move_pos.last().unwrap().body[..4].try_into().unwrap()),
        1500
    );

    // 4. Mid-match disconnect & alive reconnect to the same match_id
    drop(sock);
    timeout(Duration::from_secs(2), async {
        while rooms.detail_typed(match_id).unwrap().participants.unwrap()[0].control_connected {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();

    let still_playing = rpc(&router, RpcMethod::Update.as_str(), (&token,)).await;
    assert_eq!(
        still_playing["returnValue"]["state"],
        LobbyState::Playing.as_str()
    );
    assert_eq!(
        still_playing["returnValue"]["matchId"],
        match_id.to_string()
    );

    let mut sock = login_game(&address, player_id, &cipher).await;
    collect_until(&mut sock, &cipher, wire_opcode::DraftTransitionToLoading).await;
    send_frame(
        &mut sock,
        &cipher,
        &Message::known(Opcode::RequestSceneSnapshot, [0; 6]),
    )
    .await;
    send_frame(
        &mut sock,
        &cipher,
        &Message::known(Opcode::ConfirmSceneOwner, [0, 0, 5, 220, 1, 0]),
    )
    .await;
    let alive_rejoin = collect_until(&mut sock, &cipher, wire_opcode::SceneReadinessRows).await;
    assert!(
        alive_rejoin
            .iter()
            .any(|m| m.is(Opcode::HeroSpawn) && m.body[8..12] == 1500u32.to_be_bytes())
    );
    send_frame(
        &mut sock,
        &cipher,
        &Message::known(Opcode::SceneReadyConfirm, [0; 6]),
    )
    .await;
    collect_until(&mut sock, &cipher, wire_opcode::StartMatch).await;
    send_frame(&mut sock, &cipher, &state_upd_a(-78.0, 1.0)).await;
    let rejoin_ack = collect_until_match(&mut sock, &cipher, "slot 14 rejoin EntityFloat", |m| {
        m.is(Opcode::MoveTo) && m.body.first() == Some(&14)
    })
    .await;
    assert_eq!(rejoin_ack.last().unwrap().body[0], 14);
    let rejoin_move = collect_until_match(&mut sock, &cipher, "actor 1500 rejoin Position", |m| {
        m.is(Opcode::MoveToCorrection) && m.body.get(..4) == Some(&1500u32.to_be_bytes())
    })
    .await;
    assert_eq!(
        u32::from_be_bytes(rejoin_move.last().unwrap().body[..4].try_into().unwrap()),
        1500
    );

    // 5. Mid-match disconnect & dead-hero reconnect to the same match_id
    drop(sock);
    timeout(Duration::from_secs(2), async {
        while rooms.detail_typed(match_id).unwrap().participants.unwrap()[0].control_connected {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    rooms.defeat_hero(match_id, 0);
    assert!(!rooms.is_hero_alive(match_id, 0));

    let mut sock = login_game(&address, player_id, &cipher).await;
    collect_until(&mut sock, &cipher, wire_opcode::DraftTransitionToLoading).await;
    send_frame(
        &mut sock,
        &cipher,
        &Message::known(Opcode::RequestSceneSnapshot, [0; 6]),
    )
    .await;
    send_frame(
        &mut sock,
        &cipher,
        &Message::known(Opcode::ConfirmSceneOwner, [0, 0, 5, 220, 1, 0]),
    )
    .await;
    let dead_rejoin = collect_until(&mut sock, &cipher, wire_opcode::SceneReadinessRows).await;
    assert!(
        dead_rejoin
            .iter()
            .all(|m| !m.is(Opcode::HeroSpawn) || m.body[8..12] != 1500u32.to_be_bytes()),
        "dead hero reconnect must omit 1500 EntitySpawn in solo_opening"
    );
    send_frame(
        &mut sock,
        &cipher,
        &Message::known(Opcode::SceneReadyConfirm, [0; 6]),
    )
    .await;
    collect_until(&mut sock, &cipher, wire_opcode::StartMatch).await;

    // Step simulation while draining frames cleanly on a split reader until hero respawns.
    let (mut rd, mut wr) = sock.into_split();
    let (frame_tx, mut frame_rx) = tokio::sync::mpsc::channel::<Message>(256);
    let cipher_reader = MatchCipher::new(&match_id.to_string());
    let reader_task = tokio::spawn(async move {
        while let Ok(len) = rd.read_u16().await {
            let mut buf = vec![0; usize::from(len)];
            if rd.read_exact(&mut buf).await.is_err() {
                break;
            }
            if frame_tx
                .send(cipher_reader.decode_owned(buf).unwrap())
                .await
                .is_err()
            {
                break;
            }
        }
        let mut tail = Vec::new();
        let _ = rd.read_to_end(&mut tail).await;
    });

    let mut saw_spawn = false;
    let mut saw_respawn = false;
    timeout(Duration::from_secs(5), async {
        for _ in 0..400 {
            if !rooms.is_hero_alive(match_id, 0) {
                rooms.step_room_tick(match_id);
            }
            tokio::task::yield_now().await;
            while let Ok(msg) = frame_rx.try_recv() {
                if msg.is(Opcode::HeroSpawn) && msg.body[8..12] == 1500u32.to_be_bytes() {
                    saw_spawn = true;
                }
                if msg.is(Opcode::ActorRespawn) && msg.body[..4] == 1500u32.to_be_bytes() {
                    assert!(saw_spawn, "EntitySpawn must precede ActorRespawn");
                    saw_respawn = true;
                }
            }
            if saw_spawn && saw_respawn {
                break;
            }
            sleep(Duration::from_millis(2)).await;
        }
        assert!(saw_spawn && saw_respawn, "dead hero did not respawn");
    })
    .await
    .unwrap();

    // 6. Natural crystal defeat -> [0x048D, 0x03F1] before TCP EOF -> zero-race CE PostMatch
    let crystal_wire = rooms.prepare_crystal_defeat(match_id, 0).await.unwrap();
    let mut attack_body = Vec::with_capacity(6);
    attack_body.extend_from_slice(&crystal_wire.to_be_bytes());
    attack_body.extend_from_slice(&[0, 0]);
    wr.write_all(
        &cipher
            .encode(&Message::known(
                Opcode::RequestModifyBasicAttackTarget,
                attack_body,
            ))
            .unwrap(),
    )
    .await
    .unwrap();

    let mut final_messages = Vec::new();
    timeout(Duration::from_secs(5), async {
        while let Some(msg) = frame_rx.recv().await {
            final_messages.push(msg);
        }
    })
    .await
    .expect("game socket did not close after crystal defeat");
    let _ = reader_task.await;

    // Immediately upon observing TCP EOF, CE update MUST return PostMatch on the very first call.
    let post_match = rpc(&router, RpcMethod::Update.as_str(), (&token,)).await;
    assert_eq!(
        post_match["returnValue"]["state"],
        LobbyState::PostMatch.as_str(),
        "settlement must commit before TCP EOF"
    );

    let opcodes: Vec<u16> = final_messages.iter().map(|m| m.opcode).collect();
    assert!(
        opcodes.ends_with(&[wire_opcode::PostMatchStats, wire_opcode::EndMatch]),
        "expected [0x048D, 0x03F1] before TCP EOF, got {opcodes:?}"
    );
    let report = &final_messages[final_messages.len() - 2];
    let Fields::MatchStatistics(stats) = report.fields().unwrap() else {
        panic!("expected 0x048D StatisticsReport");
    };
    assert_eq!(stats.slot(0).unwrap().player, 1500);
    assert!(stats.slot(0).unwrap().damage_to_structures > 0.0);
    let end_sync = final_messages.last().unwrap();
    assert_eq!(end_sync.body, [0, 0, 0, 1, 0, 0]);

    // 7. notifyExitPostMatch twice (first succeeds, second idempotent, settled once) -> Menus
    for _ in 0..2 {
        let exit = rpc(&router, RpcMethod::NotifyExitPostMatch.as_str(), (&token,)).await;
        assert_eq!(exit["returnValue"]["success"], true);
    }

    let menus = rpc(&router, RpcMethod::Update.as_str(), (&token,)).await;
    assert_eq!(menus["returnValue"]["state"], LobbyState::Menus.as_str());

    let db = rusqlite::Connection::open(&db_path).unwrap();
    let settled_count: i64 = db
        .query_row(
            "SELECT count(*) FROM events WHERE player=?1 AND json_extract(body,'$.type')='match_finished' AND json_extract(body,'$.matchId')=?2",
            [player_id.to_string(), match_id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(settled_count, 1, "match must be settled exactly once");
    let left_count: i64 = db
        .query_row(
            "SELECT count(*) FROM events WHERE player=?1 AND json_extract(body,'$.type')='post_match_left'",
            [player_id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        left_count, 1,
        "notifyExitPostMatch must record post_match_left once"
    );
}
