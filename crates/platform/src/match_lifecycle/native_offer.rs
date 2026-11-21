use diesel::{
    OptionalExtension, dsl::exists, prelude::*, sql_types::Text, sqlite::SqliteConnection,
};
use halcyon_types::{NativeOffer, NativeSeat};
use uuid::Uuid;

use crate::{
    AbortReason, PlatformError, Result,
    events::orm_schema::{
        federated_queue, federated_queue_members, matches, native_offer_seats, native_rejections,
        tickets,
    },
    service::{AppState, abort, digest, parse_id, player},
};

diesel::define_sql_function!(#[sql_name = "unixepoch"] fn unixepoch_after(base: Text, modifier: Text) -> diesel::sql_types::BigInt);

impl AppState {
    /// Local rejection and its retry receipt share one transaction.
    pub async fn reject_local_offer(&self, session: &str, room: Uuid) -> Result<bool> {
        let key = digest(session);
        self.run(move |state| {
            state.transaction(|db| {
                let actor = player(db, &key)?;
                let room = room.to_string();
                let previous = diesel::select(exists(
                    native_rejections::table
                        .filter(native_rejections::player.eq(&actor))
                        .filter(native_rejections::room.eq(&room))
                        .filter(native_rejections::ticket.is_null()),
                ))
                .get_result::<bool>(db)?;
                if previous {
                    return Ok(true);
                }
                let offered = diesel::select(exists(
                    tickets::table
                        .inner_join(matches::table)
                        .filter(tickets::player.eq(&actor))
                        .filter(matches::id.eq(&room))
                        .filter(matches::phase.eq("offered"))
                        .filter(matches::offered_at.gt(unixepoch_after("now", "-30 seconds"))),
                ))
                .get_result::<bool>(db)?;
                if !offered {
                    return Ok(false);
                }
                abort(db, &room, AbortReason::PlayerLeft)?;
                diesel::insert_into(native_rejections::table)
                    .values((
                        native_rejections::player.eq(&actor),
                        native_rejections::room.eq(&room),
                    ))
                    .execute(db)?;
                Ok(true)
            })
        })
        .await
    }

    /// Save the original authenticated binding before proposing a rejection.
    /// A retry can submit the same room-scoped Raft request after ticket release.
    pub async fn native_reject_ticket(
        &self,
        session: &str,
        room: Uuid,
        pool: Uuid,
    ) -> Result<Option<Uuid>> {
        let key = digest(session);
        self.run(move |state| {
            state.transaction(|db| {
                let actor = player(db, &key)?;
                let room_str = room.to_string();
                if let Some((member, match_id, ticket)) = federated_queue_members::table
                    .inner_join(federated_queue::table)
                    .filter(federated_queue_members::player.eq(&actor))
                    .filter(federated_queue::match_id.eq(&room_str))
                    .filter(federated_queue::pool.eq(pool.to_string()))
                    .filter(federated_queue::phase.eq("offered"))
                    .filter(federated_queue_members::released.eq(false))
                    .select((
                        federated_queue_members::player,
                        federated_queue::match_id.assume_not_null(),
                        federated_queue::ticket,
                    ))
                    .first::<(String, String, String)>(db)
                    .optional()?
                {
                    diesel::insert_into(native_rejections::table)
                        .values((
                            native_rejections::player.eq(member),
                            native_rejections::room.eq(match_id),
                            native_rejections::ticket.eq(Some(ticket)),
                        ))
                        .on_conflict_do_nothing()
                        .execute(db)?;
                }
                let ticket = native_rejections::table
                    .filter(native_rejections::player.eq(&actor))
                    .filter(native_rejections::room.eq(&room_str))
                    .filter(native_rejections::ticket.is_not_null())
                    .select(native_rejections::ticket.assume_not_null())
                    .first::<String>(db)
                    .optional()?;
                ticket.as_deref().map(parse_id).transpose()
            })
        })
        .await
    }

    pub async fn native_queue_party_size(&self, session: &str) -> Result<u32> {
        let key = digest(session);
        self.run(move |state| {
            state.read_and_touch_as(&key, |db, owner| {
                if let Some(batch) = tickets::table
                    .filter(tickets::player.eq(owner))
                    .filter(tickets::match_id.is_null())
                    .select(tickets::batch)
                    .first::<Option<String>>(db)
                    .optional()?
                {
                    let count = match batch {
                        Some(batch) => tickets::table
                            .filter(tickets::batch.eq(&batch))
                            .count()
                            .get_result::<i64>(db)?,
                        None => 1,
                    };
                    return Ok(count as u32);
                }
                if let Some(ticket) = federated_queue_members::table
                    .filter(federated_queue_members::player.eq(owner))
                    .filter(federated_queue_members::released.eq(false))
                    .select(federated_queue_members::ticket)
                    .first::<String>(db)
                    .optional()?
                {
                    let count = federated_queue_members::table
                        .filter(federated_queue_members::ticket.eq(&ticket))
                        .filter(federated_queue_members::released.eq(false))
                        .count()
                        .get_result::<i64>(db)?;
                    return Ok(count.max(1) as u32);
                }
                Ok(1)
            })
        })
        .await
    }

    /// The caller must still hold their own ticket to inspect this room's roster.
    pub async fn native_offer(&self, session: &str) -> Result<Option<NativeOffer>> {
        let key = digest(session);
        self.run(move |state| state.read_as(&key, read_native_offer))
            .await
    }
}

fn read_native_offer(db: &mut SqliteConnection, owner: &str) -> Result<Option<NativeOffer>> {
    let binding = tickets::table
        .inner_join(matches::table)
        .filter(tickets::player.eq(owner))
        .select((matches::id, matches::offered_at))
        .first::<(String, i64)>(db)
        .optional()?;
    let Some((room, offered_at)) = binding else {
        return Ok(None);
    };
    let expires = offered_at
        .checked_add(30)
        .ok_or(PlatformError::WorkerUnavailable)?;
    let mut entries = std::collections::BTreeSet::new();
    let seats = native_offer_seats::table
        .filter(native_offer_seats::room.eq(&room))
        .order(native_offer_seats::position)
        .select((
            native_offer_seats::player,
            native_offer_seats::accepted,
            native_offer_seats::entry,
        ))
        .load::<(String, bool, String)>(db)?
        .into_iter()
        .map(|(player, accepted, entry)| {
            entries.insert(entry);
            Ok(NativeSeat {
                player: parse_id(&player)?,
                accepted,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(NativeOffer {
        id: parse_id(&room)?,
        seats,
        entries: entries.len() as u32,
        expires: Some(expires),
    }))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{
        AppState, HostResult, HostedMatch, MatchHost, PlatformConfig, PlatformError, Reservation,
    };
    use rusqlite::Connection;

    struct NoHost;
    impl MatchHost for NoHost {
        fn reserve(&self, _: Reservation) -> HostResult<HostedMatch> {
            panic!("reading an offer never reserves a game host")
        }
    }

    #[tokio::test]
    async fn offer_snapshot_does_not_upgrade_to_a_write_under_contention() {
        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("test.sqlite");
        let state = AppState::open(
            PlatformConfig {
                database_path: database.clone(),
                public_address: "localhost".into(),
                seats_per_match: 2,
            },
            Arc::new(NoHost),
        )
        .unwrap();
        let guest = state.create_guest("fixture".into()).await.unwrap();
        state
            .join(&guest.session_token, "5v5_pvp_casual".into())
            .await
            .unwrap();

        let second = state.create_guest("second".into()).await.unwrap();
        state
            .join(&second.session_token, "5v5_pvp_casual".into())
            .await
            .unwrap();
        let outsider = state.create_guest("outside".into()).await.unwrap();
        assert!(
            state
                .native_offer(&outsider.session_token)
                .await
                .unwrap()
                .is_none()
        );
        assert!(matches!(
            state.native_offer("invalid").await,
            Err(PlatformError::Unauthorized)
        ));

        let mut external = Connection::open(database).unwrap();
        let writer = external
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .unwrap();
        writer
            .execute(
                "UPDATE players SET last_seen=1 WHERE id=?",
                [guest.player_id.to_string()],
            )
            .unwrap();
        let offer = state
            .native_offer(&guest.session_token)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            offer
                .seats
                .iter()
                .map(|seat| seat.player)
                .collect::<Vec<_>>(),
            [guest.player_id, second.player_id],
        );
        assert_eq!(offer.entries, 2);
        assert!(offer.seats.iter().all(|seat| !seat.accepted));
        assert!(offer.expires.is_some());
        writer.commit().unwrap();
        let last_seen: i64 = external
            .query_row(
                "SELECT last_seen FROM players WHERE id=?",
                [guest.player_id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(last_seen, 1);
    }
}
