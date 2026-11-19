use rusqlite::{Transaction, params};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{Action, Community, CommunityError, Result, ensure_player, validated};

impl Community {
    /// Update the community identity from an authenticated platform profile.
    pub async fn observe_player(&self, id: Uuid, handle: String, online: bool) -> Result<()> {
        let handle = validated(handle, 32, true)?;
        self.run(move |tx| {
            tx.execute(
                "INSERT INTO community_players(id, handle, online) VALUES(?, ?, ?)
                 ON CONFLICT(id) DO UPDATE SET handle=excluded.handle, online=excluded.online, last_seen=unixepoch()
                 WHERE handle<>excluded.handle OR online<>excluded.online OR last_seen < unixepoch() - 15",
                params![id.to_string(), handle, online],
            )?;
            Ok(())
        }).await
    }

    pub async fn resolve_handle(&self, handle: String) -> Result<Uuid> {
        let handle = validated(handle, 32, true)?;
        self.run(move |tx| {
            let mut lookup = tx.prepare(
                "SELECT id FROM community_players WHERE handle=? COLLATE NOCASE ORDER BY id LIMIT 2",
            )?;
            let rows = lookup.query_map([handle], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            match rows.as_slice() {
                [id] => Uuid::parse_str(id).map_err(|_| CommunityError::WorkerUnavailable),
                [] => Err(CommunityError::Missing("player does not exist")),
                _ => Err(CommunityError::Conflict("handle identifies multiple players")),
            }
        }).await
    }
}

fn check_peer(tx: &Transaction<'_>, actor: &str, target: Uuid) -> Result<String> {
    let target = target.to_string();
    if actor == target {
        return Err(CommunityError::Invalid("player must differ from target"));
    }
    ensure_player(tx, &target)?;
    Ok(target)
}

pub(crate) fn execute(tx: &Transaction<'_>, actor: &str, action: Action) -> Result<Value> {
    match action {
        Action::Friends => {}
        Action::RequestFriend { target } => {
            let target = check_peer(tx, actor, target)?;
            if !tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM community_friends WHERE player=? AND friend=?)",
                params![actor, target],
                |row| row.get(0),
            )? {
                tx.execute(
                    "INSERT INTO community_friend_requests(requester, recipient) VALUES(?, ?)
                     ON CONFLICT DO NOTHING",
                    params![actor, target],
                )?;
            }
        }
        Action::ResolveFriend { requester, accept } => {
            let requester = check_peer(tx, actor, requester)?;
            let affected = tx.execute(
                "DELETE FROM community_friend_requests WHERE requester=? AND recipient=?",
                params![requester, actor],
            )?;
            if affected == 0 {
                let existing: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM community_friends WHERE player=? AND friend=?)",
                    params![actor, requester],
                    |row| row.get(0),
                )?;
                if !accept || !existing {
                    return Err(CommunityError::Missing("friend request does not exist"));
                }
            } else if accept {
                tx.execute(
                    "DELETE FROM community_friend_requests WHERE requester=? AND recipient=?",
                    params![actor, requester],
                )?;
                tx.execute(
                    "INSERT INTO community_friends(player, friend) VALUES(?1, ?2), (?2, ?1)
                     ON CONFLICT DO NOTHING",
                    params![actor, requester],
                )?;
            }
        }
        Action::RemoveFriend { target } => {
            let target = check_peer(tx, actor, target)?;
            tx.execute(
                "DELETE FROM community_friends WHERE (player=?1 AND friend=?2) OR (player=?2 AND friend=?1)",
                params![actor, target],
            )?;
            tx.execute(
                "DELETE FROM community_friend_requests
                 WHERE (requester=?1 AND recipient=?2) OR (requester=?2 AND recipient=?1)",
                params![actor, target],
            )?;
        }
        Action::FavoriteFriend { target, favorite } => {
            let target = check_peer(tx, actor, target)?;
            if tx.execute(
                "UPDATE community_friends SET favorite=? WHERE player=? AND friend=?",
                params![favorite, actor, target],
            )? == 0
            {
                return Err(CommunityError::Missing("friend does not exist"));
            }
        }
        Action::SetInvisible { invisible } => {
            tx.execute(
                "UPDATE community_players SET invisible=? WHERE id=?",
                params![invisible, actor],
            )?;
        }
        Action::SetReceivePresence { receive } => {
            tx.execute(
                "UPDATE community_players SET receive_presence=? WHERE id=?",
                params![receive, actor],
            )?;
        }
        Action::SetPresence { status } => {
            let status = validated(status, 64, false)?;
            tx.execute(
                "UPDATE community_players SET status=? WHERE id=?",
                params![status, actor],
            )?;
        }
        _ => return Err(CommunityError::Invalid("invalid friend action")),
    }
    friends(tx, actor)
}

fn friends(tx: &Transaction<'_>, actor: &str) -> Result<Value> {
    let receive: bool = tx.query_row(
        "SELECT receive_presence FROM community_players WHERE id=?",
        [actor],
        |row| row.get(0),
    )?;
    let mut query = tx.prepare(
        "SELECT p.id, p.handle, f.favorite, p.status,
                p.online AND NOT p.invisible AND p.last_seen >= unixepoch() - 90
         FROM community_friends f JOIN community_players p ON p.id=f.friend
         WHERE f.player=? ORDER BY f.favorite DESC, p.handle COLLATE NOCASE, p.id",
    )?;
    let contacts = query
        .query_map([actor], |row| {
            let online = receive && row.get::<_, bool>(4)?;
            Ok(json!({
                "playerId":row.get::<_, String>(0)?, "handle":row.get::<_, String>(1)?,
                "favorite":row.get::<_, bool>(2)?, "online":online,
                "status":if online { Some(row.get::<_, String>(3)?) } else { None },
            }))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut query = tx.prepare(
        "SELECT p.id, p.handle, r.created, r.recipient = ?1
         FROM community_friend_requests r JOIN community_players p
           ON p.id=CASE WHEN r.recipient=?1 THEN r.requester ELSE r.recipient END
         WHERE r.requester=?1 OR r.recipient=?1 ORDER BY r.created, p.id",
    )?;
    let requests = query
        .query_map([actor], |row| {
            Ok(json!({
                "playerId":row.get::<_, String>(0)?, "handle":row.get::<_, String>(1)?,
                "created":row.get::<_, i64>(2)?, "incoming":row.get::<_, bool>(3)?,
            }))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(json!({ "friends":contacts, "requests":requests }))
}

/// Check friendship and the target's opt-out on the platform's current transaction.
pub fn may_spectate(db: &rusqlite::Connection, viewer: &str, friend: &str) -> Result<bool> {
    db.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM community_friends f JOIN community_players p ON p.id=f.friend
            WHERE f.player=? AND f.friend=? AND p.invisible=0
         )",
        params![viewer, friend],
        |row| row.get(0),
    )
    .map_err(Into::into)
}
