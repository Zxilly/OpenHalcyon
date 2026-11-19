//! Durable, player-scoped preferences and platform telemetry.
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
            CREATE TABLE IF NOT EXISTS ce_metrics(player TEXT NOT NULL,match_id TEXT NOT NULL,data TEXT NOT NULL,
                PRIMARY KEY(player,match_id));")?;
        super::activity::initialize(&db)?;
        super::provider::initialize(&db)?;
        Ok(Self(Arc::new(Mutex::new(db))))
    }

    pub(crate) async fn transaction<R: Send + 'static>(
        &self,
        work: impl FnOnce(&Transaction<'_>) -> Result<R, Error> + Send + 'static,
    ) -> Result<R, Error> {
        self.with_behavior(rusqlite::TransactionBehavior::Deferred, work)
            .await
    }
    pub(crate) async fn with_behavior<R: Send + 'static>(
        &self,
        behavior: rusqlite::TransactionBehavior,
        work: impl FnOnce(&Transaction<'_>) -> Result<R, Error> + Send + 'static,
    ) -> Result<R, Error> {
        let db = self.0.clone();
        tokio::task::spawn_blocking(move || {
            let mut db = db.lock().map_err(|_| Error::Internal)?;
            let tx = db.transaction_with_behavior(behavior)?;
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
    pub async fn ranking(&self) -> Result<Value, Error> {
        self.transaction(|tx|{
            let mut q=tx.prepare("SELECT p.id,p.name,COUNT(*) AS played,SUM(c.won) FROM ce_completed c JOIN players p ON p.id=c.player
                GROUP BY p.id ORDER BY played DESC,p.id LIMIT 100")?;
            let entries=q.query_map([],|r|Ok(json!({"playerUUID":r.get::<_,String>(0)?,"handle":r.get::<_,String>(1)?,"completed":r.get::<_,u64>(2)?,"wins":r.get::<_,u64>(3)?})))?.collect::<Result<Vec<_>,_>>()?;
            Ok(json!({"leaderboards":[{"leaderboardId":"completed","leaderboardTitle":"Completed matches","entries":entries}],"events":[],"time":now()}))
        }).await
    }
}
