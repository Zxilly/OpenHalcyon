#[path = "../../federation/tests/support/pool.rs"]
mod pool;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use halcyon_ce_platform::{Config as CeConfig, Platform};
use halcyon_federation::Federation;
use halcyon_game::Rooms;
use halcyon_peer_play::{Config, PeerPlay};
use halcyon_platform::{AppState, PlatformConfig};
use halcyon_protocol::{ACK_FRAME, MatchCipher, Message, hello_with_port};
use serde_json::{Value, json};
use std::{collections::BTreeMap, net::Ipv4Addr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::{sleep, timeout},
};
use tower::ServiceExt;
use uuid::Uuid;
async fn rpc(router: &Router, method: &str, args: Value) -> Value {
    let req = Request::post(format!("/JSONRpc/{method}"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(
            json!({"method":method,"params":args}).to_string(),
        ))
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    let payload: Value =
        serde_json::from_slice(&to_bytes(res.into_body(), 1 << 20).await.unwrap()).unwrap();
    assert_eq!(payload["code"], 0, "{method}: {payload}");
    payload
}
async fn state(router: &Router, token: &Value, expected: &str) -> Value {
    timeout(Duration::from_secs(20), async {
        loop {
            let v = rpc(router, "update", json!([token])).await;
            if v["returnValue"]["state"] == expected {
                return v["returnValue"].clone();
            }
            sleep(Duration::from_millis(80)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {expected}"))
}
async fn login(addr: &str, id: Uuid, c: &MatchCipher) -> TcpStream {
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(&hello_with_port("127.0.0.1", s.peer_addr().unwrap().port()).unwrap())
        .await
        .unwrap();
    let mut ack = [0; 5];
    s.read_exact(&mut ack).await.unwrap();
    assert_eq!(ack, ACK_FRAME);
    let mut body = [0; 70];
    body[..36].copy_from_slice(id.to_string().as_bytes());
    s.write_all(&c.encode(&Message::new(0x3e8, body)).unwrap())
        .await
        .unwrap();
    s
}
async fn until(s: &mut TcpStream, c: &MatchCipher, op: u16) {
    timeout(Duration::from_secs(4), async {
        loop {
            let len = s.read_u16().await.unwrap();
            let mut b = vec![0; len as usize];
            s.read_exact(&mut b).await.unwrap();
            if c.decode(&b).unwrap().opcode == op {
                return;
            }
        }
    })
    .await
    .unwrap()
}
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn two_ce_homes_share_a_third_hosts_game_and_settle() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = pool::configs(tmp.path());
    let mut nodes = Vec::new();
    let mut services = Vec::new();
    let mut hosts = Vec::new();
    let mut routers = Vec::new();
    let host_id = cfg[2].node_id;
    for (i, c) in cfg.into_iter().enumerate() {
        let path = tmp.path().join(format!("platform-{i}.sqlite"));
        let rooms = Rooms::new(
            Ipv4Addr::LOCALHOST.into(),
            "127.0.0.1".into(),
            if i == 2 { 1 } else { 0 },
            None,
        );
        let app = AppState::open(
            PlatformConfig {
                database_path: path.clone(),
                public_address: "localhost:8443".into(),
                seats_per_match: 2,
            },
            Arc::new(rooms.clone()),
        )
        .unwrap();
        let node = Federation::start(c.clone()).await.unwrap();
        let peers = PeerPlay::start(
            node.clone(),
            app.clone(),
            rooms.clone(),
            Config {
                pool_id: c.pool_id,
                node_id: c.node_id,
                database: tmp.path().join(format!("host-{i}.sqlite")),
                rules_digest: "validation-v1".into(),
                peer_latency_ms: BTreeMap::from([(host_id, 12)]),
                max_latency_ms: 90,
            },
        )
        .unwrap();
        let outbox = peers.clone();
        rooms.set_finish_handler(Arc::new(move |id, v| {
            assert!(outbox.finish(id, v).unwrap())
        }));
        let ce = Platform::with_peers(
            app,
            CeConfig {
                authority: "localhost:8443".into(),
                notify_authority: "localhost:2112".into(),
            },
            &[i as u8 + 40; 32],
            &path,
            None,
            peers.clone(),
        )
        .unwrap();
        routers.push(ce.rpc());
        services.push(peers);
        hosts.push(rooms);
        nodes.push(node);
    }
    timeout(Duration::from_secs(15), async {
        while nodes[0].read().await.is_err() {
            sleep(Duration::from_millis(80)).await
        }
    })
    .await
    .unwrap();
    let mut tokens = Vec::new();
    let mut players = Vec::new();
    for r in &routers[..2] {
        let guest = rpc(
            r,
            "createGuestPlayer",
            json!([
                "kindred",
                "en",
                "US",
                "production",
                "en",
                "BAKLAVA",
                "android",
                "",
                "147219",
                1,
                "device",
                "model",
                "0"
            ]),
        )
        .await;
        let session = rpc(r, "startSessionForPlayer", json!([guest["returnValue"]["playerUUID"]])).await;
        let token = session["sessionToken"].clone();
        players.push(
            session["returnValue"]["playerUUID"]
                .as_str()
                .unwrap()
                .parse::<Uuid>()
                .unwrap(),
        );
        rpc(
            r,
            "joinLobby",
            json!([token,{"lobby":"5v5_casual"},"147219"]),
        )
        .await;
        tokens.push(token);
    }
    for (r, t) in routers.iter().zip(&tokens) {
        state(r, t, "match_pending").await;
        rpc(r, "acceptMatch", json!([t])).await;
    }
    let a = state(&routers[0], &tokens[0], "playing").await;
    let b = state(&routers[1], &tokens[1], "playing").await;
    assert_eq!(a["matchId"], b["matchId"]);
    assert_eq!(a["port"], b["port"]);
    assert_eq!(a["host"], b["host"]);
    let id: Uuid = a["matchId"].as_str().unwrap().parse().unwrap();
    let addr = format!("{}:{}", a["host"].as_str().unwrap(), a["port"]);
    let cipher = MatchCipher::new(&id.to_string());
    timeout(Duration::from_secs(5), async {
        while hosts[2]
            .detail(id)
            .as_ref()
            .is_none_or(|v| v["phase"] == "reserved")
        {
            sleep(Duration::from_millis(50)).await
        }
    })
    .await
    .unwrap();
    let mut controls = Vec::new();
    for player in &players {
        let mut c = login(&addr, *player, &cipher).await;
        until(&mut c, &cipher, 0x459).await;
        c.write_all(&cipher.encode(&Message::new(0x455, [0; 6])).unwrap())
            .await
            .unwrap();
        until(&mut c, &cipher, 0x455).await;
        controls.push(c);
    }
    let mut games = Vec::new();
    for player in &players {
        let mut g = login(&addr, *player, &cipher).await;
        until(&mut g, &cipher, 0x3f3).await;
        games.push(g);
    }
    games[0]
        .write_all(&cipher.encode(&Message::movement(4., 0.)).unwrap())
        .await
        .unwrap();
    until(&mut games[0], &cipher, 0x42e).await;
    assert_eq!(hosts[2].detail(id).unwrap()["phase"], "running");
    assert_eq!(hosts[0].active_rooms() + hosts[1].active_rooms(), 0);
    hosts[2]
        .terminate(id, "integration-test", "settlement smoke test")
        .unwrap();
    for (r, t) in routers.iter().zip(&tokens) {
        state(r, t, "post_match").await;
        rpc(r, "notifyExitPostMatch", json!([t])).await;
        state(r, t, "menus").await;
    }
    let pool = nodes[0].read().await.unwrap();
    assert_eq!(pool.allocations().len(), 1);
    assert_eq!(
        pool.allocations()[&id].result.as_ref().unwrap()["reason"],
        "admin_terminated"
    );
    assert_eq!(pool.allocations()[&id].host, host_id);
    for svc in services {
        svc.shutdown().await;
    }
    for node in nodes {
        node.shutdown().await.unwrap();
    }
}
