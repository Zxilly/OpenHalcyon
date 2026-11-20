use crate::EventBody;
use std::time::Duration;

use rusqlite::{OptionalExtension, Transaction, params};
use serde_json::Value;
use uuid::Uuid;

use super::{AppState, Event, PlatformError, Result, digest, event, player};

pub(super) fn initialize(tx: &Transaction<'_>) -> Result<()> {
    let existed: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='notification_events')",
        [],
        |row| row.get(0),
    )?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS notification_events (
            player TEXT NOT NULL REFERENCES players(id), ordinal INTEGER NOT NULL,
            event_cursor INTEGER NOT NULL UNIQUE REFERENCES events(cursor) ON DELETE CASCADE,
            PRIMARY KEY(player, ordinal)
         );
         CREATE INDEX IF NOT EXISTS pending_party_notifications
           ON events(json_extract(body,'$.party'))
           WHERE json_extract(body,'$.party') IS NOT NULL;
         CREATE TRIGGER IF NOT EXISTS event_notifications AFTER INSERT ON events
         WHEN json_extract(NEW.body, '$.type') <> 'chat'
         BEGIN
           INSERT INTO notification_events(player, ordinal, event_cursor)
           SELECT NEW.player, coalesce(max(ordinal), 0) + 1, NEW.cursor
           FROM notification_events WHERE player=NEW.player;
         END;",
    )?;
    if !existed {
        tx.execute_batch(
            "INSERT INTO notification_events(player, ordinal, event_cursor)
             SELECT player, row_number() OVER (PARTITION BY player ORDER BY cursor), cursor
             FROM events WHERE json_extract(body, '$.type') <> 'chat';",
        )?;
    }
    Ok(())
}

/// Write an integration event inside a transaction on the shared platform database.
pub fn record_external_notification(
    tx: &Transaction<'_>,
    recipient: &str,
    body: &Value,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO events(player, body) VALUES(?, ?)",
        params![recipient, body.to_string()],
    )?;
    Ok(())
}

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
            state.transaction(|tx| {
                for id in players {
                    event(tx, &id.to_string(), EventBody::FriendsChanged)?;
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
                    let db = state
                        .db
                        .lock()
                        .map_err(|_| PlatformError::WorkerUnavailable)?;
                    let id = player(&db, &read_key)?;
                    let latest: i64 = db.query_row(
                        "SELECT coalesce(max(ordinal), 0) FROM notification_events WHERE player=?",
                        [&id],
                        |row| row.get(0),
                    )?;
                    // A client may retain an old deployment's cursor across reconnect.
                    // Restart this stream from the most recent snapshot in that case.
                    let ordered_after = if initial || after > latest {
                        latest.saturating_sub(1)
                    } else {
                        after
                    };
                    let row = db
                        .query_row(
                            "SELECT n.ordinal, e.body FROM notification_events n
                     JOIN events e ON e.cursor=n.event_cursor
                     WHERE n.player=? AND n.ordinal>? ORDER BY n.ordinal LIMIT 1",
                            params![id, ordered_after],
                            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
                        )
                        .optional()?;
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
