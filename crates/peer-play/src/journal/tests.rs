use super::Journal;
use diesel::{connection::SimpleConnection, prelude::*};
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
        .batch_execute("update outgoing_chat set expires=0")
        .unwrap();
    assert!(journal.due_chat().unwrap().is_empty());
    assert_eq!(
        super::schema::outgoing_chat::table
            .count()
            .get_result::<i64>(&mut *journal.0.lock().unwrap())
            .unwrap(),
        0
    );
}

fn ticket(id: Uuid, home: Uuid, generation: u64) -> halcyon_matchmaking::Ticket {
    halcyon_matchmaking::Ticket {
        id,
        generation,
        home,
        players: vec![home],
        talent_levels: Vec::new(),
        queue: halcyon_matchmaking::Queue {
            name: "fixture".into(),
            build: 147219,
            rules_digest: "fixture".into(),
            seats: 1,
        },
        latency_ms: Default::default(),
        max_latency_ms: 100,
    }
}

#[test]
fn adopts_existing_rusqlite_database_and_replays_pending_work_only() {
    use rusqlite::{Connection, params};
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("old.sqlite");
    let pool = Uuid::new_v4();
    let node = Uuid::new_v4();
    let running = Uuid::new_v4();
    let finished = Uuid::new_v4();
    let acknowledged = Uuid::new_v4();
    let chat1 = Uuid::new_v4();
    let chat2 = Uuid::new_v4();
    let id = Uuid::new_v4();
    let old = Connection::open(&db).unwrap();
    old.execute_batch("PRAGMA journal_mode=WAL;
        CREATE TABLE peer_identity(singleton INTEGER PRIMARY KEY CHECK(singleton=1),pool TEXT NOT NULL,node TEXT NOT NULL,sequence INTEGER NOT NULL);
        CREATE TABLE hosted(id TEXT PRIMARY KEY,address TEXT NOT NULL,result TEXT,delivered INTEGER NOT NULL DEFAULT 0);
        CREATE TABLE outgoing_chat(id TEXT NOT NULL,home TEXT NOT NULL,body TEXT NOT NULL,expires INTEGER NOT NULL DEFAULT(unixepoch()+120),next_attempt INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(id,home));
        CREATE TABLE enqueue_payloads(id TEXT PRIMARY KEY,payload TEXT NOT NULL);").unwrap();
    old.execute(
        "INSERT INTO peer_identity VALUES(1,?1,?2,41)",
        params![pool.to_string(), node.to_string()],
    )
    .unwrap();
    old.execute(
        "INSERT INTO hosted VALUES(?1,'old',NULL,0),(?2,'old','true',0),(?3,'old','false',1)",
        params![
            running.to_string(),
            finished.to_string(),
            acknowledged.to_string()
        ],
    )
    .unwrap();
    for chat in [chat2, chat1] {
        let message = MatchChat {
            id: chat,
            room: running,
            sender: node,
            text: "fixture".into(),
        };
        old.execute(
            "INSERT INTO outgoing_chat(id,home,body) VALUES(?,?,?)",
            params![
                chat.to_string(),
                node.to_string(),
                serde_json::to_string(&message).unwrap()
            ],
        )
        .unwrap();
    }
    let frozen = ticket(id, node, 3);
    old.execute(
        "INSERT INTO enqueue_payloads VALUES(?,?)",
        params![id.to_string(), serde_json::to_string(&frozen).unwrap()],
    )
    .unwrap();
    drop(old);
    assert!(Journal::open(&db, pool, Uuid::new_v4()).is_err());
    let journal = Journal::open(&db, pool, node).unwrap();
    assert_eq!(journal.sequence().unwrap(), 42);
    assert_eq!(journal.ticket(id).unwrap(), Some(frozen.clone()));
    assert_eq!(journal.freeze_ticket(ticket(id, node, 4)).unwrap(), frozen);
    assert_eq!(journal.unsettled_rooms().unwrap(), [running]);
    assert_eq!(journal.outbox().unwrap(), [(finished, true.into())]);
    journal.prepared(finished, "ignored").unwrap();
    journal.discard_preparation(finished).unwrap();
    assert!(journal.finish(finished, &false.into()).is_err());
    assert!(journal.finish(finished, &true.into()).unwrap());
    assert_eq!(
        journal
            .due_chat()
            .unwrap()
            .iter()
            .map(|(_, chat)| chat.id)
            .collect::<Vec<_>>(),
        [chat2, chat1]
    );
    journal.delivered(finished).unwrap();
    journal.chat_attempt(chat2, node, true).unwrap();
    journal.adopt_legacy_ticket(&ticket(id, node, 7)).unwrap();
    drop(journal);
    let reopened = Journal::open(&db, pool, node).unwrap();
    assert!(reopened.outbox().unwrap().is_empty());
    assert_eq!(reopened.due_chat().unwrap()[0].1.id, chat1);
    assert_eq!(reopened.ticket(id).unwrap().unwrap().generation, 7);
    let legacy = Connection::open(&db).unwrap();
    assert_eq!(
        legacy
            .query_row(
                "SELECT address FROM hosted WHERE id=?",
                [finished.to_string()],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "old"
    );
}

#[test]
fn independent_connections_freeze_once_and_allocate_distinct_sequences() {
    use std::sync::{Arc, Barrier};
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("shared.sqlite");
    let pool = Uuid::new_v4();
    let node = Uuid::new_v4();
    let id = Uuid::new_v4();
    let room = Uuid::new_v4();
    let original = Journal::open(&db, pool, node).unwrap();
    original.prepared(room, "fixture").unwrap();
    let barrier = Arc::new(Barrier::new(4));
    let join: Vec<_> = (0..4)
        .map(|n| {
            let gate = barrier.clone();
            let db = db.clone();
            std::thread::spawn(move || {
                let journal = Journal::open(&db, pool, node).unwrap();
                gate.wait();
                let frozen = journal.freeze_ticket(ticket(id, node, n)).unwrap();
                let numbers = (0..12)
                    .map(|_| journal.sequence().unwrap())
                    .collect::<Vec<_>>();
                let result = journal.finish(room, &n.into()).is_ok();
                (frozen, numbers, result)
            })
        })
        .collect();
    let outcomes: Vec<_> = join.into_iter().map(|w| w.join().unwrap()).collect();
    assert!(outcomes.iter().all(|r| r.0 == outcomes[0].0));
    let mut numbers: Vec<_> = outcomes.iter().flat_map(|r| r.1.iter().copied()).collect();
    numbers.sort_unstable();
    assert_eq!(numbers, (1..=48).collect::<Vec<_>>());
    assert_eq!(outcomes.iter().filter(|r| r.2).count(), 1);
    assert_eq!(original.outbox().unwrap().len(), 1);
}

#[test]
fn enqueue_rolls_back_other_recipients_when_a_delivery_already_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let pool = Uuid::new_v4();
    let node = Uuid::new_v4();
    let journal = Journal::open(&tmp.path().join("queue.sqlite"), pool, node).unwrap();
    let before = Uuid::from_u128(1);
    let existing = Uuid::from_u128(2);
    let message = MatchChat {
        id: Uuid::new_v4(),
        room: Uuid::new_v4(),
        sender: node,
        text: "fixture".into(),
    };
    journal.queue_chat(&message, [existing].into()).unwrap();
    assert!(
        journal
            .queue_chat(&message, [before, existing].into())
            .is_err()
    );
    let due = journal.due_chat().unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].0, existing);
}
