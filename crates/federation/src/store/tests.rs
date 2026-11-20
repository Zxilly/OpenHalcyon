use super::*;
use halcyon_matchmaking::{Endpoint, Phase, Queue, Ticket, TicketRef, result_digest};
use std::time::Instant;

#[derive(serde::Serialize)]
struct StoredResultSample {
    winner: u8,
    players: [ScoreSample; 1],
    duration: u32,
}
#[derive(serde::Serialize)]
struct ScoreSample {
    kills: u32,
    assists: u32,
    deaths: u32,
}

const HOME: Uuid = Uuid::from_u128(1);
const POOL: Uuid = Uuid::from_u128(2);
fn entry(index: u64, request: Uuid, command: Command) -> Entry<Types> {
    Entry {
        log_id: LogId::new(openraft::CommittedLeaderId::new(1, HOME), index),
        payload: EntryPayload::Normal(Proposal {
            caller: HOME,
            request_id: request,
            command,
            epoch: None,
        }),
    }
}
fn fixture(rooms: u64) -> Machine {
    let mut m = Machine::default();
    let mut index = 0;
    for n in 0..rooms {
        let id = Uuid::from_u128(100_000 + u128::from(n));
        let ticket = Uuid::from_u128(200_000 + u128::from(n));
        let t = TicketRef {
            id: ticket,
            generation: 1,
        };
        let result = serde_json::to_value(StoredResultSample {
            winner: 0,
            players: [ScoreSample {
                kills: 3,
                assists: 5,
                deaths: 1,
            }],
            duration: 1721,
        })
        .unwrap();
        let digest = result_digest(&result).unwrap();
        let commands = [
            Command::Enqueue(Ticket {
                talent_levels: Vec::new(),
                id: ticket,
                generation: 1,
                home: HOME,
                players: vec![Uuid::from_u128(300_000 + u128::from(n))],
                queue: Queue {
                    name: "sim".into(),
                    build: 147219,
                    rules_digest: "test-rules".into(),
                    seats: 1,
                },
                latency_ms: BTreeMap::from([(HOME, 1)]),
                max_latency_ms: 50,
            }),
            Command::Allocate {
                id,
                host: HOME,
                tickets: vec![t],
            },
            Command::Accept { id, ticket: t },
            Command::HostReady {
                id,
                endpoint: Endpoint {
                    host: "localhost".into(),
                    port: 1234,
                },
            },
            Command::Settle { id, digest, result },
        ];
        for cmd in commands {
            index += 1;
            let req = Uuid::from_u128(400_000 + u128::from(index));
            assert_eq!(state::advance(&mut m, &entry(index, req, cmd)), Ok(()));
        }
    }
    m
}
fn legacy_put(db: &rusqlite::Connection, key: &str, value: &impl Serialize) -> anyhow::Result<()> {
    db.execute(
        "INSERT OR REPLACE INTO metadata(key,data) VALUES(?,?)",
        rusqlite::params![key, encode(value)?],
    )?;
    Ok(())
}
fn legacy_file(path: &Path, m: &Machine) {
    let db = rusqlite::Connection::open(path).unwrap();
    db.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
        CREATE TABLE metadata(key TEXT PRIMARY KEY,data BLOB NOT NULL);
        CREATE TABLE log(idx INTEGER PRIMARY KEY,data BLOB NOT NULL);",
    )
    .unwrap();
    legacy_put(&db, "identity", &(POOL, HOME)).unwrap();
    legacy_put(&db, "machine", m).unwrap();
    legacy_put(&db, "pool_revision", &m.applied.unwrap().index).unwrap();
    let old = Machine::default();
    let snap = SnapshotMeta {
        last_log_id: None,
        last_membership: old.membership.clone(),
        snapshot_id: "legacy-snapshot".into(),
    };
    legacy_put(&db, "snapshot", &(snap, encode(&old).unwrap())).unwrap();
}
fn count_journal(store: &Store) -> usize {
    store
        .read(|db| {
            Ok(usize::try_from(
                schema::sm_applied::table.count().get_result::<i64>(db)?,
            )?)
        })
        .unwrap()
}

