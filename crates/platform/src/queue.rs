use halcyon_community::{CommunityError, queue as community_queue};
use halcyon_data::{BotDifficulty, THREE_PER_SIDE_SEATS, three_per_side};
use rusqlite::{Transaction, params};
use serde_json::json;
use uuid::Uuid;

use super::{AppState, PlatformError, Result, announce, digest, event, player};

pub(super) fn add_difficulty_column(tx: &Transaction<'_>, table: &str) -> Result<()> {
    let exists: bool = tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM pragma_table_info(?) WHERE name='difficulty')",
        [table],
        |row| row.get(0),
    )?;
    if !exists {
        tx.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN difficulty TEXT"))?;
    }
    Ok(())
}

pub(super) fn read_difficulty(
    row: &rusqlite::Row<'_>,
    column: usize,
) -> rusqlite::Result<Option<BotDifficulty>> {
    row.get::<_, Option<String>>(column)?
        .map(|value| BotDifficulty::from_wire(&value).ok_or(rusqlite::Error::InvalidQuery))
        .transpose()
}

pub(super) fn validate_difficulty(mode: &str, difficulty: Option<BotDifficulty>) -> Result<()> {
    if difficulty.is_some() && !BotDifficulty::supports_queue(mode) {
        return Err(PlatformError::Invalid(
            "difficulty requires a supported bot queue",
        ));
    }
    Ok(())
}

pub(super) fn validate_mode(mode: &str, seats: usize) -> Result<()> {
    if !(1..=64).contains(&seats) {
        return Err(PlatformError::Invalid(
            "seat count must be between 1 and 64",
        ));
    }
    if three_per_side(mode) && seats != THREE_PER_SIDE_SEATS {
        return Err(PlatformError::Invalid(
            "three-per-side queue requires six human seats",
        ));
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

pub(super) fn community_error(error: CommunityError) -> PlatformError {
    match error {
        CommunityError::Forbidden => {
            PlatformError::Conflict("only the party leader can enter matchmaking")
        }
        CommunityError::Storage(error) => PlatformError::Database(error),
        CommunityError::Invalid(message) => PlatformError::Invalid(message),
        CommunityError::Conflict(message) | CommunityError::Missing(message) => {
            PlatformError::Conflict(message)
        }
        CommunityError::WorkerUnavailable => PlatformError::WorkerUnavailable,
    }
}

pub(super) struct Roster {
    pub(super) party: Option<String>,
    pub(super) players: Vec<String>,
}

impl Roster {
    pub(super) fn select(
        tx: &Transaction<'_>,
        actor: String,
        mode: &str,
        seats: usize,
        with_party: bool,
    ) -> Result<Self> {
        if with_party
            && let Some(party) =
                community_queue::roster(tx, &actor, mode).map_err(community_error)?
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

    pub(super) fn ensure_free(&self, tx: &Transaction<'_>) -> Result<()> {
        let mut query = tx.prepare(
            "SELECT EXISTS(SELECT 1 FROM players WHERE id=?1),
                    EXISTS(SELECT 1 FROM tickets WHERE player=?1)
                    OR EXISTS(SELECT 1 FROM federated_queue_members WHERE player=?1 AND released=0)
                    OR EXISTS(SELECT 1 FROM spectator_sessions WHERE player=?1)
                    OR EXISTS(SELECT 1 FROM pooled_spectators WHERE player=?1)",
        )?;
        for member in &self.players {
            let (registered, occupied): (bool, bool) =
                query.query_row([member], |row| Ok((row.get(0)?, row.get(1)?)))?;
            if !registered {
                return Err(PlatformError::Conflict(
                    "party member is not a platform player",
                ));
            }
            if occupied {
                return Err(PlatformError::Conflict(
                    "a player is already queued or in a match",
                ));
            }
        }
        Ok(())
    }

    pub(super) fn claim(&self, tx: &Transaction<'_>, ticket: &str) -> Result<()> {
        if let Some(party) = self.party.as_deref() {
            community_queue::claim(tx, party, ticket).map_err(community_error)?;
        }
        Ok(())
    }
}

pub(super) fn release_matches(tx: &Transaction<'_>, match_id: &str) -> Result<()> {
    tx.execute(
        "DELETE FROM community_party_queues WHERE ticket IN (
            SELECT batch FROM tickets WHERE match_id=? AND batch IS NOT NULL
        )",
        [match_id],
    )?;
    Ok(())
}

impl AppState {
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
            state.transaction(|tx| {
                let actor = player(tx, &key)?;
                let roster = Roster::select(tx, actor, &mode, seats, with_party)?;
                roster.ensure_free(tx)?;
                let batch = roster.party.as_ref().map(|_| Uuid::new_v4().to_string());
                if let Some(batch) = &batch {
                    roster.claim(tx, batch)?;
                }
                let queued_at_ms: i64 = tx.query_row(
                    "SELECT CAST(unixepoch('subsec')*1000 AS INTEGER)",
                    [],
                    |row| row.get(0),
                )?;
                for participant in roster.players {
                    tx.execute(
                        "INSERT INTO tickets(player, mode, batch, seat_limit, difficulty, queued_at_ms) VALUES(?, ?, ?, ?, ?, ?)",
                        params![participant, mode, batch, seats, difficulty, queued_at_ms],
                    )?;
                    event(tx, &participant, json!({"type":"queued", "mode":mode}))?;
                }
                if let Some(participants) = find_group(tx, &mode, seats, state.seats_per_match(), difficulty)? {
                    let match_id = Uuid::new_v4().to_string();
                    tx.execute(
                        "INSERT INTO matches(id, mode, phase, difficulty) VALUES(?, ?, 'offered', ?)",
                        params![match_id, mode, difficulty],
                    )?;
                    for participant in participants {
                        tx.execute(
                            "UPDATE tickets SET match_id=? WHERE player=?",
                            params![match_id, participant],
                        )?;
                    }
                    announce(
                        tx,
                        &match_id,
                        json!({"type":"match_offered", "matchId":match_id, "mode":mode}),
                    )?;
                }
                Ok(())
            })
        })
        .await
    }
}

