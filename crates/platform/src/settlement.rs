use diesel::{
    dsl::{exists, not},
    prelude::*,
    sqlite::SqliteConnection,
};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    AbortReason, AppState, EventBody, LobbyPhase, PlatformError, Result, accounts, digest,
    orm_schema::{
        community_party_queues, events, federated_queue, federated_queue_members, matches,
        spectator_sessions, tickets,
    },
    parse_id,
};

impl From<halcyon_storage::Error> for PlatformError {
    fn from(error: halcyon_storage::Error) -> Self {
        accounts::storage_error(error)
    }
}

impl From<diesel::result::Error> for PlatformError {
    fn from(error: diesel::result::Error) -> Self {
        accounts::storage_error(error.into())
    }
}

impl AppState {
    fn write_settlement(
        &self,
        apply: impl FnOnce(&mut SqliteConnection) -> Result<bool>,
    ) -> Result<()> {
        let written = self
            .accounts
            .lock()
            .map_err(|_| PlatformError::WorkerUnavailable)?
            .write(apply)?;
        if written {
            self.wake_notification_readers();
        }
        Ok(())
    }

    fn write_settlement_as(
        &self,
        key: &[u8],
        apply: impl FnOnce(&mut SqliteConnection, &str) -> Result<bool>,
    ) -> Result<()> {
        let written = self
            .accounts
            .lock()
            .map_err(|_| PlatformError::WorkerUnavailable)?
            .write_as(key, apply)?;
        if written {
            self.wake_notification_readers();
        }
        Ok(())
    }

    /// Ends an active match after the trusted host confirms it can no longer run.
    pub fn abort_match(&self, match_id: Uuid) -> Result<()> {
        self.write_settlement(|db| abort_local_match(db, match_id))
    }

    /// Record a terminal result once. Each participant acknowledges it with leave.
    pub fn finish_match(&self, match_id: Uuid, result: Value) -> Result<()> {
        self.write_settlement(|db| finish_local_match(db, match_id, &result))
    }

    /// Acknowledge a result without cancelling a pending match or queue.
    pub async fn acknowledge_post_match(&self, token: &str) -> Result<()> {
        let key = digest(token);
        self.run(move |state| state.write_settlement_as(&key, acknowledge))
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
        let phase = LobbyPhase::parse(phase)
            .ok_or(PlatformError::Invalid("invalid federated projection"))?;
        match phase {
            LobbyPhase::Queued | LobbyPhase::Closed => {}
            LobbyPhase::Offered | LobbyPhase::Finished if match_id.is_some() => {}
            LobbyPhase::Playing
                if match_id.is_some() && address.as_ref().is_some_and(|a| !a.trim().is_empty()) => {
            }
            _ => return Err(PlatformError::Invalid("invalid federated projection")),
        }
        self.run(move |state| {
            state.write_settlement(|db| {
                project_federated(
                    db,
                    ticket,
                    phase,
                    match_id,
                    address.as_deref(),
                    result.as_ref(),
                )
            })
        })
        .await
    }

    /// Release one participant's local claim after they exit results.
    pub async fn federated_acknowledge(&self, token: &str, ticket: Uuid) -> Result<()> {
        let key = digest(token);
        self.run(move |state| {
            state.write_settlement_as(&key, |db, player| acknowledge_ticket(db, player, ticket))
        })
        .await
    }

    /// Release the entire closed ticket, or acknowledge a single-player result.
    pub async fn federated_release(&self, ticket: Uuid) -> Result<()> {
        self.run(move |state| state.write_settlement(|db| release_federated(db, ticket)))
            .await
    }
}

