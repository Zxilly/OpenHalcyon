use crate::EventBody;
use rusqlite::params;
use uuid::Uuid;

use super::{AppState, PlatformError, Result, digest, parse_id, player};

impl AppState {
    /// Contacts in the authenticated player's current local game roster.
    pub async fn local_match_contacts(&self, session: &str) -> Result<Vec<(Uuid, String)>> {
        let key = digest(session);
        self.run(move |state| {
            let db = state
                .db
                .lock()
                .map_err(|_| PlatformError::WorkerUnavailable)?;
            let id = player(&db, &key)?;
            let mut query = db.prepare(
                "SELECT p.id,p.name FROM tickets mine
                 JOIN matches m ON m.id=mine.match_id AND m.phase='playing'
                 JOIN tickets peer ON peer.match_id=m.id AND peer.player<>mine.player
                 JOIN players p ON p.id=peer.player
                 WHERE mine.player=? ORDER BY p.id",
            )?;
            query
                .query_map(params![id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .map(|row| {
                    let (id, name) = row?;
                    Ok((parse_id(&id)?, name))
                })
                .collect()
        })
        .await
    }
}

impl AppState {
    /// Persist the incoming event once on this home. Its stream cursor and receipt commit together.
    pub async fn receive_pool_chat(
        &self,
        pool: Uuid,
        room: Uuid,
        id: Uuid,
        sender: Uuid,
        text: String,
        expected_seats: usize,
    ) -> Result<()> {
        let pool = pool.to_string();
        let room = room.to_string();
        self.run(move |state| {
            state.transaction(|tx| {
                tx.execute("DELETE FROM federated_chat_receipts WHERE seen < unixepoch()-3600", [])?;
                let fresh = tx.execute(
                    "INSERT OR IGNORE INTO federated_chat_receipts(pool,room,id) VALUES(?,?,?)",
                    params![pool, room, id.to_string()],
                )?;
                if fresh == 0 {
                    return Ok(());
                }
                let event = EventBody::Chat { match_id: super::parse_id(&room)?, player_id: sender, text: &text };
                let recipients = tx.execute(
                    "INSERT INTO events(player,body)
                     SELECT m.player, ? FROM federated_queue_members m JOIN federated_queue f USING(ticket)
                     WHERE f.pool=? AND f.match_id=? AND f.phase IN ('offered','playing') AND m.released=0
                     ORDER BY m.player",
                    params![event.encode()?, pool, room],
                )?;
                if expected_seats == 0 || recipients != expected_seats {
                    return Err(PlatformError::Conflict("waiting for the active room roster"));
                }
                Ok(())
            })
        }).await
    }
}

#[cfg(test)]
mod tests {
    use super::AppState;
    use crate::{HostedMatch, MatchHost, PlatformConfig, PlatformError, Reservation};
    use std::sync::Arc;
    use uuid::Uuid;

    struct NoLocalRooms;
    impl MatchHost for NoLocalRooms {
        fn reserve(&self, _: Reservation) -> Result<HostedMatch, PlatformError> {
            Err(PlatformError::HostUnavailable)
        }
    }

    #[tokio::test]
    async fn chat_waits_for_the_entire_local_roster_and_commits_once_across_restart() {
        let dir = tempfile::tempdir().unwrap();
        let config = PlatformConfig {
            database_path: dir.path().join("platform.db"),
            public_address: "localhost:8000".into(),
            seats_per_match: 2,
        };
        let state = AppState::open(config.clone(), Arc::new(NoLocalRooms)).unwrap();
        let pool = Uuid::new_v4();
        let room = Uuid::new_v4();
        let id = Uuid::new_v4();
        let sender = Uuid::new_v4();
        let mut tokens = Vec::new();
        for name in ["Alice", "Bob"] {
            let guest = state.create_guest(name.into()).await.unwrap();
            let intent = state
                .federated_join(&guest.session_token, pool, "duo".into(), 2)
                .await
                .unwrap();
            state
                .federated_project(
                    intent.ticket,
                    crate::LobbyPhase::Offered.as_str(),
                    Some(room),
                    None,
                    None,
                )
                .await
                .unwrap();
            tokens.push(guest.session_token);
            if tokens.len() == 1 {
                assert!(
                    state
                        .receive_pool_chat(pool, room, id, sender, "text".into(), 2)
                        .await
                        .is_err()
                );
            }
        }
        state
            .receive_pool_chat(pool, room, id, sender, "text".into(), 2)
            .await
            .unwrap();
        drop(state);
        let state = AppState::open(config, Arc::new(NoLocalRooms)).unwrap();
        state
            .receive_pool_chat(pool, room, id, sender, "text".into(), 2)
            .await
            .unwrap();
        for token in tokens {
            let events = state.events(&token, 0, false).await.unwrap();
            assert_eq!(
                events
                    .events
                    .iter()
                    .filter(|e| e.body["type"] == "chat")
                    .count(),
                1
            );
        }
    }
}
