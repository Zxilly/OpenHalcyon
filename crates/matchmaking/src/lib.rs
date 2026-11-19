//! Deterministic state transitions for peer matchmaking.
//!
//! Apply committed commands in log order. Host health and clocks are inputs to
//! the proposal builder, never to the replicated transition function.

mod proposal;
mod work;
pub use proposal::{HostOffer, MatchPlan, plan};
pub use work::WorkingSet;

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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub talent_levels: Vec<halcyon_data::TalentLevels>,
    pub queue: Queue,
    /// Measured or policy-approved estimates from this party to prospective hosts.
    pub latency_ms: BTreeMap<Uuid, u32>,
    pub max_latency_ms: u32,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
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

pub const OFFER_WINDOW_SECS: i64 = 35;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Allocation {
    pub id: Uuid,
    pub host: Uuid,
    pub tickets: Vec<TicketRef>,
    pub phase: Phase,
    pub accepted: BTreeSet<Uuid>,
    pub endpoint: Option<Endpoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    pub result_digest: Option<String>,
    #[serde(default)]
    pub result: Option<serde_json::Value>,
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
    Expire {
        id: Uuid,
    },
    Settle {
        id: Uuid,
        digest: String,
        result: serde_json::Value,
    },
    Finish {
        id: Uuid,
        digest: String,
    },
    Reject {
        id: Uuid,
        ticket: TicketRef,
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
#[serde(from = "work::PersistedPool")]
pub struct Pool {
    tickets: BTreeMap<Uuid, Entry>,
    allocations: BTreeMap<Uuid, Allocation>,
    /// A wire UUID can appear at different homes, but only once per roster.
    owners: BTreeMap<(Uuid, Uuid), Uuid>,
    sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    offer_epoch: Option<i64>,
    #[serde(skip)]
    active_tickets: BTreeSet<Uuid>,
    #[serde(skip)]
    active_rooms: BTreeSet<Uuid>,
    #[serde(skip)]
    finished_by_ticket: BTreeMap<TicketRef, Uuid>,
}

impl Pool {
    pub fn tickets(&self) -> &BTreeMap<Uuid, Entry> {
        &self.tickets
    }
    pub fn allocations(&self) -> &BTreeMap<Uuid, Allocation> {
        &self.allocations
    }
    pub fn active_ticket(&self, home: Uuid, player: Uuid) -> Option<Uuid> {
        self.owners.get(&(home, player)).copied()
    }

    /// Apply a pre-deadline historical entry or a direct standalone model command.
    pub fn apply(&mut self, caller: Uuid, command: Command) -> Result<(), Error> {
        self.apply_at(caller, command, None)
    }

    /// The transport supplies time from the proposing leader's committed entry.
    /// No replica reads its own wall clock while applying a command.
    pub fn apply_at(
        &mut self,
        caller: Uuid,
        command: Command,
        epoch: Option<i64>,
    ) -> Result<(), Error> {
        let epoch = epoch.map(|epoch| {
            let fenced = self
                .offer_epoch
                .map_or(epoch, |previous| previous.max(epoch));
            self.offer_epoch = Some(fenced);
            fenced
        });
        match command {
            Command::Enqueue(ticket) => self.enqueue(caller, ticket),
            Command::Cancel(ticket) => self.cancel(caller, ticket),
            Command::Reject { id, ticket } => {
                if self.owned(caller, ticket)?.phase != TicketPhase::Assigned(id) {
                    return Err(Error::Conflict);
                }
                let room = self.allocations.get_mut(&id).ok_or(Error::Missing)?;
                if room.phase != Phase::Preparing {
                    return Err(Error::Conflict);
                }
                Self::check_window(room, epoch)?;
                self.cancel(caller, ticket)
            }
            Command::Allocate { id, host, tickets } => self.allocate(id, host, tickets, epoch),
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
                Self::check_window(room, epoch)?;
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
                Self::check_window(room, epoch)?;
                room.endpoint = Some(endpoint);
                Self::activate(room);
                Ok(())
            }
            Command::Abort { id } => self.abort(id),
            Command::Expire { id } => {
                let now = epoch.ok_or(Error::Invalid)?;
                let room = self.allocations.get_mut(&id).ok_or(Error::Missing)?;
                if room.phase != Phase::Preparing {
                    return Ok(());
                }
                let deadline = room
                    .expires_at
                    .get_or_insert(now.saturating_add(OFFER_WINDOW_SECS));
                if now < *deadline {
                    return Ok(());
                }
                self.abort(id)
            }
            Command::Finish { id, digest } => self.finish(caller, id, digest, None),
            Command::Settle { id, digest, result } => {
                if result_digest(&result).as_deref() != Ok(digest.as_str()) {
                    return Err(Error::Invalid);
                }
                self.finish(caller, id, digest, Some(result))
            }
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
            || (!ticket.talent_levels.is_empty()
                && !ticket
                    .queue
                    .rules_digest
                    .starts_with(halcyon_data::ARAL_TALENT_RULES_PREFIX))
            || !halcyon_data::valid_talent_levels(
                &ticket.queue.name,
                ticket.players.len(),
                &ticket.talent_levels,
            )
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
        self.active_tickets.insert(ticket.id);
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
        self.active_tickets.remove(&id);
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

    fn allocate(
        &mut self,
        id: Uuid,
        host: Uuid,
        tickets: Vec<TicketRef>,
        epoch: Option<i64>,
    ) -> Result<(), Error> {
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
        self.active_rooms.insert(id);
        self.allocations.insert(
            id,
            Allocation {
                id,
                host,
                tickets,
                accepted: BTreeSet::new(),
                endpoint: None,
                expires_at: epoch.map(|epoch| epoch.saturating_add(OFFER_WINDOW_SECS)),
                result_digest: None,
                result: None,
                phase: Phase::Preparing,
            },
        );
        Ok(())
    }

    fn check_window(room: &mut Allocation, epoch: Option<i64>) -> Result<(), Error> {
        if epoch.is_none() && room.expires_at.is_some() {
            return Err(Error::Invalid);
        }
        if let Some(now) = epoch {
            let deadline = room
                .expires_at
                .get_or_insert(now.saturating_add(OFFER_WINDOW_SECS));
            if now >= *deadline {
                return Err(Error::Conflict);
            }
        }
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
        self.active_rooms.remove(&id);
        for key in &room.tickets {
            self.tickets.get_mut(&key.id).unwrap().phase = TicketPhase::Queued;
        }
        Ok(())
    }

    fn finish(
        &mut self,
        caller: Uuid,
        id: Uuid,
        digest: String,
        result: Option<serde_json::Value>,
    ) -> Result<(), Error> {
        let room = self.allocations.get_mut(&id).ok_or(Error::Missing)?;
        if caller != room.host {
            return Err(Error::Forbidden);
        }
        if let Some(existing) = &room.result_digest {
            return if existing == &digest
                && (room.result.is_none() || result.is_none() || result == room.result)
            {
                if room.result.is_none() {
                    room.result = result;
                }
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
        room.result = result;
        room.phase = Phase::Finished;
        self.active_rooms.remove(&id);
        for key in room.tickets.clone() {
            self.finished_by_ticket
                .entry(key)
                .and_modify(|room| *room = (*room).min(id))
                .or_insert(id);
            self.close(key.id);
        }
        Ok(())
    }
}

/// SHA-256 over a bounded compact JSON summary with recursively sorted object keys.
pub fn result_digest(result: &serde_json::Value) -> Result<String, Error> {
    use sha2::{Digest, Sha256};
    let mut canonical = result.clone();
    canonical.sort_all_objects();
    let bytes = serde_json::to_vec(&canonical).map_err(|_| Error::Invalid)?;
    if bytes.len() > 8192 {
        return Err(Error::Invalid);
    }
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