#[tokio::test]
async fn legacy_upgrade_journal_recovery_retries_and_snapshot_transfer() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("node.db");
    let old = fixture(3);
    legacy_file(&path, &old);
    let mut s = Store::open(&path, POOL, HOME).unwrap();
    assert_eq!(s.machine().unwrap().pool.allocations().len(), 3);
    assert_eq!(
        s.get_current_snapshot()
            .await
            .unwrap()
            .unwrap()
            .meta
            .snapshot_id,
        "legacy-snapshot"
    );
    assert!(s.pool_changes(Some(15)).unwrap().1.is_none());
    let (req, (cmd, _)) = old.results.iter().next().unwrap();
    let same = entry(16, req.1, cmd.clone());
    let conflict = entry(17, req.1, Command::Abort { id: HOME });
    let rejected = entry(18, Uuid::from_u128(99), Command::Abort { id: HOME });
    s.append_to_log([same.clone(), conflict.clone(), rejected.clone()])
        .await
        .unwrap();
    // Inject a SQLite failure half way through one batch. The live machine,
    // cursor and durable journal must all continue to describe the old state.
    s.write(|db|Ok(db.batch_execute("CREATE TRIGGER fail_apply BEFORE INSERT ON sm_applied WHEN NEW.idx=17 BEGIN SELECT RAISE(ABORT,'injected'); END;")?)).unwrap();
    assert!(
        s.apply_to_state_machine(&[same.clone(), conflict.clone()])
            .await
            .is_err()
    );
    assert_eq!(s.last_applied_state().await.unwrap().0, old.applied);
    assert_eq!(count_journal(&s), 0);
    s.write(|db| Ok(db.batch_execute("DROP TRIGGER fail_apply")?))
        .unwrap();
    let outcomes = vec![Ok(()), Err(ApplyError::Conflict), Err(ApplyError::Missing)];
    assert_eq!(
        s.apply_to_state_machine(&[same.clone(), conflict.clone(), rejected.clone()])
            .await
            .unwrap(),
        outcomes
    );
    s.purge_logs_upto(rejected.log_id).await.unwrap();
    drop(s);
    let mut s = Store::open(&path, POOL, HOME).unwrap();
    assert_eq!(count_journal(&s), 3);
    assert_eq!(
        s.apply_to_state_machine(&[same, conflict, rejected])
            .await
            .unwrap(),
        outcomes
    );
    assert_eq!(count_journal(&s), 3);
    let current = s.machine().unwrap();
    assert!(
        current
            .pool
            .allocations()
            .values()
            .all(|r| r.phase == Phase::Finished && r.result.is_some())
    );
    let snap = s.build_snapshot().await.unwrap();
    assert_eq!(count_journal(&s), 0);
    let bytes = snap.snapshot.into_inner();
    let wire: Machine = decode(&bytes).unwrap();
    let home_ticket = TicketRef {
        id: Uuid::from_u128(200_000),
        generation: 1,
    };
    assert!(wire.pool.working_set([], []).allocations.is_empty());
    let work = wire.pool.working_set([home_ticket.id], []);
    assert_eq!(work.allocations.len(), 1);
    assert_eq!(
        work.finished(home_ticket).unwrap().id,
        Uuid::from_u128(100_000)
    );
    assert_eq!(wire.results, current.results);
    let mut fresh = Store::open(&tmp.path().join("fresh.db"), POOL, Uuid::from_u128(33)).unwrap();
    fresh
        .install_snapshot(&snap.meta, Box::new(Cursor::new(bytes)))
        .await
        .unwrap();
    assert_eq!(fresh.machine().unwrap().results, current.results);
    drop(s);
    let mut s = Store::open(&path, POOL, HOME).unwrap();
    assert_eq!(s.machine().unwrap().results, current.results);
    assert_eq!(
        s.get_current_snapshot().await.unwrap().unwrap().meta,
        snap.meta
    );
    let repeat = entry(19, Uuid::from_u128(99), Command::Abort { id: HOME });
    assert_eq!(
        s.apply_to_state_machine(&[repeat]).await.unwrap(),
        [Err(ApplyError::Missing)]
    );
}

