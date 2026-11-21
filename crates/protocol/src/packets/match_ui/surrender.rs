//! Native sixteen-entry team surrender board (`0x0473`), vote request (`0x0472`),
//! and three-seat ARAL projection.
use crate::codec::fixed_body::{self, fixed_packet};
use crate::codec::invalid;
use crate::{EndMatchBody, EndMatchReason, Message, Opcode};
use binrw::{BinRead, BinWrite};
use std::io;

pub const MAX_SURRENDER_SLOTS: usize = 16;
const SURRENDER_BOARD_PREFIX_LEN: usize = 151;
const SURRENDER_BOARD_WIRE_LEN: usize = 155;

/// Observed phase values in `0x0473` (`TeamSurrenderStateChanged`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurrenderPhase {
    Idle,
    Open,
    Passed,
    Failed,
    Unknown(u8),
}

impl SurrenderPhase {
    pub const fn from_wire(raw: u8) -> Self {
        match raw {
            0 => Self::Idle,
            1 => Self::Open,
            2 => Self::Passed,
            3 => Self::Failed,
            other => Self::Unknown(other),
        }
    }

    pub const fn raw(self) -> u8 {
        match self {
            Self::Idle => 0,
            Self::Open => 1,
            Self::Passed => 2,
            Self::Failed => 3,
            Self::Unknown(other) => other,
        }
    }
}

/// Observed vote byte values per slot in `0x0473` and `0x0472`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurrenderVote {
    None,
    Yes,
    No,
    Abstain,
    Unknown(u8),
}

impl SurrenderVote {
    pub const fn from_wire(raw: u8) -> Self {
        match raw {
            0 => Self::None,
            1 => Self::Yes,
            2 => Self::No,
            3 => Self::Abstain,
            other => Self::Unknown(other),
        }
    }

    pub const fn raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Yes => 1,
            Self::No => 2,
            Self::Abstain => 3,
            Self::Unknown(other) => other,
        }
    }
}

/// `0x0472 SetSurrenderStateRequest`: 1-byte client surrender vote packet (often 6 bytes padded).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct SurrenderRequestBody {
    /// Wire vote byte (`1` = yes, `2` = no).
    pub vote: u8,
}
fixed_packet!(SurrenderRequestBody, Opcode::SetSurrenderStateRequest, 1);

impl SurrenderRequestBody {
    /// Returns the typed [`SurrenderVote`] corresponding to [`Self::vote`].
    #[must_use]
    pub const fn typed_vote(self) -> SurrenderVote {
        SurrenderVote::from_wire(self.vote)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, BinRead, BinWrite)]
#[brw(big)]
struct SurrenderBoardWire {
    deadline: f32,
    actors: [u32; MAX_SURRENDER_SLOTS],
    votes: [u8; MAX_SURRENDER_SLOTS],
    cooldowns: [f32; MAX_SURRENDER_SLOTS],
    wire_team: u8,
    phase: u8,
    count: u8,
}

fn valid_time(t: f32) -> bool {
    t.is_finite() && t >= 0.
}

fn valid_entries(max_actor: u32, actors: &[u32], votes: &[u8], cooldowns: &[f32]) -> bool {
    actors.len() == votes.len()
        && actors.len() == cooldowns.len()
        && (1..=MAX_SURRENDER_SLOTS).contains(&actors.len())
        && (0..actors.len()).all(|i| {
            let actor = actors[i];
            if actor == u32::MAX {
                return votes[i] == 0 && cooldowns[i] == 0.;
            }
            (1500..=max_actor).contains(&actor)
                && !actors[..i].contains(&actor)
                && votes[i] <= 3
                && valid_time(cooldowns[i])
        })
}