fn find_group(
    tx: &Transaction<'_>,
    mode: &str,
    seats: usize,
    legacy_seats: usize,
    difficulty: Option<&str>,
) -> Result<Option<Vec<String>>> {
    let mut query = tx.prepare(
        "SELECT player, batch FROM tickets WHERE mode=?1 AND match_id IS NULL
         AND coalesce(seat_limit, ?2)=?3 AND difficulty IS ?4 ORDER BY serial",
    )?;
    let rows = query.query_map(params![mode, legacy_seats, seats, difficulty], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
    })?;
    let mut batches: Vec<(Option<String>, Vec<String>)> = Vec::new();
    for row in rows {
        let (player, batch) = row?;
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

pub(super) fn match_players(
    tx: &Transaction<'_>,
    match_id: &str,
    mode: &str,
) -> Result<(Vec<Uuid>, Vec<u8>, bool)> {
    let mut query =
        tx.prepare("SELECT player, batch, accepted FROM tickets WHERE match_id=? ORDER BY serial")?;
    let rows = query.query_map([match_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, bool>(2)?,
        ))
    })?;
    let mut previous = None;
    let mut sizes = Vec::<usize>::new();
    let mut players = Vec::new();
    let mut confirmed = true;
    for row in rows {
        let (player, batch, accepted) = row?;
        if batch.is_some()
            && batch == previous
            && let Some(size) = sizes.last_mut()
        {
            *size += 1;
        } else {
            sizes.push(1);
        }
        previous = batch;
        players.push(super::parse_id(&player)?);
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
    let side_capacity = if three_per_side(mode) {
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
