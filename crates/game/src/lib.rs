//! In-process room hosting with CE control and game TCP connections.
mod actor_slots;
mod admission;
mod mobile_bulwark;
mod tower_aggro;
pub use admission::{AdmissionPolicy, PortRange, ResourceMetrics};
mod aral;
mod aral_death;
mod aral_guard;
mod aral_inventory;
mod aral_item_actions;
mod aral_spawn;
mod commands;
mod compact;
mod draft_5v5;
mod hero_progress;
mod messages;
mod modes_3v3_blitz;
mod native_abilities;
mod native_abilities_catalog;
mod native_cooldown;
mod npc_attack;
mod npc_projectile;
mod observer;
mod player_actions;
mod player_presence;
mod point_move;
mod reconnect;
mod recording;
mod registry;
mod result_frames;
mod ringo;
mod scene;
mod solo_bot_paths;
mod structure_effects;
mod surrender;
mod tick_latency;
mod transport;
use chrono::{DateTime, Utc};
use halcyon_data::GameData;
use halcyon_platform::{HostedMatch, MatchHost, PlatformError, Reservation};
use halcyon_protocol::{
    Fields, Message,
    ce::{KnownQueue, NativeMode},
};
use halcyon_sim::{Command, Event, Position, World};
use messages::{setup, snapshot, spawn, wire_id, world_header};
mod responses;
pub(crate) use halcyon_protocol::{Opcode, wire_opcode};
use responses::{Channel, EventDetail, EventKind};
pub use responses::{RoomEndReason, RoomParticipant, RoomPhase, RoomSummary, TickLatencyReport};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
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

const VISIBILITY_STATE_NORMAL: u8 = 1;
const VISIBILITY_MASK_HIDDEN: u8 = 0;
const VISIBILITY_MASK_VISIBLE: u8 = 15;

/// Returns true for non-own-team mobile and neutral actors whose client visibility
/// is toggled via `0x042B` (`Opcode::ModifyVisibility`). Own-team units and static
/// lane structures (`Tower`, `Crystal`, `Armory`) remain visible via their spawn masks.
pub(crate) fn uses_visibility_toggle(viewer_team: u8, entity: &halcyon_sim::Entity) -> bool {
    entity.team != viewer_team
        && matches!(
            entity.kind,
            halcyon_sim::Kind::Hero | halcyon_sim::Kind::Minion | halcyon_sim::Kind::Neutral
        )
}

pub(crate) fn actor_visibility_message(actor_id: u32, viewer_team: u8, visible: bool) -> Message {
    let mask = if visible {
        VISIBILITY_MASK_VISIBLE
    } else {
        VISIBILITY_MASK_HIDDEN
    };
    Message::actor_visibility(
        u32::from(wire_id(actor_id)),
        viewer_team + 1,
        VISIBILITY_STATE_NORMAL,
        [mask, 0],
    )
}
#[derive(Clone)]
pub struct Rooms(Arc<Manager>);
struct Manager {
    free_aral_reroll: AtomicBool,
    unlock_all_talents: AtomicBool,
    bind_ip: IpAddr,
    replay_directory: Mutex<Option<std::path::PathBuf>>,
    gate: Mutex<admission::Gate>,
    monitor_started: AtomicBool,
    ports: admission::Ports,
    host: String,
    capacity: usize,
    data: Option<Arc<GameData>>,
    catalog: Arc<[[u8; 32]]>,
    rules: Option<halcyon_sim::WorldRules>,
    rooms: Mutex<registry::RoomRegistry>,
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
    last_hud: Option<Instant>,
    used_aral_reroll: bool,
    presence: player_presence::Presence,
    action_receipts: player_actions::Receipts,
    locked: bool,
    draft_sync_sent: bool,
    recommended_path: Option<halcyon_protocol::RecommendedBuildPath>,
    equipped_talent: Option<halcyon_protocol::EquippedTalent>,
    aral_reorder_view: Option<aral_inventory::ProjectedSlots>,
    preamble: scene::Preamble,
    loading_roster_sent: bool,
    control_world: bool,
    scene_written: bool,
    world_actor_written: bool,
    native_abilities: native_abilities::NativeAbilities,
    peers: [Option<Peer>; 2],
    last: Option<DateTime<Utc>>,
    seen: [HashSet<u16>; 2],
}
impl Seat {
    fn gameplay_peer(&self) -> Option<&Peer> {
        self.peers[1]
            .as_ref()
            .or_else(|| self.peers[0].as_ref().filter(|_| self.preamble.confirmed()))
    }
}
mod draft_preferences;
mod native_hud;
pub(crate) fn authorize_talent_level(
    room: &Room,
    seat: usize,
    hero_type_id: u16,
    selection: halcyon_protocol::TalentSelection,
) -> Option<u8> {
    use halcyon_protocol::TalentSelection;
    if selection == TalentSelection::NoTalent {
        return Some(0);
    }
    if let Some(level) = room
        .talent_levels
        .get(seat)
        .and_then(|levels| levels.get(&hero_type_id))
    {
        return Some(level.get());
    }
    room.unlock_all_talents.then_some(match selection {
        TalentSelection::Slot0 | TalentSelection::Slot1 => 10,
        TalentSelection::Slot2 => 5,
        TalentSelection::NoTalent => 0,
    })
}

struct Pick {
    preferences: draft_preferences::Preferences,
    name: String,
    skin: u32,
    selected: bool,
    locked: bool,
}
#[derive(Clone, Copy, Default, serde::Serialize)]
struct Combat {
    kills: u32,
    deaths: u32,
    damage: f64,
    healing: f64,
}
#[derive(PartialEq, Eq)]
struct FederationOrigin {
    pool: Uuid,
    homes: Vec<Uuid>,
}

