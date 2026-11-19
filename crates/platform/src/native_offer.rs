use super::*;

#[derive(Debug)]
pub struct NativeSeat {
    pub player: Uuid,
    pub accepted: bool,
}

#[derive(Debug)]
pub struct NativeOffer {
    pub id: Uuid,
    pub seats: Vec<NativeSeat>,
    pub entries: u32,
    pub expires: Option<i64>,
}

pub(super) fn initialize(tx: &rusqlite::Transaction<'_>) -> Result<()> {
    tx.execute_batch("CREATE TABLE IF NOT EXISTS native_offer_seats(
        room TEXT NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
        player TEXT NOT NULL, entry TEXT NOT NULL, position INTEGER NOT NULL, accepted INTEGER NOT NULL,
        PRIMARY KEY(room,player));
        INSERT OR IGNORE INTO native_offer_seats SELECT match_id, player, coalesce(batch,player), serial, accepted FROM tickets WHERE match_id IS NOT NULL;
        CREATE TRIGGER IF NOT EXISTS native_offer_assignment AFTER UPDATE OF match_id ON tickets WHEN NEW.match_id IS NOT NULL BEGIN
        INSERT INTO native_offer_seats VALUES(NEW.match_id, NEW.player, coalesce(NEW.batch,NEW.player), NEW.serial, NEW.accepted); END;
        CREATE TRIGGER IF NOT EXISTS native_offer_accept AFTER UPDATE OF accepted ON tickets WHEN NEW.match_id IS NOT NULL BEGIN
        UPDATE native_offer_seats SET accepted=NEW.accepted WHERE room=NEW.match_id AND player=NEW.player; END;")?;
    Ok(())
}

impl AppState {
    pub async fn native_queue_party_size(&self, session: &str) -> Result<u32> {
        let key = digest(session);
        self.run(move |state| {
            let db = state.db.lock().map_err(|_| PlatformError::WorkerUnavailable)?;
            let owner = player(&db, &key)?;
            db.query_row(
                "SELECT coalesce(
                    (SELECT count(*) FROM tickets a JOIN tickets b ON a.player=b.player OR a.batch=b.batch
                        WHERE a.player=?1 AND a.match_id IS NULL GROUP BY a.player),
                    (SELECT count(*) FROM federated_queue_members a JOIN federated_queue_members b ON a.ticket=b.ticket
                        WHERE a.player=?1 AND a.released=0 AND b.released=0 GROUP BY a.ticket),
                    1)", [owner], |row| row.get(0)
            ).map_err(Into::into)
        }).await
    }

    /// The caller must still hold their own ticket to inspect this room's roster.
    pub async fn native_offer(&self, session: &str) -> Result<Option<NativeOffer>> {
        let key = digest(session);
        self.run(move |state| {
            let mut db = state.db.lock().map_err(|_| PlatformError::WorkerUnavailable)?;
            let tx = db.transaction()?;
            let owner = player(&tx, &key)?;
            let binding: Option<(String, i64)> = tx.query_row(
                "SELECT m.id, m.offered_at + 30 FROM tickets t JOIN matches m ON m.id=t.match_id WHERE t.player=?",
                [owner], |row| Ok((row.get(0)?, row.get(1)?)),
            ).optional()?;
            let Some((room, expires)) = binding else { return Ok(None) };
            let mut stmt = tx.prepare("SELECT player, accepted, entry FROM native_offer_seats WHERE room=? ORDER BY position")?;
            let mut rows = stmt.query([&room])?;
            let mut seats = Vec::new();
            let mut entries = std::collections::BTreeSet::new();
            while let Some(row) = rows.next()? {
                let raw: String = row.get(0)?;
                seats.push(NativeSeat {
                    player: Uuid::parse_str(&raw).map_err(|_| PlatformError::WorkerUnavailable)?,
                    accepted: row.get(1)?,
                });
                entries.insert(row.get::<_, String>(2)?);
            }
            Ok(Some(NativeOffer {
                id: Uuid::parse_str(&room).map_err(|_| PlatformError::WorkerUnavailable)?,
                seats,
                entries: entries.len() as u32,
                expires: Some(expires),
            }))
        }).await
    }
}
