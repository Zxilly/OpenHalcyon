//! Durable, player-scoped preferences and platform telemetry.
use super::{Error, now};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde_json::Value;
use std::{
    path::Path,
    sync::{Arc, Mutex},
};
use uuid::Uuid;

#[derive(Clone)]
pub(crate) struct Meta(Arc<Mutex<Connection>>);
impl Meta {
    pub fn open(path: &Path) -> Result<Self, Error> {
        let mut db = Connection::open(path)?;
        db.execute_batch("PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS ce_preferences(player TEXT NOT NULL, category TEXT NOT NULL, data TEXT NOT NULL,
                PRIMARY KEY(player,category));
            CREATE TABLE IF NOT EXISTS ce_metrics(player TEXT NOT NULL,match_id TEXT NOT NULL,data TEXT NOT NULL,
                PRIMARY KEY(player,match_id));")?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        super::activity::initialize(&tx)?;
        tx.commit()?;
        Ok(Self(Arc::new(Mutex::new(db))))
    }

    pub(crate) async fn transaction<R: Send + 'static>(
        &self,
        work: impl FnOnce(&Transaction<'_>) -> Result<R, Error> + Send + 'static,
    ) -> Result<R, Error> {
        self.with_behavior(rusqlite::TransactionBehavior::Deferred, work)
            .await
    }
    pub(crate) async fn with_behavior<R: Send + 'static>(
        &self,
        behavior: rusqlite::TransactionBehavior,
        work: impl FnOnce(&Transaction<'_>) -> Result<R, Error> + Send + 'static,
    ) -> Result<R, Error> {
        let db = self.0.clone();
        tokio::task::spawn_blocking(move || {
            let mut db = db.lock().map_err(|_| Error::Internal)?;
            let tx = db.transaction_with_behavior(behavior)?;
            let result = work(&tx)?;
            tx.commit()?;
            Ok(result)
        })
        .await
        .map_err(|_| Error::Internal)?
    }
    pub async fn queued_party_size(&self, id: Uuid) -> Result<u32, Error> {
        self.transaction(move |tx| {
            tx.query_row(
                "SELECT max(1, count(*)) FROM community_members WHERE group_id IN (
                     SELECT m.group_id FROM community_members m JOIN community_groups g
                     ON g.id=m.group_id WHERE m.player=? AND g.kind='party'
                 )",
                [id.to_string()],
                |row| row.get(0),
            )
            .map_err(Into::into)
        })
        .await
    }

    pub async fn prefs(&self, id: Uuid, category: Preference) -> Result<Value, Error> {
        self.transaction(move |tx| {
            let text: Option<String> = tx
                .query_row(
                    "SELECT data FROM ce_preferences WHERE player=? AND category=?",
                    params![id.to_string(), category.as_str()],
                    |r| r.get(0),
                )
                .optional()?;
            text.map(|v| serde_json::from_str(&v))
                .transpose()
                .map(|v| v.unwrap_or(Value::Null))
                .map_err(Into::into)
        })
        .await
    }
    pub async fn set(&self, id: Uuid, category: Preference, value: Value) -> Result<(), Error> {
        self.transaction(move|tx|{
            tx.execute("INSERT INTO ce_preferences VALUES(?,?,?) ON CONFLICT(player,category) DO UPDATE SET data=excluded.data",
                params![id.to_string(),category.as_str(),value.to_string()])?;Ok(())
        }).await
    }
    pub async fn metrics(&self, id: Uuid, room: Uuid, fields: Vec<Value>) -> Result<(), Error> {
        self.transaction(move|tx|{
            tx.execute("INSERT INTO ce_metrics VALUES(?,?,?) ON CONFLICT(player,match_id) DO UPDATE SET data=excluded.data",
                params![id.to_string(),room.to_string(),Value::Array(fields).to_string()])?;Ok(())
        }).await
    }
    pub async fn ranking(&self) -> Result<Value, Error> {
        self.transaction(|tx|{
            let mut q=tx.prepare("SELECT p.id,p.name,c.completed,c.wins FROM ce_player_matches c JOIN players p ON p.id=c.player
                ORDER BY c.completed DESC,c.player LIMIT 100")?;
            let entries=q.query_map([],|r|Ok(RankingEntry { player_uuid: r.get(0)?, handle: r.get(1)?, completed: r.get(2)?, wins: r.get(3)? }))?.collect::<Result<Vec<_>,_>>()?;
            Ok(serde_json::to_value(Ranking { leaderboards: [Leaderboard { leaderboard_id: "completed", leaderboard_title: "Completed matches", entries }], events: [], time: now() })?)
        }).await
    }

    pub async fn hero_stats(&self, id: Uuid) -> Result<Value, Error> {
        self.transaction(move |tx| {
            let mut query = tx.prepare(
                "SELECT hero,completed,wins FROM ce_hero_matches WHERE player=? ORDER BY hero",
            )?;
            let rows = query.query_map([id.to_string()], |row| {
                Ok(HeroStats {
                    hero: row.get(0)?,
                    completed: row.get(1)?,
                    wins: row.get(2)?,
                })
            })?;
            Ok(serde_json::to_value(
                rows.collect::<rusqlite::Result<Vec<_>>>()?,
            )?)
        })
        .await
    }
}

#[derive(serde::Serialize)]
struct Ranking {
    leaderboards: [Leaderboard; 1],
    events: [(); 0],
    time: u64,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Leaderboard {
    leaderboard_id: &'static str,
    leaderboard_title: &'static str,
    entries: Vec<RankingEntry>,
}

#[derive(serde::Serialize)]
struct RankingEntry {
    #[serde(rename = "playerUUID")]
    player_uuid: String,
    handle: String,
    completed: u64,
    wins: u64,
}

#[derive(serde::Serialize)]
struct HeroStats {
    hero: String,
    completed: u64,
    wins: u64,
}

#[derive(Clone, Copy)]
pub(crate) enum Preference {
    LastLogin,
    PlatformConfig,
    Tutorial,
    RecentAction,
    DeviceToken,
    ReportedResult,
    ReportedMatch,
}

impl Preference {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LastLogin => "last_login",
            Self::PlatformConfig => "platform_config",
            Self::Tutorial => "tutorial",
            Self::RecentAction => "recent_action",
            Self::DeviceToken => "device_token",
            Self::ReportedResult => "reported_result",
            Self::ReportedMatch => "reported_match",
        }
    }
}