struct Room {
    free_aral_reroll: bool,
    unlock_all_talents: bool,
    federation: Option<FederationOrigin>,
    draft_5v5: Option<draft_5v5::Draft5v5>,
    scene_views: Vec<tower_aggro::View>,
    ringo_attacks: ringo::AttackHistory,
    solo_slots: actor_slots::ActorSlots,
    solo_structure_snapshot: structure_effects::SoloSnapshot,
    aral_slots: aral_spawn::WaveSlots,
    dead_actors: Vec<aral_death::Pending>,
    aral_announcements: aral::announcements::Announcements,
    surrender: [surrender::Ballot; 2],
    visible: [HashSet<u32>; 2],
    next_visible: [HashSet<u32>; 2],
    replay_bytes: Arc<AtomicU64>,
    observers: HashMap<Uuid, observer::Observer>,
    latency: tick_latency::TickLatency,
    admitted: bool,
    admission_ready: watch::Sender<bool>,
    data: Option<Arc<GameData>>,
    picks: Vec<Pick>,
    id: Uuid,
    address: String,
    queue: String,
    known_queue: Option<KnownQueue>,
    difficulty: Option<halcyon_data::BotDifficulty>,
    players: Vec<Uuid>,
    requested_teams: Vec<u8>,
    talent_levels: Vec<halcyon_data::TalentLevels>,
    combat: [Combat; 10],
    catalog: Arc<[[u8; 32]]>,
    seats: Vec<Seat>,
    world: World,
    phase: RoomPhase,
    created: DateTime<Utc>,
    since: DateTime<Utc>,
    ended: Option<DateTime<Utc>>,
    reason: Option<RoomEndReason>,
    termination: Option<responses::Termination>,
    events: Vec<responses::RoomEvent>,
    stop: watch::Sender<bool>,
    settled: watch::Sender<bool>,
    generation: u64,
    last_active: Instant,
    phase_start: Instant,
    last_roster: Instant,
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
                    PlatformError::Invalid("invalid simulation rules")
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
        self.list_typed().iter().map(responses::value).collect()
    }
    pub fn list_typed(&self) -> Vec<RoomSummary> {
        let rooms = self.0.rooms.lock().unwrap().handles();
        rooms
            .into_iter()
            .map(|r| r.lock().unwrap().view(false))
            .collect()
    }
    fn room(&self, id: Uuid) -> Option<Shared> {
        self.0.rooms.lock().unwrap().get(&id).cloned()
    }
    pub fn detail(&self, id: Uuid) -> Option<Value> {
        self.detail_typed(id).map(responses::value)
    }
    pub fn detail_typed(&self, id: Uuid) -> Option<RoomSummary> {
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
        Some(responses::value(responses::EventPage {
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
                Ok((r.ended.is_some(), responses::value(prior)))
            } else {
                Err(TerminateError::Conflict)
            };
        }
        if matches!(r.phase, crate::RoomPhase::Ending | crate::RoomPhase::Ended) {
            return Err(TerminateError::Conflict);
        }
        let termination = responses::Termination {
            match_id: id,
            requested_at: now(),
            requested_by: actor.to_owned(),
            reason: reason.to_owned(),
        };
        let response = responses::value(&termination);
        r.termination = Some(termination);
        r.reason = Some(RoomEndReason::AdminTerminated);
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
    async fn finished(&self, room: &Shared) {
        let (id, result) = {
            let mut r = room.lock().unwrap();

            let end = r.reason.unwrap_or(RoomEndReason::Error);
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
                        responses::PlayerResult {
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
                responses::value(responses::ResultReport {
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
}
fn now() -> String {
    Utc::now().to_rfc3339()
}
impl MatchHost for Rooms {
    fn local_action(
        &self,
        room: Uuid,
        player: Uuid,
        call: halcyon_protocol::player_actions::ActionCall,
    ) -> halcyon_platform::Result<halcyon_protocol::player_actions::ActionReply> {
        Ok(self.room_action(room, player, None, call))
    }
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
    fn spectate_with_exit(
        &self,
        room: Uuid,
        viewer: Uuid,
        exit: halcyon_platform::SpectatorExit,
    ) -> Result<halcyon_platform::HostedSpectator, PlatformError> {
        let (address, spectator_id) = self.observe_with_exit(room, viewer, Some(exit))?;
        Ok(halcyon_platform::HostedSpectator {
            address,
            spectator_id,
        })
    }
    fn stop_spectating(&self, match_id: Uuid, spectator_id: Uuid) -> Result<(), PlatformError> {
        self.revoke_observer(match_id, spectator_id)
    }

    fn reserve(&self, req: Reservation) -> Result<HostedMatch, PlatformError> {
        self.allocate(req, true, None)
    }
}
impl Rooms {
    pub fn prepare(&self, req: Reservation) -> Result<HostedMatch, PlatformError> {
        self.allocate(req, false, None)
    }
    /// Prepare a room from the committed allocation and its ordered ticket roster.
    pub fn prepare_federated(
        &self,
        req: Reservation,
        pool: Uuid,
        homes: Vec<Uuid>,
    ) -> Result<HostedMatch, PlatformError> {
        if homes.len() != req.players.len() {
            return Err(PlatformError::Invalid("invalid home roster"));
        }
        self.allocate(req, false, Some(FederationOrigin { pool, homes }))
    }

    pub fn activate(&self, id: Uuid) -> Result<(), PlatformError> {
        let rooms = self.0.rooms.lock().unwrap();
        let mut room = rooms
            .get(&id)
            .ok_or(PlatformError::HostUnavailable)?
            .lock()
            .unwrap();
        if matches!(
            room.phase,
            crate::RoomPhase::Ending | crate::RoomPhase::Ended
        ) {
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
    ) -> Result<HostedMatch, PlatformError> {
        if !halcyon_data::valid_talent_levels(&req.mode, req.players.len(), &req.talent_levels) {
            return Err(PlatformError::Invalid("invalid talent grants for roster"));
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
        let known_queue = KnownQueue::parse(&req.mode);
        let legacy_aral = known_queue == Some(KnownQueue::CasualAral);
        let six_humans = known_queue.is_some_and(KnownQueue::requires_six_humans);
        if six_humans && req.players.len() != halcyon_data::THREE_PER_SIDE_SEATS {
            return Err(PlatformError::Invalid(
                "three-per-side queue requires six human players",
            ));
        }
        let hero_seats = known_queue.map_or(10, KnownQueue::hero_seats);
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
            return Err(PlatformError::Invalid("invalid human teams"));
        }
        if six_humans {
            world
                .use_six_human_roster()
                .map_err(|_| PlatformError::Invalid("invalid six-human roster"))?;
        } else if hero_seats == halcyon_data::THREE_PER_SIDE_SEATS {
            modes_3v3_blitz::trim_to_three_per_side_roster(&mut world, hero_seats)
                .map_err(|_| PlatformError::Invalid("invalid three-per-side solo roster"))?;
        }
        world
            .assign_teams(&req.teams)
            .map_err(|_| PlatformError::Invalid("invalid human teams"))?;
        if legacy_aral || known_queue == Some(KnownQueue::BattleRoyaleBotsSolo) {
            world
                .use_aral_lane()
                .map_err(|_| PlatformError::Invalid("invalid ARAL layout"))?;
            if let Some(data) = &self.0.data {
                world.install_map_data(data).map_err(|error| {
                    tracing::warn!(%error, "invalid ARAL map assets");
                    PlatformError::HostUnavailable
                })?;
            }
        } else if req.players.len() == 10
            && !six_humans
            && !known_queue.is_some_and(KnownQueue::is_single_human)
        {
            world
                .use_sovereigns_rise_5v5(self.0.data.as_deref())
                .map_err(|_| PlatformError::HostUnavailable)?;
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
        let listener = tokio::net::TcpListener::from_std(listener)
            .map_err(|_| PlatformError::HostUnavailable)?;
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
            latency: tick_latency::TickLatency::default(),
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
            RoomPhase::Allocated.as_str(),
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
impl Room {
    fn event(&mut self, kind: EventKind, message: &str, detail: EventDetail) {
        match &detail {
            EventDetail::Connection {
                channel,
                generation,
                opcode,
                ..
            } => {
                tracing::debug!(room=%self.id,?kind,message,?channel,generation,?opcode,"room event")
            }
            EventDetail::Action { opcode, .. } => {
                tracing::debug!(room=%self.id,?kind,message,?opcode,"room event")
            }
            _ => tracing::debug!(room=%self.id,?kind,message,"room event"),
        }
        self.events.push(responses::RoomEvent {
            seq: self.events.len() + 1,
            kind,
            message: message.into(),
            at: now(),
            detail,
        });
    }
    fn phase(&mut self, phase: RoomPhase) {
        if self.phase != phase {
            self.phase = phase;
            self.phase_start = Instant::now();
            self.since = Utc::now();
            self.event(
                EventKind::PhaseChanged,
                phase.as_str(),
                EventDetail::Phase {},
            );
        }
    }
    fn view(&self, detail: bool) -> RoomSummary {
        let participants = detail.then(|| {
            (0..self.picks.len())
                .map(|slot| {
                    let s = self.seats.get(slot);
                    RoomParticipant {
                        slot,
                        team: self.world.entities[slot].team,
                        player_id: self.players.get(slot).copied(),
                        home_instance_id: self
                            .federation
                            .as_ref()
                            .and_then(|f| f.homes.get(slot))
                            .copied(),
                        bot_profile: s.is_none().then_some("lane-v1"),
                        display_name: if s.is_some() {
                            "Guest".into()
                        } else {
                            format!("{} Bot", self.picks[slot].name)
                        },
                        hero: self.picks[slot].name.clone(),
                        locked: s.is_none_or(|s| s.locked),
                        control_connected: s.is_some_and(|s| s.peers[0].is_some()),
                        game_connected: (!self.legacy_aral()
                            || self.phase == crate::RoomPhase::Running)
                            && s.is_some_and(|s| s.gameplay_peer().is_some()),
                        last_event_at: s.and_then(|s| s.last).map(|x| x.to_rfc3339()),
                    }
                })
                .collect()
        });
        RoomSummary {
            id: self.id,
            queue: self.queue.clone(),
            phase: self.phase,
            created_at: self.created.to_rfc3339(),
            phase_since: self.since.to_rfc3339(),
            ended_at: self.ended.map(|x| x.to_rfc3339()),
            end_reason: self.reason.map(RoomEndReason::as_str),
            human_players: self.players.len(),
            bot_players: self.picks.len() - self.players.len(),
            pool_id: self.federation.as_ref().map(|f| f.pool),
            tick_latency: self.latency.report(),
            difficulty: self.difficulty,
            participants,
        }
    }
    fn legacy_aral(&self) -> bool {
        self.known_queue == Some(KnownQueue::CasualAral)
    }

    fn solo_scene(&self) -> bool {
        self.players.len() == 1 && self.known_queue.is_some_and(KnownQueue::is_single_human)
    }

    pub(crate) fn hero_slot(&self, actor: &halcyon_sim::Entity) -> Option<u8> {
        if self.legacy_aral() || self.known_queue == Some(KnownQueue::BattleRoyaleBotsSolo) {
            point_move::aral_slot(actor)
        } else if modes_3v3_blitz::is_managed(self.known_queue) {
            modes_3v3_blitz::hero_slot(actor)
        } else {
            self.solo_slots.fixed(actor)
        }
    }

    fn solo_loading(&mut self) {
        if self.phase != crate::RoomPhase::Loading
            || self.phase_start.elapsed() < std::time::Duration::from_secs(7)
            || self.seats[0].loading_roster_sent
            || self.seats[0].peers[1].is_some()
        {
            return;
        }
        if let Some(peer) = &self.seats[0].peers[0] {
            peer.send(transport::Delivery::Private(scene::begin_loading(self)));
            self.seats[0].loading_roster_sent = true;
        }
    }
    fn aral_remaining(&self) -> f32 {
        (aral::DRAFT_SECONDS - self.phase_start.elapsed().as_secs_f32()).max(0.)
    }

    fn start_control_world(&mut self) {
        if self.phase != crate::RoomPhase::Loading
            || !self
                .seats
                .iter()
                .all(|seat| seat.preamble.confirmed() && seat.peers[0].is_some())
        {
            return;
        }
        // Every control stream has completed the same pristine ARAL opening.
        // Mark only currently visible actors to retain normal team visibility.
        for (team, visible) in self.visible.iter_mut().enumerate() {
            visible.extend(self.world.entities.iter().filter_map(|actor| {
                (actor.hp > 0.0 && self.world.visible(team as u8, actor)).then_some(actor.id)
            }));
        }
        let mut records = self.recommended_paths(None);
        records.push(Message::known(crate::Opcode::StartMatch, []));
        let marker = transport::TeamBatch::new(records);
        for peer in self.seats.iter().filter_map(Seat::gameplay_peer) {
            peer.send(transport::Delivery::Team(marker.clone()));
        }
        self.phase(crate::RoomPhase::Running);
    }

    fn aral_progress(&mut self) {
        if !self.seats.iter().all(|seat| seat.peers[0].is_some()) {
            return;
        }
        match self.phase {
            crate::RoomPhase::HeroSelect if self.aral_remaining() <= 0. => {
                for (seat, pick) in self.seats.iter_mut().zip(&mut self.picks) {
                    seat.locked = true;
                    pick.selected = true;
                    pick.locked = true;
                }
                let mut opening = vec![aral::roster(self, 0., false)];
                opening.extend(scene::begin_loading(self));
                let opening = transport::TeamBatch::new(opening);
                for peer in self.seats.iter().filter_map(|seat| seat.peers[0].as_ref()) {
                    peer.send(transport::Delivery::Team(opening.clone()));
                }
                self.phase(crate::RoomPhase::Loading);
            }
            crate::RoomPhase::Loading => self.start_control_world(),
            _ => {}
        }
    }

    fn attach(
        &mut self,
        uuid: Uuid,
        tx: Sink,
        stalled: watch::Sender<bool>,
    ) -> Option<(usize, usize, u64, Batch)> {
        let idx = self.players.iter().position(|p| *p == uuid)?;
        if matches!(
            self.phase,
            crate::RoomPhase::Ending | crate::RoomPhase::Ended
        ) || self.events.len() >= 4096
        {
            return None;
        }
        // A locked seat can open its data connection while other humans are still selecting.
        let legacy_aral = self.legacy_aral();
        let game = !legacy_aral
            && !self.seats[idx].control_world
            && self.seats[idx].locked
            && self.seats[idx].peers[1].is_none()
            && (self.seats[idx].peers[0].is_some() || self.phase == crate::RoomPhase::Running);
        let ch = usize::from(game);
        if let Some(existing) = &self.seats[idx].peers[ch] {
            if ch == 0 && (*existing.stalled.borrow() || existing.tx.is_closed()) {
                let generation = existing.generation;
                self.detach(idx, ch, generation);
            } else {
                return None;
            }
        }
        self.last_active = Instant::now();
        self.generation += 1;
        if game {
            let _ = modes_3v3_blitz::prepare_world(self);
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
        } else {
            self.seats[idx].loading_roster_sent = false;
            self.seats[idx].scene_written = false;
            self.seats[idx].world_actor_written = false;
            self.seats[idx].aral_reorder_view = None;
            self.scene_views[idx] = Default::default();
            self.seats[idx].draft_sync_sent = false;
            self.seats[idx].preamble = Default::default();
        }
        self.seats[idx].seen[ch].clear();
        self.seats[idx].peers[ch] = Some(Peer {
            generation: self.generation,
            tx,
            stalled,
        });
        let first_registration = self.seats[idx].last.is_none();
        self.seats[idx].last = Some(Utc::now());
        if self.phase == crate::RoomPhase::Allocated {
            self.phase(crate::RoomPhase::HeroSelect);
        }
        self.event(
            EventKind::Connection,
            "Authenticated",
            EventDetail::Connection {
                player_id: uuid,
                channel: Channel::for_game(game),
                generation: Some(self.generation),
                opcode: None,
            },
        );
        let solo_rejoin = reconnect::is_solo_rejoin(self, idx, game);
        let mut batch = if solo_rejoin {
            // A fresh control stream has never received the seat and world mode.
            self.seats[idx].loading_roster_sent = true;
            reconnect::solo_rejoin_batch(self, idx)
        } else if legacy_aral {
            aral::setup(
                self,
                idx,
                if self.phase == crate::RoomPhase::Running {
                    0.
                } else {
                    self.aral_remaining()
                },
            )
        } else if game {
            let mut header =
                world_header(&self.world, &self.players, &self.picks, &self.queue, idx);
            reconnect::populate_world_header(
                &mut header,
                &self.world.entities[idx],
                &self.picks[idx],
            );
            header
        } else {
            let mut out = setup(
                &self.world,
                &self.players,
                &self.catalog,
                idx,
                &self.queue,
                &self.picks,
            );
            if let Some(d) = self.draft_5v5.as_ref().filter(|d| d.is_ranked()) {
                out.push(d.ranked_packet(&self.world, &self.picks).encode());
            }
            out
        };
        if !game
            && !self.solo_scene()
            && matches!(
                self.phase,
                crate::RoomPhase::Loading | crate::RoomPhase::Running
            )
            && (legacy_aral || self.draft_5v5.is_some())
        {
            self.seats[idx].loading_roster_sent = true;
            batch.extend(scene::begin_loading(self));
        }
        if game {
            let team = self.world.entities[idx].team;
            if modes_3v3_blitz::is_managed(self.known_queue) {
                modes_3v3_blitz::append_opening(self, Some(team), false, &mut batch);
                for e in &self.world.entities {
                    if e.hp > 0.0 && self.world.visible(team, e) {
                        self.visible[usize::from(team)].insert(e.id);
                        if uses_visibility_toggle(team, e) {
                            batch.push(actor_visibility_message(e.id, team, true));
                        }
                    }
                }
            } else {
                batch.extend(self.spawn_view(Some(team)));
            }
            let ready_for_5v5 =
                self.world.is_5v5_enabled()
                    && self.phase == crate::RoomPhase::Loading
                    && self.seats.iter().enumerate().all(|(seat_idx, s)| {
                        self.world.entities[seat_idx].bot || s.peers[1].is_some()
                    });
            if self.seats.iter().all(|s| s.peers[1].is_some()) || ready_for_5v5 {
                self.phase(crate::RoomPhase::Running);
            }
        }
        if legacy_aral && first_registration && self.phase == crate::RoomPhase::HeroSelect {
            scene::announce_aral_registration(self, idx);
        }
        Some((idx, ch, self.generation, batch))
    }
    fn spawn_view(&self, team: Option<u8>) -> Batch {
        if modes_3v3_blitz::is_managed(self.known_queue) {
            let mut out = Vec::new();
            modes_3v3_blitz::append_opening_snapshot(self, team, false, &mut out);
            return out;
        }
        let mut out = vec![];
        for e in &self.world.entities {
            if e.hp <= 0. || team.is_some_and(|t| !self.world.visible(t, e)) {
                continue;
            }
            out.push(point_move::full_spawn(
                &self.world,
                e,
                &self.picks,
                self.data.as_deref(),
                self.legacy_aral(),
            ));
            messages::append_inventory(&mut out, &self.world, e);
            if let Some(viewer_team) = team
                && uses_visibility_toggle(viewer_team, e)
            {
                out.push(actor_visibility_message(e.id, viewer_team, true));
            }
        }
        out.extend(self.recommended_paths(team));
        out.extend(aral::talent_snapshot(self));
        messages::append_5v5_camp_markers(&mut out, &self.world, team.map_or(0, |t| t + 1));
        out
    }
    fn detach(&mut self, i: usize, ch: usize, generation: u64) {
        if self.seats[i].peers[ch]
            .as_ref()
            .is_some_and(|p| p.generation == generation)
        {
            self.seats[i].peers[ch] = None;
            self.world_disconnected(i, ch);
            // The empty-room grace starts when the last current connection leaves.
            // A connected player may have received updates without sending input.
            self.last_active = Instant::now();
            if ch == 0 {
                self.seats[i].aral_reorder_view = None;
            }
            self.seats[i].last = Some(Utc::now());
            self.event(
                EventKind::Connection,
                "Disconnected",
                EventDetail::Connection {
                    player_id: self.players[i],
                    channel: Channel::for_game(ch != 0),
                    generation: Some(generation),
                    opcode: None,
                },
            );
        }
    }
    fn roster_update(&self, except: Option<usize>) -> Message {
        self.roster_with_echo(except, None)
    }
    fn roster_with_echo(&self, except: Option<usize>, echo: Option<&Message>) -> Message {
        let legacy = self.legacy_aral();
        let mut roster = if legacy {
            aral::roster(self, self.aral_remaining(), false)
        } else {
            snapshot(&self.world, &self.players, &self.picks)
        };
        if !legacy && self.phase == crate::RoomPhase::HeroSelect {
            roster.body[..4].copy_from_slice(
                &(300. - self.phase_start.elapsed().as_secs_f32())
                    .max(0.)
                    .to_be_bytes(),
            );
        }
        if !legacy && self.phase == crate::RoomPhase::Loading {
            roster.body[..4].copy_from_slice(
                &(7. - self.phase_start.elapsed().as_secs_f32())
                    .max(0.)
                    .to_be_bytes(),
            );
            roster.body[4..8].copy_from_slice(&7f32.to_be_bytes());
        }
        let ranked_045b = (!legacy)
            .then(|| {
                self.draft_5v5
                    .as_ref()
                    .filter(|d| d.is_ranked())
                    .map(|d| d.ranked_packet(&self.world, &self.picks).encode())
            })
            .flatten();
        for (i, s) in self.seats.iter().enumerate() {
            if except != Some(i)
                && let Some(p) = &s.peers[0]
            {
                let mut batch = vec![roster.clone()];
                if let Some(pkt) = &ranked_045b {
                    batch.push(pkt.clone());
                }
                batch.extend(echo.cloned());
                p.send(transport::Delivery::Private(batch));
            }
        }
        roster
    }
    fn enter_5v5_loading(&mut self, actor_seat: usize, out: &mut Vec<Message>) {
        if !self.world.is_5v5_enabled()
            && !self.solo_scene()
            && !self.legacy_aral()
            && !halcyon_data::three_per_side(&self.queue)
            && self.world.tick == 0
        {
            if let Err(err) = self.world.use_sovereigns_rise_5v5(self.data.as_deref()) {
                tracing::warn!(%err, "failed to install 5v5 Sovereign's Rise state");
            } else {
                self.solo_slots = actor_slots::ActorSlots::from_roster(&self.world.entities);
            }
        }
        self.phase(crate::RoomPhase::Loading);
        let loading_batch = scene::begin_loading(self);
        for (idx, seat) in self.seats.iter_mut().enumerate() {
            if seat.peers[0].is_some() {
                seat.loading_roster_sent = true;
            }
            if idx != actor_seat
                && let Some(peer) = &seat.peers[0]
            {
                peer.send(transport::Delivery::Private(loading_batch.clone()));
            }
        }
        out.extend(loading_batch);
    }
    fn validated_skin(&self, hero: &str, requested: u32) -> u32 {
        if let Some(skin) = self
            .data
            .as_ref()
            .and_then(|data| data.indexed_skin(hero, Some(requested)))
        {
            return skin;
        }
        let default = halcyon_data::skins::default_skin_hash(hero);
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
        let actor_id = self.world.entities[i].id;
        let Ok(receipt) = self.world.try_purchase(actor_id, offer) else {
            return vec![];
        };
        let mut ack = vec![Message::known(
            crate::Opcode::BuyItem,
            [actor.to_be_bytes(), item.to_be_bytes()].concat(),
        )];
        if let Some(receipt) = receipt {
            messages::append_purchase(&mut ack, &self.world, &receipt, true);
        } else if self.legacy_aral() && self.seats[i].world_actor_written {
            ack.push(Message::known(crate::Opcode::RejectBuyItem, []));
        } else {
            return vec![];
        }
        let mut shared = None;
        for (j, seat) in self.seats.iter().enumerate() {
            if j != i
                && self.visible[self.world.entities[j].team as usize].contains(&actor_id)
                && self
                    .world
                    .visible(self.world.entities[j].team, &self.world.entities[i])
                && let Some(peer) = seat.gameplay_peer()
            {
                let batch = shared.get_or_insert_with(|| transport::TeamBatch::new(ack.clone()));
                peer.send(transport::Delivery::Team(batch.clone()));
            }
        }
        self.publish_observers(&ack);
        ack
    }
    fn receive(&mut self, i: usize, ch: usize, m: Message) -> std::io::Result<Batch> {
        self.last_active = Instant::now();
        let legacy = self.legacy_aral();
        let solo_scene = self.solo_scene()
            && self.seats[i].loading_roster_sent
            && self.seats[i].peers[1].is_none();
        let mut scene_requested = false;
        let mut scene_confirmed = false;
        if (legacy || solo_scene)
            && ch == 0
            && matches!(
                self.phase,
                crate::RoomPhase::Loading | crate::RoomPhase::Running
            )
            && !self.seats[i].preamble.confirmed()
        {
            let preamble = &mut self.seats[i].preamble;
            let requested = preamble.scene_requested();
            preamble.observe(&m, u32::from(wire_id(self.world.entities[i].id)));
            scene_requested = preamble.scene_requested() && !requested;
            scene_confirmed = preamble.confirmed();
            if solo_scene && scene_requested {
                self.seats[i].control_world = true;
            }
        }
        let game = self.phase == crate::RoomPhase::Running
            && (ch == 1
                || ((legacy || solo_scene) && ch == 0 && self.seats[i].preamble.confirmed()));
        let pending_world_input = solo_scene
            && ch == 0
            && self.seats[i].scene_written
            && self.seats[i].preamble.scene_requested()
            && !self.seats[i].preamble.confirmed();
        let mut accepted_world_input = false;
        let mut first_point_ack = None;
        let sync_after_pick = !legacy
            && ch == 0
            && m.is(crate::Opcode::DraftSelectHero)
            && !self.seats[i].draft_sync_sent;
        let seen = &mut self.seats[i].seen[ch];
        if !m.is(crate::Opcode::DraftReadyAccept) && seen.len() < 32 && seen.insert(m.opcode) {
            let generation = self.seats[i].peers[ch].as_ref().map(|peer| peer.generation);
            self.event(
                EventKind::Protocol,
                "First client message of this type",
                EventDetail::Connection {
                    player_id: self.players[i],
                    channel: Channel::for_game(ch != 0),
                    generation,
                    opcode: Some(m.opcode.into()),
                },
            );
        }
        let mut out = vec![];
        if ch == 0
            && self.phase == crate::RoomPhase::HeroSelect
            && !legacy
            && !solo_scene
            && self.known_queue != Some(KnownQueue::Ranked)
            && let Some(action) = halcyon_protocol::draft::DraftAction::decode(&m)
            && let Some(mut draft) = self.draft_5v5.take()
        {
            if let halcyon_protocol::draft::DraftAction::SelectSkin { skin_id, .. } = action
                && !self.seats[i].loading_roster_sent
            {
                self.picks[i].skin = self.validated_skin(&self.picks[i].name, skin_id);
            }
            let outcome = draft.handle_action(
                i,
                action,
                &mut self.world,
                &mut self.seats,
                &mut self.picks,
                self.data.as_deref(),
            );
            let ranked_msg = draft
                .is_ranked()
                .then(|| draft.ranked_packet(&self.world, &self.picks).encode());
            self.draft_5v5 = Some(draft);
            if let Ok(outcome) = outcome {
                if outcome.state_changed {
                    let roster = self.roster_with_echo(Some(i), outcome.echo.then_some(&m));
                    out.push(roster);
                    if let Some(pkt) = ranked_msg {
                        out.push(pkt);
                    }
                }
                if outcome.echo {
                    out.push(m);
                }
                if outcome.start_loading {
                    self.enter_5v5_loading(i, &mut out);
                }
                if sync_after_pick {
                    out.push(Message::known(crate::Opcode::EndMatch, [0, 0, 0, 0, 8, 0]));
                    self.seats[i].draft_sync_sent = true;
                }
            }
            return Ok(out);
        }
        match m.fields()? {
            _ if game && m.is(crate::Opcode::RequestCancelAbility) => {
                out = native_abilities::receive(self, i, ch, m);
            }
            Fields::Live(halcyon_protocol::LiveFields::RecommendedBuildPath(path))
                if game
                    || pending_world_input
                    || (legacy
                        && ch == 0
                        && self.seats[i].preamble.scene_requested()
                        && matches!(
                            self.phase,
                            crate::RoomPhase::Loading | crate::RoomPhase::Running
                        )) =>
            {
                out = self.set_recommended_path(i, path);
                accepted_world_input = pending_world_input;
            }
            Fields::Live(halcyon_protocol::LiveFields::MovePoint { x, y })
            | Fields::Movement { x, y }
                if ch == 0
                    && self.seats[i].world_actor_written
                    && (legacy
                        || m.is(crate::Opcode::RequestMovePoint)
                            && solo_scene
                            && self.seats[i].scene_written)
                    && (game || pending_world_input) =>
            {
                let actor = &self.world.entities[i];
                let slot = self.hero_slot(actor);
                if let Some(slot) = slot
                    && let Some(ack) = point_move::submit(
                        &mut self.world,
                        i as u32 + 1,
                        slot,
                        Position { x, y },
                        &m.body,
                    )
                {
                    if pending_world_input {
                        accepted_world_input = true;
                        first_point_ack = Some(ack);
                    } else {
                        if legacy {
                            point_move::publish_aral(self, i, &ack);
                        }
                        out.push(ack);
                    }
                }
            }
            Fields::Movement { x, y } if pending_world_input => {
                accepted_world_input = m.body.len() == 14
                    && x.is_finite()
                    && y.is_finite()
                    && self
                        .world
                        .command(i as u32 + 1, Command::MoveTo(Position { x, y }))
                        .is_ok();
            }
            Fields::Clock(seconds) if pending_world_input => {
                accepted_world_input = m.body.len() == 6 && seconds.is_finite() && seconds >= 0.;
                out.push(m);
            }
            Fields::Movement { x, y } if game && !(legacy && ch == 0) => {
                let _ = self
                    .world
                    .command(i as u32 + 1, Command::MoveTo(Position { x, y }));
            }
            Fields::Live(halcyon_protocol::LiveFields::Item(
                halcyon_protocol::ItemAction::Reorder { actor, slots },
            )) => aral_inventory::reorder(self, i, ch, actor, slots),
            Fields::Live(halcyon_protocol::LiveFields::Item(
                halcyon_protocol::ItemAction::Sell {
                    actor,
                    instance,
                    suffix,
                },
            )) if game && suffix.iter().all(|&byte| byte == 0) => {
                out = aral_item_actions::sell(self, i, actor, instance);
            }
            Fields::Live(halcyon_protocol::LiveFields::Item(
                halcyon_protocol::ItemAction::Activate { instance, suffix },
            )) if game && suffix.iter().all(|&byte| byte == 0) => {
                use halcyon_sim::{AralStarter, BasicItem, ItemTarget};
                let hero = &self.world.entities[i];
                let command = if instance == AralStarter::HealingFlask.grant().instance {
                    Command::ActivateHealingFlask { instance }
                } else {
                    match hero
                        .inventory
                        .iter()
                        .find(|h| h.instance == instance)
                        .map(|h| h.kind)
                    {
                        Some(BasicItem::WeaponInfusion | BasicItem::CrystalInfusion) => {
                            Command::ConsumeInfusion { instance }
                        }
                        Some(BasicItem::ReflexBlock) => Command::ActivateReflexBlock { instance },
                        Some(BasicItem::SprintBoots) => Command::ActivateSprintBoots { instance },
                        Some(BasicItem::ScoutTrap) => Command::DeployScoutTrap { instance },
                        Some(_) => Command::ActivateItem {
                            instance,
                            target: ItemTarget::SelfCast,
                        },
                        None => return Ok(out),
                    }
                };
                let _ = self.world.command(hero.id, command);
            }
            Fields::Live(halcyon_protocol::LiveFields::Item(
                halcyon_protocol::ItemAction::TargetRequest {
                    arguments: [target_wire, instance],
                    suffix,
                },
            )) if game && suffix.iter().all(|&byte| byte == 0) => {
                let hero_id = self.world.entities[i].id;
                let target = if target_wire == u32::MAX {
                    halcyon_sim::ItemTarget::SelfCast
                } else if let Some(actor_id) = target_wire
                    .checked_sub(u32::from(wire_id(0)))
                    .filter(|&id| id != 0)
                {
                    halcyon_sim::ItemTarget::Entity(actor_id)
                } else {
                    return Ok(out);
                };
                let _ = self
                    .world
                    .command(hero_id, Command::ActivateItem { instance, target });
            }
            Fields::Live(halcyon_protocol::LiveFields::Item(
                halcyon_protocol::ItemAction::VectorRequest {
                    vector_bits: [x_bits, _, z_bits],
                    argument: instance,
                    suffix,
                },
            )) if game && suffix.iter().all(|&byte| byte == 0) => {
                let (x, y) = (f32::from_bits(x_bits), f32::from_bits(z_bits));
                if x.is_finite() && y.is_finite() {
                    let hero_id = self.world.entities[i].id;
                    let point = Position { x, y };
                    let cmd = if instance == halcyon_sim::AralStarter::VisionTotem.grant().instance
                    {
                        Command::DeployVisionTotem { instance, point }
                    } else {
                        Command::ActivateItem {
                            instance,
                            target: halcyon_sim::ItemTarget::Point(point),
                        }
                    };
                    let _ = self.world.command(hero_id, cmd);
                }
            }
            Fields::Live(halcyon_protocol::LiveFields::TalentSelection(selection)) => {
                out = if modes_3v3_blitz::is_blitz(self.known_queue) {
                    modes_3v3_blitz::talent_response(self, i, selection)
                } else {
                    aral::talent_response(self, i, ch, selection)
                };
            }
            Fields::Live(halcyon_protocol::LiveFields::MovePoint { .. })
                if ch == 0 && (legacy || solo_scene) => {}
            Fields::Live(
                halcyon_protocol::LiveFields::LearnSelection(_)
                | halcyon_protocol::LiveFields::AbilityPoint { .. }
                | halcyon_protocol::LiveFields::AbilityTarget { .. },
            )
            | Fields::Outbound(halcyon_protocol::OutboundAction {
                operands: halcyon_protocol::OutboundOperands::AbilityAlternate { .. },
                ..
            }) if game => {
                out = native_abilities::receive(self, i, ch, m);
            }
            Fields::Live(halcyon_protocol::LiveFields::SurrenderRequest(vote))
                if game
                    && (ch == 1 || self.seats[i].world_actor_written)
                    && (m.body.len() == 1 || (m.body.len() == 6 && m.body[1..] == [0; 5])) =>
            {
                self.vote_surrender(i, vote);
            }
            Fields::Live(
                request @ (halcyon_protocol::LiveFields::MapPing(_)
                | halcyon_protocol::LiveFields::HudQuickMessage(_)),
            ) if game && self.room_hud_connection(i, ch) => {
                out = native_hud::submit(self, i, request);
            }
            Fields::Live(request) if game => {
                if commands::clears_attack(&request) {
                    if self.world.entities[i].attack_target().is_some()
                        && self.world.command(i as u32 + 1, Command::Stop).is_ok()
                        && !legacy
                    {
                        out.push(native_abilities::modify_basic_attack_target_message(None));
                    }
                } else if let Some(command) = commands::from_live(request) {
                    let aral_target = legacy && ch == 0 && matches!(command, Command::Attack(_));
                    let accepted = self.world.command(i as u32 + 1, command).is_ok();
                    if accepted {
                        if aral_target {
                            out.push(m);
                        } else if !legacy && let Command::Attack(target) = command {
                            out.push(native_abilities::modify_basic_attack_target_message(Some(
                                target,
                            )));
                        }
                    }
                }
            }
            Fields::ShopRequest { actor, item }
                if game && actor == u32::from(wire_id(i as u32 + 1)) =>
            {
                out = self.buy(i, actor, item);
            }
            Fields::ControlByte {
                value: 0,
                remaining,
            } if m.is(crate::Opcode::DraftRerollHero)
                && ch == 0
                && remaining.iter().all(|&byte| byte == 0) =>
            {
                out = aral::reroll(self, i);
            }
            Fields::ControlByte { value, remaining }
                if ch == 0
                    && self.phase == crate::RoomPhase::HeroSelect
                    && self.seats[i].peers[0].is_some()
                    && remaining.iter().all(|b| *b == 0)
                    && let Some(choice) =
                        draft_preferences::Choice::parse(m.known_opcode(), value) =>
            {
                choice.apply(&mut self.picks[i].preferences);
                out.push(self.roster_with_echo(Some(i), Some(&m)));
                out.push(m);
            }
            Fields::ControlByte { value: 0, .. }
                if m.is(crate::Opcode::DraftRefreshRequest)
                    && (legacy
                        || self.solo_scene()
                        || self.known_queue == Some(KnownQueue::Ranked)
                        || modes_3v3_blitz::is_managed(self.known_queue))
                    && ch == 0
                    && self.phase == crate::RoomPhase::HeroSelect
                    && self.seats[i].peers[0].is_some()
                    && m.body == [0; 6] =>
            {
                out = if legacy {
                    aral::refresh(self, i)
                } else {
                    messages::draft_catalog(
                        &self.world,
                        &self.players,
                        &self.catalog,
                        &self.queue,
                        &self.picks,
                    )
                };
                out.push(m);
            }
            Fields::HeroLock if ch == 0 && !legacy => {
                let op = m.opcode;
                out.push(m);
                if !self.seats[i].locked {
                    self.seats[i].locked = true;
                    self.picks[i].selected = true;
                    self.picks[i].locked = true;
                    self.event(
                        EventKind::Protocol,
                        "Hero locked",
                        EventDetail::Action {
                            player_id: self.players[i],
                            opcode: op.into(),
                        },
                    );
                    let roster = self.roster_update(Some(i));
                    if op == crate::wire_opcode::DraftLockHero {
                        out.insert(0, roster);
                    } else {
                        out.push(roster);
                    }
                    if self.seats.iter().all(|s| s.locked) {
                        let _ = modes_3v3_blitz::prepare_world(self);
                        self.phase(crate::RoomPhase::Loading);
                    }
                }
            }
            Fields::HeroSelection { template, choice }
                if ch == 0 && !legacy && !self.seats[i].locked =>
            {
                let name = draft_5v5::hero_name_for_template(self.data.as_deref(), template);
                if let Some(name) = name {
                    let valid = if let Some(d) = &self.data {
                        self.world.select_hero(i, &name, d).is_ok()
                    } else {
                        name == "Ringo"
                    };
                    if valid {
                        let requested = match choice {
                            halcyon_protocol::DraftHeroChoice::Skin(skin) => skin,
                            halcyon_protocol::DraftHeroChoice::Flags(_)
                                if self.picks[i].name == name =>
                            {
                                self.picks[i].skin
                            }
                            halcyon_protocol::DraftHeroChoice::Flags(_) => 0,
                        };
                        let skin = self.validated_skin(&name, requested);
                        self.picks[i] = Pick {
                            preferences: self.picks[i].preferences,
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
            Fields::SkinSelection(skin)
                if ch == 0
                    && (!legacy || self.phase == crate::RoomPhase::HeroSelect)
                    && !self.seats[i].loading_roster_sent =>
            {
                self.picks[i].skin = self.validated_skin(&self.picks[i].name, skin);
                out.push(self.roster_update(Some(i)));
                out.push(m);
            }
            Fields::Clock(_) | Fields::Heartbeat => out.push(m),
            _ if (legacy || solo_scene)
                && matches!(
                    m.opcode,
                    crate::wire_opcode::RequestSceneSnapshot
                        | crate::wire_opcode::ConfirmSceneOwner
                        | crate::wire_opcode::SceneReadyConfirm
                ) =>
            {
                let late = solo_scene && self.seats[i].preamble.take_late_confirmation(&m);
                if scene_confirmed || late {
                    if solo_scene {
                        out.push(Message::known(crate::Opcode::SceneLoadedDismiss, [0]));
                    }
                    out.push(m);
                    if solo_scene {
                        out.push(Message::known(crate::Opcode::SceneLoadedDismiss, [0]));
                    }
                    if solo_scene && scene_confirmed {
                        self.phase(crate::RoomPhase::Running);
                        out.push(Message::known(crate::Opcode::StartMatch, []));
                    } else if !solo_scene && self.phase == crate::RoomPhase::Running {
                        out.push(Message::known(crate::Opcode::StartMatch, []));
                    }
                } else if scene_requested {
                    let team = (self.phase == crate::RoomPhase::Running)
                        .then_some(self.world.entities[i].team);
                    out = if solo_scene {
                        scene::solo_opening(self, i)?
                    } else {
                        scene::announce_aral_entry(self, i, &m);
                        aral::opening(self, team)
                    };
                    out.push(Message::known(crate::Opcode::RequestSceneSnapshot, [0; 6]));
                    out.push(m);
                    if solo_scene {
                        out.push(scene::solo_table(self));
                    }
                }
            }
            _ if [
                crate::wire_opcode::DraftRefreshRequest,
                crate::wire_opcode::DraftSelectHero,
                crate::wire_opcode::DraftReadyControl,
                crate::wire_opcode::DraftSelectSkin,
                crate::wire_opcode::RequestSceneSnapshot,
                crate::wire_opcode::ConfirmSceneOwner,
                crate::wire_opcode::SceneReadyConfirm,
            ]
            .contains(&m.opcode) =>
            {
                out.push(m)
            }
            _ => {}
        }
        if accepted_world_input {
            self.seats[i].preamble.confirm_world_input();
            self.phase(crate::RoomPhase::Running);
            out.push(Message::known(crate::Opcode::StartMatch, []));
            out.extend(first_point_ack);
        }
        if sync_after_pick {
            // Android's first 0x045E response precedes this one-time draft message.
            out.push(Message::known(crate::Opcode::EndMatch, [0, 0, 0, 0, 8, 0]));
            self.seats[i].draft_sync_sent = true;
        }
        if ch == 0 && scene_requested {
            self.scene_views[i].declarations(&out);
            if self.legacy_aral() {
                self.scene_views[i].accept_guard_links(&out);
            }
        }
        Ok(out)
    }
    fn tick(&mut self) {
        if !self.admitted {
            return;
        }
        if self.phase == crate::RoomPhase::Ending && self.reason == Some(RoomEndReason::Completed) {
            if !self.dead_actors.is_empty() {
                let elapsed = self.phase_start.elapsed().as_millis()
                    * u128::from(halcyon_sim::TICK_HZ)
                    / 1000;
                aral_death::flush(self, self.world.tick.saturating_add(elapsed as u64));
            }
            let delay = if self.surrender.iter().any(surrender::Ballot::passed) {
                15
            } else {
                6
            };
            if self.phase_start.elapsed() >= std::time::Duration::from_secs(delay) {
                self.finish_world();
            }
            return;
        }
        if self.last_active.elapsed().as_secs() > 300
            && self
                .seats
                .iter()
                .all(|s| s.peers.iter().all(Option::is_none))
        {
            self.reason = Some(RoomEndReason::Error);
            self.phase(crate::RoomPhase::Ending);
            self.stop.send_replace(true);
            return;
        }
        let legacy = self.legacy_aral();
        let roster_interval = if legacy {
            std::time::Duration::from_millis(100)
        } else {
            std::time::Duration::from_secs(1)
        };
        if self.solo_scene() && self.phase == crate::RoomPhase::Loading {
            self.solo_loading();
        }
        if (self.phase == crate::RoomPhase::HeroSelect
            || !legacy
                && self.phase == crate::RoomPhase::Loading
                && !self.seats[0].loading_roster_sent)
            && self.last_roster.elapsed() >= roster_interval
        {
            self.last_roster = Instant::now();
            self.roster_update(None);
        }

        if legacy
            && matches!(
                self.phase,
                crate::RoomPhase::HeroSelect | crate::RoomPhase::Loading
            )
        {
            self.aral_progress();
            return;
        }

        // The world advances once the selected transport has confirmed admission.
        if self.phase != crate::RoomPhase::Running {
            return;
        }
        if self.world.tick == 0 && self.solo_scene() && self.seats[0].control_world {
            self.solo_structure_snapshot.start();
            scene::initial_structures(self);
        }
        let events = self.world.step();
        modes_3v3_blitz::publish_tick(self, &events);
        let (observer_camps, team_camps) = self.drain_5v5_camp_messages();
        self.solo_structure_snapshot.observe(&events);
        if legacy && self.world.winner.is_none() {
            self.advance_absences();
        }
        aral_death::flush(self, self.world.tick);
        aral_death::schedule(self, &events);
        if legacy {
            aral_death::publish_approaches(self, &events);
        }
        self.publish_aral_items(&events);
        self.tick_observers(&events);
        if !observer_camps.is_empty() {
            self.publish_observers(&observer_camps);
        }
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
        native_abilities::publish_before_impacts(self, &events);
        self.replicate_with_camps(&events, &team_camps);
        if legacy || self.solo_scene() {
            npc_projectile::publish(self, &events);
        }
        if legacy {
            aral::announcements::publish(self, &events);
        }
        scene::sync_5v5_shops(self);
        modes_3v3_blitz::sync_shops(self);
        if legacy || self.solo_scene() {
            for (i, (seat, actor)) in self.seats.iter().zip(&self.world.entities).enumerate() {
                if !seat.world_actor_written
                    || !seat.preamble.confirmed()
                    || seat.peers[1].is_some()
                {
                    continue;
                }
                if let Some(peer) = &seat.peers[0] {
                    let mut output = Vec::new();
                    let visible = &self.visible[usize::from(actor.team)];
                    self.scene_views[i].append(
                        &self.world,
                        |id| visible.contains(&id),
                        &mut output,
                    );
                    if !output.is_empty() {
                        peer.send(transport::Delivery::Private(output));
                    }
                }
            }
        }
        native_abilities::publish(self, &events);
        if self.world.winner.is_none() {
            self.advance_ballots();
        }
        if self.world.winner.is_some() {
            self.reason = Some(RoomEndReason::Completed);
            self.phase(crate::RoomPhase::Ending);
            if self.known_queue.map(KnownQueue::mode) != Some(NativeMode::Aral) {
                self.finish_world();
            }
        }
    }
    fn finish_world(&mut self) {
        if *self.stop.borrow() {
            return;
        }
        let result = match self.surrender.iter().position(surrender::Ballot::passed) {
            Some(loser) => Message::aral_surrender_result(2 - loser as u8).ok(),
            None => result_frames::crystal_end(&self.queue, &self.world)
                .or_else(|| result_frames::score_or_time_end(&self.queue, &self.world)),
        };
        if let Some(result) = result {
            let mut final_batch = Vec::with_capacity(2);
            match result_frames::statistics_snapshot(&self.world) {
                Ok(report) => final_batch.push(report),
                Err(error) => {
                    tracing::error!(room=%self.id, %error, "cannot encode final match statistics");
                }
            }
            final_batch.push(result);
            self.publish_observers(&final_batch);
            let delivery = transport::TeamBatch::new(final_batch);
            for peer in self.seats.iter().filter_map(|s| s.gameplay_peer()) {
                peer.send(transport::Delivery::Team(delivery.clone()));
            }
        }
        self.stop.send_replace(true);
    }

    fn allocate_entity_declaration(
        &mut self,
        index: usize,
        team: u8,
        native_solo: bool,
        legacy_aral: bool,
        born: bool,
    ) -> Option<Message> {
        if modes_3v3_blitz::is_managed(self.known_queue) {
            return modes_3v3_blitz::allocate_declaration(self, index, native_solo);
        }
        if self.known_queue == Some(KnownQueue::BattleRoyaleBotsSolo) {
            return modes_3v3_blitz::allocate_aral_solo_declaration(self, index);
        }
        let e = &self.world.entities[index];
        if native_solo {
            self.solo_slots
                .declaration_at(&self.world, e, &self.picks, self.data.as_deref(), born)
        } else if legacy_aral {
            self.aral_slots.declaration(
                &self.world,
                e,
                &self.picks,
                self.data.as_deref(),
                team,
                born,
            )
        } else {
            Some(spawn(e, &self.picks, self.data.as_deref()))
        }
    }

    fn drain_5v5_camp_messages(&mut self) -> (Batch, [Batch; 2]) {
        let mut obs = Vec::new();
        let mut teams: [Batch; 2] = [Vec::new(), Vec::new()];
        if !self.world.is_5v5_enabled() {
            return (obs, teams);
        }
        for t in self.world.take_5v5_camp_transitions() {
            let msg = Message::camp_visibility(
                t.actor_id,
                t.viewer_index,
                halcyon_protocol::CampVisibilityState::from_wire(t.state as u8),
            );
            match t.viewer_index {
                0 => obs.push(msg),
                1 => teams[0].push(msg),
                2 => teams[1].push(msg),
                _ => {}
            }
        }
        (obs, teams)
    }

    #[cfg(test)]
    fn replicate(&mut self, events: &[Event]) {
        self.replicate_with_camps(events, &[Vec::new(), Vec::new()]);
    }

    fn replicate_with_camps(&mut self, events: &[Event], team_camps: &[Batch; 2]) {
        let fallen: HashSet<u32> = events
            .iter()
            .filter_map(|event| match event {
                Event::Death { id, .. } => Some(*id),
                _ => None,
            })
            .collect();
        let native_solo = self.solo_scene() && self.seats[0].control_world;
        let postgame = result_frames::aral_lost_crystal(self.legacy_aral(), &self.world);
        let legacy_aral = self.legacy_aral();
        let facing_targets = if legacy_aral {
            self.ringo_attacks.observe(events, &self.world)
        } else {
            [None; aral::SEATS]
        };
        let control_stream = legacy_aral || native_solo;
        let born: HashSet<u32> = if control_stream {
            events
                .iter()
                .filter_map(|event| match event {
                    Event::Spawn { id } => Some(*id),
                    _ => None,
                })
                .collect()
        } else {
            HashSet::new()
        };
        for team in 0..2 {
            let has_peers = self
                .seats
                .iter()
                .zip(&self.world.entities)
                .any(|(seat, actor)| {
                    usize::from(actor.team) == team && seat.gameplay_peer().is_some()
                });
            if !has_peers {
                continue;
            }
            let control_seats: Vec<usize> = if control_stream {
                self.seats
                    .iter()
                    .zip(&self.world.entities)
                    .enumerate()
                    .filter(|(_, (seat, actor))| {
                        usize::from(actor.team) == team
                            && seat.gameplay_peer().is_some()
                            && seat.peers[1].is_none()
                            && seat.preamble.confirmed()
                    })
                    .map(|(i, _)| i)
                    .collect()
            } else {
                Vec::new()
            };
            let mut out = vec![];
            let mut per_seat_decls: Vec<(usize, Batch)> = control_seats
                .iter()
                .map(|&seat| (seat, Vec::new()))
                .collect();
            let mut newly_declared = HashSet::new();
            let mut guard_deaths = Vec::new();
            // Compute new team visibility into `self.next_visible[team]` without aliasing `self`.
            let mut current = std::mem::take(&mut self.next_visible[team]);
            current.clear();
            for idx in 0..self.world.entities.len() {
                let (id, hp, is_visible) = {
                    let e = &self.world.entities[idx];
                    (e.id, e.hp, e.hp > 0. && self.world.visible(team as u8, e))
                };
                if !is_visible || hp <= 0. {
                    continue;
                }
                if !control_seats.is_empty() {
                    let missing = control_seats
                        .iter()
                        .filter(|&&seat| !self.scene_views[seat].knows(id))
                        .count();
                    if missing == 0 {
                        current.insert(id);
                        continue;
                    }
                    let Some(message) = self.allocate_entity_declaration(
                        idx,
                        team as u8,
                        native_solo,
                        legacy_aral,
                        born.contains(&id),
                    ) else {
                        continue;
                    };
                    current.insert(id);
                    let e = &self.world.entities[idx];
                    if missing == control_seats.len() {
                        newly_declared.insert(id);
                        out.push(message);
                        messages::append_inventory(&mut out, &self.world, e);
                    } else {
                        for (seat, decls) in &mut per_seat_decls {
                            if !self.scene_views[*seat].knows(id) {
                                decls.push(message.clone());
                                messages::append_inventory(decls, &self.world, e);
                                if uses_visibility_toggle(team as u8, e)
                                    && self.visible[team].contains(&id)
                                {
                                    decls.push(actor_visibility_message(id, team as u8, true));
                                }
                            }
                        }
                    }
                } else {
                    if !self.visible[team].contains(&id) {
                        let Some(message) = self.allocate_entity_declaration(
                            idx,
                            team as u8,
                            native_solo,
                            legacy_aral,
                            born.contains(&id),
                        ) else {
                            continue;
                        };
                        newly_declared.insert(id);
                        out.push(message);
                        messages::append_inventory(
                            &mut out,
                            &self.world,
                            &self.world.entities[idx],
                        );
                    }
                    current.insert(id);
                }
            }
            self.solo_structure_snapshot.append(&mut out);
            for (seat, decls) in &mut per_seat_decls {
                if !decls.is_empty() {
                    self.solo_structure_snapshot.append(decls);
                    self.scene_views[*seat].declarations(decls);
                }
            }
            if !out.is_empty() {
                for &seat in &control_seats {
                    self.scene_views[seat].declarations(&out);
                }
            }
            if native_solo
                && usize::from(self.world.entities[0].team) == team
                && self.seats[0].world_actor_written
                && self.seats[0].preamble.confirmed()
                && self.seats[0].peers[1].is_none()
            {
                self.scene_views[0].append_solo_bot_paths(
                    &self.world,
                    &self.world.entities[0],
                    &current,
                    events,
                    &mut out,
                );
            }
            let previous = &mut self.visible[team];
            let viewer_team = team as u8;
            for e in &self.world.entities {
                if !uses_visibility_toggle(viewer_team, e) {
                    continue;
                }
                let was_visible = previous.contains(&e.id);
                let now_visible = current.contains(&e.id);
                if !was_visible && now_visible {
                    out.push(actor_visibility_message(e.id, viewer_team, true));
                    if !newly_declared.contains(&e.id)
                        && !events
                            .iter()
                            .any(|ev| matches!(ev, Event::Move { id, .. } if *id == e.id))
                    {
                        out.push(Message::position(wire_id(e.id), e.position.x, e.position.y));
                    }
                } else if was_visible && !now_visible && e.hp > 0.0 && !fallen.contains(&e.id) {
                    out.push(actor_visibility_message(e.id, viewer_team, false));
                }
            }
            let mut has_attack_release = false;
            for ev in events {
                if legacy_aral
                    && ringo::append(&mut out, ev, |id| {
                        current.contains(&id) || fallen.contains(&id) && previous.contains(&id)
                    })
                {
                    has_attack_release = true;
                    continue;
                }
                if control_stream
                    && let Some(m) = npc_attack::release(&self.world, ev, |id| {
                        current.contains(&id) || fallen.contains(&id) && previous.contains(&id)
                    })
                {
                    out.push(m);
                    has_attack_release = true;
                    continue;
                }
                result_frames::append_postgame(&mut out, ev, postgame);
                if hero_progress::append(&self.world, &mut out, ev, |id| {
                    !newly_declared.contains(&id)
                        && (current.contains(&id)
                            || (fallen.contains(&id) && previous.contains(&id)))
                }) {
                    continue;
                }
                if let Event::Purchase(receipt) = ev {
                    if legacy_aral && newly_declared.contains(&receipt.actor) {
                        continue;
                    }
                    if current.contains(&receipt.actor)
                        || fallen.contains(&receipt.actor) && previous.contains(&receipt.actor)
                    {
                        messages::append_purchase(
                            &mut out,
                            &self.world,
                            receipt,
                            !newly_declared.contains(&receipt.actor),
                        );
                    }
                    continue;
                }
                if let Event::GoldAward { id, amount } = ev {
                    if legacy_aral && newly_declared.contains(id) {
                        continue;
                    }
                    if current.contains(id) || fallen.contains(id) && previous.contains(id) {
                        out.push(Message::entity_stat(wire_id(*id), *amount as f32, 6));
                    }
                    continue;
                }
                if let Event::Move { id, position } = ev
                    && current.contains(id)
                {
                    out.push(Message::position(wire_id(*id), position.x, position.y));
                } else {
                    let before = out.len();
                    messages::append_combat(&mut out, ev, |actor| {
                        current.contains(&actor)
                            || (fallen.contains(&actor) && previous.contains(&actor))
                    });
                    if legacy_aral && let Event::Death { id, .. } = ev {
                        let death_sent = out.len() != before;
                        if death_sent {
                            guard_deaths.extend(self.world.aral_structure_slot(*id));
                            aral_death::witness(
                                &mut self.dead_actors,
                                *id,
                                team as u8,
                                &self.seats,
                                &self.world,
                            );
                        }
                        let known_ally = previous.contains(id)
                            && self.world.entities.iter().any(|actor| {
                                actor.id == *id
                                    && actor.kind == halcyon_sim::Kind::Hero
                                    && usize::from(actor.team) == team
                            });
                        if death_sent || known_ally {
                            messages::append_respawn_timer(&mut out, &self.world, *id);
                        }
                    }
                }
            }
            *previous = current;
            self.next_visible[team] = HashSet::new();
            if control_stream {
                if self.world.tick.is_multiple_of(10 * halcyon_sim::TICK_HZ) {
                    out.push(messages::world_clock(self.world.tick));
                }
            } else if self.world.tick.is_multiple_of(20) {
                out.push(snapshot(&self.world, &self.players, &self.picks));
            }
            out.extend(team_camps[team].iter().cloned());
            for (seat, decls) in per_seat_decls {
                if !decls.is_empty()
                    && let Some(peer) = self.seats[seat].gameplay_peer()
                {
                    peer.send(transport::Delivery::Private(decls));
                }
            }
            if out.is_empty() {
                continue;
            }
            let mut peers = self
                .seats
                .iter()
                .zip(&self.world.entities)
                .filter(|(_, actor)| usize::from(actor.team) == team)
                .filter_map(|(seat, _)| seat.gameplay_peer())
                .peekable();
            let Some(first) = peers.next() else {
                continue;
            };
            if has_attack_release {
                npc_attack::deliver(
                    out,
                    &self.seats,
                    &self.world,
                    &mut self.scene_views,
                    team,
                    &facing_targets,
                );
            } else if peers.peek().is_none() {
                first.send(transport::Delivery::Private(out));
            } else {
                let batch = transport::TeamBatch::new(out);
                for peer in std::iter::once(first).chain(peers) {
                    peer.send(transport::Delivery::Team(batch.clone()));
                }
            }
            if !guard_deaths.is_empty() {
                for ((seat, hero), view) in self
                    .seats
                    .iter()
                    .zip(&self.world.entities)
                    .zip(&mut self.scene_views)
                {
                    if usize::from(hero.team) != team
                        || !seat.world_actor_written
                        || !seat.preamble.confirmed()
                        || seat.peers[1].is_some()
                    {
                        continue;
                    }
                    let Some(peer) = seat.gameplay_peer() else {
                        continue;
                    };
                    let mut updates = Batch::new();
                    for &slot in &guard_deaths {
                        view.guard_death(slot, &mut updates);
                    }
                    if !updates.is_empty() {
                        peer.send(transport::Delivery::Private(updates));
                    }
                }
            }
        }
        if self.solo_scene() || modes_3v3_blitz::is_managed(self.known_queue) {
            for ev in events {
                if let Event::Death { id, .. } = *ev {
                    self.solo_slots.released(id);
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn returning_socket_records_its_own_first_message() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        let player = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: id,
                players: vec![player],
                teams: vec![0],
                mode: "solo".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut room = shared.lock().unwrap();
        for _ in 0..2 {
            let (tx, _rx) = mpsc::channel(16);
            let (seat, ch, generation, _) =
                room.attach(player, tx, watch::channel(false).0).unwrap();
            for _ in 0..2 {
                room.receive(
                    seat,
                    ch,
                    Message::known(crate::Opcode::RequestSceneSnapshot, [0; 6]),
                )
                .unwrap();
            }
            room.detach(seat, ch, generation);
        }
        let events: Vec<_> = room
            .events
            .iter()
            .map(responses::value)
            .filter(|event| event["opcode"] == "0x046E")
            .collect();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["generation"], 1);
        assert_eq!(events[1]["generation"], 2);
    }
    #[tokio::test]
    async fn ranked_and_solo_draft_requests_repeat_the_registered_catalog_and_current_roster() {
        for (queue, mode, count) in [
            ("ranked", "*GameMode_HF_Ranked*", 6),
            ("solo", "*GameMode_5v5_SoloBots*", 1),
        ] {
            let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
            let id = Uuid::new_v4();
            let players = (0..count).map(|_| Uuid::new_v4()).collect::<Vec<_>>();
            rooms
                .reserve(Reservation {
                    talent_levels: Vec::new(),
                    match_id: id,
                    players: players.clone(),
                    teams: vec![],
                    mode: queue.into(),
                    difficulty: None,
                })
                .unwrap();
            let shared = rooms.room(id).unwrap();
            let mut room = shared.lock().unwrap();
            let (tx, _rx) = mpsc::channel(16);
            let (_, _, _, initial) = room
                .attach(players[0], tx, watch::channel(false).0)
                .unwrap();
            assert_eq!(initial[0].opcode, crate::wire_opcode::MatchSettings);
            assert_eq!(initial[1].opcode, crate::wire_opcode::DraftModeAnnouncement);
            assert_eq!(&initial[1].body[4..4 + mode.len()], mode.as_bytes());
            for _ in 0..3 {
                let request = Message::known(crate::Opcode::DraftRefreshRequest, [0; 6]);
                let result = room.receive(0, 0, request.clone()).unwrap();
                assert_eq!(result.last(), Some(&request));
                assert_eq!(result[..result.len() - 1], initial[1..]);
            }
            let malformed = Message::known(crate::Opcode::DraftRefreshRequest, [0, 1, 0, 0, 0, 0]);
            assert_eq!(room.receive(0, 0, malformed.clone()).unwrap(), [malformed]);
        }
    }
    #[tokio::test]
    async fn ranked_three_per_side_from_platform_to_each_draft_seat() {
        use halcyon_platform::{AppState, PlatformConfig, plan_teams_for_mode};
        let dir = tempfile::tempdir().unwrap();
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 2, None);
        let app = AppState::open(
            PlatformConfig {
                database_path: dir.path().join("ranked.sqlite"),
                public_address: "localhost:8443".into(),
                seats_per_match: 10,
            },
            Arc::new(rooms.clone()),
        )
        .unwrap();
        assert_eq!(app.seats_for_mode("ranked"), 6);
        assert_eq!(app.seats_for_mode("ranked_5v5"), 10);
        assert_eq!(plan_teams_for_mode("ranked", &[3, 3]), [0, 0, 0, 1, 1, 1]);
        let mut guests = Vec::new();
        for i in 0..6 {
            guests.push(app.create_guest(format!("Seat{i}")).await.unwrap());
        }
        assert!(
            app.join_with_seats(&guests[0].session_token, "ranked".into(), 10)
                .await
                .is_err()
        );
        assert!(
            app.federated_join(&guests[0].session_token, Uuid::new_v4(), "ranked".into(), 5)
                .await
                .is_err()
        );
        for guest in &guests {
            app.join(&guest.session_token, "ranked".into())
                .await
                .unwrap();
        }
        let id = app
            .profile(&guests[0].session_token)
            .await
            .unwrap()
            .lobby
            .unwrap()
            .match_id
            .unwrap();
        for guest in &guests {
            app.accept(&guest.session_token, id).await.unwrap();
        }
        let shared = rooms.room(id).unwrap();
        let mut room = shared.lock().unwrap();
        assert_eq!(room.picks.len(), 6);
        let heroes: Vec<_> = room
            .world
            .entities
            .iter()
            .filter(|e| e.kind == halcyon_sim::Kind::Hero)
            .collect();
        assert_eq!(heroes.len(), 6);
        assert!(heroes.iter().all(|e| !e.bot && e.gold == 0));
        assert_eq!(heroes.iter().filter(|e| e.team == 0).count(), 3);
        assert!(
            heroes
                .iter()
                .filter(|e| e.team == 0)
                .all(|e| e.position.x == heroes[0].position.x)
        );
        assert_ne!(heroes[0].position.x, heroes[1].position.x);
        drop(heroes);
        for (seat, guest) in guests.iter().enumerate() {
            let (tx, _rx) = mpsc::channel(16);
            let (_, _, _, opening) = room
                .attach(guest.player_id, tx, watch::channel(false).0)
                .unwrap();
            assert_eq!(&opening[0].body[16..18], &[3, 3]);
            assert_eq!(opening[0].body[100], room.world.entities[seat].team + 1);
            let roster = opening.last().unwrap();
            assert_eq!(roster.opcode, crate::wire_opcode::DraftRosterFull);
            for (i, row) in roster.body.as_chunks::<161>().0.iter().enumerate() {
                assert_eq!(row[8], u8::from(i < 6));
                if let Some(participant) = guests.get(i) {
                    assert_eq!(&row[105..141], participant.player_id.to_string().as_bytes());
                    assert_eq!(row[16], room.world.entities[i].team + 1);
                }
            }
            let req = Message::known(crate::Opcode::DraftRefreshRequest, [0; 6]);
            let repeat = room.receive(seat, 0, req).unwrap();
            assert_eq!(&repeat[..repeat.len() - 1], &opening[1..]);
        }
        drop(room);
        let players: Vec<_> = guests.iter().map(|g| g.player_id).collect();
        for (roster, teams) in [
            (players[..5].to_vec(), vec![]),
            (players.clone(), vec![0, 0, 0, 0, 1, 1]),
        ] {
            assert!(
                rooms
                    .reserve(Reservation {
                        talent_levels: Vec::new(),
                        match_id: Uuid::new_v4(),
                        mode: "ranked".into(),
                        players: roster,
                        teams,
                        difficulty: None
                    })
                    .is_err()
            );
        }
        let five = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: five,
                mode: "ranked_5v5".into(),
                players,
                teams: vec![],
                difficulty: None,
            })
            .unwrap();
        assert_eq!(rooms.room(five).unwrap().lock().unwrap().picks.len(), 10);
    }

    #[tokio::test]
    async fn shop_uses_authenticated_seat_and_authoritative_gold() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: id,
                players: vec![Uuid::new_v4()],
                teams: vec![0],
                mode: "solo".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut room = shared.lock().unwrap();
        room.phase(crate::RoomPhase::Running);
        room.world.entities[0].gold = 300;
        let buy = |actor: u32| {
            Message::known(
                crate::Opcode::BuyItem,
                [actor.to_be_bytes(), 458u32.to_be_bytes()].concat(),
            )
        };
        assert!(room.receive(0, 1, buy(1501)).unwrap().is_empty());
        assert!(room.receive(0, 0, buy(1500)).unwrap().is_empty());
        assert_eq!(room.world.entities[0].gold, 300);
        let ack = room.receive(0, 1, buy(1500)).unwrap();
        assert_eq!(
            ack.iter().map(|m| m.opcode).collect::<Vec<_>>(),
            [
                crate::wire_opcode::BuyItem,
                crate::wire_opcode::ModifyActorResource,
                crate::wire_opcode::ItemGrant,
                crate::wire_opcode::ModifyActorAttribute
            ]
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
        let upgrade = Message::known(
            crate::Opcode::BuyItem,
            [1500u32.to_be_bytes(), 505u32.to_be_bytes()].concat(),
        );
        let upgraded = room.receive(0, 1, upgrade).unwrap();
        assert_eq!(
            upgraded.iter().map(|m| m.opcode).collect::<Vec<_>>(),
            [
                crate::wire_opcode::BuyItem,
                crate::wire_opcode::ModifyActorResource,
                crate::wire_opcode::ModifyActorAttribute,
                crate::wire_opcode::ConsumeItem,
                crate::wire_opcode::ItemGrant,
                crate::wire_opcode::ModifyActorAttribute
            ]
        );
        assert_eq!(&upgraded[3].body[4..8], &ack[2].body[8..12]);
        assert_eq!(room.world.entities[0].inventory.len(), 1);
        room.world.entities[0].gold = 300;
        let request = |template: u32| {
            Message::known(
                Opcode::BuyItem,
                [1500u32.to_be_bytes(), template.to_be_bytes()].concat(),
            )
        };
        assert!(room.receive(0, 1, request(549)).unwrap().is_empty());
        assert!(room.receive(0, 1, request(495)).unwrap().is_empty());
        let granted = room.receive(0, 1, request(532)).unwrap();
        assert!(granted.iter().any(|m| matches!(
            m.fields(),
            Ok(Fields::ItemGrant {
                actor: 1500,
                item: 532,
                ..
            })
        )));
        assert_eq!(room.world.entities[0].gold, 0);
    }
}
