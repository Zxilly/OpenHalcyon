//! Team-private surrender ballots across 3v3, 5v5, Blitz, ARAL, and Solo/Co-op bot modes.
//! Timers run on the authoritative room clock.
use crate::Message;
use crate::replication::messages::wire_id;
use crate::room::Room;
use crate::transport::{Delivery, TeamBatch};
use halcyon_protocol::MAX_SURRENDER_SLOTS;
use halcyon_sim::{Kind, TICK_HZ};

const OPEN: u8 = 1;
const PASSED: u8 = 2;
const FAILED: u8 = 3;
const YES: u8 = 1;
const NO: u8 = 2;

#[derive(Default)]
pub(crate) struct Ballot {
    phase: u8,
    count: usize,
    votes: [u8; MAX_SURRENDER_SLOTS],
    deadline: u64,
    retry_at: u64,
}

impl Ballot {
    pub fn passed(&self) -> bool {
        self.phase == PASSED
    }

    fn vote(&mut self, slot: usize, count: usize, human_mask: u16, value: u8, now: u64) -> bool {
        if !(1..=MAX_SURRENDER_SLOTS).contains(&count)
            || slot >= count
            || (human_mask & (1 << slot)) == 0
            || !matches!(value, YES | NO)
        {
            return false;
        }
        if self.phase == 0 && value == YES && now >= self.retry_at {
            self.phase = OPEN;
            self.count = count;
            self.votes = [0; MAX_SURRENDER_SLOTS];
            for (idx, slot_vote) in self.votes.iter_mut().enumerate().take(count) {
                if (human_mask & (1 << idx)) == 0 {
                    // Allied AI bots never block or stall a human surrender vote.
                    *slot_vote = YES;
                }
            }
            self.deadline = now + 30 * TICK_HZ;
            self.retry_at = now + 120 * TICK_HZ;
        }
        if self.phase != OPEN
            || count != self.count
            || now >= self.deadline
            || self.votes[slot] != 0
        {
            return false;
        }
        self.votes[slot] = value;
        if value == NO {
            self.phase = FAILED;
            self.deadline = now + 5 * TICK_HZ;
        } else if (0..count).all(|idx| (human_mask & (1 << idx)) == 0 || self.votes[idx] == YES) {
            self.phase = PASSED;
            self.deadline = now + 15 * TICK_HZ;
        }
        true
    }

    fn advance(&mut self, now: u64) -> bool {
        if now < self.deadline {
            return false;
        }
        match self.phase {
            OPEN => {
                self.phase = FAILED;
                self.deadline = now + 5 * TICK_HZ;
            }
            FAILED => {
                self.phase = 0;
                self.deadline = 0;
            }
            _ => return false,
        }
        true
    }
}

struct TeamRoster {
    actors: [u32; MAX_SURRENDER_SLOTS],
    count: usize,
    human_mask: u16,
}

impl TeamRoster {
    fn slot_of(&self, actor_id: u32) -> Option<usize> {
        self.actors[..self.count]
            .iter()
            .position(|&id| id == actor_id)
    }
}

impl Room {
    fn voting_team(&self, team: u8) -> Option<TeamRoster> {
        let mut actors = [0u32; MAX_SURRENDER_SLOTS];
        let mut count = 0usize;
        let mut human_mask = 0u16;
        for (seat_idx, entity) in self
            .world
            .entities
            .iter()
            .take(self.picks.len())
            .enumerate()
            .filter(|(_, e)| e.kind == Kind::Hero && e.team == team)
        {
            if count >= MAX_SURRENDER_SLOTS {
                return None;
            }
            actors[count] = entity.id;
            if !entity.bot && seat_idx < self.players.len() {
                human_mask |= 1 << count;
            }
            count += 1;
        }
        (count > 0 && human_mask != 0).then_some(TeamRoster {
            actors,
            count,
            human_mask,
        })
    }

