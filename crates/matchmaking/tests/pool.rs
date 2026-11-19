use halcyon_matchmaking::*;
use std::collections::BTreeMap;
use uuid::Uuid;

fn id(i: u128) -> Uuid {
    Uuid::from_u128(i)
}
fn ticket(i: u128, home: Uuid, players: &[u128]) -> Ticket {
    Ticket {
        talent_levels: Vec::new(),
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

#[test]
fn working_set_keeps_only_active_and_requested_terminal_entries() {
    let mut pool = Pool::default();
    let host = id(50);
    let room = id(60);
    let old = ticket(1, id(10), &[101, 102, 103, 104]);
    for command in [
        Command::Enqueue(old.clone()),
        Command::Allocate {
            id: room,
            host,
            tickets: vec![key(&old)],
        },
        Command::Accept {
            id: room,
            ticket: key(&old),
        },
    ] {
        pool.apply(old.home, command).unwrap();
    }
    pool.apply(
        host,
        Command::HostReady {
            id: room,
            endpoint: Endpoint {
                host: "localhost".into(),
                port: 8443,
            },
        },
    )
    .unwrap();
    pool.apply(
        host,
        Command::Finish {
            id: room,
            digest: "finished".into(),
        },
    )
    .unwrap();

    let waiting = ticket(2, id(10), &[201, 202, 203, 204]);
    pool.apply(waiting.home, Command::Enqueue(waiting.clone()))
        .unwrap();
    let aborted = id(61);
    pool.apply(
        waiting.home,
        Command::Allocate {
            id: aborted,
            host,
            tickets: vec![key(&waiting)],
        },
    )
    .unwrap();
    pool.apply(waiting.home, Command::Abort { id: aborted })
        .unwrap();

    let work = pool.working_set([old.id], [aborted]);
    assert_eq!(work.allocations.len(), 2);
    assert_eq!(work.finished(key(&old)).unwrap().id, room);
    assert_eq!(work.allocations[&aborted].phase, Phase::Aborted);
    assert_eq!(work.tickets[&waiting.id].phase, TicketPhase::Queued);
    let only_active = pool.working_set([], []);
    assert_eq!(only_active.tickets.len(), 1);
    assert!(only_active.allocations.is_empty());
    assert_eq!(pool.allocations().len(), 2);

    let newer = Ticket {
        generation: 2,
        ..old
    };
    pool.apply(newer.home, Command::Enqueue(newer.clone()))
        .unwrap();
    pool.apply(newer.home, Command::Cancel(key(&newer)))
        .unwrap();
    let work = pool.working_set([newer.id], []);
    assert!(work.finished(key(&newer)).is_none());
    assert!(work.allocations.is_empty());
}

#[test]
fn committed_offer_deadline_is_fixed_and_acceptance_ordered() {
    let mut pool = Pool::default();
    let a = ticket(1, id(10), &[101, 102]);
    let b = ticket(2, id(20), &[103, 104]);
    for t in [&a, &b] {
        pool.apply(t.home, Command::Enqueue(t.clone())).unwrap();
    }
    let room = id(60);
    let host = id(50);
    let allocate = Command::Allocate {
        id: room,
        host,
        tickets: vec![key(&a), key(&b)],
    };
    let accept_a = Command::Accept {
        id: room,
        ticket: key(&a),
    };
    let accept_b = Command::Accept {
        id: room,
        ticket: key(&b),
    };
    let ready = Command::HostReady {
        id: room,
        endpoint: Endpoint {
            host: "test.example".into(),
            port: 7,
        },
    };
    let expire = Command::Expire { id: room };
    pool.apply_at(a.home, allocate.clone(), Some(100)).unwrap();
    let deadline = 100 + OFFER_WINDOW_SECS;
    assert_eq!(pool.allocations()[&room].expires_at, Some(deadline));
    pool.apply_at(b.home, allocate, Some(110)).unwrap();
    assert_eq!(pool.allocations()[&room].expires_at, Some(deadline));
    let reject = Command::Reject {
        id: room,
        ticket: key(&a),
    };
    let mut early_rejection = pool.clone();
    early_rejection
        .apply_at(a.home, reject.clone(), Some(deadline - 1))
        .unwrap();
    assert_eq!(early_rejection.allocations()[&room].phase, Phase::Aborted);
    let mut expired_offer = pool.clone();
    for at in [deadline, deadline + 1] {
        assert_eq!(
            expired_offer.apply_at(a.home, reject.clone(), Some(at)),
            Err(Error::Conflict)
        );
        assert_eq!(expired_offer.allocations()[&room].phase, Phase::Preparing);
    }
    let mut late = pool.clone();
    pool.apply_at(a.home, accept_a.clone(), Some(deadline - 1))
        .unwrap();
    pool.apply_at(b.home, accept_b.clone(), Some(deadline - 1))
        .unwrap();
    pool.apply_at(host, ready.clone(), Some(deadline - 1))
        .unwrap();
    pool.apply_at(host, expire.clone(), Some(deadline)).unwrap();
    assert_eq!(
        pool.apply_at(a.home, reject, Some(deadline - 1)),
        Err(Error::Conflict)
    );
    assert_eq!(pool.allocations()[&room].phase, Phase::Ready);

    late.apply_at(a.home, accept_a.clone(), Some(deadline - 1))
        .unwrap();
    late.apply_at(host, expire.clone(), Some(deadline - 1))
        .unwrap();
    assert_eq!(late.allocations()[&room].phase, Phase::Preparing);
    assert_eq!(
        late.apply_at(b.home, accept_b.clone(), Some(deadline)),
        Err(Error::Conflict)
    );
    assert_eq!(
        late.apply_at(host, ready, Some(deadline - 1)),
        Err(Error::Conflict)
    );
    late.apply_at(a.home, accept_a, Some(deadline)).unwrap(); // replay of accepted choice
    late.apply_at(host, expire.clone(), Some(deadline)).unwrap();
    late.apply_at(host, expire, Some(deadline + 1)).unwrap();
    assert_eq!(late.allocations()[&room].phase, Phase::Aborted);
    assert_eq!(
        late.apply_at(b.home, accept_b, Some(deadline + 1)),
        Err(Error::Conflict)
    );
    assert_eq!(late.tickets()[&a.id].phase, TicketPhase::Queued);
    assert_eq!(late.tickets()[&b.id].phase, TicketPhase::Queued);
}

#[test]
fn a_legacy_preparation_gets_one_persisted_grace_window() {
    let mut pool = Pool::default();
    let t = ticket(1, id(10), &[101, 102, 103, 104]);
    let room = id(60);
    let host = id(50);
    pool.apply(t.home, Command::Enqueue(t.clone())).unwrap();
    pool.apply(
        t.home,
        Command::Allocate {
            id: room,
            host,
            tickets: vec![key(&t)],
        },
    )
    .unwrap();
    assert_eq!(pool.allocations()[&room].expires_at, None);
    let expire = Command::Expire { id: room };
    pool.apply_at(host, expire.clone(), Some(500)).unwrap();
    let deadline = 500 + OFFER_WINDOW_SECS;
    pool.apply_at(t.home, expire.clone(), Some(deadline - 1))
        .unwrap();
    assert_eq!(pool.allocations()[&room].expires_at, Some(deadline));
    assert_eq!(
        pool.apply(
            t.home,
            Command::Accept {
                id: room,
                ticket: key(&t)
            }
        ),
        Err(Error::Invalid)
    );
    pool.apply_at(host, expire, Some(deadline)).unwrap();
    assert_eq!(pool.allocations()[&room].phase, Phase::Aborted);
}

#[test]
fn rejecting_only_affects_the_offered_room_and_the_owners_ticket() {
    let mut p = Pool::default();
    let a = ticket(1, id(10), &[101, 102]);
    let b = ticket(2, id(20), &[103, 104]);
    for t in [&a, &b] {
        p.apply(t.home, Command::Enqueue(t.clone())).unwrap();
    }
    let allocate = |room| Command::Allocate {
        id: room,
        host: id(50),
        tickets: vec![key(&a), key(&b)],
    };
    let first = id(60);
    let next = id(61);
    p.apply(a.home, allocate(first)).unwrap();
    p.apply(a.home, Command::Abort { id: first }).unwrap();
    p.apply(a.home, allocate(next)).unwrap();
    let reject = |room| Command::Reject {
        id: room,
        ticket: key(&a),
    };
    assert!(p.apply(a.home, reject(first)).is_err());
    assert!(p.apply(b.home, reject(next)).is_err());
    assert_eq!(p.allocations()[&next].phase, Phase::Preparing);
    p.apply(a.home, reject(next)).unwrap();
    assert_eq!(p.allocations()[&next].phase, Phase::Aborted);
    assert_eq!(p.tickets()[&a.id].phase, TicketPhase::Closed);
    assert_eq!(p.tickets()[&b.id].phase, TicketPhase::Queued);
}

#[test]
fn numeric_talent_grants_are_scoped_to_the_committed_home_and_roster() {
    let mut pool = Pool::default();
    let home = Uuid::from_u128(1);
    let mut payload = ticket(60, home, &[61]);
    let old_wire = serde_json::to_value(&payload).unwrap();
    assert!(old_wire.get("talent_levels").is_none());
    assert_eq!(serde_json::from_value::<Ticket>(old_wire).unwrap(), payload);
    payload.queue.name = "casual_aral".into();
    payload.queue.rules_digest = "aral-talent-levels:test".into();
    payload.talent_levels = vec![[(273, std::num::NonZeroU8::new(3).unwrap())].into()];
    assert!(
        pool.apply(Uuid::from_u128(2), Command::Enqueue(payload.clone()))
            .is_err()
    );
    let mut invalid = payload.clone();
    invalid.talent_levels.push(Default::default());
    assert!(pool.apply(home, Command::Enqueue(invalid)).is_err());
    assert!(pool.apply(home, Command::Enqueue(payload.clone())).is_ok());
    payload.talent_levels[0].insert(273, std::num::NonZeroU8::new(4).unwrap());
    assert!(pool.apply(home, Command::Enqueue(payload)).is_err());
}
