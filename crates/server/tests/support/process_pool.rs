//! Run both subprocess scenarios with
//! `cargo test -p openhalcyon --test peer_play subprocess_ -- --ignored --nocapture --test-threads=1`.
//! The long scenario waits up to 45 minutes of real time for the simulated match to finish.
//! All credentials, pool certificates, binaries and SQLite files are scoped to a temporary directory.
use super::{echo_clock, login, pool, until};
use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use halcyon_protocol::{MatchCipher, Message};
use reqwest::{Certificate, Client};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::{Instant, sleep, timeout},
};
use uuid::Uuid;

struct Node {
    path: PathBuf,
    binary: PathBuf,
    db: PathBuf,
    origin: String,
    child: Option<Child>,
}
impl Node {
    fn start(&mut self) {
        self.child = Some(
            Command::new(&self.binary)
                .args(["serve", "--config"])
                .arg(&self.path)
                .env("TOKIO_WORKER_THREADS", "2")
                .env("RUST_LOG", "warn")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
        );
    }
    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
impl Drop for Node {
    fn drop(&mut self) {
        self.stop()
    }
}
async fn rpc(client: &Client, n: &Node, method: &str, args: Value) -> Value {
    let r = client
        .post(format!("{}/JSONRpc/{method}", n.origin))
        .header("content-type", "application/x-www-form-urlencoded")
        .body(json!({"method":method,"params":args}).to_string())
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(r["code"], 0, "RPC {method}");
    r
}
async fn session(client: &Client, n: &Node) -> (Uuid, Value) {
    let guest = rpc(
        client,
        n,
        "createGuestPlayer",
        json!([
            "kindred", "en", "US", "local", "en", "device", "android", "", "147219", 1, "model",
            "brand", "0"
        ]),
    )
    .await;
    let s = rpc(
        client,
        n,
        "startSessionForPlayer",
        json!([guest["returnValue"]["playerUUID"]]),
    )
    .await;
    (
        s["returnValue"]["playerUUID"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap(),
        s["sessionToken"].clone(),
    )
}
async fn state(client: &Client, n: &Node, token: &Value, want: &str) -> Value {
    let mut last_phase = Value::Null;
    timeout(Duration::from_secs(90), async {
        loop {
            let r = rpc(client, n, "update", json!([token])).await;
            last_phase = r["returnValue"]["state"].clone();
            if last_phase == want {
                return r["returnValue"].clone();
            }
            sleep(Duration::from_millis(300)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "{} did not reach {want}; last observed phase: {last_phase}",
            n.origin
        )
    })
}
async fn admin(client: &Client, n: &Node) -> String {
    timeout(Duration::from_secs(45), async {
        loop {
            if let Ok(r) = client
                .post(format!("{}/api/admin/v1/session", n.origin))
                .header("origin", &n.origin)
                .json(&json!({"username":"operator","password":"synthetic-test-password"}))
                .send()
                .await
                && r.status().is_success()
            {
                return r.headers()["set-cookie"]
                    .to_str()
                    .unwrap()
                    .split(';')
                    .next()
                    .unwrap()
                    .to_string();
            }
            sleep(Duration::from_millis(500)).await;
        }
    })
    .await
    .expect("admin service did not start")
}
async fn get(client: &Client, n: &Node, cookie: &str, path: &str) -> Value {
    client
        .get(format!("{}/api/admin/v1/{path}", n.origin))
        .header("cookie", cookie)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap()
}
fn spawn_nodes(dir: &Path) -> Vec<Node> {
    // Each pool restarts its original executable even if the workspace rebuilds.
    let binary = dir.join(format!("pool-server{}", std::env::consts::EXE_SUFFIX));
    fs::copy(env!("CARGO_BIN_EXE_openhalcyon"), &binary).unwrap();
    let (cfg, federation_listeners) = pool::configs(dir);
    let mut public_listeners = Vec::new();
    let hash = Argon2::default()
        .hash_password(
            b"synthetic-test-password",
            &SaltString::encode_b64(b"halcyon-process-test").unwrap(),
        )
        .unwrap()
        .to_string();
    let mut nodes = vec![];
    for (i, f) in cfg.iter().enumerate() {
        let folder = dir.join(format!("node-{i}"));
        fs::create_dir(&folder).unwrap();
        let path = folder.join("server.toml");
        let db = folder.join("platform.sqlite");
        fs::write(folder.join("pool.json"), serde_json::to_vec(f).unwrap()).unwrap();
        fs::write(
            folder.join("secret"),
            [*Uuid::new_v4().as_bytes(), *Uuid::new_v4().as_bytes()].concat(),
        )
        .unwrap();
        let listeners = [(); 3].map(|()| std::net::TcpListener::bind("127.0.0.1:0").unwrap());
        let [bootstrap, web, notify] = listeners.each_ref().map(|sock| sock.local_addr().unwrap());
        public_listeners.push(listeners);
        let origin = format!("https://{web}");
        let values = json!({"instance_id":f.node_id,"public_host":"127.0.0.1","bootstrap_listen":bootstrap,"platform_listen":web,"notify_listen":notify,"game_bind":"127.0.0.1","max_rooms":8,"seats_per_match":2,"database":"platform.sqlite","jwt_secret":"secret","certificate":f.cert,"private_key":f.key,"admin":{"origin":origin,"username":"operator","password_hash":hash},"federation":{"cluster_file":"pool.json","journal":"journal.db","latency_ms":BTreeMap::from([(cfg[2].node_id.to_string(),5)]),"max_latency_ms":50}});
        fs::write(&path, toml::to_string(&values).unwrap()).unwrap();
        nodes.push(Node {
            binary: binary.clone(),
            path,
            db,
            origin,
            child: None,
        });
    }
    for ((n, federation_listener), listeners) in nodes
        .iter_mut()
        .zip(federation_listeners)
        .zip(public_listeners)
    {
        // The child uses the regular binary and binds its own listeners.
        // Keep every other node's ports reserved until launching that node.
        drop((federation_listener, listeners));
        n.start();
    }
    nodes
}
async fn drain(socket: TcpStream, cipher: MatchCipher) {
    let (mut r, mut w) = socket.into_split();
    let mut clock = 0.;
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    let mut buf = [0; 8192];
    loop {
        tokio::select! {
            read=r.read(&mut buf)=>if !matches!(read,Ok(n) if n>0){break},
            _=interval.tick()=>{
                if w.write_all(&cipher.encode(&Message::clock(clock)).unwrap()).await.is_err(){break}
                clock+=1.;
            }
        }
    }
}

struct RunningPool {
    nodes: Vec<Node>,
    client: Client,
    cookies: Vec<String>,
    identities: Vec<(Uuid, Value)>,
    room_id: String,
    created: Value,
    _drains: tokio::task::JoinSet<()>,
}
async fn running_pool(dir: &Path) -> RunningPool {
    let nodes = spawn_nodes(dir);
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .add_root_certificate(
            Certificate::from_pem(&fs::read(dir.join("ca.pem")).unwrap()).unwrap(),
        )
        .build()
        .unwrap();
    let mut cookies = vec![];
    for n in &nodes {
        cookies.push(admin(&client, n).await);
    }
    let mut identities = vec![];
    for n in &nodes[..2] {
        identities.push(session(&client, n).await);
    }
    // Two human homes use the configured 5v5 seat count and the separate
    // gameplay connection. The legacy ARAL queue requires six human seats.
    for i in 0..2 {
        rpc(
            &client,
            &nodes[i],
            "joinLobby",
            json!([identities[i].1, {"lobby":"5v5_pvp_casual"}, "147219"]),
        )
        .await;
    }
    for i in 0..2 {
        state(&client, &nodes[i], &identities[i].1, "match_pending").await;
        rpc(&client, &nodes[i], "acceptMatch", json!([identities[i].1])).await;
    }
    let a = state(&client, &nodes[0], &identities[0].1, "playing").await;
    let b = state(&client, &nodes[1], &identities[1].1, "playing").await;
    assert_eq!(a["matchId"], b["matchId"]);
    let id = a["matchId"].as_str().unwrap();
    let addr = format!("127.0.0.1:{}", a["port"]);
    assert_eq!(a["port"], b["port"]);
    let cipher = MatchCipher::new(id);
    let mut sockets = vec![];
    for (player, _) in &identities {
        let mut s = login(&addr, *player, &cipher).await;
        until(&mut s, &cipher, 0x459).await;
        s.write_all(&cipher.encode(&Message::new(0x455, [0; 6])).unwrap())
            .await
            .unwrap();
        until(&mut s, &cipher, 0x455).await;
        sockets.push(s);
    }
    for (player, _) in &identities {
        let mut s = login(&addr, *player, &cipher).await;
        until(&mut s, &cipher, 0x3f3).await;
        echo_clock(&mut s, &cipher, 13.25).await;
        sockets.push(s);
    }
    let mut drains = tokio::task::JoinSet::new();
    for s in sockets {
        drains.spawn(drain(s, MatchCipher::new(id)));
    }
    let room_path = format!("matches/{id}");
    let room = get(&client, &nodes[2], &cookies[2], &room_path).await;
    let started = room["createdAt"].clone();
    assert!(started.is_string());
    assert_eq!(room["phase"], "running");
    RunningPool {
        nodes,
        client,
        cookies,
        identities,
        room_id: id.to_owned(),
        created: started,
        _drains: drains,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "three local OS processes; waits for a natural simulated crystal death"]
async fn subprocess_match_finishes_on_both_homes() {
    let dir = tempfile::tempdir().unwrap();
    let RunningPool {
        mut nodes,
        client,
        mut cookies,
        identities,
        room_id,
        created: started,
        _drains,
    } = running_pool(dir.path()).await;
    let id = room_id.as_str();
    let room_path = format!("matches/{id}");
    let pool = get(&client, &nodes[2], &cookies[2], "federation/pools").await;
    let leader: Uuid = pool["items"][0]["leaderId"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let leader = usize::try_from(leader.as_u128() - 1).unwrap();
    if leader < 2 {
        nodes[leader].stop();
        timeout(Duration::from_secs(40), async {
            loop {
                let p = get(&client, &nodes[2], &cookies[2], "federation/pools").await;
                if p["items"][0]["leaderId"]
                    .as_str()
                    .is_some_and(|s| s != Uuid::from_u128(leader as u128 + 1).to_string())
                {
                    break;
                }
                sleep(Duration::from_secs(1)).await;
            }
        })
        .await
        .unwrap();
        nodes[leader].start();
        cookies[leader] = admin(&client, &nodes[leader]).await;
        let resumed = state(&client, &nodes[leader], &identities[leader].1, "playing").await;
        assert_eq!(resumed["matchId"], id);
        assert_eq!(
            get(&client, &nodes[2], &cookies[2], &room_path).await["createdAt"],
            started
        );
        eprintln!("non-host leader restarted; room preserved");
    } else {
        eprintln!("host elected leader; left host running");
    }
    let t = Instant::now();
    eprintln!("natural match running");
    timeout(Duration::from_secs(45 * 60), async {
        loop {
            let first = rpc(&client, &nodes[0], "update", json!([identities[0].1])).await;
            if first["returnValue"]["state"] == "post_match" {
                break;
            }
            sleep(Duration::from_secs(5)).await;
        }
    })
    .await
    .expect("natural match completion timed out");
    let mut first_result = None;
    for i in 0..2 {
        state(&client, &nodes[i], &identities[i].1, "post_match").await;
        let db = rusqlite::Connection::open_with_flags(
            &nodes[i].db,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let mut query=db.prepare("SELECT json_extract(body,'$.result') FROM events WHERE player=?1 AND json_extract(body,'$.type')='match_finished' AND json_extract(body,'$.matchId')=?2").unwrap();
        let results = query
            .query_map(rusqlite::params![identities[i].0.to_string(), id], |row| {
                row.get::<_, String>(0)
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(results.len(), 1);
        let result: Value = serde_json::from_str(&results[0]).unwrap();
        assert_eq!(result["reason"], "completed");
        assert!(result["winner"].as_u64().is_some_and(|team| team <= 1));
        assert!(result["ticks"].as_u64().is_some_and(|ticks| ticks > 0));
        let reached_five_minutes = result["ticks"].as_u64().unwrap() >= 300 * halcyon_sim::TICK_HZ;
        for (id, _) in &identities {
            let player = &result["players"][id.to_string()];
            assert!(player["damageToHeroes"].is_number());
            assert!(player["damageToStructures"].is_number());
            assert!(player["laneLastHits"].is_u64());
            assert_eq!(player["netWorthAtFive"].is_u64(), reached_five_minutes);
            assert_eq!(player["laneLastHitsAtFive"].is_u64(), reached_five_minutes);
        }
        if let Some(prior) = &first_result {
            assert_eq!(prior, &result);
        } else {
            first_result = Some(result);
        }
        rpc(
            &client,
            &nodes[i],
            "notifyExitPostMatch",
            json!([identities[i].1]),
        )
        .await;
        state(&client, &nodes[i], &identities[i].1, "menus").await;
    }
    eprintln!(
        "natural result on both homes after {}s",
        t.elapsed().as_secs()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "starts three local OS processes and restarts the active room host"]
async fn subprocess_lost_host_aborts_without_awarding_result() {
    let dir = tempfile::tempdir().unwrap();
    let RunningPool {
        mut nodes,
        client,
        identities,
        room_id,
        _drains,
        ..
    } = running_pool(dir.path()).await;
    nodes[2].stop();
    nodes[2].start();
    let _host_session = admin(&client, &nodes[2]).await;
    for i in 0..2 {
        state(&client, &nodes[i], &identities[i].1, "menus").await;
        let db = rusqlite::Connection::open_with_flags(
            &nodes[i].db,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let player = identities[i].0.to_string();
        let count = |kind: &str| -> i64 {
            db.query_row("SELECT count(*) FROM events WHERE player=?1 AND json_extract(body,'$.matchId')=?2 AND json_extract(body,'$.type')=?3",rusqlite::params![player,room_id,kind],|row|row.get(0)).unwrap()
        };
        assert_eq!(count("match_aborted"), 1);
        assert_eq!(count("match_finished"), 0);
        let progress: i64 = db
            .query_row(
                "SELECT count(*) FROM ce_completed WHERE player=? AND room=?",
                rusqlite::params![player, room_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(progress, 0);
    }
}