#[tokio::test]
async fn checkpoint_at_an_earlier_fence_retains_concurrently_applied_tail() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("node.db");
    let mut store = Store::open(&path, POOL, HOME).unwrap();
    let one = entry(1, Uuid::from_u128(50), Command::Abort { id: HOME });
    let two = entry(2, Uuid::from_u128(51), Command::Abort { id: HOME });
    store.apply_to_state_machine(&[one]).await.unwrap();
    let earlier = store.machine().unwrap();
    let meta = SnapshotMeta {
        last_log_id: earlier.applied,
        last_membership: earlier.membership.clone(),
        snapshot_id: "earlier".into(),
    };
    let raw = encode(&earlier).unwrap();
    store.apply_to_state_machine(&[two]).await.unwrap();
    {
        let mut state = store.0.lock().unwrap();
        state
            .db
            .transaction(|tx| super::state::checkpoint(tx, &meta, &raw))
            .unwrap();
    }
    assert_eq!(count_journal(&store), 1);
    drop(store);
    let mut store = Store::open(&path, POOL, HOME).unwrap();
    assert_eq!(store.machine().unwrap().results.len(), 2);
    assert!(
        store
            .install_snapshot(&meta, Box::new(Cursor::new(raw)))
            .await
            .is_err()
    );
    assert_eq!(store.machine().unwrap().applied.unwrap().index, 2);
}

#[tokio::test]
#[ignore = "release storage benchmark, creates disposable legacy and journal SQLite files"]
async fn store_write_amplification() {
    for rooms in [128, 1024] {
        let tmp = tempfile::tempdir().unwrap();
        let original = fixture(rooms);
        let initial = original.applied.unwrap().index;
        let sequence: Vec<_> = (1..=32)
            .map(|n| {
                entry(
                    initial + n,
                    Uuid::from_u128(900_000 + n as u128),
                    Command::Abort { id: HOME },
                )
            })
            .collect();
        for legacy in [true, false] {
            let path = tmp.path().join(if legacy { "old.db" } else { "new.db" });
            legacy_file(&path, &original);
            let (millis, wal_bytes, checkpoint_ms);
            if legacy {
                let mut c = rusqlite::Connection::open(&path).unwrap();
                c.execute_batch("PRAGMA synchronous=FULL;PRAGMA wal_autocheckpoint=0;PRAGMA wal_checkpoint(TRUNCATE)").unwrap();
                let start = Instant::now();
                for e in &sequence {
                    let tx = c.transaction().unwrap();
                    let mut m: Machine = decode(
                        &tx.query_row("SELECT data FROM metadata WHERE key='machine'", [], |r| {
                            r.get::<_, Vec<u8>>(0)
                        })
                        .unwrap(),
                    )
                    .unwrap();
                    assert_eq!(state::advance(&mut m, e), Err(ApplyError::Missing));
                    legacy_put(&tx, "machine", &m).unwrap();
                    legacy_put(&tx, "pool_revision", &e.log_id.index).unwrap();
                    tx.commit().unwrap();
                }
                millis = start.elapsed().as_secs_f64() * 1000.;
                wal_bytes = std::fs::metadata(tmp.path().join("old.db-wal"))
                    .unwrap()
                    .len();
                checkpoint_ms = None;
            } else {
                let mut store = Store::open(&path, POOL, HOME).unwrap();
                store
                    .read(|db| {
                        Ok(db.batch_execute(
                            "PRAGMA wal_autocheckpoint=0;PRAGMA wal_checkpoint(TRUNCATE)",
                        )?)
                    })
                    .unwrap();
                let start = Instant::now();
                for e in &sequence {
                    assert_eq!(
                        store
                            .apply_to_state_machine(std::slice::from_ref(e))
                            .await
                            .unwrap(),
                        [Err(ApplyError::Missing)]
                    );
                }
                millis = start.elapsed().as_secs_f64() * 1000.;
                wal_bytes = std::fs::metadata(tmp.path().join("new.db-wal"))
                    .unwrap()
                    .len();
                let start = Instant::now();
                store.build_snapshot().await.unwrap();
                checkpoint_ms = Some(start.elapsed().as_secs_f64() * 1000.);
            }
            println!(
                "rooms={rooms} previous_outcomes={initial} legacy={legacy} apply_32_ms={millis:.1} wal_bytes={wal_bytes} checkpoint_ms={checkpoint_ms:?}"
            );
        }
    }
}

