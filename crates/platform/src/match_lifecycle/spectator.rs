use diesel::{dsl::exists, prelude::*, sqlite::SqliteConnection};
use halcyon_types::{HostError, SpectatorExit};
use uuid::Uuid;

use crate::{
    EventBody, PlatformError, Result,
    events::orm_schema::{
        events, federated_queue_members, matches, pooled_spectators, spectator_sessions, tickets,
    },
    match_lifecycle::queue::community_error,
    service::{AppState, digest, insert_event, parse_id, player},
};

pub(crate) fn close_match(db: &mut SqliteConnection, match_id: &str) -> Result<()> {
    let message = EventBody::SpectatorLeft {
        match_id: parse_id(match_id)?,
    }
    .encode()?;
    let viewers = spectator_sessions::table
        .filter(spectator_sessions::match_id.eq(match_id))
        .filter(spectator_sessions::spectator_id.is_not_null())
        .order(spectator_sessions::player)
        .select(spectator_sessions::player)
        .load::<String>(db)?;
    if !viewers.is_empty() {
        let rows: Vec<_> = viewers
            .iter()
            .map(|viewer| (events::player.eq(viewer), events::body.eq(&message)))
            .collect();
        diesel::insert_into(events::table)
            .values(&rows)
            .execute(db)?;
    }
    diesel::delete(spectator_sessions::table.filter(spectator_sessions::match_id.eq(match_id)))
        .execute(db)?;
    Ok(())
}

impl AppState {
    fn spectator_exit_handler(&self, player: Uuid, room: Uuid, request: String) -> SpectatorExit {
        // The host retains this callback for reconnects; do not retain the host through AppState.
        let accounts = std::sync::Arc::downgrade(&self.accounts);
        let changes = self.changes.clone();
        std::sync::Arc::new(move |grant| {
            let update = || -> Result<()> {
                let accounts = accounts.upgrade().ok_or(PlatformError::WorkerUnavailable)?;
                let mut store = accounts
                    .lock()
                    .map_err(|_| PlatformError::WorkerUnavailable)?;
                let room_str = room.to_string();
                let player_str = player.to_string();
                let grant_str = grant.to_string();
                let ((), changed) = store.write_observing_changes(|db| {
                    let removed = diesel::delete(
                        spectator_sessions::table
                            .filter(spectator_sessions::match_id.eq(&room_str))
                            .filter(spectator_sessions::player.eq(&player_str))
                            .filter(
                                spectator_sessions::spectator_id.eq(&grant_str).or(
                                    spectator_sessions::spectator_id
                                        .is_null()
                                        .and(spectator_sessions::request.eq(&request)),
                                ),
                            ),
                    )
                    .execute(db)?;
                    if removed > 0 {
                        insert_event(db, &player_str, EventBody::SpectatorLeft { match_id: room })?;
                    }
                    Ok::<(), PlatformError>(())
                })?;
                if changed {
                    changes.send_replace(());
                }
                Ok(())
            };
            update().map_err(HostError::from)
        })
    }

