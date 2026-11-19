//! Independent CE load client. All mutation and traffic are through public client sockets.
use anyhow::{Context, Result, ensure};
use clap::Parser;
use halcyon_protocol::{ACK_FRAME, MAX_PAYLOAD, MatchCipher, Message, hello_with_port};
use reqwest::{Certificate, Client};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
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
    #[arg(long)]
    native_solo: bool,
    /// The operator is deliberately terminating rooms via the admin API.
    #[arg(long)]
    allow_admin_rotation: bool,
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
impl Stats {
    fn received(&self, wire_len: usize, msg: &Message) {
        self.frames.fetch_add(1, Ordering::Relaxed);
        self.bytes.fetch_add(wire_len as u64, Ordering::Relaxed);
        let counter = match msg.opcode {
            0x42e => &self.positions,
            0x430 => &self.death_frames,
            0x41c if msg.body.get(12) == Some(&41) => &self.hero_kill_increments,
            0x41c if msg.body.get(12) == Some(&42) => &self.hero_death_increments,
            0x41e if msg.body.len() >= 12 => {
                let delta = f32::from_be_bytes(msg.body[8..12].try_into().unwrap());
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
}
impl Api {
    async fn rpc(&self, method: &str, args: Value) -> Result<Value> {
        let v: Value = self
            .client
            .post(format!("{}/JSONRpc/{method}", self.base))
            .header("content-type", "application/x-www-form-urlencoded")
            .body(json!({"method":method,"params":args}).to_string())
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        ensure!(v["code"] == 0, "{method} returned {}", v["code"]);
        Ok(v)
    }
    async fn guest(&self) -> Result<Player> {
        let v = self
            .rpc(
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
                    "benchmark",
                    "synthetic",
                    "0"
                ]),
            )
            .await?;
        let v = self
            .rpc(
                "startSessionForPlayer",
                json!([v["returnValue"]["playerUUID"]]),
            )
            .await?;
        Ok(Player {
            id: v["returnValue"]["playerUUID"]
                .as_str()
                .context("missing playerUUID")?
                .parse()?,
            token: v["sessionToken"].as_str().context("missing token")?.into(),
        })
    }
}
struct Player {
    id: Uuid,
    token: String,
}
struct Socket {
    stream: TcpStream,
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
        let mut ack = [0; 5];
        stream.read_exact(&mut ack).await?;
        ensure!(ack == ACK_FRAME, "unexpected server ACK");
        let mut sock = Self {
            stream,
            cipher: MatchCipher::new(room),
            stats,
        };
        let mut id = [0; 70];
        id[..36].copy_from_slice(player.to_string().as_bytes());
        sock.write(Message::new(0x3e8, id)).await?;
        Ok(sock)
    }
    async fn write(&mut self, msg: Message) -> Result<()> {
        self.stream.write_all(&self.cipher.encode(&msg)?).await?;
        self.stats.writes.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    async fn read(&mut self) -> Result<Message> {
        let len = self.stream.read_u16().await? as usize;
        ensure!(
            (2..=MAX_PAYLOAD).contains(&len),
            "invalid frame length {len}"
        );
        let mut bytes = vec![0; len];
        self.stream.read_exact(&mut bytes).await?;
        let msg = self.cipher.decode(&bytes)?;
        self.stats.received(len + 2, &msg);
        Ok(msg)
    }
    async fn until(&mut self, op: u16) -> Result<()> {
        timeout(
            Duration::from_secs(if op == 0x046c { 15 } else { 8 }),
            async {
                loop {
                    if self.read().await?.opcode == op {
                        return Ok(());
                    }
                }
            },
        )
        .await?
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
        let cipher = &self.cipher;
        let stats = &self.stats;
        // Keep the frame reader alive across timer ticks. Cancelling a partial
        // read when writing a movement command would desynchronize the stream.
        let reader = async {
            let mut last_clock = 0u32;
            loop {
                let n = rd.read_u16().await? as usize;
                ensure!((2..=MAX_PAYLOAD).contains(&n), "invalid frame");
                let mut b = vec![0; n];
                rd.read_exact(&mut b).await?;
                let m = cipher.decode(&b)?;
                stats.received(n + 2, &m);
                if m.opcode == 0 && m.body.len() >= 4 {
                    let time = f32::from_be_bytes(m.body[..4].try_into().unwrap());
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
                if m.opcode == 0x03f1 && matches!(m.body.as_slice(), [0, 0, 0, 1 | 2, 0 | 2, 0]) {
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
                if msg.opcode == 0 {
                    stats.clocks_sent.fetch_add(1, Ordering::Relaxed);
                }
                stats.writes.fetch_add(1, Ordering::Relaxed);
            }
        };
        tokio::select! {v=reader=>v,v=writer=>v,_=stop.changed()=>Ok(false)}
    }
}
async fn setup(
    api: &Api,
    players: &[Player],
    mode: &str,
    stats: &Arc<Stats>,
) -> Result<(String, Vec<(Option<Socket>, Socket)>)> {
    for p in players {
        api.rpc("joinLobby", json!([p.token,{"lobby":mode},"147219"]))
            .await?;
    }
    for p in players {
        api.rpc("acceptMatch", json!([p.token])).await?;
    }
    let mut endpoint = None;
    let mut match_id = None;
    for p in players {
        let v = api.rpc("update", json!([p.token])).await?;
        let v = &v["returnValue"];
        ensure!(
            v["state"] == "playing",
            "room hasn't started: {}",
            v["state"]
        );
        let id = v["matchId"].as_str().context("missing matchId")?.to_owned();
        if let Some(old) = &match_id {
            ensure!(old == &id, "players assigned to different rooms")
        };
        match_id = Some(id);
        endpoint = Some(format!(
            "{}:{}",
            v["host"].as_str().context("missing host")?,
            v["port"].as_u64().context("missing port")?
        ));
    }
    let room = match_id.context("no seats")?;
    let addr = endpoint.context("no endpoint")?;
    let mut controls = Vec::new();
    for p in players {
        let mut s = Socket::connect(&addr, &room, p.id, stats.clone()).await?;
        s.until(0x459).await?;
        let lock = if mode == "5v5_bots_solo" {
            0x0463
        } else {
            0x0455
        };
        s.write(Message::new(lock, [0; 6])).await?;
        s.until(lock).await?;
        controls.push(s);
    }
    let mut sockets = Vec::new();
    for (p, mut c) in players.iter().zip(controls) {
        if mode == "5v5_bots_solo" {
            c.until(0x046c).await?;
            c.write(Message::new(0x046e, [0; 6])).await?;
            c.write(Message::new(0x0471, [0, 0, 5, 220, 1, 0])).await?;
            c.until(0x045c).await?;
            c.write(Message::new(0x046d, [0; 6])).await?;
            c.until(0x03ef).await?;
            sockets.push((None, c));
            continue;
        }
        let mut game = Socket::connect(&addr, &room, p.id, stats.clone()).await?;
        game.until(0x3f3).await?;
        sockets.push((Some(c), game));
    }
    stats.opened.fetch_add(1, Ordering::Relaxed);
    Ok((room, sockets))
}
enum Workload {
    Validation(usize),
    NativeSolo,
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
    let (human_count, mode) = match workload {
        Workload::NativeSolo => (1, "5v5_bots_solo".to_owned()),
        Workload::Validation(n) => (n, format!("bench_{number}")),
    };
    let mut players = Vec::new();
    for _ in 0..human_count {
        players.push(api.guest().await?)
    }
    let mut round = 0u64;
    loop {
        if *stop.borrow() {
            return Ok(());
        }
        let (room_id, sockets) = setup(&api, &players, &mode, &stats).await?;
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
                let state = api.rpc("update", json!([players[0].token])).await?;
                if state["returnValue"]["state"] == "playing" {
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
            let v = api.rpc("update", json!([p.token])).await?;
            if v["returnValue"]["state"] == "post_match"
                || (allow_admin_rotation && v["returnValue"]["state"] == "menus")
            {
                break;
            }
            ensure!(
                Instant::now() < until,
                "socket ended before settlement: group={number} round={round} room={room_id} seat={failed_seat} game={game_channel} state={} result={res:?}",
                v["returnValue"]["state"]
            );
            sleep(Duration::from_millis(100)).await;
        }
        peers.shutdown().await;
        for p in &players {
            api.rpc("notifyExitPostMatch", json!([p.token])).await?;
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
            && a.rooms > 0
            && a.seconds > 0,
        "invalid benchmark parameters"
    );
    let api = Api {
        base: a.platform,
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
            if a.native_solo {
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
        let sample = json!({
            "elapsed_s":started.elapsed().as_secs_f64(),"steady_s":full.map(|t|t.elapsed().as_secs_f64()),
            "opened":opened,"initial_rooms":established,"completed":stats.completed.load(Ordering::Relaxed),
            "slow_disconnects":stats.slow_disconnects.load(Ordering::Relaxed),
            "clocks_sent":stats.clocks_sent.load(Ordering::Relaxed),"clock_echoes":stats.clock_echoes.load(Ordering::Relaxed),
            "clock_sequence_errors":stats.clock_sequence_errors.load(Ordering::Relaxed),"end_frames":stats.end_frames.load(Ordering::Relaxed),
            "damage_frames":stats.damage_frames.load(Ordering::Relaxed),"heal_frames":stats.heal_frames.load(Ordering::Relaxed),"death_frames":stats.death_frames.load(Ordering::Relaxed),
            "hero_kill_increments":stats.hero_kill_increments.load(Ordering::Relaxed),"hero_death_increments":stats.hero_death_increments.load(Ordering::Relaxed),
            "frames":stats.frames.load(Ordering::Relaxed),"gs_bytes":stats.bytes.load(Ordering::Relaxed),
            "positions":stats.positions.load(Ordering::Relaxed),"client_commands":stats.writes.load(Ordering::Relaxed)
        });
        println!("{sample}");
        if full.is_some_and(|t| t.elapsed() >= Duration::from_secs(a.seconds)) {
            break;
        }
    }
    stop.send_replace(true);
    workers.shutdown().await;
    Ok(())
}