#[tokio::test]
async fn a_preparing_deadline_and_replayed_acceptance_survive_storage_restart() {
    use halcyon_matchmaking::{OFFER_WINDOW_SECS, TicketPhase};

    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("offers.db");
    let id = Uuid::from_u128(80);
    let ticket = Ticket {
        talent_levels: Vec::new(),
        id: Uuid::from_u128(81),
        generation: 1,
        home: HOME,
        players: vec![Uuid::from_u128(82)],
        queue: Queue {
            name: "casual".into(),
            build: 147219,
            rules_digest: "test-rules".into(),
            seats: 1,
        },
        latency_ms: BTreeMap::from([(HOME, 1)]),
        max_latency_ms: 50,
    };
    let reference = TicketRef {
        id: ticket.id,
        generation: 1,
    };
    let timed = |index: u64, request: u128, epoch, command| {
        let mut entry = entry(index, Uuid::from_u128(request), command);
        if let EntryPayload::Normal(proposal) = &mut entry.payload {
            proposal.epoch = Some(epoch);
        }
        entry
    };
    let deadline = 100 + OFFER_WINDOW_SECS;
    let accept = Command::Accept {
        id,
        ticket: reference,
    };
    let mut store = Store::open(&path, POOL, HOME).unwrap();
    assert_eq!(
        store
            .apply_to_state_machine(&[
                entry(1, Uuid::from_u128(86), Command::Enqueue(ticket)),
                timed(
                    2,
                    87,
                    100,
                    Command::Allocate {
                        id,
                        host: HOME,
                        tickets: vec![reference]
                    }
                ),
                timed(3, 88, deadline - 1, accept.clone()),
            ])
            .await
            .unwrap(),
        [Ok(()); 3]
    );
    store.build_snapshot().await.unwrap();
    drop(store);

    let mut reopened = Store::open(&path, POOL, HOME).unwrap();
    let saved = reopened.machine().unwrap();
    assert_eq!(saved.pool.allocations()[&id].expires_at, Some(deadline));
    assert_eq!(saved.pool.allocations()[&id].phase, Phase::Preparing);
    assert_eq!(
        reopened
            .apply_to_state_machine(&[
                timed(4, 88, deadline, accept), // same request and outcome, no new deadline
                timed(
                    5,
                    89,
                    deadline,
                    Command::HostReady {
                        id,
                        endpoint: Endpoint {
                            host: "test.example".into(),
                            port: 7
                        },
                    }
                ),
                timed(6, 90, deadline, Command::Expire { id }),
            ])
            .await
            .unwrap(),
        [Ok(()), Err(ApplyError::Conflict), Ok(())]
    );
    let expired = reopened.machine().unwrap();
    assert_eq!(expired.pool.allocations()[&id].expires_at, Some(deadline));
    assert_eq!(expired.pool.allocations()[&id].phase, Phase::Aborted);
    assert_eq!(
        expired.pool.tickets()[&reference.id].phase,
        TicketPhase::Queued
    );
}

#[tokio::test]
async fn joint_voters_vote_commit_and_purge_survive_reopen_and_invalid_snapshot() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("raft.db");
    let remote = Uuid::from_u128(3);
    let nodes = BTreeMap::from([
        (HOME, BasicNode::new("local")),
        (remote, BasicNode::new("remote")),
    ]);
    let member = Entry::<Types> {
        log_id: LogId::new(openraft::CommittedLeaderId::new(4, HOME), 1),
        payload: EntryPayload::Membership(openraft::Membership::new(
            vec![[HOME].into(), [HOME, remote].into()],
            nodes,
        )),
    };
    let vote = Vote::new_committed(4, HOME);
    let mut store = Store::open(&path, POOL, HOME).unwrap();
    store.save_vote(&vote).await.unwrap();
    store.append_to_log([member.clone()]).await.unwrap();
    store
        .apply_to_state_machine(std::slice::from_ref(&member))
        .await
        .unwrap();
    store.save_committed(Some(member.log_id)).await.unwrap();
    let snap = store.build_snapshot().await.unwrap();
    store.purge_logs_upto(member.log_id).await.unwrap();
    let mut invalid = snap.meta.clone();
    invalid.last_membership = StoredMembership::default();
    assert!(
        store
            .install_snapshot(&invalid, snap.snapshot)
            .await
            .is_err()
    );
    drop(store);
    assert!(Store::open(&path, POOL, remote).is_err());
    let mut store = Store::open(&path, POOL, HOME).unwrap();
    assert_eq!(store.read_vote().await.unwrap(), Some(vote));
    assert_eq!(store.read_committed().await.unwrap(), Some(member.log_id));
    assert_eq!(store.membership(), snap.meta.last_membership);
    assert_eq!(
        store.get_log_state().await.unwrap().last_purged_log_id,
        Some(member.log_id)
    );
    assert!(store.try_get_log_entries(..).await.unwrap().is_empty());
    assert_eq!(
        store.get_current_snapshot().await.unwrap().unwrap().meta,
        snap.meta
    );
}
