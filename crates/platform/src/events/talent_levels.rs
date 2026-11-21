use super::orm_schema::{federated_queue, federated_talent_levels};
use crate::{PlatformError, Result, service::AppState};
use diesel::{prelude::*, sqlite::SqliteConnection};
use halcyon_data::TalentLevels;
use std::sync::Arc;
use uuid::Uuid;

pub(crate) type Resolver =
    Arc<dyn Fn(&mut SqliteConnection, Uuid) -> Result<TalentLevels> + Send + Sync>;

pub(crate) fn decode(raw: Option<&str>) -> Result<Vec<TalentLevels>> {
    raw.map(|data| serde_json::from_str(data).map_err(|_| PlatformError::WorkerUnavailable))
        .transpose()
        .map(Option::unwrap_or_default)
}

impl AppState {
    /// Install the local inventory authority during startup.
    pub fn set_talent_levels_resolver(
        &self,
        resolver: impl Fn(&mut SqliteConnection, Uuid) -> Result<TalentLevels> + Send + Sync + 'static,
    ) -> Result<()> {
        self.talent_levels
            .lock()
            .map_err(|_| PlatformError::WorkerUnavailable)?
            .replace(Arc::new(resolver));
        Ok(())
    }

    pub(crate) fn resolve_talent_levels(
        &self,
        db: &mut SqliteConnection,
        mode: &str,
        players: &[Uuid],
    ) -> Result<Vec<TalentLevels>> {
        if !halcyon_data::supports_talents(mode) {
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
            .map(|&player| resolver(db, player))
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
        self.run(move |state| {
            state.read(|db| {
                let raw = federated_talent_levels::table
                    .inner_join(federated_queue::table)
                    .filter(federated_queue::ticket.eq(ticket.to_string()))
                    .filter(federated_queue::pool.eq(pool.to_string()))
                    .select(federated_talent_levels::levels)
                    .first::<String>(db)
                    .optional()?;
                decode(raw.as_deref())
            })
        })
        .await
    }
}
