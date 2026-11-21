use super::orm_schema::{
    events, federated_chat_receipts, federated_queue, federated_queue_members, matches, players,
    tickets,
};
use crate::{
    EventBody, PlatformError, Result,
    service::{AppState, digest, parse_id},
};
use diesel::{prelude::*, sql_types::Text};
use uuid::Uuid;

diesel::define_sql_function!(fn unixepoch() -> diesel::sql_types::BigInt);
diesel::define_sql_function!(#[sql_name = "unixepoch"] fn unixepoch_after(base: Text, modifier: Text) -> diesel::sql_types::BigInt);

impl AppState {
    /// Contacts in the authenticated player's current local game roster.
    pub async fn local_match_contacts(&self, session: &str) -> Result<Vec<(Uuid, String)>> {
        let key = digest(session);
        self.run(move |state| {
            state.read_and_touch_as(&key, |db, actor| {
                let room = tickets::table
                    .inner_join(matches::table)
                    .filter(tickets::player.eq(actor))
                    .filter(matches::phase.eq("playing"))
                    .select(matches::id)
                    .first::<String>(db)
                    .optional()?;
                let Some(room) = room else {
                    return Ok(Vec::new());
                };
                tickets::table
                    .inner_join(players::table.on(players::id.eq(tickets::player)))
                    .filter(tickets::match_id.eq(&room))
                    .filter(tickets::player.ne(actor))
                    .order(players::id)
                    .select((players::id, players::name))
                    .load::<(String, String)>(db)?
                    .into_iter()
                    .map(|(id, name)| Ok((parse_id(&id)?, name)))
                    .collect()
            })
        })
        .await
    }

    /// Persist the incoming event once on this home. Its stream cursor and receipt commit together.
    pub async fn receive_pool_chat(
        &self,
        pool: Uuid,
        room: Uuid,
        id: Uuid,
        sender: Uuid,
        text: String,
        expected_seats: usize,
    ) -> Result<()> {
        let pool = pool.to_string();
        let room_str = room.to_string();
        let receipt_id = id.to_string();
        self.run(move |state| {
            state.transaction(|db| {
                diesel::delete(federated_chat_receipts::table.filter(
                    federated_chat_receipts::seen.lt(unixepoch_after("now", "-3600 seconds")),
                ))
                .execute(db)?;
                let fresh = diesel::insert_into(federated_chat_receipts::table)
                    .values((
                        federated_chat_receipts::pool.eq(&pool),
                        federated_chat_receipts::room.eq(&room_str),
                        federated_chat_receipts::id.eq(&receipt_id),
                    ))
                    .on_conflict_do_nothing()
                    .execute(db)?;
                if fresh == 0 {
                    return Ok(());
                }
                let event = EventBody::Chat {
                    match_id: room,
                    player_id: sender,
                    text: &text,
                }
                .encode()?;
                let recipients = federated_queue_members::table
                    .inner_join(federated_queue::table)
                    .filter(federated_queue::pool.eq(&pool))
                    .filter(federated_queue::match_id.eq(&room_str))
                    .filter(federated_queue::phase.eq_any(["offered", "playing"]))
                    .filter(federated_queue_members::released.eq(false))
                    .order(federated_queue_members::player)
                    .select(federated_queue_members::player)
                    .load::<String>(db)?;
                if !recipients.is_empty() {
                    let rows: Vec<_> = recipients
                        .iter()
                        .map(|member| (events::player.eq(member), events::body.eq(&event)))
                        .collect();
                    diesel::insert_into(events::table)
                        .values(&rows)
                        .execute(db)?;
                }
                if expected_seats == 0 || recipients.len() != expected_seats {
                    return Err(PlatformError::Conflict(
                        "waiting for the active room roster",
                    ));
                }
                Ok(())
            })
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::AppState;
    use crate::{HostError, HostResult, HostedMatch, MatchHost, PlatformConfig, Reservation};
    use std::sync::Arc;
    use uuid::Uuid;

    struct NoLocalRooms;
    impl MatchHost for NoLocalRooms {
        fn reserve(&self, _: Reservation) -> HostResult<HostedMatch> {
            Err(HostError::HostUnavailable)
        }
    }

    #[tokio::test]
    async fn chat_waits_for_the_entire_local_roster_and_commits_once_across_restart() {
        let dir = tempfile::tempdir().unwrap();
        let config = PlatformConfig {
            database_path: dir.path().join("platform.db"),
            public_address: "localhost:8000".into(),
            seats_per_match: 2,
        };
        let state = AppState::open(config.clone(), Arc::new(NoLocalRooms)).unwrap();
        let pool = Uuid::new_v4();
        let room = Uuid::new_v4();
        let id = Uuid::new_v4();
        let sender = Uuid::new_v4();
        let mut tokens = Vec::new();
        for name in ["Alice", "Bob"] {
            let guest = state.create_guest(name.into()).await.unwrap();
            let intent = state
                .federated_join(&guest.session_token, pool, "duo".into(), 2)
                .await
                .unwrap();
            state
                .federated_project(
                    intent.ticket,
                    crate::LobbyPhase::Offered.as_str(),
                    Some(room),
                    None,
                    None,
                )
                .await
                .unwrap();
            tokens.push(guest.session_token);
            if tokens.len() == 1 {
                assert!(
                    state
                        .receive_pool_chat(pool, room, id, sender, "text".into(), 2)
                        .await
                        .is_err()
                );
            }
        }
        state
            .receive_pool_chat(pool, room, id, sender, "text".into(), 2)
            .await
            .unwrap();
        drop(state);
        let state = AppState::open(config, Arc::new(NoLocalRooms)).unwrap();
        state
            .receive_pool_chat(pool, room, id, sender, "text".into(), 2)
            .await
            .unwrap();
        for token in tokens {
            let events = state.events(&token, 0, false).await.unwrap();
            assert_eq!(
                events
                    .events
                    .iter()
                    .filter(|e| e.body["type"] == "chat")
                    .count(),
                1
            );
        }
    }
}
