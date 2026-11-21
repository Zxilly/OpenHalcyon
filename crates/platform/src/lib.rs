pub use halcyon_data::{
    ARAL_TALENT_HEROES, ARAL_TALENT_RULES_PREFIX, BotDifficulty, BotDifficultyExt, KnownQueue,
    LEGACY_ARAL_QUEUE, LEGACY_ARAL_SEATS, TalentLevels,
};
pub use halcyon_types::{
    HostError, HostResult, HostedMatch, HostedSpectator, MatchHost, NativeOffer, NativeSeat,
    Reservation, SpectatorExit, player_actions,
};
mod accounts;
mod chat;
mod event_body;
mod federation;
mod native_offer;
mod orm_schema;
mod settlement;
use event_body::EventBody;
pub use event_body::{AbortReason, EventKind, LobbyPhase};
mod notification;
mod pooled_spectator;
pub use pooled_spectator::PooledSpectator;
mod queue;
mod recovery;
mod spectator;
mod talent_levels;
pub use federation::FederatedIntent;
pub use queue::{plan_teams, plan_teams_for_mode};

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Extension, Json, Router,
    extract::{
        Path, Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::watch;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct PlatformConfig {
    pub database_path: PathBuf,
    pub public_address: String,
    pub seats_per_match: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("authentication required")]
    Unauthorized,
    #[error("{0}")]
    Invalid(&'static str),
    #[error("{0}")]
    Conflict(&'static str),
    #[error("match host unavailable")]
    HostUnavailable,
    #[error("storage unavailable: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("platform worker unavailable")]
    WorkerUnavailable,
}

impl From<HostError> for PlatformError {
    fn from(error: HostError) -> Self {
        match error {
            HostError::Unauthorized => Self::Unauthorized,
            HostError::Invalid(message) => Self::Invalid(message),
            HostError::Conflict(message) => Self::Conflict(message),
            HostError::HostUnavailable => Self::HostUnavailable,
            HostError::Database(message) => {
                tracing::error!(error = %message, "host database request failed");
                Self::WorkerUnavailable
            }
            HostError::WorkerUnavailable => Self::WorkerUnavailable,
        }
    }
}

impl From<PlatformError> for HostError {
    fn from(error: PlatformError) -> Self {
        match error {
            PlatformError::Unauthorized => Self::Unauthorized,
            PlatformError::Invalid(message) => Self::Invalid(message),
            PlatformError::Conflict(message) => Self::Conflict(message),
            PlatformError::HostUnavailable => Self::HostUnavailable,
            PlatformError::Database(error) => Self::Database(error.to_string()),
            PlatformError::WorkerUnavailable => Self::WorkerUnavailable,
        }
    }
}

impl IntoResponse for PlatformError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "authentication required"),
            Self::Invalid(message) => (StatusCode::BAD_REQUEST, message),
            Self::Conflict(message) => (StatusCode::CONFLICT, message),
            Self::HostUnavailable => (StatusCode::SERVICE_UNAVAILABLE, "match host unavailable"),
            Self::Database(_) | Self::WorkerUnavailable => {
                tracing::error!(error = %self, "platform request failed");
                (StatusCode::INTERNAL_SERVER_ERROR, "platform unavailable")
            }
        };
        #[derive(Serialize)]
        struct ErrorResponse {
            error: &'static str,
        }
        (status, Json(ErrorResponse { error: message })).into_response()
    }
}

