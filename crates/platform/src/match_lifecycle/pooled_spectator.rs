use diesel::{dsl::exists, prelude::*, sqlite::SqliteConnection};
use halcyon_types::HostedSpectator;
use uuid::Uuid;

use crate::{
    EventBody, PlatformError, Result,
    events::orm_schema::{
        federated_queue, federated_queue_members, pooled_spectators, spectator_sessions, tickets,
    },
    match_lifecycle::queue::community_error,
    service::{AppState, digest, insert_event, parse_id, player},
};

#[derive(Debug)]
pub struct PooledSpectator {
    pub viewer: Uuid,
    pub friend: Uuid,
    pub pool: Uuid,
    pub host: Uuid,
    pub room: Uuid,
    pub request: Uuid,
    pub cancelled: bool,
    pub spectator_id: Option<Uuid>,
}

type RawPooledSpectator = (
    String,
    String,
    String,
    String,
    String,
    String,
    bool,
    Option<String>,
);

fn decode(row: RawPooledSpectator) -> Result<PooledSpectator> {
    let (viewer, friend, pool, host, room, request, cancelled, spectator_id) = row;
    Ok(PooledSpectator {
        viewer: parse_id(&viewer)?,
        friend: parse_id(&friend)?,
        pool: parse_id(&pool)?,
        host: parse_id(&host)?,
        room: parse_id(&room)?,
        request: parse_id(&request)?,
        cancelled,
        spectator_id: spectator_id.as_deref().map(parse_id).transpose()?,
    })
}

fn current(db: &mut SqliteConnection, viewer: &str) -> Result<Option<PooledSpectator>> {
    pooled_spectators::table
        .find(viewer)
        .select((
            pooled_spectators::player,
            pooled_spectators::friend,
            pooled_spectators::pool,
            pooled_spectators::host,
            pooled_spectators::match_id,
            pooled_spectators::request,
            pooled_spectators::cancelled,
            pooled_spectators::spectator_id,
        ))
        .first::<RawPooledSpectator>(db)
        .optional()?
        .map(decode)
        .transpose()
}

fn confirmed_friend(db: &mut SqliteConnection, viewer: &str, friend: &str) -> Result<()> {
    if !halcyon_community::may_spectate(db, viewer, friend).map_err(community_error)? {
        return Err(PlatformError::Conflict(
            "friend is unavailable for spectating",
        ));
    }
    Ok(())
}

fn friend_match(
    db: &mut SqliteConnection,
    friend: &str,
    pool: Uuid,
) -> Result<Option<(String, String)>> {
    Ok(federated_queue_members::table
        .inner_join(federated_queue::table)
        .filter(federated_queue_members::player.eq(friend))
        .filter(federated_queue_members::released.eq(false))
        .filter(federated_queue::pool.eq(pool.to_string()))
        .filter(federated_queue::phase.eq("playing"))
        .select((
            federated_queue::match_id.assume_not_null(),
            federated_queue::mode,
        ))
        .first::<(String, String)>(db)
        .optional()?)
}

impl AppState {
    /// Resolve a confirmed local friend's projected pooled match. Authority is checked by the peer.
    pub async fn pooled_spectator_target(
        &self,
        token: &str,
        friend: Uuid,
        pool: Uuid,
    ) -> Result<Option<Uuid>> {
        let key = digest(token);
        self.run(move |state| {
            state.read_and_touch_as(&key, |db, viewer| {
                let friend = friend.to_string();
                confirmed_friend(db, viewer, &friend)?;
                friend_match(db, &friend, pool)?
                    .map(|(room, _)| parse_id(&room))
                    .transpose()
            })
        })
        .await
    }

    /// Claim the local viewer while the matched host establishes a read-only capability.
    pub async fn claim_pooled_spectator(
        &self,
        token: &str,
        friend: Uuid,
        pool: Uuid,
        host: Uuid,
        room: Uuid,
    ) -> Result<PooledSpectator> {
        let key = digest(token);
        self.run(move |state| {
            state.transaction(|db| {
                let id = player(db, &key)?;
                let friend_str = friend.to_string();
                confirmed_friend(db, &id, &friend_str)?;
                let room_text = room.to_string();
                let (actual, mode) = friend_match(db, &friend_str, pool)?.ok_or(
                    PlatformError::Conflict("friend is unavailable for spectating"),
                )?;
                if actual != room_text {
                    return Err(PlatformError::Conflict("friend changed rooms"));
                }
                if let Some(prior) = current(db, &id)? {
                    if prior.pool == pool
                        && prior.host == host
                        && prior.friend == friend
                        && prior.room == room
                        && !prior.cancelled
                    {
                        return Ok(prior);
                    }
                    return Err(PlatformError::Conflict(
                        "another spectator session is in use",
                    ));
                }
                let occupied =
                    diesel::select(exists(tickets::table.filter(tickets::player.eq(&id))))
                        .get_result::<bool>(db)?
                        || diesel::select(exists(
                            federated_queue_members::table
                                .filter(federated_queue_members::player.eq(&id))
                                .filter(federated_queue_members::released.eq(false)),
                        ))
                        .get_result::<bool>(db)?
                        || diesel::select(exists(spectator_sessions::table.find(&id)))
                            .get_result::<bool>(db)?;
                if occupied {
                    return Err(PlatformError::Conflict("player is already participating"));
                }
                let request = Uuid::new_v4();
                diesel::insert_into(pooled_spectators::table)
                    .values((
                        pooled_spectators::player.eq(&id),
                        pooled_spectators::friend.eq(&friend_str),
                        pooled_spectators::pool.eq(pool.to_string()),
                        pooled_spectators::host.eq(host.to_string()),
                        pooled_spectators::match_id.eq(&room_text),
                        pooled_spectators::mode.eq(&mode),
                        pooled_spectators::request.eq(request.to_string()),
                    ))
                    .execute(db)?;
                Ok(PooledSpectator {
                    viewer: parse_id(&id)?,
                    friend,
                    pool,
                    host,
                    room,
                    request,
                    cancelled: false,
                    spectator_id: None,
                })
            })
        })
        .await
    }

