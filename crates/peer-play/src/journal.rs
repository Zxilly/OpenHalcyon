//! Durable local host outbox. Admitting a room records its identity before activating the game server.
use anyhow::ensure;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use std::{path::Path, sync::Mutex};
use uuid::Uuid;

pub(crate) struct Journal(Mutex<Connection>);
impl Journal {
    pub fn open(path: &Path, pool: Uuid, node: Uuid) -> anyhow::Result<Self> {
        let mut c = Connection::open(path)?;
        c.execute_batch("pragma journal_mode=WAL; pragma synchronous=FULL;
            create table if not exists peer_identity(singleton integer primary key check(singleton=1),pool text not null,node text not null,sequence integer not null);
            create table if not exists hosted(id text primary key, address text not null, result text, delivered integer not null default 0);")?;
        let tx = c.transaction()?;
        tx.execute(
            "insert or ignore into peer_identity values(1,?,?,0)",
            params![pool.to_string(), node.to_string()],
        )?;
        let correct: bool = tx.query_row(
            "select pool=? and node=? from peer_identity",
            params![pool.to_string(), node.to_string()],
            |r| r.get(0),
        )?;
        ensure!(correct, "host journal belongs to a different pool or node");
        tx.commit()?;
        Ok(Self(Mutex::new(c)))
    }
    pub fn sequence(&self) -> anyhow::Result<u64> {
        Ok(self.0.lock().unwrap().query_row(
            "update peer_identity set sequence=sequence+1 returning sequence",
            [],
            |r| r.get(0),
        )?)
    }
    pub fn prepared(&self, id: Uuid, address: &str) -> anyhow::Result<()> {
        self.0.lock().unwrap().execute("insert into hosted(id,address) values(?,?) on conflict(id) do update set address=excluded.address where result is null",params![id.to_string(),address])?;
        Ok(())
    }
    pub fn finish(&self, id: Uuid, result: &Value) -> anyhow::Result<bool> {
        let mut db = self.0.lock().unwrap();
        let tx = db.transaction()?;
        let prev = tx
            .query_row(
                "select result from hosted where id=?",
                [id.to_string()],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?;
        let Some(prior) = prev else { return Ok(false) };
        if let Some(prior) = prior {
            ensure!(
                serde_json::from_str::<Value>(&prior)? == *result,
                "conflicting local settlement"
            )
        } else {
            tx.execute(
                "update hosted set result=? where id=?",
                params![serde_json::to_string(result)?, id.to_string()],
            )?;
        }
        tx.commit()?;
        Ok(true)
    }
    pub fn outbox(&self) -> anyhow::Result<Vec<(Uuid, Value)>> {
        let db = self.0.lock().unwrap();
        let mut stmt =
            db.prepare("select id,result from hosted where result is not null and delivered=0")?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push((
                r.get::<_, String>(0)?.parse()?,
                serde_json::from_str(&r.get::<_, String>(1)?)?,
            ));
        }
        Ok(out)
    }
    pub fn delivered(&self, id: Uuid) -> anyhow::Result<()> {
        self.0
            .lock()
            .unwrap()
            .execute("update hosted set delivered=1 where id=?", [id.to_string()])?;
        Ok(())
    }
}
