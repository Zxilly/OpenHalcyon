//! Independent CE load client. All mutation and traffic are through public client sockets.
use anyhow::{Context, Result, ensure};
use clap::Parser;
use halcyon_protocol::{
    ACK_FRAME, MatchCipher, Message, async_frame::FrameReader, hello_with_port,
};
use halcyon_protocol::{
    Opcode,
    ce::{KnownQueue, LobbyState, RpcMethod},
};
use reqwest::{Certificate, Client};
use serde::{
    Deserialize, Serialize,
    de::{DeserializeOwned, IgnoredAny},
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::AsyncWriteExt,
    net::TcpStream,
    sync::watch,
    task::JoinSet,
    time::{Instant, interval, sleep, timeout},
};
use uuid::Uuid;

#[derive(Parser)]
struct Args {
    /// HTTPS platform URL of the benchmark instance.
    #[arg(long)]
    platform: String,
    #[arg(long)]
    certificate: PathBuf,
    #[arg(long)]
    rooms: usize,
    #[arg(long)]
    humans: usize,
    #[arg(long)]
    seconds: u64,
    /// Spread the initial room groups evenly across this many seconds.
    #[arg(long, default_value_t = 0)]
    ramp_seconds: u64,
    /// Delay each game frame for the last human seat in each room (zero for normal reading).
    #[arg(long, default_value_t = 0)]
    slow_reader_ms: u64,
    /// Play one native Solo control stream with nine server bots per room.
    #[arg(long, conflicts_with = "native_aral")]
    native_solo: bool,
    /// Play six native human seats on the ARAL control streams.
    #[arg(long)]
    native_aral: bool,
    /// The operator is deliberately terminating rooms via the admin API.
    #[arg(long)]
    allow_admin_rotation: bool,
}
#[derive(Serialize)]
struct Sample {
    elapsed_s: f64,
    steady_s: Option<f64>,
    opened: u64,
    initial_rooms: u64,
    completed: u64,
    slow_disconnects: u64,
    clocks_sent: u64,
    clock_echoes: u64,
    clock_sequence_errors: u64,
    end_frames: u64,
    damage_frames: u64,
    heal_frames: u64,
    death_frames: u64,
    hero_kill_increments: u64,
    hero_death_increments: u64,
    frames: u64,
    gs_bytes: u64,
    positions: u64,
    client_commands: u64,
}
#[derive(Default)]
struct Stats {
    frames: AtomicU64,
    bytes: AtomicU64,
    writes: AtomicU64,
    positions: AtomicU64,
    opened: AtomicU64,
    initial_rooms: AtomicU64,
    completed: AtomicU64,
    slow_disconnects: AtomicU64,
    clocks_sent: AtomicU64,
    clock_echoes: AtomicU64,
    clock_sequence_errors: AtomicU64,
    end_frames: AtomicU64,
    damage_frames: AtomicU64,
    heal_frames: AtomicU64,
    death_frames: AtomicU64,
    hero_kill_increments: AtomicU64,
    hero_death_increments: AtomicU64,
}
#[derive(Clone, Copy)]
#[repr(u8)]
enum HeroCounter {
    Kills = 41,
    Deaths = 42,
}

