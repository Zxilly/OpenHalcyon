use super::admin_views::{self, EventDetail, EventKind};
use super::{admission, registry};
use crate::modes::{draft_5v5, fold_blitz};
#[cfg(feature = "test-hooks")]
use crate::replication::messages::wire_id;
use crate::replication::messages::{self};
use crate::replication::slots::actor_slots;
use crate::room::actions::surrender;
use crate::room::tick_latency::TickLatency;
use crate::room::{Combat, FederationOrigin, Pick, Room, Seat};
use crate::transport;
use chrono::Utc;
use halcyon_data::GameData;
use halcyon_protocol::ce::KnownQueue;
#[cfg(feature = "test-hooks")]
use halcyon_sim::Command;
use halcyon_sim::World;
use halcyon_types::{
    HostError, HostResult, HostedMatch, HostedSpectator, MatchHost, Reservation, SpectatorExit,
};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    net::IpAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};
use tokio::sync::watch;
use uuid::Uuid;

pub(crate) type Finish = Arc<dyn Fn(Uuid, Value) + Send + Sync>;
pub(crate) type Shared = Arc<Mutex<Room>>;

#[derive(Clone)]
pub struct Rooms(pub(crate) Arc<Manager>);

pub(crate) struct Manager {
    pub(crate) free_aral_reroll: AtomicBool,
    pub(crate) unlock_all_talents: AtomicBool,
    pub(crate) bind_ip: IpAddr,
    pub(crate) replay_directory: Mutex<Option<std::path::PathBuf>>,
    pub(crate) gate: Mutex<admission::Gate>,
    pub(crate) monitor_started: AtomicBool,
    pub(crate) ports: admission::Ports,
    pub(crate) host: String,
    pub(crate) capacity: usize,
    pub(crate) data: Option<Arc<GameData>>,
    pub(crate) catalog: Arc<[[u8; 32]]>,
    pub(crate) rules: Option<halcyon_sim::WorldRules>,
    pub(crate) rooms: Mutex<registry::RoomRegistry>,
    pub(crate) finish: Mutex<Option<Finish>>,
    pub(crate) bytes: AtomicU64,
}

#[derive(Debug, PartialEq)]
pub enum TerminateError {
    Missing,
    Conflict,
    Invalid,
}

pub(crate) fn now() -> String {
    Utc::now().to_rfc3339()
}