fn finish_local_match(db: &mut SqliteConnection, match_uuid: Uuid, result: &Value) -> Result<bool> {
    let match_id = match_uuid.to_string();
    let row = matches::table
        .find(&match_id)
        .select((matches::phase, matches::result))
        .first::<(String, Option<String>)>(db)
        .optional()?;
    let encoded_result = result.to_string();
    match row.map(|(phase, stored)| (LobbyPhase::parse(&phase), stored)) {
        Some((Some(LobbyPhase::Finished), stored)) => {
            if stored.as_deref() == Some(encoded_result.as_str()) {
                return Ok(false);
            }
            return Err(PlatformError::Conflict("match has a different result"));
        }
        Some((Some(LobbyPhase::Playing), _)) => {}
        _ => return Err(PlatformError::Conflict("match is not playing")),
    }
    diesel::update(matches::table.find(&match_id))
        .set((
            matches::phase.eq(LobbyPhase::Finished.as_str()),
            matches::result.eq(&encoded_result),
        ))
        .execute(db)?;
    release_match_parties(db, &match_id)?;
    close_spectators(db, match_uuid, &match_id)?;
    announce_match(
        db,
        &match_id,
        EventBody::MatchFinished {
            match_id: match_uuid,
            result: Some(result),
        },
    )?;
    Ok(true)
}

fn abort_local_match(db: &mut SqliteConnection, match_uuid: Uuid) -> Result<bool> {
    let match_id = match_uuid.to_string();
    let phase = matches::table
        .find(&match_id)
        .select(matches::phase)
        .first::<String>(db)
        .optional()?;
    match phase.as_deref().and_then(LobbyPhase::parse) {
        Some(LobbyPhase::Aborted) => return Ok(false),
        Some(LobbyPhase::Playing) => {}
        _ => return Err(PlatformError::Conflict("match is not playing")),
    }
    release_match_parties(db, &match_id)?;
    close_spectators(db, match_uuid, &match_id)?;
    diesel::update(matches::table.find(&match_id))
        .set(matches::phase.eq(LobbyPhase::Aborted.as_str()))
        .execute(db)?;
    announce_match(
        db,
        &match_id,
        EventBody::MatchAborted {
            match_id: match_uuid,
            reason: AbortReason::HostStopped,
        },
    )?;
    diesel::delete(tickets::table.filter(tickets::match_id.eq(&match_id))).execute(db)?;
    Ok(true)
}

fn project_federated(
    db: &mut SqliteConnection,
    ticket_uuid: Uuid,
    phase: LobbyPhase,
    match_id: Option<Uuid>,
    address: Option<&str>,
    result: Option<&Value>,
) -> Result<bool> {
    let ticket = ticket_uuid.to_string();
    let Some((mode, accepted_match, current_phase, current_match, current_address, current_result)) =
        federated_queue::table
            .find(&ticket)
            .select((
                federated_queue::mode,
                federated_queue::accepted_match,
                federated_queue::phase,
                federated_queue::match_id,
                federated_queue::address,
                federated_queue::result,
            ))
            .first::<(
                String,
                Option<String>,
                String,
                Option<String>,
                Option<String>,
                Option<String>,
            )>(db)
            .optional()?
    else {
        return Ok(false);
    };

    let previous = LobbyPhase::parse(&current_phase);
    let stored_match = current_match.as_deref().map(parse_id).transpose()?;
    let stored_accepted = accepted_match.as_deref().map(parse_id).transpose()?;
    let stored_result: Option<Value> = current_result
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|_| PlatformError::WorkerUnavailable)?;

    if previous == Some(phase)
        && stored_match == match_id
        && current_address.as_deref() == address
        && stored_result.as_ref() == result
    {
        return Ok(false);
    }
    if previous == Some(LobbyPhase::Closed)
        || (previous == Some(LobbyPhase::Finished) && phase != LobbyPhase::Closed)
        || (previous == Some(LobbyPhase::Playing)
            && !matches!(
                phase,
                LobbyPhase::Playing | LobbyPhase::Finished | LobbyPhase::Closed
            ))
    {
        return Err(PlatformError::Conflict("stale federated state"));
    }
    if matches!(previous, Some(LobbyPhase::Playing | LobbyPhase::Finished))
        && phase != LobbyPhase::Closed
        && match_id != stored_match
    {
        return Err(PlatformError::Conflict("federated match identity changed"));
    }

    let match_string = match_id.map(|id| id.to_string());
    let accepted = stored_accepted
        .filter(|id| Some(*id) == match_id)
        .map(|id| id.to_string());
    let encoded_result = result.map(Value::to_string);

    diesel::update(federated_queue::table.find(&ticket))
        .set((
            federated_queue::phase.eq(phase.as_str()),
            federated_queue::match_id.eq(match_string.as_deref()),
            federated_queue::address.eq(address),
            federated_queue::result.eq(encoded_result.as_deref()),
            federated_queue::accepted_match.eq(accepted.as_deref()),
        ))
        .execute(db)?;

    if stored_match != match_id {
        diesel::update(
            federated_queue_members::table.filter(federated_queue_members::ticket.eq(&ticket)),
        )
        .set(federated_queue_members::accepted_match.eq(None::<&str>))
        .execute(db)?;
    }
    if matches!(phase, LobbyPhase::Closed | LobbyPhase::Finished) {
        diesel::delete(
            community_party_queues::table.filter(community_party_queues::ticket.eq(&ticket)),
        )
        .execute(db)?;
    }
    let message = match phase {
        LobbyPhase::Queued => EventBody::Queued { mode: &mode },
        LobbyPhase::Offered => EventBody::MatchOffered {
            match_id: match_id.ok_or(PlatformError::HostUnavailable)?,
            mode: &mode,
        },
        LobbyPhase::Playing => EventBody::MatchPlaying {
            match_id: match_id.ok_or(PlatformError::HostUnavailable)?,
            address: address.ok_or(PlatformError::HostUnavailable)?,
        },
        LobbyPhase::Finished => EventBody::MatchFinished {
            match_id: match_id.ok_or(PlatformError::HostUnavailable)?,
            result,
        },
        LobbyPhase::Closed if stored_match.or(match_id).is_some() => EventBody::MatchAborted {
            match_id: match_id
                .or(stored_match)
                .ok_or(PlatformError::HostUnavailable)?,
            reason: AbortReason::FederationClosed,
        },
        _ => EventBody::QueueLeft,
    };
    let members = federated_queue_members::table
        .filter(federated_queue_members::ticket.eq(&ticket))
        .filter(federated_queue_members::released.eq(false))
        .order(federated_queue_members::pos)
        .select(federated_queue_members::player)
        .load::<String>(db)?;
    insert_events_for(db, &members, &message)?;
    Ok(true)
}

