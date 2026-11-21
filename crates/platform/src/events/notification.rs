use crate::EventBody;
use std::time::Duration;

use serde_json::Value;
use uuid::Uuid;

use super::accounts;
use crate::{
    PlatformError, Result,
    service::{AppState, Event, digest, insert_event},
};

impl AppState {
    /// Wake readers after a successful commit made through another connection.
    pub fn wake_notification_readers(&self) {
        self.changes.send_replace(());
    }

    /// Record a parser-supported refresh for local social relationships.
    pub async fn refresh_friends(&self, mut players: Vec<Uuid>) -> Result<()> {
        players.sort_unstable();
        players.dedup();
        self.run(move |state| {
            state.transaction(|db| {
                for id in players {
                    insert_event(db, &id.to_string(), EventBody::FriendsChanged)?;
                }
                Ok(())
            })
        })
        .await
    }

    /// Read one recipient-local notification after the previous cursor.
    /// Chat frame traffic and other players' activity never advance this feed.
    pub async fn next_notification(
        &self,
        session: &str,
        after: i64,
        initial: bool,
    ) -> Result<Option<Event>> {
        if after < 0 {
            return Err(PlatformError::Invalid("invalid notification cursor"));
        }
        let key = digest(session);
        let mut changes = self.changes.subscribe();
        let until = tokio::time::Instant::now() + Duration::from_secs(25);
        self.expire_offers().await?;
        loop {
            let read_key = key.clone();
            let result = self
                .run(move |state| {
                    let row = state
                        .accounts
                        .lock()
                        .map_err(|_| PlatformError::WorkerUnavailable)?
                        .next_notification(&read_key, after, initial)
                        .map_err(accounts::storage_error)?;
                    row.map(|(cursor, body)| {
                        let body: Value = serde_json::from_str(&body)
                            .map_err(|_| PlatformError::WorkerUnavailable)?;
                        Ok(Event { cursor, body })
                    })
                    .transpose()
                })
                .await?;
            if result.is_some() || initial || tokio::time::Instant::now() >= until {
                return Ok(result);
            }
            if tokio::time::timeout_at(until, changes.changed())
                .await
                .is_err()
            {
                // Check expired offers and revalidate the session before a 304.
                self.expire_offers().await?;
            }
        }
    }
}
