//! Run the subprocess scenarios with
//! `cargo test -p openhalcyon --test peer_play subprocess_ -- --ignored --nocapture --test-threads=1`.
//! The long scenario waits up to 45 minutes of real time for the simulated match to finish.
//! The supplemental data case also needs HALCYON_DELIVERY and HALCYON_DATA_EXTRA.
//! All credentials, pool certificates, binaries and SQLite files are scoped to a temporary directory.
use super::RpcEnvelope;
use super::{echo_clock, login, pool, until};
use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use halcyon_ce_platform::{LobbyState, RpcMethod};
use halcyon_federation::{MemberChange, Members, PoolAdmin as FederationAdmin};
use halcyon_protocol::{Fields, MatchCipher, Message, Opcode, wire_opcode};
use reqwest::{Certificate, Client};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::watch,
    time::{Instant, sleep, timeout},
};
use uuid::Uuid;

struct Node {
    id: Uuid,
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
async fn rpc_raw(client: &Client, n: &Node, method: &str, args: impl serde::Serialize) -> Value {
    client
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
        .unwrap()
}

async fn rpc(client: &Client, n: &Node, method: &str, args: impl serde::Serialize) -> Value {
    let r = rpc_raw(client, n, method, args).await;
    assert_eq!(r["code"], 0, "RPC {method}: {r}");
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
            id: f.node_id,
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

fn build_client(dir: &Path) -> Client {
    Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .add_root_certificate(
            Certificate::from_pem(&fs::read(dir.join("ca.pem")).unwrap()).unwrap(),
        )
        .build()
        .unwrap()
}

async fn start_cluster(
    dir: &Path,
    data: Option<[ServerData<'_>; 3]>,
) -> (Vec<Node>, Client, Vec<String>) {
    let nodes = spawn_nodes(dir, data);
    let client = build_client(dir);
    let mut cookies = Vec::with_capacity(nodes.len());
    for n in &nodes {
        cookies.push(admin(&client, n).await);
    }
    (nodes, client, cookies)
}

async fn connect_5v5_sockets(
    addr: &str,
    cipher: &MatchCipher,
    identities: &[(Uuid, Value)],
    selection: Option<[u16; 2]>,
) -> Vec<TcpStream> {
    let mut sockets = Vec::with_capacity(identities.len() * 2);
    for (index, (player, _)) in identities.iter().enumerate() {
        let mut s = login(addr, *player, cipher).await;
        until(&mut s, cipher, wire_opcode::Snapshot5V5).await;
        if let Some(heroes) = selection {
            let mut body = [0; 8];
            body[..4].copy_from_slice(&u32::from(heroes[index]).to_be_bytes());
            let choice = Message::new(wire_opcode::HeroSelection, body);
            s.write_all(&cipher.encode(&choice).unwrap()).await.unwrap();
            let echo = until(&mut s, cipher, wire_opcode::HeroSelection).await;
            assert_eq!(echo.body[..8], choice.body);
        }
        s.write_all(
            &cipher
                .encode(&Message::new(wire_opcode::HeroLock, [0; 6]))
                .unwrap(),
        )
        .await
        .unwrap();
        until(&mut s, cipher, wire_opcode::HeroLock).await;
        sockets.push(s);
    }
    for (index, (player, _)) in identities.iter().enumerate() {
        let mut s = login(addr, *player, cipher).await;
        if let Some(heroes) = selection {
            timeout(Duration::from_secs(10), async {
                loop {
                    let spawn = until(&mut s, cipher, wire_opcode::EntitySpawn).await;
                    if matches!(
                        spawn.fields(),
                        Ok(Fields::Spawn {
                            template,
                            entity: 1500..=1501,
                            ..
                        }) if template == u32::from(heroes[index])
                    ) {
                        break;
                    }
                }
            })
            .await
            .expect("selected human hero was not spawned by the third host");
        } else {
            until(&mut s, cipher, wire_opcode::EntitySpawn).await;
        }
        echo_clock(&mut s, cipher, 13.25).await;
        sockets.push(s);
    }
    sockets
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
    let (nodes, client, cookies) = start_cluster(dir, data).await;
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
    let sockets = connect_5v5_sockets(&addr, &cipher, &identities, selection).await;
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
    state: String,
    leader_id: Option<Uuid>,
}

#[derive(serde::Deserialize)]
struct AdminMatches {
    items: Vec<AdminMatchSummary>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdminMatchSummary {
    id: Uuid,
    phase: String,
    created_at: String,
    end_reason: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdminContent {
    rules_content_digest: Option<String>,
    reference_digest: Option<String>,
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
    write_mutated_bear_extra(&supplement, &different);

    let dictionary = localized.join("localization/en.json");
    let mut text: BTreeMap<String, String> =
        serde_json::from_slice(&fs::read(&dictionary).unwrap()).unwrap();
    text.insert(
        "RETRIEVE_CREDENTIALS_DESCRIPTION".into(),
        "Pool test translation".into(),
    );
    fs::write(dictionary, serde_json::to_vec(&text).unwrap()).unwrap();

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
    let replaced = update_data_extra_dir(&mut nodes[1], &client, &different).await;
    let altered_content = get(&client, &nodes[1], &replaced, "content").await;
    assert_ne!(
        altered_content["rulesContentDigest"],
        content["rulesContentDigest"]
    );
    assert_eq!(
        altered_content["referenceDigest"],
        content["referenceDigest"]
    );

    let negative = join_5v5_casual_on_homes(&client, &nodes[..2]).await;
    let committed = wait_committed_queued_tickets(&nodes[..2], &negative).await;

    for (index, id) in committed.iter().enumerate() {
        let ticket = read_frozen_ticket(&nodes[index], id);
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
    assert_stays_pending_auto(&client, &nodes[..2], &negative, Duration::from_secs(3)).await;
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
    assert_eq!(
        hosted_count(&nodes[2]),
        1,
        "incompatible homes must not allocate another room"
    );
}

fn write_mutated_bear_extra(supplement: &Path, destination: &Path) {
    copy_data_tree(supplement, destination);
    let units = destination.join("units_full.json");
    let mut defs: Vec<Value> = serde_json::from_slice(&fs::read(&units).unwrap()).unwrap();
    let bear = defs
        .iter_mut()
        .find(|row| row["name"] == "5v5_BigBear")
        .unwrap();
    bear["gold_bounty"] = Value::from(bear["gold_bounty"].as_f64().unwrap() + 1.);
    fs::write(units, serde_json::to_vec(&defs).unwrap()).unwrap();
}

async fn update_data_extra_dir(node: &mut Node, client: &Client, extra: &Path) -> String {
    node.stop();
    let mut config: toml::Table = fs::read_to_string(&node.path).unwrap().parse().unwrap();
    config.insert(
        "data_extra_dir".into(),
        toml::Value::String(extra.to_str().unwrap().into()),
    );
    fs::write(&node.path, toml::to_string(&config).unwrap()).unwrap();
    node.start();
    admin(client, node).await
}

async fn wait_committed_queued_tickets(
    home_nodes: &[Node],
    players: &[(Uuid, Value)],
) -> Vec<String> {
    timeout(Duration::from_secs(30), async {
        loop {
            let mut ids = Vec::new();
            for (node, (player, _)) in home_nodes.iter().zip(players) {
                let db = read_only(&node.db);
                let mut rows = db
                    .prepare("SELECT ticket FROM federated_queue WHERE player=? AND phase='queued' AND match_id IS NULL")
                    .unwrap();
                if let Ok(ticket) = rows.query_row([player.to_string()], |row| row.get::<_, String>(0)) {
                    ids.push(ticket);
                }
            }
            if ids.len() == players.len() {
                return ids;
            }
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("tickets must be committed and queued")
}

fn read_frozen_ticket(node: &Node, ticket_id: &str) -> halcyon_matchmaking::Ticket {
    let journal = read_only(&node.path.with_file_name("journal.db"));
    let payload: String = journal
        .query_row(
            "SELECT payload FROM enqueue_payloads WHERE id=?",
            [ticket_id],
            |r| r.get(0),
        )
        .unwrap();
    serde_json::from_str(&payload).unwrap()
}

async fn assert_stays_pending_auto(
    client: &Client,
    home_nodes: &[Node],
    players: &[(Uuid, Value)],
    duration: Duration,
) {
    let until_time = Instant::now() + duration;
    while Instant::now() < until_time {
        for (node, (_, token)) in home_nodes.iter().zip(players) {
            let view = rpc(client, node, RpcMethod::Update.as_str(), (token,)).await;
            assert_eq!(
                view["returnValue"]["state"],
                LobbyState::PendingAuto.as_str()
            );
        }
        sleep(Duration::from_millis(250)).await;
    }
}

fn hosted_count(host_node: &Node) -> i64 {
    let journal = read_only(&host_node.path.with_file_name("journal.db"));
    journal
        .query_row("SELECT count(*) FROM hosted", [], |r| r.get(0))
        .unwrap()
}

async fn join_5v5_casual_on_homes(client: &Client, home_nodes: &[Node]) -> Vec<(Uuid, Value)> {
    let mut players = Vec::with_capacity(home_nodes.len());
    for node in home_nodes {
        let (id, token) = session(client, node).await;
        rpc(
            client,
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
        players.push((id, token));
    }
    players
}

async fn wait_admin_pool_state(client: &Client, node: &Node, cookie: &str, expected: &str) {
    timeout(Duration::from_secs(12), async {
        loop {
            let status: PoolList =
                serde_json::from_value(get(client, node, cookie, "federation/pools").await)
                    .unwrap();
            if status.items.first().is_some_and(|p| p.state == expected) {
                break;
            }
            sleep(Duration::from_millis(500)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("admin federation status did not report {expected}"));
}

async fn wait_hosted_delivered(host_node: &Node, room_id: &str, within: Duration) {
    timeout(within, async {
        loop {
            if let Some((Some(_), 1)) = read_hosted_row(host_node, room_id) {
                break;
            }
            sleep(Duration::from_millis(150)).await;
        }
    })
    .await
    .expect("host outbox did not mark room settlement as delivered");
}

fn read_hosted_row(host_node: &Node, room_id: &str) -> Option<(Option<String>, i64)> {
    let journal = read_only(&host_node.path.with_file_name("journal.db"));
    journal
        .query_row(
            "SELECT result, delivered FROM hosted WHERE id=?",
            [room_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok()
}

async fn admin_matches(client: &Client, node: &Node, cookie: &str) -> AdminMatches {
    serde_json::from_value(get(client, node, cookie, "matches").await).unwrap()
}

async fn admin_match_detail(
    client: &Client,
    node: &Node,
    cookie: &str,
    room_id: &str,
) -> AdminMatchSummary {
    serde_json::from_value(get(client, node, cookie, &format!("matches/{room_id}")).await).unwrap()
}

async fn admin_content(client: &Client, node: &Node, cookie: &str) -> AdminContent {
    serde_json::from_value(get(client, node, cookie, "content").await).unwrap()
}

fn pool_admin(node: &Node) -> FederationAdmin {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let cfg = halcyon_federation::Config::read(&node.path.with_file_name("pool.json")).unwrap();
    FederationAdmin::new(cfg).unwrap()
}

async fn wait_raft_members(probe: &Node, previous_leader: Option<Uuid>) -> Members {
    let admin = pool_admin(probe);
    timeout(Duration::from_secs(45), async {
        loop {
            if let Ok(members) = admin.members().await
                && Some(members.leader) != previous_leader
            {
                return members;
            }
            sleep(Duration::from_millis(150)).await;
        }
    })
    .await
    .expect("raft cluster did not elect a leader in time")
}

async fn ensure_non_host_leader(nodes: &[Node], host_id: Uuid) -> usize {
    let all: BTreeSet<Uuid> = nodes.iter().map(|n| n.id).collect();
    let homes: BTreeSet<Uuid> = nodes[..2].iter().map(|n| n.id).collect();
    let admin = pool_admin(&nodes[0]);
    timeout(Duration::from_secs(45), async {
        loop {
            if let Ok(members) = admin.members().await {
                if members.configurations == vec![all.clone()] && members.leader != host_id {
                    return nodes.iter().position(|n| n.id == members.leader).unwrap();
                }
                if members.configurations == vec![all.clone()] && members.leader == host_id {
                    let _ = admin
                        .change(&MemberChange {
                            expected: all.clone(),
                            voters: homes.clone(),
                        })
                        .await;
                } else if members.configurations.len() == 1 && members.configurations[0] == homes {
                    let _ = admin
                        .change(&MemberChange {
                            expected: homes.clone(),
                            voters: all.clone(),
                        })
                        .await;
                }
            }
            sleep(Duration::from_millis(150)).await;
        }
    })
    .await
    .expect("failed to establish a non-host Raft leader")
}

struct LiveClockPumps {
    stop: watch::Sender<bool>,
    counts: Vec<Arc<AtomicU64>>,
    tasks: tokio::task::JoinSet<()>,
}

impl LiveClockPumps {
    fn start(sockets: Vec<TcpStream>, room_id: &str) -> Self {
        let (stop, _) = watch::channel(false);
        let mut counts = Vec::with_capacity(sockets.len());
        let mut tasks = tokio::task::JoinSet::new();
        for mut socket in sockets {
            let cipher = MatchCipher::new(room_id);
            let mut stopped = stop.subscribe();
            let count = Arc::new(AtomicU64::new(0));
            counts.push(count.clone());
            tasks.spawn(async move {
                let mut seq = 1u32;
                loop {
                    if *stopped.borrow() {
                        break;
                    }
                    let mut body = [0; 6];
                    body[..4].copy_from_slice(&(100.0 + seq as f32).to_be_bytes());
                    seq += 1;
                    let frame = cipher
                        .encode(&Message::known(Opcode::ClockSync, body))
                        .unwrap();
                    timeout(Duration::from_secs(2), socket.write_all(&frame))
                        .await
                        .expect("ClockSync write timed out")
                        .expect("ClockSync write failed");
                    timeout(Duration::from_secs(4), async {
                        loop {
                            let len = socket.read_u16().await.expect("game socket closed");
                            let mut packet = vec![0; len as usize];
                            socket
                                .read_exact(&mut packet)
                                .await
                                .expect("game socket truncated");
                            let decoded = cipher.decode(&packet).unwrap();
                            if decoded.is(Opcode::ClockSync) && decoded.body == body {
                                break;
                            }
                        }
                    })
                    .await
                    .expect("ClockSync echo timed out on live socket");
                    count.fetch_add(1, Ordering::Relaxed);
                    tokio::select! {
                        _ = stopped.changed() => break,
                        _ = sleep(Duration::from_millis(200)) => {}
                    }
                }
            });
        }
        Self {
            stop,
            counts,
            tasks,
        }
    }

    async fn assert_all_echoes_advance(&self, min_delta: u64, within: Duration, phase: &str) {
        let start: Vec<u64> = self
            .counts
            .iter()
            .map(|c| c.load(Ordering::Relaxed))
            .collect();
        timeout(within, async {
            loop {
                if self
                    .counts
                    .iter()
                    .zip(&start)
                    .all(|(c, s)| c.load(Ordering::Relaxed) >= s + min_delta)
                {
                    return;
                }
                sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("ClockSync echoes stalled {phase}"));
    }

    async fn stop_and_verify(mut self) {
        self.stop.send_replace(true);
        while let Some(result) = self.tasks.join_next().await {
            result.expect("live socket ClockSync task failed");
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "three local OS processes; deterministic leader failover in Preparing and Running plus minority quorum loss"]
async fn subprocess_leader_failover_during_prepare_and_running_preserves_room() {
    let dir = tempfile::tempdir().unwrap();
    let (mut nodes, client, mut cookies) = start_cluster(dir.path(), None).await;
    let host_id = nodes[2].id;
    let initial_leader = ensure_non_host_leader(&nodes, host_id).await;

    let identities = join_5v5_casual_on_homes(&client, &nodes[..2]).await;
    for i in 0..2 {
        state(
            &client,
            &nodes[i],
            &identities[i].1,
            LobbyState::MatchPending,
        )
        .await;
    }

    // First home accepts while second waits: allocation stays in Preparing.
    rpc(
        &client,
        &nodes[initial_leader],
        RpcMethod::AcceptMatch.as_str(),
        (&identities[initial_leader].1,),
    )
    .await;
    let prepared = timeout(Duration::from_secs(20), async {
        loop {
            let list = admin_matches(&client, &nodes[2], &cookies[2]).await;
            if let Some(room) = list.items.into_iter().next()
                && hosted_count(&nodes[2]) == 1
            {
                return room;
            }
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("host did not prepare room during Preparing phase");
    assert_eq!(prepared.phase, "allocated");
    let room_id = prepared.id.to_string();
    let initial_created = prepared.created_at;
    for i in 0..2 {
        assert!(
            admin_matches(&client, &nodes[i], &cookies[i])
                .await
                .items
                .is_empty()
        );
    }

    // Failover 1: restart the elected non-host Raft leader during Preparing.
    let initial_leader_id = nodes[initial_leader].id;
    nodes[initial_leader].stop();
    let after_prepare = wait_raft_members(&nodes[2], Some(initial_leader_id)).await;
    assert_ne!(after_prepare.leader, initial_leader_id);
    nodes[initial_leader].start();
    cookies[initial_leader] = admin(&client, &nodes[initial_leader]).await;
    let running_leader = ensure_non_host_leader(&nodes, host_id).await;

    let still_preparing = admin_match_detail(&client, &nodes[2], &cookies[2], &room_id).await;
    assert_eq!(still_preparing.phase, "allocated");
    assert_eq!(still_preparing.created_at, initial_created);
    assert_eq!(hosted_count(&nodes[2]), 1);

    // Second home accepts and the single prepared room opens for play.
    let other_home = 1 - initial_leader;
    rpc(
        &client,
        &nodes[other_home],
        RpcMethod::AcceptMatch.as_str(),
        (&identities[other_home].1,),
    )
    .await;
    let a = state(&client, &nodes[0], &identities[0].1, LobbyState::Playing).await;
    let b = state(&client, &nodes[1], &identities[1].1, LobbyState::Playing).await;
    assert_eq!(a["matchId"], room_id);
    assert_eq!(b["matchId"], room_id);
    assert_eq!(a["port"], b["port"]);
    let addr = format!("127.0.0.1:{}", a["port"]);
    let cipher = MatchCipher::new(&room_id);
    let sockets = connect_5v5_sockets(&addr, &cipher, &identities, None).await;
    let detail = admin_match_detail(&client, &nodes[2], &cookies[2], &room_id).await;
    assert_eq!(detail.phase, "running");
    assert_eq!(detail.created_at, initial_created);
    assert_eq!(hosted_count(&nodes[2]), 1);

    let pumps = LiveClockPumps::start(sockets, &room_id);
    pumps
        .assert_all_echoes_advance(2, Duration::from_secs(5), "before mid-match failover")
        .await;

    // Failover 2: restart the elected non-host Raft leader mid-match during Running.
    let running_leader_id = nodes[running_leader].id;
    nodes[running_leader].stop();
    let during_running = wait_raft_members(&nodes[2], Some(running_leader_id)).await;
    assert_ne!(during_running.leader, running_leader_id);
    pumps
        .assert_all_echoes_advance(
            2,
            Duration::from_secs(6),
            "during mid-match leader failover",
        )
        .await;
    nodes[running_leader].start();
    cookies[running_leader] = admin(&client, &nodes[running_leader]).await;
    pumps
        .assert_all_echoes_advance(2, Duration::from_secs(5), "after mid-match leader restart")
        .await;
    let after_running_failover =
        admin_match_detail(&client, &nodes[2], &cookies[2], &room_id).await;
    assert_eq!(after_running_failover.phase, "running");
    assert_eq!(after_running_failover.created_at, initial_created);
    for i in 0..2 {
        let resumed = state(&client, &nodes[i], &identities[i].1, LobbyState::Playing).await;
        assert_eq!(resumed["matchId"], room_id);
    }

    // Minority / quorum-loss: stop both non-host nodes while the room runs.
    nodes[0].stop();
    nodes[1].stop();
    pumps
        .assert_all_echoes_advance(6, Duration::from_secs(6), "during minority quorum loss")
        .await;
    wait_admin_pool_state(&client, &nodes[2], &cookies[2], "no_quorum").await;

    // New queue attempts must fail/reject when Raft quorum is lost.
    let (_minority_player, minority_token) = session(&client, &nodes[2]).await;
    let rejected = rpc_raw(
        &client,
        &nodes[2],
        RpcMethod::JoinLobby.as_str(),
        (
            &minority_token,
            LobbyJoin {
                lobby: "5v5_pvp_casual",
            },
            "147219",
        ),
    )
    .await;
    assert_ne!(
        rejected["code"], 0,
        "queue enqueue must reject without Raft quorum: {rejected}"
    );
    assert_eq!(hosted_count(&nodes[2]), 1);

    // Recover quorum: restart both non-host nodes and verify continuous room + queue recovery.
    nodes[0].start();
    nodes[1].start();
    cookies[0] = admin(&client, &nodes[0]).await;
    cookies[1] = admin(&client, &nodes[1]).await;
    let recovered = wait_raft_members(&nodes[2], None).await;
    assert_eq!(recovered.configurations.len(), 1);
    assert_eq!(recovered.configurations[0].len(), 3);
    wait_admin_pool_state(&client, &nodes[2], &cookies[2], "online").await;
    pumps
        .assert_all_echoes_advance(3, Duration::from_secs(5), "after quorum recovery")
        .await;

    // Clear the aborted minority attempt, then verify new cross-home queuing recovers.
    rpc(
        &client,
        &nodes[2],
        RpcMethod::ExitLobby.as_str(),
        (&minority_token,),
    )
    .await;
    state(&client, &nodes[2], &minority_token, LobbyState::Menus).await;

    let recovered_players = join_5v5_casual_on_homes(&client, &nodes[..2]).await;
    for (n, (_, token)) in nodes[..2].iter().zip(&recovered_players) {
        state(&client, n, token, LobbyState::MatchPending).await;
        rpc(&client, n, RpcMethod::AcceptMatch.as_str(), (token,)).await;
    }
    let second_a = state(
        &client,
        &nodes[0],
        &recovered_players[0].1,
        LobbyState::Playing,
    )
    .await;
    let second_b = state(
        &client,
        &nodes[1],
        &recovered_players[1].1,
        LobbyState::Playing,
    )
    .await;
    assert_eq!(second_a["matchId"], second_b["matchId"]);
    assert_ne!(second_a["matchId"], room_id);
    assert_eq!(hosted_count(&nodes[2]), 2);

    pumps.stop_and_verify().await;
}

struct AralSeatSocket {
    home: usize,
    player_id: Uuid,
    token: Value,
    seat: usize,
    actor: u32,
    wire_team: u8,
    socket: TcpStream,
}

struct RunningAral {
    nodes: Vec<Node>,
    client: Client,
    cookies: Vec<String>,
    room_id: String,
    cipher: MatchCipher,
    seats: Vec<AralSeatSocket>,
}

async fn next_frame(socket: &mut TcpStream, cipher: &MatchCipher, limit: Duration) -> Message {
    timeout(limit, async {
        let len = socket.read_u16().await.unwrap();
        let mut body = vec![0; len as usize];
        socket.read_exact(&mut body).await.unwrap();
        cipher.decode(&body).unwrap()
    })
    .await
    .expect("timed out waiting for ARAL game frame")
}

async fn until_within(
    socket: &mut TcpStream,
    cipher: &MatchCipher,
    opcode: u16,
    limit: Duration,
) -> Message {
    timeout(limit, async {
        loop {
            let len = socket.read_u16().await.unwrap();
            let mut body = vec![0; len as usize];
            socket.read_exact(&mut body).await.unwrap();
            let m = cipher.decode(&body).unwrap();
            if m.opcode == opcode {
                return m;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for opcode {opcode:#x}"))
}

async fn start_aral_match(dir: &Path) -> RunningAral {
    let (nodes, client, cookies) = start_cluster(dir, None).await;
    let mut sessions = Vec::with_capacity(6);
    for index in 0..6 {
        let home = index % 2;
        let (player_id, token) = session(&client, &nodes[home]).await;
        rpc(
            &client,
            &nodes[home],
            RpcMethod::JoinLobby.as_str(),
            (
                &token,
                LobbyJoin {
                    lobby: "casual_aral",
                },
                "147219",
            ),
        )
        .await;
        sessions.push((home, player_id, token));
    }
    for (home, _, token) in &sessions {
        state(&client, &nodes[*home], token, LobbyState::MatchedPartners).await;
        rpc(
            &client,
            &nodes[*home],
            RpcMethod::AcceptMatch.as_str(),
            (token,),
        )
        .await;
    }

    let mut room_id: Option<String> = None;
    let mut addr: Option<String> = None;
    let mut pre_sockets = Vec::with_capacity(6);
    for (home, player_id, token) in sessions {
        let playing = state(&client, &nodes[home], &token, LobbyState::Playing).await;
        let current_id = playing["matchId"].as_str().unwrap().to_owned();
        let current_addr = format!("127.0.0.1:{}", playing["port"]);
        if let Some(expected_id) = &room_id {
            assert_eq!(expected_id, &current_id);
            assert_eq!(addr.as_ref().unwrap(), &current_addr);
        } else {
            room_id = Some(current_id);
            addr = Some(current_addr);
        }
        let cipher = MatchCipher::new(room_id.as_deref().unwrap());
        let mut socket = login(addr.as_deref().unwrap(), player_id, &cipher).await;
        let roster_msg = until(&mut socket, &cipher, wire_opcode::SnapshotAral).await;
        let Fields::AralRoster(roster) = roster_msg.fields().unwrap() else {
            panic!("expected SnapshotAral roster");
        };
        let seat = (0..6)
            .find(|&s| roster.identity(s) == Some(player_id.to_string().as_bytes()))
            .expect("player missing from SnapshotAral");
        let slot = roster.slot(seat).unwrap();
        pre_sockets.push(AralSeatSocket {
            home,
            player_id,
            token,
            seat,
            actor: 1500 + seat as u32,
            wire_team: slot.team,
            socket,
        });
    }
    let room_id = room_id.unwrap();
    let cipher = MatchCipher::new(&room_id);

    // Drain the 20-second ARAL draft timer concurrently across all 6 seats.
    let mut loading = tokio::task::JoinSet::new();
    for mut seat_sock in pre_sockets.drain(..) {
        let task_cipher = MatchCipher::new(&room_id);
        loading.spawn(async move {
            timeout(Duration::from_secs(35), async {
                while next_frame(&mut seat_sock.socket, &task_cipher, Duration::from_secs(4))
                    .await
                    .opcode
                    != wire_opcode::BeginLoading
                {}
            })
            .await
            .expect("ARAL draft did not enter BeginLoading");
            seat_sock
        });
    }
    let mut seats = Vec::with_capacity(6);
    while let Some(res) = loading.join_next().await {
        seats.push(res.unwrap());
    }
    seats.sort_by_key(|s| s.seat);

    for s in &mut seats {
        s.socket
            .write_all(
                &cipher
                    .encode(&Message::new(wire_opcode::RequestScene, [0; 6]))
                    .unwrap(),
            )
            .await
            .unwrap();
        let mut ready = [0; 6];
        ready[..4].copy_from_slice(&s.actor.to_be_bytes());
        ready[4] = 1;
        s.socket
            .write_all(
                &cipher
                    .encode(&Message::new(wire_opcode::SceneOwner, ready))
                    .unwrap(),
            )
            .await
            .unwrap();
        until(&mut s.socket, &cipher, wire_opcode::SceneOwner).await;
        s.socket
            .write_all(
                &cipher
                    .encode(&Message::new(wire_opcode::SceneConfirmed, [0; 6]))
                    .unwrap(),
            )
            .await
            .unwrap();
        until(&mut s.socket, &cipher, wire_opcode::SceneConfirmed).await;
    }
    for s in &mut seats {
        until(&mut s.socket, &cipher, wire_opcode::TickSyncA).await;
    }
    assert_eq!(
        admin_match_detail(&client, &nodes[2], &cookies[2], &room_id)
            .await
            .phase,
        "running"
    );
    // Let the 20 Hz simulation advance a few ticks before voting to surrender.
    sleep(Duration::from_millis(300)).await;
    echo_clock(&mut seats[0].socket, &cipher, 1.0).await;
    RunningAral {
        nodes,
        client,
        cookies,
        room_id,
        cipher,
        seats,
    }
}

async fn trigger_aral_surrender(
    seats: &mut [AralSeatSocket],
    cipher: &MatchCipher,
    loser_wire_team: u8,
) {
    let losing_indices: Vec<usize> = seats
        .iter()
        .enumerate()
        .filter_map(|(i, s)| (s.wire_team == loser_wire_team).then_some(i))
        .collect();
    assert_eq!(losing_indices.len(), 3);
    let vote_frame = cipher.encode(&Message::aral_surrender_request()).unwrap();
    for &idx in &losing_indices {
        seats[idx].socket.write_all(&vote_frame).await.unwrap();
    }
    for &idx in &losing_indices {
        timeout(Duration::from_secs(6), async {
            loop {
                let msg = next_frame(&mut seats[idx].socket, cipher, Duration::from_secs(6)).await;
                if msg.opcode == wire_opcode::TeamSurrenderStateChanged
                    && msg.body.len() >= 151
                    && msg.body[148..151] == [loser_wire_team, 2, 3]
                    && msg.body[68..71].iter().filter(|&&v| v == 1).count() == 3
                {
                    break;
                }
            }
        })
        .await
        .expect("teammate did not receive passed surrender board");
    }
}

async fn drain_aral_completion(
    seats: Vec<AralSeatSocket>,
    room_id: &str,
) -> Vec<(usize, Uuid, Value, u8)> {
    let mut finishing = tokio::task::JoinSet::new();
    for mut s in seats {
        let task_cipher = MatchCipher::new(room_id);
        finishing.spawn(async move {
            let report = until_within(
                &mut s.socket,
                &task_cipher,
                wire_opcode::StatisticsReport,
                Duration::from_secs(25),
            )
            .await;
            assert!(matches!(report.fields(), Ok(Fields::MatchStatistics(_))));
            let victory = until_within(
                &mut s.socket,
                &task_cipher,
                wire_opcode::TickSyncC,
                Duration::from_secs(5),
            )
            .await;
            assert_eq!(victory.body.as_slice(), &[0, 0, 0, 1, 2, 0]);
            (s.home, s.player_id, s.token, s.wire_team)
        });
    }
    let mut participants = Vec::with_capacity(6);
    while let Some(res) = finishing.join_next().await {
        participants.push(res.unwrap());
    }
    participants
}

async fn verify_aral_settlement(
    client: &Client,
    nodes: &[Node],
    participants: &[(usize, Uuid, Value, u8)],
    room_id: &str,
    expected_result: Option<&Value>,
) {
    let mut settled_result: Option<Value> = expected_result.cloned();
    for (home, player_id, token, wire_team) in participants {
        state(client, &nodes[*home], token, LobbyState::PostMatch).await;
        let db = read_only(&nodes[*home].db);
        let player_str = player_id.to_string();
        let mut query = db
            .prepare(
                "SELECT json_extract(body,'$.result') FROM events
                 WHERE player=?1 AND json_extract(body,'$.type')='match_finished'
                   AND json_extract(body,'$.matchId')=?2",
            )
            .unwrap();
        let results: Vec<String> = query
            .query_map(rusqlite::params![player_str, room_id], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(results.len(), 1, "expected exactly 1 match_finished event");
        let aborted: i64 = db
            .query_row(
                "SELECT count(*) FROM events
                 WHERE player=?1 AND json_extract(body,'$.type')='match_aborted'
                   AND json_extract(body,'$.matchId')=?2",
                rusqlite::params![player_str, room_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(aborted, 0, "unexpected match_aborted event");
        let result: Value = serde_json::from_str(&results[0]).unwrap();
        assert_eq!(result["reason"], "completed");
        assert_eq!(result["winner"], 0);
        assert_eq!(result["surrenderedTeam"], 1);
        assert!(result["ticks"].as_u64().is_some_and(|t| t > 0));
        if let Some(expected) = &settled_result {
            assert_eq!(expected, &result);
        } else {
            settled_result = Some(result);
        }
        let won: i64 = db
            .query_row(
                "SELECT won FROM ce_completed WHERE player=?1 AND room=?2",
                rusqlite::params![player_str, room_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(won, i64::from(*wire_team == 1));

        rpc(
            client,
            &nodes[*home],
            RpcMethod::NotifyExitPostMatch.as_str(),
            (token,),
        )
        .await;
        state(client, &nodes[*home], token, LobbyState::Menus).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "three local OS processes; fast 6-seat ARAL surrender settles once on both homes"]
async fn subprocess_aral_surrender_settles_on_both_homes() {
    let dir = tempfile::tempdir().unwrap();
    let RunningAral {
        nodes,
        client,
        cookies,
        room_id,
        cipher,
        mut seats,
    } = start_aral_match(dir.path()).await;

    trigger_aral_surrender(&mut seats, &cipher, 2).await;
    let participants = drain_aral_completion(seats, &room_id).await;
    wait_hosted_delivered(&nodes[2], &room_id, Duration::from_secs(15)).await;

    verify_aral_settlement(&client, &nodes, &participants, &room_id, None).await;
    let detail = admin_match_detail(&client, &nodes[2], &cookies[2], &room_id).await;
    assert_eq!(detail.phase, "ended");
    assert_eq!(detail.end_reason.as_deref(), Some("completed"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "three local OS processes; verifies Diesel peer journal outbox durability across quorum loss"]
async fn subprocess_settlement_outbox_persists_across_quorum_loss() {
    let dir = tempfile::tempdir().unwrap();
    let RunningAral {
        mut nodes,
        client,
        mut cookies,
        room_id,
        cipher,
        mut seats,
    } = start_aral_match(dir.path()).await;

    trigger_aral_surrender(&mut seats, &cipher, 2).await;
    let ending = admin_match_detail(&client, &nodes[2], &cookies[2], &room_id).await;
    assert_eq!(ending.phase, "ending");

    // Lose Raft quorum during the 15s surrender outro right before match finish.
    nodes[0].stop();
    nodes[1].stop();

    let participants = drain_aral_completion(seats, &room_id).await;
    let outbox_raw = timeout(Duration::from_secs(10), async {
        loop {
            if let Some((Some(result), delivered)) = read_hosted_row(&nodes[2], &room_id) {
                assert_eq!(
                    delivered, 0,
                    "cannot deliver settlement without Raft quorum"
                );
                return result;
            }
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("host did not persist finished result in Diesel peer journal");
    let outbox_result: Value = serde_json::from_str(&outbox_raw).unwrap();
    assert_eq!(outbox_result["reason"], "completed");
    assert_eq!(outbox_result["winner"], 0);
    assert_eq!(outbox_result["surrenderedTeam"], 1);

    // Stay in minority across multiple 300ms worker cycles to ensure outbox holds durability.
    sleep(Duration::from_millis(1500)).await;
    assert_eq!(
        read_hosted_row(&nodes[2], &room_id),
        Some((Some(outbox_raw), 0))
    );

    // Restore quorum by restarting both non-host homes (host never restarted).
    nodes[0].start();
    nodes[1].start();
    cookies[0] = admin(&client, &nodes[0]).await;
    cookies[1] = admin(&client, &nodes[1]).await;
    wait_hosted_delivered(&nodes[2], &room_id, Duration::from_secs(20)).await;

    verify_aral_settlement(
        &client,
        &nodes,
        &participants,
        &room_id,
        Some(&outbox_result),
    )
    .await;
    let detail = admin_match_detail(&client, &nodes[2], &cookies[2], &room_id).await;
    assert_eq!(detail.phase, "ended");
    assert_eq!(detail.end_reason.as_deref(), Some("completed"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "three local OS processes using HALCYON_DELIVERY and HALCYON_DATA_EXTRA"]
async fn subprocess_host_rules_mismatch_blocks_allocation() {
    use halcyon_data::GameData;

    let base = PathBuf::from(std::env::var_os("HALCYON_DELIVERY").unwrap())
        .canonicalize()
        .unwrap();
    let supplement = PathBuf::from(std::env::var_os("HALCYON_DATA_EXTRA").unwrap())
        .canonicalize()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let different = directory.path().join("different-host-rules");
    write_mutated_bear_extra(&supplement, &different);

    let original = GameData::load_with_extra(&base, Some(&supplement)).unwrap();
    let changed = GameData::load_with_extra(&base, Some(&different)).unwrap();
    let original_rules = halcyon_sim::rules_digest(Some(&original));
    let changed_rules = halcyon_sim::rules_digest(Some(&changed));
    assert_ne!(changed_rules, original_rules);
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
    let (mut nodes, client, mut cookies) = start_cluster(
        &fleet,
        Some([
            common,
            common,
            ServerData {
                base: &base,
                extra: &different,
            },
        ]),
    )
    .await;

    let home0 = admin_content(&client, &nodes[0], &cookies[0]).await;
    let home1 = admin_content(&client, &nodes[1], &cookies[1]).await;
    let host = admin_content(&client, &nodes[2], &cookies[2]).await;
    assert_eq!(home0.rules_content_digest, home1.rules_content_digest);
    assert_eq!(home0.reference_digest, host.reference_digest);
    assert_ne!(host.rules_content_digest, home0.rules_content_digest);

    let queued = join_5v5_casual_on_homes(&client, &nodes[..2]).await;
    let committed = wait_committed_queued_tickets(&nodes[..2], &queued).await;
    let first_ticket = read_frozen_ticket(&nodes[0], &committed[0]);
    let second_ticket = read_frozen_ticket(&nodes[1], &committed[1]);
    assert_eq!(first_ticket.queue, second_ticket.queue);
    assert_eq!(first_ticket.queue.rules_digest, original_rules);
    assert_eq!(first_ticket.latency_ms, BTreeMap::from([(nodes[2].id, 5)]));

    // Compatible homes remain queued while the rules-mismatched host is never allocated.
    assert_stays_pending_auto(&client, &nodes[..2], &queued, Duration::from_secs(3)).await;
    assert_eq!(hosted_count(&nodes[2]), 0);
    assert!(
        admin_matches(&client, &nodes[2], &cookies[2])
            .await
            .items
            .is_empty()
    );

    // Aligning only the host's rules immediately lets the exact queued tickets allocate.
    cookies[2] = update_data_extra_dir(&mut nodes[2], &client, &supplement).await;
    let aligned_host = admin_content(&client, &nodes[2], &cookies[2]).await;
    assert_eq!(
        aligned_host.rules_content_digest,
        home0.rules_content_digest
    );

    for (node, (_, token)) in nodes[..2].iter().zip(&queued) {
        state(&client, node, token, LobbyState::MatchPending).await;
        rpc(&client, node, RpcMethod::AcceptMatch.as_str(), (token,)).await;
    }
    let a = state(&client, &nodes[0], &queued[0].1, LobbyState::Playing).await;
    let b = state(&client, &nodes[1], &queued[1].1, LobbyState::Playing).await;
    assert_eq!(a["matchId"], b["matchId"]);
    assert_eq!(hosted_count(&nodes[2]), 1);
}
