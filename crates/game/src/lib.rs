//! In-process room hosting with CE control and game TCP connections.
mod admission;
pub use admission::{AdmissionPolicy, PortRange, ResourceMetrics};
mod messages;
mod observer;
mod recording;
mod transport;
use chrono::{DateTime, Utc};
use halcyon_data::GameData;
use halcyon_platform::{HostedMatch, MatchHost, PlatformError, Reservation};
use halcyon_protocol::{Fields, Message};
use halcyon_sim::{Command, Event, Position, World};
use messages::{setup, snapshot, spawn, wire_id, world_header};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    net::{IpAddr, TcpListener},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};
use tokio::sync::{mpsc, watch};
use uuid::Uuid;

type Batch = Vec<Message>;
type Sink = mpsc::Sender<transport::Delivery>;
type Finish = Arc<dyn Fn(Uuid, Value) + Send + Sync>;
type Shared = Arc<Mutex<Room>>;
#[derive(Clone)]
pub struct Rooms(Arc<Manager>);
struct Manager {
    bind_ip: IpAddr,
    replay_directory: Mutex<Option<std::path::PathBuf>>,
    gate: Mutex<admission::Gate>,
    monitor_started: AtomicBool,
    ports: admission::Ports,
    host: String,
    capacity: usize,
    data: Option<Arc<GameData>>,
    rooms: Mutex<HashMap<Uuid, Shared>>,
    finish: Mutex<Option<Finish>>,
    bytes: AtomicU64,
}
#[derive(Debug, PartialEq)]
pub enum TerminateError {
    Missing,
    Conflict,
    Invalid,
}
struct Peer {
    generation: u64,
    tx: Sink,
    stalled: watch::Sender<bool>,
}
impl Peer {
    fn send(&self, batch: transport::Delivery) {
        if self.tx.try_send(batch).is_err() {
            self.stalled.send_replace(true);
        }
    }
}
#[derive(Default)]
struct Seat {
    locked: bool,
    peers: [Option<Peer>; 2],
    last: Option<DateTime<Utc>>,
    seen: [HashSet<u16>; 2],
    inventory: Vec<(halcyon_sim::BasicItem, u32, u32)>,
}
struct Pick {
    name: String,
    skin: u32,
    selected: bool,
    locked: bool,
}
#[derive(Default)]
struct TickLatency {
    count: u64,
    total_us: u64,
    max_us: u64,
    misses: u64,
    over40: u64,
    recent: VecDeque<u64>,
}
impl TickLatency {
    fn observe(&mut self, us: u64) {
        self.count += 1;
        self.total_us = self.total_us.saturating_add(us);
        self.max_us = self.max_us.max(us);
        self.over40 += u64::from(us > 40_000);
        self.misses += u64::from(us > 1_000_000 / halcyon_sim::TICK_HZ);
        if self.recent.len() == 1024 {
            self.recent.pop_front();
        }
        self.recent.push_back(us);
    }
    fn report(&self) -> Value {
        let mut recent: Vec<_> = self.recent.iter().copied().collect();
        recent.sort_unstable();
        let index = (recent.len() * 99).div_ceil(100).saturating_sub(1);
        json!({"count":self.count,"totalUs":self.total_us,"maxUs":self.max_us,"missedDeadlines":self.misses,"over40ms":self.over40,"recentSampleCount":recent.len(),"p99Us":recent.get(index)})
    }
}
#[derive(Clone, Copy, Default)]
struct Combat {
    kills: u32,
    deaths: u32,
    damage: f64,
    healing: f64,
}
struct Room {
    visible: [HashSet<u32>; 2],
    next_visible: [HashSet<u32>; 2],
    replay_bytes: Arc<AtomicU64>,
    observers: HashMap<Uuid, observer::Observer>,
    latency: TickLatency,
    admitted: bool,
    admission_ready: watch::Sender<bool>,
    data: Option<Arc<GameData>>,
    picks: Vec<Pick>,
    id: Uuid,
    address: String,
    queue: String,
    players: Vec<Uuid>,
    requested_teams: Vec<u8>,
    next_item: u32,
    combat: [Combat; 10],
    catalog: Vec<String>,
    seats: Vec<Seat>,
    world: World,
    phase: &'static str,
    created: DateTime<Utc>,
    since: DateTime<Utc>,
    ended: Option<DateTime<Utc>>,
    reason: Option<&'static str>,
    termination: Option<Value>,
    events: Vec<Value>,
    stop: watch::Sender<bool>,
    generation: u64,
    last_active: Instant,
    phase_start: Instant,
    last_roster: Instant,
}
impl Rooms {
    pub fn new(
        bind_ip: IpAddr,
        advertised_host: String,
        capacity: usize,
        data: Option<Arc<GameData>>,
    ) -> Self {
        Self::with_policy(
            bind_ip,
            advertised_host,
            capacity,
            data,
            AdmissionPolicy::default(),
            None,
        )
        .expect("valid default room policy")
    }
    pub fn with_policy(
        bind_ip: IpAddr,
        advertised_host: String,
        capacity: usize,
        data: Option<Arc<GameData>>,
        policy: AdmissionPolicy,
        game_port_range: Option<PortRange>,
    ) -> Result<Self, PlatformError> {
        let ports = admission::Ports::new(game_port_range)?;
        let capacity = capacity.min(ports.slots());
        let this = Self(Arc::new(Manager {
            gate: Mutex::new(admission::Gate::new(policy)?),
            monitor_started: AtomicBool::default(),
            ports,
            replay_directory: Mutex::default(),
            bind_ip,
            host: advertised_host,
            capacity,
            data,
            rooms: Mutex::default(),
            finish: Mutex::default(),
            bytes: AtomicU64::default(),
        }));
        this.monitor_resources();
        Ok(this)
    }
    pub fn set_finish_handler(&self, handler: Finish) {
        *self.0.finish.lock().unwrap() = Some(handler);
    }
    pub fn clear_finish_handler(&self) {
        self.0.finish.lock().unwrap().take();
    }
    /// Active rooms, admitted human and bot seats, and all staged seats.
    pub fn hosting_counts(&self) -> (usize, usize, usize, usize) {
        let rooms = self.0.rooms.lock().unwrap();
        rooms
            .values()
            .fold((0, 0, 0, 0), |(active, human, bots, prepared), room| {
                let room = room.lock().unwrap();
                if room.ended.is_some() {
                    (active, human, bots, prepared)
                } else if room.admitted {
                    (
                        active + 1,
                        human + room.players.len(),
                        bots + 10 - room.players.len(),
                        prepared,
                    )
                } else {
                    (active + 1, human, bots, prepared + 10)
                }
            })
    }
    fn advertise(&self, port: u16) -> String {
        let host = &self.0.host;
        if host.contains(':') && !host.starts_with('[') {
            format!("[{host}]:{port}")
        } else {
            format!("{host}:{port}")
        }
    }
    pub fn capacity(&self) -> usize {
        self.0.capacity
    }
    pub fn active_rooms(&self) -> usize {
        self.0
            .rooms
            .lock()
            .unwrap()
            .values()
            .filter(|r| r.lock().unwrap().ended.is_none())
            .count()
    }
    pub fn outbound_bytes(&self) -> u64 {
        self.0.bytes.load(Ordering::Relaxed)
    }
    pub fn list(&self) -> Vec<Value> {
        self.0
            .rooms
            .lock()
            .unwrap()
            .values()
            .map(|r| r.lock().unwrap().view(false))
            .collect()
    }
    fn room(&self, id: Uuid) -> Option<Shared> {
        self.0.rooms.lock().unwrap().get(&id).cloned()
    }
    pub fn detail(&self, id: Uuid) -> Option<Value> {
        Some(self.room(id)?.lock().unwrap().view(true))
    }
    pub fn contains(&self, id: Uuid) -> bool {
        self.room(id).is_some()
    }
    pub fn events(&self, id: Uuid, after: u64, limit: usize) -> Option<Value> {
        let r = self.room(id)?;
        let r = r.lock().unwrap();
        let start = (after as usize).min(r.events.len());
        let end = start.saturating_add(limit).min(r.events.len());
        Some(
            json!({"items":&r.events[start..end],"hasMore":end<r.events.len(),"nextAfter":if start==end {after}else{end as u64}}),
        )
    }
    pub fn terminate(
        &self,
        id: Uuid,
        actor: &str,
        reason: &str,
    ) -> Result<(bool, Value), TerminateError> {
        if reason.trim().is_empty() || reason.chars().count() > 256 {
            return Err(TerminateError::Invalid);
        }
        let room = self.room(id).ok_or(TerminateError::Missing)?;
        let mut r = room.lock().unwrap();
        if let Some(prior) = &r.termination {
            return if prior["reason"] == reason {
                Ok((r.ended.is_some(), prior.clone()))
            } else {
                Err(TerminateError::Conflict)
            };
        }
        if matches!(r.phase, "ending" | "ended") {
            return Err(TerminateError::Conflict);
        }
        let v = json!({"matchId":id,"requestedAt":now(),"requestedBy":actor,"reason":reason});
        r.termination = Some(v.clone());
        r.reason = Some("admin_terminated");
        r.event(
            "admin_action",
            "Termination requested",
            json!({"actor":actor}),
        );
        r.phase("ending");
        r.stop.send_replace(true);
        Ok((false, v))
    }
    fn finished(&self, room: &Shared) {
        let (id, result) = {
            let mut r = room.lock().unwrap();

            let end = r.reason.unwrap_or("error");
            r.reason = Some(end);
            r.ended = Some(Utc::now());
            r.phase("ended");
            if !r.admitted {
                return;
            }
            let players:serde_json::Map<_,_>=r.players.iter().enumerate().map(|(i,id)|{
                let e=&r.world.entities[i];(id.to_string(),json!({"team":e.team,"victory":r.world.winner.map(|w|e.team==w),"hero":r.picks[i].name,"gold":e.gold,"level":e.level,"xp":e.xp,"kills":r.combat[i].kills,"deaths":r.combat[i].deaths,"damage":r.combat[i].damage,"healing":r.combat[i].healing}))
            }).collect();
            (
                r.id,
                json!({"reason":end,"winner":r.world.winner,"ticks":r.world.tick,"players":players}),
            )
        };
        if let Some(f) = self.0.finish.lock().unwrap().clone() {
            f(id, result);
        }
    }
}
fn now() -> String {
    Utc::now().to_rfc3339()
}
impl MatchHost for Rooms {
    fn spectate(
        &self,
        match_id: Uuid,
        viewer: Uuid,
    ) -> Result<halcyon_platform::HostedSpectator, PlatformError> {
        let (address, spectator_id) = self.observe(match_id, viewer)?;
        Ok(halcyon_platform::HostedSpectator {
            address,
            spectator_id,
        })
    }
    fn stop_spectating(&self, match_id: Uuid, spectator_id: Uuid) -> Result<(), PlatformError> {
        self.revoke_observer(match_id, spectator_id)
    }

