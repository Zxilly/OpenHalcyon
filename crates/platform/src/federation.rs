use halcyon_community::queue as community_queue;
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use uuid::Uuid;

use super::{
    AppState, BotDifficulty, PlatformError, Result, digest, event, player,
    queue::{
        Roster, add_difficulty_column, community_error, read_difficulty, validate_difficulty,
        validate_mode,
    },
};

pub struct FederatedOfferBinding {
    pub player: Uuid,
    pub ticket: Uuid,
    pub room: Uuid,
    pub accepted: std::collections::BTreeSet<Uuid>,
}

#[derive(Clone, Debug)]
pub struct FederatedIntent {
    pub ticket: Uuid,
    pub reported_ms: BTreeMap<Uuid, u32>,
    pub player: Uuid,
    pub players: Vec<Uuid>,
    pub pool: Uuid,
    pub mode: String,
    pub difficulty: Option<BotDifficulty>,
    pub seats: u8,
    pub accepted_match: Option<Uuid>,
    pub cancel: bool,
    pub phase: String,
    pub match_id: Option<Uuid>,
    pub address: Option<String>,
    pub result: Option<Value>,
    pub pre_freeze: bool,
}

const FIELDS: &str =
    "f.ticket, f.player, f.pool, f.mode, f.seats, f.accepted_match, f.cancel, f.phase,
     f.match_id, f.address, f.result,
     (SELECT json_group_array(player ORDER BY pos) FROM federated_queue_members WHERE ticket=f.ticket), f.enqueue_schema, f.difficulty, (SELECT estimates FROM federated_site_reports WHERE ticket=f.ticket)";

pub(super) fn initialize(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS federated_chat_receipts(pool TEXT NOT NULL, room TEXT NOT NULL, id TEXT NOT NULL, seen INTEGER NOT NULL DEFAULT(unixepoch()), PRIMARY KEY(pool, room, id));
         CREATE INDEX IF NOT EXISTS federated_chat_receipt_expiry ON federated_chat_receipts(seen);
         CREATE TABLE IF NOT EXISTS federated_site_reports(ticket TEXT PRIMARY KEY REFERENCES federated_queue(ticket) ON DELETE CASCADE, estimates TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS federated_queue (
            ticket TEXT PRIMARY KEY, player TEXT NOT NULL REFERENCES players(id),
            pool TEXT NOT NULL, mode TEXT NOT NULL, seats INTEGER NOT NULL,
            accepted_match TEXT, cancel INTEGER NOT NULL DEFAULT 0, phase TEXT NOT NULL DEFAULT 'pending',
            match_id TEXT, address TEXT, result TEXT, enqueue_schema INTEGER NOT NULL DEFAULT 0, difficulty TEXT
         );
         CREATE INDEX IF NOT EXISTS federated_queue_by_pool ON federated_queue(pool);
         CREATE INDEX IF NOT EXISTS federated_queue_pending ON federated_queue(pool,ticket)
         WHERE phase<>'finished';
         CREATE TABLE IF NOT EXISTS federated_queue_members (
            ticket TEXT NOT NULL REFERENCES federated_queue(ticket) ON DELETE CASCADE,
            player TEXT NOT NULL REFERENCES players(id), pos INTEGER NOT NULL,
            accepted_match TEXT, released INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY(ticket, player), UNIQUE(ticket, pos)
         );
         CREATE UNIQUE INDEX IF NOT EXISTS federated_one_active_ticket
         ON federated_queue_members(player) WHERE released=0;",
    )?;
    let has_schema: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('federated_queue') WHERE name='enqueue_schema')",
        [], |row| row.get(0),
    )?;
    if !has_schema {
        tx.execute_batch(
            "ALTER TABLE federated_queue ADD COLUMN enqueue_schema INTEGER NOT NULL DEFAULT 0",
        )?;
    }
    add_difficulty_column(tx, "federated_queue")?;
    let legacy: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='federated_intents')",
        [], |row| row.get(0),
    )?;
    if legacy {
        tx.execute_batch(
            "INSERT INTO federated_queue(ticket,player,pool,mode,seats,accepted_match,cancel,phase,match_id,address,result)
             SELECT ticket, player, pool, mode, seats, accepted_match, cancel, phase, match_id, address, result
             FROM federated_intents;
             INSERT INTO federated_queue_members(ticket, player, pos, accepted_match)
             SELECT ticket, player, 0, accepted_match FROM federated_intents;
             DROP TABLE federated_intents;",
        )?;
    }
    Ok(())
}

