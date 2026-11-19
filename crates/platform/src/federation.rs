use rusqlite::{OptionalExtension, Row, params};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{AppState, PlatformError, Result, digest, event, player};

#[derive(Clone, Debug)]
pub struct FederatedIntent {
    pub ticket: Uuid,
    pub player: Uuid,
    pub pool: Uuid,
    pub mode: String,
    pub seats: u8,
    pub accepted_match: Option<Uuid>,
    pub cancel: bool,
    pub phase: String,
    pub match_id: Option<Uuid>,
    pub address: Option<String>,
    pub result: Option<Value>,
}

const FIELDS: &str =
    "ticket, player, pool, mode, seats, accepted_match, cancel, phase, match_id, address, result";

fn parse<T>(
    row: &Row<'_>,
    idx: usize,
    decode: impl Fn(&str) -> std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>,
) -> rusqlite::Result<Option<T>> {
    row.get::<_, Option<String>>(idx)?
        .as_deref()
        .map(decode)
        .transpose()
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, error)
        })
}

fn row_intent(row: &Row<'_>) -> rusqlite::Result<FederatedIntent> {
    let parse_uuid = |s: &str| Uuid::parse_str(s).map_err(Into::into);
    let uuid = |idx| {
        parse(row, idx, parse_uuid)?.ok_or(rusqlite::Error::InvalidColumnType(
            idx,
            idx.to_string(),
            rusqlite::types::Type::Null,
        ))
    };
    Ok(FederatedIntent {
        ticket: uuid(0)?,
        player: uuid(1)?,
        pool: uuid(2)?,
        mode: row.get(3)?,
        seats: row.get(4)?,
        accepted_match: parse(row, 5, parse_uuid)?,
        cancel: row.get(6)?,
        phase: row.get(7)?,
        match_id: parse(row, 8, parse_uuid)?,
        address: row.get(9)?,
        result: parse(row, 10, |s| serde_json::from_str(s).map_err(Into::into))?,
    })
}

