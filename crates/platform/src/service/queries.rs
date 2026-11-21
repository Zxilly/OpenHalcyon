use std::time::Duration;

use diesel::prelude::*;
use serde::Serialize;
use serde_json::Value;

use super::{AppState, PlatformError, Result, digest};
use crate::events::orm_schema::events;

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

    pub(crate) async fn read_events(&self, key: Vec<u8>, after: i64) -> Result<EventBatch> {
        self.run(move |state| {
            state.read_and_touch_as(&key, |db, id| {
                let records = events::table
                    .filter(events::player.eq(id))
                    .filter(events::cursor.gt(after))
                    .order(events::cursor)
                    .limit(128)
                    .select((events::cursor, events::body))
                    .load::<(i64, String)>(db)?;
                let events = records
                    .into_iter()
                    .map(|(cursor, body)| {
                        Ok(Event {
                            cursor,
                            body: serde_json::from_str(&body)
                                .map_err(|_| PlatformError::WorkerUnavailable)?,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                let cursor = events.last().map_or(after, |entry| entry.cursor);
                Ok(EventBatch { cursor, events })
            })
        })
        .await
    }

    pub(crate) async fn wait_events(&self, key: &[u8], after: i64) -> Result<EventBatch> {
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