    pub async fn publish_pooled_spectator(
        &self,
        request: Uuid,
        hosted: HostedSpectator,
    ) -> Result<()> {
        if hosted.address.is_empty() || hosted.spectator_id.is_nil() {
            return Err(PlatformError::HostUnavailable);
        }
        self.run(move |state| {
            state.transaction(|db| {
                let request = request.to_string();
                let row = pooled_spectators::table
                    .filter(pooled_spectators::request.eq(&request))
                    .filter(pooled_spectators::cancelled.eq(false))
                    .select((
                        pooled_spectators::player,
                        pooled_spectators::match_id,
                        pooled_spectators::spectator_id,
                    ))
                    .first::<(String, String, Option<String>)>(db)
                    .optional()?
                    .ok_or(PlatformError::Conflict("spectator session was closed"))?;
                let new_id = hosted.spectator_id.to_string();
                diesel::update(
                    pooled_spectators::table.filter(pooled_spectators::request.eq(&request)),
                )
                .set((
                    pooled_spectators::address.eq(Some(&hosted.address)),
                    pooled_spectators::spectator_id.eq(Some(&new_id)),
                ))
                .execute(db)?;
                if row.2.as_deref() != Some(new_id.as_str()) {
                    insert_event(
                        db,
                        &row.0,
                        EventBody::SpectatorEntered {
                            match_id: parse_id(&row.1)?,
                        },
                    )?;
                }
                Ok(())
            })
        })
        .await
    }

    pub async fn cancel_pooled_spectator(&self, token: &str) -> Result<Option<PooledSpectator>> {
        let key = digest(token);
        self.run(move |state| {
            state.transaction(|db| {
                let viewer = player(db, &key)?;
                diesel::update(pooled_spectators::table.find(&viewer))
                    .set(pooled_spectators::cancelled.eq(true))
                    .execute(db)?;
                current(db, &viewer)
            })
        })
        .await
    }

    pub async fn cancel_pooled_spectator_request(&self, request: Uuid) -> Result<()> {
        self.run(move |state| {
            state.transaction(|db| {
                diesel::update(
                    pooled_spectators::table
                        .filter(pooled_spectators::request.eq(request.to_string())),
                )
                .set(pooled_spectators::cancelled.eq(true))
                .execute(db)?;
                Ok(())
            })
        })
        .await
    }

    pub async fn pooled_spectators(&self, pool: Uuid) -> Result<Vec<PooledSpectator>> {
        self.run(move |state| {
            state.transaction(|db| {
                let rows = pooled_spectators::table
                    .filter(pooled_spectators::pool.eq(pool.to_string()))
                    .select((
                        pooled_spectators::player,
                        pooled_spectators::friend,
                        pooled_spectators::pool,
                        pooled_spectators::host,
                        pooled_spectators::match_id,
                        pooled_spectators::request,
                        pooled_spectators::cancelled,
                        pooled_spectators::spectator_id,
                    ))
                    .load::<RawPooledSpectator>(db)?;
                let mut claims = rows.into_iter().map(decode).collect::<Result<Vec<_>>>()?;
                for claim in claims.iter_mut().filter(|claim| !claim.cancelled) {
                    let still_visible = halcyon_community::may_spectate(
                        db,
                        &claim.viewer.to_string(),
                        &claim.friend.to_string(),
                    )
                    .map_err(community_error)?;
                    if !still_visible {
                        diesel::update(
                            pooled_spectators::table
                                .filter(pooled_spectators::request.eq(claim.request.to_string())),
                        )
                        .set(pooled_spectators::cancelled.eq(true))
                        .execute(db)?;
                        claim.cancelled = true;
                    }
                }
                Ok(claims)
            })
        })
        .await
    }

    /// Clear the local claim after authenticated host revocation or a committed terminal allocation.
    pub async fn close_pooled_spectator(&self, request: Uuid) -> Result<()> {
        self.run(move |state| {
            state.transaction(|db| {
                let prior = diesel::delete(
                    pooled_spectators::table
                        .filter(pooled_spectators::request.eq(request.to_string())),
                )
                .returning((
                    pooled_spectators::player,
                    pooled_spectators::match_id,
                    pooled_spectators::spectator_id,
                ))
                .get_result::<(String, String, Option<String>)>(db)
                .optional()?;
                if let Some((viewer, room, Some(_))) = prior {
                    insert_event(
                        db,
                        &viewer,
                        EventBody::SpectatorLeft {
                            match_id: parse_id(&room)?,
                        },
                    )?;
                }
                Ok(())
            })
        })
        .await
    }
}