    fn reserve(&self, req: Reservation) -> Result<HostedMatch, PlatformError> {
        self.allocate(req, true)
    }
}
impl Rooms {
    pub fn prepare(&self, req: Reservation) -> Result<HostedMatch, PlatformError> {
        self.allocate(req, false)
    }
    pub fn activate(&self, id: Uuid) -> Result<(), PlatformError> {
        let rooms = self.0.rooms.lock().unwrap();
        let mut room = rooms
            .get(&id)
            .ok_or(PlatformError::HostUnavailable)?
            .lock()
            .unwrap();
        if matches!(room.phase, "ending" | "ended") {
            return Err(PlatformError::Conflict("closed reservation"));
        }
        room.admitted = true;
        room.admission_ready.send_replace(true);
        Ok(())
    }
    pub fn discard(&self, id: Uuid) -> Result<(), PlatformError> {
        let mut rooms = self.0.rooms.lock().unwrap();
        if let Some(room) = rooms.get(&id) {
            let room = room.lock().unwrap();
            if room.admitted {
                return Err(PlatformError::Conflict("already active"));
            }
            room.stop.send_replace(true);
        }
        rooms.remove(&id);
        Ok(())
    }
    fn allocate(&self, req: Reservation, admitted: bool) -> Result<HostedMatch, PlatformError> {
        self.monitor_resources();
        let mut rooms = self.0.rooms.lock().unwrap();
        rooms.retain(|_, room| {
            room.lock()
                .unwrap()
                .ended
                .is_none_or(|end| Utc::now() - end < chrono::Duration::minutes(30))
        });
        if let Some(r) = rooms.get(&req.match_id) {
            let r = r.lock().unwrap();
            if r.requested_teams != req.teams
                || r.queue != req.mode
                || r.players != req.players
                || r.ended.is_some()
                || (admitted && !r.admitted)
            {
                return Err(PlatformError::Conflict("reservation conflict"));
            }
            return Ok(HostedMatch {
                address: r.address.clone(),
            });
        }
        if req.players.is_empty()
            || req.players.len() > 10
            || req.players.iter().collect::<HashSet<_>>().len() != req.players.len()
        {
            return Err(PlatformError::Invalid("invalid roster"));
        }
        let occupied = rooms
            .values()
            .filter(|r| r.lock().unwrap().ended.is_none())
            .count();
        if self.free_with_count(occupied) == 0 {
            return Err(PlatformError::HostUnavailable);
        }
        let rt =
            tokio::runtime::Handle::try_current().map_err(|_| PlatformError::HostUnavailable)?;
        let listener = self
            .0
            .ports
            .bind(self.0.bind_ip)
            .map_err(|_| PlatformError::HostUnavailable)?;
        listener
            .set_nonblocking(true)
            .map_err(|_| PlatformError::HostUnavailable)?;
        let port = listener
            .local_addr()
            .map_err(|_| PlatformError::HostUnavailable)?
            .port();
        let address = self.advertise(port);
        let seed = req.match_id.as_u128() as u64;
        let n = req.players.len() as u8;
        let mut world = match &self.0.data {
            Some(d) => World::from_data(seed, n, d).map_err(|_| PlatformError::HostUnavailable)?,
            None => World::new(seed, n),
        };
        if !req.teams.is_empty()
            && (req.teams.len() != req.players.len()
                || req.teams.iter().any(|t| *t > 1)
                || [0, 1]
                    .iter()
                    .any(|team| req.teams.iter().filter(|t| *t == team).count() > 5))
        {
            return Err(PlatformError::Invalid("invalid human teams"));
        }
        world
            .assign_teams(&req.teams)
            .map_err(|_| PlatformError::Invalid("invalid human teams"))?;
        let mut picks = Vec::with_capacity(10);
        for i in 0..10 {
            let bot = i >= usize::from(n);
            let mut name = "Ringo";
            if bot
                && let Some(d) = &self.0.data
                && !d.hero_names.is_empty()
            {
                let idx = (seed as usize).wrapping_add(i * 7) % d.hero_names.len();
                let candidate = &d.hero_names[idx];
                if world.select_hero(i, candidate, d).is_ok() {
                    name = candidate;
                }
            }
            picks.push(Pick {
                name: name.into(),
                skin: halcyon_protocol::skin_hash(&format!("{name}_DefaultSkin")),
                selected: bot,
                locked: bot,
            });
        }
        let catalog = match &self.0.data {
            Some(data) if !data.hero_names.is_empty() => data
                .hero_names
                .iter()
                .map(|h| format!("*{h}*"))
                .chain(data.skin_names.iter().cloned())
                .collect(),
            _ => vec!["*Ringo*".to_owned()],
        };
        let listener = tokio::net::TcpListener::from_std(listener)
            .map_err(|_| PlatformError::HostUnavailable)?;
        let (stop, stopped) = watch::channel(false);
        let t = Utc::now();
        let mut room = Room {
            visible: Default::default(),
            next_visible: Default::default(),
            replay_bytes: Arc::default(),
            admission_ready: watch::channel(admitted).0,
            observers: HashMap::new(),
            latency: TickLatency::default(),
            admitted,
            data: self.0.data.clone(),
            picks,
            id: req.match_id,
            address: address.clone(),
            queue: req.mode,
            players: req.players,
            requested_teams: req.teams,
            next_item: 40000,
            combat: [Combat::default(); 10],
            catalog,
            seats: (0..n).map(|_| Seat::default()).collect(),
            world,
            phase: "allocated",
            created: t,
            since: t,
            ended: None,
            reason: None,
            termination: None,
            events: vec![],
            stop,
            generation: 0,
            last_active: Instant::now(),
            phase_start: Instant::now(),
            last_roster: Instant::now(),
        };
        room.event("phase_changed", "allocated", json!({}));
        let room = Arc::new(Mutex::new(room));
        rooms.insert(req.match_id, room.clone());
        let manager = self.clone();
        rt.spawn(async move {
            transport::serve(listener, room.clone(), stopped, &manager).await;
            manager.finished(&room);
        });
        Ok(HostedMatch { address })
    }
}
impl Room {
    fn event(&mut self, kind: &str, message: &str, mut extra: Value) {
        tracing::debug!(room=%self.id,kind,message,opcode=extra["opcode"].as_str(),channel=extra["channel"].as_str(),"room event");
        let o = extra.as_object_mut().unwrap();
        o.insert("seq".into(), json!(self.events.len() + 1));
        o.insert("kind".into(), json!(kind));
        o.insert("message".into(), json!(message));
        o.insert("at".into(), json!(now()));
        self.events.push(extra);
    }
    fn phase(&mut self, phase: &'static str) {
        if self.phase != phase {
            self.phase = phase;
            self.phase_start = Instant::now();
            self.since = Utc::now();
            self.event("phase_changed", phase, json!({}));
        }
    }
    fn view(&self, detail: bool) -> Value {
        let mut v = json!({"id":self.id,"queue":self.queue,"phase":self.phase,"createdAt":self.created.to_rfc3339(),"phaseSince":self.since.to_rfc3339(),
        "endedAt":self.ended.map(|x|x.to_rfc3339()),"endReason":self.reason,"humanPlayers":self.players.len(),"botPlayers":10-self.players.len(),"poolId":null});
        v["tickLatency"] = self.latency.report();
        if detail {
            v["participants"]=Value::Array((0..10).map(|i|{let s=self.seats.get(i);
            json!({"slot":i,"team":self.world.entities[i].team,"playerId":self.players.get(i),"homeInstanceId":null,"botProfile":if s.is_none(){Some("lane-v1")}else{None},"displayName":if s.is_some(){"Guest".into()}else{format!("{} Bot",self.picks[i].name)},"hero":self.picks[i].name,"locked":s.is_none_or(|s|s.locked),"controlConnected":s.is_some_and(|s|s.peers[0].is_some()),"gameConnected":s.is_some_and(|s|s.peers[1].is_some()),"lastEventAt":s.and_then(|s|s.last).map(|x|x.to_rfc3339())})
        }).collect());
        }
        v
    }
    fn attach(
        &mut self,
        uuid: Uuid,
        tx: Sink,
        stalled: watch::Sender<bool>,
    ) -> Option<(usize, usize, u64, Batch)> {
        let idx = self.players.iter().position(|p| *p == uuid)?;
        if matches!(self.phase, "ending" | "ended") || self.events.len() >= 4096 {
            return None;
        }
        // A locked seat can open its data connection while other humans are still selecting.
        let game = self.seats[idx].locked
            && self.seats[idx].peers[1].is_none()
            && (self.seats[idx].peers[0].is_some() || self.phase == "running");
        let ch = usize::from(game);
        if self.seats[idx].peers[ch].is_some() {
            return None;
        }
        self.last_active = Instant::now();
        self.generation += 1;
        if game {
            // The reconnect snapshot represents precisely this simulation tick.
            let team = self.world.entities[idx].team;
            let visible = &mut self.visible[usize::from(team)];
            visible.clear();
            visible.extend(
                self.world
                    .entities
                    .iter()
                    .filter(|e| e.hp > 0. && self.world.visible(team, e))
                    .map(|e| e.id),
            );
        }
        self.seats[idx].peers[ch] = Some(Peer {
            generation: self.generation,
            tx,
            stalled,
        });
        self.seats[idx].last = Some(Utc::now());
        if self.phase == "allocated" {
            self.phase("hero_select");
        }
        self.event(
            "connection",
            "Authenticated",
            json!({"playerId":uuid,"channel":if game{"game"}else{"control"}}),
        );
        let mut batch = if game {
            world_header(&self.world, &self.players, &self.picks, &self.queue, idx)
        } else {
            setup(
                &self.world,
                &self.players,
                &self.catalog,
                idx,
                &self.queue,
                &self.picks,
            )
        };
        if game {
            batch.extend(self.spawn_view(Some(self.world.entities[idx].team)));
            if self.seats.iter().all(|s| s.peers[1].is_some()) {
                self.phase("running");
            }
        }
        Some((idx, ch, self.generation, batch))
    }
    fn spawn_view(&self, team: Option<u8>) -> Batch {
        let mut out = vec![];
        for e in &self.world.entities {
            if e.hp <= 0. || team.is_some_and(|t| !self.world.visible(t, e)) {
                continue;
            }
            out.push(spawn(e, &self.picks, self.data.as_deref()));
            if let Some(s) = self.seats.get((e.id - 1) as usize) {
                for &(_, item, instance) in &s.inventory {
                    out.push(messages::item_grant(
                        u32::from(wire_id(e.id)),
                        item,
                        instance,
                    ));
                }
            }
        }
        out
    }
    fn detach(&mut self, i: usize, ch: usize, generation: u64) {
        if self.seats[i].peers[ch]
            .as_ref()
            .is_some_and(|p| p.generation == generation)
        {
            self.seats[i].peers[ch] = None;
            self.seats[i].last = Some(Utc::now());
            self.event(
                "connection",
                "Disconnected",
                json!({"playerId":self.players[i],"channel":if ch==0{"control"}else{"game"}}),
            );
        }
    }
    fn roster_update(&self, except: Option<usize>) -> Message {
        let mut roster = snapshot(&self.world, &self.players, &self.picks);
        if self.phase == "hero_select" {
            roster.body[..4].copy_from_slice(
                &(300. - self.phase_start.elapsed().as_secs_f32())
                    .max(0.)
                    .to_be_bytes(),
            );
        }
        if self.phase == "loading" {
            roster.body[..4].copy_from_slice(
                &(7. - self.phase_start.elapsed().as_secs_f32())
                    .max(0.)
                    .to_be_bytes(),
            );
            roster.body[4..8].copy_from_slice(&7f32.to_be_bytes());
        }
        for (i, s) in self.seats.iter().enumerate() {
            if except != Some(i)
                && let Some(p) = &s.peers[0]
            {
                p.send(transport::Delivery::Private(vec![roster.clone()]));
            }
        }
        roster
    }
    fn validated_skin(&self, hero: &str, requested: u32) -> u32 {
        let default = halcyon_protocol::skin_hash(&format!("{hero}_DefaultSkin"));
        if requested == default {
            return default;
        }
        let prefix = format!("{hero}_");
        let valid = self.data.as_ref().is_some_and(|data| {
            data.skin_names.iter().any(|name| {
                name.starts_with(&prefix) && halcyon_protocol::skin_hash(name) == requested
            })
        });
        if valid { requested } else { default }
    }
    fn buy(&mut self, i: usize, actor: u32, item: u32) -> Batch {
        use halcyon_sim::BasicItem;
        let Some(offer) = BasicItem::from_template(item) else {
            return vec![];
        };
        let before = self.world.entities[i].gold;
        if self
            .world
            .command(i as u32 + 1, Command::Buy(offer))
            .is_err()
        {
            return vec![];
        }
        let paid = before - self.world.entities[i].gold;
        let mut ack = vec![
            Message::new(0x439, [actor.to_be_bytes(), item.to_be_bytes()].concat()),
            Message::entity_stat(actor as u16, -(paid as f32), 6),
        ];
        if matches!(offer, BasicItem::HeavySteel | BasicItem::SixSins)
            && let Some(slot) = self.seats[i]
                .inventory
                .iter()
                .position(|(k, _, _)| *k == BasicItem::WeaponBlade)
        {
            let (old, _, instance) = self.seats[i].inventory.remove(slot);
            let (stat, bonus) = self.world.item_bonus(old);
            ack.push(messages::modifier(actor, stat, -bonus));
            let mut b = [actor.to_be_bytes(), instance.to_be_bytes()].concat();
            b.extend(1u16.to_be_bytes());
            ack.push(Message::new(0x44b, b));
        }
        let instance = self.next_item;
        self.next_item += 1;
        self.seats[i].inventory.push((offer, item, instance));
        ack.push(messages::item_grant(actor, item, instance));
        let (stat, bonus) = self.world.item_bonus(offer);
        ack.push(messages::modifier(actor, stat, bonus));
        for (j, seat) in self.seats.iter().enumerate() {
            if j != i
                && self
                    .world
                    .visible(self.world.entities[j].team, &self.world.entities[i])
                && let Some(peer) = &seat.peers[1]
            {
                peer.send(transport::Delivery::Private(ack.clone()));
            }
        }
        self.publish_observers(&ack);
        ack
    }
    fn receive(&mut self, i: usize, ch: usize, m: Message) -> std::io::Result<Batch> {
        self.last_active = Instant::now();
        let seen = &mut self.seats[i].seen[ch];
        if m.opcode != 0x455 && seen.len() < 32 && seen.insert(m.opcode) {
            self.event("protocol","First client message of this type",json!({"playerId":self.players[i],"channel":if ch==0{"control"}else{"game"},"opcode":format!("0x{:04X}",m.opcode)}));
        }
        let mut out = vec![];
        match m.fields()? {
            Fields::Movement { x, y } if ch == 1 && self.phase == "running" => {
                let _ = self
                    .world
                    .command(i as u32 + 1, Command::MoveTo(Position { x, y }));
            }
            Fields::ShopRequest { actor, item }
                if ch == 1
                    && self.phase == "running"
                    && actor == u32::from(wire_id(i as u32 + 1)) =>
            {
                out = self.buy(i, actor, item);
            }
            Fields::HeroLock if ch == 0 => {
                let op = m.opcode;
                out.push(m);
                if !self.seats[i].locked {
                    self.seats[i].locked = true;
                    self.picks[i].selected = true;
                    self.picks[i].locked = true;
                    self.event(
                        "protocol",
                        "Hero locked",
                        json!({"playerId":self.players[i],"opcode":format!("0x{op:04X}")}),
                    );
                    out.push(self.roster_update(Some(i)));
                    if self.seats.iter().all(|s| s.locked) {
                        self.phase("loading");
                    }
                }
            }
            Fields::HeroSelection { template, skin } if ch == 0 && !self.seats[i].locked => {
                let name = if template == 243 {
                    Some("Ringo".to_owned())
                } else {
                    self.data.as_ref().and_then(|d| {
                        d.manifest
                            .iter()
                            .find(|e| {
                                u32::from(e.entity_type_id) == template
                                    && e.path.starts_with("build://Characters/")
                            })
                            .map(|e| e.name.trim_matches('*').to_owned())
                    })
                };
                if let Some(name) = name {
                    let valid = if let Some(d) = &self.data {
                        self.world.select_hero(i, &name, d).is_ok()
                    } else {
                        name == "Ringo"
                    };
                    if valid {
                        let skin = self.validated_skin(&name, skin);
                        self.picks[i] = Pick {
                            name,
                            skin,
                            selected: true,
                            locked: false,
                        };
                        out.push(self.roster_update(Some(i)));
                    }
                }
                out.push(m);
            }
            Fields::SkinSelection(skin) if ch == 0 => {
                self.picks[i].skin = self.validated_skin(&self.picks[i].name, skin);
                out.push(self.roster_update(Some(i)));
                out.push(m);
            }
            Fields::Clock(_) | Fields::Heartbeat => out.push(m),
            _ if [0x458, 0x45e, 0x46b, 0x45f, 0x46e, 0x471, 0x46d].contains(&m.opcode) => {
                out.push(m)
            }
            _ => {}
        }
        Ok(out)
    }
    fn tick(&mut self) {
        if !self.admitted {
            return;
        }
        if self.last_active.elapsed().as_secs() > 300
            && self
                .seats
                .iter()
                .all(|s| s.peers.iter().all(Option::is_none))
        {
            self.reason = Some("error");
            self.phase("ending");
            self.stop.send_replace(true);
            return;
        }
        if matches!(self.phase, "hero_select" | "loading")
            && self.last_roster.elapsed().as_secs() >= 1
        {
            self.last_roster = Instant::now();
            self.roster_update(None);
        }

        // Native CE control admissions stay in loading until a data channel authenticates.
        // Replay chunk headers share opcodes with TCP, but are not a control transition.
        if self.phase != "running" {
            return;
        }
        let events = self.world.step();
        self.tick_observers(&events);
        for event in &events {
            match *event {
                Event::Death { id, killer } if (1..=10).contains(&id) => {
                    self.combat[(id - 1) as usize].deaths += 1;
                    if (1..=10).contains(&killer) {
                        self.combat[(killer - 1) as usize].kills += 1;
                    }
                }
                Event::Damage { source, amount, .. } if (1..=10).contains(&source) => {
                    self.combat[(source - 1) as usize].damage += f64::from(amount)
                }
                Event::Heal { source, amount, .. } if (1..=10).contains(&source) => {
                    self.combat[(source - 1) as usize].healing += f64::from(amount)
                }
                _ => {}
            }
        }
        self.replicate(&events);
        if self.world.winner.is_some() {
            self.reason = Some("completed");
            self.phase("ending");
            self.stop.send_replace(true);
        }
    }

