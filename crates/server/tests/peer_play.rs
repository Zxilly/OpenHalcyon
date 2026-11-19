#[path = "../../federation/tests/support/pool.rs"]
mod pool;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use halcyon_ce_platform::{Config as CeConfig, Platform};
use halcyon_federation::{Config as FederationConfig, Federation};
use halcyon_game::Rooms;
use halcyon_matchmaking::{Phase, Pool};
use halcyon_peer_play::{Config, PeerPlay};
use halcyon_platform::{AppState, PlatformConfig};
use halcyon_protocol::{ACK_FRAME, MatchCipher, Message, hello_with_port};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    net::Ipv4Addr,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::{sleep, timeout},
};
use tower::ServiceExt;
use uuid::Uuid;
async fn rpc_result(router: &Router, method: &str, args: Value) -> Value {
    let req = Request::post(format!("/JSONRpc/{method}"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(
            json!({"method":method,"params":args}).to_string(),
        ))
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    serde_json::from_slice(&to_bytes(res.into_body(), 1 << 20).await.unwrap()).unwrap()
}

async fn rpc(router: &Router, method: &str, args: Value) -> Value {
    let payload = rpc_result(router, method, args).await;
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
    timeout(Duration::from_secs(4), s.read_exact(&mut ack))
        .await
        .expect("game hello acknowledgement timed out")
        .expect("game socket closed before hello acknowledgement");
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
struct Member {
    raft: Federation,
    peer_play: PeerPlay,
    rooms: Rooms,
    rpc: Router,
    app: AppState,
    path: PathBuf,
    peer_config: Config,
}

impl Member {
    async fn start(path: &Path, index: usize, config: FederationConfig, host: Uuid) -> Self {
        let rooms = Rooms::new(
            Ipv4Addr::LOCALHOST.into(),
            "127.0.0.1".into(),
            usize::from(config.node_id == host),
            None,
        );
        let path_to_db = path.join(format!("platform-{index}.sqlite"));
        let app = AppState::open(
            PlatformConfig {
                database_path: path_to_db.clone(),
                public_address: "localhost:8443".into(),
                seats_per_match: 3,
            },
            Arc::new(rooms.clone()),
        )
        .unwrap();
        let peer_config = Config {
            pool_id: config.pool_id,
            node_id: config.node_id,
            database: path.join(format!("host-{index}.sqlite")),
            rules_digest: "validation-v1".into(),
            peer_latency_ms: BTreeMap::from([(host, 12)]),
            max_latency_ms: 90,
        };
        let raft = Federation::start(config).await.unwrap();
        if peer_config.node_id == host {
            // Both homes contend normally. The third member still casts a real vote.
            raft.suppress_test_nomination();
        }
        Self::start_worker(index, raft, rooms, app, path_to_db, peer_config)
    }

    fn start_worker(
        index: usize,
        raft: Federation,
        rooms: Rooms,
        app: AppState,
        path_to_db: PathBuf,
        peer_config: Config,
    ) -> Self {
        let peer_play = PeerPlay::start(
            raft.clone(),
            app.clone(),
            rooms.clone(),
            peer_config.clone(),
        )
        .unwrap();
        install_finish_handler(&rooms, &peer_play);
        let rpc = Platform::with_peers(
            app.clone(),
            CeConfig {
                authority: "localhost:8443".into(),
                notify_authority: "localhost:2112".into(),
            },
            &[index as u8 + 40; 32],
            &path_to_db,
            None,
            peer_play.clone(),
        )
        .unwrap()
        .rpc();
        Self {
            raft,
            peer_play,
            rooms,
            rpc,
            app,
            path: path_to_db,
            peer_config,
        }
    }

    fn reopen_worker(self, index: usize) -> Self {
        let Self {
            raft,
            peer_play,
            rooms,
            rpc,
            app,
            path,
            peer_config,
        } = self;
        rooms.set_finish_handler(Arc::new(|_, _| {}));
        drop(rpc);
        drop(peer_play);
        Self::start_worker(index, raft, rooms, app, path, peer_config)
    }

    async fn stop(self) {
        timeout(Duration::from_secs(8), async {
            let (_, raft) = tokio::join!(self.peer_play.shutdown(), self.raft.shutdown());
            raft.unwrap();
        })
        .await
        .expect("member shutdown timed out");
        self.rooms.set_finish_handler(Arc::new(|_, _| {}));
    }
}

fn install_finish_handler(rooms: &Rooms, peers: &PeerPlay) {
    let outbox = peers.clone();
    rooms.set_finish_handler(Arc::new(move |id, result| {
        assert!(outbox.finish(id, result).unwrap());
    }));
}

fn node(cluster: &[Option<Member>], index: usize) -> &Member {
    cluster[index].as_ref().expect("member is online")
}

async fn elected(cluster: &[Option<Member>], minimum_term: u64) -> (usize, u64) {
    let election = timeout(Duration::from_secs(12), async {
        loop {
            for (index, member) in cluster.iter().enumerate() {
                let Some(member) = member else { continue };
                let metrics = member.raft.metrics();
                if metrics.current_term >= minimum_term
                    && metrics.state.is_leader()
                    && metrics.current_leader == Some(metrics.id)
                    && member.raft.read().await.is_ok()
                {
                    assert_eq!(metrics.membership_config.voter_ids().count(), 3);
                    assert!(
                        index < 2,
                        "home voters should lead while test host is not a contender"
                    );
                    return (index, metrics.current_term);
                }
            }
            sleep(Duration::from_millis(40)).await;
        }
    })
    .await;
    election.unwrap_or_else(|_| {
        let metrics: Vec<_> = cluster
            .iter()
            .map(|member| {
                member
                    .as_ref()
                    .map(|member| member.raft.metrics().to_string())
            })
            .collect();
        panic!("quorum did not elect a home leader in term {minimum_term}; {metrics:?}")
    })
}

async fn wait_replication(cluster: &[Option<Member>], leader: usize) {
    let applied = node(cluster, leader)
        .raft
        .metrics()
        .last_applied
        .expect("leader applied a log")
        .index;
    timeout(Duration::from_secs(5), async {
        loop {
            if cluster.iter().flatten().all(|member| {
                member
                    .raft
                    .metrics()
                    .last_applied
                    .is_some_and(|log| log.index >= applied)
            }) {
                return;
            }
            sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("all voters did not apply the pre-failover log");
    let voter_ids: Vec<_> = cluster
        .iter()
        .flatten()
        .map(|member| member.raft.metrics().id)
        .collect();
    for (index, member) in cluster.iter().enumerate() {
        let Some(member) = member else { continue };
        let metrics = member.raft.metrics();
        assert_eq!(
            metrics.membership_config.voter_ids().collect::<Vec<_>>(),
            voter_ids
        );
        assert_eq!(metrics.state.is_leader(), index == leader);
    }
}

async fn recover<'a>(
    cluster: &mut [Option<Member>],
    root: &Path,
    config: FederationConfig,
    index: usize,
    host: Uuid,
    sessions: impl Iterator<Item = (&'a Uuid, &'a mut Value)>,
) {
    assert!(cluster[index].is_none());
    let member = Member::start(root, index, config, host).await;
    wait_pool(&member, |_| true).await;
    for (player, token) in sessions {
        let resumed = rpc(&member.rpc, "startSessionForPlayer", json!([token])).await;
        assert_eq!(resumed["returnValue"]["playerUUID"], player.to_string());
        *token = resumed["sessionToken"].clone();
    }
    cluster[index] = Some(member);
}

async fn wait_pool(member: &Member, condition: impl Fn(&Pool) -> bool) -> Pool {
    timeout(Duration::from_secs(8), async {
        loop {
            if let Ok(pool) = member.raft.read().await
                && condition(&pool)
            {
                return pool;
            }
            sleep(Duration::from_millis(40)).await;
        }
    })
    .await
    .expect("committed pool condition timed out")
}

async fn echo_clock(socket: &mut TcpStream, cipher: &MatchCipher, value: f32) {
    let mut body = [0; 6];
    body[..4].copy_from_slice(&value.to_be_bytes());
    socket
        .write_all(&cipher.encode(&Message::new(0, body)).unwrap())
        .await
        .unwrap();
    timeout(Duration::from_secs(4), async {
        loop {
            let len = socket.read_u16().await.unwrap();
            let mut packet = vec![0; len as usize];
            socket.read_exact(&mut packet).await.unwrap();
            let m = cipher.decode(&packet).unwrap();
            if m.opcode == 0 && m.body == body {
                break;
            }
        }
    })
    .await
    .expect("established game socket did not answer after election");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn party_game_survives_two_home_elections_and_replays_result_once() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = pool::configs(tmp.path());
    let host_id = cfg[2].node_id;
    let mut cluster = Vec::new();
    for (i, config) in cfg.iter().enumerate() {
        cluster.push(Some(
            Member::start(tmp.path(), i, config.clone(), host_id).await,
        ));
    }
    let (first_leader, first_term) = elected(&cluster, 1).await;
    let homes = [0, 0, 1];
    let mut tokens = Vec::new();
    let mut players = Vec::new();
    for home in homes {
        let home = &node(&cluster, home).rpc;
        let guest = rpc(
            home,
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
        let session = rpc(
            home,
            "startSessionForPlayer",
            json!([guest["returnValue"]["playerUUID"]]),
        )
        .await;
        players.push(
            serde_json::from_value::<Uuid>(session["returnValue"]["playerUUID"].clone()).unwrap(),
        );
        tokens.push(session["sessionToken"].clone());
    }
    let party = rpc(
        &node(&cluster, homes[0]).rpc,
        "createParty",
        json!([tokens[0], {"partyQueueMode":"5v5_casual"}]),
    )
    .await;
    let party_id = party["returnValue"]["partyUUID"].clone();
    rpc(
        &node(&cluster, homes[0]).rpc,
        "partyInviteSend",
        json!([tokens[0], {"playerUUID":players[1]}]),
    )
    .await;
    let invite = rpc(
        &node(&cluster, homes[1]).rpc,
        "queryPartyInvites",
        json!([tokens[1]]),
    )
    .await;
    assert_eq!(
        invite["returnValue"]["pendingPartyInvites"][0]["groupId"],
        party_id
    );
    rpc(
        &node(&cluster, homes[1]).rpc,
        "partyInviteConfirm",
        json!([tokens[1], {"partyUUID":party_id}]),
    )
    .await;
    for i in [0, 2] {
        rpc(
            &node(&cluster, homes[i]).rpc,
            "joinLobby",
            json!([tokens[i], {"lobby":"5v5_casual"}, "147219"]),
        )
        .await;
    }
    for (home, token) in homes.iter().zip(&tokens) {
        state(&node(&cluster, *home).rpc, token, "match_pending").await;
    }
    rpc(
        &node(&cluster, homes[0]).rpc,
        "acceptMatch",
        json!([tokens[0]]),
    )
    .await;
    rpc(
        &node(&cluster, homes[2]).rpc,
        "acceptMatch",
        json!([tokens[2]]),
    )
    .await;
    let snapshot = wait_pool(node(&cluster, first_leader), |pool| {
        pool.allocations()
            .values()
            .any(|a| a.phase == Phase::Preparing && a.endpoint.is_some() && a.accepted.len() == 1)
    })
    .await;
    let active = snapshot
        .allocations()
        .values()
        .find(|a| a.tickets.len() == 2)
        .unwrap();
    assert_eq!(
        active.accepted.len(),
        1,
        "captain's confirmation cannot accept the party ticket"
    );
    let roster = active
        .tickets
        .iter()
        .map(|ticket| &snapshot.tickets()[&ticket.id].ticket.players);
    assert!(
        roster
            .into_iter()
            .any(|players_in_ticket| players_in_ticket == &players[..2])
    );
    let allocation = active.id;
    assert_eq!(active.host, host_id);
    let initial_created = node(&cluster, 2).rooms.detail(allocation).unwrap()["createdAt"].clone();
    wait_replication(&cluster, first_leader).await;
    cluster[first_leader].take().unwrap().stop().await;
    assert!(TcpStream::connect(cfg[first_leader].listen).await.is_err());
    let (second_leader, second_term) = elected(&cluster, first_term + 1).await;
    assert_ne!(second_leader, first_leader);
    let remote = wait_pool(node(&cluster, second_leader), |pool| {
        pool.allocations()
            .get(&allocation)
            .is_some_and(|a| a.phase == Phase::Preparing && a.accepted.len() == 1)
    })
    .await;
    assert_eq!(remote.allocations().len(), 1);
    recover(
        &mut cluster,
        tmp.path(),
        cfg[first_leader].clone(),
        first_leader,
        host_id,
        players
            .iter()
            .zip(&mut tokens)
            .zip(homes)
            .filter(|(_, home)| *home == first_leader)
            .map(|(session, _)| session),
    )
    .await;
    rpc(
        &node(&cluster, homes[1]).rpc,
        "acceptMatch",
        json!([tokens[1]]),
    )
    .await;
    let a = state(&node(&cluster, homes[0]).rpc, &tokens[0], "playing").await;
    let b = state(&node(&cluster, homes[2]).rpc, &tokens[2], "playing").await;
    let party_member = state(&node(&cluster, homes[1]).rpc, &tokens[1], "playing").await;
    assert_eq!(a["matchId"], party_member["matchId"]);
    assert_eq!(a["matchId"], b["matchId"]);
    assert_eq!(a["port"], b["port"]);
    assert_eq!(a["host"], b["host"]);
    let id: Uuid = a["matchId"].as_str().unwrap().parse().unwrap();
    let addr = format!("{}:{}", a["host"].as_str().unwrap(), a["port"]);
    assert_eq!(id, allocation);
    let room = node(&cluster, 2).rooms.detail(id).unwrap();
    assert_eq!(room["createdAt"], initial_created);
    let side = |player: Uuid| {
        room["participants"]
            .as_array()
            .unwrap()
            .iter()
            .find(|seat| seat["playerId"] == player.to_string())
            .and_then(|seat| seat["team"].as_u64())
            .unwrap()
    };
    assert_eq!(side(players[0]), side(players[1]));
    assert_ne!(side(players[0]), side(players[2]));
    let cipher = MatchCipher::new(&id.to_string());
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
    assert_eq!(
        node(&cluster, 2).rooms.detail(id).unwrap()["phase"],
        "running"
    );
    assert_eq!(
        node(&cluster, 0).rooms.active_rooms() + node(&cluster, 1).rooms.active_rooms(),
        0
    );

    wait_replication(&cluster, second_leader).await;
    cluster[second_leader].take().unwrap().stop().await;
    assert!(TcpStream::connect(cfg[second_leader].listen).await.is_err());
    let (third_leader, third_term) = elected(&cluster, second_term + 1).await;
    assert_eq!(third_leader, first_leader);
    assert!(third_term > second_term);
    let active = wait_pool(node(&cluster, third_leader), |pool| {
        pool.allocations()[&id].phase == Phase::Ready
    })
    .await;
    assert_eq!(active.allocations().len(), 1);
    assert_eq!(
        node(&cluster, 2).rooms.detail(id).unwrap()["createdAt"],
        initial_created
    );
    for (index, (control, game)) in controls.iter_mut().zip(&mut games).enumerate() {
        echo_clock(control, &cipher, 300. + index as f32).await;
        echo_clock(game, &cipher, 400. + index as f32).await;
    }
    recover(
        &mut cluster,
        tmp.path(),
        cfg[second_leader].clone(),
        second_leader,
        host_id,
        players
            .iter()
            .zip(&mut tokens)
            .zip(homes)
            .filter(|(_, home)| *home == second_leader)
            .map(|(session, _)| session),
    )
    .await;

    // Finish with the worker offline, then rebuild it from the same host journal.
    let gs = cluster[2].as_mut().unwrap();
    timeout(Duration::from_secs(8), gs.peer_play.shutdown())
        .await
        .unwrap();
    gs.rooms
        .terminate(id, "integration-test", "settlement outbox replay")
        .unwrap();
    let outbox = timeout(Duration::from_secs(4), async {
        loop {
            let row: (Option<String>, bool) = rusqlite::Connection::open(&gs.peer_config.database)
                .unwrap()
                .query_row(
                    "SELECT result,delivered FROM hosted WHERE id=?",
                    [id.to_string()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            if let (Some(result), false) = row {
                break serde_json::from_str::<Value>(&result).unwrap();
            }
            sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("host never stored an undelivered result");
    assert_eq!(outbox["reason"], "admin_terminated");
    assert!(
        gs.raft.read().await.unwrap().allocations()[&id]
            .result
            .is_none()
    );

    cluster[2] = Some(cluster[2].take().unwrap().reopen_worker(2));
    for (p, home) in homes.iter().enumerate() {
        let member = node(&cluster, *home);
        state(&member.rpc, &tokens[p], "post_match").await;
        rpc(&member.rpc, "notifyExitPostMatch", json!([tokens[p]])).await;
        state(&member.rpc, &tokens[p], "menus").await;
        let db = rusqlite::Connection::open(&member.path).unwrap();
        let applied: u64=db.query_row(
            "SELECT count(*) FROM events WHERE player=?1 AND json_extract(body,'$.type')='match_finished'
               AND json_extract(body,'$.matchId')=?2",
            [players[p].to_string(),id.to_string()],|row|row.get(0),
        ).unwrap();
        assert_eq!(
            applied, 1,
            "exactly one settlement was applied for each player"
        );
    }
    let host = node(&cluster, 2);
    let pool = wait_pool(host, |state| {
        state.allocations()[&id].phase == Phase::Finished
    })
    .await;
    assert_eq!(pool.allocations().len(), 1);
    assert_eq!(pool.allocations()[&id].result.as_ref(), Some(&outbox));
    assert_eq!(pool.allocations()[&id].host, host_id);
    assert_eq!(host.rooms.list().len(), 1);
    timeout(Duration::from_secs(4), async {
        let journal = rusqlite::Connection::open(&host.peer_config.database).unwrap();
        loop {
            let delivered: u64 = journal
                .query_row("SELECT count(*) FROM hosted WHERE delivered=1", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert!(delivered <= 1);
            if delivered == 1 {
                break;
            }
            sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("outbox delivery was never acknowledged");
    for node in cluster.into_iter().flatten() {
        node.stop().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn local_friend_spectates_read_only_and_revokes_on_exit() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("spectator.sqlite");
    let rooms = Rooms::new(Ipv4Addr::LOCALHOST.into(), "127.0.0.1".into(), 1, None);
    let app = AppState::open(
        PlatformConfig {
            database_path: path.clone(),
            public_address: "localhost:8443".into(),
            seats_per_match: 1,
        },
        Arc::new(rooms.clone()),
    )
    .unwrap();
    let platform = Platform::new(
        app,
        CeConfig {
            authority: "localhost:8443".into(),
            notify_authority: "localhost:2112".into(),
        },
        &[73; 32],
        &path,
        None,
    )
    .unwrap();
    let rpc_router = platform.rpc();
    let mut sessions = Vec::new();
    for name in ["Operator", "Watcher"] {
        let guest = rpc(
            &rpc_router,
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
        let session = rpc(
            &rpc_router,
            "startSessionForPlayer",
            json!([guest["returnValue"]["playerUUID"]]),
        )
        .await;
        rpc(
            &rpc_router,
            "renamePlayerHandle",
            json!([session["sessionToken"], name]),
        )
        .await;
        sessions.push(session);
    }
    let player = &sessions[0]["sessionToken"];
    let watcher = &sessions[1]["sessionToken"];
    let player_id: Uuid =
        serde_json::from_value(sessions[0]["returnValue"]["playerUUID"].clone()).unwrap();
    let watcher_id: Uuid =
        serde_json::from_value(sessions[1]["returnValue"]["playerUUID"].clone()).unwrap();
    rpc(
        &rpc_router,
        "joinLobby",
        json!([player,{"lobby":"5v5_casual"}]),
    )
    .await;
    rpc(&rpc_router, "acceptMatch", json!([player])).await;
    let playing = state(&rpc_router, player, "playing").await;
    let id: Uuid = serde_json::from_value(playing["matchId"].clone()).unwrap();
    let addr = format!("{}:{}", playing["host"].as_str().unwrap(), playing["port"]);
    let cipher = MatchCipher::new(&id.to_string());
    let mut control = login(&addr, player_id, &cipher).await;
    until(&mut control, &cipher, 0x459).await;
    control
        .write_all(&cipher.encode(&Message::new(0x455, [0; 6])).unwrap())
        .await
        .unwrap();
    until(&mut control, &cipher, 0x455).await;
    let mut game = login(&addr, player_id, &cipher).await;
    until(&mut game, &cipher, 0x3f3).await;

    let denied = rpc_result(&rpc_router, "spectateFriend", json!([watcher, player_id])).await;
    assert_ne!(denied["code"], 0);
    rpc(
        &rpc_router,
        "friendRequestIssueByHandle",
        json!([watcher, "Operator"]),
    )
    .await;
    rpc(
        &rpc_router,
        "friendRequestConfirm",
        json!([player, watcher_id]),
    )
    .await;
    rpc(
        &rpc_router,
        "setPresenceInvisibility",
        json!([player, true]),
    )
    .await;
    let hidden = rpc_result(&rpc_router, "spectateFriend", json!([watcher, player_id])).await;
    assert_ne!(hidden["code"], 0);
    rpc(
        &rpc_router,
        "setPresenceInvisibility",
        json!([player, false]),
    )
    .await;
    let response = rpc(
        &rpc_router,
        "spectateFriend",
        json!([watcher,{"friendUUID":player_id}]),
    )
    .await;
    let value = response["returnValue"].clone();
    assert_eq!(value["state"], "spectating");
    assert_eq!(value["matchId"], id.to_string());
    let credential: Uuid = serde_json::from_value(value["spectatorId"].clone()).unwrap();
    assert_ne!(credential, watcher_id);
    let spectator_addr = format!("{}:{}", value["host"].as_str().unwrap(), value["port"]);
    assert_ne!(spectator_addr, addr);
    let mut viewing = login(&spectator_addr, credential, &cipher).await;
    until(&mut viewing, &cipher, 0x459).await;
    let in_use = rpc_result(
        &rpc_router,
        "joinLobby",
        json!([watcher,{"lobby":"5v5_casual"}]),
    )
    .await;
    assert_ne!(in_use["code"], 0);
    let fallback = value["notifyFallbackUrl"].as_str().unwrap();
    let uri: axum::http::Uri = fallback.parse().unwrap();
    let poll = platform
        .notifications()
        .oneshot(Request::get(uri.path()).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let notification: Value =
        serde_json::from_slice(&to_bytes(poll.into_body(), 1 << 20).await.unwrap()).unwrap();
    assert_eq!(notification["text"]["states"]["state"], "spectating");
    assert!(notification["text"]["states"].get("spectatorId").is_none());
    let exited = rpc(&rpc_router, "spectatorExitMatch", json!([watcher])).await;
    assert_eq!(exited["returnValue"]["handle"], "Watcher");
    state(&rpc_router, watcher, "menus").await;
    timeout(Duration::from_secs(2), async {
        let mut bytes = [0; 4096];
        while viewing.read(&mut bytes).await.unwrap_or(0) != 0 {}
    })
    .await
    .unwrap();
    assert_eq!(rooms.detail(id).unwrap()["phase"], "running");
}
