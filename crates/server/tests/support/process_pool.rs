//! Run the subprocess scenarios with
//! `cargo test -p openhalcyon --test peer_play subprocess_ -- --ignored --nocapture --test-threads=1`.
//! The long scenario waits up to 45 minutes of real time for the simulated match to finish.
//! The supplemental data case also needs HALCYON_DELIVERY and HALCYON_DATA_EXTRA.
//! All credentials, pool certificates, binaries and SQLite files are scoped to a temporary directory.
use super::RpcEnvelope;
use super::{echo_clock, login, pool, until};
use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use halcyon_ce_platform::{LobbyState, RpcMethod};
use halcyon_protocol::wire_opcode;
use halcyon_protocol::{MatchCipher, Message};
use reqwest::{Certificate, Client};
use serde_json::Value;
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
async fn rpc(client: &Client, n: &Node, method: &str, args: impl serde::Serialize) -> Value {
    let r = client
        .post(format!("{}/JSONRpc/{method}", n.origin))
        .header("content-type", "application/x-www-form-urlencoded")
        .body(
            serde_json::to_string(&RpcEnvelope {
                method,
                params: args,
            })
            .unwrap(),
        )
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
        RpcMethod::CreateGuestPlayer.as_str(),
        (
            "kindred", "en", "US", "local", "en", "device", "android", "", "147219", 1, "model",
            "brand", "0",
        ),
    )
    .await;
    let s = rpc(
        client,
        n,
        RpcMethod::StartSessionForPlayer.as_str(),
        (&guest["returnValue"]["playerUUID"],),
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
async fn state(client: &Client, n: &Node, token: &Value, want: LobbyState) -> Value {
    let want = want.as_str();
    let mut last_phase = Value::Null;
    timeout(Duration::from_secs(90), async {
        loop {
            let r = rpc(client, n, RpcMethod::Update.as_str(), (&token,)).await;
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
                .json(
                    &serde_json::to_value(Credentials {
                        username: "operator",
                        password: "synthetic-test-password",
                    })
                    .unwrap(),
                )
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
#[derive(Clone, Copy)]
struct ServerData<'a> {
    base: &'a Path,
    extra: &'a Path,
}

fn spawn_nodes(dir: &Path, data: Option<[ServerData<'_>; 3]>) -> Vec<Node> {
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
        let values = PoolServerConfig {
            instance_id: f.node_id,
            public_host: "127.0.0.1",
            bootstrap_listen: bootstrap,
            platform_listen: web,
            notify_listen: notify,
            game_bind: "127.0.0.1",
            max_rooms: 8,
            seats_per_match: 2,
            database: "platform.sqlite",
            jwt_secret: "secret",
            certificate: &f.cert,
            private_key: &f.key,
            data_dir: data.map(|assets| assets[i].base),
            data_extra_dir: data.map(|assets| assets[i].extra),
            admin: PoolAdmin {
                origin: &origin,
                username: "operator",
                password_hash: &hash,
            },
            federation: PoolFederation {
                cluster_file: "pool.json",
                journal: "journal.db",
                latency_ms: BTreeMap::from([(cfg[2].node_id, 5)]),
                max_latency_ms: 50,
            },
        };
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
    running_pool_with_data(dir, None, None).await
}

async fn running_pool_with_data(
    dir: &Path,
    data: Option<[ServerData<'_>; 3]>,
    selection: Option<[u16; 2]>,
) -> RunningPool {
    let nodes = spawn_nodes(dir, data);
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
            RpcMethod::JoinLobby.as_str(),
            (
                &identities[i].1,
                LobbyJoin {
                    lobby: "5v5_pvp_casual",
                },
                "147219",
            ),
        )
        .await;
    }
    for i in 0..2 {
        state(
            &client,
            &nodes[i],
            &identities[i].1,
            LobbyState::MatchPending,
        )
        .await;
        rpc(
            &client,
            &nodes[i],
            RpcMethod::AcceptMatch.as_str(),
            (&identities[i].1,),
        )
        .await;
    }
    let a = state(&client, &nodes[0], &identities[0].1, LobbyState::Playing).await;
    let b = state(&client, &nodes[1], &identities[1].1, LobbyState::Playing).await;
    assert_eq!(a["matchId"], b["matchId"]);
    let id = a["matchId"].as_str().unwrap();
    let addr = format!("127.0.0.1:{}", a["port"]);
    assert_eq!(a["port"], b["port"]);
    let cipher = MatchCipher::new(id);
    let mut sockets = vec![];
    for (index, (player, _)) in identities.iter().enumerate() {
        let mut s = login(&addr, *player, &cipher).await;
        until(&mut s, &cipher, wire_opcode::Snapshot5V5).await;
        if let Some(heroes) = selection {
            let mut body = [0; 8];
            body[..4].copy_from_slice(&u32::from(heroes[index]).to_be_bytes());
            let choice = Message::new(wire_opcode::HeroSelection, body);
            s.write_all(&cipher.encode(&choice).unwrap()).await.unwrap();
            let echo = until(&mut s, &cipher, wire_opcode::HeroSelection).await;
            assert_eq!(echo.body[..8], choice.body);
        }
        s.write_all(
            &cipher
                .encode(&Message::new(wire_opcode::HeroLock, [0; 6]))
                .unwrap(),
        )
        .await
        .unwrap();
        until(&mut s, &cipher, wire_opcode::HeroLock).await;
        sockets.push(s);
    }
    for (index, (player, _)) in identities.iter().enumerate() {
        let mut s = login(&addr, *player, &cipher).await;
        if let Some(heroes) = selection {
            timeout(Duration::from_secs(10), async {
                loop {
                    let spawn = until(&mut s, &cipher, wire_opcode::EntitySpawn).await;
                    if matches!(spawn.fields(), Ok(halcyon_protocol::Fields::Spawn {template, entity:1500..=1501, ..})
                        if template == u32::from(heroes[index]))
                    {
                        break;
                    }
                }
            }).await.expect("selected human hero was not spawned by the third host");
        } else {
            until(&mut s, &cipher, wire_opcode::EntitySpawn).await;
        }
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

#[derive(serde::Deserialize)]
struct PoolList {
    items: Vec<ObservedPool>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ObservedPool {
    leader_id: Option<Uuid>,
}

async fn wait_for_leader(
    client: &Client,
    node: &Node,
    cookie: &str,
    previous: Option<Uuid>,
) -> Uuid {
    timeout(Duration::from_secs(40), async {
        loop {
            // The admin sampler may still expose its pre-election snapshot while rooms are live.
            let sample: PoolList =
                serde_json::from_value(get(client, node, cookie, "federation/pools").await)
                    .expect("valid federation pool response");
            if let Some(id) = sample.items.first().and_then(|pool| pool.leader_id)
                && Some(id) != previous
            {
                return id;
            }
            sleep(Duration::from_secs(1)).await;
        }
    })
    .await
    .expect("admin observer did not report an elected leader")
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
    let elected = wait_for_leader(&client, &nodes[2], &cookies[2], None).await;
    let leader = usize::try_from(elected.as_u128() - 1).unwrap();
    if leader < 2 {
        nodes[leader].stop();
        wait_for_leader(&client, &nodes[2], &cookies[2], Some(elected)).await;
        nodes[leader].start();
        cookies[leader] = admin(&client, &nodes[leader]).await;
        let resumed = state(
            &client,
            &nodes[leader],
            &identities[leader].1,
            LobbyState::Playing,
        )
        .await;
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
            let first = rpc(
                &client,
                &nodes[0],
                RpcMethod::Update.as_str(),
                (&identities[0].1,),
            )
            .await;
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
        state(&client, &nodes[i], &identities[i].1, LobbyState::PostMatch).await;
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
            RpcMethod::NotifyExitPostMatch.as_str(),
            (&identities[i].1,),
        )
        .await;
        state(&client, &nodes[i], &identities[i].1, LobbyState::Menus).await;
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
        state(&client, &nodes[i], &identities[i].1, LobbyState::Menus).await;
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

#[derive(serde::Serialize)]
struct LobbyJoin<'a> {
    lobby: &'a str,
}

#[derive(serde::Serialize)]
struct Credentials<'a> {
    username: &'a str,
    password: &'a str,
}

#[derive(serde::Serialize)]
struct PoolServerConfig<'a> {
    instance_id: Uuid,
    public_host: &'a str,
    bootstrap_listen: std::net::SocketAddr,
    platform_listen: std::net::SocketAddr,
    notify_listen: std::net::SocketAddr,
    game_bind: &'a str,
    max_rooms: u32,
    seats_per_match: u32,
    database: &'a str,
    jwt_secret: &'a str,
    certificate: &'a Path,
    private_key: &'a Path,
    #[serde(skip_serializing_if = "Option::is_none")]
    data_dir: Option<&'a Path>,
    #[serde(skip_serializing_if = "Option::is_none")]
    data_extra_dir: Option<&'a Path>,
    admin: PoolAdmin<'a>,
    federation: PoolFederation<'a>,
}
#[derive(serde::Serialize)]
struct PoolAdmin<'a> {
    origin: &'a str,
    username: &'a str,
    password_hash: &'a str,
}
#[derive(serde::Serialize)]
struct PoolFederation<'a> {
    cluster_file: &'a str,
    journal: &'a str,
    latency_ms: BTreeMap<Uuid, u32>,
    max_latency_ms: u32,
}

fn copy_data_tree(source: &Path, destination: &Path) {
    fs::create_dir(destination).unwrap();
    for item in fs::read_dir(source).unwrap() {
        let item = item.unwrap();
        let path = destination.join(item.file_name());
        let kind = item.file_type().unwrap();
        if kind.is_dir() {
            copy_data_tree(&item.path(), &path);
        } else if kind.is_file() {
            fs::copy(item.path(), path).unwrap();
        }
    }
}

fn read_only(path: &Path) -> rusqlite::Connection {
    let connection =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    connection.busy_timeout(Duration::from_secs(2)).unwrap();
    connection
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "starts three local OS processes using HALCYON_DELIVERY and HALCYON_DATA_EXTRA"]
async fn subprocess_supplement_rules_gate_cross_home_matching() {
    use halcyon_data::GameData;
    use std::collections::BTreeMap;

    let base = PathBuf::from(std::env::var_os("HALCYON_DELIVERY").unwrap())
        .canonicalize()
        .unwrap();
    let supplement = PathBuf::from(std::env::var_os("HALCYON_DATA_EXTRA").unwrap())
        .canonicalize()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let localized = directory.path().join("localized");
    let different = directory.path().join("different-rules");
    copy_data_tree(&supplement, &localized);
    copy_data_tree(&supplement, &different);

    let dictionary = localized.join("localization/en.json");
    let mut text: BTreeMap<String, String> =
        serde_json::from_slice(&fs::read(&dictionary).unwrap()).unwrap();
    text.insert(
        "RETRIEVE_CREDENTIALS_DESCRIPTION".into(),
        "Pool test translation".into(),
    );
    fs::write(dictionary, serde_json::to_vec(&text).unwrap()).unwrap();

    let units = different.join("units_full.json");
    let mut defs: Vec<Value> = serde_json::from_slice(&fs::read(&units).unwrap()).unwrap();
    let bear = defs
        .iter_mut()
        .find(|row| row["name"] == "5v5_BigBear")
        .unwrap();
    bear["gold_bounty"] = Value::from(bear["gold_bounty"].as_f64().unwrap() + 1.);
    fs::write(units, serde_json::to_vec(&defs).unwrap()).unwrap();

    let original = GameData::load_with_extra(&base, Some(&supplement)).unwrap();
    let translation = GameData::load_with_extra(&base, Some(&localized)).unwrap();
    let changed = GameData::load_with_extra(&base, Some(&different)).unwrap();
    let original_rules = halcyon_sim::rules_digest(Some(&original));
    let changed_rules = halcyon_sim::rules_digest(Some(&changed));
    assert_eq!(
        halcyon_sim::rules_digest(Some(&translation)),
        original_rules
    );
    assert_ne!(changed_rules, original_rules);
    assert!(
        changed.full_unit("5v5_BigBear").unwrap().gold_bounty
            > original.full_unit("5v5_BigBear").unwrap().gold_bounty
    );
    assert_ne!(
        translation.references().unwrap().digest,
        original.references().unwrap().digest
    );
    assert_eq!(
        changed.references().unwrap().digest,
        original.references().unwrap().digest
    );

    let fleet = directory.path().join("fleet");
    fs::create_dir(&fleet).unwrap();
    let common = ServerData {
        base: &base,
        extra: &supplement,
    };
    let RunningPool {
        mut nodes,
        client,
        cookies,
        room_id,
        _drains,
        ..
    } = running_pool_with_data(
        &fleet,
        Some([
            common,
            common,
            ServerData {
                base: &base,
                extra: &localized,
            },
        ]),
        Some([
            original.type_id("Idris").unwrap(),
            original.type_id("Lorelai").unwrap(),
        ]),
    )
    .await;

    let content = get(&client, &nodes[0], &cookies[0], "content").await;
    let host_content = get(&client, &nodes[2], &cookies[2], "content").await;
    assert_eq!(
        content["rulesContentDigest"],
        host_content["rulesContentDigest"]
    );
    assert_eq!(
        host_content["referenceDigest"],
        translation.references().unwrap().digest
    );
    assert_ne!(content["referenceDigest"], host_content["referenceDigest"]);
    assert!(!room_id.is_empty());

    // Change just one home's installed gameplay file while the same quorum,
    // host eligibility and established match stay intact.
    nodes[1].stop();
    let mut config: toml::Table = fs::read_to_string(&nodes[1].path).unwrap().parse().unwrap();
    config.insert(
        "data_extra_dir".into(),
        toml::Value::String(different.to_str().unwrap().into()),
    );
    fs::write(&nodes[1].path, toml::to_string(&config).unwrap()).unwrap();
    nodes[1].start();
    let replaced = admin(&client, &nodes[1]).await;
    let altered_content = get(&client, &nodes[1], &replaced, "content").await;
    assert_ne!(
        altered_content["rulesContentDigest"],
        content["rulesContentDigest"]
    );
    assert_eq!(
        altered_content["referenceDigest"],
        content["referenceDigest"]
    );

    let mut negative = Vec::new();
    for node in &nodes[..2] {
        let (id, token) = session(&client, node).await;
        rpc(
            &client,
            node,
            RpcMethod::JoinLobby.as_str(),
            (
                &token,
                LobbyJoin {
                    lobby: "5v5_pvp_casual",
                },
                "147219",
            ),
        )
        .await;
        negative.push((id, token));
    }
    let committed = timeout(Duration::from_secs(30), async {
        loop {
            let mut ids = Vec::new();
            for (node, (player, _)) in nodes.iter().zip(&negative) {
                let db = read_only(&node.db);
                let mut rows = db.prepare("SELECT ticket FROM federated_queue WHERE player=? AND phase='queued' AND match_id IS NULL").unwrap();
                if let Ok(ticket) = rows.query_row([player.to_string()], |row|row.get::<_,String>(0)) {
                    ids.push(ticket);
                }
            }
            if ids.len() == negative.len() {return ids;}
            sleep(Duration::from_millis(100)).await;
        }
    }).await.expect("both incompatible tickets must be committed and queued");

    for (index, id) in committed.iter().enumerate() {
        let journal = read_only(&nodes[index].path.with_file_name("journal.db"));
        let payload: String = journal
            .query_row(
                "SELECT payload FROM enqueue_payloads WHERE id=?",
                [id],
                |r| r.get(0),
            )
            .unwrap();
        let ticket: halcyon_matchmaking::Ticket = serde_json::from_str(&payload).unwrap();
        assert_eq!(
            ticket.queue.rules_digest.as_str(),
            if index == 0 {
                original_rules.as_str()
            } else {
                changed_rules.as_str()
            }
        );
        assert_eq!(ticket.latency_ms.len(), 1);
        assert_eq!(ticket.latency_ms.values().next(), Some(&5));
    }
    // Multiple worker cycles with both homes queued and the host still healthy.
    let until_time = Instant::now() + Duration::from_secs(3);
    while Instant::now() < until_time {
        for (node, (_, token)) in nodes.iter().zip(&negative) {
            let view = rpc(&client, node, RpcMethod::Update.as_str(), (token,)).await;
            assert_eq!(
                view["returnValue"]["state"],
                LobbyState::PendingAuto.as_str()
            );
        }
        sleep(Duration::from_millis(250)).await;
    }
    assert_eq!(
        get(
            &client,
            &nodes[2],
            &cookies[2],
            &format!("matches/{room_id}")
        )
        .await["phase"],
        "running"
    );
    let journal = read_only(&nodes[2].path.with_file_name("journal.db"));
    let how_many: i64 = journal
        .query_row("SELECT count(*) FROM hosted", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        how_many, 1,
        "incompatible homes must not allocate another room"
    );
}
