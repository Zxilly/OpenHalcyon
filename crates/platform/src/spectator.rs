use crate::EventBody;
use rusqlite::{OptionalExtension, Transaction, params};
use uuid::Uuid;

use super::{
    AppState, HostError, PlatformError, Result, digest, event, parse_id, player,
    queue::community_error,
};

pub(super) fn initialize(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS spectator_sessions (
            player TEXT PRIMARY KEY REFERENCES players(id),
            request TEXT NOT NULL UNIQUE, match_id TEXT NOT NULL REFERENCES matches(id),
            spectator_id TEXT UNIQUE, address TEXT, cancelled INTEGER NOT NULL DEFAULT 0,
            CHECK ((spectator_id IS NULL) = (address IS NULL))
         );
         CREATE INDEX IF NOT EXISTS spectators_by_match ON spectator_sessions(match_id);
         DELETE FROM spectator_sessions WHERE spectator_id IS NULL;",
    )?;
    Ok(())
}

pub(super) fn close_match(tx: &Transaction<'_>, match_id: &str) -> Result<()> {
    let message = EventBody::SpectatorLeft {
        match_id: parse_id(match_id)?,
    };
    tx.execute(
        "INSERT INTO events(player,body) SELECT player, ? FROM spectator_sessions
         WHERE match_id=? AND spectator_id IS NOT NULL ORDER BY player",
        params![message.encode()?, match_id],
    )?;
    tx.execute(
        "DELETE FROM spectator_sessions WHERE match_id=?",
        [match_id],
    )?;
    Ok(())
}

impl AppState {
    fn spectator_exit_handler(
        &self,
        player: Uuid,
        room: Uuid,
        request: String,
    ) -> super::SpectatorExit {
        // The host retains this callback for reconnects; do not retain the host through AppState.
        let db = std::sync::Arc::downgrade(&self.db);
        let changes = self.changes.clone();
        std::sync::Arc::new(move |grant| {
            let update = || -> Result<()> {
                let db = db.upgrade().ok_or(PlatformError::WorkerUnavailable)?;
                let mut db = db.lock().map_err(|_| PlatformError::WorkerUnavailable)?;
                let tx = db.transaction()?;
                let removed = tx.execute(
                    "DELETE FROM spectator_sessions WHERE match_id=? AND player=?
                     AND (spectator_id=? OR (spectator_id IS NULL AND request=?))",
                    params![
                        room.to_string(),
                        player.to_string(),
                        grant.to_string(),
                        request
                    ],
                )?;
                if removed > 0 {
                    event(
                        &tx,
                        &player.to_string(),
                        EventBody::SpectatorLeft { match_id: room },
                    )?;
                }
                tx.commit()?;
                if removed > 0 {
                    changes.send_replace(());
                }
                Ok(())
            };
            update().map_err(HostError::from)
        })
    }

    pub async fn start_spectating(&self, token: &str, friend: Uuid) -> Result<()> {
        let key = digest(token);
        let friend = friend.to_string();
        let request = Uuid::new_v4().to_string();
        self.run(move |state| {
            let begin = state.transaction(|tx| {
                let actor = player(tx, &key)?;
                if !halcyon_community::may_spectate(tx, &actor, &friend).map_err(community_error)? {
                    return Err(PlatformError::Conflict("friend is unavailable for spectating"));
                }
                let room: String = tx.query_row(
                    "SELECT m.id FROM tickets t JOIN matches m ON m.id=t.match_id
                     WHERE t.player=? AND m.phase='playing'",
                    [&friend], |row| row.get(0),
                ).optional()?.ok_or(PlatformError::Conflict("friend is unavailable for spectating"))?;
                let existing = tx.query_row(
                    "SELECT match_id, spectator_id FROM spectator_sessions WHERE player=?",
                    [&actor], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
                ).optional()?;
                if let Some((current, prior)) = existing {
                    let prior = prior.filter(|_| current == room)
                        .ok_or(PlatformError::Conflict("a spectator session is already in use"))?;
                    tx.execute(
                        "UPDATE spectator_sessions SET request=?, cancelled=0 WHERE player=?",
                        params![request, actor],
                    )?;
                    return Ok((parse_id(&actor)?, parse_id(&room)?, Some(parse_id(&prior)?)));
                }
                let occupied: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM tickets WHERE player=?1)
                      OR EXISTS(SELECT 1 FROM federated_queue_members WHERE player=?1 AND released=0)
                      OR EXISTS(SELECT 1 FROM pooled_spectators WHERE player=?1)",
                    [&actor], |row| row.get(0),
                )?;
                if occupied {
                    return Err(PlatformError::Conflict("player is queued or in another match"));
                }
                tx.execute(
                    "INSERT INTO spectator_sessions(player, request, match_id) VALUES(?, ?, ?)",
                    params![actor, request, room],
                )?;
                Ok((parse_id(&actor)?, parse_id(&room)?, None))
            })?;
            let (player, match_id, previous) = begin;
            let exit = state.spectator_exit_handler(player, match_id, request.clone());
            match state.host.spectate_with_exit(match_id, player, exit) {
                Ok(hosted) => {
                    let published = state.transaction(|tx| {
                        if hosted.address.trim().is_empty() || hosted.spectator_id.is_nil() {
                            return Err(PlatformError::HostUnavailable);
                        }
                        let actor = player.to_string();
                        let updated = tx.execute(
                            "UPDATE spectator_sessions SET spectator_id=?, address=? WHERE request=? AND cancelled=0",
                            params![hosted.spectator_id.to_string(), hosted.address, request],
                        )?;
                        if updated == 0 {
                            return Err(PlatformError::Conflict("spectator session was closed"));
                        }
                        event(tx, &actor, EventBody::SpectatorEntered { match_id })
                    });
                    if published.is_err() {
                        let _ = state.host.stop_spectating(match_id, hosted.spectator_id);
                        state.transaction(|tx| {
                            tx.execute("DELETE FROM spectator_sessions WHERE request=?", [&request])?;
                            Ok(())
                        })?;
                    }
                    published
                }
                Err(error) => {
                    if let Some(previous) = previous {
                        let _ = state.host.stop_spectating(match_id, previous);
                    }
                    state.transaction(|tx| {
                        tx.execute("DELETE FROM spectator_sessions WHERE request=?", [&request])?;
                        Ok(())
                    })?;
                    Err(error.into())
                }
            }
        }).await
    }

    /// Revoke the host capability before clearing an active viewer session.
    pub async fn leave_spectator(&self, token: &str) -> Result<()> {
        let key = digest(token);
        self.run(move |state| {
            let prior = state.transaction(|tx| {
                let actor = player(tx, &key)?;
                let prior = tx.query_row(
                    "SELECT request, match_id, spectator_id FROM spectator_sessions WHERE player=?",
                    [&actor], |row| Ok((
                        row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?,
                    )),
                ).optional()?;
                let Some((request, room, spectator)) = prior else { return Ok(None) };
                if let Some(spectator) = spectator {
                    Ok(Some((actor, request, parse_id(&room)?, parse_id(&spectator)?)))
                } else {
                    tx.execute("UPDATE spectator_sessions SET cancelled=1 WHERE request=?", [&request])?;
                    Ok(None)
                }
            })?;
            let Some((actor, request, room, spectator)) = prior else { return Ok(()) };
            state.host.stop_spectating(room, spectator)?;
            state.transaction(|tx| {
                if tx.execute("DELETE FROM spectator_sessions WHERE request=?", [&request])? > 0 {
                    event(tx, &actor, EventBody::SpectatorLeft { match_id: room })?;
                }
                Ok(())
            })
        }).await
    }
}
