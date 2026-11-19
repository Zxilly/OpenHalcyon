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