    pub async fn start_spectating(&self, token: &str, friend: Uuid) -> Result<()> {
        let key = digest(token);
        let friend = friend.to_string();
        let request = Uuid::new_v4().to_string();
        self.run(move |state| {
            let begin = state.transaction(|db| {
                let actor = player(db, &key)?;
                if !halcyon_community::may_spectate(db, &actor, &friend).map_err(community_error)? {
                    return Err(PlatformError::Conflict(
                        "friend is unavailable for spectating",
                    ));
                }
                let room = tickets::table
                    .inner_join(matches::table)
                    .filter(tickets::player.eq(&friend))
                    .filter(matches::phase.eq("playing"))
                    .select(matches::id)
                    .first::<String>(db)
                    .optional()?
                    .ok_or(PlatformError::Conflict(
                        "friend is unavailable for spectating",
                    ))?;
                let existing = spectator_sessions::table
                    .find(&actor)
                    .select((
                        spectator_sessions::match_id,
                        spectator_sessions::spectator_id,
                    ))
                    .first::<(String, Option<String>)>(db)
                    .optional()?;
                if let Some((current, prior)) = existing {
                    let prior =
                        prior
                            .filter(|_| current == room)
                            .ok_or(PlatformError::Conflict(
                                "a spectator session is already in use",
                            ))?;
                    diesel::update(spectator_sessions::table.find(&actor))
                        .set((
                            spectator_sessions::request.eq(&request),
                            spectator_sessions::cancelled.eq(false),
                        ))
                        .execute(db)?;
                    return Ok((parse_id(&actor)?, parse_id(&room)?, Some(parse_id(&prior)?)));
                }
                let occupied =
                    diesel::select(exists(tickets::table.filter(tickets::player.eq(&actor))))
                        .get_result::<bool>(db)?
                        || diesel::select(exists(
                            federated_queue_members::table
                                .filter(federated_queue_members::player.eq(&actor))
                                .filter(federated_queue_members::released.eq(false)),
                        ))
                        .get_result::<bool>(db)?
                        || diesel::select(exists(pooled_spectators::table.find(&actor)))
                            .get_result::<bool>(db)?;
                if occupied {
                    return Err(PlatformError::Conflict(
                        "player is queued or in another match",
                    ));
                }
                diesel::insert_into(spectator_sessions::table)
                    .values((
                        spectator_sessions::player.eq(&actor),
                        spectator_sessions::request.eq(&request),
                        spectator_sessions::match_id.eq(&room),
                    ))
                    .execute(db)?;
                Ok((parse_id(&actor)?, parse_id(&room)?, None))
            })?;
            let (player_id, match_id, previous) = begin;
            let exit = state.spectator_exit_handler(player_id, match_id, request.clone());
            match state.host.spectate_with_exit(match_id, player_id, exit) {
                Ok(hosted) => {
                    let published = state.transaction(|db| {
                        if hosted.address.trim().is_empty() || hosted.spectator_id.is_nil() {
                            return Err(PlatformError::HostUnavailable);
                        }
                        let actor = player_id.to_string();
                        let updated = diesel::update(
                            spectator_sessions::table
                                .filter(spectator_sessions::request.eq(&request))
                                .filter(spectator_sessions::cancelled.eq(false)),
                        )
                        .set((
                            spectator_sessions::spectator_id
                                .eq(Some(hosted.spectator_id.to_string())),
                            spectator_sessions::address.eq(Some(&hosted.address)),
                        ))
                        .execute(db)?;
                        if updated == 0 {
                            return Err(PlatformError::Conflict("spectator session was closed"));
                        }
                        insert_event(db, &actor, EventBody::SpectatorEntered { match_id })
                    });
                    if published.is_err() {
                        let _ = state.host.stop_spectating(match_id, hosted.spectator_id);
                        state.transaction(|db| {
                            diesel::delete(
                                spectator_sessions::table
                                    .filter(spectator_sessions::request.eq(&request)),
                            )
                            .execute(db)?;
                            Ok(())
                        })?;
                    }
                    published
                }
                Err(error) => {
                    if let Some(previous) = previous {
                        let _ = state.host.stop_spectating(match_id, previous);
                    }
                    state.transaction(|db| {
                        diesel::delete(
                            spectator_sessions::table
                                .filter(spectator_sessions::request.eq(&request)),
                        )
                        .execute(db)?;
                        Ok(())
                    })?;
                    Err(error.into())
                }
            }
        })
        .await
    }

    /// Revoke the host capability before clearing an active viewer session.
    pub async fn leave_spectator(&self, token: &str) -> Result<()> {
        let key = digest(token);
        self.run(move |state| {
            let prior = state.transaction(|db| {
                let actor = player(db, &key)?;
                let prior = spectator_sessions::table
                    .find(&actor)
                    .select((
                        spectator_sessions::request,
                        spectator_sessions::match_id,
                        spectator_sessions::spectator_id,
                    ))
                    .first::<(String, String, Option<String>)>(db)
                    .optional()?;
                let Some((request, room, spectator)) = prior else {
                    return Ok(None);
                };
                if let Some(spectator) = spectator {
                    Ok(Some((
                        actor,
                        request,
                        parse_id(&room)?,
                        parse_id(&spectator)?,
                    )))
                } else {
                    diesel::update(
                        spectator_sessions::table.filter(spectator_sessions::request.eq(&request)),
                    )
                    .set(spectator_sessions::cancelled.eq(true))
                    .execute(db)?;
                    Ok(None)
                }
            })?;
            let Some((actor, request, room, spectator)) = prior else {
                return Ok(());
            };
            state.host.stop_spectating(room, spectator)?;
            state.transaction(|db| {
                if diesel::delete(
                    spectator_sessions::table.filter(spectator_sessions::request.eq(&request)),
                )
                .execute(db)?
                    > 0
                {
                    insert_event(db, &actor, EventBody::SpectatorLeft { match_id: room })?;
                }
                Ok(())
            })
        })
        .await
    }
}
