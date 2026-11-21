//! Durable, player-scoped preferences, telemetry, and CE match-statistics observer hooks.
use crate::{Error, now};
use diesel::{Connection, prelude::*, sqlite::SqliteConnection, upsert::excluded};
use halcyon_platform::{AppState, MatchEventObserver};
use halcyon_storage::schema::{
    ce_hero_matches, ce_metrics, ce_player_matches, ce_preferences, community_groups,
    community_members, players,
};
use serde_json::Value;
use std::{
    path::Path,
    sync::{Arc, Mutex},
};
use uuid::Uuid;

struct CeMatchObserver;

impl MatchEventObserver for CeMatchObserver {
    fn on_queued(
        &self,
        db: &mut SqliteConnection,
        players: &[String],
        mode: &str,
    ) -> halcyon_platform::Result<()> {
        halcyon_storage::ce_stats::record_queued(db, players, mode)?;
        Ok(())
    }

    fn on_match_offered(
        &self,
        db: &mut SqliteConnection,
        players: &[String],
    ) -> halcyon_platform::Result<()> {
        halcyon_storage::ce_stats::record_match_offered(db, players)?;
        Ok(())
    }

    fn on_match_finished(
        &self,
        db: &mut SqliteConnection,
        match_id: Uuid,
        mode: Option<&str>,
        players: &[String],
        result: Option<&Value>,
    ) -> halcyon_platform::Result<()> {
        halcyon_storage::ce_stats::record_match_finished(
            db,
            &match_id.to_string(),
            mode,
            players,
            result,
        )?;
        Ok(())
    }
}

pub(crate) fn install_match_stats_hook(app: &AppState) -> Result<(), Error> {
    app.set_event_observer(Arc::new(CeMatchObserver))?;
    Ok(())
}

#[derive(Clone)]
pub(crate) struct Meta(Arc<Mutex<SqliteConnection>>);

impl Meta {
    pub fn open(path: &Path) -> Result<Self, Error> {
        let address = halcyon_storage::DatabaseAddress::new(path);
        let mut db = address.connect()?;
        halcyon_storage::ensure_schema(&mut db)?;
        Ok(Self(Arc::new(Mutex::new(db))))
    }

    pub(crate) async fn read<R: Send + 'static>(
        &self,
        work: impl FnOnce(&mut SqliteConnection) -> Result<R, Error> + Send + 'static,
    ) -> Result<R, Error> {
        let db = self.0.clone();
        tokio::task::spawn_blocking(move || {
            let mut db = db.lock().map_err(|_| Error::Internal)?;
            db.transaction(work)
        })
        .await
        .map_err(|_| Error::Internal)?
    }

    pub(crate) async fn write<R: Send + 'static>(
        &self,
        work: impl FnOnce(&mut SqliteConnection) -> Result<R, Error> + Send + 'static,
    ) -> Result<R, Error> {
        let db = self.0.clone();
        tokio::task::spawn_blocking(move || {
            let mut db = db.lock().map_err(|_| Error::Internal)?;
            db.immediate_transaction(work)
        })
        .await
        .map_err(|_| Error::Internal)?
    }

    pub async fn queued_party_size(&self, id: Uuid) -> Result<u32, Error> {
        self.read(move |db| {
            let group = community_members::table
                .inner_join(community_groups::table)
                .filter(community_members::player.eq(id.to_string()))
                .filter(community_groups::kind.eq("party"))
                .select(community_members::group_id)
                .first::<String>(db)
                .optional()?;
            let count = match group {
                Some(group_id) => community_members::table
                    .filter(community_members::group_id.eq(group_id))
                    .count()
                    .get_result::<i64>(db)?,
                None => 0,
            };
            Ok(count.max(1) as u32)
        })
        .await
    }

    pub async fn prefs(&self, id: Uuid, category: Preference) -> Result<Value, Error> {
        self.read(move |db| {
            let text = ce_preferences::table
                .find((id.to_string(), category.as_str()))
                .select(ce_preferences::data)
                .first::<String>(db)
                .optional()?;
            text.map(|v| serde_json::from_str(&v))
                .transpose()
                .map(|v| v.unwrap_or(Value::Null))
                .map_err(Into::into)
        })
        .await
    }

    pub async fn set(&self, id: Uuid, category: Preference, value: Value) -> Result<(), Error> {
        self.write(move |db| {
            let player = id.to_string();
            let category = category.as_str();
            let encoded = value.to_string();
            diesel::insert_into(ce_preferences::table)
                .values((
                    ce_preferences::player.eq(&player),
                    ce_preferences::category.eq(category),
                    ce_preferences::data.eq(&encoded),
                ))
                .on_conflict((ce_preferences::player, ce_preferences::category))
                .do_update()
                .set(ce_preferences::data.eq(excluded(ce_preferences::data)))
                .execute(db)?;
            Ok(())
        })
        .await
    }

    pub async fn metrics(&self, id: Uuid, room: Uuid, fields: Vec<Value>) -> Result<(), Error> {
        self.write(move |db| {
            let player = id.to_string();
            let room = room.to_string();
            let encoded = Value::Array(fields).to_string();
            diesel::insert_into(ce_metrics::table)
                .values((
                    ce_metrics::player.eq(&player),
                    ce_metrics::match_id.eq(&room),
                    ce_metrics::data.eq(&encoded),
                ))
                .on_conflict((ce_metrics::player, ce_metrics::match_id))
                .do_update()
                .set(ce_metrics::data.eq(excluded(ce_metrics::data)))
                .execute(db)?;
            Ok(())
        })
        .await
    }

    pub async fn ranking(&self) -> Result<Value, Error> {
        self.read(|db| {
            let rows = ce_player_matches::table
                .inner_join(players::table.on(players::id.eq(ce_player_matches::player)))
                .order((
                    ce_player_matches::completed.desc(),
                    ce_player_matches::player,
                ))
                .limit(100)
                .select((
                    players::id,
                    players::name,
                    ce_player_matches::completed,
                    ce_player_matches::wins,
                ))
                .load::<(String, String, i64, i64)>(db)?;
            let entries = rows
                .into_iter()
                .map(|(player_uuid, handle, completed, wins)| RankingEntry {
                    player_uuid,
                    handle,
                    completed: completed as u64,
                    wins: wins as u64,
                })
                .collect();
            Ok(serde_json::to_value(Ranking {
                leaderboards: [Leaderboard {
                    leaderboard_id: "completed",
                    leaderboard_title: "Completed matches",
                    entries,
                }],
                events: [],
                time: now(),
            })?)
        })
        .await
    }

    pub async fn hero_stats(&self, id: Uuid) -> Result<Value, Error> {
        self.read(move |db| {
            let rows = ce_hero_matches::table
                .filter(ce_hero_matches::player.eq(id.to_string()))
                .order(ce_hero_matches::hero)
                .select((
                    ce_hero_matches::hero,
                    ce_hero_matches::completed,
                    ce_hero_matches::wins,
                ))
                .load::<(String, i64, i64)>(db)?;
            let stats: Vec<_> = rows
                .into_iter()
                .map(|(hero, completed, wins)| HeroStats {
                    hero,
                    completed: completed as u64,
                    wins: wins as u64,
                })
                .collect();
            Ok(serde_json::to_value(stats)?)
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
