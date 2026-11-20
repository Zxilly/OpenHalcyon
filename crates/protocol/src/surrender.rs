//! ARAL's three-seat view of the sixteen-entry native team surrender board.
use crate::{Message, invalid};
use std::io;

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
        if b[150] != 3 {
            return None;
        }
        let word = |i: usize| u32::from_be_bytes(b[i..i + 4].try_into().unwrap());
        let board = Self {
            wire_team: b[148],
            phase: b[149],
            deadline: f32::from_bits(word(0)),
            actors: std::array::from_fn(|i| word(4 + 4 * i)),
            votes: b[68..71].try_into().unwrap(),
            cooldowns: std::array::from_fn(|i| f32::from_bits(word(84 + 4 * i))),
        };
        board.valid().then_some(board)
    }
    fn valid(&self) -> bool {
        let time = |t: f32| t.is_finite() && t >= 0.;
        (1..=2).contains(&self.wire_team)
            && self.phase <= 3
            && time(self.deadline)
            && (0..3).all(|i| {
                let actor = self.actors[i];
                if actor == u32::MAX {
                    return self.votes[i] == 0 && self.cooldowns[i] == 0.;
                }
                (1500..=1505).contains(&actor)
                    && !self.actors[..i].contains(&actor)
                    && self.votes[i] <= 3
                    && time(self.cooldowns[i])
            })
    }
}

impl Message {
    /// The stock ARAL client emitted state 1. Other state bytes are preserved by the reader.
    pub fn aral_surrender_request() -> Self {
        Self::known(crate::Opcode::SetSurrenderStateRequest, [1])
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
        let mut body = [0; 155];
        body[..4].copy_from_slice(&deadline.to_be_bytes());
        for i in 0..3 {
            body[4 + 4 * i..8 + 4 * i].copy_from_slice(&actors[i].to_be_bytes());
            body[68 + i] = votes[i];
            body[84 + 4 * i..88 + 4 * i].copy_from_slice(&cooldowns[i].to_be_bytes());
        }
        body[148..151].copy_from_slice(&[wire_team, phase, 3]);
        Ok(Self::known(crate::Opcode::TeamSurrenderStateChanged, body))
    }

    pub fn aral_surrender_result(winning_wire_team: u8) -> io::Result<Self> {
        if !(1..=2).contains(&winning_wire_team) {
            return Err(invalid("invalid ARAL winning team"));
        }
        Ok(Self::known(
            crate::Opcode::EndMatch,
            [0, 0, 0, winning_wire_team, 2, 0],
        ))
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
}
