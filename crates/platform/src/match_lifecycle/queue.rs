use diesel::{
    OptionalExtension, RunQueryDsl,
    dsl::exists,
    prelude::*,
    sql_types::{BigInt, Text},
    sqlite::SqliteConnection,
};
use halcyon_community::{CommunityError, queue as community_queue};
use halcyon_data::{BotDifficulty, THREE_PER_SIDE_SEATS, three_per_side_map};
use halcyon_types::Reservation;
use uuid::Uuid;

use crate::{
    AbortReason, EventBody, LobbyPhase, PlatformError, Result,
    events::orm_schema::{
        community_party_queues, federated_queue_members, matches, players, pooled_spectators,
        spectator_sessions, tickets,
    },
    service::{AppState, abort, announce, digest, insert_event, parse_id, player},
};

diesel::define_sql_function!(#[sql_name = "unixepoch"] fn unixepoch_after(base: Text, modifier: Text) -> BigInt);

#[derive(QueryableByName)]
struct NowMs {
    #[diesel(sql_type = BigInt)]
    ms: i64,
}

pub(crate) fn validate_difficulty(mode: &str, difficulty: Option<BotDifficulty>) -> Result<()> {
    if difficulty.is_some() && !BotDifficulty::supports_queue(mode) {
        return Err(PlatformError::Invalid(
            "difficulty requires a supported bot queue",
        ));
    }
    Ok(())
}

pub(crate) fn validate_mode(mode: &str, seats: usize) -> Result<()> {
    if !(1..=64).contains(&seats) {
        return Err(PlatformError::Invalid(
            "seat count must be between 1 and 64",
        ));
    }
    if let Some(queue) = halcyon_data::KnownQueue::parse(mode) {
        if queue.is_single_human() && seats != 1 {
            return Err(PlatformError::Invalid(
                "solo bot or practice queue requires one human seat",
            ));
        }
        if queue.requires_six_humans() && seats != THREE_PER_SIDE_SEATS {
            return Err(PlatformError::Invalid(
                "three-per-side queue requires six human seats",
            ));
        }
    }
    if mode.is_empty()
        || mode.len() > 32
        || !mode
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Err(PlatformError::Invalid("invalid mode"));
    }
    Ok(())
}

pub(crate) fn community_error(error: CommunityError) -> PlatformError {
    match error {
        CommunityError::Forbidden => {
            PlatformError::Conflict("only the party leader can enter matchmaking")
        }
        CommunityError::Storage(error) => PlatformError::Database(error),
        CommunityError::Invalid(message) => PlatformError::Invalid(message),
        CommunityError::Conflict(message) | CommunityError::Missing(message) => {
            PlatformError::Conflict(message)
        }
        CommunityError::Serialization(error) => {
            tracing::error!(%error, "could not encode community response");
            PlatformError::WorkerUnavailable
        }
        CommunityError::WorkerUnavailable => PlatformError::WorkerUnavailable,
    }
}

pub(crate) fn parse_difficulty(raw: Option<String>) -> Result<Option<BotDifficulty>> {
    raw.map(|value| BotDifficulty::from_wire(&value).ok_or(PlatformError::WorkerUnavailable))
        .transpose()
}

pub(crate) struct Roster {
    pub(crate) party: Option<String>,
    pub(crate) players: Vec<String>,
}

impl Roster {
    pub(crate) fn select(
        db: &mut SqliteConnection,
        actor: String,
        mode: &str,
        seats: usize,
        with_party: bool,
    ) -> Result<Self> {
        if with_party
            && let Some(party) =
                community_queue::roster(db, &actor, mode).map_err(community_error)?
        {
            if party.players.len() > seats {
                return Err(PlatformError::Conflict("party is larger than this queue"));
            }
            return Ok(Self {
                party: Some(party.id),
                players: party.players,
            });
        }
        Ok(Self {
            party: None,
            players: vec![actor],
        })
    }