impl AppState {
    pub async fn federated_join(
        &self,
        token: &str,
        pool: Uuid,
        mode: String,
        seats: u8,
    ) -> Result<FederatedIntent> {
        if !(1..=64).contains(&seats)
            || mode.is_empty()
            || mode.len() > 32
            || !mode
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(PlatformError::Invalid(
                "invalid federated mode or seat count",
            ));
        }
        let key = digest(token);
        self.run(move |state| state.transaction(|tx| {
            let id = player(tx, &key)?;
            let sql = format!("SELECT {FIELDS} FROM federated_intents WHERE player=?");
            let existing = tx.query_row(&sql, [&id], row_intent).optional()?;
            if let Some(existing) = existing {
                if existing.pool == pool && existing.mode == mode && existing.seats == seats
                    && matches!(existing.phase.as_str(), "pending" | "queued") && !existing.cancel
                {
                    return Ok(existing);
                }
                return Err(PlatformError::Conflict("player has a federated ticket"));
            }
            if tx.query_row("SELECT EXISTS(SELECT 1 FROM tickets WHERE player=?)", [&id], |row| row.get(0))? {
                return Err(PlatformError::Conflict("player has a local ticket"));
            }
            let ticket = Uuid::new_v4();
            tx.execute(
                "INSERT INTO federated_intents(ticket, player, pool, mode, seats) VALUES(?, ?, ?, ?, ?)",
                params![ticket.to_string(), id, pool.to_string(), mode, seats],
            )?;

            Ok(tx.query_row(&sql, [&id], row_intent)?)
        })).await
    }

    pub async fn federated_intent(&self, token: &str) -> Result<Option<FederatedIntent>> {
        let key = digest(token);
        self.run(move |state| {
            let db = state
                .db
                .lock()
                .map_err(|_| PlatformError::WorkerUnavailable)?;
            let id = player(&db, &key)?;
            let sql = format!("SELECT {FIELDS} FROM federated_intents WHERE player=?");
            db.query_row(&sql, [&id], row_intent)
                .optional()
                .map_err(Into::into)
        })
        .await
    }

    pub async fn federated_intents(&self, pool: Uuid) -> Result<Vec<FederatedIntent>> {
        self.run(move |state| {
            let db = state
                .db
                .lock()
                .map_err(|_| PlatformError::WorkerUnavailable)?;
            let sql =
                format!("SELECT {FIELDS} FROM federated_intents WHERE pool=? ORDER BY ticket");
            let mut query = db.prepare(&sql)?;
            query
                .query_map([pool.to_string()], row_intent)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(Into::into)
        })
        .await
    }

    /// Persist a participant's acceptance or cancellation for the federation worker to submit.
    pub async fn federated_action(
        &self,
        ticket: Uuid,
        accept: Option<Uuid>,
        cancel: bool,
    ) -> Result<()> {
        let ticket = ticket.to_string();
        self.run(move |state| {
            state.transaction(|tx| {
                let sql = format!("SELECT {FIELDS} FROM federated_intents WHERE ticket=?");
                let intent = tx
                    .query_row(&sql, [&ticket], row_intent)
                    .optional()?
                    .ok_or(PlatformError::Conflict("federated ticket does not exist"))?;
                if let Some(match_id) = accept {
                    if cancel
                        || intent.cancel
                        || intent.match_id != Some(match_id)
                        || !matches!(intent.phase.as_str(), "offered" | "playing")
                    {
                        return Err(PlatformError::Conflict("no federated offer for this match"));
                    }
                    tx.execute(
                        "UPDATE federated_intents SET accepted_match=? WHERE ticket=?",
                        params![match_id.to_string(), ticket],
                    )?;
                } else if cancel {
                    if !matches!(intent.phase.as_str(), "pending" | "queued" | "offered") {
                        return Err(PlatformError::Conflict(
                            "federated match has already started",
                        ));
                    }
                    tx.execute(
                        "UPDATE federated_intents SET cancel=1 WHERE ticket=?",
                        [&ticket],
                    )?;
                } else {
                    return Err(PlatformError::Invalid("no federated action"));
                }
                Ok(())
            })
        })
        .await
    }

    /// Apply an observed state from a committed federation record.
    pub async fn federated_project(
        &self,
        ticket: Uuid,
        phase: &str,
        match_id: Option<Uuid>,
        address: Option<String>,
        result: Option<Value>,
    ) -> Result<()> {
        match phase {
            "queued" | "closed" => {}
            "offered" | "finished" if match_id.is_some() => {}
            "playing"
                if match_id.is_some() && address.as_ref().is_some_and(|s| !s.trim().is_empty()) => {
            }
            _ => return Err(PlatformError::Invalid("invalid federated projection")),
        }
        let phase = phase.to_owned();
        let ticket = ticket.to_string();
        self.run(move |state| state.transaction(|tx| {
            let sql = format!("SELECT {FIELDS} FROM federated_intents WHERE ticket=?");
            let Some(intent) = tx.query_row(&sql, [&ticket], row_intent).optional()? else {
                return Ok(());
            };
            if intent.phase == phase && intent.match_id == match_id && intent.address == address && intent.result == result {
                return Ok(());
            }
            if intent.phase == "closed" || (intent.phase == "finished" && phase != "closed")
                || (intent.phase == "playing" && !matches!(phase.as_str(), "playing" | "finished" | "closed"))
            {
                return Err(PlatformError::Conflict("stale federated state"));
            }
            if matches!(intent.phase.as_str(), "playing" | "finished") && phase != "closed" && match_id != intent.match_id {
                return Err(PlatformError::Conflict("federated match identity changed"));
            }
            let match_string = match_id.map(|id| id.to_string());
            let accepted = intent.accepted_match.filter(|id| Some(*id) == match_id)
                .map(|id| id.to_string());
            tx.execute(
                "UPDATE federated_intents SET phase=?, match_id=?, address=?, result=?, accepted_match=? WHERE ticket=?",
                params![phase, match_string, address, result.as_ref().map(Value::to_string), accepted, ticket],
            )?;
            let message = match phase.as_str() {
                "queued" => json!({"type":"queued", "mode":intent.mode}),
                "offered" => json!({"type":"match_offered", "matchId":match_id, "mode":intent.mode}),
                "playing" => json!({"type":"match_playing", "matchId":match_id, "address":address}),
                "finished" => json!({"type":"match_finished", "matchId":match_id, "result":result}),
                "closed" if intent.match_id.or(match_id).is_some() => json!({
                    "type":"match_aborted", "matchId":match_id.or(intent.match_id), "reason":"federation_closed"
                }),
                _ => json!({"type":"queue_left"}),
            };
            event(tx, &intent.player.to_string(), message)
        })).await
    }

    /// Release local exclusivity only once the federation confirms closure or the user leaves results.
    pub async fn federated_release(&self, ticket: Uuid) -> Result<()> {
        let ticket = ticket.to_string();
        self.run(move |state| {
            state.transaction(|tx| {
                let sql = format!("SELECT {FIELDS} FROM federated_intents WHERE ticket=?");
                let Some(intent) = tx.query_row(&sql, [&ticket], row_intent).optional()? else {
                    return Ok(());
                };
                match intent.phase.as_str() {
                    "closed" => {}
                    "finished" => event(
                        tx,
                        &intent.player.to_string(),
                        json!({
                            "type":"post_match_left", "matchId":intent.match_id
                        }),
                    )?,
                    _ => return Err(PlatformError::Conflict("federated ticket is still active")),
                }
                tx.execute("DELETE FROM federated_intents WHERE ticket=?", [&ticket])?;
                Ok(())
            })
        })
        .await
    }
}
