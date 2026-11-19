//! In-process room hosting with CE control and game TCP connections.
mod messages;
mod transport;
use chrono::{DateTime, Utc};
use halcyon_data::GameData;
use halcyon_platform::{HostedMatch, MatchHost, PlatformError, Reservation};
use halcyon_protocol::{Fields, Message};
use halcyon_sim::{Command, Event, Position, World};
use messages::{setup, snapshot, spawn, wire_id, world_header};
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
impl Peer {
    fn send(&self, batch: Batch) {
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
}
struct Pick {
    name: String,
    skin: u32,
    selected: bool,
    locked: bool,
}
struct Room {
    admitted: bool,
    data: Option<Arc<GameData>>,
    picks: Vec<Pick>,
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
        let mut rooms = self.0.rooms.lock().unwrap();
        rooms.retain(|_, room| {
            room.lock()
                .unwrap()
                .ended
                .is_none_or(|end| Utc::now() - end < chrono::Duration::minutes(30))
        });
        if let Some(r) = rooms.get(&req.match_id) {
            let r = r.lock().unwrap();
            if r.queue != req.mode
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
            admitted,
            data: self.0.data.clone(),
            picks: (0..10)
                .map(|i| Pick {
                    name: "Ringo".into(),
                    skin: halcyon_protocol::skin_hash("Ringo_DefaultSkin"),
                    selected: i >= usize::from(n),
                    locked: i >= usize::from(n),
                })
                .collect(),
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
        if detail {
            v["participants"]=Value::Array((0..10).map(|i|{let s=self.seats.get(i);
            json!({"slot":i,"playerId":self.players.get(i),"homeInstanceId":null,"botProfile":if s.is_none(){Some("lane-v1")}else{None},"displayName":if s.is_some(){"Guest".into()}else{format!("Ringo Bot {}",i+1)},"hero":self.picks[i].name,"locked":s.is_none_or(|s|s.locked),"controlConnected":s.is_some_and(|s|s.peers[0].is_some()),"gameConnected":s.is_some_and(|s|s.peers[1].is_some()),"lastEventAt":s.and_then(|s|s.last).map(|x|x.to_rfc3339())})
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
            batch.extend(
                self.world
                    .entities
                    .iter()
                    .filter(|e| self.world.visible((idx % 2) as u8, e))
                    .map(|e| spawn(e, &self.picks, self.data.as_deref())),
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
    fn roster_update(&self, except: Option<usize>) -> Message {
        let mut roster = snapshot(&self.world, &self.players, &self.picks);
        if self.phase == "hero_select" {
            roster.body[..4].copy_from_slice(
                &(300. - self.phase_start.elapsed().as_secs_f32())
                    .max(0.)
                    .to_be_bytes(),
            );
        }
        for (i, s) in self.seats.iter().enumerate() {
            if except != Some(i)
                && let Some(p) = &s.peers[0]
            {
                p.send(vec![roster.clone()]);
            }
        }
        roster
    }
    fn receive(&mut self, i: usize, ch: usize, m: Message) -> std::io::Result<Batch> {
        self.last_active = Instant::now();
        let seen = &mut self.seats[i].seen[ch];
        if m.opcode != 0x455 && seen.len() < 32 && seen.insert(m.opcode) {
            self.event("protocol","First client message of this type",json!({"playerId":self.players[i],"channel":if ch==0{"control"}else{"game"},"opcode":format!("0x{:04X}",m.opcode)}));
        }
        let mut out = vec![];
        match m.fields()? {
            Fields::Movement { x, y }
                if (ch == 1 || self.seats[i].peers[1].is_none()) && self.phase == "running" =>
            {
                let _ = self
                    .world
                    .command(i as u32 + 1, Command::MoveTo(Position { x, y }));
            }
            Fields::HeroLock if ch == 0 => {
                out.push(m);
                if !self.seats[i].locked {
                    self.seats[i].locked = true;
                    self.picks[i].selected = true;
                    self.picks[i].locked = true;
                    self.event(
                        "protocol",
                        "Hero locked",
                        json!({"playerId":self.players[i],"opcode":"0x0455"}),
                    );
                    out.push(self.roster_update(Some(i)));
                    if self.seats.iter().all(|s| s.locked) {
                        self.phase("loading");
                        // Some CE builds continue on control. A separate game socket
                        // will receive the same world header at its own attach time.
                        for j in 0..self.seats.len() {
                            if self.seats[j].peers[1].is_some() {
                                continue;
                            }
                            let mut header = world_header(
                                &self.world,
                                &self.players,
                                &self.picks,
                                &self.queue,
                                j,
                            );
                            header.extend(
                                self.world
                                    .entities
                                    .iter()
                                    .filter(|e| self.world.visible((j % 2) as u8, e))
                                    .map(|e| spawn(e, &self.picks, self.data.as_deref())),
                            );
                            if j == i {
                                out.extend(header);
                            } else if let Some(peer) = &self.seats[j].peers[0] {
                                peer.send(header);
                            }
                        }
                    }
                }
            }
            _ if m.opcode == 0x45e && ch == 0 && !self.seats[i].locked && m.body.len() >= 8 => {
                let template = u32::from_be_bytes(m.body[..4].try_into().unwrap());
                let skin = u32::from_be_bytes(m.body[4..8].try_into().unwrap());
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
        if self.phase == "hero_select" && self.last_roster.elapsed().as_secs() >= 1 {
            self.last_roster = Instant::now();
            self.roster_update(None);
        }
        if self.phase == "loading" && self.phase_start.elapsed().as_secs() >= 2 {
            self.phase("running");
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
            let channel = usize::from(seat.peers[1].is_some());
            let Some(peer) = &mut seat.peers[channel] else {
                continue;
            };
            let mut out = vec![];
            let team = i % 2;
            let current = &visible[team];
            for e in &self.world.entities {
                if current.contains(&e.id) && !peer.visible.contains(&e.id) {
                    out.push(spawn(e, &self.picks, self.data.as_deref()));
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
                out.push(snapshot(&self.world, &self.players, &self.picks));
            }
            if self.world.winner.is_some() {
                out.push(Message::new(0x485, [0; 6]));
            }
            if !out.is_empty() {
                peer.send(out);
            }
        }
        if self.world.winner.is_some() {
            self.reason = Some("completed");
            self.phase("ending");
            self.stop.send_replace(true);
        }
    }
}