pub type Result<T> = std::result::Result<T, PlatformError>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Guest {
    pub player_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_token: Option<String>,
    pub session_token: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerProfile {
    pub player_id: Uuid,
    pub name: String,
    pub lobby: Option<Lobby>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Lobby {
    pub mode: String,
    pub match_id: Option<Uuid>,
    pub phase: String,
    pub address: Option<String>,
    pub result: Option<Value>,
    #[serde(skip_serializing)]
    pub spectator_id: Option<Uuid>,
    #[serde(skip_serializing)]
    pub settled_at_ms: Option<i64>,
    #[serde(skip_serializing)]
    pub queued_at_ms: Option<i64>,
}

impl Lobby {
    pub fn phase_kind(&self) -> Option<LobbyPhase> {
        LobbyPhase::parse(&self.phase)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerPage {
    pub total: u64,
    pub players: Vec<PlayerProfile>,
}

pub struct PlayerActivity {
    pub profile: PlayerProfile,
    pub last_seen: i64,
    pub online: bool,
}

pub struct PlatformCounts {
    pub online_players: u64,
    pub queued_players: u64,
}

fn parse_id(id: &str) -> Result<Uuid> {
    Uuid::parse_str(id).map_err(|_| PlatformError::WorkerUnavailable)
}

#[derive(Clone)]
pub struct AppState {
    db: Arc<Mutex<Connection>>,
    accounts: Arc<Mutex<halcyon_storage::AccountStore>>,
    host: Arc<dyn MatchHost>,
    changes: watch::Sender<()>,
    config: Arc<PlatformConfig>,
    talent_levels: Arc<Mutex<Option<talent_levels::Resolver>>>,
}

impl AppState {
    pub fn seats_per_match(&self) -> usize {
        self.config.seats_per_match
    }

    pub fn seats_for_mode(&self, mode: &str) -> usize {
        match KnownQueue::parse(mode) {
            Some(queue) => queue.human_seats(self.seats_per_match()),
            None => self.seats_per_match(),
        }
    }

    pub fn open(config: PlatformConfig, host: Arc<dyn MatchHost>) -> Result<Self> {
        if !(1..=64).contains(&config.seats_per_match) {
            return Err(PlatformError::Invalid(
                "seats_per_match must be between 1 and 64",
            ));
        }
        let address = halcyon_storage::DatabaseAddress::new(&config.database_path);
        let mut db = Connection::open(address.path())?;
        db.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
             CREATE TABLE IF NOT EXISTS players (
                id TEXT PRIMARY KEY, name TEXT NOT NULL, recovery BLOB NOT NULL UNIQUE,
                last_seen INTEGER NOT NULL DEFAULT (unixepoch())
             );
             CREATE TABLE IF NOT EXISTS sessions (
                key BLOB PRIMARY KEY, player TEXT NOT NULL UNIQUE REFERENCES players(id)
             );
             CREATE TABLE IF NOT EXISTS matches (
                id TEXT PRIMARY KEY, mode TEXT NOT NULL, phase TEXT NOT NULL,
                address TEXT, result TEXT, offered_at INTEGER NOT NULL DEFAULT (unixepoch()), difficulty TEXT
             );
             CREATE TABLE IF NOT EXISTS tickets (
                serial INTEGER PRIMARY KEY AUTOINCREMENT,
                player TEXT NOT NULL UNIQUE REFERENCES players(id), mode TEXT NOT NULL,
                match_id TEXT REFERENCES matches(id), accepted INTEGER NOT NULL DEFAULT 0,
                batch TEXT, seat_limit INTEGER, difficulty TEXT
             );
             CREATE TABLE IF NOT EXISTS events (
                cursor INTEGER PRIMARY KEY AUTOINCREMENT,
                player TEXT NOT NULL REFERENCES players(id), body TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS events_by_player ON events(player, cursor);
             CREATE INDEX IF NOT EXISTS tickets_by_mode ON tickets(mode, match_id, serial);
             CREATE INDEX IF NOT EXISTS tickets_by_match ON tickets(match_id);",
        )?;
        {
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let mut query = tx.prepare("SELECT name FROM pragma_table_info('tickets')")?;
            let columns = query
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            drop(query);
            if !columns.iter().any(|column| column == "batch") {
                tx.execute_batch("ALTER TABLE tickets ADD COLUMN batch TEXT")?;
            }
            if !columns.iter().any(|column| column == "seat_limit") {
                tx.execute_batch("ALTER TABLE tickets ADD COLUMN seat_limit INTEGER")?;
            }
            if !columns.iter().any(|column| column == "difficulty") {
                tx.execute_batch("ALTER TABLE tickets ADD COLUMN difficulty TEXT")?;
            }
            queue::add_difficulty_column(&tx, "matches")?;
            halcyon_community::queue::initialize(&tx).map_err(queue::community_error)?;
            federation::initialize(&tx)?;
            native_offer::initialize(&tx)?;
            notification::initialize(&tx)?;
            spectator::initialize(&tx)?;
            pooled_spectator::initialize(&tx)?;
            talent_levels::initialize(&tx)?;
            tx.commit()?;
        }
        let accounts =
            halcyon_storage::AccountStore::open(&address).map_err(accounts::storage_error)?;
        let (changes, _) = watch::channel(());
        let state = Self {
            db: Arc::new(Mutex::new(db)),
            accounts: Arc::new(Mutex::new(accounts)),
            host,
            changes,
            config: Arc::new(config),
            talent_levels: Default::default(),
        };
        state.resume_reservations()?;
        Ok(state)
    }

    fn transaction<T>(&self, work: impl FnOnce(&Transaction<'_>) -> Result<T>) -> Result<T> {
        let mut db = self
            .db
            .lock()
            .map_err(|_| PlatformError::WorkerUnavailable)?;
        let before = db.total_changes();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let value = work(&tx)?;
        tx.commit()?;
        let changed = db.total_changes() != before;
        drop(db);
        if changed {
            self.changes.send_replace(());
        }
        Ok(value)
    }

    async fn run<T: Send + 'static>(
        &self,
        work: impl FnOnce(&AppState) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let state = self.clone();
        tokio::task::spawn_blocking(move || work(&state))
            .await
            .map_err(|_| PlatformError::WorkerUnavailable)?
    }

    fn resume_reservations(&self) -> Result<()> {
        let reservations = self.transaction(|tx| {
            let mut matches = tx.prepare(
                "SELECT id, mode, difficulty, talent_levels FROM matches WHERE phase='reserving'",
            )?;
            let rows = matches
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        queue::read_difficulty(row, 2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows.into_iter()
                .map(|(match_id, mode, difficulty, stored)| {
                    let (players, teams, _) = queue::match_players(tx, &match_id, &mode)?;
                    Ok(Reservation {
                        talent_levels: talent_levels::decode(stored.as_deref())?,
                        match_id: parse_id(&match_id)?,
                        players,
                        teams,
                        mode,
                        difficulty,
                    })
                })
                .collect::<Result<Vec<_>>>()
        })?;
        for reservation in reservations {
            match self.reserve_match(reservation) {
                Ok(()) | Err(PlatformError::HostUnavailable) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    fn reserve_match(&self, reservation: Reservation) -> Result<()> {
        let match_id = reservation.match_id.to_string();
        let host = match self.host.reserve(reservation) {
            Ok(hosted) if !hosted.address.trim().is_empty() => hosted,
            result => {
                if let Err(error) = result {
                    tracing::warn!(%match_id, %error, "match host rejected reservation");
                }
                self.transaction(|tx| abort(tx, &match_id, AbortReason::HostUnavailable))?;
                return Err(PlatformError::HostUnavailable);
            }
        };
        self.transaction(|tx| {
            tx.execute(
                "UPDATE matches SET phase='playing', address=? WHERE id=?",
                params![host.address, match_id],
            )?;
            announce(
                tx,
                &match_id,
                EventBody::MatchPlaying {
                    match_id: parse_id(&match_id)?,
                    address: &host.address,
                },
            )
        })
    }

    /// Expire unanswered offers. A deployment may call this periodically.
    pub async fn expire_offers(&self) -> Result<()> {
        self.run(|state| {
            state.transaction(|tx| {
                let mut query = tx.prepare(
                "SELECT id FROM matches WHERE phase='offered' AND offered_at <= unixepoch() - 30",
            )?;
                let ids = query
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                for id in ids {
                    abort(tx, &id, AbortReason::OfferExpired)?;
                }
                Ok(())
            })
        })
        .await
    }
}

mod action_api;
pub fn router(state: AppState) -> Router {
    router_with_matches(state.clone(), Arc::new(state))
}

/// Match operations selected by the home service. The public API authenticates
/// with the same opaque bearer token regardless of the configured authority.
#[async_trait::async_trait]
pub trait PublicMatches: Send + Sync {
    async fn player_action(
        &self,
        _token: &str,
        _room: Uuid,
        _call: player_actions::ActionCall,
    ) -> Result<player_actions::ActionReply> {
        Err(PlatformError::HostUnavailable)
    }

    async fn enqueue(
        &self,
        token: &str,
        mode: String,
        difficulty: Option<BotDifficulty>,
    ) -> Result<()>;
    async fn cancel_queue(&self, token: &str) -> Result<()>;
    async fn accept_offer(&self, token: &str, id: Uuid) -> Result<()>;
    async fn match_chat(&self, token: &str, id: Uuid, text: String) -> Result<()>;
}

pub fn router_with_matches(state: AppState, matches: Arc<dyn PublicMatches>) -> Router {
    Router::new()
        .route("/bootstrap", get(bootstrap))
        .route("/api/v1/guests", post(create_guest))
        .route("/api/v1/sessions", post(resume_guest))
        .route("/api/v1/me", get(profile))
        .route("/api/v1/queue", post(queue).delete(leave))
        .route("/api/v1/matches/{id}/accept", post(accept))
        .route("/api/v1/matches/{id}/chat", post(chat))
        .route(
            "/api/v1/matches/{id}/actions",
            get(action_api::inspect)
                .post(action_api::execute)
                .layer(axum::extract::DefaultBodyLimit::max(1024)),
        )
        .route("/api/v1/events", get(poll))
        .route("/api/v1/events/ws", get(websocket))
        .layer(Extension(matches))
        .with_state(state)
}

#[async_trait::async_trait]
impl PublicMatches for AppState {
    async fn player_action(
        &self,
        token: &str,
        room: Uuid,
        call: player_actions::ActionCall,
    ) -> Result<player_actions::ActionReply> {
        let player = self.playing_player(token, room).await?;
        self.host
            .local_action(room, player, call)
            .map_err(Into::into)
    }
    async fn enqueue(
        &self,
        token: &str,
        mode: String,
        difficulty: Option<BotDifficulty>,
    ) -> Result<()> {
        let seats = self.seats_for_mode(&mode);
        self.join_with_difficulty(token, mode, seats, difficulty)
            .await
    }

    async fn cancel_queue(&self, token: &str) -> Result<()> {
        self.leave(token).await
    }
    async fn accept_offer(&self, token: &str, id: Uuid) -> Result<()> {
        self.accept(token, id).await
    }
    async fn match_chat(&self, token: &str, id: Uuid, text: String) -> Result<()> {
        self.send_chat(token, id, text).await
    }
}

async fn bootstrap(State(state): State<AppState>) -> String {
    format!(
        "{}
",
        state.config.public_address
    )
}

fn display_name(name: String) -> Result<String> {
    let name = name.trim().to_owned();
    if name.is_empty() || name.chars().count() > 32 || name.chars().any(char::is_control) {
        return Err(PlatformError::Invalid("invalid display name"));
    }
    Ok(name)
}

fn credential() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn digest(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

fn bearer(headers: &HeaderMap) -> Result<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .filter(|v| v.len() == 64)
        .ok_or(PlatformError::Unauthorized)
}

fn session_owner(db: &Connection, key: &[u8]) -> Result<String> {
    db.query_row("SELECT player FROM sessions WHERE key=?", [key], |row| {
        row.get(0)
    })
    .optional()?
    .ok_or(PlatformError::Unauthorized)
}

fn player(db: &Connection, key: &[u8]) -> Result<String> {
    let id = session_owner(db, key)?;
    db.execute(
        "UPDATE players SET last_seen=unixepoch() WHERE id=? AND last_seen < unixepoch() - 15",
        [&id],
    )?;
    Ok(id)
}

#[derive(Deserialize, Serialize)]
pub struct CreateGuest {
    pub name: String,
}

async fn create_guest(
    State(state): State<AppState>,
    Json(input): Json<CreateGuest>,
) -> Result<Json<Guest>> {
    state.create_guest(input.name).await.map(Json)
}
impl AppState {
    pub async fn create_guest(&self, name: String) -> Result<Guest> {
        let name = display_name(name)?;
        self.run(move |state| {
            let player_id = Uuid::new_v4();
            let recovery = credential();
            let session = credential();
            state
                .accounts
                .lock()
                .map_err(|_| PlatformError::WorkerUnavailable)?
                .create_guest(player_id, &name, &digest(&recovery), &digest(&session))
                .map_err(accounts::storage_error)?;
            state.changes.send_replace(());
            Ok(Guest {
                player_id,
                recovery_token: Some(recovery),
                session_token: session,
            })
        })
        .await
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeGuest {
    pub recovery_token: String,
}

async fn resume_guest(
    State(state): State<AppState>,
    Json(input): Json<ResumeGuest>,
) -> Result<Json<Guest>> {
    state.resume_guest(input.recovery_token).await.map(Json)
}
impl AppState {
    pub async fn resume_guest(&self, recovery_token: String) -> Result<Guest> {
        self.run(move |state| {
            let session = credential();
            let player_id = state
                .accounts
                .lock()
                .map_err(|_| PlatformError::WorkerUnavailable)?
                .resume_guest(&digest(&recovery_token), &digest(&session))
                .map_err(accounts::storage_error)?;
            state.changes.send_replace(());
            Ok(Guest {
                player_id,
                recovery_token: None,
                session_token: session,
            })
        })
        .await
    }
}

async fn profile(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<PlayerProfile>> {
    state.profile(bearer(&headers)?).await.map(Json)
}
impl AppState {
    pub async fn profile(&self, session_token: &str) -> Result<PlayerProfile> {
        let key = digest(session_token);
        self.run(move |state| {
            let db = state
                .db
                .lock()
                .map_err(|_| PlatformError::WorkerUnavailable)?;
            let id = player(&db, &key)?;
            let sql = format!("SELECT {PROFILE_FIELDS} {PROFILE_JOINS} WHERE p.id=?");
            db.query_row(&sql, [&id], read_profile).map_err(Into::into)
        })
        .await
    }
}

impl AppState {
    pub async fn player_activity(&self, id: Uuid) -> Result<Option<PlayerActivity>> {
        self.run(move |state| {
            let db = state
                .db
                .lock()
                .map_err(|_| PlatformError::WorkerUnavailable)?;
            let sql = format!(
                "SELECT {PROFILE_FIELDS}, p.last_seen, {ONLINE_SQL} {PROFILE_JOINS} WHERE p.id=?"
            );
            db.query_row(&sql, [id.to_string()], read_activity)
                .optional()
                .map_err(Into::into)
        })
        .await
    }

    pub async fn player_activity_page(
        &self,
        after: Option<Uuid>,
        online: Option<bool>,
        limit: usize,
    ) -> Result<Vec<PlayerActivity>> {
        if !(1..=101).contains(&limit) {
            return Err(PlatformError::Invalid("invalid activity page size"));
        }
        self.run(move |state| {
            let db = state
                .db
                .lock()
                .map_err(|_| PlatformError::WorkerUnavailable)?;
            let sql = format!(
                "SELECT {PROFILE_FIELDS}, p.last_seen, {ONLINE_SQL} {PROFILE_JOINS} WHERE p.id > ?1 AND (?2 IS NULL OR ({ONLINE_SQL}) = ?2)
                 ORDER BY p.id LIMIT ?3"
            );
            let after = after.map_or_else(String::new, |id| id.to_string());
            let mut statement = db.prepare(&sql)?;
            statement
                .query_map(params![after, online, limit], read_activity)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(Into::into)
        })
        .await
    }

    pub async fn counts(&self) -> Result<PlatformCounts> {
        self.expire_offers().await?;
        self.run(|state| {
            let db = state
                .db
                .lock()
                .map_err(|_| PlatformError::WorkerUnavailable)?;
            let sql = format!(
                "SELECT count(*) FILTER (WHERE {ONLINE_SQL}),
                        count(*) FILTER (WHERE (t.player IS NOT NULL AND
                          (t.match_id IS NULL OR m.phase IN ('offered', 'reserving')))
                          OR f.phase IN ('pending', 'queued', 'offered'))
                 FROM players p LEFT JOIN sessions s ON s.player=p.id
                 LEFT JOIN tickets t ON t.player=p.id LEFT JOIN matches m ON m.id=t.match_id
                 LEFT JOIN federated_queue_members fm ON fm.player=p.id AND fm.released=0
                 LEFT JOIN federated_queue f ON f.ticket=fm.ticket"
            );
            db.query_row(&sql, [], |row| {
                Ok(PlatformCounts {
                    online_players: row.get(0)?,
                    queued_players: row.get(1)?,
                })
            })
            .map_err(Into::into)
        })
        .await
    }

    pub async fn rename(&self, session_token: &str, name: String) -> Result<()> {
        let key = digest(session_token);
        let name = display_name(name)?;
        self.run(move |state| {
            state
                .accounts
                .lock()
                .map_err(|_| PlatformError::WorkerUnavailable)?
                .rename(&key, &name)
                .map_err(accounts::storage_error)?;
            state.changes.send_replace(());
            Ok(())
        })
        .await
    }

    pub async fn end_session(&self, session_token: &str) -> Result<()> {
        let key = digest(session_token);
        self.run(move |state| {
            let removed = state
                .accounts
                .lock()
                .map_err(|_| PlatformError::WorkerUnavailable)?
                .end_session(&key)
                .map_err(accounts::storage_error)?;
            if removed {
                state.changes.send_replace(());
            }
            Ok(())
        })
        .await
    }

    /// Trusted host-side listing. The caller must apply its administrative access policy.
    pub async fn players(&self, offset: u32, limit: u32) -> Result<PlayerPage> {
        if !(1..=100).contains(&limit) {
            return Err(PlatformError::Invalid(
                "player page size must be between 1 and 100",
            ));
        }
        self.run(move |state| {
            let db = state
                .db
                .lock()
                .map_err(|_| PlatformError::WorkerUnavailable)?;
            let total = db.query_row("SELECT count(*) FROM players", [], |row| row.get(0))?;
            let sql = format!(
                "SELECT {PROFILE_FIELDS} {PROFILE_JOINS} ORDER BY p.rowid LIMIT ? OFFSET ?"
            );
            let mut statement = db.prepare(&sql)?;
            let players = statement
                .query_map(params![limit, offset], read_profile)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(PlayerPage { total, players })
        })
        .await
    }
}

const ONLINE_SQL: &str = "s.player IS NOT NULL AND p.last_seen >= unixepoch() - 90";
const PROFILE_FIELDS: &str =
    "p.id, p.name, coalesce(t.mode, f.mode, sm.mode, ps.mode), coalesce(m.id, f.match_id, sm.id, ps.match_id),
     coalesce(m.phase, f.phase, CASE WHEN sm.id IS NOT NULL OR ps.match_id IS NOT NULL THEN 'spectating' END),
     coalesce(m.address, f.address, sp.address, ps.address), coalesce(m.result, f.result), coalesce(sp.spectator_id, ps.spectator_id), settlement.at_ms, coalesce(t.queued_at_ms,f.queued_at_ms)";
const PROFILE_JOINS: &str = "FROM players p LEFT JOIN sessions s ON s.player=p.id
     LEFT JOIN tickets t ON t.player=p.id LEFT JOIN matches m ON m.id=t.match_id
     LEFT JOIN federated_queue_members fm ON fm.player=p.id AND fm.released=0
     LEFT JOIN federated_queue f ON f.ticket=fm.ticket AND f.phase<>'closed'
     LEFT JOIN native_settlements settlement ON settlement.room=coalesce(m.id,f.match_id)
     LEFT JOIN spectator_sessions sp ON sp.player=p.id AND sp.spectator_id IS NOT NULL
     LEFT JOIN matches sm ON sm.id=sp.match_id AND sm.phase='playing'
     LEFT JOIN pooled_spectators ps ON ps.player=p.id AND ps.spectator_id IS NOT NULL";

fn read_activity(row: &rusqlite::Row<'_>) -> rusqlite::Result<PlayerActivity> {
    Ok(PlayerActivity {
        profile: read_profile(row)?,
        last_seen: row.get(10)?,
        online: row.get(11)?,
    })
}

fn read_profile(row: &rusqlite::Row<'_>) -> rusqlite::Result<PlayerProfile> {
    fn uuid_at(value: &str, column: usize) -> rusqlite::Result<Uuid> {
        Uuid::parse_str(value).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                column,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })
    }
    let lobby = match row.get::<_, Option<String>>(2)? {
        Some(mode) => Some(Lobby {
            mode,
            match_id: row
                .get::<_, Option<String>>(3)?
                .as_deref()
                .map(|id| uuid_at(id, 3))
                .transpose()?,
            phase: row
                .get::<_, Option<String>>(4)?
                .unwrap_or_else(|| LobbyPhase::Queued.as_str().into()),
            address: row.get(5)?,
            settled_at_ms: row.get(8)?,
            queued_at_ms: row.get(9)?,
            spectator_id: row
                .get::<_, Option<String>>(7)?
                .as_deref()
                .map(|id| uuid_at(id, 7))
                .transpose()?,
            result: row
                .get::<_, Option<String>>(6)?
                .as_deref()
                .map(serde_json::from_str)
                .transpose()
                .map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        6,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?,
        }),
        None => None,
    };
    Ok(PlayerProfile {
        player_id: uuid_at(&row.get::<_, String>(0)?, 0)?,
        name: row.get(1)?,
        lobby,
    })
}

fn event(tx: &Transaction<'_>, id: &str, body: EventBody<'_>) -> Result<()> {
    tx.execute(
        "INSERT INTO events(player, body) VALUES(?, ?)",
        params![id, body.encode()?],
    )?;
    Ok(())
}

fn announce(tx: &Transaction<'_>, match_id: &str, body: EventBody<'_>) -> Result<()> {
    tx.execute(
        "INSERT INTO events(player, body) SELECT player, ? FROM tickets WHERE match_id=? ORDER BY serial",
        params![body.encode()?, match_id],
    )?;
    Ok(())
}

#[derive(Deserialize, Serialize)]
pub struct Queue {
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub difficulty: Option<BotDifficulty>,
}

async fn queue(
    Extension(matches): Extension<Arc<dyn PublicMatches>>,
    headers: HeaderMap,
    Json(input): Json<Queue>,
) -> Result<StatusCode> {
    let session = bearer(&headers)?;
    matches
        .enqueue(session, input.mode, input.difficulty)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
impl AppState {
    pub async fn join(&self, session_token: &str, mode: String) -> Result<()> {
        let seats = self.seats_for_mode(&mode);
        self.join_with_seats(session_token, mode, seats).await
    }
}

fn abort(tx: &Transaction<'_>, match_id: &str, reason: AbortReason) -> Result<()> {
    queue::release_matches(tx, match_id)?;
    spectator::close_match(tx, match_id)?;
    tx.execute("UPDATE matches SET phase='aborted' WHERE id=?", [match_id])?;
    announce(
        tx,
        match_id,
        EventBody::MatchAborted {
            match_id: parse_id(match_id)?,
            reason,
        },
    )?;
    tx.execute("DELETE FROM tickets WHERE match_id=?", [match_id])?;
    Ok(())
}

async fn leave(
    Extension(matches): Extension<Arc<dyn PublicMatches>>,
    headers: HeaderMap,
) -> Result<StatusCode> {
    matches.cancel_queue(bearer(&headers)?).await?;
    Ok(StatusCode::NO_CONTENT)
}
fn acknowledge_local_result(
    tx: &rusqlite::Transaction<'_>,
    id: &str,
    match_id: &str,
) -> Result<()> {
    tx.execute("DELETE FROM tickets WHERE player=?", [id])?;
    event(
        tx,
        id,
        EventBody::PostMatchLeft {
            match_id: Some(parse_id(match_id)?),
        },
    )?;
    Ok(())
}

impl AppState {
    pub async fn leave(&self, session_token: &str) -> Result<()> {
        let key = digest(session_token);
        self.run(move |state| {
            state.transaction(|tx| {
                let id = player(tx, &key)?;
                let lobby = tx.query_row(
            "SELECT m.id, m.phase FROM tickets t JOIN matches m ON m.id=t.match_id WHERE player=?",
            [&id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        ).optional()?;
                if let Some((match_id, phase)) = lobby {
                    match LobbyPhase::parse(&phase) {
                        Some(LobbyPhase::Offered) => abort(tx, &match_id, AbortReason::PlayerLeft)?,
                        Some(LobbyPhase::Finished) => acknowledge_local_result(tx, &id, &match_id)?,
                        _ => {
                            return Err(PlatformError::Conflict(
                                "match is already starting or playing",
                            ));
                        }
                    }
                } else {
                    let batch: Option<String> = tx.query_row(
                        "SELECT batch FROM tickets WHERE player=?", [&id], |row| row.get(0),
                    ).optional()?.flatten();
                    if let Some(batch) = batch {
                        tx.execute(
                            "INSERT INTO events(player, body) SELECT player, ? FROM tickets WHERE batch=? ORDER BY serial",
                            params![EventBody::QueueLeft.encode()?, batch],
                        )?;
                        tx.execute("DELETE FROM tickets WHERE batch=?", [&batch])?;
                        halcyon_community::queue::release(tx, &batch).map_err(queue::community_error)?;
                    } else if tx.execute("DELETE FROM tickets WHERE player=?", [&id])? != 0 {
                        event(tx, &id, EventBody::QueueLeft)?;
                    }
                }
                Ok(())
            })
        })
        .await
    }
}

async fn accept(
    Extension(matches): Extension<Arc<dyn PublicMatches>>,
    headers: HeaderMap,
    Path(match_uuid): Path<Uuid>,
) -> Result<StatusCode> {
    matches.accept_offer(bearer(&headers)?, match_uuid).await?;
    Ok(StatusCode::NO_CONTENT)
}
impl AppState {
    pub async fn accept(&self, session_token: &str, match_uuid: Uuid) -> Result<()> {
        let key = digest(session_token);
        let match_id = match_uuid.to_string();
        self.expire_offers().await?;
        self.run(move |state| {
        let reservation = state.transaction(|tx| {
            let id = player(tx, &key)?;
            let (mode, phase, difficulty): (String, String, _) = tx.query_row(
                "SELECT m.mode, m.phase, m.difficulty FROM tickets t JOIN matches m ON m.id=t.match_id WHERE t.player=? AND m.id=? AND (m.phase<>'offered' OR m.offered_at > unixepoch() - 30)",
                params![id, match_id], |row| Ok((row.get(0)?, row.get(1)?, queue::read_difficulty(row, 2)?)),
            ).optional()?.ok_or(PlatformError::Conflict("no offer for this match"))?;
            match LobbyPhase::parse(&phase) {
                Some(LobbyPhase::Playing | LobbyPhase::Reserving) => return Ok(None),
                Some(LobbyPhase::Offered) => {},
                _ => return Err(PlatformError::Conflict("match is already starting")),
            }
            tx.execute("UPDATE tickets SET accepted=1 WHERE player=?", [&id])?;
            let (players, teams, confirmed) = queue::match_players(tx, &match_id, &mode)?;
            if !confirmed {
                return Ok(None);
            }
            let talent_levels = state.resolve_talent_levels(tx, &mode, &players)?;
            let stored = serde_json::to_string(&talent_levels).map_err(|_| PlatformError::WorkerUnavailable)?;
            tx.execute("UPDATE matches SET phase='reserving',talent_levels=? WHERE id=?", params![stored,match_id])?;
            Ok(Some(Reservation { match_id: match_uuid, players, teams, mode, difficulty, talent_levels }))
        })?;
        if let Some(reservation) = reservation {
            state.reserve_match(reservation)?;
        }
        Ok(())
    }).await
    }
}

#[derive(Deserialize, Serialize)]
pub struct Chat {
    pub text: String,
}

async fn chat(
    Extension(matches): Extension<Arc<dyn PublicMatches>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<Chat>,
) -> Result<StatusCode> {
    matches
        .match_chat(bearer(&headers)?, id, input.text)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
impl AppState {
    pub async fn send_chat(&self, session_token: &str, id: Uuid, message: String) -> Result<()> {
        let key = digest(session_token);
        let message = message.trim().to_owned();
        if message.is_empty() || message.len() > 512 || message.chars().any(char::is_control) {
            return Err(PlatformError::Invalid("invalid message"));
        }
        self.run(move |state| {
            state.transaction(|tx| {
                let player_id = player(tx, &key)?;
                let match_id = id.to_string();
                if !tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM tickets WHERE player=? AND match_id=?)",
                    params![player_id, match_id],
                    |row| row.get::<_, bool>(0),
                )? {
                    return Err(PlatformError::Conflict("no active seat in this match"));
                }
                announce(
                    tx,
                    &match_id,
                    EventBody::Chat {
                        match_id: id,
                        player_id: parse_id(&player_id)?,
                        text: &message,
                    },
                )?;
                Ok(())
            })
        })
        .await
    }
}

#[derive(Clone, Copy, Deserialize)]
struct Cursor {
    #[serde(default)]
    after: i64,
}

#[derive(Serialize)]
pub struct Event {
    pub cursor: i64,
    pub body: Value,
}

#[derive(Serialize)]
pub struct EventBatch {
    pub cursor: i64,
    pub events: Vec<Event>,
}

impl AppState {
    pub async fn events(&self, session_token: &str, after: i64, wait: bool) -> Result<EventBatch> {
        if after < 0 {
            return Err(PlatformError::Invalid("invalid event cursor"));
        }
        let key = digest(session_token);
        if wait {
            self.wait_events(&key, after).await
        } else {
            self.read_events(key, after).await
        }
    }

    async fn read_events(&self, key: Vec<u8>, after: i64) -> Result<EventBatch> {
        self.run(move |state| {
            let db = state.db.lock().map_err(|_| PlatformError::WorkerUnavailable)?;
            let id = player(&db, &key)?;
            let mut query = db.prepare(
                "SELECT cursor, body FROM events WHERE player=? AND cursor>? ORDER BY cursor LIMIT 128",
            )?;
            let records = query
                .query_map(params![id, after], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let events = records
                .into_iter()
                .map(|(cursor, body)| {
                    Ok(Event {
                        cursor,
                        body: serde_json::from_str(&body).map_err(|_| PlatformError::WorkerUnavailable)?,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let cursor = events.last().map_or(after, |entry| entry.cursor);
            Ok(EventBatch { cursor, events })
        }).await
    }

    async fn wait_events(&self, key: &[u8], after: i64) -> Result<EventBatch> {
        let mut changes = self.changes.subscribe();
        let until = tokio::time::Instant::now() + Duration::from_secs(25);
        self.expire_offers().await?;
        loop {
            let batch = self.read_events(key.to_vec(), after).await?;
            if !batch.events.is_empty() {
                return Ok(batch);
            }
            if tokio::time::timeout_at(until, changes.changed())
                .await
                .is_err()
            {
                self.expire_offers().await?;
                return self.read_events(key.to_vec(), after).await;
            }
        }
    }
}

async fn poll(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(cursor): Query<Cursor>,
) -> Result<Json<EventBatch>> {
    if cursor.after < 0 {
        return Err(PlatformError::Invalid("invalid event cursor"));
    }
    let key = digest(bearer(&headers)?);
    state.wait_events(&key, cursor.after).await.map(Json)
}

async fn websocket(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(cursor): Query<Cursor>,
    ws: WebSocketUpgrade,
) -> Result<Response> {
    if cursor.after < 0 {
        return Err(PlatformError::Invalid("invalid event cursor"));
    }
    let key = digest(bearer(&headers)?);
    state.read_events(key.clone(), cursor.after).await?;
    Ok(ws
        .max_message_size(1024)
        .on_upgrade(move |socket| stream(state, key, cursor.after, socket)))
}

async fn stream(state: AppState, key: Vec<u8>, mut after: i64, mut socket: WebSocket) {
    loop {
        let update = state.wait_events(&key, after);
        tokio::pin!(update);
        let batch = loop {
            tokio::select! {
                result = &mut update => match result {
                    Ok(batch) => break batch,
                    Err(_) => {
                        let _ = socket.send(Message::Close(None)).await;
                        return;
                    },
                },
                input = socket.recv() => match input {
                    Some(Ok(Message::Pong(_) | Message::Ping(_))) => {},
                    _ => return,
                }
            }
        };
        after = batch.cursor;
        let message = if batch.events.is_empty() {
            Message::Ping(Vec::new().into())
        } else if let Ok(body) = serde_json::to_string(&batch) {
            Message::Text(body.into())
        } else {
            return;
        };
        if !matches!(
            tokio::time::timeout(Duration::from_secs(10), socket.send(message)).await,
            Ok(Ok(()))
        ) {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_error_maps_losslessly_across_platform_boundary() {
        assert!(matches!(
            PlatformError::from(HostError::Unauthorized),
            PlatformError::Unauthorized
        ));
        assert!(matches!(
            PlatformError::from(HostError::Invalid("bad")),
            PlatformError::Invalid("bad")
        ));
        assert!(matches!(
            PlatformError::from(HostError::Conflict("busy")),
            PlatformError::Conflict("busy")
        ));
        assert!(matches!(
            PlatformError::from(HostError::HostUnavailable),
            PlatformError::HostUnavailable
        ));
        assert!(matches!(
            PlatformError::from(HostError::WorkerUnavailable),
            PlatformError::WorkerUnavailable
        ));
        assert!(matches!(
            PlatformError::from(HostError::Database("locked".into())),
            PlatformError::WorkerUnavailable
        ));
        assert_eq!(
            HostError::from(PlatformError::Conflict("taken")),
            HostError::Conflict("taken")
        );
    }
}
