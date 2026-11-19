use super::{MAX_SEATS, Pool, Queue, TicketPhase, TicketRef};
use uuid::Uuid;

/// The caller filters expired offers before invoking the planner.
#[derive(Debug, Clone)]
pub struct HostOffer {
    pub id: Uuid,
    pub queue: Queue,
    pub free_seats: usize,
    pub total_seats: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchPlan {
    pub host: Uuid,
    pub tickets: Vec<TicketRef>,
}

/// Keep parties intact and include the anchor. A bounded subset-sum table
/// avoids greedily picking parties whose sizes leave an unfillable last seat.
pub fn plan(pool: &Pool, anchor: Uuid, hosts: &[HostOffer]) -> Option<MatchPlan> {
    let first = pool.tickets().get(&anchor)?;
    if first.phase != TicketPhase::Queued {
        return None;
    }
    let seats = usize::from(first.ticket.queue.seats);
    let mut waiting: Vec<_> = pool
        .tickets()
        .values()
        .filter(|entry| {
            entry.phase == TicketPhase::Queued
                && entry.ticket.queue == first.ticket.queue
                && entry.ticket.id != anchor
        })
        .collect();
    waiting.sort_unstable_by_key(|entry| entry.sequence);
    hosts
        .iter()
        .filter_map(|offer| {
            if offer.queue != first.ticket.queue
                || offer.free_seats < seats
                || offer.total_seats < offer.free_seats
            {
                return None;
            }
            let latency = |ticket: &super::Ticket| {
                ticket
                    .latency_ms
                    .get(&offer.id)
                    .copied()
                    .filter(|ms| *ms <= ticket.max_latency_ms)
            };
            latency(&first.ticket)?;
            let mut by_size: [Option<Vec<&super::Entry>>; MAX_SEATS + 1] = Default::default();
            by_size[first.ticket.players.len()] = Some(vec![first]);
            for candidate in &waiting {
                if by_size[seats].is_some() {
                    break;
                }
                if latency(&candidate.ticket).is_none() {
                    continue;
                }
                let n = candidate.ticket.players.len();
                for used in (1..=seats - n).rev() {
                    if by_size[used + n].is_some() {
                        continue;
                    }
                    let Some(team) = &by_size[used] else { continue };
                    if team.iter().any(|other| {
                        other
                            .ticket
                            .players
                            .iter()
                            .any(|id| candidate.ticket.players.contains(id))
                    }) {
                        continue;
                    }
                    let mut next = team.clone();
                    next.push(candidate);
                    by_size[used + n] = Some(next);
                }
            }
            let team = by_size[seats].take()?;
            let oldest: Vec<_> = team.iter().map(|e| e.sequence).collect();
            let worst_ms = team.iter().filter_map(|e| latency(&e.ticket)).max()?;
            let load =
                (offer.total_seats - offer.free_seats) as u128 * 10_000 / offer.total_seats as u128;
            let tickets = team
                .iter()
                .map(|e| TicketRef {
                    id: e.ticket.id,
                    generation: e.ticket.generation,
                })
                .collect();
            Some((
                (oldest, worst_ms, load, offer.id),
                MatchPlan {
                    host: offer.id,
                    tickets,
                },
            ))
        })
        .min_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, plan)| plan)
}
