use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};
use serde_json::json;
use uuid::Uuid;

use super::{
    AppState, HostedSpectator, PlatformError, Result, digest, event, parse_id, player,
    queue::community_error,
};

pub(super) fn initialize(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS pooled_spectators (
            player TEXT PRIMARY KEY REFERENCES players(id), friend TEXT NOT NULL REFERENCES players(id),
            pool TEXT NOT NULL, host TEXT NOT NULL, match_id TEXT NOT NULL, mode TEXT NOT NULL,
            request TEXT NOT NULL UNIQUE, address TEXT, spectator_id TEXT, cancelled INTEGER NOT NULL DEFAULT 0
         );
         CREATE INDEX IF NOT EXISTS pooled_spectators_by_pool ON pooled_spectators(pool);",
    )?;
    Ok(())
}

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

fn decode(row: &Row<'_>) -> rusqlite::Result<PooledSpectator> {
    let id = |idx| -> rusqlite::Result<Uuid> {
        let text: String = row.get(idx)?;
        Uuid::parse_str(&text).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(
                idx,
                rusqlite::types::Type::Text,
                Box::new(err),
            )
        })
    };
    Ok(PooledSpectator {
        viewer: id(0)?,
        friend: id(1)?,
        pool: id(2)?,
        host: id(3)?,
        room: id(4)?,
        request: id(5)?,
        cancelled: row.get(6)?,
        spectator_id: if row.get_ref(7)?.data_type() == rusqlite::types::Type::Null {
            None
        } else {
            Some(id(7)?)
        },
    })
}

fn current(db: &Connection, viewer: &str) -> Result<Option<PooledSpectator>> {
    db.query_row(
        "SELECT player,friend,pool,host,match_id,request,cancelled,spectator_id FROM pooled_spectators WHERE player=?",
        [viewer],decode,
    ).optional().map_err(Into::into)
}

fn confirmed_friend(db: &Connection, viewer: &str, friend: &str) -> Result<()> {
    if !halcyon_community::may_spectate(db, viewer, friend).map_err(community_error)? {
        return Err(PlatformError::Conflict(
            "friend is unavailable for spectating",
        ));
    }
    Ok(())
}