    pub(crate) fn surrender_snapshot(&self, team: u8) -> Option<Message> {
        let ballot = self.surrender.get(usize::from(team))?;
        if ballot.retry_at == 0 {
            return None;
        }
        let roster = self.voting_team(team)?;
        let count = if ballot.count > 0 {
            ballot.count.min(roster.count)
        } else {
            roster.count
        };
        let seconds = |ticks: u64| ticks as f32 / TICK_HZ as f32;
        let cooldown = seconds(ballot.retry_at);
        let wire_actors: [u32; MAX_SURRENDER_SLOTS] = std::array::from_fn(|idx| {
            if idx < count {
                u32::from(wire_id(roster.actors[idx]))
            } else {
                0
            }
        });
        if count == 3
            && wire_actors[..3]
                .iter()
                .all(|&actor| (1500..=1505).contains(&actor))
        {
            Message::aral_surrender_board(
                team + 1,
                ballot.phase,
                seconds(ballot.deadline),
                [wire_actors[0], wire_actors[1], wire_actors[2]],
                [ballot.votes[0], ballot.votes[1], ballot.votes[2]],
                [cooldown; 3],
            )
            .ok()
        } else {
            let cooldowns = [cooldown; MAX_SURRENDER_SLOTS];
            Message::team_surrender_board(
                team + 1,
                ballot.phase,
                seconds(ballot.deadline),
                &wire_actors[..count],
                &ballot.votes[..count],
                &cooldowns[..count],
            )
            .ok()
        }
    }

    fn send_surrender(&self, team: u8) {
        if let Some(message) = self.surrender_snapshot(team) {
            let delivery = TeamBatch::new(vec![message]);
            for (seat, hero) in self.seats.iter().zip(&self.world.entities) {
                if hero.team == team
                    && let Some(peer) = seat.gameplay_peer()
                {
                    peer.send(Delivery::Team(delivery.clone()));
                }
            }
        }
    }

    pub(crate) fn vote_surrender(&mut self, seat: usize, value: u8) {
        let Some(actor) = self.world.entities.get(seat) else {
            return;
        };
        if actor.bot || seat >= self.players.len() {
            return;
        }
        let actor_id = actor.id;
        let team = actor.team;
        let Some(roster) = self.voting_team(team) else {
            return;
        };
        let Some(slot) = roster.slot_of(actor_id) else {
            return;
        };
        if !self.surrender[usize::from(team)].vote(
            slot,
            roster.count,
            roster.human_mask,
            value,
            self.world.tick,
        ) {
            return;
        }
        self.send_surrender(team);
        if self.surrender[usize::from(team)].passed() {
            self.world.winner = Some(1 - team);
            self.reason = Some(crate::RoomEndReason::Completed);
            self.phase(crate::RoomPhase::Ending);
        }
    }

    pub(crate) fn advance_ballots(&mut self) {
        for team in 0..2 {
            if self.surrender[usize::from(team)].advance(self.world.tick) {
                self.send_surrender(team);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn each_seat_votes_once_and_cooldown_survives_closed_ballots() {
        let mut ballot = Ballot::default();
        assert!(!ballot.vote(0, 3, 0b111, NO, 0));
        assert!(!ballot.vote(0, 3, 0b111, 8, 0));
        assert!(ballot.vote(0, 3, 0b111, YES, 0));
        assert!(!ballot.vote(0, 3, 0b111, YES, 1));
        assert!(ballot.vote(2, 3, 0b111, NO, 1));
        assert_eq!(ballot.phase, FAILED);
        assert!(!ballot.advance(5 * TICK_HZ));
        assert!(ballot.advance(5 * TICK_HZ + 1));
        assert!(!ballot.vote(1, 3, 0b111, YES, 120 * TICK_HZ - 1));
        assert!(ballot.vote(1, 3, 0b111, YES, 120 * TICK_HZ));
        assert_eq!(&ballot.votes[..3], &[0, 1, 0]);
        assert!(ballot.advance(150 * TICK_HZ));
        assert!(!ballot.vote(0, 3, 0b111, YES, 150 * TICK_HZ));
    }

    #[test]
    fn five_player_and_solo_bot_ballots_resolve_without_bot_stalls() {
        let mut five = Ballot::default();
        for slot in 0..4 {
            assert!(five.vote(slot, 5, 0b1_1111, YES, 10));
            assert!(!five.passed());
        }
        assert!(five.vote(4, 5, 0b1_1111, YES, 11));
        assert!(five.passed());

        let mut solo = Ballot::default();
        assert!(solo.vote(0, 5, 0b0_0001, YES, 50));
        assert!(solo.passed());
        assert_eq!(&solo.votes[..5], &[YES; 5]);

        let mut coop_partial = Ballot::default();
        assert!(coop_partial.vote(0, 5, 0b0_0011, YES, 50));
        assert!(!coop_partial.passed());
        assert!(coop_partial.vote(1, 5, 0b0_0011, YES, 51));
        assert!(coop_partial.passed());
    }
}