impl Rooms {
    pub fn game_data(&self) -> Option<&Arc<GameData>> {
        self.0.data.as_ref()
    }

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
            admission::AdmissionPolicy::default(),
            None,
        )
        .expect("valid default room policy")
    }

    pub fn with_policy(
        bind_ip: IpAddr,
        advertised_host: String,
        capacity: usize,
        data: Option<Arc<GameData>>,
        policy: admission::AdmissionPolicy,
        game_port_range: Option<admission::PortRange>,
    ) -> HostResult<Self> {
        let ports = admission::Ports::new(game_port_range)?;
        let capacity = capacity.min(ports.slots());
        let this = Self(Arc::new(Manager {
            free_aral_reroll: AtomicBool::default(),
            unlock_all_talents: AtomicBool::default(),
            gate: Mutex::new(admission::Gate::new(policy)?),
            monitor_started: AtomicBool::default(),
            ports,
            replay_directory: Mutex::default(),
            bind_ip,
            host: advertised_host,
            capacity,
            rules: data
                .as_deref()
                .map(halcyon_sim::WorldRules::load)
                .transpose()
                .map_err(|error| {
                    tracing::error!(%error, "invalid simulation rules");
                    HostError::Invalid("invalid simulation rules")
                })?,
            catalog: messages::catalog_entries(data.as_deref()),
            data,
            rooms: Mutex::default(),
            finish: Mutex::default(),
            bytes: AtomicU64::default(),
        }));
        this.monitor_resources();
        Ok(this)
    }

    /// Apply the optional free draft reroll to rooms reserved after this call.
    pub fn set_free_aral_reroll(&self, enabled: bool) {
        self.0.free_aral_reroll.store(enabled, Ordering::Relaxed);
    }

    pub fn free_aral_reroll(&self) -> bool {
        self.0.free_aral_reroll.load(Ordering::Relaxed)
    }

    /// Unlock all hero talents in-match with CE official levels (`10/10/5`) when inventory has no explicit grant.
    pub fn set_unlock_all_talents(&self, enabled: bool) {
        self.0.unlock_all_talents.store(enabled, Ordering::Relaxed);
    }

    pub fn unlock_all_talents(&self) -> bool {
        self.0.unlock_all_talents.load(Ordering::Relaxed)
    }

    pub fn set_finish_handler(&self, handler: Finish) {
        *self.0.finish.lock().unwrap() = Some(handler);
    }

    pub fn clear_finish_handler(&self) {
        self.0.finish.lock().unwrap().take();
    }

    /// Active rooms, admitted human and bot seats, and all staged seats.
    pub fn hosting_counts(&self) -> (usize, usize, usize, usize) {
        let mut rooms = self.0.rooms.lock().unwrap();
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
                        bots + room.picks.len() - room.players.len(),
                        prepared,
                    )
                } else {
                    (active + 1, human, bots, prepared + room.picks.len())
                }
            })
    }

    pub(crate) fn advertise(&self, port: u16) -> String {
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
        self.list_typed().iter().map(admin_views::value).collect()
    }

    pub fn list_typed(&self) -> Vec<admin_views::RoomSummary> {
        let rooms = self.0.rooms.lock().unwrap().handles();
        rooms
            .into_iter()
            .map(|r| r.lock().unwrap().view(false))
            .collect()
    }

    pub(crate) fn room(&self, id: Uuid) -> Option<Shared> {
        self.0.rooms.lock().unwrap().get(&id).cloned()
    }

    pub fn detail(&self, id: Uuid) -> Option<Value> {
        self.detail_typed(id).map(admin_views::value)
    }

    pub fn detail_typed(&self, id: Uuid) -> Option<admin_views::RoomSummary> {
        Some(self.room(id)?.lock().unwrap().view(true))
    }

    pub fn contains(&self, id: Uuid) -> bool {
        self.0.rooms.lock().unwrap().get(&id).is_some()
    }

    pub fn events(&self, id: Uuid, after: u64, limit: usize) -> Option<Value> {
        let r = self.room(id)?;
        let r = r.lock().unwrap();
        let start = (after as usize).min(r.events.len());
        let end = start.saturating_add(limit).min(r.events.len());
        Some(admin_views::value(admin_views::EventPage {
            items: &r.events[start..end],
            has_more: end < r.events.len(),
            next_after: if start == end { after } else { end as u64 },
        }))
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
            return if prior.reason == reason {
                Ok((r.ended.is_some(), admin_views::value(prior)))
            } else {
                Err(TerminateError::Conflict)
            };
        }
        if matches!(r.phase, crate::RoomPhase::Ending | crate::RoomPhase::Ended) {
            return Err(TerminateError::Conflict);
        }
        let termination = admin_views::Termination {
            match_id: id,
            requested_at: now(),
            requested_by: actor.to_owned(),
            reason: reason.to_owned(),
        };
        let response = admin_views::value(&termination);
        r.termination = Some(termination);
        r.reason = Some(crate::RoomEndReason::AdminTerminated);
        r.event(
            EventKind::AdminAction,
            "Termination requested",
            EventDetail::Admin {
                actor: actor.to_owned(),
            },
        );
        r.phase(crate::RoomPhase::Ending);
        r.stop.send_replace(true);
        Ok((false, response))
    }

    pub(crate) async fn finished(&self, room: &Shared) {
        let (id, result) = {
            let mut r = room.lock().unwrap();

            let end = r.reason.unwrap_or(crate::RoomEndReason::Error);
            r.reason = Some(end);
            r.ended = Some(Utc::now());
            r.phase(crate::RoomPhase::Ended);
            if !r.admitted {
                return;
            }
            let players = r
                .players
                .iter()
                .enumerate()
                .map(|(seat, id)| {
                    let actor = &r.world.entities[seat];
                    let combat = &r.combat[(actor.id - 1) as usize];
                    let score = r.world.player_score(actor.id).expect("reserved hero actor");
                    (
                        *id,
                        admin_views::PlayerResult {
                            score,
                            combat,
                            team: actor.team,
                            victory: r.world.winner.map(|w| actor.team == w),
                            hero: &r.picks[seat].name,
                            gold: actor.gold,
                            item_templates: actor
                                .inventory
                                .iter()
                                .map(|held| held.kind.template())
                                .collect(),
                            level: actor.level,
                            xp: actor.xp,
                        },
                    )
                })
                .collect();
            (
                r.id,
                admin_views::value(admin_views::ResultReport {
                    reason: end.as_str(),
                    surrendered_team: r.surrender.iter().position(surrender::Ballot::passed),
                    winner: r.world.winner,
                    ticks: r.world.tick,
                    players,
                    teams: r.world.team_scores(),
                }),
            )
        };
        let handler = self.0.finish.lock().unwrap().clone();
        if let Some(f) = handler
            && let Err(error) = tokio::task::spawn_blocking(move || f(id, result)).await
        {
            tracing::error!(%id, %error, "finish handler task failed");
        }
    }

    pub fn prepare(&self, req: Reservation) -> HostResult<HostedMatch> {
        self.allocate(req, false, None)
    }

    /// Prepare a room from the committed allocation and its ordered ticket roster.
    pub fn prepare_federated(
        &self,
        req: Reservation,
        pool: Uuid,
        homes: Vec<Uuid>,
    ) -> HostResult<HostedMatch> {
        if homes.len() != req.players.len() {
            return Err(HostError::Invalid("invalid home roster"));
        }
        self.allocate(req, false, Some(FederationOrigin { pool, homes }))
    }

    pub fn activate(&self, id: Uuid) -> HostResult<()> {
        let rooms = self.0.rooms.lock().unwrap();
        let mut room = rooms
            .get(&id)
            .ok_or(HostError::HostUnavailable)?
            .lock()
            .unwrap();
        if matches!(
            room.phase,
            crate::RoomPhase::Ending | crate::RoomPhase::Ended
        ) {
            return Err(HostError::Conflict("closed reservation"));
        }
        room.admitted = true;
        room.admission_ready.send_replace(true);
        Ok(())
    }

    pub fn discard(&self, id: Uuid) -> HostResult<()> {
        let mut rooms = self.0.rooms.lock().unwrap();
        if let Some(room) = rooms.get(&id) {
            let room = room.lock().unwrap();
            if room.admitted {
                return Err(HostError::Conflict("already active"));
            }
            room.stop.send_replace(true);
        }
        rooms.remove(&id);
        Ok(())
    }

    #[cfg(feature = "test-hooks")]
    #[doc(hidden)]
    pub fn advance_solo_loading(&self, id: Uuid) {
        if let Some(room) = self.room(id) {
            room.lock().unwrap().phase_start = Instant::now() - std::time::Duration::from_secs(8);
        }
    }

    #[cfg(feature = "test-hooks")]
    #[doc(hidden)]
    pub fn defeat_hero(&self, id: Uuid, seat: usize) {
        if let Some(room) = self.room(id) {
            let mut r = room.lock().unwrap();
            let victim_team = r.world.entities[seat].team;
            // Prefer a base/armory tower with no allied wave nearby to take agro cleanly.
            let tower_pos = r
                .world
                .entities
                .iter()
                .rev()
                .find(|e| e.kind == halcyon_sim::Kind::Tower && e.team != victim_team && e.hp > 0.0)
                .expect("living opposing tower")
                .position;
            r.world.entities[seat].position = tower_pos;
            r.world.entities[seat].hp = 0.1;
            for _ in 0..60 {
                if r.world.entities[seat].hp <= 0.0 {
                    return;
                }
                r.tick();
            }
            panic!(
                "defeat_hero did not kill seat {seat} (hp={})",
                r.world.entities[seat].hp
            );
        }
    }

    #[cfg(feature = "test-hooks")]
    #[doc(hidden)]
    pub fn step_room_tick(&self, id: Uuid) {
        if let Some(room) = self.room(id) {
            room.lock().unwrap().tick();
        }
    }

    #[cfg(feature = "test-hooks")]
    #[doc(hidden)]
    pub fn is_hero_alive(&self, id: Uuid, seat: usize) -> bool {
        self.room(id)
            .is_some_and(|room| room.lock().unwrap().world.entities[seat].hp > 0.0)
    }

    #[cfg(feature = "test-hooks")]
    #[doc(hidden)]
    pub async fn prepare_crystal_defeat(&self, id: Uuid, attacker_seat: usize) -> Option<u32> {
        let room = self.room(id)?;
        let (attacker_id, attacker_team, armory_info) = {
            let mut r = room.lock().unwrap();
            let attacker_id = r.world.entities[attacker_seat].id;
            let attacker_team = r.world.entities[attacker_seat].team;
            let armory_info = r
                .world
                .entities
                .iter()
                .find(|e| {
                    e.kind == halcyon_sim::Kind::Armory && e.team != attacker_team && e.hp > 0.0
                })
                .map(|e| (e.id, e.position, e.max_hp));
            if let Some((armory_id, armory_pos, _)) = armory_info {
                for i in 0..r.world.entities.len() {
                    if r.world.entities[i].kind == halcyon_sim::Kind::Tower
                        && r.world.entities[i].team != attacker_team
                    {
                        r.world.entities[i].hp = 0.0;
                    }
                }
                r.world.entities[attacker_seat].hp = r.world.entities[attacker_seat].max_hp;
                r.world.entities[attacker_seat].position = armory_pos;
                r.world
                    .command(attacker_id, Command::Attack(armory_id))
                    .unwrap();
            }
            (attacker_id, attacker_team, armory_info)
        };
        if let Some((armory_id, _, max_hp)) = armory_info {
            let mut damaged = false;
            let mut breached = false;
            for _ in 0..80 {
                {
                    let mut r = room.lock().unwrap();
                    let armory = r.world.entities.iter().find(|e| e.id == armory_id);
                    if armory.is_none_or(|e| e.hp <= 0.0) {
                        breached = true;
                        break;
                    }
                    // After the first real hit sets `armory.respawn`, lower HP to 0.1 so the
                    // next attack finishes the breach without `advance_armory` healing it first.
                    if !damaged && armory.is_some_and(|e| e.hp < max_hp) {
                        damaged = true;
                        r.world.entities.find_mut(|e| e.id == armory_id).unwrap().hp = 0.1;
                    }
                    r.world.entities[attacker_seat].hp = r.world.entities[attacker_seat].max_hp;
                    r.tick();
                }
                tokio::task::yield_now().await;
            }
            assert!(
                breached,
                "armory {armory_id} was not breached within 80 ticks"
            );
            room.lock()
                .unwrap()
                .world
                .command(attacker_id, Command::Stop)
                .unwrap();
        }
        let mut r = room.lock().unwrap();
        let crystal = r
            .world
            .entities
            .find_mut(|e| e.kind == halcyon_sim::Kind::Crystal && e.team != attacker_team)?;
        crystal.hp = 0.1;
        let wire_crystal = u32::from(wire_id(crystal.id));
        let pos = crystal.position;
        r.world.entities[attacker_seat].hp = r.world.entities[attacker_seat].max_hp;
        r.world.entities[attacker_seat].position = pos;
        Some(wire_crystal)
    }

    fn allocate(
        &self,
        req: Reservation,
        admitted: bool,
        federation: Option<FederationOrigin>,
    ) -> HostResult<HostedMatch> {
        if !halcyon_data::valid_talent_levels(&req.mode, req.players.len(), &req.talent_levels) {
            return Err(HostError::Invalid("invalid talent grants for roster"));
        }
        let talent_levels = if req.talent_levels.iter().all(|grants| grants.is_empty()) {
            Vec::new()
        } else {
            req.talent_levels
        };
        self.monitor_resources();
        let mut rooms = self.0.rooms.lock().unwrap();
        let now = Utc::now();
        rooms.retain(|room| {
            room.lock()
                .unwrap()
                .ended
                .is_none_or(|end| now - end < chrono::Duration::minutes(30))
        });
        if let Some(r) = rooms.get(&req.match_id) {
            let r = r.lock().unwrap();
            if r.federation != federation
                || r.requested_teams != req.teams
                || r.talent_levels != talent_levels
                || r.queue != req.mode
                || r.difficulty != req.difficulty
                || r.players != req.players
                || r.ended.is_some()
                || (admitted && !r.admitted)
            {
                return Err(HostError::Conflict("reservation conflict"));
            }
            return Ok(HostedMatch {
                address: r.address.clone(),
            });
        }
        if req.players.is_empty()
            || req.players.len() > 10
            || req.players.iter().collect::<HashSet<_>>().len() != req.players.len()
        {
            return Err(HostError::Invalid("invalid roster"));
        }
        let known_queue = KnownQueue::parse(&req.mode);
        let legacy_aral = known_queue == Some(KnownQueue::CasualAral);
        let six_humans = known_queue.is_some_and(KnownQueue::requires_six_humans);
        if six_humans && req.players.len() != halcyon_data::THREE_PER_SIDE_SEATS {
            return Err(HostError::Invalid(
                "three-per-side queue requires six human players",
            ));
        }
        let hero_seats = known_queue.map_or(10, KnownQueue::hero_seats);
        let occupied = rooms
            .values()
            .filter(|r| r.lock().unwrap().ended.is_none())
            .count();
        if self.free_with_count(occupied) == 0 {
            return Err(HostError::HostUnavailable);
        }
        let rt = tokio::runtime::Handle::try_current().map_err(|_| HostError::HostUnavailable)?;
        let listener = self
            .0
            .ports
            .bind(self.0.bind_ip)
            .map_err(|_| HostError::HostUnavailable)?;
        listener
            .set_nonblocking(true)
            .map_err(|_| HostError::HostUnavailable)?;
        let port = listener
            .local_addr()
            .map_err(|_| HostError::HostUnavailable)?
            .port();
        let address = self.advertise(port);
        let seed = req.match_id.as_u128() as u64;
        let n = req.players.len() as u8;
        let mut world = match &self.0.rules {
            Some(rules) => rules.world(seed, n),
            None => World::new(seed, n),
        };
        if let Some(settings) =
            known_queue.and_then(|q| halcyon_data::items::StoreSettings::for_mode(q.mode()))
        {
            world.use_catalog_store(settings);
            if let Some(entries) = self
                .0
                .data
                .as_ref()
                .and_then(|data| data.store_for_settings(settings.key()))
            {
                world.use_item_store(entries);
            }
        }
        if !req.teams.is_empty()
            && (req.teams.len() != req.players.len()
                || req.teams.iter().any(|t| *t > 1)
                || [0, 1]
                    .iter()
                    .any(|team| req.teams.iter().filter(|t| *t == team).count() > hero_seats / 2))
        {
            return Err(HostError::Invalid("invalid human teams"));
        }
        if six_humans {
            world
                .use_six_human_roster()
                .map_err(|_| HostError::Invalid("invalid six-human roster"))?;
        } else if hero_seats == halcyon_data::THREE_PER_SIDE_SEATS {
            fold_blitz::trim_to_three_per_side_roster(&mut world, hero_seats)
                .map_err(|_| HostError::Invalid("invalid three-per-side solo roster"))?;
        }
        world
            .assign_teams(&req.teams)
            .map_err(|_| HostError::Invalid("invalid human teams"))?;
        if legacy_aral || known_queue == Some(KnownQueue::BattleRoyaleBotsSolo) {
            world
                .use_aral_lane()
                .map_err(|_| HostError::Invalid("invalid ARAL layout"))?;
            if let Some(data) = &self.0.data {
                world.install_map_data(data).map_err(|error| {
                    tracing::warn!(%error, "invalid ARAL map assets");
                    HostError::HostUnavailable
                })?;
            }
        } else if req.players.len() == 10
            && !six_humans
            && !known_queue.is_some_and(KnownQueue::is_single_human)
        {
            world
                .use_sovereigns_rise_5v5(self.0.data.as_deref())
                .map_err(|_| HostError::HostUnavailable)?;
        }
        let draft_5v5 = draft_5v5::Draft5v5::for_room(known_queue, &world);
        world.set_bot_difficulty(req.difficulty);
        let mut picks = Vec::with_capacity(hero_seats);
        for i in 0..hero_seats {
            let bot = i >= usize::from(n);
            let mut name = "Ringo";
            if (bot || legacy_aral)
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
                preferences: Default::default(),
                name: name.into(),
                skin: self
                    .0
                    .data
                    .as_ref()
                    .and_then(|data| data.indexed_skin(name, None))
                    .unwrap_or_else(|| halcyon_data::skins::default_skin_hash(name)),
                selected: bot,
                locked: bot,
            });
        }
        let listener =
            tokio::net::TcpListener::from_std(listener).map_err(|_| HostError::HostUnavailable)?;
        let (stop, stopped) = watch::channel(false);
        let t = Utc::now();
        let mut room = Room {
            free_aral_reroll: self.free_aral_reroll(),
            unlock_all_talents: self.unlock_all_talents(),
            federation,
            draft_5v5,
            ringo_attacks: Default::default(),
            scene_views: (0..req.players.len()).map(|_| Default::default()).collect(),
            solo_slots: actor_slots::ActorSlots::from_roster(&world.entities),
            solo_structure_snapshot: Default::default(),
            aral_slots: Default::default(),
            dead_actors: Vec::new(),
            aral_announcements: Default::default(),
            surrender: Default::default(),
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
            known_queue,
            queue: req.mode,
            difficulty: req.difficulty,
            players: req.players,
            requested_teams: req.teams,
            talent_levels,
            combat: [Combat::default(); 10],
            catalog: self.0.catalog.clone(),
            seats: (0..n).map(|_| Seat::default()).collect(),
            world,
            phase: crate::RoomPhase::Allocated,
            created: t,
            since: t,
            ended: None,
            reason: None,
            termination: None,
            events: vec![],
            stop,
            settled: watch::channel(false).0,
            generation: 0,
            last_active: Instant::now(),
            phase_start: Instant::now(),
            last_roster: Instant::now(),
        };
        room.event(
            EventKind::PhaseChanged,
            crate::RoomPhase::Allocated.as_str(),
            EventDetail::Phase {},
        );
        let room = Arc::new(Mutex::new(room));
        rooms.insert(req.match_id, room.clone());
        let manager = self.clone();
        rt.spawn(async move {
            transport::serve(listener, room, stopped, &manager).await;
        });
        Ok(HostedMatch { address })
    }
}

impl MatchHost for Rooms {
    fn local_action(
        &self,
        room: Uuid,
        player: Uuid,
        call: halcyon_types::player_actions::ActionCall,
    ) -> HostResult<halcyon_types::player_actions::ActionReply> {
        Ok(self.room_action(room, player, None, call))
    }

    fn spectate(&self, match_id: Uuid, viewer: Uuid) -> HostResult<HostedSpectator> {
        let (address, spectator_id) = self.observe(match_id, viewer)?;
        Ok(HostedSpectator {
            address,
            spectator_id,
        })
    }

    fn spectate_with_exit(
        &self,
        room: Uuid,
        viewer: Uuid,
        exit: SpectatorExit,
    ) -> HostResult<HostedSpectator> {
        let (address, spectator_id) = self.observe_with_exit(room, viewer, Some(exit))?;
        Ok(HostedSpectator {
            address,
            spectator_id,
        })
    }

    fn stop_spectating(&self, match_id: Uuid, spectator_id: Uuid) -> HostResult<()> {
        self.revoke_observer(match_id, spectator_id)
    }

    fn reserve(&self, req: Reservation) -> HostResult<HostedMatch> {
        self.allocate(req, true, None)
    }
}
