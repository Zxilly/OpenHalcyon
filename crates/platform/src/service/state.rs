use std::sync::{Arc, Mutex};

use diesel::{dsl::exists, prelude::*, sql_types::Text, sqlite::SqliteConnection};
use halcyon_data::{BotDifficulty, KnownQueue};
use halcyon_storage::{AccountStore, DatabaseAddress};
use halcyon_types::{MatchHost, Reservation};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::watch;
use uuid::Uuid;

use super::{PlatformConfig, PlatformError, Result};
use crate::{
    AbortReason, EventBody,
    events::{
        accounts,
        orm_schema::{events, matches, players, tickets},
        talent_levels,
    },
    match_lifecycle::{queue, spectator},
};

diesel::define_sql_function!(#[sql_name = "unixepoch"] fn unixepoch_after(base: Text, modifier: Text) -> diesel::sql_types::BigInt);

/// Application-level observer invoked atomically inside platform queue and match transactions.
pub trait MatchEventObserver: Send + Sync {
    fn on_queued(&self, db: &mut SqliteConnection, players: &[String], mode: &str) -> Result<()>;

    fn on_match_offered(&self, db: &mut SqliteConnection, players: &[String]) -> Result<()>;

    fn on_match_finished(
        &self,
        db: &mut SqliteConnection,
        match_id: Uuid,
        mode: Option<&str>,
        players: &[String],
        result: Option<&Value>,
    ) -> Result<()>;
}

#[derive(Clone)]
pub struct AppState {
    pub(crate) accounts: Arc<Mutex<AccountStore>>,
    pub(crate) host: Arc<dyn MatchHost>,
    pub(crate) changes: watch::Sender<()>,
    pub(crate) config: Arc<PlatformConfig>,
    pub(crate) talent_levels: Arc<Mutex<Option<talent_levels::Resolver>>>,
    pub(crate) event_observer: Arc<Mutex<Option<Arc<dyn MatchEventObserver>>>>,
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
        let address = DatabaseAddress::new(&config.database_path);
        let accounts = AccountStore::open(&address).map_err(PlatformError::Database)?;
        let (changes, _) = watch::channel(());
        let state = Self {
            accounts: Arc::new(Mutex::new(accounts)),
            host,
            changes,
            config: Arc::new(config),
            talent_levels: Default::default(),
            event_observer: Default::default(),
        };
        state.resume_reservations()?;
        Ok(state)
    }

    /// Install an application observer notified inside queue, offer, and settlement transactions.
    pub fn set_event_observer(&self, observer: Arc<dyn MatchEventObserver>) -> Result<()> {
        self.event_observer
            .lock()
            .map_err(|_| PlatformError::WorkerUnavailable)?
            .replace(observer);
        Ok(())
    }

    pub(crate) fn observer(&self) -> Result<Option<Arc<dyn MatchEventObserver>>> {
        Ok(self
            .event_observer
            .lock()
            .map_err(|_| PlatformError::WorkerUnavailable)?
            .clone())
    }

    pub(crate) fn transaction<T>(
        &self,
        work: impl FnOnce(&mut SqliteConnection) -> Result<T>,
    ) -> Result<T> {
        let (value, changed) = self
            .accounts
            .lock()
            .map_err(|_| PlatformError::WorkerUnavailable)?
            .write_observing_changes(work)?;
        if changed {
            self.changes.send_replace(());
        }
        Ok(value)
    }

    pub(crate) fn read<T>(
        &self,
        work: impl FnOnce(&mut SqliteConnection) -> Result<T>,
    ) -> Result<T> {
        self.accounts
            .lock()
            .map_err(|_| PlatformError::WorkerUnavailable)?
            .read(work)
    }

    pub(crate) fn read_as<T>(
        &self,
        key: &[u8],
        work: impl FnOnce(&mut SqliteConnection, &str) -> Result<T>,
    ) -> Result<T> {
        self.accounts
            .lock()
            .map_err(|_| PlatformError::WorkerUnavailable)?
            .read_as(key, work)
    }

    pub(crate) fn read_and_touch_as<T>(
        &self,
        key: &[u8],
        work: impl FnOnce(&mut SqliteConnection, &str) -> Result<T>,
    ) -> Result<T> {
        let (value, stale, id) = self.read_as(key, |db, id| {
            let stale = diesel::select(exists(
                players::table
                    .find(id)
                    .filter(players::last_seen.lt(unixepoch_after("now", "-15 seconds"))),
            ))
            .get_result::<bool>(db)?;
            Ok((work(db, id)?, stale, id.to_owned()))
        })?;
        if stale {
            self.accounts
                .lock()
                .map_err(|_| PlatformError::WorkerUnavailable)?
                .write(|db| halcyon_storage::touch_presence(db, &id))
                .map_err(accounts::storage_error)?;
        }
        Ok(value)
    }

    pub(crate) async fn run<T: Send + 'static>(
        &self,
        work: impl FnOnce(&AppState) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let state = self.clone();
        tokio::task::spawn_blocking(move || work(&state))
            .await
            .map_err(|_| PlatformError::WorkerUnavailable)?
    }

    fn resume_reservations(&self) -> Result<()> {
        let reservations = self.transaction(|db| {
            let rows = matches::table
                .filter(matches::phase.eq("reserving"))
                .select((
                    matches::id,
                    matches::mode,
                    matches::difficulty,
                    matches::talent_levels,
                ))
                .load::<(String, String, Option<String>, Option<String>)>(db)?;
            rows.into_iter()
                .map(|(match_id, mode, difficulty, stored)| {
                    let difficulty = difficulty
                        .as_deref()
                        .map(|raw| {
                            BotDifficulty::from_wire(raw).ok_or(PlatformError::WorkerUnavailable)
                        })
                        .transpose()?;
                    let (players, teams, _) = queue::match_players(db, &match_id, &mode)?;
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

    pub(crate) fn reserve_match(&self, reservation: Reservation) -> Result<()> {
        let match_id = reservation.match_id.to_string();
        let host = match self.host.reserve(reservation) {
            Ok(hosted) if !hosted.address.trim().is_empty() => hosted,
            result => {
                if let Err(error) = result {
                    tracing::warn!(%match_id, %error, "match host rejected reservation");
                }
                self.transaction(|db| abort(db, &match_id, AbortReason::HostUnavailable))?;
                return Err(PlatformError::HostUnavailable);
            }
        };
        self.transaction(|db| {
            diesel::update(matches::table.find(&match_id))
                .set((
                    matches::phase.eq("playing"),
                    matches::address.eq(&host.address),
                ))
                .execute(db)?;
            announce(
                db,
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
            state.transaction(|db| {
                let ids = matches::table
                    .filter(matches::phase.eq("offered"))
                    .filter(matches::offered_at.le(unixepoch_after("now", "-30 seconds")))
                    .select(matches::id)
                    .load::<String>(db)?;
                for id in ids {
                    abort(db, &id, AbortReason::OfferExpired)?;
                }
                Ok(())
            })
        })
        .await
    }
}

pub(crate) fn parse_id(id: &str) -> Result<Uuid> {
    Uuid::parse_str(id).map_err(|_| PlatformError::WorkerUnavailable)
}

pub(crate) fn display_name(name: String) -> Result<String> {
    let name = name.trim().to_owned();
    if name.is_empty() || name.chars().count() > 32 || name.chars().any(char::is_control) {
        return Err(PlatformError::Invalid("invalid display name"));
    }
    Ok(name)
}

pub(crate) fn credential() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

pub(crate) fn digest(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

pub(crate) fn session_owner(db: &mut SqliteConnection, key: &[u8]) -> Result<String> {
    halcyon_storage::session_owner(db, key).map_err(accounts::storage_error)
}

pub(crate) fn player(db: &mut SqliteConnection, key: &[u8]) -> Result<String> {
    let id = session_owner(db, key)?;
    halcyon_storage::touch_presence(db, &id).map_err(accounts::storage_error)?;
    Ok(id)
}

pub(crate) fn insert_event(db: &mut SqliteConnection, id: &str, body: EventBody<'_>) -> Result<()> {
    let encoded = body.encode()?;
    diesel::insert_into(events::table)
        .values((events::player.eq(id), events::body.eq(&encoded)))
        .execute(db)?;
    Ok(())
}

pub(crate) fn announce(
    db: &mut SqliteConnection,
    match_id: &str,
    body: EventBody<'_>,
) -> Result<()> {
    let encoded = body.encode()?;
    let recipients = tickets::table
        .filter(tickets::match_id.eq(match_id))
        .order(tickets::serial)
        .select(tickets::player)
        .load::<String>(db)?;
    if !recipients.is_empty() {
        let rows: Vec<_> = recipients
            .iter()
            .map(|id| (events::player.eq(id), events::body.eq(&encoded)))
            .collect();
        diesel::insert_into(events::table)
            .values(&rows)
            .execute(db)?;
    }
    Ok(())
}

pub(crate) fn abort(db: &mut SqliteConnection, match_id: &str, reason: AbortReason) -> Result<()> {
    queue::release_matches(db, match_id)?;
    spectator::close_match(db, match_id)?;
    diesel::update(matches::table.find(match_id))
        .set(matches::phase.eq("aborted"))
        .execute(db)?;
    announce(
        db,
        match_id,
        EventBody::MatchAborted {
            match_id: parse_id(match_id)?,
            reason,
        },
    )?;
    diesel::delete(tickets::table.filter(tickets::match_id.eq(match_id))).execute(db)?;
    Ok(())
}