fn encode_board(
    wire_team: u8,
    phase: u8,
    deadline: f32,
    actors: &[u32],
    votes: &[u8],
    cooldowns: &[f32],
) -> Message {
    let count = actors.len();
    let mut wire = SurrenderBoardWire {
        deadline,
        actors: [0; MAX_SURRENDER_SLOTS],
        votes: [0; MAX_SURRENDER_SLOTS],
        cooldowns: [0.0; MAX_SURRENDER_SLOTS],
        wire_team,
        phase,
        count: count as u8,
    };
    wire.actors[..count].copy_from_slice(actors);
    wire.votes[..count].copy_from_slice(votes);
    wire.cooldowns[..count].copy_from_slice(cooldowns);
    let mut body = vec![0; SURRENDER_BOARD_WIRE_LEN];
    fixed_body::write(&wire, &mut body, SURRENDER_BOARD_PREFIX_LEN)
        .expect("surrender board prefix fits 155-byte body");
    Message::known(Opcode::TeamSurrenderStateChanged, body)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AralSurrenderBoard {
    pub wire_team: u8,
    pub phase: u8,
    pub deadline: f32,
    /// An unfilled position among the three declared slots carries u32::MAX.
    pub actors: [u32; 3],
    pub votes: [u8; 3],
    pub cooldowns: [f32; 3],
}

impl AralSurrenderBoard {
    pub(crate) fn read(b: &[u8]) -> Option<Self> {
        let wire: SurrenderBoardWire = fixed_body::read(b).ok()?;
        if wire.count != 3 {
            return None;
        }
        let board = Self {
            wire_team: wire.wire_team,
            phase: wire.phase,
            deadline: wire.deadline,
            actors: wire.actors[..3].try_into().unwrap(),
            votes: wire.votes[..3].try_into().unwrap(),
            cooldowns: wire.cooldowns[..3].try_into().unwrap(),
        };
        board.valid().then_some(board)
    }

    fn valid(&self) -> bool {
        (1..=2).contains(&self.wire_team)
            && self.phase <= 3
            && valid_time(self.deadline)
            && valid_entries(1505, &self.actors, &self.votes, &self.cooldowns)
    }
}

/// Sixteen-slot team surrender board supporting 3-per-side, 5-per-side, and solo rosters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TeamSurrenderBoard {
    pub wire_team: u8,
    pub phase: u8,
    pub count: u8,
    pub deadline: f32,
    /// Active entries occupy `..usize::from(count)`; unfilled slots carry `u32::MAX`.
    pub actors: [u32; MAX_SURRENDER_SLOTS],
    pub votes: [u8; MAX_SURRENDER_SLOTS],
    pub cooldowns: [f32; MAX_SURRENDER_SLOTS],
}

impl TeamSurrenderBoard {
    pub(crate) fn read(b: &[u8]) -> Option<Self> {
        let wire: SurrenderBoardWire = fixed_body::read(b).ok()?;
        let count = usize::from(wire.count);
        if !(1..=MAX_SURRENDER_SLOTS).contains(&count) {
            return None;
        }
        let mut actors = [0; MAX_SURRENDER_SLOTS];
        let mut votes = [0; MAX_SURRENDER_SLOTS];
        let mut cooldowns = [0.0; MAX_SURRENDER_SLOTS];
        actors[..count].copy_from_slice(&wire.actors[..count]);
        votes[..count].copy_from_slice(&wire.votes[..count]);
        cooldowns[..count].copy_from_slice(&wire.cooldowns[..count]);
        let board = Self {
            wire_team: wire.wire_team,
            phase: wire.phase,
            count: wire.count,
            deadline: wire.deadline,
            actors,
            votes,
            cooldowns,
        };
        board.valid().then_some(board)
    }

    fn valid(&self) -> bool {
        let count = usize::from(self.count);
        (1..=2).contains(&self.wire_team)
            && self.phase <= 3
            && (1..=MAX_SURRENDER_SLOTS).contains(&count)
            && valid_time(self.deadline)
            && valid_entries(
                1515,
                &self.actors[..count],
                &self.votes[..count],
                &self.cooldowns[..count],
            )
    }
}

impl Message {
    /// The stock client emits state 1 on vote initiation. Other state bytes are preserved by the reader.
    pub fn aral_surrender_request() -> Self {
        SurrenderRequestBody {
            vote: SurrenderVote::Yes.raw(),
        }
        .encode()
    }

    pub fn aral_surrender_board(
        wire_team: u8,
        phase: u8,
        deadline: f32,
        actors: [u32; 3],
        votes: [u8; 3],
        cooldowns: [f32; 3],
    ) -> io::Result<Self> {
        let board = AralSurrenderBoard {
            wire_team,
            phase,
            deadline,
            actors,
            votes,
            cooldowns,
        };
        if !board.valid() {
            return Err(invalid("invalid ARAL surrender board"));
        }
        Ok(encode_board(
            wire_team, phase, deadline, &actors, &votes, &cooldowns,
        ))
    }

    /// Builds a `0x0473` surrender board for any active slot count in `1..=16` (3 for 3v3/ARAL/Blitz, 5 for 5v5).
    pub fn team_surrender_board(
        wire_team: u8,
        phase: u8,
        deadline: f32,
        actors: &[u32],
        votes: &[u8],
        cooldowns: &[f32],
    ) -> io::Result<Self> {
        if !(1..=MAX_SURRENDER_SLOTS).contains(&actors.len())
            || actors.len() != votes.len()
            || actors.len() != cooldowns.len()
        {
            return Err(invalid("invalid team surrender board slot count"));
        }
        let count = actors.len();
        let mut board = TeamSurrenderBoard {
            wire_team,
            phase,
            count: count as u8,
            deadline,
            actors: [0; MAX_SURRENDER_SLOTS],
            votes: [0; MAX_SURRENDER_SLOTS],
            cooldowns: [0.0; MAX_SURRENDER_SLOTS],
        };
        board.actors[..count].copy_from_slice(actors);
        board.votes[..count].copy_from_slice(votes);
        board.cooldowns[..count].copy_from_slice(cooldowns);
        if !board.valid() {
            return Err(invalid("invalid team surrender board"));
        }
        Ok(encode_board(
            wire_team, phase, deadline, actors, votes, cooldowns,
        ))
    }

