use super::{AppState, PlatformError, Result};
use halcyon_data::{KnownQueue, TalentLevels};
use rusqlite::Transaction;
use std::sync::Arc;
use uuid::Uuid;

pub(super) type Resolver =
    Arc<dyn Fn(&Transaction<'_>, Uuid) -> Result<TalentLevels> + Send + Sync>;

pub(super) fn initialize(tx: &Transaction<'_>) -> Result<()> {
    let count: u32 = tx.query_row(
        "SELECT count(*) FROM pragma_table_info('matches') WHERE name='talent_levels'",
        [],
        |row| row.get(0),
    )?;
    if count == 0 {
        tx.execute("ALTER TABLE matches ADD COLUMN talent_levels TEXT", [])?;
    }
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS federated_talent_levels (
        ticket TEXT PRIMARY KEY REFERENCES federated_queue(ticket) ON DELETE CASCADE,
        levels TEXT NOT NULL
    )",
    )?;
    Ok(())
}

pub(super) fn decode(raw: Option<&str>) -> Result<Vec<TalentLevels>> {
    raw.map(|data| serde_json::from_str(data).map_err(|_| PlatformError::WorkerUnavailable))
        .transpose()
        .map(Option::unwrap_or_default)
}

impl AppState {
    /// Install the local inventory authority during startup.
    pub fn set_talent_levels_resolver(
        &self,
        resolver: impl Fn(&Transaction<'_>, Uuid) -> Result<TalentLevels> + Send + Sync + 'static,
    ) -> Result<()> {
        self.talent_levels
            .lock()
            .map_err(|_| PlatformError::WorkerUnavailable)?
            .replace(Arc::new(resolver));
        Ok(())
    }

    pub(super) fn resolve_talent_levels(
        &self,
        tx: &Transaction<'_>,
        mode: &str,
        players: &[Uuid],
    ) -> Result<Vec<TalentLevels>> {
        if KnownQueue::parse(mode) != Some(KnownQueue::CasualAral) {
            return Ok(Vec::new());
        }
        let resolver = self
            .talent_levels
            .lock()
            .map_err(|_| PlatformError::WorkerUnavailable)?
            .clone();
        let Some(resolver) = resolver else {
            return Ok(Vec::new());
        };
        let grants = players
            .iter()
            .map(|&player| resolver(tx, player))
            .collect::<Result<Vec<_>>>()?;
        if !halcyon_data::valid_talent_levels(mode, players.len(), &grants) {
            return Err(PlatformError::WorkerUnavailable);
        }
        Ok(if grants.iter().all(TalentLevels::is_empty) {
            Vec::new()
        } else {
            grants
        })
    }

    /// Read the snapshot written in the trusted home ticket's creation transaction.
    pub async fn pooled_talent_levels(
        &self,
        pool: Uuid,
        ticket: Uuid,
    ) -> Result<Vec<TalentLevels>> {
        use rusqlite::{OptionalExtension, params};
        self.run(move |state| {
            state.transaction(|tx| {
                let raw: Option<String> = tx
                    .query_row(
                        "SELECT g.levels FROM federated_talent_levels g
                     JOIN federated_queue q ON q.ticket=g.ticket WHERE q.ticket=? AND q.pool=?",
                        params![ticket.to_string(), pool.to_string()],
                        |row| row.get(0),
                    )
                    .optional()?;
                decode(raw.as_deref())
            })
        })
        .await
    }
}
