//! Player-owned metadata and social graph. Changes are serialized in SQLite
//! transactions so repeat deliveries cannot create duplicate friendships.
use super::{Error, now};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};
use uuid::Uuid;

#[derive(Clone)]
pub(crate) struct Meta(Arc<Mutex<Connection>>);
impl Meta {
    pub fn open(path: &Path) -> Result<Self, Error> {
        let db = Connection::open(path)?;
        db.execute_batch("PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS ce_preferences(player TEXT NOT NULL, category TEXT NOT NULL, data TEXT NOT NULL,
                PRIMARY KEY(player,category));
            CREATE TABLE IF NOT EXISTS ce_friends(sender TEXT NOT NULL,recipient TEXT NOT NULL,confirmed INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY(sender,recipient),CHECK(sender<>recipient));
            CREATE INDEX IF NOT EXISTS ce_friends_recipient ON ce_friends(recipient);
            CREATE TABLE IF NOT EXISTS ce_metrics(player TEXT NOT NULL,match_id TEXT NOT NULL,data TEXT NOT NULL,
                PRIMARY KEY(player,match_id));")?;
        Ok(Self(Arc::new(Mutex::new(db))))
    }

    async fn transaction<R: Send + 'static>(
        &self,
        work: impl FnOnce(&Transaction<'_>) -> Result<R, Error> + Send + 'static,
    ) -> Result<R, Error> {
        let db = self.0.clone();
        tokio::task::spawn_blocking(move || {
            let mut db = db.lock().map_err(|_| Error::Internal)?;
            let tx = db.transaction()?;
            let result = work(&tx)?;
            tx.commit()?;
            Ok(result)
        })
        .await
        .map_err(|_| Error::Internal)?
    }
    pub async fn prefs(&self, id: Uuid, category: &'static str) -> Result<Value, Error> {
        self.transaction(move |tx| {
            let text: Option<String> = tx
                .query_row(
                    "SELECT data FROM ce_preferences WHERE player=? AND category=?",
                    params![id.to_string(), category],
                    |r| r.get(0),
                )
                .optional()?;
            text.map(|v| serde_json::from_str(&v))
                .transpose()
                .map(|v| v.unwrap_or(Value::Null))
                .map_err(Into::into)
        })
        .await
    }
    pub async fn set(&self, id: Uuid, category: &'static str, value: Value) -> Result<(), Error> {
        self.transaction(move|tx|{
            tx.execute("INSERT INTO ce_preferences VALUES(?,?,?) ON CONFLICT(player,category) DO UPDATE SET data=excluded.data",
                params![id.to_string(),category,value.to_string()])?;Ok(())
        }).await
    }
    pub async fn metrics(&self, id: Uuid, room: Uuid, fields: Vec<Value>) -> Result<(), Error> {
        self.transaction(move|tx|{
            tx.execute("INSERT INTO ce_metrics VALUES(?,?,?) ON CONFLICT(player,match_id) DO UPDATE SET data=excluded.data",
                params![id.to_string(),room.to_string(),Value::Array(fields).to_string()])?;Ok(())
        }).await
    }
    pub async fn friends(&self, id: Uuid) -> Result<Value, Error> {
        self.transaction(move|tx|{
            let me=id.to_string();
            let mut q=tx.prepare("SELECT CASE WHEN sender=?1 THEN recipient ELSE sender END,confirmed FROM ce_friends
                WHERE sender=?1 OR recipient=?1 ORDER BY 1")?;
            let mut confirmed=Vec::new();let mut pending=Vec::new();
            for row in q.query_map([&me],|r|Ok((r.get::<_,String>(0)?,r.get::<_,bool>(1)?)))?{
                let (player,accepted)=row?;
                let value=json!({"playerUUID":player});
                if accepted{confirmed.push(value)}else{pending.push(value)}
            }
            Ok(json!({"confirmed":confirmed,"pending":pending,"numOffline":0}))
        }).await
    }
    pub async fn friendship(
        &self,
        id: Uuid,
        target: String,
        method: &'static str,
    ) -> Result<Value, Error> {
        self.transaction(move|tx|{
            let me=id.to_string();
            let mut query=tx.prepare("SELECT id FROM players WHERE id=? OR name=? LIMIT 2")?;
            let ids=query.query_map(params![target,target],|r|r.get::<_,String>(0))?.collect::<Result<Vec<_>,_>>()?;
            let [other]=ids.as_slice() else{return Err(Error::Invalid)};
            if *other==me{return Err(Error::Invalid)}
            match method{
                "request"=>{
                    let reverse=tx.execute("UPDATE ce_friends SET confirmed=1 WHERE sender=? AND recipient=?",params![other,me])?;
                    if reverse==0{tx.execute("INSERT OR IGNORE INTO ce_friends(sender,recipient) VALUES(?,?)",params![me,other])?;}
                }
                "confirm"=>{if tx.execute("UPDATE ce_friends SET confirmed=1 WHERE sender=? AND recipient=?",params![other,me])?==0{return Err(Error::Invalid)}}
                "reject"=>{tx.execute("DELETE FROM ce_friends WHERE sender=? AND recipient=? AND confirmed=0",params![other,me])?;}
                "delete"=>{tx.execute("DELETE FROM ce_friends WHERE (sender=?1 AND recipient=?2) OR (sender=?2 AND recipient=?1)",params![me,other])?;}
                _=>return Err(Error::Unsupported),
            }
            Ok(json!({"success":true}))
        }).await
    }
    pub async fn ranking(&self) -> Result<Value, Error> {
        self.transaction(|tx|{
            let mut q=tx.prepare("SELECT p.id,p.name,COUNT(*) AS played FROM events e JOIN players p ON p.id=e.player
                WHERE json_extract(e.body,'$.type')='match_finished' GROUP BY p.id ORDER BY played DESC,p.id LIMIT 100")?;
            let entries=q.query_map([],|r|Ok(json!({"playerUUID":r.get::<_,String>(0)?,"handle":r.get::<_,String>(1)?,"completed":r.get::<_,u64>(2)?})))?.collect::<Result<Vec<_>,_>>()?;
            Ok(json!({"leaderboards":[{"entries":entries}],"events":[],"time":now()}))
        }).await
    }
}
