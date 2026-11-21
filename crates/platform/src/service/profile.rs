use diesel::{
    prelude::*,
    sql_types::{BigInt, Bool, Nullable, Text},
};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use super::{AppState, PlatformError, Result, credential, digest, display_name, parse_id};
use crate::{LobbyPhase, events::accounts};

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

#[derive(QueryableByName)]
struct ProfileRow {
    #[diesel(sql_type = Text)]
    id: String,
    #[diesel(sql_type = Text)]
    name: String,
    #[diesel(sql_type = Nullable<Text>)]
    mode: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    match_id: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    phase: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    address: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    result: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    spectator_id: Option<String>,
    #[diesel(sql_type = Nullable<BigInt>)]
    settled_at_ms: Option<i64>,
    #[diesel(sql_type = Nullable<BigInt>)]
    queued_at_ms: Option<i64>,
}

#[derive(QueryableByName)]
struct ActivityRow {
    #[diesel(sql_type = Text)]
    id: String,
    #[diesel(sql_type = Text)]
    name: String,
    #[diesel(sql_type = Nullable<Text>)]
    mode: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    match_id: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    phase: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    address: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    result: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    spectator_id: Option<String>,
    #[diesel(sql_type = Nullable<BigInt>)]
    settled_at_ms: Option<i64>,
    #[diesel(sql_type = Nullable<BigInt>)]
    queued_at_ms: Option<i64>,
    #[diesel(sql_type = BigInt)]
    last_seen: i64,
    #[diesel(sql_type = Bool)]
    online: bool,
}

#[derive(QueryableByName)]
struct TotalRow {
    #[diesel(sql_type = BigInt)]
    total: i64,
}

#[derive(QueryableByName)]
struct CountsRow {
    #[diesel(sql_type = BigInt)]
    online_players: i64,
    #[diesel(sql_type = BigInt)]
    queued_players: i64,
}

const ONLINE_SQL: &str = "s.player IS NOT NULL AND p.last_seen >= unixepoch() - 90";
const PROFILE_FIELDS: &str = "p.id AS id, p.name AS name,
    coalesce(t.mode, f.mode, sm.mode, ps.mode) AS mode,
    coalesce(m.id, f.match_id, sm.id, ps.match_id) AS match_id,
    coalesce(m.phase, f.phase, CASE WHEN sm.id IS NOT NULL OR ps.match_id IS NOT NULL THEN 'spectating' END) AS phase,
    coalesce(m.address, f.address, sp.address, ps.address) AS address,
    coalesce(m.result, f.result) AS result,
    coalesce(sp.spectator_id, ps.spectator_id) AS spectator_id,
    settlement.at_ms AS settled_at_ms,
    coalesce(t.queued_at_ms, f.queued_at_ms) AS queued_at_ms";
const PROFILE_JOINS: &str = "FROM players p LEFT JOIN sessions s ON s.player = p.id
    LEFT JOIN tickets t ON t.player = p.id LEFT JOIN matches m ON m.id = t.match_id
    LEFT JOIN federated_queue_members fm ON fm.player = p.id AND fm.released = 0
    LEFT JOIN federated_queue f ON f.ticket = fm.ticket AND f.phase <> 'closed'
    LEFT JOIN native_settlements settlement ON settlement.room = coalesce(m.id, f.match_id)
    LEFT JOIN spectator_sessions sp ON sp.player = p.id AND sp.spectator_id IS NOT NULL
    LEFT JOIN matches sm ON sm.id = sp.match_id AND sm.phase = 'playing'
    LEFT JOIN pooled_spectators ps ON ps.player = p.id AND ps.spectator_id IS NOT NULL";

impl ProfileRow {
    fn into_profile(self) -> Result<PlayerProfile> {
        let lobby = match self.mode {
            Some(mode) => Some(Lobby {
                mode,
                match_id: self.match_id.as_deref().map(parse_id).transpose()?,
                phase: self
                    .phase
                    .unwrap_or_else(|| LobbyPhase::Queued.as_str().into()),
                address: self.address,
                settled_at_ms: self.settled_at_ms,
                queued_at_ms: self.queued_at_ms,
                spectator_id: self.spectator_id.as_deref().map(parse_id).transpose()?,
                result: self
                    .result
                    .as_deref()
                    .map(serde_json::from_str)
                    .transpose()
                    .map_err(|_| PlatformError::WorkerUnavailable)?,
            }),
            None => None,
        };
        Ok(PlayerProfile {
            player_id: parse_id(&self.id)?,
            name: self.name,
            lobby,
        })
    }
}

