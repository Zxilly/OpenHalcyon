//! Durable state is a full checkpoint plus only the entries applied after it.
//! The Raft log may be purged independently of this journal.
use super::*;
use schema::{sm_applied, sm_checkpoint};

pub(super) fn initialize(tx: &mut SqliteConnection) -> anyhow::Result<()> {
    match get::<u32>(tx, "sm_version")? {
        Some(2) => return Ok(()),
        Some(_) => anyhow::bail!("unknown federation state machine storage version"),
        None => {}
    }
    // Previous versions stored the most recently applied state independently of
    // the last published snapshot. Keep the older snapshot available until a
    // new snapshot at least as recent as the migration checkpoint is published.
    let image = metadata::table
        .find("machine")
        .select(metadata::data)
        .first::<Vec<u8>>(tx)
        .optional()?;
    if let Some(image) = image {
        diesel::insert_into(sm_checkpoint::table)
            .values((sm_checkpoint::id.eq(1), sm_checkpoint::image.eq(image)))
            .execute(tx)?;
    }
    put(tx, "sm_version", &2u32)?;
    // An older binary would otherwise silently start from an obsolete state.
    put(tx, "machine", &2u32)?;
    Ok(())
}

pub(super) fn restore(db: &mut SqliteConnection) -> anyhow::Result<Machine> {
    let base = sm_checkpoint::table
        .find(1)
        .select(sm_checkpoint::image)
        .first::<Vec<u8>>(db)
        .optional()?;
    let mut machine: Machine = base.map(|b| decode(&b)).transpose()?.unwrap_or_default();
    let rows = sm_applied::table
        .order(sm_applied::idx)
        .select((sm_applied::idx, sm_applied::entry))
        .load_iter::<(i64, Vec<u8>), diesel::connection::DefaultLoadingMode>(db)?;
    for row in rows {
        let (index, bytes) = row?;
        let idx = u64::try_from(index)?;
        let e: Entry<Types> = decode(&bytes)?;
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
    state.db.transaction::<_, anyhow::Error, _>(|tx| {
        for e in pending {
            anyhow::ensure!(
                latest.is_none_or(|id| id.index.checked_add(1) == Some(e.log_id.index)),
                "non-contiguous applied index {}",
                e.log_id.index
            );
            diesel::insert_into(sm_applied::table)
                .values((
                    sm_applied::idx.eq(i64::try_from(e.log_id.index)?),
                    sm_applied::entry.eq(encode(e)?),
                ))
                .execute(tx)?;
            latest = Some(e.log_id);
        }
        Ok(())
    })?;
    // Publish only after the entire batch is durable, under the same mutex
    // used by readers. Restart after COMMIT replays these same pure transitions.
    outcomes.extend(pending.iter().map(|e| advance(&mut state.machine, e)));
    Ok(outcomes)
}

pub(super) fn checkpoint(
    tx: &mut SqliteConnection,
    meta: &SnapshotMeta<Uuid, BasicNode>,
    image: &[u8],
) -> anyhow::Result<()> {
    let metadata_bytes = encode(meta)?;
    diesel::insert_into(sm_checkpoint::table)
        .values((
            sm_checkpoint::id.eq(1),
            sm_checkpoint::meta.eq(&metadata_bytes),
            sm_checkpoint::image.eq(image),
        ))
        .on_conflict(sm_checkpoint::id)
        .do_update()
        .set((
            sm_checkpoint::meta.eq(diesel::upsert::excluded(sm_checkpoint::meta)),
            sm_checkpoint::image.eq(diesel::upsert::excluded(sm_checkpoint::image)),
        ))
        .execute(tx)?;
    if let Some(last) = meta.last_log_id {
        diesel::delete(sm_applied::table.filter(sm_applied::idx.le(i64::try_from(last.index)?)))
            .execute(tx)?;
    }
    diesel::delete(metadata::table.filter(metadata::key.eq_any(["snapshot", "pool_revision"])))
        .execute(tx)?;
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
    let saved = sm_checkpoint::table
        .find(1)
        .select(sm_checkpoint::meta)
        .first::<Option<Vec<u8>>>(&mut state.db)
        .optional()?
        .flatten();
    if let Some(saved) = saved {
        let current: SnapshotMeta<Uuid, BasicNode> = decode(&saved)?;
        if current.last_log_id.map(|id| id.index) >= meta.last_log_id.map(|id| id.index) {
            return snapshot(&mut state.db)?
                .ok_or_else(|| anyhow::anyhow!("missing checkpoint image"));
        }
    }
    state.db.transaction(|tx| checkpoint(tx, &meta, &bytes))?;
    Ok(Snapshot {
        meta,
        snapshot: Box::new(Cursor::new(bytes)),
    })
}
pub(super) fn snapshot(db: &mut SqliteConnection) -> anyhow::Result<Option<Snapshot<Types>>> {
    type Meta = SnapshotMeta<Uuid, BasicNode>;
    let saved = sm_checkpoint::table
        .find(1)
        .filter(sm_checkpoint::meta.is_not_null())
        .select((sm_checkpoint::meta.assume_not_null(), sm_checkpoint::image))
        .first::<(Vec<u8>, Vec<u8>)>(db)
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
