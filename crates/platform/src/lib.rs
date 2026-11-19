pub use halcyon_data::{BotDifficulty, LEGACY_ARAL_QUEUE, LEGACY_ARAL_SEATS};
mod accounts;
mod chat;
mod federation;
mod native_offer;
mod notification;
pub use native_offer::{NativeOffer, NativeSeat};
pub use notification::record_external_notification;
mod pooled_spectator;
pub use pooled_spectator::PooledSpectator;
mod queue;
mod recovery;
mod spectator;
pub use federation::FederatedIntent;
pub use queue::{plan_teams, plan_teams_for_mode};
pub use spectator::HostedSpectator;

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
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::watch;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct PlatformConfig {
    pub database_path: PathBuf,
    pub public_address: String,
    pub seats_per_match: usize,
}

#[derive(Debug, Clone)]
pub struct Reservation {
    pub match_id: Uuid,
    pub players: Vec<Uuid>,
    /// Parallel human team indices (0 or 1). Empty retains the default room layout.
    pub teams: Vec<u8>,
    pub mode: String,
    pub difficulty: Option<BotDifficulty>,
}

#[derive(Debug, Clone)]
pub struct HostedMatch {
    pub address: String,
}

/// Implementations must make repeated reservations for the same match id idempotent.
pub trait MatchHost: Send + Sync + 'static {
    fn reserve(&self, reservation: Reservation) -> Result<HostedMatch>;

    /// Issue or refresh an isolated read-only credential for this viewer.
    fn spectate(&self, _match_id: Uuid, _viewer: Uuid) -> Result<HostedSpectator> {
        Err(PlatformError::HostUnavailable)
    }

    /// Revoke the credential and close its connections. Repeated calls are harmless.
    fn stop_spectating(&self, _match_id: Uuid, _spectator_id: Uuid) -> Result<()> {
        Err(PlatformError::HostUnavailable)
    }
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
        (status, Json(json!({ "error": message }))).into_response()
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
    host: Arc<dyn MatchHost>,
    changes: watch::Sender<()>,
    config: Arc<PlatformConfig>,
}

impl AppState {
    pub fn seats_per_match(&self) -> usize {
        self.config.seats_per_match
    }

    pub fn seats_for_mode(&self, mode: &str) -> usize {
        match mode {
            "5v5_bots_solo" | "solo" => 1,
            LEGACY_ARAL_QUEUE => LEGACY_ARAL_SEATS,
            _ => self.seats_per_match(),
        }
    }

    pub fn open(config: PlatformConfig, host: Arc<dyn MatchHost>) -> Result<Self> {
        if !(1..=64).contains(&config.seats_per_match) {
            return Err(PlatformError::Invalid(
                "seats_per_match must be between 1 and 64",
            ));
        }
        let mut db = Connection::open(&config.database_path)?;
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
            tx.commit()?;
        }
        accounts::initialize(&db)?;
        let (changes, _) = watch::channel(());
        let state = Self {
            db: Arc::new(Mutex::new(db)),
            host,
            changes,
            config: Arc::new(config),
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
            let mut matches =
                tx.prepare("SELECT id, mode, difficulty FROM matches WHERE phase='reserving'")?;
            let rows = matches
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        queue::read_difficulty(row, 2)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows.into_iter()
                .map(|(match_id, mode, difficulty)| {
                    let (players, teams, _) = queue::match_players(tx, &match_id, &mode)?;
                    Ok(Reservation {
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
                self.transaction(|tx| abort(tx, &match_id, "host_unavailable"))?;
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
                json!({"type":"match_playing", "matchId":match_id, "address":host.address}),
            )
        })
    }

    /// Ends an active match after the trusted host confirms it can no longer run.
    pub fn abort_match(&self, match_id: Uuid) -> Result<()> {
        let match_id = match_id.to_string();
        self.transaction(|tx| {
            let phase: Option<String> = tx
                .query_row("SELECT phase FROM matches WHERE id=?", [&match_id], |row| {
                    row.get(0)
                })
                .optional()?;
            match phase.as_deref() {
                Some("playing") => abort(tx, &match_id, "host_stopped"),
                Some("aborted") => Ok(()),
                _ => Err(PlatformError::Conflict("match is not playing")),
            }
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
                    abort(tx, &id, "offer_expired")?;
                }
                Ok(())
            })
        })
        .await
    }

    /// Record a terminal result once. Each participant acknowledges it with leave.
    pub fn finish_match(&self, match_id: Uuid, result: Value) -> Result<()> {
        let match_id = match_id.to_string();
        self.transaction(|tx| {
            let row = tx
                .query_row(
                    "SELECT phase, result FROM matches WHERE id=?",
                    [&match_id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
                )
                .optional()?;
            match row {
                Some((phase, stored)) if phase == "finished" => {
                    if stored.as_deref() == Some(result.to_string().as_str()) {
                        return Ok(());
                    }
                    return Err(PlatformError::Conflict("match has a different result"));
                }
                Some((phase, _)) if phase == "playing" => {}
                _ => return Err(PlatformError::Conflict("match is not playing")),
            }
            tx.execute(
                "UPDATE matches SET phase='finished', result=? WHERE id=?",
                params![result.to_string(), match_id],
            )?;
            queue::release_matches(tx, &match_id)?;
            spectator::close_match(tx, &match_id)?;
            announce(
                tx,
                &match_id,
                json!({"type":"match_finished", "matchId":match_id, "result":result}),
            )?;
            Ok(())
        })
    }
}