impl ActivityRow {
    fn into_activity(self) -> Result<PlayerActivity> {
        let profile = ProfileRow {
            id: self.id,
            name: self.name,
            mode: self.mode,
            match_id: self.match_id,
            phase: self.phase,
            address: self.address,
            result: self.result,
            spectator_id: self.spectator_id,
            settled_at_ms: self.settled_at_ms,
            queued_at_ms: self.queued_at_ms,
        }
        .into_profile()?;
        Ok(PlayerActivity {
            profile,
            last_seen: self.last_seen,
            online: self.online,
        })
    }
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

    pub async fn profile(&self, session_token: &str) -> Result<PlayerProfile> {
        let key = digest(session_token);
        self.run(move |state| {
            state.read_and_touch_as(&key, |db, id| {
                let sql = format!("SELECT {PROFILE_FIELDS} {PROFILE_JOINS} WHERE p.id = ?1");
                diesel::sql_query(sql)
                    .bind::<Text, _>(id)
                    .get_result::<ProfileRow>(db)?
                    .into_profile()
            })
        })
        .await
    }

    pub async fn player_activity(&self, id: Uuid) -> Result<Option<PlayerActivity>> {
        self.run(move |state| {
            state.read(|db| {
                let sql = format!(
                    "SELECT {PROFILE_FIELDS}, p.last_seen AS last_seen, ({ONLINE_SQL}) AS online
                     {PROFILE_JOINS} WHERE p.id = ?1"
                );
                diesel::sql_query(sql)
                    .bind::<Text, _>(id.to_string())
                    .get_result::<ActivityRow>(db)
                    .optional()?
                    .map(ActivityRow::into_activity)
                    .transpose()
            })
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
            state.read(|db| {
                let sql = format!(
                    "SELECT {PROFILE_FIELDS}, p.last_seen AS last_seen, ({ONLINE_SQL}) AS online
                     {PROFILE_JOINS}
                     WHERE p.id > ?1 AND (?2 IS NULL OR ({ONLINE_SQL}) = ?2)
                     ORDER BY p.id LIMIT ?3"
                );
                let after = after.map_or_else(String::new, |id| id.to_string());
                diesel::sql_query(sql)
                    .bind::<Text, _>(after)
                    .bind::<Nullable<Bool>, _>(online)
                    .bind::<BigInt, _>(limit as i64)
                    .load::<ActivityRow>(db)?
                    .into_iter()
                    .map(ActivityRow::into_activity)
                    .collect()
            })
        })
        .await
    }

    pub async fn counts(&self) -> Result<PlatformCounts> {
        self.expire_offers().await?;
        self.run(|state| {
            state.read(|db| {
                let sql = format!(
                    "SELECT count(*) FILTER (WHERE {ONLINE_SQL}) AS online_players,
                            count(*) FILTER (WHERE (t.player IS NOT NULL AND
                              (t.match_id IS NULL OR m.phase IN ('offered', 'reserving')))
                              OR f.phase IN ('pending', 'queued', 'offered')) AS queued_players
                     FROM players p LEFT JOIN sessions s ON s.player = p.id
                     LEFT JOIN tickets t ON t.player = p.id LEFT JOIN matches m ON m.id = t.match_id
                     LEFT JOIN federated_queue_members fm ON fm.player = p.id AND fm.released = 0
                     LEFT JOIN federated_queue f ON f.ticket = fm.ticket"
                );
                let counts = diesel::sql_query(sql).get_result::<CountsRow>(db)?;
                Ok(PlatformCounts {
                    online_players: counts.online_players as u64,
                    queued_players: counts.queued_players as u64,
                })
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
            state.read(|db| {
                let total = diesel::sql_query("SELECT count(*) AS total FROM players")
                    .get_result::<TotalRow>(db)?
                    .total as u64;
                let sql = format!(
                    "SELECT {PROFILE_FIELDS} {PROFILE_JOINS} ORDER BY p.rowid LIMIT ?1 OFFSET ?2"
                );
                let players = diesel::sql_query(sql)
                    .bind::<BigInt, _>(i64::from(limit))
                    .bind::<BigInt, _>(i64::from(offset))
                    .load::<ProfileRow>(db)?
                    .into_iter()
                    .map(ProfileRow::into_profile)
                    .collect::<Result<Vec<_>>>()?;
                Ok(PlayerPage { total, players })
            })
        })
        .await
    }
}