fn intent_for_player(db: &Connection, player: &str) -> Result<Option<FederatedIntent>> {
    let sql = format!(
        "SELECT {FIELDS} FROM federated_queue f JOIN federated_queue_members m ON m.ticket=f.ticket
         WHERE m.player=? AND m.released=0"
    );
    db.query_row(&sql, [player], row_intent)
        .optional()
        .map_err(Into::into)
}

fn intent_by_ticket(db: &Connection, ticket: &str) -> Result<Option<FederatedIntent>> {
    let sql = format!("SELECT {FIELDS} FROM federated_queue f WHERE f.ticket=?");
    db.query_row(&sql, [ticket], row_intent)
        .optional()
        .map_err(Into::into)
}

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
        players: parse(row, 11, |s| serde_json::from_str(s).map_err(Into::into))?
            .unwrap_or_default(),
        pre_freeze: row.get::<_, u32>(12)? == 0,
        difficulty: read_difficulty(row, 13)?,
        reported_ms: parse(row, 14, |s| serde_json::from_str(s).map_err(Into::into))?
            .unwrap_or_default(),
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
        self.federated_join_roster(token, pool, mode, seats, None, (false, BTreeMap::new()))
            .await
    }

    pub async fn federated_join_party_or_solo(
        &self,
        token: &str,
        pool: Uuid,
        mode: String,
        seats: u8,
    ) -> Result<FederatedIntent> {
        self.federated_join_roster(token, pool, mode, seats, None, (true, BTreeMap::new()))
            .await
    }

    pub async fn federated_join_party_with_difficulty(
        &self,
        token: &str,
        pool: Uuid,
        mode: String,
        seats: u8,
        difficulty: Option<BotDifficulty>,
    ) -> Result<FederatedIntent> {
        self.federated_join_roster(
            token,
            pool,
            mode,
            seats,
            difficulty,
            (true, BTreeMap::new()),
        )
        .await
    }

    pub async fn federated_join_party_with_site_report(
        &self,
        token: &str,
        pool: Uuid,
        mode: String,
        seats: u8,
        difficulty: Option<BotDifficulty>,
        estimates: BTreeMap<Uuid, u32>,
    ) -> Result<FederatedIntent> {
        self.federated_join_roster(token, pool, mode, seats, difficulty, (true, estimates))
            .await
    }

    async fn federated_join_roster(
        &self,
        token: &str,
        pool: Uuid,
        mode: String,
        seats: u8,
        difficulty: Option<BotDifficulty>,
        roster_input: (bool, BTreeMap<Uuid, u32>),
    ) -> Result<FederatedIntent> {
        let (with_party, estimates) = roster_input;
        if estimates.len() > 32 || estimates.values().any(|ms| !(1..=2000).contains(ms)) {
            return Err(PlatformError::Invalid("invalid site estimates"));
        }
        validate_mode(&mode, usize::from(seats))?;
        validate_difficulty(&mode, difficulty)?;
        let key = digest(token);
        self.run(move |state| state.transaction(|tx| {
            let id = player(tx, &key)?;
            let roster = Roster::select(tx, id.clone(), &mode, usize::from(seats), with_party)?;
            if let Some(existing) = intent_for_player(tx, &id)? {
                let claimed_party: Option<String> = tx.query_row(
                    "SELECT party FROM community_party_queues WHERE ticket=?",
                    [existing.ticket.to_string()], |row| row.get(0),
                ).optional()?;
                if existing.player.to_string() == id && existing.pool == pool
                    && existing.mode == mode && existing.seats == seats && existing.difficulty == difficulty
                    && matches!(existing.phase.as_str(), "pending" | "queued") && !existing.cancel
                    && claimed_party == roster.party
                    && existing.players.iter().map(Uuid::to_string).eq(roster.players.iter().cloned())
                {
                    return Ok(existing);
                }
                return Err(PlatformError::Conflict("player has a federated ticket"));
            }
            roster.ensure_free(tx)?;
            let ticket = Uuid::new_v4().to_string();
            roster.claim(tx, &ticket)?;
            tx.execute(
                "INSERT INTO federated_queue(ticket, player, pool, mode, seats, enqueue_schema, difficulty, queued_at_ms) VALUES(?, ?, ?, ?, ?, 1, ?, CAST(unixepoch('subsec')*1000 AS INTEGER))",
                params![ticket, id, pool.to_string(), mode, seats, difficulty.map(BotDifficulty::as_str)],
            )?;
            if !estimates.is_empty() {
                tx.execute("INSERT INTO federated_site_reports(ticket,estimates) VALUES(?,?)",params![ticket,json!(estimates).to_string()])?;
            }
            for (index, member) in roster.players.iter().enumerate() {
                tx.execute(
                    "INSERT INTO federated_queue_members(ticket, player, pos) VALUES(?, ?, ?)",
                    params![ticket, member, index],
                )?;
            }
            let intent = intent_by_ticket(tx, &ticket)?.ok_or(PlatformError::WorkerUnavailable)?;
            let levels = state.resolve_talent_levels(tx, &intent.mode, &intent.players)?;
            if !levels.is_empty() {
                let levels = serde_json::to_string(&levels).map_err(|_| PlatformError::WorkerUnavailable)?;
                tx.execute("INSERT INTO federated_talent_levels(ticket,levels) VALUES(?,?)", params![ticket, levels])?;
            }
            Ok(intent)
        })).await
    }

    /// Bind an offer read to this session's unreleased home ticket, including
    /// individual local party responses before the whole ticket accepts.
    pub async fn federated_offer_binding(
        &self,
        token: &str,
        pool: Uuid,
    ) -> Result<Option<FederatedOfferBinding>> {
        let key = digest(token);
        self.run(move |state| {
            let db = state
                .db
                .lock()
                .map_err(|_| PlatformError::WorkerUnavailable)?;
            let player = player(&db, &key)?;
            let binding: Option<(String,String)> = db.query_row(
                "SELECT f.ticket,f.match_id FROM federated_queue_members m JOIN federated_queue f
                 ON f.ticket=m.ticket WHERE m.player=? AND m.released=0 AND f.pool=?
                 AND f.phase IN ('offered','playing','finished') AND f.match_id IS NOT NULL",
                params![player,pool.to_string()],|row| Ok((row.get(0)?,row.get(1)?))
            ).optional()?;
            let Some((ticket, room)) = binding else {
                return Ok(None);
            };
            let mut stmt = db.prepare(
                "SELECT player FROM federated_queue_members WHERE ticket=? AND accepted_match=?",
            )?;
            let uuid = |s: &str| Uuid::parse_str(s).map_err(|_| PlatformError::WorkerUnavailable);
            let mut accepted = std::collections::BTreeSet::new();
            for id in stmt.query_map(params![ticket, room], |r| r.get::<_, String>(0))? {
                accepted.insert(uuid(&id?)?);
            }
            Ok(Some(FederatedOfferBinding {
                player: uuid(&player)?,
                ticket: uuid(&ticket)?,
                room: uuid(&room)?,
                accepted,
            }))
        })
        .await
    }

    pub async fn federated_intent(&self, token: &str) -> Result<Option<FederatedIntent>> {
        let key = digest(token);
        self.run(move |state| {
            let db = state
                .db
                .lock()
                .map_err(|_| PlatformError::WorkerUnavailable)?;
            let id = player(&db, &key)?;
            intent_for_player(&db, &id)
        })
        .await
    }

    pub async fn federated_intents(&self, pool: Uuid) -> Result<Vec<FederatedIntent>> {
        self.read_federated_intents(pool, false).await
    }

    /// Fetch home intents which still need reconciliation. Completed views are
    /// retained separately until all party members exit post-match.
    pub async fn pending_federated_intents(&self, pool: Uuid) -> Result<Vec<FederatedIntent>> {
        self.read_federated_intents(pool, true).await
    }

    async fn read_federated_intents(
        &self,
        pool: Uuid,
        pending: bool,
    ) -> Result<Vec<FederatedIntent>> {
        self.run(move |state| {
            let db = state
                .db
                .lock()
                .map_err(|_| PlatformError::WorkerUnavailable)?;
            let filter = if pending {
                "AND f.phase<>'finished'"
            } else {
                ""
            };
            let sql = format!(
                "SELECT {FIELDS} FROM federated_queue f WHERE f.pool=? {filter} ORDER BY f.ticket"
            );
            let mut query = db.prepare(&sql)?;
            query
                .query_map([pool.to_string()], row_intent)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(Into::into)
        })
        .await
    }

    /// Record a leave request or acknowledge finished results atomically.
    /// A caller absent from this pool already satisfies the leave request.
    pub async fn federated_begin_leave(
        &self,
        token: &str,
        pool: Uuid,
    ) -> Result<Option<FederatedIntent>> {
        let key = digest(token);
        self.run(move |state| {
            state.transaction(|tx| {
                let id = player(tx, &key)?;
                let Some(mut intent) =
                    intent_for_player(tx, &id)?.filter(|intent| intent.pool == pool)
                else {
                    return Ok(None);
                };
                match intent.phase.as_str() {
                    "closed" => Ok(None),
                    "finished" => {
                        acknowledge(tx, &intent, &id)?;
                        Ok(None)
                    }
                    _ => {
                        apply_action(tx, &intent, &id, None, true)?;
                        intent.cancel = true;
                        Ok(Some(intent))
                    }
                }
            })
        })
        .await
    }

    /// Persist the caller's choice. A party becomes accepted after every member confirms.
    pub async fn federated_action_as(
        &self,
        token: &str,
        ticket: Uuid,
        accept: Option<Uuid>,
        cancel: bool,
    ) -> Result<FederatedIntent> {
        self.federated_action_with_deadline(token, ticket, accept, cancel, None)
            .await
    }

    /// Record an individual confirmation only within the committed offer window.
    pub async fn federated_accept_in_window(
        &self,
        token: &str,
        ticket: Uuid,
        room: Uuid,
        deadline: i64,
    ) -> Result<FederatedIntent> {
        self.federated_action_with_deadline(token, ticket, Some(room), false, Some(deadline))
            .await
    }

    async fn federated_action_with_deadline(
        &self,
        token: &str,
        ticket: Uuid,
        accept: Option<Uuid>,
        cancel: bool,
        deadline: Option<i64>,
    ) -> Result<FederatedIntent> {
        let key = digest(token);
        let ticket = ticket.to_string();
        self.run(move |state| {
            state.transaction(|tx| {
                let id = player(tx, &key)?;
                let intent = intent_for_player(tx, &id)?
                    .filter(|active| active.ticket.to_string() == ticket)
                    .ok_or(PlatformError::Conflict(
                        "federated ticket does not belong to player",
                    ))?;
                if let Some(deadline) = deadline
                    && !tx.query_row("SELECT unixepoch() < ?", [deadline], |row| {
                        row.get::<_, bool>(0)
                    })?
                {
                    return Err(PlatformError::Conflict("federated offer has expired"));
                }
                apply_action(tx, &intent, &id, accept, cancel)?;
                intent_by_ticket(tx, &ticket)?.ok_or(PlatformError::WorkerUnavailable)
            })
        })
        .await
    }

    /// Host-side action for a single-player ticket.
    pub async fn federated_action(
        &self,
        ticket: Uuid,
        accept: Option<Uuid>,
        cancel: bool,
    ) -> Result<()> {
        let ticket = ticket.to_string();
        self.run(move |state| {
            state.transaction(|tx| {
                let intent = intent_by_ticket(tx, &ticket)?
                    .ok_or(PlatformError::Conflict("federated ticket does not exist"))?;
                if intent.players.len() != 1 {
                    return Err(PlatformError::Conflict(
                        "party action requires an individual session",
                    ));
                }
                apply_action(tx, &intent, &intent.player.to_string(), accept, cancel)
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
            let Some(intent) = intent_by_ticket(tx, &ticket)? else {
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
                "UPDATE federated_queue SET phase=?, match_id=?, address=?, result=?, accepted_match=? WHERE ticket=?",
                params![phase, match_string, address, result.as_ref().map(Value::to_string), accepted, ticket],
            )?;
            if intent.match_id != match_id {
                tx.execute(
                    "UPDATE federated_queue_members SET accepted_match=NULL WHERE ticket=?",
                    [&ticket],
                )?;
            }
            if matches!(phase.as_str(), "closed" | "finished") {
                community_queue::release(tx, &ticket).map_err(community_error)?;
            }
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
            tx.execute(
                "INSERT INTO events(player,body) SELECT player, ? FROM federated_queue_members
                 WHERE ticket=? AND released=0 ORDER BY pos",
                params![message.to_string(), ticket],
            )?;
            Ok(())
        })).await
    }

    /// Release one participant's local claim after they exit results.
    pub async fn federated_acknowledge(&self, token: &str, ticket: Uuid) -> Result<()> {
        let key = digest(token);
        self.run(move |state| {
            state.transaction(|tx| {
                let id = player(tx, &key)?;
                let Some(intent) = intent_for_player(tx, &id)? else {
                    return Ok(());
                };
                if intent.ticket != ticket || intent.phase != "finished" {
                    return Err(PlatformError::Conflict("federated ticket is still active"));
                }
                acknowledge(tx, &intent, &id)
            })
        })
        .await
    }

    /// Release the entire closed ticket, or acknowledge a single-player result.
    pub async fn federated_release(&self, ticket: Uuid) -> Result<()> {
        let ticket = ticket.to_string();
        self.run(move |state| {
            state.transaction(|tx| {
                let Some(intent) = intent_by_ticket(tx, &ticket)? else {
                    return Ok(());
                };
                match intent.phase.as_str() {
                    "closed" => {
                        community_queue::release(tx, &ticket).map_err(community_error)?;
                        tx.execute("DELETE FROM federated_queue WHERE ticket=?", [&ticket])?;
                        Ok(())
                    }
                    "finished" if intent.players.len() == 1 => {
                        acknowledge(tx, &intent, &intent.player.to_string())
                    }
                    _ => Err(PlatformError::Conflict(
                        "each party member must exit individually",
                    )),
                }
            })
        })
        .await
    }
}

