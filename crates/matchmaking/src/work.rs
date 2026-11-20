use super::{
    Allocation, Entry, HostOffer, MatchPlan, Phase, Pool, TicketPhase, TicketRef, proposal,
};
use serde::Deserialize;
use std::collections::BTreeMap;
use uuid::Uuid;

/// The original snapshot/replication format. Indexes are derived on recovery.
#[derive(Deserialize)]
pub(super) struct PersistedPool {
    tickets: BTreeMap<Uuid, Entry>,
    allocations: BTreeMap<Uuid, Allocation>,
    owners: BTreeMap<(Uuid, Uuid), Uuid>,
    sequence: u64,
    #[serde(default)]
    offer_epoch: Option<i64>,
}

impl From<PersistedPool> for Pool {
    fn from(source: PersistedPool) -> Self {
        let mut pool = Self {
            tickets: source.tickets,
            allocations: source.allocations,
            owners: source.owners,
            sequence: source.sequence,
            offer_epoch: source.offer_epoch,
            ..Self::default()
        };
        pool.active_tickets.extend(
            pool.tickets
                .iter()
                .filter(|(_, entry)| entry.phase != TicketPhase::Closed)
                .map(|(&id, _)| id),
        );
        for (&id, room) in &pool.allocations {
            match room.phase {
                Phase::Preparing | Phase::Ready => {
                    pool.active_rooms.insert(id);
                }
                Phase::Finished => {
                    for &ticket in &room.tickets {
                        // Allocations are iterated in UUID order.
                        pool.finished_by_ticket.entry(ticket).or_insert(id);
                    }
                }
                Phase::Aborted => {}
            }
        }
        pool
    }
}

/// Owned projection for one matchmaking worker round.
pub struct WorkingSet {
    pub tickets: BTreeMap<Uuid, Entry>,
    pub allocations: BTreeMap<Uuid, Allocation>,
    settled: BTreeMap<TicketRef, Uuid>,
}

impl Pool {
    /// Count queued and assigned tickets for the home without scanning closed entries.
    pub fn active_ticket_count(&self, home: Uuid) -> usize {
        self.active_tickets
            .iter()
            .filter(|&id| self.tickets[id].ticket.home == home)
            .count()
    }

    /// Copy only active data plus explicitly requested home and host history.
    /// Callers must obtain the underlying pool through a quorum-confirmed read.
    pub fn working_set(
        &self,
        home: impl IntoIterator<Item = Uuid>,
        hosted: impl IntoIterator<Item = Uuid>,
    ) -> WorkingSet {
        let mut result = WorkingSet {
            tickets: self
                .active_tickets
                .iter()
                .map(|id| (*id, self.tickets[id].clone()))
                .collect(),
            allocations: self
                .active_rooms
                .iter()
                .map(|id| (*id, self.allocations[id].clone()))
                .collect(),
            settled: BTreeMap::new(),
        };
        for id in home {
            let Some(entry) = self.tickets.get(&id) else {
                continue;
            };
            if entry.phase == TicketPhase::Closed {
                let key = TicketRef {
                    id,
                    generation: entry.ticket.generation,
                };
                if let Some(room) = self.finished_by_ticket.get(&key) {
                    result.settled.insert(key, *room);
                    result
                        .allocations
                        .entry(*room)
                        .or_insert_with(|| self.allocations[room].clone());
                }
            }
            result.tickets.entry(id).or_insert_with(|| entry.clone());
        }
        for id in hosted {
            if let Some(room) = self.allocations.get(&id) {
                result.allocations.entry(id).or_insert_with(|| room.clone());
            }
        }
        result
    }
}

impl WorkingSet {
    pub fn finished(&self, ticket: TicketRef) -> Option<&Allocation> {
        self.settled
            .get(&ticket)
            .and_then(|id| self.allocations.get(id))
    }

    pub fn plan(&self, anchor: Uuid, offers: &[HostOffer]) -> Option<MatchPlan> {
        proposal::plan_active(self.tickets.get(&anchor)?, self.tickets.values(), offers)
    }
}
