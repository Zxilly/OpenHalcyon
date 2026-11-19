//! Team-private ARAL surrender ballots. Timers run on the authoritative room clock.
use super::{Message, Room, messages::wire_id, transport::Delivery, transport::TeamBatch};
use halcyon_sim::TICK_HZ;

const OPEN: u8 = 1;
const PASSED: u8 = 2;
const FAILED: u8 = 3;
const YES: u8 = 1;
const NO: u8 = 2;

#[derive(Default)]
pub(super) struct Ballot {
    phase: u8,
    votes: [u8; 3],
    deadline: u64,
    retry_at: u64,
}

impl Ballot {
    pub fn passed(&self) -> bool {
        self.phase == PASSED
    }

    fn vote(&mut self, seat: usize, value: u8, now: u64) -> bool {
        if !matches!(value, YES | NO) {
            return false;
        }
        if self.phase == 0 && value == YES && now >= self.retry_at {
            self.phase = OPEN;
            self.votes = [0; 3];
            self.deadline = now + 30 * TICK_HZ;
            self.retry_at = now + 120 * TICK_HZ;
        }
        if self.phase != OPEN || now >= self.deadline || self.votes[seat] != 0 {
            return false;
        }
        self.votes[seat] = value;
        if value == NO {
            self.phase = FAILED;
            self.deadline = now + 5 * TICK_HZ;
        } else if self.votes == [YES; 3] {
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

impl Room {
    fn voting_team(&self, team: u8) -> Option<[u32; 3]> {
        let mut members = self
            .world
            .entities
            .iter()
            .take(self.seats.len())
            .filter(|e| e.team == team);
        let ids = [members.next()?.id, members.next()?.id, members.next()?.id];
        members.next().is_none().then_some(ids)
    }

    pub(super) fn surrender_snapshot(&self, team: u8) -> Option<Message> {
        let ballot = self.surrender.get(usize::from(team))?;
        if ballot.retry_at == 0 {
            return None;
        }
        let actors = self
            .voting_team(team)?
            .map(|actor| u32::from(wire_id(actor)));
        let seconds = |ticks: u64| ticks as f32 / TICK_HZ as f32;
        Message::aral_surrender_board(
            team + 1,
            ballot.phase,
            seconds(ballot.deadline),
            actors,
            ballot.votes,
            [seconds(ballot.retry_at); 3],
        )
        .ok()
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

    pub(super) fn vote_surrender(&mut self, seat: usize, value: u8) {
        let actor = &self.world.entities[seat];
        let team = actor.team;
        let Some(index) = self
            .voting_team(team)
            .and_then(|ids| ids.iter().position(|id| *id == actor.id))
        else {
            return;
        };
        if !self.surrender[usize::from(team)].vote(index, value, self.world.tick) {
            return;
        }
        self.send_surrender(team);
        if self.surrender[usize::from(team)].passed() {
            self.world.winner = Some(1 - team);
            self.reason = Some("completed");
            self.phase("ending");
        }
    }

    pub(super) fn advance_ballots(&mut self) {
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
        assert!(!ballot.vote(0, NO, 0));
        assert!(!ballot.vote(0, 8, 0));
        assert!(ballot.vote(0, YES, 0));
        assert!(!ballot.vote(0, YES, 1));
        assert!(ballot.vote(2, NO, 1));
        assert_eq!(ballot.phase, FAILED);
        assert!(!ballot.advance(5 * TICK_HZ));
        assert!(ballot.advance(5 * TICK_HZ + 1));
        assert!(!ballot.vote(1, YES, 120 * TICK_HZ - 1));
        assert!(ballot.vote(1, YES, 120 * TICK_HZ));
        assert_eq!(ballot.votes, [0, 1, 0]);
        assert!(ballot.advance(150 * TICK_HZ));
        assert!(!ballot.vote(0, YES, 150 * TICK_HZ));
    }
}
