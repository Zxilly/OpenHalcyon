use crate::{
    Action, Community, CommunityError, Result,
    orm::{ensure_player, nocase_handle, unixepoch, unixepoch_after},
    schema::{
        community_friend_requests as requests, community_friends as friends,
        community_players as players,
    },
    validated,
    views::{Contact, FriendRequest, Friends},
};
use diesel::{
    dsl::{exists, not},
    prelude::*,
    upsert::excluded,
};
use serde_json::Value;
use uuid::Uuid;

impl Community {
    /// Update the community identity from an authenticated platform profile.
    pub async fn observe_player(&self, id: Uuid, handle: String, online: bool) -> Result<()> {
        let handle = validated(handle, 32, true)?;
        self.orm_transaction(move |db| {
            let upsert = diesel::insert_into(players::table)
                .values((
                    players::id.eq(id.to_string()),
                    players::handle.eq(&handle),
                    players::online.eq(online),
                ))
                .on_conflict(players::id)
                .do_update()
                .set((
                    players::handle.eq(excluded(players::handle)),
                    players::online.eq(excluded(players::online)),
                    players::last_seen.eq(unixepoch()),
                ));
            diesel::query_dsl::methods::FilterDsl::filter(
                upsert,
                players::handle
                    .ne(&handle)
                    .or(players::online.ne(online))
                    .or(players::last_seen.lt(unixepoch_after("now", "-15 seconds"))),
            )
            .execute(db)?;
            Ok(())
        })
        .await
    }

    pub async fn resolve_handle(&self, handle: String) -> Result<Uuid> {
        let handle = validated(handle, 32, true)?;
        self.orm_transaction(move |db| {
            let users = players::table
                .filter(nocase_handle().eq(&handle))
                .order(players::id)
                .limit(2)
                .select(players::id)
                .load::<String>(db)?;
            match users.as_slice() {
                [id] => Uuid::parse_str(id).map_err(|_| CommunityError::WorkerUnavailable),
                [] => Err(CommunityError::Missing("player does not exist")),
                _ => Err(CommunityError::Conflict(
                    "handle identifies multiple players",
                )),
            }
        })
        .await
    }

    /// Presence visibility is independent from a player's direct-message permissions.
    pub async fn presence_preferences(&self, player: Uuid) -> Result<(bool, bool)> {
        self.orm_transaction(move |db| {
            players::table
                .find(player.to_string())
                .select((not(players::invisible), players::receive_presence))
                .first(db)
                .map_err(Into::into)
        })
        .await
    }
}

fn check_peer(db: &mut SqliteConnection, actor: &str, target: Uuid) -> Result<String> {
    let target = target.to_string();
    if actor == target {
        return Err(CommunityError::Invalid("player must differ from target"));
    }
    ensure_player(db, &target)?;
    Ok(target)
}

fn are_friends(db: &mut SqliteConnection, actor: &str, peer: &str) -> Result<bool> {
    Ok(diesel::select(exists(friends::table.find((actor, peer)))).get_result(db)?)
}

