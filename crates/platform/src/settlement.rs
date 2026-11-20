use diesel::{
    dsl::{exists, not},
    prelude::*,
    sqlite::SqliteConnection,
};

use crate::{AppState, EventBody, LobbyPhase, PlatformError, Result, accounts, digest, parse_id};

use crate::orm_schema::{events, federated_queue, federated_queue_members, matches, tickets};

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
    /// Acknowledge a result without cancelling a pending match or queue.
    pub async fn acknowledge_post_match(&self, token: &str) -> Result<()> {
        let key = digest(token);
        self.run(move |state| {
            let acknowledged = state
                .accounts
                .lock()
                .map_err(|_| PlatformError::WorkerUnavailable)?
                .write_as(&key, acknowledge)?;
            if acknowledged {
                state.wake_notification_readers();
            }
            Ok(())
        })
        .await
    }
}

fn acknowledge(db: &mut SqliteConnection, player: &str) -> Result<bool> {
    use federated_queue_members as members;

    let conflict = || PlatformError::Conflict("no finished match to acknowledge");
    if let Some((ticket, phase, room)) = members::table
        .inner_join(federated_queue::table)
        .filter(members::player.eq(player))
        .filter(members::released.eq(false))
        .select((
            federated_queue::ticket,
            federated_queue::phase,
            federated_queue::match_id,
        ))
        .first::<(String, String, Option<String>)>(db)
        .optional()?
    {
        match LobbyPhase::parse(&phase) {
            Some(LobbyPhase::Closed) => return Ok(false),
            Some(LobbyPhase::Finished) => (),
            _ => return Err(conflict()),
        }
        diesel::update(members::table.find((&ticket, player)))
            .set(members::released.eq(true))
            .execute(db)?;
        post_match_left(db, player, room.as_deref())?;
        diesel::delete(
            federated_queue::table.find(&ticket).filter(not(exists(
                members::table
                    .filter(members::ticket.eq(&ticket))
                    .filter(members::released.eq(false)),
            ))),
        )
        .execute(db)?;
        return Ok(true);
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
