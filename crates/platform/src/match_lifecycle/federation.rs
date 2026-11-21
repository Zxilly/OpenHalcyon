use std::collections::BTreeMap;

use diesel::{RunQueryDsl, dsl::exists, prelude::*, sql_types::BigInt, sqlite::SqliteConnection};
use halcyon_data::BotDifficulty;
use serde_json::Value;
use uuid::Uuid;

use crate::{
    EventBody, LobbyPhase, PlatformError, Result,
    events::orm_schema::{
        community_party_queues, federated_queue, federated_queue_members, federated_site_reports,
        federated_talent_levels,
    },
    match_lifecycle::queue::{Roster, parse_difficulty, validate_difficulty, validate_mode},
    service::{AppState, digest, insert_event, parse_id, player},
};

diesel::define_sql_function!(fn unixepoch() -> BigInt);

#[derive(QueryableByName)]
struct NowMs {
    #[diesel(sql_type = BigInt)]
    ms: i64,
}

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

impl FederatedIntent {
    /// Unknown persisted phases remain available in `phase` and do not authorize seat actions.
    pub fn phase_kind(&self) -> Option<LobbyPhase> {
        LobbyPhase::parse(&self.phase)
    }
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = federated_queue)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct QueueRow {
    ticket: String,
    player: String,
    pool: String,
    mode: String,
    seats: i64,
    accepted_match: Option<String>,
    cancel: bool,
    phase: String,
    match_id: Option<String>,
    address: Option<String>,
    result: Option<String>,
    enqueue_schema: i64,
    difficulty: Option<String>,
}

fn hydrate_intent(db: &mut SqliteConnection, row: QueueRow) -> Result<FederatedIntent> {
    let member_strs = federated_queue_members::table
        .filter(federated_queue_members::ticket.eq(&row.ticket))
        .order(federated_queue_members::pos)
        .select(federated_queue_members::player)
        .load::<String>(db)?;
    let players = member_strs
        .iter()
        .map(|s| parse_id(s))
        .collect::<Result<Vec<_>>>()?;
    let estimates_raw = federated_site_reports::table
        .find(&row.ticket)
        .select(federated_site_reports::estimates)
        .first::<String>(db)
        .optional()?;
    let reported_ms = estimates_raw
        .as_deref()
        .map(|s| serde_json::from_str(s).map_err(|_| PlatformError::WorkerUnavailable))
        .transpose()?
        .unwrap_or_default();
    let seats = u8::try_from(row.seats).map_err(|_| PlatformError::WorkerUnavailable)?;
    Ok(FederatedIntent {
        ticket: parse_id(&row.ticket)?,
        player: parse_id(&row.player)?,
        pool: parse_id(&row.pool)?,
        mode: row.mode,
        seats,
        accepted_match: row.accepted_match.as_deref().map(parse_id).transpose()?,
        cancel: row.cancel,
        phase: row.phase,
        match_id: row.match_id.as_deref().map(parse_id).transpose()?,
        address: row.address,
        result: row
            .result
            .as_deref()
            .map(|s| serde_json::from_str(s).map_err(|_| PlatformError::WorkerUnavailable))
            .transpose()?,
        players,
        pre_freeze: row.enqueue_schema == 0,
        difficulty: parse_difficulty(row.difficulty)?,
        reported_ms,
    })
}

fn intent_for_player(db: &mut SqliteConnection, player: &str) -> Result<Option<FederatedIntent>> {
    let row = federated_queue::table
        .inner_join(federated_queue_members::table)
        .filter(federated_queue_members::player.eq(player))
        .filter(federated_queue_members::released.eq(false))
        .select(QueueRow::as_select())
        .first::<QueueRow>(db)
        .optional()?;
    row.map(|r| hydrate_intent(db, r)).transpose()
}