pub(crate) fn execute(db: &mut SqliteConnection, actor: &str, action: Action) -> Result<Value> {
    match action {
        Action::Friends => {}
        Action::RequestFriend { target } => {
            let target = check_peer(db, actor, target)?;
            if !are_friends(db, actor, &target)? {
                diesel::insert_into(requests::table)
                    .values((
                        requests::requester.eq(actor),
                        requests::recipient.eq(&target),
                    ))
                    .on_conflict_do_nothing()
                    .execute(db)?;
            }
        }
        Action::ResolveFriend { requester, accept } => {
            let asker = check_peer(db, actor, requester)?;
            let affected = diesel::delete(requests::table.find((&asker, actor))).execute(db)?;
            if affected == 0 {
                if !accept || !are_friends(db, actor, &asker)? {
                    return Err(CommunityError::Missing("friend request does not exist"));
                }
            } else if accept {
                diesel::delete(requests::table.find((actor, &asker))).execute(db)?;
                for (viewer, peer) in [(actor, asker.as_str()), (asker.as_str(), actor)] {
                    diesel::insert_into(friends::table)
                        .values((friends::player.eq(viewer), friends::friend.eq(peer)))
                        .on_conflict_do_nothing()
                        .execute(db)?;
                }
            }
        }
        Action::RemoveFriend { target } => {
            let target = check_peer(db, actor, target)?;
            diesel::delete(
                friends::table.filter(
                    friends::player
                        .eq(actor)
                        .and(friends::friend.eq(&target))
                        .or(friends::player.eq(&target).and(friends::friend.eq(actor))),
                ),
            )
            .execute(db)?;
            diesel::delete(
                requests::table.filter(
                    requests::requester
                        .eq(actor)
                        .and(requests::recipient.eq(&target))
                        .or(requests::requester
                            .eq(&target)
                            .and(requests::recipient.eq(actor))),
                ),
            )
            .execute(db)?;
        }
        Action::FavoriteFriend { target, favorite } => {
            let target = check_peer(db, actor, target)?;
            if diesel::update(friends::table.find((actor, &target)))
                .set(friends::favorite.eq(favorite))
                .execute(db)?
                == 0
            {
                return Err(CommunityError::Missing("friend does not exist"));
            }
        }
        Action::SetInvisible { invisible } => {
            diesel::update(players::table.find(actor))
                .set(players::invisible.eq(invisible))
                .execute(db)?;
        }
        Action::SetReceivePresence { receive } => {
            diesel::update(players::table.find(actor))
                .set(players::receive_presence.eq(receive))
                .execute(db)?;
        }
        Action::SetPresence { status } => {
            let status = validated(status, 64, false)?;
            diesel::update(players::table.find(actor))
                .set(players::status.eq(status))
                .execute(db)?;
        }
        _ => return Err(CommunityError::Invalid("invalid friend action")),
    }
    friend_view(db, actor)
}

fn friend_view(db: &mut SqliteConnection, actor: &str) -> Result<Value> {
    let receive = players::table
        .find(actor)
        .select(players::receive_presence)
        .first::<bool>(db)?;
    let contacts = friends::table
        .inner_join(players::table.on(players::id.eq(friends::friend)))
        .filter(friends::player.eq(actor))
        .order((friends::favorite.desc(), nocase_handle(), players::id))
        .select((
            players::id,
            players::handle,
            friends::favorite,
            players::status,
            players::online
                .and(not(players::invisible))
                .and(players::last_seen.ge(unixepoch_after("now", "-90 seconds"))),
        ))
        .load::<(String, String, bool, String, bool)>(db)?
        .into_iter()
        .map(|(player_id, handle, favorite, status, available)| {
            let online = receive && available;
            Contact {
                player_id,
                handle,
                favorite,
                online,
                status: online.then_some(status),
            }
        })
        .collect();

    let incoming = requests::table
        .inner_join(players::table.on(players::id.eq(requests::requester)))
        .filter(requests::recipient.eq(actor))
        .select((players::id, players::handle, requests::created));
    let outgoing = requests::table
        .inner_join(players::table.on(players::id.eq(requests::recipient)))
        .filter(requests::requester.eq(actor))
        .select((players::id, players::handle, requests::created));
    let mut pending = Vec::new();
    for (rows, incoming) in [
        (incoming.load::<(String, String, i64)>(db)?, true),
        (outgoing.load(db)?, false),
    ] {
        pending.extend(
            rows.into_iter()
                .map(|(player_id, handle, created)| FriendRequest {
                    player_id,
                    handle,
                    created,
                    incoming,
                }),
        );
    }
    pending.sort_by(|a, b| {
        a.created
            .cmp(&b.created)
            .then_with(|| a.player_id.cmp(&b.player_id))
    });
    Ok(serde_json::to_value(Friends {
        friends: contacts,
        requests: pending,
    })?)
}

/// Verify friendship and presence in the caller's spectator admission transaction.
pub fn may_spectate(db: &mut SqliteConnection, viewer: &str, friend: &str) -> Result<bool> {
    Ok(halcyon_storage::community_queues::may_spectate(
        db, viewer, friend,
    )?)
}
