use super::*;
use openraft::{
    Entry, EntryPayload, ErrorSubject, ErrorVerb, LogState, RaftLogReader, RaftSnapshotBuilder,
    RaftStorage, Snapshot, SnapshotMeta, StorageError, Vote,
};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    fmt::Debug,
    io::Cursor,
    ops::{Bound, RangeBounds},
    path::Path,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub(crate) struct Store(Arc<Mutex<Database>>);
struct Database {
    db: Connection,
    machine: Machine,
}
mod state;
#[cfg(test)]
mod tests;
type E = StorageError<Uuid>;
fn maperr(e: anyhow::Error, verb: ErrorVerb) -> E {
    StorageError::from_io_error(
        ErrorSubject::Store,
        verb,
        std::io::Error::other(e.to_string()),
    )
}
fn get<T: DeserializeOwned>(c: &Connection, k: &str) -> anyhow::Result<Option<T>> {
    let b: Option<Vec<u8>> = c
        .query_row("select data from metadata where key=?", [k], |r| r.get(0))
        .optional()?;
    b.map(|b| decode(&b)).transpose()
}
fn put(c: &Connection, k: &str, v: &impl Serialize) -> anyhow::Result<()> {
    c.execute(
        "insert or replace into metadata values(?,?)",
        params![k, encode(v)?],
    )?;
    Ok(())
}
#[allow(
    clippy::result_large_err,
    reason = "OpenRaft storage helpers share the trait error type"
)]
impl Store {
    pub fn open(path: &Path, pool: Uuid, node: Uuid) -> anyhow::Result<Self> {
        let mut c = Connection::open(path)?;
        c.execute_batch("pragma journal_mode=WAL; pragma synchronous=FULL; create table if not exists metadata(key text primary key,data blob not null); create table if not exists log(idx integer primary key,data blob not null);")?;
        let tx = c.transaction()?;
        match get::<(Uuid, Uuid)>(&tx, "identity")? {
            Some(id) => anyhow::ensure!(
                id == (pool, node),
                "database belongs to another pool or node"
            ),
            None => put(&tx, "identity", &(pool, node))?,
        }
        state::initialize(&tx)?;
        let machine = state::restore(&tx)?;
        tx.commit()?;
        Ok(Self(Arc::new(Mutex::new(Database { db: c, machine }))))
    }
    fn read<T>(&self, f: impl FnOnce(&Connection) -> anyhow::Result<T>) -> Result<T, E> {
        f(&self.0.lock().unwrap().db).map_err(|e| maperr(e, ErrorVerb::Read))
    }
    fn write<T>(&self, f: impl FnOnce(&Connection) -> anyhow::Result<T>) -> Result<T, E> {
        let mut c = self.0.lock().unwrap();
        let job = || -> anyhow::Result<T> {
            let tx = c.db.transaction()?;
            let v = f(&tx)?;
            tx.commit()?;
            Ok(v)
        };
        job().map_err(|e| maperr(e, ErrorVerb::Write))
    }
    #[cfg(test)]
    pub fn machine(&self) -> Result<Machine, E> {
        Ok(self.0.lock().unwrap().machine.clone())
    }
    pub fn membership(&self) -> StoredMembership<Uuid, BasicNode> {
        self.0.lock().unwrap().machine.membership.clone()
    }
    pub fn inspect<T>(&self, reader: impl FnOnce(&Pool) -> T) -> T {
        reader(&self.0.lock().unwrap().machine.pool)
    }
    pub fn pool_changes(&self, known: Option<u64>) -> Result<(u64, Option<Pool>), E> {
        let state = self.0.lock().unwrap();
        let index = state.machine.applied.map_or(0, |id| id.index);
        Ok((
            index,
            (known != Some(index)).then(|| state.machine.pool.clone()),
        ))
    }
}
impl RaftLogReader<Types> for Store {
    async fn try_get_log_entries<R: RangeBounds<u64> + Clone + Debug + Send>(
        &mut self,
        range: R,
    ) -> Result<Vec<Entry<Types>>, E> {
        let start = match range.start_bound() {
            Bound::Included(x) => *x,
            Bound::Excluded(x) => x.saturating_add(1),
            Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            Bound::Included(x) => *x,
            Bound::Excluded(0) => return Ok(Vec::new()),
            Bound::Excluded(x) => x - 1,
            Bound::Unbounded => i64::MAX as u64,
        };
        self.read(|c| {
            let mut stmt =
                c.prepare("select data from log where idx between ? and ? order by idx")?;
            let mut r = stmt.query(params![start, end])?;
            let mut out = Vec::new();
            while let Some(row) = r.next()? {
                out.push(decode(&row.get::<_, Vec<u8>>(0)?)?)
            }
            Ok(out)
        })
    }
}
impl RaftSnapshotBuilder<Types> for Store {
    async fn build_snapshot(&mut self) -> Result<Snapshot<Types>, E> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || state::build(&store))
            .await
            .map_err(|e| maperr(e.into(), ErrorVerb::Write))?
            .map_err(|e| maperr(e, ErrorVerb::Write))
    }
}
impl RaftStorage<Types> for Store {
    type LogReader = Self;
    type SnapshotBuilder = Self;
    async fn save_vote(&mut self, v: &Vote<Uuid>) -> Result<(), E> {
        self.write(|c| put(c, "vote", v))
    }
    async fn read_vote(&mut self) -> Result<Option<Vote<Uuid>>, E> {
        self.read(|c| get(c, "vote"))
    }
    async fn save_committed(&mut self, v: Option<LogId<Uuid>>) -> Result<(), E> {
        self.write(|c| put(c, "commit", &v))
    }
    async fn read_committed(&mut self) -> Result<Option<LogId<Uuid>>, E> {
        self.read(|c| Ok(get(c, "commit")?.flatten()))
    }
    async fn get_log_state(&mut self) -> Result<LogState<Types>, E> {
        self.read(|c| {
            let purged = get(c, "purged")?;
            let last: Option<Vec<u8>> = c
                .query_row("select data from log order by idx desc limit 1", [], |r| {
                    r.get(0)
                })
                .optional()?;
            let last = last
                .map(|b| decode::<Entry<Types>>(&b))
                .transpose()?
                .map(|e| e.log_id)
                .or(purged);
            Ok(LogState {
                last_purged_log_id: purged,
                last_log_id: last,
            })
        })
    }
    async fn get_log_reader(&mut self) -> Self {
        self.clone()
    }
    async fn append_to_log<I: IntoIterator<Item = Entry<Types>> + Send>(
        &mut self,
        entries: I,
    ) -> Result<(), E> {
        self.write(|c| {
            let mut stmt = c.prepare_cached("insert or replace into log values(?,?)")?;
            for e in entries {
                stmt.execute(params![e.log_id.index, encode(&e)?])?;
            }
            Ok(())
        })
    }
    async fn delete_conflict_logs_since(&mut self, id: LogId<Uuid>) -> Result<(), E> {
        self.write(|c| {
            c.execute("delete from log where idx>=?", [id.index])?;
            Ok(())
        })
    }
    async fn purge_logs_upto(&mut self, id: LogId<Uuid>) -> Result<(), E> {
        self.write(|c| {
            c.execute("delete from log where idx<=?", [id.index])?;
            put(c, "purged", &id)
        })
    }
    async fn last_applied_state(
        &mut self,
    ) -> Result<(Option<LogId<Uuid>>, StoredMembership<Uuid, BasicNode>), E> {
        let state = self.0.lock().unwrap();
        Ok((state.machine.applied, state.machine.membership.clone()))
    }
    async fn apply_to_state_machine(
        &mut self,
        entries: &[Entry<Types>],
    ) -> Result<Vec<Outcome>, E> {
        state::apply(&mut self.0.lock().unwrap(), entries).map_err(|e| maperr(e, ErrorVerb::Write))
    }
    async fn get_snapshot_builder(&mut self) -> Self {
        self.clone()
    }
    async fn begin_receiving_snapshot(&mut self) -> Result<Box<Cursor<Vec<u8>>>, E> {
        Ok(Box::new(Cursor::default()))
    }
    async fn install_snapshot(
        &mut self,
        meta: &SnapshotMeta<Uuid, BasicNode>,
        snap: Box<Cursor<Vec<u8>>>,
    ) -> Result<(), E> {
        let bytes = snap.into_inner();
        let meta = meta.clone();
        let store = self.clone();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let machine: Machine = decode(&bytes)?;
            anyhow::ensure!(
                machine.applied == meta.last_log_id && machine.membership == meta.last_membership,
                "snapshot metadata mismatch"
            );
            let mut state = store.0.lock().unwrap();
            anyhow::ensure!(
                machine.applied.map(|id| id.index) >= state.machine.applied.map(|id| id.index),
                "obsolete incoming snapshot"
            );
            let tx = state.db.transaction()?;
            state::checkpoint(&tx, &meta, &bytes)?;
            tx.commit()?;
            state.machine = machine;
            Ok(())
        })
        .await
        .map_err(|e| maperr(e.into(), ErrorVerb::Write))?
        .map_err(|e| maperr(e, ErrorVerb::Write))
    }
    async fn get_current_snapshot(&mut self) -> Result<Option<Snapshot<Types>>, E> {
        self.read(state::snapshot)
    }
}