    pub fn aral_surrender_result(winning_wire_team: u8) -> io::Result<Self> {
        if !(1..=2).contains(&winning_wire_team) {
            return Err(invalid("invalid surrender winning team"));
        }
        Ok(EndMatchBody {
            team_word: u32::from(winning_wire_team),
            reason: EndMatchReason::Surrender,
        }
        .encode_padded())
    }

    pub fn surrender_result(winning_wire_team: u8) -> io::Result<Self> {
        Self::aral_surrender_result(winning_wire_team)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Fields, LiveFields, MatchCipher};

    #[test]
    fn three_slot_board_and_request_survive_encryption() {
        let cipher = MatchCipher::new("synthetic-offer");
        let roundtrip = |m: &Message| cipher.decode(&cipher.encode(m).unwrap()[2..]).unwrap();
        let req = roundtrip(&Message::aral_surrender_request());
        assert!(matches!(
            req.fields().unwrap(),
            Fields::Live(LiveFields::SurrenderRequest(1))
        ));
        let surr_req = SurrenderRequestBody { vote: 1 };
        assert_eq!(surr_req.typed_vote(), SurrenderVote::Yes);
        assert_eq!(
            SurrenderRequestBody::read_padded_zero(&surr_req.encode_padded().body).ok(),
            Some(surr_req)
        );
        let actors = [1504, 1500, u32::MAX];
        let msg =
            Message::aral_surrender_board(1, 1, 120., actors, [1, 3, 0], [210., 210., 0.]).unwrap();
        let mut decoded = roundtrip(&msg);
        assert_eq!(decoded.body.len(), 158);
        for range in [16..68, 71..84, 96..148, 151..158] {
            assert!(decoded.body[range.clone()].iter().all(|v| *v == 0));
            decoded.body[range].fill(0xa5);
        }
        assert_eq!(
            decoded.fields().unwrap(),
            Fields::Live(LiveFields::AralSurrenderBoard(AralSurrenderBoard {
                wire_team: 1,
                phase: 1,
                deadline: 120.,
                actors,
                votes: [1, 3, 0],
                cooldowns: [210., 210., 0.],
            }))
        );
        assert_eq!(roundtrip(&decoded), decoded);
    }

    #[test]
    fn validates_actors_times_and_both_terminal_teams() {
        for team in [1, 2] {
            assert_eq!(
                Message::aral_surrender_result(team).unwrap().body,
                [0, 0, 0, team, 2, 0]
            );
        }
        assert!(Message::aral_surrender_result(0).is_err());
        for (actors, cooldowns) in [
            ([1500, 1500, 1501], [0.; 3]),
            ([1500, 1501, u32::MAX], [0., 0., 1.]),
            ([1500, 1501, 1502], [0., f32::NAN, 0.]),
        ] {
            assert!(Message::aral_surrender_board(1, 2, 1., actors, [0; 3], cooldowns).is_err());
        }
    }

    #[test]
    fn five_slot_and_solo_team_boards_survive_encryption() {
        let cipher = MatchCipher::new("synthetic-5v5");
        let roundtrip = |m: &Message| cipher.decode(&cipher.encode(m).unwrap()[2..]).unwrap();
        let actors = [1501, 1503, 1505, 1507, 1509];
        let votes = [1, 1, 0, 2, 1];
        let cooldowns = [210., 210., 0., 210., 210.];
        let msg = Message::team_surrender_board(2, 3, 5., &actors, &votes, &cooldowns).unwrap();
        let decoded = roundtrip(&msg);
        let Fields::Live(LiveFields::TeamSurrenderBoard(board)) = decoded.fields().unwrap() else {
            panic!("expected 5-seat TeamSurrenderBoard");
        };
        assert_eq!(board.wire_team, 2);
        assert_eq!(board.phase, 3);
        assert_eq!(board.count, 5);
        assert_eq!(board.deadline, 5.);
        assert_eq!(&board.actors[..5], &actors);
        assert_eq!(&board.votes[..5], &votes);
        assert_eq!(&board.cooldowns[..5], &cooldowns);
        assert!(board.actors[5..].iter().all(|&a| a == 0));

        assert_eq!(
            Message::surrender_result(1).unwrap().body,
            [0, 0, 0, 1, 2, 0]
        );
        assert!(Message::team_surrender_board(1, 1, 30., &[], &[], &[]).is_err());
        assert!(Message::team_surrender_board(1, 1, 30., &[1516; 1], &[1], &[0.]).is_err());
    }
}
