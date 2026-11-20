//! Durable local host outbox. A prepared room is recorded before game admission.
mod schema;
#[cfg(test)]
mod tests;
use anyhow::{Context, ensure};
use diesel::{connection::SimpleConnection, prelude::*, upsert::excluded};
use diesel_migrations::{MigrationHarness, embed_migrations};
use halcyon_matchmaking::Ticket;
use schema::{
    enqueue_payloads as tickets, hosted, outgoing_chat as chat, peer_identity as identity,
    unixepoch,
};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    path::Path,
    sync::{Mutex, MutexGuard},
};
use uuid::Uuid;

const MIGRATIONS: diesel_migrations::EmbeddedMigrations = embed_migrations!("migrations");

pub(crate) struct Journal(Mutex<SqliteConnection>);
impl Journal {
    pub fn open(path: &Path, pool: Uuid, node: Uuid) -> anyhow::Result<Self> {
        let mut db = halcyon_storage::DatabaseAddress::new(path).connect()?;
        db.batch_execute("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")?;
        db.immediate_transaction::<_, anyhow::Error, _>(|db| {
            db.run_pending_migrations(MIGRATIONS)
                .map_err(anyhow::Error::from_boxed)?;
            let pool = pool.to_string();
            let node = node.to_string();
            diesel::insert_into(identity::table)
                .values((
                    identity::singleton.eq(1),
                    identity::pool.eq(&pool),
                    identity::node.eq(&node),
                    identity::sequence.eq(0i64),
                ))
                .on_conflict(identity::singleton)
                .do_nothing()
                .execute(db)?;
            let owners = identity::table
                .find(1)
                .select((identity::pool, identity::node))
                .first::<(String, String)>(db)?;
            ensure!(
                owners == (pool, node),
                "host journal belongs to a different pool or node"
            );
            Ok(())
        })?;
        Ok(Self(Mutex::new(db)))
    }
    fn db(&self) -> anyhow::Result<MutexGuard<'_, SqliteConnection>> {
        self.0
            .lock()
            .map_err(|_| anyhow::anyhow!("host journal connection lock poisoned"))
    }
    pub fn queue_chat(
        &self,
        message: &halcyon_federation::MatchChat,
        homes: BTreeSet<Uuid>,
    ) -> anyhow::Result<()> {
        let id = message.id.to_string();
        let body = serde_json::to_string(message)?;
        self.db()?.immediate_transaction(|db| {
            diesel::delete(chat::table.filter(chat::expires.le(unixepoch()))).execute(db)?;
            let queued = chat::table.count().get_result::<i64>(db)?;
            ensure!(
                queued + i64::try_from(homes.len())? <= 4096,
                "outgoing chat is full"
            );
            for home in homes {
                diesel::insert_into(chat::table)
                    .values((
                        chat::id.eq(&id),
                        chat::home.eq(home.to_string()),
                        chat::body.eq(&body),
                    ))
                    .execute(db)?;
            }
            Ok(())
        })
    }
    pub fn due_chat(&self) -> anyhow::Result<Vec<(Uuid, halcyon_federation::MatchChat)>> {
        let mut db = self.db()?;
        diesel::delete(chat::table.filter(chat::expires.le(unixepoch()))).execute(&mut *db)?;
        chat::table
            .filter(chat::next_attempt.le(unixepoch()))
            .order(chat::rowid.asc())
            .limit(32)
            .select((chat::home, chat::body))
            .load::<(String, String)>(&mut *db)?
            .into_iter()
            .map(|(home, body)| Ok((home.parse()?, serde_json::from_str(&body)?)))
            .collect()
    }
    pub fn chat_attempt(&self, id: Uuid, home: Uuid, delivered: bool) -> anyhow::Result<()> {
        let row = chat::table.find((id.to_string(), home.to_string()));
        let mut db = self.db()?;
        if delivered {
            diesel::delete(row).execute(&mut *db)?;
        } else {
            diesel::update(row)
                .set(chat::next_attempt.eq(schema::unixepoch_after("now", "+3 seconds")))
                .execute(&mut *db)?;
        }
        Ok(())
    }
    pub fn sequence(&self) -> anyhow::Result<u64> {
        let seq = diesel::update(
            identity::table
                .find(1)
                .filter(identity::sequence.lt(i64::MAX)),
        )
        .set(identity::sequence.eq(identity::sequence + 1i64))
        .returning(identity::sequence)
        .get_result::<i64>(&mut *self.db()?)
        .optional()?
        .context("journal sequence exhausted")?;
        Ok(seq.try_into()?)
    }
    pub fn ticket(&self, id: Uuid) -> anyhow::Result<Option<Ticket>> {
        let body = tickets::table
            .find(id.to_string())
            .select(tickets::payload)
            .first::<String>(&mut *self.db()?)
            .optional()?;
        Ok(body.map(|b| serde_json::from_str(&b)).transpose()?)
    }
    pub fn freeze_ticket(&self, ticket: Ticket) -> anyhow::Result<Ticket> {
        let id = ticket.id.to_string();
        let payload = serde_json::to_string(&ticket)?;
        self.db()?.immediate_transaction(|db| {
            diesel::insert_into(tickets::table)
                .values((tickets::id.eq(&id), tickets::payload.eq(payload)))
                .on_conflict(tickets::id)
                .do_nothing()
                .execute(db)?;
            let body = tickets::table
                .find(id)
                .select(tickets::payload)
                .first::<String>(db)?;
            Ok(serde_json::from_str(&body)?)
        })
    }
    pub fn adopt_legacy_ticket(&self, ticket: &Ticket) -> anyhow::Result<()> {
        diesel::insert_into(tickets::table)
            .values((
                tickets::id.eq(ticket.id.to_string()),
                tickets::payload.eq(serde_json::to_string(ticket)?),
            ))
            .on_conflict(tickets::id)
            .do_update()
            .set(tickets::payload.eq(excluded(tickets::payload)))
            .execute(&mut *self.db()?)?;
        Ok(())
    }
    pub fn prepared(&self, id: Uuid, address: &str) -> anyhow::Result<()> {
        use diesel::query_dsl::methods::FilterDsl;
        FilterDsl::filter(
            diesel::insert_into(hosted::table)
                .values((hosted::id.eq(id.to_string()), hosted::address.eq(address)))
                .on_conflict(hosted::id)
                .do_update()
                .set(hosted::address.eq(excluded(hosted::address))),
            hosted::result.is_null(),
        )
        .execute(&mut *self.db()?)?;
        Ok(())
    }
    pub fn unsettled_rooms(&self) -> anyhow::Result<Vec<Uuid>> {
        hosted::table
            .filter(hosted::result.is_null())
            .order(hosted::id.asc())
            .select(hosted::id)
            .load::<String>(&mut *self.db()?)?
            .into_iter()
            .map(|id| id.parse().context("invalid stored room id"))
            .collect()
    }
    pub fn discard_preparation(&self, id: Uuid) -> anyhow::Result<()> {
        diesel::delete(
            hosted::table
                .find(id.to_string())
                .filter(hosted::result.is_null()),
        )
        .execute(&mut *self.db()?)?;
        Ok(())
    }
    pub fn finish(&self, id: Uuid, value: &Value) -> anyhow::Result<bool> {
        let id = id.to_string();
        self.db()?.immediate_transaction(|db| {
            let row = hosted::table.find(&id);
            match row
                .select(hosted::result)
                .first::<Option<String>>(db)
                .optional()?
            {
                None => return Ok(false),
                Some(None) => {
                    diesel::update(row)
                        .set(hosted::result.eq(serde_json::to_string(value)?))
                        .execute(db)?;
                }
                Some(Some(body)) => ensure!(
                    serde_json::from_str::<Value>(&body)? == *value,
                    "conflicting local settlement"
                ),
            }
            Ok(true)
        })
    }
    pub fn outbox(&self) -> anyhow::Result<Vec<(Uuid, Value)>> {
        hosted::table
            .filter(hosted::result.is_not_null().and(hosted::delivered.eq(0)))
            .order(hosted::id.asc())
            .select((hosted::id, hosted::result.assume_not_null()))
            .load::<(String, String)>(&mut *self.db()?)?
            .into_iter()
            .map(|(id, body)| Ok((id.parse()?, serde_json::from_str(&body)?)))
            .collect()
    }
    pub fn delivered(&self, id: Uuid) -> anyhow::Result<()> {
        diesel::update(hosted::table.find(id.to_string()))
            .set(hosted::delivered.eq(1))
            .execute(&mut *self.db()?)?;
        Ok(())
    }
}