fn release_match_parties(db: &mut SqliteConnection, match_id: &str) -> Result<()> {
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

fn close_spectators(db: &mut SqliteConnection, match_uuid: Uuid, match_id: &str) -> Result<()> {
    let players = spectator_sessions::table
        .filter(spectator_sessions::match_id.eq(match_id))
        .filter(spectator_sessions::spectator_id.is_not_null())
        .order(spectator_sessions::player)
        .select(spectator_sessions::player)
        .load::<String>(db)?;
    insert_events_for(
        db,
        &players,
        &EventBody::SpectatorLeft {
            match_id: match_uuid,
        },
    )?;
    diesel::delete(spectator_sessions::table.filter(spectator_sessions::match_id.eq(match_id)))
        .execute(db)?;
    Ok(())
}

fn announce_match(db: &mut SqliteConnection, match_id: &str, body: EventBody<'_>) -> Result<()> {
    let players = tickets::table
        .filter(tickets::match_id.eq(match_id))
        .order(tickets::serial)
        .select(tickets::player)
        .load::<String>(db)?;
    insert_events_for(db, &players, &body)
}

fn insert_events_for(
    db: &mut SqliteConnection,
    players: &[String],
    body: &EventBody<'_>,
) -> Result<()> {
    let encoded = body.encode()?;
    if players.is_empty() {
        return Ok(());
    }
    let rows: Vec<_> = players
        .iter()
        .map(|player| (events::player.eq(player), events::body.eq(&encoded)))
        .collect();
    diesel::insert_into(events::table)
        .values(&rows)
        .execute(db)?;
    Ok(())
}

fn active_federated_ticket(
    db: &mut SqliteConnection,
    player: &str,
) -> Result<Option<(String, String, Option<String>)>> {
    use federated_queue_members as members;

    members::table
        .inner_join(federated_queue::table)
        .filter(members::player.eq(player))
        .filter(members::released.eq(false))
        .select((
            federated_queue::ticket,
            federated_queue::phase,
            federated_queue::match_id,
        ))
        .first::<(String, String, Option<String>)>(db)
        .optional()
        .map_err(Into::into)
}

fn acknowledge_ticket(db: &mut SqliteConnection, player: &str, ticket_uuid: Uuid) -> Result<bool> {
    let Some((ticket, phase, room)) = active_federated_ticket(db, player)? else {
        return Ok(false);
    };
    if parse_id(&ticket)? != ticket_uuid || LobbyPhase::parse(&phase) != Some(LobbyPhase::Finished)
    {
        return Err(PlatformError::Conflict("federated ticket is still active"));
    }
    acknowledge_federated(db, &ticket, player, room.as_deref())
}

fn release_federated(db: &mut SqliteConnection, ticket_uuid: Uuid) -> Result<bool> {
    let ticket = ticket_uuid.to_string();
    let Some((owner, phase, room)) = federated_queue::table
        .find(&ticket)
        .select((
            federated_queue::player,
            federated_queue::phase,
            federated_queue::match_id,
        ))
        .first::<(String, String, Option<String>)>(db)
        .optional()?
    else {
        return Ok(false);
    };
    let member_count = federated_queue_members::table
        .filter(federated_queue_members::ticket.eq(&ticket))
        .count()
        .get_result::<i64>(db)?;
    match LobbyPhase::parse(&phase) {
        Some(LobbyPhase::Closed) => {
            diesel::delete(
                community_party_queues::table.filter(community_party_queues::ticket.eq(&ticket)),
            )
            .execute(db)?;
            let deleted = diesel::delete(federated_queue::table.find(&ticket)).execute(db)?;
            Ok(deleted != 0)
        }
        Some(LobbyPhase::Finished) if member_count == 1 => {
            acknowledge_federated(db, &ticket, &owner, room.as_deref())
        }
        _ => Err(PlatformError::Conflict(
            "each party member must exit individually",
        )),
    }
}

fn acknowledge(db: &mut SqliteConnection, player: &str) -> Result<bool> {
    let conflict = || PlatformError::Conflict("no finished match to acknowledge");
    if let Some((ticket, phase, room)) = active_federated_ticket(db, player)? {
        match LobbyPhase::parse(&phase) {
            Some(LobbyPhase::Closed) => return Ok(false),
            Some(LobbyPhase::Finished) => (),
            _ => return Err(conflict()),
        }
        return acknowledge_federated(db, &ticket, player, room.as_deref());
    }

    let Some((room, phase)) = tickets::table
        .left_join(matches::table)
        .filter(tickets::player.eq(player))
        .select((matches::id.nullable(), matches::phase.nullable()))
        .first::<(Option<String>, Option<String>)>(db)
        .optional()?
    else {
        return Ok(false);
    };
    if phase.as_deref().and_then(LobbyPhase::parse) != Some(LobbyPhase::Finished) {
        return Err(conflict());
    }
    let Some(room) = room else {
        return Err(conflict());
    };
    diesel::delete(tickets::table.filter(tickets::player.eq(player))).execute(db)?;
    post_match_left(db, player, Some(&room))?;
    Ok(true)
}

fn acknowledge_federated(
    db: &mut SqliteConnection,
    ticket: &str,
    player: &str,
    room: Option<&str>,
) -> Result<bool> {
    use federated_queue_members as members;

    let updated = diesel::update(
        members::table
            .find((ticket, player))
            .filter(members::released.eq(false)),
    )
    .set(members::released.eq(true))
    .execute(db)?;
    if updated != 0 {
        post_match_left(db, player, room)?;
    }
    let deleted = diesel::delete(
        federated_queue::table.find(ticket).filter(not(exists(
            members::table
                .filter(members::ticket.eq(ticket))
                .filter(members::released.eq(false)),
        ))),
    )
    .execute(db)?;
    Ok(updated != 0 || deleted != 0)
}

fn post_match_left(db: &mut SqliteConnection, player: &str, room: Option<&str>) -> Result<()> {
    let body = EventBody::PostMatchLeft {
        match_id: room.map(parse_id).transpose()?,
    }
    .encode()?;
    diesel::insert_into(events::table)
        .values((events::player.eq(player), events::body.eq(body)))
        .execute(db)?;
    Ok(())
}
