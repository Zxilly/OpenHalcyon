//! Durable local host outbox. Admitting a room records its identity before activating the game server.
use anyhow::ensure;
use halcyon_matchmaking::Ticket;
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
            create table if not exists hosted(id text primary key, address text not null, result text, delivered integer not null default 0);
            create index if not exists hosted_outbox on hosted(id) where result is not null and delivered=0;
            create index if not exists hosted_unsettled on hosted(id) where result is null;
            create table if not exists outgoing_chat(id text not null, home text not null, body text not null,
                expires integer not null default(unixepoch()+120), next_attempt integer not null default 0,
                primary key(id,home));
            create index if not exists outgoing_chat_due on outgoing_chat(next_attempt);
            create table if not exists enqueue_payloads(id text primary key, payload text not null);")?;
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
    pub fn queue_chat(
        &self,
        chat: &halcyon_federation::MatchChat,
        homes: std::collections::BTreeSet<Uuid>,
    ) -> anyhow::Result<()> {
        let mut db = self.0.lock().unwrap();
        let tx = db.transaction()?;
        tx.execute("delete from outgoing_chat where expires <= unixepoch()", [])?;
        let queued: usize = tx.query_row("select count(*) from outgoing_chat", [], |r| r.get(0))?;
        ensure!(queued + homes.len() <= 4096, "outgoing chat is full");
        let body = serde_json::to_string(chat)?;
        for home in homes {
            tx.execute(
                "insert into outgoing_chat(id,home,body) values(?,?,?)",
                params![chat.id.to_string(), home.to_string(), body],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn due_chat(&self) -> anyhow::Result<Vec<(Uuid, halcyon_federation::MatchChat)>> {
        let db = self.0.lock().unwrap();
        db.execute("delete from outgoing_chat where expires <= unixepoch()", [])?;
        let mut stmt = db.prepare(
            "select home,body from outgoing_chat where next_attempt <= unixepoch() order by rowid limit 32",
        )?;
        let mut rows = stmt.query([])?;
        let mut batch = Vec::new();
        while let Some(row) = rows.next()? {
            batch.push((
                row.get::<_, String>(0)?.parse()?,
                serde_json::from_str(&row.get::<_, String>(1)?)?,
            ));
        }
        Ok(batch)
    }

    pub fn chat_attempt(&self, id: Uuid, home: Uuid, delivered: bool) -> anyhow::Result<()> {
        let query = if delivered {
            "delete from outgoing_chat where id=? and home=?"
        } else {
            "update outgoing_chat set next_attempt=unixepoch()+3 where id=? and home=?"
        };
        self.0
            .lock()
            .unwrap()
            .execute(query, params![id.to_string(), home.to_string()])?;
        Ok(())
    }

    pub fn sequence(&self) -> anyhow::Result<u64> {
        Ok(self.0.lock().unwrap().query_row(
            "update peer_identity set sequence=sequence+1 returning sequence",
            [],
            |r| r.get(0),
        )?)
    }
    pub fn ticket(&self, id: Uuid) -> anyhow::Result<Option<Ticket>> {
        let payload: Option<String> = self
            .0
            .lock()
            .unwrap()
            .query_row(
                "SELECT payload FROM enqueue_payloads WHERE id=?",
                [id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        payload
            .map(|body| serde_json::from_str(&body))
            .transpose()
            .map_err(Into::into)
    }

    /// Concurrent enqueues for one intent always return the first durable payload.
    pub fn freeze_ticket(&self, ticket: Ticket) -> anyhow::Result<Ticket> {
        let mut connection = self.0.lock().unwrap();
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let id = ticket.id.to_string();
        tx.execute(
            "INSERT INTO enqueue_payloads(id,payload) VALUES(?,?) ON CONFLICT(id) DO NOTHING",
            params![id, serde_json::to_string(&ticket)?],
        )?;
        let payload: String = tx.query_row(
            "SELECT payload FROM enqueue_payloads WHERE id=?",
            [&id],
            |row| row.get(0),
        )?;
        let frozen = serde_json::from_str(&payload)?;
        tx.commit()?;
        Ok(frozen)
    }

    /// Upgrade an intent created before payload freezing. The pool's exact
    /// committed ticket wins over a provisional snapshot made during upgrade.
    pub fn adopt_legacy_ticket(&self, ticket: &Ticket) -> anyhow::Result<()> {
        self.0.lock().unwrap().execute(
            "INSERT INTO enqueue_payloads(id,payload) VALUES(?,?)
             ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",
            params![ticket.id.to_string(), serde_json::to_string(ticket)?],
        )?;
        Ok(())
    }
    pub fn prepared(&self, id: Uuid, address: &str) -> anyhow::Result<()> {
        self.0.lock().unwrap().execute("insert into hosted(id,address) values(?,?) on conflict(id) do update set address=excluded.address where result is null",params![id.to_string(),address])?;
        Ok(())
    }
    pub fn unsettled_rooms(&self) -> anyhow::Result<Vec<Uuid>> {
        let db = self.0.lock().unwrap();
        let mut query = db.prepare("SELECT id FROM hosted WHERE result IS NULL")?;
        let ids = query.query_map([], |r| r.get::<_, String>(0))?;
        ids.map(|id| Ok(id?.parse()?)).collect()
    }

    pub fn discard_preparation(&self, room: Uuid) -> anyhow::Result<()> {
        self.0.lock().unwrap().execute(
            "DELETE FROM hosted WHERE id=? AND result IS NULL",
            [room.to_string()],
        )?;
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

#[cfg(test)]
mod chat_tests {
    use super::Journal;
    use halcyon_federation::MatchChat;
    use uuid::Uuid;

    #[test]
    fn undelivered_home_reopens_without_resending_to_the_acknowledged_home() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("peer.sqlite");
        let pool = Uuid::new_v4();
        let home = Uuid::new_v4();
        let peer = Uuid::new_v4();
        let message = MatchChat {
            id: Uuid::new_v4(),
            room: Uuid::new_v4(),
            sender: Uuid::new_v4(),
            text: "hello".into(),
        };
        let journal = Journal::open(&database, pool, home).unwrap();
        journal.queue_chat(&message, [home, peer].into()).unwrap();
        journal.chat_attempt(message.id, home, true).unwrap();
        drop(journal);
        let journal = Journal::open(&database, pool, home).unwrap();
        let pending = journal.due_chat().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!((pending[0].0, pending[0].1.id), (peer, message.id));
        journal.chat_attempt(message.id, peer, false).unwrap();
        assert!(journal.due_chat().unwrap().is_empty());
        journal
            .0
            .lock()
            .unwrap()
            .execute("update outgoing_chat set expires=0", [])
            .unwrap();
        assert!(journal.due_chat().unwrap().is_empty());
        assert_eq!(
            journal
                .0
                .lock()
                .unwrap()
                .query_row("select count(*) from outgoing_chat", [], |r| r
                    .get::<_, u32>(0))
                .unwrap(),
            0
        );
    }
}