fn friend_match(db: &Connection, friend: &str, pool: Uuid) -> Result<Option<(String, String)>> {
    db.query_row(
        "SELECT q.match_id,q.mode FROM federated_queue_members m JOIN federated_queue q ON m.ticket=q.ticket
         WHERE m.player=? AND m.released=0 AND q.pool=? AND q.phase='playing'",
        params![friend,pool.to_string()], |r| Ok((r.get(0)?,r.get(1)?)),
    ).optional().map_err(Into::into)
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
            let db = state
                .db
                .lock()
                .map_err(|_| PlatformError::WorkerUnavailable)?;
            let viewer = player(&db, &key)?;
            let friend = friend.to_string();
            confirmed_friend(&db, &viewer, &friend)?;
            friend_match(&db, &friend, pool)?
                .map(|(room, _)| parse_id(&room))
                .transpose()
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
        self.run(move |state|state.transaction(|tx| {
            let id=player(tx,&key)?;
            confirmed_friend(tx,&id,&friend.to_string())?;
            let room_text=room.to_string();
            let (actual,mode)=friend_match(tx,&friend.to_string(),pool)?.ok_or(PlatformError::Conflict("friend is unavailable for spectating"))?;
            if actual!=room_text {
                return Err(PlatformError::Conflict("friend changed rooms"));
            }
            if let Some(prior)=current(tx,&id)? {
                if prior.pool==pool && prior.host==host && prior.friend==friend && prior.room==room && !prior.cancelled {
                    return Ok(prior);
                }
                return Err(PlatformError::Conflict("another spectator session is in use"));
            }
            if tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM tickets WHERE player=?1)
                 OR EXISTS(SELECT 1 FROM federated_queue_members WHERE player=?1 AND released=0)
                 OR EXISTS(SELECT 1 FROM spectator_sessions WHERE player=?1)",
                [&id],|r|r.get::<_,bool>(0),
            )? {
                return Err(PlatformError::Conflict("player is already participating"));
            }
            let request=Uuid::new_v4();
            tx.execute(
                "INSERT INTO pooled_spectators(player,friend,pool,host,match_id,mode,request) VALUES(?,?,?,?,?,?,?)",
                params![id,friend.to_string(),pool.to_string(),host.to_string(),room_text,mode,request.to_string()],
            )?;
            Ok(PooledSpectator {viewer:parse_id(&id)?,friend,pool,host,room,request,cancelled:false,spectator_id:None})
        })).await
    }

    pub async fn publish_pooled_spectator(
        &self,
        request: Uuid,
        hosted: HostedSpectator,
    ) -> Result<()> {
        if hosted.address.is_empty() || hosted.spectator_id.is_nil() {
            return Err(PlatformError::HostUnavailable);
        }
        self.run(move |state|state.transaction(|tx| {
            let request=request.to_string();
            let row=tx.query_row(
                "SELECT player, match_id, spectator_id FROM pooled_spectators WHERE request=? AND NOT cancelled",
                [&request],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<String>>(2)?)),
            ).optional()?.ok_or(PlatformError::Conflict("spectator session was closed"))?;
            let new_id=hosted.spectator_id.to_string();
            tx.execute("UPDATE pooled_spectators SET address=?,spectator_id=? WHERE request=?",
                params![hosted.address,new_id,request],
            )?;
            if row.2.as_deref()!=Some(new_id.as_str()) {
                event(tx,&row.0,json!({"type":"spectator_entered","matchId":row.1}))?;
            }
            Ok(())
        })).await
    }

    pub async fn cancel_pooled_spectator(&self, token: &str) -> Result<Option<PooledSpectator>> {
        let key = digest(token);
        self.run(move |state| {
            state.transaction(|tx| {
                let viewer = player(tx, &key)?;
                tx.execute(
                    "UPDATE pooled_spectators SET cancelled=1 WHERE player=?",
                    [&viewer],
                )?;
                current(tx, &viewer)
            })
        })
        .await
    }

    pub async fn cancel_pooled_spectator_request(&self, request: Uuid) -> Result<()> {
        self.run(move |state| {
            state.transaction(|tx| {
                tx.execute(
                    "UPDATE pooled_spectators SET cancelled=1 WHERE request=?",
                    [request.to_string()],
                )?;
                Ok(())
            })
        })
        .await
    }

    pub async fn pooled_spectators(&self, pool: Uuid) -> Result<Vec<PooledSpectator>> {
        self.run(move |state| {
            let db=state.db.lock().map_err(|_|PlatformError::WorkerUnavailable)?;
            let mut stmt=db.prepare(
                "SELECT player,friend,pool,host,match_id,request,cancelled,spectator_id FROM pooled_spectators WHERE pool=?",
            )?;
            let mut claims = stmt.query_map([pool.to_string()],decode)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            drop(stmt);
            for claim in claims.iter_mut().filter(|claim| !claim.cancelled) {
                let still_visible = halcyon_community::may_spectate(
                    &db, &claim.viewer.to_string(), &claim.friend.to_string(),
                ).map_err(community_error)?;
                if !still_visible {
                    db.execute(
                        "UPDATE pooled_spectators SET cancelled=1 WHERE request=?",
                        [claim.request.to_string()],
                    )?;
                    claim.cancelled = true;
                }
            }
            Ok(claims)
        }).await
    }

    /// Clear the local claim after authenticated host revocation or a committed terminal allocation.
    pub async fn close_pooled_spectator(&self, request: Uuid) -> Result<()> {
        self.run(move |state|state.transaction(|tx| {
            let prior=tx.query_row(
                "DELETE FROM pooled_spectators WHERE request=? RETURNING player,match_id,spectator_id",
                [request.to_string()],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<String>>(2)?)),
            ).optional()?;
            if let Some((viewer,room,Some(_)))=prior {
                event(tx,&viewer,json!({"type":"spectator_left","matchId":room}))?;
            }
            Ok(())
        })).await
    }
}