fn intent_by_ticket(db: &mut SqliteConnection, ticket: &str) -> Result<Option<FederatedIntent>> {
    let row = federated_queue::table
        .find(ticket)
        .select(QueueRow::as_select())
        .first::<QueueRow>(db)
        .optional()?;
    row.map(|r| hydrate_intent(db, r)).transpose()
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
        self.run(move |state| {
            state.transaction(|db| {
                let id = player(db, &key)?;
                let roster = Roster::select(db, id.clone(), &mode, usize::from(seats), with_party)?;
                if let Some(existing) = intent_for_player(db, &id)? {
                    let claimed_party = community_party_queues::table
                        .filter(community_party_queues::ticket.eq(existing.ticket.to_string()))
                        .select(community_party_queues::party)
                        .first::<String>(db)
                        .optional()?;
                    if existing.player.to_string() == id
                        && existing.pool == pool
                        && existing.mode == mode
                        && existing.seats == seats
                        && existing.difficulty == difficulty
                        && matches!(
                            existing.phase_kind(),
                            Some(LobbyPhase::Pending | LobbyPhase::Queued)
                        )
                        && !existing.cancel
                        && claimed_party == roster.party
                        && existing
                            .players
                            .iter()
                            .map(Uuid::to_string)
                            .eq(roster.players.iter().cloned())
                    {
                        return Ok(existing);
                    }
                    return Err(PlatformError::Conflict("player has a federated ticket"));
                }
                roster.ensure_free(db)?;
                let ticket = Uuid::new_v4().to_string();
                roster.claim(db, &ticket)?;
                let queued_at_ms =
                    diesel::sql_query("SELECT CAST(unixepoch('subsec') * 1000 AS INTEGER) AS ms")
                        .get_result::<NowMs>(db)?
                        .ms;
                diesel::insert_into(federated_queue::table)
                    .values((
                        federated_queue::ticket.eq(&ticket),
                        federated_queue::player.eq(&id),
                        federated_queue::pool.eq(pool.to_string()),
                        federated_queue::mode.eq(&mode),
                        federated_queue::seats.eq(i64::from(seats)),
                        federated_queue::enqueue_schema.eq(1_i64),
                        federated_queue::difficulty.eq(difficulty.map(BotDifficulty::as_str)),
                        federated_queue::queued_at_ms.eq(Some(queued_at_ms)),
                    ))
                    .execute(db)?;
                if !estimates.is_empty() {
                    let encoded = serde_json::to_string(&estimates)
                        .map_err(|_| PlatformError::WorkerUnavailable)?;
                    diesel::insert_into(federated_site_reports::table)
                        .values((
                            federated_site_reports::ticket.eq(&ticket),
                            federated_site_reports::estimates.eq(&encoded),
                        ))
                        .execute(db)?;
                }
                for (index, member) in roster.players.iter().enumerate() {
                    let pos = i64::try_from(index).map_err(|_| PlatformError::WorkerUnavailable)?;
                    diesel::insert_into(federated_queue_members::table)
                        .values((
                            federated_queue_members::ticket.eq(&ticket),
                            federated_queue_members::player.eq(member),
                            federated_queue_members::pos.eq(pos),
                        ))
                        .execute(db)?;
                }
                let intent =
                    intent_by_ticket(db, &ticket)?.ok_or(PlatformError::WorkerUnavailable)?;
                let levels = state.resolve_talent_levels(db, &intent.mode, &intent.players)?;
                if !levels.is_empty() {
                    let levels = serde_json::to_string(&levels)
                        .map_err(|_| PlatformError::WorkerUnavailable)?;
                    diesel::insert_into(federated_talent_levels::table)
                        .values((
                            federated_talent_levels::ticket.eq(&ticket),
                            federated_talent_levels::levels.eq(&levels),
                        ))
                        .execute(db)?;
                }
                Ok(intent)
            })
        })
        .await
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
            state.read_and_touch_as(&key, |db, player| {
                let binding = federated_queue_members::table
                    .inner_join(federated_queue::table)
                    .filter(federated_queue_members::player.eq(player))
                    .filter(federated_queue_members::released.eq(false))
                    .filter(federated_queue::pool.eq(pool.to_string()))
                    .filter(federated_queue::phase.eq_any(["offered", "playing", "finished"]))
                    .filter(federated_queue::match_id.is_not_null())
                    .select((
                        federated_queue::ticket,
                        federated_queue::match_id.assume_not_null(),
                    ))
                    .first::<(String, String)>(db)
                    .optional()?;
                let Some((ticket, room)) = binding else {
                    return Ok(None);
                };
                let rows = federated_queue_members::table
                    .filter(federated_queue_members::ticket.eq(&ticket))
                    .filter(federated_queue_members::accepted_match.eq(&room))
                    .select(federated_queue_members::player)
                    .load::<String>(db)?;
                let mut accepted = std::collections::BTreeSet::new();
                for id in rows {
                    accepted.insert(parse_id(&id)?);
                }
                Ok(Some(FederatedOfferBinding {
                    player: parse_id(player)?,
                    ticket: parse_id(&ticket)?,
                    room: parse_id(&room)?,
                    accepted,
                }))
            })
        })
        .await
    }

    pub async fn federated_intent(&self, token: &str) -> Result<Option<FederatedIntent>> {
        let key = digest(token);
        self.run(move |state| state.read_and_touch_as(&key, intent_for_player))
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
            state.read(|db| {
                let mut query = federated_queue::table
                    .filter(federated_queue::pool.eq(pool.to_string()))
                    .into_boxed();
                if pending {
                    query = query.filter(federated_queue::phase.ne("finished"));
                }
                let rows = query
                    .order(federated_queue::ticket)
                    .select(QueueRow::as_select())
                    .load::<QueueRow>(db)?;
                rows.into_iter()
                    .map(|row| hydrate_intent(db, row))
                    .collect()
            })
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
            state.transaction(|db| {
                let id = player(db, &key)?;
                let Some(mut intent) =
                    intent_for_player(db, &id)?.filter(|intent| intent.pool == pool)
                else {
                    return Ok(None);
                };
                match intent.phase_kind() {
                    Some(LobbyPhase::Closed) => Ok(None),
                    Some(LobbyPhase::Finished) => {
                        acknowledge(db, &intent, &id)?;
                        Ok(None)
                    }
                    _ => {
                        apply_action(db, &intent, &id, None, true)?;
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
            state.transaction(|db| {
                let id = player(db, &key)?;
                let intent = intent_for_player(db, &id)?
                    .filter(|active| active.ticket.to_string() == ticket)
                    .ok_or(PlatformError::Conflict(
                        "federated ticket does not belong to player",
                    ))?;
                if let Some(deadline) = deadline {
                    let valid = diesel::select(unixepoch().lt(deadline)).get_result::<bool>(db)?;
                    if !valid {
                        return Err(PlatformError::Conflict("federated offer has expired"));
                    }
                }
                apply_action(db, &intent, &id, accept, cancel)?;
                intent_by_ticket(db, &ticket)?.ok_or(PlatformError::WorkerUnavailable)
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
            state.transaction(|db| {
                let intent = intent_by_ticket(db, &ticket)?
                    .ok_or(PlatformError::Conflict("federated ticket does not exist"))?;
                if intent.players.len() != 1 {
                    return Err(PlatformError::Conflict(
                        "party action requires an individual session",
                    ));
                }
                apply_action(db, &intent, &intent.player.to_string(), accept, cancel)
            })
        })
        .await
    }
}

fn apply_action(
    db: &mut SqliteConnection,
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
            || !matches!(
                intent.phase_kind(),
                Some(LobbyPhase::Offered | LobbyPhase::Playing)
            )
        {
            return Err(PlatformError::Conflict("no federated offer for this match"));
        }
        let id = match_id.to_string();
        diesel::update(
            federated_queue_members::table
                .filter(federated_queue_members::ticket.eq(&ticket))
                .filter(federated_queue_members::player.eq(actor))
                .filter(federated_queue_members::released.eq(false)),
        )
        .set(federated_queue_members::accepted_match.eq(Some(&id)))
        .execute(db)?;
        let unconfirmed = diesel::select(exists(
            federated_queue_members::table
                .filter(federated_queue_members::ticket.eq(&ticket))
                .filter(
                    federated_queue_members::accepted_match
                        .is_null()
                        .or(federated_queue_members::accepted_match.ne(&id)),
                ),
        ))
        .get_result::<bool>(db)?;
        if !unconfirmed {
            diesel::update(federated_queue::table.find(&ticket))
                .set(federated_queue::accepted_match.eq(Some(&id)))
                .execute(db)?;
        }
    } else if cancel {
        if !matches!(
            intent.phase_kind(),
            Some(LobbyPhase::Pending | LobbyPhase::Queued | LobbyPhase::Offered)
        ) {
            return Err(PlatformError::Conflict(
                "federated match has already started",
            ));
        }
        diesel::update(federated_queue::table.find(&ticket))
            .set(federated_queue::cancel.eq(true))
            .execute(db)?;
    } else {
        return Err(PlatformError::Invalid("no federated action"));
    }
    Ok(())
}

fn acknowledge(db: &mut SqliteConnection, intent: &FederatedIntent, player: &str) -> Result<()> {
    let ticket = intent.ticket.to_string();
    let released = diesel::update(
        federated_queue_members::table
            .filter(federated_queue_members::ticket.eq(&ticket))
            .filter(federated_queue_members::player.eq(player))
            .filter(federated_queue_members::released.eq(false)),
    )
    .set(federated_queue_members::released.eq(true))
    .execute(db)?;
    if released != 0 {
        insert_event(
            db,
            player,
            EventBody::PostMatchLeft {
                match_id: intent.match_id,
            },
        )?;
    }
    let remaining = diesel::select(exists(
        federated_queue_members::table
            .filter(federated_queue_members::ticket.eq(&ticket))
            .filter(federated_queue_members::released.eq(false)),
    ))
    .get_result::<bool>(db)?;
    if !remaining {
        diesel::delete(federated_queue::table.find(&ticket)).execute(db)?;
    }
    Ok(())
}