fn apply_action(
    tx: &Transaction<'_>,
    intent: &FederatedIntent,
    actor: &str,
    accept: Option<Uuid>,
    cancel: bool,
) -> Result<()> {
    let ticket = intent.ticket.to_string();
    if let Some(match_id) = accept {
        if cancel
            || intent.cancel
            || intent.match_id != Some(match_id)
            || !matches!(intent.phase.as_str(), "offered" | "playing")
        {
            return Err(PlatformError::Conflict("no federated offer for this match"));
        }
        let id = match_id.to_string();
        tx.execute(
            "UPDATE federated_queue_members SET accepted_match=? WHERE ticket=? AND player=? AND released=0",
            params![id, ticket, actor],
        )?;
        tx.execute(
            "UPDATE federated_queue SET accepted_match=?1 WHERE ticket=?2 AND NOT EXISTS (
                SELECT 1 FROM federated_queue_members WHERE ticket=?2 AND accepted_match IS NOT ?1
             )",
            params![id, ticket],
        )?;
    } else if cancel {
        if !matches!(intent.phase.as_str(), "pending" | "queued" | "offered") {
            return Err(PlatformError::Conflict(
                "federated match has already started",
            ));
        }
        tx.execute(
            "UPDATE federated_queue SET cancel=1 WHERE ticket=?",
            [&ticket],
        )?;
    } else {
        return Err(PlatformError::Invalid("no federated action"));
    }
    Ok(())
}

fn acknowledge(tx: &Transaction<'_>, intent: &FederatedIntent, player: &str) -> Result<()> {
    let ticket = intent.ticket.to_string();
    if tx.execute(
        "UPDATE federated_queue_members SET released=1 WHERE ticket=? AND player=? AND released=0",
        params![ticket, player],
    )? != 0
    {
        event(
            tx,
            player,
            json!({"type":"post_match_left", "matchId":intent.match_id}),
        )?;
    }
    tx.execute(
        "DELETE FROM federated_queue WHERE ticket=?1 AND NOT EXISTS (
            SELECT 1 FROM federated_queue_members WHERE ticket=?1 AND released=0
         )",
        [&ticket],
    )?;
    Ok(())
}
