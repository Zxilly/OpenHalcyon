//! Durable state is a full checkpoint plus only the entries applied after it.
//! The Raft log may be purged independently of this journal.
use super::*;
use rusqlite::Transaction;

pub(super) fn initialize(tx: &Transaction<'_>) -> anyhow::Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS sm_checkpoint(
        id INTEGER PRIMARY KEY CHECK(id=1), meta BLOB, image BLOB NOT NULL);
        CREATE TABLE IF NOT EXISTS sm_applied(idx INTEGER PRIMARY KEY, entry BLOB NOT NULL);",
    )?;
    match get::<u32>(tx, "sm_version")? {
        Some(2) => return Ok(()),
        Some(_) => anyhow::bail!("unknown federation state machine storage version"),
        None => {}
    }
    // Previous versions stored the most recently applied state independently of
    // the last published snapshot. Keep the older snapshot available until a
    // new snapshot at least as recent as the migration checkpoint is published.
    tx.execute(
        "INSERT INTO sm_checkpoint(id,image) SELECT 1,data FROM metadata WHERE key='machine'",
        [],
    )?;
    put(tx, "sm_version", &2u32)?;
    // An older binary would otherwise silently start from an obsolete state.
    put(tx, "machine", &2u32)?;
    Ok(())
}

pub(super) fn restore(db: &Connection) -> anyhow::Result<Machine> {
    let base: Option<Vec<u8>> = db
        .query_row("SELECT image FROM sm_checkpoint WHERE id=1", [], |r| {
            r.get(0)
        })
        .optional()?;
    let mut machine: Machine = base.map(|b| decode(&b)).transpose()?.unwrap_or_default();
    let mut stmt = db.prepare("SELECT idx,entry FROM sm_applied ORDER BY idx")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let idx: u64 = row.get(0)?;
        let e: Entry<Types> = decode(&row.get::<_, Vec<u8>>(1)?)?;
        anyhow::ensure!(
            e.log_id.index == idx
                && machine
                    .applied
                    .is_none_or(|id| id.index.checked_add(1) == Some(idx)),
            "inconsistent applied-entry journal at index {idx}"
        );
        let _ = advance(&mut machine, &e);
    }
    Ok(machine)
}

pub(super) fn advance(m: &mut Machine, e: &Entry<Types>) -> Outcome {
    let result = match &e.payload {
        EntryPayload::Blank => Ok(()),
        EntryPayload::Membership(membership) => {
            m.membership = StoredMembership::new(Some(e.log_id), membership.clone());
            Ok(())
        }
        EntryPayload::Normal(p) => match m.results.entry((p.caller, p.request_id)) {
            std::collections::btree_map::Entry::Occupied(existing) => {
                let (command, outcome) = existing.get();
                if command == &p.command {
                    *outcome
                } else {
                    Err(ApplyError::Conflict)
                }
            }
            std::collections::btree_map::Entry::Vacant(slot) => {
                let outcome = m.pool.apply_at(p.caller, p.command.clone(), p.epoch);
                slot.insert((p.command.clone(), outcome));
                outcome
            }
        },
    };
    m.applied = Some(e.log_id);
    result
}

pub(super) fn apply(
    state: &mut Database,
    entries: &[Entry<Types>],
) -> anyhow::Result<Vec<Outcome>> {
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    let mut latest = state.machine.applied;
    let prior = entries.partition_point(|e| latest.is_some_and(|id| e.log_id.index <= id.index));
    let (replayed, pending) = entries.split_at(prior);
    let mut outcomes = Vec::with_capacity(entries.len());
    for e in replayed {
        let outcome = match &e.payload {
            EntryPayload::Normal(p) => {
                let (command, outcome) = state
                    .machine
                    .results
                    .get(&(p.caller, p.request_id))
                    .ok_or_else(|| anyhow::anyhow!("unknown proposal below last applied index"))?;
                if command == &p.command {
                    *outcome
                } else {
                    Err(ApplyError::Conflict)
                }
            }
            _ => Ok(()),
        };
        outcomes.push(outcome);
    }
    let tx = state.db.transaction()?;
    {
        let mut append = tx.prepare_cached("INSERT INTO sm_applied(idx,entry) VALUES(?,?)")?;
        for e in pending {
            anyhow::ensure!(
                latest.is_none_or(|id| id.index.checked_add(1) == Some(e.log_id.index)),
                "non-contiguous applied index {}",
                e.log_id.index
            );
            append.execute(params![e.log_id.index, encode(e)?])?;
            latest = Some(e.log_id);
        }
    }
    tx.commit()?;
    // Publish only after the entire batch is durable, under the same mutex
    // used by readers. Restart after COMMIT replays these same pure transitions.
    outcomes.extend(pending.iter().map(|e| advance(&mut state.machine, e)));
    Ok(outcomes)
}

pub(super) fn checkpoint(
    tx: &Transaction<'_>,
    meta: &SnapshotMeta<Uuid, BasicNode>,
    image: &[u8],
) -> anyhow::Result<()> {
    tx.execute(
        "INSERT INTO sm_checkpoint(id,meta,image) VALUES(1,?,?) ON CONFLICT(id)
        DO UPDATE SET meta=excluded.meta,image=excluded.image",
        params![encode(meta)?, image],
    )?;
    if let Some(last) = meta.last_log_id {
        tx.execute("DELETE FROM sm_applied WHERE idx<=?", [last.index])?;
    }
    tx.execute(
        "DELETE FROM metadata WHERE key IN ('snapshot','pool_revision')",
        [],
    )?;
    Ok(())
}
pub(super) fn build(store: &Store) -> anyhow::Result<Snapshot<Types>> {
    // The expensive CBOR pass runs outside the store lock. The journal tail
    // can advance while this worker holds its consistent point-in-time copy.
    let machine = store.0.lock().unwrap().machine.clone();
    let meta = SnapshotMeta {
        last_log_id: machine.applied,
        last_membership: machine.membership.clone(),
        snapshot_id: Uuid::new_v4().to_string(),
    };
    let bytes = encode(&machine)?;
    drop(machine);
    let mut state = store.0.lock().unwrap();
    let saved: Option<Vec<u8>> = state
        .db
        .query_row(
            "SELECT meta FROM sm_checkpoint WHERE id=1 AND meta IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(saved) = saved {
        let current: SnapshotMeta<Uuid, BasicNode> = decode(&saved)?;
        if current.last_log_id.map(|id| id.index) >= meta.last_log_id.map(|id| id.index) {
            return snapshot(&state.db)?.ok_or_else(|| anyhow::anyhow!("missing checkpoint image"));
        }
    }
    let tx = state.db.transaction()?;
    checkpoint(&tx, &meta, &bytes)?;
    tx.commit()?;
    Ok(Snapshot {
        meta,
        snapshot: Box::new(Cursor::new(bytes)),
    })
}
pub(super) fn snapshot(db: &Connection) -> anyhow::Result<Option<Snapshot<Types>>> {
    type Meta = SnapshotMeta<Uuid, BasicNode>;
    let saved: Option<(Vec<u8>, Vec<u8>)> = db
        .query_row(
            "SELECT meta,image FROM sm_checkpoint WHERE id=1 AND meta IS NOT NULL",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let pair = match saved {
        Some((m, b)) => Some((decode::<Meta>(&m)?, b)),
        None => get::<(Meta, Vec<u8>)>(db, "snapshot")?,
    };
    Ok(pair.map(|(meta, b)| Snapshot {
        meta,
        snapshot: Box::new(Cursor::new(b)),
    }))
}