pub fn router(state: AppState) -> Router {
    router_with_matches(state.clone(), Arc::new(state))
}

/// Match operations selected by the home service. The public API authenticates
/// with the same opaque bearer token regardless of the configured authority.
#[async_trait::async_trait]
pub trait PublicMatches: Send + Sync {
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
        .route("/api/v1/events", get(poll))
        .route("/api/v1/events/ws", get(websocket))
        .layer(Extension(matches))
        .with_state(state)
}

#[async_trait::async_trait]
impl PublicMatches for AppState {
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

fn player(db: &Connection, key: &[u8]) -> Result<String> {
    let id: String = db
        .query_row("SELECT player FROM sessions WHERE key=?", [key], |row| {
            row.get(0)
        })
        .optional()?
        .ok_or(PlatformError::Unauthorized)?;
    db.execute(
        "UPDATE players SET last_seen=unixepoch() WHERE id=? AND last_seen < unixepoch() - 15",
        [&id],
    )?;
    Ok(id)
}

fn issue_session(tx: &Transaction<'_>, id: &str) -> Result<String> {
    let token = credential();
    tx.execute(
        "INSERT INTO sessions(key, player) VALUES(?, ?) ON CONFLICT(player) DO UPDATE SET key=excluded.key",
        params![digest(&token), id],
    )?;
    tx.execute("UPDATE players SET last_seen=unixepoch() WHERE id=?", [id])?;
    Ok(token)
}

#[derive(Deserialize)]
struct CreateGuest {
    name: String,
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
            state.transaction(|tx| {
                let id = Uuid::new_v4().to_string();
                let recovery = credential();
                tx.execute(
                    "INSERT INTO players(id, name, recovery) VALUES(?, ?, ?)",
                    params![id, name, digest(&recovery)],
                )?;
                let token = issue_session(tx, &id)?;
                Ok(Guest {
                    player_id: parse_id(&id)?,
                    recovery_token: Some(recovery),
                    session_token: token,
                })
            })
        })
        .await
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResumeGuest {
    recovery_token: String,
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
            state.transaction(|tx| {
                let id: String = tx
                    .query_row(
                        "SELECT id FROM players WHERE recovery=?",
                        [digest(&recovery_token)],
                        |row| row.get(0),
                    )
                    .optional()?
                    .ok_or(PlatformError::Unauthorized)?;
                let token = issue_session(tx, &id)?;
                Ok(Guest {
                    player_id: parse_id(&id)?,
                    recovery_token: None,
                    session_token: token,
                })
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
            state.transaction(|tx| {
                let id = player(tx, &key)?;
                tx.execute("UPDATE players SET name=? WHERE id=?", params![name, id])?;
                Ok(())
            })
        })
        .await
    }

    pub async fn end_session(&self, session_token: &str) -> Result<()> {
        let key = digest(session_token);
        self.run(move |state| {
            state.transaction(|tx| {
                tx.execute("DELETE FROM sessions WHERE key=?", [&key])?;
                Ok(())
            })
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
     coalesce(m.address, f.address, sp.address, ps.address), coalesce(m.result, f.result), coalesce(sp.spectator_id, ps.spectator_id)";
const PROFILE_JOINS: &str = "FROM players p LEFT JOIN sessions s ON s.player=p.id
     LEFT JOIN tickets t ON t.player=p.id LEFT JOIN matches m ON m.id=t.match_id
     LEFT JOIN federated_queue_members fm ON fm.player=p.id AND fm.released=0
     LEFT JOIN federated_queue f ON f.ticket=fm.ticket AND f.phase<>'closed'
     LEFT JOIN spectator_sessions sp ON sp.player=p.id AND sp.spectator_id IS NOT NULL
     LEFT JOIN matches sm ON sm.id=sp.match_id AND sm.phase='playing'
     LEFT JOIN pooled_spectators ps ON ps.player=p.id AND ps.spectator_id IS NOT NULL";

fn read_activity(row: &rusqlite::Row<'_>) -> rusqlite::Result<PlayerActivity> {
    Ok(PlayerActivity {
        profile: read_profile(row)?,
        last_seen: row.get(8)?,
        online: row.get(9)?,
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
                .unwrap_or_else(|| "queued".into()),
            address: row.get(5)?,
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

fn event(tx: &Transaction<'_>, id: &str, body: Value) -> Result<()> {
    tx.execute(
        "INSERT INTO events(player, body) VALUES(?, ?)",
        params![id, body.to_string()],
    )?;
    Ok(())
}

fn announce(tx: &Transaction<'_>, match_id: &str, body: Value) -> Result<()> {
    tx.execute(
        "INSERT INTO events(player, body) SELECT player, ? FROM tickets WHERE match_id=? ORDER BY serial",
        params![body.to_string(), match_id],
    )?;
    Ok(())
}

#[derive(Deserialize)]
struct Queue {
    mode: String,
    difficulty: Option<BotDifficulty>,
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

fn abort(tx: &Transaction<'_>, match_id: &str, reason: &'static str) -> Result<()> {
    queue::release_matches(tx, match_id)?;
    spectator::close_match(tx, match_id)?;
    tx.execute("UPDATE matches SET phase='aborted' WHERE id=?", [match_id])?;
    announce(
        tx,
        match_id,
        json!({"type":"match_aborted", "matchId":match_id, "reason":reason}),
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
                    match phase.as_str() {
                        "offered" => abort(tx, &match_id, "player_left")?,
                        "finished" => {
                            tx.execute("DELETE FROM tickets WHERE player=?", [&id])?;
                            event(
                                tx,
                                &id,
                                json!({"type":"post_match_left", "matchId":match_id}),
                            )?;
                        }
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
                            params![json!({"type":"queue_left"}).to_string(), batch],
                        )?;
                        tx.execute("DELETE FROM tickets WHERE batch=?", [&batch])?;
                        halcyon_community::queue::release(tx, &batch).map_err(queue::community_error)?;
                    } else if tx.execute("DELETE FROM tickets WHERE player=?", [&id])? != 0 {
                        event(tx, &id, json!({"type":"queue_left"}))?;
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
            match phase.as_str() {
                "playing" | "reserving" => return Ok(None),
                "offered" => {},
                _ => return Err(PlatformError::Conflict("match is already starting")),
            }
            tx.execute("UPDATE tickets SET accepted=1 WHERE player=?", [&id])?;
            let (players, teams, confirmed) = queue::match_players(tx, &match_id, &mode)?;
            if !confirmed {
                return Ok(None);
            }
            tx.execute("UPDATE matches SET phase='reserving' WHERE id=?", [&match_id])?;
            Ok(Some(Reservation { match_id: match_uuid, players, teams, mode, difficulty }))
        })?;
        if let Some(reservation) = reservation {
            state.reserve_match(reservation)?;
        }
        Ok(())
    }).await
    }
}

#[derive(Deserialize)]
struct Chat {
    text: String,
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
                    json!({"type":"chat", "matchId":id, "playerId":player_id, "text":message}),
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
