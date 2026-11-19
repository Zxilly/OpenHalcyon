//! In-process room hosting with CE control and game TCP connections.
mod messages;
mod transport;
use chrono::{DateTime, Utc};
use halcyon_data::GameData;
use halcyon_platform::{HostedMatch, MatchHost, PlatformError, Reservation};
use halcyon_protocol::{Fields, Message};
use halcyon_sim::{Command, Event, Position, World};
use messages::{setup, snapshot, spawn, wire_id};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    net::{IpAddr, TcpListener},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};
use tokio::sync::{mpsc, watch};
use uuid::Uuid;

type Batch = Vec<Message>;
type Sink = mpsc::Sender<Batch>;
type Finish = Arc<dyn Fn(Uuid, Value) + Send + Sync>;
type Shared = Arc<Mutex<Room>>;
#[derive(Clone)]
pub struct Rooms(Arc<Manager>);
struct Manager {
    bind_ip: IpAddr,
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
    visible: HashSet<u32>,
    stalled: watch::Sender<bool>,
}
#[derive(Default)]
struct Seat {
    locked: bool,
    peers: [Option<Peer>; 2],
    last: Option<DateTime<Utc>>,
}
struct Room {
    id: Uuid,
    address: String,
    queue: String,
    players: Vec<Uuid>,
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
}
impl Rooms {
    pub fn new(
        bind_ip: IpAddr,
        advertised_host: String,
        capacity: usize,
        data: Option<Arc<GameData>>,
    ) -> Self {
        Self(Arc::new(Manager {
            bind_ip,
            host: advertised_host,
            capacity,
            data,
            rooms: Mutex::default(),
            finish: Mutex::default(),
            bytes: AtomicU64::default(),
        }))
    }
    pub fn set_finish_handler(&self, handler: Finish) {
        *self.0.finish.lock().unwrap() = Some(handler);
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
            (r.id, json!({"reason":end,"winner":r.world.winner}))
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
    fn reserve(&self, req: Reservation) -> Result<HostedMatch, PlatformError> {
        let mut rooms = self.0.rooms.lock().unwrap();
        rooms.retain(|_, room| {
            room.lock()
                .unwrap()
                .ended
                .is_none_or(|end| Utc::now() - end < chrono::Duration::minutes(30))
        });
        if let Some(r) = rooms.get(&req.match_id) {
            let r = r.lock().unwrap();
            if r.queue != req.mode || r.players != req.players || r.ended.is_some() {
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
        if rooms
            .values()
            .filter(|r| r.lock().unwrap().ended.is_none())
            .count()
            >= self.0.capacity
        {
            return Err(PlatformError::HostUnavailable);
        }
        let rt =
            tokio::runtime::Handle::try_current().map_err(|_| PlatformError::HostUnavailable)?;
        let listener =
            TcpListener::bind((self.0.bind_ip, 0)).map_err(|_| PlatformError::HostUnavailable)?;
        listener
            .set_nonblocking(true)
            .map_err(|_| PlatformError::HostUnavailable)?;
        let port = listener
            .local_addr()
            .map_err(|_| PlatformError::HostUnavailable)?
            .port();
        let host = &self.0.host;
        let address = if host.contains(':') && !host.starts_with('[') {
            format!("[{host}]:{port}")
        } else {
            format!("{host}:{port}")
        };
        let seed = req.match_id.as_u128() as u64;
        let n = req.players.len() as u8;
        let world = match &self.0.data {
            Some(d) => World::from_data(seed, n, d).map_err(|_| PlatformError::HostUnavailable)?,
            None => World::new(seed, n),
        };
        let mut catalog = vec!["*Ringo*".to_owned()];
        if let Some(d) = &self.0.data {
            for entry in &d.manifest {
                if entry.path.starts_with("build://Characters/") && entry.name.len() <= 32 {
                    catalog.push(entry.name.clone());
                }
            }
            catalog.sort();
            catalog.dedup();
        }
        let listener = tokio::net::TcpListener::from_std(listener)
            .map_err(|_| PlatformError::HostUnavailable)?;
        let (stop, stopped) = watch::channel(false);
        let t = Utc::now();
        let mut room = Room {
            id: req.match_id,
            address: address.clone(),
            queue: req.mode,
            players: req.players,
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
            self.since = Utc::now();
            self.event("phase_changed", phase, json!({}));
        }
    }
    fn view(&self, detail: bool) -> Value {
        let mut v = json!({"id":self.id,"queue":self.queue,"phase":self.phase,"createdAt":self.created.to_rfc3339(),"phaseSince":self.since.to_rfc3339(),
        "endedAt":self.ended.map(|x|x.to_rfc3339()),"endReason":self.reason,"humanPlayers":self.players.len(),"botPlayers":10-self.players.len(),"poolId":null});
        if detail {
            v["participants"]=Value::Array((0..10).map(|i|{let s=self.seats.get(i);
            json!({"slot":i,"playerId":self.players.get(i),"homeInstanceId":null,"botProfile":if s.is_none(){Some("lane-v1")}else{None},"displayName":if s.is_some(){"Guest".into()}else{format!("Ringo Bot {}",i+1)},"hero":"Ringo","locked":s.is_none_or(|s|s.locked),"controlConnected":s.is_some_and(|s|s.peers[0].is_some()),"gameConnected":s.is_some_and(|s|s.peers[1].is_some()),"lastEventAt":s.and_then(|s|s.last).map(|x|x.to_rfc3339())})
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
        let game = self.seats[idx].locked && self.seats[idx].peers[1].is_none();
        let ch = usize::from(game);
        if self.seats[idx].peers[ch].is_some() {
            return None;
        }
        self.last_active = Instant::now();
        self.generation += 1;
        self.seats[idx].peers[ch] = Some(Peer {
            generation: self.generation,
            tx,
            stalled,
            visible: if game {
                self.world
                    .entities
                    .iter()
                    .filter(|e| self.world.visible((idx % 2) as u8, e))
                    .map(|e| e.id)
                    .collect()
            } else {
                HashSet::new()
            },
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
        let mut batch = setup(&self.world, &self.players, &self.catalog, idx, &self.queue);
        if game {
            batch.extend(
                self.world
                    .entities
                    .iter()
                    .filter(|e| self.world.visible((idx % 2) as u8, e))
                    .map(spawn),
            );
            if self.seats.iter().all(|s| s.peers[1].is_some()) {
                self.phase("running");
            }
        }
        Some((idx, ch, self.generation, batch))
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
    fn receive(&mut self, i: usize, ch: usize, m: Message) -> std::io::Result<Batch> {
        self.last_active = Instant::now();
        let mut out = vec![];
        match m.fields()? {
            Fields::Movement { x, y } if ch == 1 && self.phase == "running" => {
                let _ = self
                    .world
                    .command(i as u32 + 1, Command::MoveTo(Position { x, y }));
            }
            Fields::HeroLock if ch == 0 => {
                if !self.seats[i].locked {
                    self.seats[i].locked = true;
                    self.event(
                        "protocol",
                        "Hero locked",
                        json!({"playerId":self.players[i],"opcode":"0x0455"}),
                    );
                    if self.seats.iter().all(|s| s.locked) {
                        self.phase("loading");
                    }
                }
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
        if self.phase != "running" {
            return;
        }
        let events = self.world.step();
        // Both teams share visibility, so compute at most twice per tick.
        let visible: [HashSet<u32>; 2] = std::array::from_fn(|team| {
            self.world
                .entities
                .iter()
                .filter(|e| e.hp > 0. && self.world.visible(team as u8, e))
                .map(|e| e.id)
                .collect()
        });
        for (i, seat) in self.seats.iter_mut().enumerate() {
            let Some(peer) = &mut seat.peers[1] else {
                continue;
            };
            let mut out = vec![];
            let team = i % 2;
            let current = &visible[team];
            for e in &self.world.entities {
                if current.contains(&e.id) && !peer.visible.contains(&e.id) {
                    out.push(spawn(e));
                }
            }

            for ev in &events {
                let id = match ev {
                    Event::Move { id, .. } => *id,
                    Event::Damage { target, .. } => *target,
                    _ => continue,
                };
                if !current.contains(&id) && !peer.visible.contains(&id) {
                    continue;
                }
                out.push(match ev {
                    Event::Move { position, .. } => {
                        Message::position(wire_id(id), position.x, position.y)
                    }
                    Event::Damage { amount, .. } => Message::entity_stat(wire_id(id), -*amount, 6),
                    _ => unreachable!(),
                });
            }
            peer.visible.clone_from(current);
            if self.world.tick.is_multiple_of(20) {
                out.push(snapshot(&self.world, &self.players));
            }
            if self.world.winner.is_some() {
                out.push(Message::new(0x485, [0; 6]));
            }
            if !out.is_empty() {
                if peer.tx.try_send(out).is_err() {
                    peer.stalled.send_replace(true);
                }
            }
        }
        if self.world.winner.is_some() {
            self.reason = Some("completed");
            self.phase("ending");
            self.stop.send_replace(true);
        }
    }
}