    fn replicate(&mut self, events: &[Event]) {
        let fallen: HashSet<u32> = events
            .iter()
            .filter_map(|event| match event {
                Event::Death { id, .. } => Some(*id),
                _ => None,
            })
            .collect();
        for team in 0..2 {
            let mut peers = self
                .seats
                .iter()
                .zip(&self.world.entities)
                .filter(|(_, actor)| usize::from(actor.team) == team)
                .filter_map(|(seat, _)| seat.peers[1].as_ref())
                .peekable();
            let Some(first) = peers.next() else {
                continue;
            };
            let mut out = vec![];
            let previous = &mut self.visible[team];
            let current = &mut self.next_visible[team];
            current.clear();
            for e in &self.world.entities {
                if e.hp > 0. && self.world.visible(team as u8, e) {
                    current.insert(e.id);
                    if !previous.contains(&e.id) {
                        out.push(spawn(e, &self.picks, self.data.as_deref()));
                    }
                }
            }
            for ev in events {
                if let Event::Move { id, position } = ev
                    && current.contains(id)
                {
                    out.push(Message::position(wire_id(*id), position.x, position.y));
                } else {
                    messages::append_combat(&mut out, ev, |actor| {
                        current.contains(&actor)
                            || (fallen.contains(&actor) && previous.contains(&actor))
                    });
                }
            }
            std::mem::swap(previous, current);
            if self.world.tick.is_multiple_of(20) {
                out.push(snapshot(&self.world, &self.players, &self.picks));
            }
            if out.is_empty() {
                continue;
            }
            if peers.peek().is_none() {
                first.send(transport::Delivery::Private(out));
            } else {
                let batch = transport::TeamBatch::new(out);
                for peer in std::iter::once(first).chain(peers) {
                    peer.send(transport::Delivery::Team(batch.clone()));
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn shop_uses_authenticated_seat_and_authoritative_gold() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                match_id: id,
                players: vec![Uuid::new_v4()],
                teams: vec![0],
                mode: "solo".into(),
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut room = shared.lock().unwrap();
        room.phase("running");
        room.world.entities[0].gold = 300;
        let buy =
            |actor: u32| Message::new(0x439, [actor.to_be_bytes(), 458u32.to_be_bytes()].concat());
        assert!(room.receive(0, 1, buy(1501)).unwrap().is_empty());
        assert!(room.receive(0, 0, buy(1500)).unwrap().is_empty());
        assert_eq!(room.world.entities[0].gold, 300);
        let ack = room.receive(0, 1, buy(1500)).unwrap();
        assert_eq!(
            ack.iter().map(|m| m.opcode).collect::<Vec<_>>(),
            [0x439, 0x41d, 0x43d, 0x41c]
        );
        assert_eq!(room.world.entities[0].gold, 0);
        assert!(matches!(
            ack[1].fields().unwrap(),
            Fields::EntityStat {
                entity: 1500,
                value: -300.,
                stat: 6
            }
        ));
        assert!(matches!(
            ack[2].fields().unwrap(),
            Fields::ItemGrant {
                actor: 1500,
                item: 458,
                ..
            }
        ));
        assert!(room.receive(0, 1, buy(1500)).unwrap().is_empty());
        room.world.entities[0].gold = 850;
        let upgrade = Message::new(
            0x439,
            [1500u32.to_be_bytes(), 505u32.to_be_bytes()].concat(),
        );
        let upgraded = room.receive(0, 1, upgrade).unwrap();
        assert_eq!(
            upgraded.iter().map(|m| m.opcode).collect::<Vec<_>>(),
            [0x439, 0x41d, 0x41c, 0x44b, 0x43d, 0x41c]
        );
        assert_eq!(&upgraded[3].body[4..8], &ack[2].body[8..12]);
        assert_eq!(room.seats[0].inventory.len(), 1);
    }
}
