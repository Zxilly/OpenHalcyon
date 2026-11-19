use halcyon_matchmaking::*;
use std::collections::BTreeMap;
use uuid::Uuid;

fn id(i: u128) -> Uuid {
    Uuid::from_u128(i)
}
fn ticket(i: u128, home: Uuid, players: &[u128]) -> Ticket {
    Ticket {
        id: id(i),
        generation: 1,
        home,
        players: players.iter().copied().map(id).collect(),
        queue: Queue {
            name: "casual".into(),
            build: 147219,
            rules_digest: "test".into(),
            seats: 4,
        },
        latency_ms: BTreeMap::from([(id(50), 75)]),
        max_latency_ms: 100,
    }
}
fn key(t: &Ticket) -> TicketRef {
    TicketRef {
        id: t.id,
        generation: t.generation,
    }
}

#[test]
fn two_homes_one_host_order_independent_readiness() {
    let mut p = Pool::default();
    let a = ticket(1, id(10), &[101, 102]);
    let b = ticket(2, id(20), &[103, 104]);
    for t in [&a, &b] {
        p.apply(t.home, Command::Enqueue(t.clone())).unwrap();
    }
    let host = id(50);
    let room = id(60);
    let proposal = plan(
        &p,
        a.id,
        &[HostOffer {
            id: host,
            queue: a.queue.clone(),
            free_seats: 4,
            total_seats: 4,
        }],
    )
    .unwrap();
    p.apply(
        a.home,
        Command::Allocate {
            id: room,
            host,
            tickets: proposal.tickets.clone(),
        },
    )
    .unwrap();
    assert_eq!(
        p.apply(
            b.home,
            Command::Allocate {
                id: id(61),
                host,
                tickets: proposal.tickets
            }
        ),
        Err(Error::Conflict)
    );
    p.apply(
        host,
        Command::HostReady {
            id: room,
            endpoint: Endpoint {
                host: "127.0.0.1".into(),
                port: 7000,
            },
        },
    )
    .unwrap();
    for t in [&b, &a] {
        p.apply(
            t.home,
            Command::Accept {
                id: room,
                ticket: key(t),
            },
        )
        .unwrap();
    }
    assert_eq!(p.allocations()[&room].phase, Phase::Ready);
    assert_eq!(
        p.apply(a.home, Command::Cancel(key(&a))),
        Err(Error::Conflict)
    );
    let result = serde_json::json!({"winner":1,"reason":"completed"});
    let hash = result_digest(&result).unwrap();
    assert_eq!(
        p.apply(
            a.home,
            Command::Settle {
                id: room,
                digest: hash.clone(),
                result: result.clone()
            }
        ),
        Err(Error::Forbidden)
    );
    assert_eq!(
        p.apply(
            host,
            Command::Settle {
                id: room,
                digest: "invalid".into(),
                result: result.clone()
            }
        ),
        Err(Error::Invalid)
    );
    for _ in 0..2 {
        p.apply(
            host,
            Command::Settle {
                id: room,
                digest: hash.clone(),
                result: result.clone(),
            },
        )
        .unwrap();
        p.apply(
            host,
            Command::Finish {
                id: room,
                digest: hash.clone(),
            },
        )
        .unwrap();
    }
    assert_eq!(p.allocations()[&room].result.as_ref(), Some(&result));
    assert_eq!(p.tickets()[&a.id].phase, TicketPhase::Closed);
    assert_eq!(
        p.apply(
            host,
            Command::Finish {
                id: room,
                digest: "other".into()
            }
        ),
        Err(Error::Conflict)
    );
}

#[test]
fn cancellation_releases_party_and_rejects_old_generation() {
    let mut p = Pool::default();
    let a = ticket(1, id(11), &[1, 2]);
    let b = ticket(2, id(22), &[3, 4]);
    for t in [&a, &b] {
        p.apply(t.home, Command::Enqueue(t.clone())).unwrap();
    }
    p.apply(
        a.home,
        Command::Allocate {
            id: id(4),
            host: id(50),
            tickets: vec![key(&a), key(&b)],
        },
    )
    .unwrap();
    assert_eq!(
        p.apply(b.home, Command::Cancel(key(&a))),
        Err(Error::Forbidden)
    );
    p.apply(a.home, Command::Cancel(key(&a))).unwrap();
    assert_eq!(p.tickets()[&b.id].phase, TicketPhase::Queued);
    let mut retry = a.clone();
    retry.generation += 1;
    p.apply(a.home, Command::Enqueue(retry)).unwrap();
    assert_eq!(
        p.apply(a.home, Command::Cancel(key(&a))),
        Err(Error::Conflict)
    );
}
