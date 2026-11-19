//! Deterministic state transitions for peer matchmaking.
//!
//! Apply committed commands in log order. Host health and clocks are inputs to
//! the proposal builder, never to the replicated transition function.

mod proposal;
pub use proposal::{HostOffer, MatchPlan, plan};

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;
use uuid::Uuid;

pub const MAX_SEATS: usize = 10;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct Queue {
    pub name: String,
    pub build: u32,
    pub rules_digest: String,
    pub seats: u8,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Ticket {
    pub id: Uuid,
    pub generation: u64,
    pub home: Uuid,
    pub players: Vec<Uuid>,
    pub queue: Queue,
    /// Measured or policy-approved estimates from this party to prospective hosts.
    pub latency_ms: BTreeMap<Uuid, u32>,
    pub max_latency_ms: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TicketRef {
    pub id: Uuid,
    pub generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum TicketPhase {
    Queued,
    Assigned(Uuid),
    Closed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub ticket: Ticket,
    pub sequence: u64,
    pub phase: TicketPhase,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Phase {
    Preparing,
    Ready,
    Aborted,
    Finished,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Allocation {
    pub id: Uuid,
    pub host: Uuid,
    pub tickets: Vec<TicketRef>,
    pub phase: Phase,
    pub accepted: BTreeSet<Uuid>,
    pub endpoint: Option<Endpoint>,
    pub result_digest: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Command {
    Enqueue(Ticket),
    Cancel(TicketRef),
    Allocate {
        id: Uuid,
        host: Uuid,
        tickets: Vec<TicketRef>,
    },
    Accept {
        id: Uuid,
        ticket: TicketRef,
    },
    HostReady {
        id: Uuid,
        endpoint: Endpoint,
    },
    Abort {
        id: Uuid,
    },
    Finish {
        id: Uuid,
        digest: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Error)]
pub enum Error {
    #[error("invalid queue, roster or endpoint")]
    Invalid,
    #[error("ticket or allocation does not exist")]
    Missing,
    #[error("caller does not own this resource")]
    Forbidden,
    #[error("ticket generation or allocation state conflicts")]
    Conflict,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct Pool {
    tickets: BTreeMap<Uuid, Entry>,
    allocations: BTreeMap<Uuid, Allocation>,
    /// A wire UUID can appear at different homes, but only once per roster.
    owners: BTreeMap<(Uuid, Uuid), Uuid>,
    sequence: u64,
}

impl Pool {
    pub fn tickets(&self) -> &BTreeMap<Uuid, Entry> {
        &self.tickets
    }
    pub fn allocations(&self) -> &BTreeMap<Uuid, Allocation> {
        &self.allocations
    }

    /// The transport authenticates the caller as a member before submission.
    pub fn apply(&mut self, caller: Uuid, command: Command) -> Result<(), Error> {
        match command {
            Command::Enqueue(ticket) => self.enqueue(caller, ticket),
            Command::Cancel(ticket) => self.cancel(caller, ticket),
            Command::Allocate { id, host, tickets } => self.allocate(id, host, tickets),
            Command::Accept { id, ticket } => {
                let entry = self.owned(caller, ticket)?;
                if entry.phase != TicketPhase::Assigned(id) {
                    return Err(Error::Conflict);
                }
                let room = self.allocations.get_mut(&id).ok_or(Error::Missing)?;
                if room.accepted.contains(&ticket.id) {
                    return Ok(());
                }
                if room.phase != Phase::Preparing {
                    return Err(Error::Conflict);
                }
                room.accepted.insert(ticket.id);
                Self::activate(room);
                Ok(())
            }
            Command::HostReady { id, endpoint } => {
                let room = self.allocations.get_mut(&id).ok_or(Error::Missing)?;
                if caller != room.host {
                    return Err(Error::Forbidden);
                }
                if !matches!(room.phase, Phase::Preparing | Phase::Ready) {
                    return Err(Error::Conflict);
                }
                if let Some(existing) = &room.endpoint {
                    return if existing == &endpoint {
                        Ok(())
                    } else {
                        Err(Error::Conflict)
                    };
                }
                if endpoint.port == 0 || endpoint.host.trim().is_empty() {
                    return Err(Error::Invalid);
                }
                room.endpoint = Some(endpoint);
                Self::activate(room);
                Ok(())
            }
            Command::Abort { id } => self.abort(id),
            Command::Finish { id, digest } => self.finish(caller, id, digest),
        }
    }

    fn enqueue(&mut self, caller: Uuid, ticket: Ticket) -> Result<(), Error> {
        if ticket.home != caller {
            return Err(Error::Forbidden);
        }
        if let Some(old) = self.tickets.get(&ticket.id) {
            if old.ticket.home != caller {
                return Err(Error::Forbidden);
            }
            if old.ticket == ticket {
                return Ok(());
            }
            if old.phase != TicketPhase::Closed || ticket.generation <= old.ticket.generation {
                return Err(Error::Conflict);
            }
        }
        let size = usize::from(ticket.queue.seats);
        if size == 0
            || size > MAX_SEATS
            || ticket.players.is_empty()
            || ticket.players.len() > size
            || ticket.queue.name.is_empty()
            || ticket.queue.rules_digest.is_empty()
            || ticket.players.iter().collect::<BTreeSet<_>>().len() != ticket.players.len()
        {
            return Err(Error::Invalid);
        }
        if ticket
            .players
            .iter()
            .any(|p| self.owners.contains_key(&(caller, *p)))
        {
            return Err(Error::Conflict);
        }
        self.sequence = self.sequence.checked_add(1).ok_or(Error::Conflict)?;
        for p in &ticket.players {
            self.owners.insert((caller, *p), ticket.id);
        }
        self.tickets.insert(
            ticket.id,
            Entry {
                ticket,
                sequence: self.sequence,
                phase: TicketPhase::Queued,
            },
        );
        Ok(())
    }

    fn owned(&self, caller: Uuid, key: TicketRef) -> Result<&Entry, Error> {
        let entry = self.tickets.get(&key.id).ok_or(Error::Missing)?;
        if entry.ticket.home != caller {
            return Err(Error::Forbidden);
        }
        if entry.ticket.generation != key.generation {
            return Err(Error::Conflict);
        }
        Ok(entry)
    }

    fn close(&mut self, id: Uuid) {
        let entry = self.tickets.get_mut(&id).expect("committed ticket");
        entry.phase = TicketPhase::Closed;
        for p in &entry.ticket.players {
            self.owners.remove(&(entry.ticket.home, *p));
        }
    }

    fn cancel(&mut self, caller: Uuid, ticket: TicketRef) -> Result<(), Error> {
        let entry = self.owned(caller, ticket)?;
        if let TicketPhase::Assigned(room) = entry.phase {
            self.abort(room)?;
        }
        self.close(ticket.id);
        Ok(())
    }

    fn allocate(&mut self, id: Uuid, host: Uuid, tickets: Vec<TicketRef>) -> Result<(), Error> {
        if let Some(old) = self.allocations.get(&id) {
            return if old.host == host && old.tickets == tickets {
                Ok(())
            } else {
                Err(Error::Conflict)
            };
        }
        if tickets.is_empty() || tickets.len() > MAX_SEATS {
            return Err(Error::Invalid);
        }
        let mut queue = None;
        let mut roster = BTreeSet::new();
        for key in &tickets {
            let e = self.tickets.get(&key.id).ok_or(Error::Missing)?;
            if e.ticket.generation != key.generation || e.phase != TicketPhase::Queued {
                return Err(Error::Conflict);
            }
            if queue
                .replace(&e.ticket.queue)
                .is_some_and(|q| q != &e.ticket.queue)
                || !e.ticket.players.iter().all(|p| roster.insert(*p))
                || !e
                    .ticket
                    .latency_ms
                    .get(&host)
                    .is_some_and(|v| *v <= e.ticket.max_latency_ms)
            {
                return Err(Error::Invalid);
            }
        }
        if roster.len() != usize::from(queue.expect("nonempty allocation").seats) {
            return Err(Error::Invalid);
        }
        for key in &tickets {
            self.tickets.get_mut(&key.id).unwrap().phase = TicketPhase::Assigned(id);
        }
        self.allocations.insert(
            id,
            Allocation {
                id,
                host,
                tickets,
                accepted: BTreeSet::new(),
                endpoint: None,
                result_digest: None,
                phase: Phase::Preparing,
            },
        );
        Ok(())
    }

    fn activate(room: &mut Allocation) {
        if room.endpoint.is_some() && room.accepted.len() == room.tickets.len() {
            room.phase = Phase::Ready;
        }
    }

    fn abort(&mut self, id: Uuid) -> Result<(), Error> {
        let room = self.allocations.get_mut(&id).ok_or(Error::Missing)?;
        if room.phase == Phase::Aborted {
            return Ok(());
        }
        if room.phase != Phase::Preparing {
            return Err(Error::Conflict);
        }
        room.phase = Phase::Aborted;
        for key in &room.tickets {
            self.tickets.get_mut(&key.id).unwrap().phase = TicketPhase::Queued;
        }
        Ok(())
    }

    fn finish(&mut self, caller: Uuid, id: Uuid, digest: String) -> Result<(), Error> {
        let room = self.allocations.get_mut(&id).ok_or(Error::Missing)?;
        if caller != room.host {
            return Err(Error::Forbidden);
        }
        if let Some(existing) = &room.result_digest {
            return if existing == &digest {
                Ok(())
            } else {
                Err(Error::Conflict)
            };
        }
        if room.phase != Phase::Ready {
            return Err(Error::Conflict);
        }
        if digest.is_empty() || digest.len() > 128 {
            return Err(Error::Invalid);
        }
        room.result_digest = Some(digest);
        room.phase = Phase::Finished;
        for key in room.tickets.clone() {
            self.close(key.id);
        }
        Ok(())
    }
}