impl Stats {
    fn received(&self, wire_len: usize, opcode: u16, body: &[u8]) {
        self.frames.fetch_add(1, Ordering::Relaxed);
        self.bytes.fetch_add(wire_len as u64, Ordering::Relaxed);
        let counter = match Opcode::try_from(opcode).ok() {
            Some(Opcode::Position) => &self.positions,
            Some(Opcode::ActorDeath) => &self.death_frames,
            Some(Opcode::StatModifier) if body.get(12) == Some(&(HeroCounter::Kills as u8)) => {
                &self.hero_kill_increments
            }
            Some(Opcode::StatModifier) if body.get(12) == Some(&(HeroCounter::Deaths as u8)) => {
                &self.hero_death_increments
            }
            Some(Opcode::HealthImpact) if body.len() >= 12 => {
                let delta = f32::from_be_bytes(body[8..12].try_into().unwrap());
                if delta < 0. {
                    &self.damage_frames
                } else if delta > 0. {
                    &self.heal_frames
                } else {
                    return;
                }
            }
            _ => return,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }
}
#[derive(Clone)]
struct Api {
    base: String,
    client: Client,
    aral_queue: Arc<tokio::sync::Mutex<()>>,
}
const BUILD: &str = "147219";

#[derive(Serialize)]
struct Request<P> {
    method: RpcMethod,
    params: P,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Reply<T> {
    code: i64,
    return_value: Option<T>,
    session_token: Option<String>,
}

#[derive(Deserialize)]
struct Identity<T> {
    #[serde(rename = "playerUUID")]
    player: T,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LobbyUpdate {
    state: LobbyState,
    match_id: Option<String>,
    host: Option<String>,
    port: Option<u16>,
}

#[derive(Serialize)]
struct LobbySelection<'a> {
    lobby: &'a str,
}

impl Api {
    async fn rpc<P: Serialize, T: DeserializeOwned>(
        &self,
        method: RpcMethod,
        params: P,
    ) -> Result<Reply<T>> {
        let name = method.as_str();
        let reply: Reply<T> = self
            .client
            .post(format!("{}/JSONRpc/{name}", self.base))
            .header("content-type", "application/x-www-form-urlencoded")
            .body(serde_json::to_vec(&Request { method, params })?)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        ensure!(reply.code == 0, "{name} returned {}", reply.code);
        Ok(reply)
    }
    async fn command(&self, method: RpcMethod, params: impl Serialize) -> Result<()> {
        self.rpc::<_, IgnoredAny>(method, params).await?;
        Ok(())
    }
    async fn update(&self, player: &Player) -> Result<LobbyUpdate> {
        self.rpc(RpcMethod::Update, [&player.token])
            .await?
            .return_value
            .context("missing lobby update")
    }
    async fn guest(&self) -> Result<Player> {
        let guest = self
            .rpc::<_, Identity<String>>(
                RpcMethod::CreateGuestPlayer,
                (
                    "kindred",
                    "en",
                    "US",
                    "production",
                    "en",
                    "BAKLAVA",
                    "android",
                    "",
                    BUILD,
                    1,
                    "benchmark",
                    "synthetic",
                    "0",
                ),
            )
            .await?;
        let recovery = guest.return_value.context("missing guest identity")?.player;
        let reply = self
            .rpc::<_, Identity<Uuid>>(RpcMethod::StartSessionForPlayer, [recovery])
            .await?;
        Ok(Player {
            id: reply
                .return_value
                .context("missing public identity")?
                .player,
            token: reply.session_token.context("missing token")?,
        })
    }
}
struct Player {
    id: Uuid,
    token: String,
}
struct Socket {
    stream: TcpStream,
    frames: FrameReader,
    cipher: MatchCipher,
    stats: Arc<Stats>,
}
impl Socket {
    async fn connect(addr: &str, room: &str, player: Uuid, stats: Arc<Stats>) -> Result<Self> {
        let mut stream = TcpStream::connect(addr).await?;
        stream.set_nodelay(true)?;
        stream
            .write_all(&hello_with_port("127.0.0.1", stream.peer_addr()?.port())?)
            .await?;
        let mut frames = FrameReader::new();
        let ack = frames.required(&mut stream).await?;
        ensure!(ack[..] == ACK_FRAME[2..], "unexpected server ACK");
        let mut sock = Self {
            stream,
            frames,
            cipher: MatchCipher::new(room),
            stats,
        };
        let mut id = [0; 70];
        id[..36].copy_from_slice(player.to_string().as_bytes());
        sock.write(Message::known(Opcode::PlayerUuid, id)).await?;
        Ok(sock)
    }
    async fn write(&mut self, msg: Message) -> Result<()> {
        self.stream.write_all(&self.cipher.encode(&msg)?).await?;
        self.stats.writes.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    async fn read(&mut self) -> Result<Message> {
        let bytes = self.frames.required(&mut self.stream).await?;
        let len = bytes.len();
        let msg = self.cipher.decode_owned(bytes.into())?;
        self.stats.received(len + 2, msg.opcode, &msg.body);
        Ok(msg)
    }
    async fn until(&mut self, op: Opcode) -> Result<Message> {
        timeout(
            Duration::from_secs(if op == Opcode::BeginLoading { 40 } else { 8 }),
            async {
                loop {
                    let message = self.read().await?;
                    if message.is(op) {
                        return Ok(message);
                    }
                }
            },
        )
        .await
        .with_context(|| format!("waiting for opcode {:#06x}", op.raw()))?
    }
    async fn play(
        self,
        seat: usize,
        game: bool,
        delay: Duration,
        mut stop: watch::Receiver<bool>,
    ) -> Result<bool> {
        if !delay.is_zero() {
            socket2::SockRef::from(&self.stream).set_recv_buffer_size(4096)?;
        }
        let (mut rd, mut wr) = self.stream.into_split();
        let mut frames = self.frames;
        let cipher = &self.cipher;
        let stats = &self.stats;
        // Keep the frame reader alive across timer ticks. Cancelling a partial
        // read when writing a movement command would desynchronize the stream.
        let reader = async {
            let mut last_clock = 0u32;
            loop {
                let mut bytes = frames.required(&mut rd).await?;
                let n = bytes.len();
                let (opcode, body) = cipher.decrypt_in_place(&mut bytes)?;
                stats.received(n + 2, opcode, body);
                if opcode == Opcode::ClockSync.raw() && body.len() >= 4 {
                    let time = f32::from_be_bytes(body[..4].try_into().unwrap());
                    let index = (time / 0.4).round() as u32;
                    let stride = if game { 5 } else { 1 };
                    if index != last_clock + stride {
                        stats.clock_sequence_errors.fetch_add(1, Ordering::Relaxed);
                    }
                    last_clock = index;
                    stats.clock_echoes.fetch_add(1, Ordering::Relaxed);
                }
                if !delay.is_zero() {
                    sleep(delay).await;
                }
                if opcode == Opcode::TickSyncC.raw() && matches!(body, [0, 0, 0, 1 | 2, 0 | 2, 0]) {
                    stats.end_frames.fetch_add(1, Ordering::Relaxed);
                    return Ok(true);
                }
            }
        };
        let writer = async {
            let mut period = interval(Duration::from_millis(if game { 400 } else { 1500 }));
            let mut index = 0u32;
            loop {
                period.tick().await;
                index += 1;
                let center = if seat.is_multiple_of(2) { 24. } else { 36. };
                let msg = if game && !index.is_multiple_of(5) {
                    let x = center + ((((index / 12 + seat as u32) % 3) as f32) - 1.) * 5.;
                    Message::movement(x, (seat % 5) as f32 - 2.)
                } else {
                    Message::clock(index as f32 * 0.4)
                };
                wr.write_all(&cipher.encode(&msg)?).await?;
                if msg.is(Opcode::ClockSync) {
                    stats.clocks_sent.fetch_add(1, Ordering::Relaxed);
                }
                stats.writes.fetch_add(1, Ordering::Relaxed);
            }
        };
        tokio::select! {v=reader=>v,v=writer=>v,_=stop.changed()=>Ok(false)}
    }
}
async fn aral_streams(
    players: &[Player],
    addr: &str,
    room: &str,
    stats: &Arc<Stats>,
) -> Result<Vec<(Option<Socket>, Socket)>> {
    let mut entrants = JoinSet::new();
    for (index, player) in players.iter().enumerate() {
        let (id, addr, room, stats) = (player.id, addr.to_owned(), room.to_owned(), stats.clone());
        // Every seat must continuously drain the timed draft roster broadcasts.
        entrants.spawn(async move {
            let mut socket = Socket::connect(&addr, &room, id, stats).await?;
            let draft = socket.until(Opcode::SnapshotAral).await?;
            let roster = halcyon_protocol::AralRoster::new(&draft.body)?;
            let identity = id.to_string();
            let seat = (0..6)
                .find(|&i| roster.identity(i) == Some(identity.as_bytes()))
                .context("ARAL player absent from assigned roster")?;
            socket.until(Opcode::BeginLoading).await?;
            socket
                .write(Message::known(Opcode::RequestScene, [0; 6]))
                .await?;
            let mut request = [0; 6];
            request[..4].copy_from_slice(&(1500 + seat as u32).to_be_bytes());
            request[4] = 1;
            socket
                .write(Message::known(Opcode::SceneOwner, request))
                .await?;
            // Peer entry also triggers 046E/0471 notices. Wait for our own actor.
            timeout(Duration::from_secs(10), async {
                loop {
                    let echoed = socket.read().await?;
                    if echoed.is(Opcode::SceneOwner) && echoed.body == request {
                        return Ok::<_, anyhow::Error>(());
                    }
                }
            })
            .await
            .context("waiting for own ARAL scene confirmation")??;
            socket
                .write(Message::known(Opcode::SceneConfirmed, [0; 6]))
                .await?;
            socket.until(Opcode::TickSyncA).await?;
            Ok::<_, anyhow::Error>((index, socket))
        });
    }
    let mut playing = Vec::with_capacity(players.len());
    while let Some(entrant) = entrants.join_next().await {
        playing.push(entrant??);
    }
    playing.sort_unstable_by_key(|(index, _)| *index);
    Ok(playing
        .into_iter()
        .map(|(_, socket)| (None, socket))
        .collect())
}

async fn setup(
    api: &Api,
    players: &[Player],
    workload: &Workload,
    group_number: usize,
    stats: &Arc<Stats>,
) -> Result<(String, Vec<(Option<Socket>, Socket)>)> {
    // Cohorts use the same native queue. Serialize lobby entry only so that a
    // later cohort cannot consume seats belonging to this one.
    let group = if matches!(workload, Workload::NativeAral) {
        Some(api.aral_queue.lock().await)
    } else {
        None
    };
    let queue = workload.queue(group_number);
    for p in players {
        api.command(
            RpcMethod::JoinLobby,
            (&p.token, LobbySelection { lobby: &queue }, BUILD),
        )
        .await?;
    }
    for p in players {
        api.command(RpcMethod::AcceptMatch, [&p.token]).await?;
    }
    let mut endpoint = None;
    let mut match_id = None;
    for p in players {
        let update = api.update(p).await?;
        ensure!(
            update.state == LobbyState::Playing,
            "room hasn't started: {}",
            update.state.as_str()
        );
        let id = update.match_id.context("missing matchId")?;
        if let Some(old) = &match_id {
            ensure!(old == &id, "players assigned to different rooms")
        };
        match_id = Some(id);
        endpoint = Some(format!(
            "{}:{}",
            update.host.context("missing host")?,
            update.port.context("missing port")?
        ));
    }
    let room = match_id.context("no seats")?;
    let addr = endpoint.context("no endpoint")?;
    drop(group);
    if matches!(workload, Workload::NativeAral) {
        let sockets = aral_streams(players, &addr, &room, stats).await?;
        stats.opened.fetch_add(1, Ordering::Relaxed);
        return Ok((room, sockets));
    }
    let mut controls = Vec::new();
    for p in players {
        let mut s = Socket::connect(&addr, &room, p.id, stats.clone()).await?;
        s.until(Opcode::Snapshot5V5).await?;
        let lock = if matches!(workload, Workload::NativeSolo) {
            Opcode::LockSelection
        } else {
            Opcode::HeroLock
        };
        s.write(Message::known(lock, [0; 6])).await?;
        s.until(lock).await?;
        controls.push(s);
    }
    let mut sockets = Vec::new();
    for (p, mut c) in players.iter().zip(controls) {
        if matches!(workload, Workload::NativeSolo) {
            c.until(Opcode::BeginLoading).await?;
            c.write(Message::known(Opcode::RequestScene, [0; 6]))
                .await?;
            c.write(Message::known(Opcode::SceneOwner, [0, 0, 5, 220, 1, 0]))
                .await?;
            c.until(Opcode::SceneReadiness).await?;
            c.write(Message::known(Opcode::SceneConfirmed, [0; 6]))
                .await?;
            c.until(Opcode::TickSyncA).await?;
            sockets.push((None, c));
            continue;
        }
        let mut game = Socket::connect(&addr, &room, p.id, stats.clone()).await?;
        game.until(Opcode::EntitySpawn).await?;
        sockets.push((Some(c), game));
    }
    stats.opened.fetch_add(1, Ordering::Relaxed);
    Ok((room, sockets))
}
enum Workload {
    Validation(usize),
    NativeSolo,
    NativeAral,
}
impl Workload {
    fn queue(&self, group: usize) -> std::borrow::Cow<'static, str> {
        match self {
            Self::NativeAral => KnownQueue::CasualAral.as_str().into(),
            Self::NativeSolo => KnownQueue::SoloBots.as_str().into(),
            Self::Validation(_) => format!("bench_{group}").into(),
        }
    }
    fn seats(&self) -> usize {
        match self {
            Self::NativeAral => 6,
            Self::NativeSolo => 1,
            Self::Validation(n) => *n,
        }
    }
}
async fn room(
    api: Api,
    workload: Workload,
    slow_reader_ms: u64,
    allow_admin_rotation: bool,
    number: usize,
    stats: Arc<Stats>,
    stop: watch::Receiver<bool>,
) -> Result<()> {
    let human_count = workload.seats();
    let mut players = Vec::new();
    for _ in 0..human_count {
        players.push(api.guest().await?)
    }
    let mut round = 0u64;
    loop {
        if *stop.borrow() {
            return Ok(());
        }
        let (room_id, sockets) = setup(&api, &players, &workload, number, &stats).await?;
        round += 1;
        if round == 1 {
            stats.initial_rooms.fetch_add(1, Ordering::Relaxed);
        }
        let mut peers = JoinSet::new();
        for (seat, (ctrl, game)) in sockets.into_iter().enumerate() {
            if let Some(ctrl) = ctrl {
                let ctrl_stop = stop.clone();
                peers.spawn(async move {
                    (
                        seat,
                        false,
                        false,
                        ctrl.play(seat, false, Duration::ZERO, ctrl_stop).await,
                    )
                });
            }
            let slow = slow_reader_ms > 0 && seat + 1 == human_count;
            let delay = if slow {
                Duration::from_millis(slow_reader_ms)
            } else {
                Duration::ZERO
            };
            let game_stop = stop.clone();
            peers.spawn(async move {
                (
                    seat,
                    true,
                    slow,
                    game.play(seat, true, delay, game_stop).await,
                )
            });
        }
        let (failed_seat, game_channel, res) = loop {
            let (seat, game, slow, res) = peers.join_next().await.context("no sockets")??;
            if slow && res.is_err() {
                let state = api.update(&players[0]).await?;
                if state.state == LobbyState::Playing {
                    stats.slow_disconnects.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
            }
            break (seat, game, res);
        };
        if *stop.borrow() {
            return Ok(());
        }
        // The control socket may close at match end before the world reader sees its end frame.
        let until = Instant::now() + Duration::from_secs(3);
        loop {
            let p = &players[0];
            let v = api.update(p).await?;
            if v.state == LobbyState::PostMatch
                || (allow_admin_rotation && v.state == LobbyState::Menus)
            {
                break;
            }
            ensure!(
                Instant::now() < until,
                "socket ended before settlement: group={number} round={round} room={room_id} seat={failed_seat} game={game_channel} state={} result={res:?}",
                v.state.as_str()
            );
            sleep(Duration::from_millis(100)).await;
        }
        peers.shutdown().await;
        for p in &players {
            api.command(RpcMethod::NotifyExitPostMatch, [&p.token])
                .await?;
        }
        stats.completed.fetch_add(1, Ordering::Relaxed);
    }
}
#[tokio::main]
async fn main() -> Result<()> {
    let a = Args::parse();
    ensure!(
        a.platform.starts_with("https://")
            && (1..=10).contains(&a.humans)
            && (!a.native_solo || a.humans == 1)
            && (!a.native_aral || a.humans == 6)
            && a.rooms > 0
            && a.seconds > 0,
        "invalid benchmark parameters"
    );
    let api = Api {
        base: a.platform,
        aral_queue: Arc::default(),
        client: Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .add_root_certificate(Certificate::from_pem(&std::fs::read(a.certificate)?)?)
            .build()?,
    };
    let stats = Arc::new(Stats::default());
    let mut workers = JoinSet::new();
    let (stop, recv) = watch::channel(false);
    for i in 0..a.rooms {
        let worker = room(
            api.clone(),
            if a.native_aral {
                Workload::NativeAral
            } else if a.native_solo {
                Workload::NativeSolo
            } else {
                Workload::Validation(a.humans)
            },
            a.slow_reader_ms,
            a.allow_admin_rotation,
            i,
            stats.clone(),
            recv.clone(),
        );
        let delay = Duration::from_secs_f64(
            a.ramp_seconds as f64 * i as f64 / a.rooms.saturating_sub(1).max(1) as f64,
        );
        workers.spawn(async move {
            sleep(delay).await;
            worker.await
        });
    }
    let started = Instant::now();
    let mut full = None;
    let mut interval = interval(Duration::from_secs(2));
    loop {
        tokio::select! {
            done=workers.join_next()=>{
                if let Some(job)=done { job??; }
                anyhow::bail!("load worker exited before deadline")
            }
            _=interval.tick()=>{}
        }
        let opened = stats.opened.load(Ordering::Relaxed);
        let established = stats.initial_rooms.load(Ordering::Relaxed);
        if established == a.rooms as u64 && full.is_none() {
            full = Some(Instant::now());
        }
        ensure!(
            full.is_some()
                || started.elapsed() < Duration::from_secs(90_u64.saturating_add(a.ramp_seconds)),
            "startup deadline"
        );
        let sample = Sample {
            elapsed_s: started.elapsed().as_secs_f64(),
            steady_s: full.map(|t| t.elapsed().as_secs_f64()),
            opened,
            initial_rooms: established,
            completed: stats.completed.load(Ordering::Relaxed),
            slow_disconnects: stats.slow_disconnects.load(Ordering::Relaxed),
            clocks_sent: stats.clocks_sent.load(Ordering::Relaxed),
            clock_echoes: stats.clock_echoes.load(Ordering::Relaxed),
            clock_sequence_errors: stats.clock_sequence_errors.load(Ordering::Relaxed),
            end_frames: stats.end_frames.load(Ordering::Relaxed),
            damage_frames: stats.damage_frames.load(Ordering::Relaxed),
            heal_frames: stats.heal_frames.load(Ordering::Relaxed),
            death_frames: stats.death_frames.load(Ordering::Relaxed),
            hero_kill_increments: stats.hero_kill_increments.load(Ordering::Relaxed),
            hero_death_increments: stats.hero_death_increments.load(Ordering::Relaxed),
            frames: stats.frames.load(Ordering::Relaxed),
            gs_bytes: stats.bytes.load(Ordering::Relaxed),
            positions: stats.positions.load(Ordering::Relaxed),
            client_commands: stats.writes.load(Ordering::Relaxed),
        };
        println!("{}", serde_json::to_string(&sample)?);
        if full.is_some_and(|t| t.elapsed() >= Duration::from_secs(a.seconds)) {
            break;
        }
    }
    stop.send_replace(true);
    workers.shutdown().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;
    use tokio::{
        io::{AsyncBufReadExt, BufReader},
        net::TcpListener,
    };

    #[tokio::test]
    async fn guest_session_and_lobby_use_the_public_wire_contract() -> Result<()> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let base = format!("http://{}", listener.local_addr()?);
        let server = tokio::spawn(async move {
            for (name, params, body) in [
                (
                    "createGuestPlayer",
                    r#"["kindred","en","US","production","en","BAKLAVA","android","","147219",1,"benchmark","synthetic","0"]"#,
                    r#"{"code":0,"returnValue":{"playerUUID":"recovery-fixture"},"sessionToken":""}"#,
                ),
                (
                    "startSessionForPlayer",
                    r#"["recovery-fixture"]"#,
                    r#"{"code":0,"returnValue":{"playerUUID":"00000000-0000-0000-0000-000000000001"},"sessionToken":"session-fixture"}"#,
                ),
                (
                    "joinLobby",
                    r#"["session-fixture",{"lobby":"bench_27"},"147219"]"#,
                    r#"{"code":0,"returnValue":null}"#,
                ),
                (
                    "acceptMatch",
                    r#"["session-fixture"]"#,
                    r#"{"code":0,"returnValue":true}"#,
                ),
                (
                    "update",
                    r#"["session-fixture"]"#,
                    r#"{"code":0,"returnValue":{"state":"playing","matchId":"scene","host":"127.0.0.1","port":41123}}"#,
                ),
                (
                    "notifyExitPostMatch",
                    r#"["session-fixture"]"#,
                    r#"{"code":0}"#,
                ),
            ] {
                let (mut socket, _) = listener.accept().await?;
                let mut rd = BufReader::new(&mut socket);
                let mut header = String::new();
                rd.read_line(&mut header).await?;
                assert_eq!(header, format!("POST /JSONRpc/{name} HTTP/1.1\r\n"));
                let mut length = None;
                loop {
                    header.clear();
                    rd.read_line(&mut header).await?;
                    if header == "\r\n" {
                        break;
                    }
                    if let Some(n) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = Some(n.trim().parse::<usize>()?);
                    }
                }
                let mut request = vec![0; length.context("missing HTTP length")?];
                rd.read_exact(&mut request).await?;
                assert_eq!(
                    String::from_utf8(request)?,
                    format!(r#"{{"method":"{name}","params":{params}}}"#)
                );
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await?;
            }
            Ok::<_, anyhow::Error>(())
        });
        let api = Api {
            base,
            client: Client::builder().no_proxy().build()?,
            aral_queue: Arc::default(),
        };
        let player = api.guest().await?;
        assert_eq!(player.id, Uuid::from_u128(1));
        let queue = Workload::Validation(4).queue(27);
        api.command(
            RpcMethod::JoinLobby,
            (&player.token, LobbySelection { lobby: &queue }, BUILD),
        )
        .await?;
        api.command(RpcMethod::AcceptMatch, [&player.token]).await?;
        let update = api.update(&player).await?;
        assert_eq!(
            (update.state, update.port),
            (LobbyState::Playing, Some(41123))
        );
        api.command(RpcMethod::NotifyExitPostMatch, [&player.token])
            .await?;
        server.await??;
        Ok(())
    }
}