    pub(crate) fn ensure_free(&self, db: &mut SqliteConnection) -> Result<()> {
        for member in &self.players {
            let registered =
                diesel::select(exists(players::table.find(member))).get_result::<bool>(db)?;
            if !registered {
                return Err(PlatformError::Conflict(
                    "party member is not a platform player",
                ));
            }
            let occupied =
                diesel::select(exists(tickets::table.filter(tickets::player.eq(member))))
                    .get_result::<bool>(db)?
                    || diesel::select(exists(
                        federated_queue_members::table
                            .filter(federated_queue_members::player.eq(member))
                            .filter(federated_queue_members::released.eq(false)),
                    ))
                    .get_result::<bool>(db)?
                    || diesel::select(exists(spectator_sessions::table.find(member)))
                        .get_result::<bool>(db)?
                    || diesel::select(exists(pooled_spectators::table.find(member)))
                        .get_result::<bool>(db)?;
            if occupied {
                return Err(PlatformError::Conflict(
                    "a player is already queued or in a match",
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn claim(&self, db: &mut SqliteConnection, ticket: &str) -> Result<()> {
        if let Some(party) = self.party.as_deref() {
            community_queue::claim(db, party, ticket).map_err(community_error)?;
        }
        Ok(())
    }
}

pub(crate) fn release_matches(db: &mut SqliteConnection, match_id: &str) -> Result<()> {
    diesel::delete(
        community_party_queues::table.filter(
            community_party_queues::ticket.eq_any(
                tickets::table
                    .filter(tickets::match_id.eq(match_id))
                    .filter(tickets::batch.is_not_null())
                    .select(tickets::batch.assume_not_null()),
            ),
        ),
    )
    .execute(db)?;
    Ok(())
}

impl AppState {
    pub async fn join(&self, session_token: &str, mode: String) -> Result<()> {
        let seats = self.seats_for_mode(&mode);
        self.join_with_seats(session_token, mode, seats).await
    }

    pub async fn join_with_seats(
        &self,
        session_token: &str,
        mode: String,
        seats: usize,
    ) -> Result<()> {
        self.join_local(session_token, mode, seats, None, false)
            .await
    }

    pub async fn join_with_difficulty(
        &self,
        session_token: &str,
        mode: String,
        seats: usize,
        difficulty: Option<BotDifficulty>,
    ) -> Result<()> {
        self.join_local(session_token, mode, seats, difficulty, false)
            .await
    }

    /// Enqueue the authenticated leader's entire party or that player alone.
    /// The community and platform must use the same SQLite database.
    pub async fn join_party_or_solo(
        &self,
        session_token: &str,
        mode: String,
        seats: usize,
    ) -> Result<()> {
        self.join_local(session_token, mode, seats, None, true)
            .await
    }

    pub async fn join_party_with_difficulty(
        &self,
        session_token: &str,
        mode: String,
        seats: usize,
        difficulty: Option<BotDifficulty>,
    ) -> Result<()> {
        self.join_local(session_token, mode, seats, difficulty, true)
            .await
    }

    async fn join_local(
        &self,
        session_token: &str,
        mode: String,
        seats: usize,
        difficulty: Option<BotDifficulty>,
        with_party: bool,
    ) -> Result<()> {
        validate_mode(&mode, seats)?;
        validate_difficulty(&mode, difficulty)?;
        let difficulty = difficulty.map(BotDifficulty::as_str);
        let key = digest(session_token);
        self.expire_offers().await?;
        self.run(move |state| {
            let observer = state.observer()?;
            state.transaction(|db| {
                let actor = player(db, &key)?;
                let roster = Roster::select(db, actor, &mode, seats, with_party)?;
                roster.ensure_free(db)?;
                let batch = roster.party.as_ref().map(|_| Uuid::new_v4().to_string());
                if let Some(batch) = &batch {
                    roster.claim(db, batch)?;
                }
                let queued_at_ms =
                    diesel::sql_query("SELECT CAST(unixepoch('subsec') * 1000 AS INTEGER) AS ms")
                        .get_result::<NowMs>(db)?
                        .ms;
                let seat_limit =
                    i64::try_from(seats).map_err(|_| PlatformError::WorkerUnavailable)?;
                for participant in &roster.players {
                    diesel::insert_into(tickets::table)
                        .values((
                            tickets::player.eq(participant),
                            tickets::mode.eq(&mode),
                            tickets::batch.eq(batch.as_deref()),
                            tickets::seat_limit.eq(Some(seat_limit)),
                            tickets::difficulty.eq(difficulty),
                            tickets::queued_at_ms.eq(Some(queued_at_ms)),
                        ))
                        .execute(db)?;
                    insert_event(db, participant, EventBody::Queued { mode: &mode })?;
                }
                if let Some(obs) = &observer {
                    obs.on_queued(db, &roster.players, &mode)?;
                }
                if let Some(participants) =
                    find_group(db, &mode, seats, state.seats_per_match(), difficulty)?
                {
                    let match_uuid = Uuid::new_v4();
                    let match_id = match_uuid.to_string();
                    diesel::insert_into(matches::table)
                        .values((
                            matches::id.eq(&match_id),
                            matches::mode.eq(&mode),
                            matches::phase.eq("offered"),
                            matches::difficulty.eq(difficulty),
                        ))
                        .execute(db)?;
                    for participant in &participants {
                        diesel::update(tickets::table.filter(tickets::player.eq(participant)))
                            .set(tickets::match_id.eq(Some(&match_id)))
                            .execute(db)?;
                    }
                    announce(
                        db,
                        &match_id,
                        EventBody::MatchOffered {
                            match_id: match_uuid,
                            mode: &mode,
                        },
                    )?;
                    if let Some(obs) = &observer {
                        obs.on_match_offered(db, &participants)?;
                    }
                }
                Ok(())
            })
        })
        .await
    }

    pub async fn leave(&self, session_token: &str) -> Result<()> {
        let key = digest(session_token);
        self.run(move |state| {
            state.transaction(|db| {
                let id = player(db, &key)?;
                let lobby = tickets::table
                    .inner_join(matches::table)
                    .filter(tickets::player.eq(&id))
                    .select((matches::id, matches::phase))
                    .first::<(String, String)>(db)
                    .optional()?;
                if let Some((match_id, phase)) = lobby {
                    match LobbyPhase::parse(&phase) {
                        Some(LobbyPhase::Offered) => abort(db, &match_id, AbortReason::PlayerLeft)?,
                        Some(LobbyPhase::Finished) => acknowledge_local_result(db, &id, &match_id)?,
                        _ => {
                            return Err(PlatformError::Conflict(
                                "match is already starting or playing",
                            ));
                        }
                    }
                } else {
                    let batch = tickets::table
                        .filter(tickets::player.eq(&id))
                        .select(tickets::batch)
                        .first::<Option<String>>(db)
                        .optional()?
                        .flatten();
                    if let Some(batch) = batch {
                        let members = tickets::table
                            .filter(tickets::batch.eq(&batch))
                            .order(tickets::serial)
                            .select(tickets::player)
                            .load::<String>(db)?;
                        if !members.is_empty() {
                            let encoded = EventBody::QueueLeft.encode()?;
                            let rows: Vec<_> = members
                                .iter()
                                .map(|member| {
                                    (
                                        crate::events::orm_schema::events::player.eq(member),
                                        crate::events::orm_schema::events::body.eq(&encoded),
                                    )
                                })
                                .collect();
                            diesel::insert_into(crate::events::orm_schema::events::table)
                                .values(&rows)
                                .execute(db)?;
                        }
                        diesel::delete(tickets::table.filter(tickets::batch.eq(&batch)))
                            .execute(db)?;
                        community_queue::release(db, &batch).map_err(community_error)?;
                    } else if diesel::delete(tickets::table.filter(tickets::player.eq(&id)))
                        .execute(db)?
                        != 0
                    {
                        insert_event(db, &id, EventBody::QueueLeft)?;
                    }
                }
                Ok(())
            })
        })
        .await
    }

    pub async fn accept(&self, session_token: &str, match_uuid: Uuid) -> Result<()> {
        let key = digest(session_token);
        let match_id = match_uuid.to_string();
        self.expire_offers().await?;
        self.run(move |state| {
            let reservation = state.transaction(|db| {
                let id = player(db, &key)?;
                let (mode, phase, difficulty) = tickets::table
                    .inner_join(matches::table)
                    .filter(tickets::player.eq(&id))
                    .filter(matches::id.eq(&match_id))
                    .filter(
                        matches::phase
                            .ne("offered")
                            .or(matches::offered_at.gt(unixepoch_after("now", "-30 seconds"))),
                    )
                    .select((matches::mode, matches::phase, matches::difficulty))
                    .first::<(String, String, Option<String>)>(db)
                    .optional()?
                    .ok_or(PlatformError::Conflict("no offer for this match"))?;
                let difficulty = parse_difficulty(difficulty)?;
                match LobbyPhase::parse(&phase) {
                    Some(LobbyPhase::Playing | LobbyPhase::Reserving) => return Ok(None),
                    Some(LobbyPhase::Offered) => {}
                    _ => return Err(PlatformError::Conflict("match is already starting")),
                }
                diesel::update(tickets::table.filter(tickets::player.eq(&id)))
                    .set(tickets::accepted.eq(true))
                    .execute(db)?;
                let (players, teams, confirmed) = match_players(db, &match_id, &mode)?;
                if !confirmed {
                    return Ok(None);
                }
                let talent_levels = state.resolve_talent_levels(db, &mode, &players)?;
                let stored = serde_json::to_string(&talent_levels)
                    .map_err(|_| PlatformError::WorkerUnavailable)?;
                diesel::update(matches::table.find(&match_id))
                    .set((
                        matches::phase.eq("reserving"),
                        matches::talent_levels.eq(&stored),
                    ))
                    .execute(db)?;
                Ok(Some(Reservation {
                    match_id: match_uuid,
                    players,
                    teams,
                    mode,
                    difficulty,
                    talent_levels,
                }))
            })?;
            if let Some(reservation) = reservation {
                state.reserve_match(reservation)?;
            }
            Ok(())
        })
        .await
    }

    pub async fn send_chat(&self, session_token: &str, id: Uuid, message: String) -> Result<()> {
        let key = digest(session_token);
        let message = message.trim().to_owned();
        if message.is_empty() || message.len() > 512 || message.chars().any(char::is_control) {
            return Err(PlatformError::Invalid("invalid message"));
        }
        self.run(move |state| {
            state.transaction(|db| {
                let player_id = player(db, &key)?;
                let match_id = id.to_string();
                let active = diesel::select(exists(
                    tickets::table
                        .filter(tickets::player.eq(&player_id))
                        .filter(tickets::match_id.eq(&match_id)),
                ))
                .get_result::<bool>(db)?;
                if !active {
                    return Err(PlatformError::Conflict("no active seat in this match"));
                }
                announce(
                    db,
                    &match_id,
                    EventBody::Chat {
                        match_id: id,
                        player_id: parse_id(&player_id)?,
                        text: &message,
                    },
                )?;
                Ok(())
            })
        })
        .await
    }
}

fn acknowledge_local_result(db: &mut SqliteConnection, id: &str, match_id: &str) -> Result<()> {
    diesel::delete(tickets::table.filter(tickets::player.eq(id))).execute(db)?;
    insert_event(
        db,
        id,
        EventBody::PostMatchLeft {
            match_id: Some(parse_id(match_id)?),
        },
    )?;
    Ok(())
}

fn find_group(
    db: &mut SqliteConnection,
    mode: &str,
    seats: usize,
    legacy_seats: usize,
    difficulty: Option<&str>,
) -> Result<Option<Vec<String>>> {
    let legacy = i64::try_from(legacy_seats).map_err(|_| PlatformError::WorkerUnavailable)?;
    let target = i64::try_from(seats).map_err(|_| PlatformError::WorkerUnavailable)?;
    let mut query = tickets::table
        .filter(tickets::mode.eq(mode))
        .filter(tickets::match_id.is_null())
        .into_boxed();
    query = if legacy == target {
        query.filter(
            tickets::seat_limit
                .eq(target)
                .or(tickets::seat_limit.is_null()),
        )
    } else {
        query.filter(tickets::seat_limit.eq(target))
    };
    query = match difficulty {
        Some(diff) => query.filter(tickets::difficulty.eq(diff)),
        None => query.filter(tickets::difficulty.is_null()),
    };
    let rows = query
        .order(tickets::serial)
        .select((tickets::player, tickets::batch))
        .load::<(String, Option<String>)>(db)?;
    let mut batches: Vec<(Option<String>, Vec<String>)> = Vec::new();
    for (player, batch) in rows {
        // A party's rows are always inserted contiguously in one transaction.
        if let Some((previous, players)) = batches.last_mut()
            && batch.is_some()
            && *previous == batch
        {
            players.push(player);
        } else {
            batches.push((batch, vec![player]));
        }
    }
    let mut exact: Vec<Option<Vec<String>>> = vec![None; seats + 1];
    exact[0] = Some(Vec::new());
    for (_, batch) in batches {
        let count = batch.len();
        if count > seats {
            continue;
        }
        for used in (0..=seats - count).rev() {
            if exact[used + count].is_none()
                && let Some(selected) = &exact[used]
            {
                let mut result = selected.clone();
                result.extend(batch.iter().cloned());
                exact[used + count] = Some(result);
            }
        }
        if exact[seats].is_some() {
            break;
        }
    }
    Ok(exact.pop().flatten())
}

pub(crate) fn match_players(
    db: &mut SqliteConnection,
    match_id: &str,
    mode: &str,
) -> Result<(Vec<Uuid>, Vec<u8>, bool)> {
    let rows = tickets::table
        .filter(tickets::match_id.eq(match_id))
        .order(tickets::serial)
        .select((tickets::player, tickets::batch, tickets::accepted))
        .load::<(String, Option<String>, bool)>(db)?;
    let mut previous = None;
    let mut sizes = Vec::<usize>::new();
    let mut players = Vec::new();
    let mut confirmed = true;
    for (player, batch, accepted) in rows {
        if batch.is_some()
            && batch == previous
            && let Some(size) = sizes.last_mut()
        {
            *size += 1;
        } else {
            sizes.push(1);
        }
        previous = batch;
        players.push(parse_id(&player)?);
        confirmed &= accepted;
    }
    Ok((players, plan_teams_for_mode(mode, &sizes), confirmed))
}

/// Return a stable side for each human in contiguous party rosters.
/// A room's sides each hold at most five humans. Among feasible layouts,
/// minimize split parties first and then minimize the difference in human counts.
/// Solo-only rooms retain their existing alternating seat assignment.
pub fn plan_teams(party_sizes: &[usize]) -> Vec<u8> {
    plan_sides(party_sizes, 5)
}

pub fn plan_teams_for_mode(mode: &str, party_sizes: &[usize]) -> Vec<u8> {
    let side_capacity = if three_per_side_map(mode) {
        THREE_PER_SIDE_SEATS / 2
    } else {
        5
    };
    plan_sides(party_sizes, side_capacity)
}

fn plan_sides(party_sizes: &[usize], side_capacity: usize) -> Vec<u8> {
    if party_sizes.iter().all(|&size| size == 1)
        || party_sizes
            .iter()
            .any(|&size| size == 0 || size > 2 * side_capacity)
        || party_sizes.iter().sum::<usize>() > 2 * side_capacity
    {
        return Vec::new();
    }
    let mut placed = 0;
    type Layout = (usize, Vec<u8>);
    let mut routes: [Option<Layout>; 6] = Default::default();
    routes[0] = Some((0, Vec::new()));
    for &size in party_sizes {
        let mut next: [Option<Layout>; 6] = Default::default();
        for (left, entry) in routes.iter().enumerate().rev() {
            let Some((cost, assignments)) = entry else {
                continue;
            };
            let right = placed - left;
            for add_left in (0..=size.min(side_capacity - left)).rev() {
                let add_right = size - add_left;
                if right + add_right > side_capacity {
                    continue;
                }
                let new_cost = cost + usize::from(add_left > 0 && add_right > 0);
                let option = &mut next[left + add_left];
                if option.as_ref().is_none_or(|(best, _)| new_cost < *best) {
                    let mut new = assignments.clone();
                    new.extend(std::iter::repeat_n(0, add_left));
                    new.extend(std::iter::repeat_n(1, add_right));
                    *option = Some((new_cost, new));
                }
            }
        }
        routes = next;
        placed += size;
    }
    routes
        .into_iter()
        .enumerate()
        .rev()
        .filter_map(|(left, entry)| {
            entry.map(|(cost, layout)| ((cost, left.abs_diff(placed - left)), layout))
        })
        .min_by_key(|(rank, _)| *rank)
        .map_or_else(Vec::new, |(_, layout)| layout)
}
