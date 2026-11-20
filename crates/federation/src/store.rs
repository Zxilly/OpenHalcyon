use super::*;
use diesel::{connection::SimpleConnection, prelude::*};
use diesel_migrations::{MigrationHarness, embed_migrations};
use openraft::{
    Entry, EntryPayload, ErrorSubject, ErrorVerb, LogState, RaftLogReader, RaftSnapshotBuilder,
    RaftStorage, Snapshot, SnapshotMeta, StorageError, Vote,
};
use schema::{log, metadata};
mod schema;
const MIGRATIONS: diesel_migrations::EmbeddedMigrations = embed_migrations!("migrations");
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
    db: SqliteConnection,
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
#[derive(Clone, Copy)]
enum MetadataKey {
    Identity,
    Vote,
    Commit,
    Purged,
    StateVersion,
    LegacyMachine,
    LegacySnapshot,
    LegacyPoolRevision,
}

impl MetadataKey {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Identity => "identity",
            Self::Vote => "vote",
            Self::Commit => "commit",
            Self::Purged => "purged",
            Self::StateVersion => "sm_version",
            Self::LegacyMachine => "machine",
            Self::LegacySnapshot => "snapshot",
            Self::LegacyPoolRevision => "pool_revision",
        }
    }
}

fn get<T: DeserializeOwned>(
    c: &mut SqliteConnection,
    key: MetadataKey,
) -> anyhow::Result<Option<T>> {
    let b = metadata::table
        .find(key.as_str())
        .select(metadata::data)
        .first::<Vec<u8>>(c)
        .optional()?;
    b.map(|b| decode(&b)).transpose()
}
fn put(c: &mut SqliteConnection, key: MetadataKey, v: &impl Serialize) -> anyhow::Result<()> {
    diesel::replace_into(metadata::table)
        .values((
            metadata::key.eq(key.as_str()),
            metadata::data.eq(encode(v)?),
        ))
        .execute(c)?;
    Ok(())
}
#[allow(
    clippy::result_large_err,
    reason = "OpenRaft storage helpers share the trait error type"
)]
impl Store {
    pub fn open(path: &Path, pool: Uuid, node: Uuid) -> anyhow::Result<Self> {
        let mut c = halcyon_storage::DatabaseAddress::new(path).connect()?;
        c.batch_execute("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")?;
        let machine = c.immediate_transaction::<_, anyhow::Error, _>(|tx| {
            tx.run_pending_migrations(MIGRATIONS)
                .map_err(anyhow::Error::from_boxed)?;
            match get::<(Uuid, Uuid)>(tx, MetadataKey::Identity)? {
                Some(id) => anyhow::ensure!(
                    id == (pool, node),
                    "database belongs to another pool or node"
                ),
                None => put(tx, MetadataKey::Identity, &(pool, node))?,
            }
            state::initialize(tx)?;
            state::restore(tx)
        })?;
        Ok(Self(Arc::new(Mutex::new(Database { db: c, machine }))))
    }
    fn read<T>(&self, f: impl FnOnce(&mut SqliteConnection) -> anyhow::Result<T>) -> Result<T, E> {
        f(&mut self.0.lock().unwrap().db).map_err(|e| maperr(e, ErrorVerb::Read))
    }
    fn write<T>(&self, f: impl FnOnce(&mut SqliteConnection) -> anyhow::Result<T>) -> Result<T, E> {
        let mut c = self.0.lock().unwrap();
        c.db.transaction(f).map_err(|e| maperr(e, ErrorVerb::Write))
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
            log::table
                .filter(log::idx.between(i64::try_from(start)?, i64::try_from(end)?))
                .order(log::idx)
                .select(log::data)
                .load_iter::<Vec<u8>, diesel::connection::DefaultLoadingMode>(c)?
                .map(|row| decode(&row?))
                .collect()
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
        self.write(|c| put(c, MetadataKey::Vote, v))
    }
    async fn read_vote(&mut self) -> Result<Option<Vote<Uuid>>, E> {
        self.read(|c| get(c, MetadataKey::Vote))
    }
    async fn save_committed(&mut self, v: Option<LogId<Uuid>>) -> Result<(), E> {
        self.write(|c| put(c, MetadataKey::Commit, &v))
    }
    async fn read_committed(&mut self) -> Result<Option<LogId<Uuid>>, E> {
        self.read(|c| Ok(get(c, MetadataKey::Commit)?.flatten()))
    }
    async fn get_log_state(&mut self) -> Result<LogState<Types>, E> {
        self.read(|c| {
            let purged = get(c, MetadataKey::Purged)?;
            let last = log::table
                .order(log::idx.desc())
                .select(log::data)
                .first::<Vec<u8>>(c)
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
            for e in entries {
                diesel::replace_into(log::table)
                    .values((
                        log::idx.eq(i64::try_from(e.log_id.index)?),
                        log::data.eq(encode(&e)?),
                    ))
                    .execute(c)?;
            }
            Ok(())
        })
    }
    async fn delete_conflict_logs_since(&mut self, id: LogId<Uuid>) -> Result<(), E> {
        self.write(|c| {
            diesel::delete(log::table.filter(log::idx.ge(i64::try_from(id.index)?))).execute(c)?;
            Ok(())
        })
    }
    async fn purge_logs_upto(&mut self, id: LogId<Uuid>) -> Result<(), E> {
        self.write(|c| {
            diesel::delete(log::table.filter(log::idx.le(i64::try_from(id.index)?))).execute(c)?;
            put(c, MetadataKey::Purged, &id)
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
            state
                .db
                .transaction(|tx| state::checkpoint(tx, &meta, &bytes))?;
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
