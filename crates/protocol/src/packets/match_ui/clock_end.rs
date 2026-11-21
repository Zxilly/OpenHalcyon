//! World clock, mode variables, match start/end markers, and spectator exit packets.

use crate::codec::fixed_body::fixed_packet;
use crate::{Message, Opcode};
use binrw::{BinRead, BinWrite};

/// `0x041f ModifyGameModeVar`: 8-byte game mode variable assignment (often padded to 14 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct ModifyGameModeVarBody {
    /// Variable key hash/identifier.
    pub key: u32,
    /// Assigned integer value.
    pub value: u32,
}
fixed_packet!(ModifyGameModeVarBody, Opcode::ModifyGameModeVar, 8);

/// `0x0451 SyncWorldClock`: 4-byte authoritative match clock synchronization packet.
#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct WorldClockBody {
    /// Elapsed world time in seconds.
    pub seconds: f32,
}
fixed_packet!(WorldClockBody, Opcode::SyncWorldClock, 4);

/// `0x0000 ClockSync`: 4-byte heartbeat/clock synchronization packet.
#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct ClockSyncBody {
    /// Clock value in seconds.
    pub seconds: f32,
}
fixed_packet!(ClockSyncBody, Opcode::ClockSync, 4);

/// Known `0x03f1 EndMatch` completion reason codes, preserving unknown bytes losslessly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, BinRead, BinWrite)]
#[brw(big)]
pub enum EndMatchReason {
    /// Vain crystal shattered (`0`).
    #[brw(magic = 0u8)]
    CrystalShattered,
    /// Match ended by team surrender (`2`).
    #[brw(magic = 2u8)]
    Surrender,
    /// Match ended by score or time limit (`3`).
    #[brw(magic = 3u8)]
    ScoreOrTimeLimit,
    /// Match ended during draft/early termination (`8`).
    #[brw(magic = 8u8)]
    DraftSync,
    /// Uncatalogued match end reason byte.
    Unknown(u8),
}

impl EndMatchReason {
    /// Converts a raw wire byte into [`EndMatchReason`].
    #[must_use]
    pub const fn from_wire(raw: u8) -> Self {
        match raw {
            0 => Self::CrystalShattered,
            2 => Self::Surrender,
            3 => Self::ScoreOrTimeLimit,
            8 => Self::DraftSync,
            other => Self::Unknown(other),
        }
    }

    /// Converts [`EndMatchReason`] into its wire byte.
    #[must_use]
    pub const fn to_wire(self) -> u8 {
        match self {
            Self::CrystalShattered => 0,
            Self::Surrender => 2,
            Self::ScoreOrTimeLimit => 3,
            Self::DraftSync => 8,
            Self::Unknown(other) => other,
        }
    }
}

/// `0x03f1 EndMatch`: 5-byte match result frame (often padded to 6 bytes on the wire).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct EndMatchBody {
    /// Winning wire team word (`1` = Left, `2` = Right).
    pub team_word: u32,
    /// Match termination reason code.
    pub reason: EndMatchReason,
}
fixed_packet!(EndMatchBody, Opcode::EndMatch, 5);

/// `0x0452 CrystalShattered`: 0-byte vain-crystal destruction marker frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct CrystalShatteredBody;
fixed_packet!(CrystalShatteredBody, Opcode::CrystalShattered, 0);

/// `0x03ef StartMatch`: 0-byte match start marker frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct StartMatchBody;
fixed_packet!(StartMatchBody, Opcode::StartMatch, 0);

impl Message {
    /// The Android spectator-exit wrapper writes one zero operand before block padding.
    pub fn is_spectator_exit(&self) -> bool {
        Self::is_spectator_exit_frame(self.opcode, &self.body)
    }

    pub fn is_spectator_exit_frame(opcode: u16, body: &[u8]) -> bool {
        opcode == Opcode::SpectatorExitMatchRequest.raw() && (body == [0] || body == [0; 6])
    }

    pub fn clock(time: f32) -> Self {
        ClockSyncBody { seconds: time }.encode()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_clock_sync_and_mode_var_roundtrip() {
        let clock = WorldClockBody { seconds: 42.25 };
        assert_eq!(clock.encode().body, 42.25f32.to_be_bytes());
        assert_eq!(
            WorldClockBody::read_exact(&clock.encode().body).ok(),
            Some(clock)
        );

        let sync = ClockSyncBody { seconds: 7.5 };
        assert_eq!(
            ClockSyncBody::read_exact(&sync.encode().body).ok(),
            Some(sync)
        );

        let mode_var = ModifyGameModeVarBody {
            key: 0x1234_5678,
            value: 5,
        };
        let mode_msg = mode_var.encode_padded();
        assert_eq!(mode_msg.body.len(), 14);
        assert_eq!(
            ModifyGameModeVarBody::read_padded_zero(&mode_msg.body).ok(),
            Some(mode_var)
        );
    }

    #[test]
    fn end_match_and_start_match_markers_roundtrip() {
        let end = EndMatchBody {
            team_word: 2,
            reason: EndMatchReason::ScoreOrTimeLimit,
        };
        assert_eq!(end.encode_padded().body, [0, 0, 0, 2, 3, 0]);
        assert_eq!(
            EndMatchBody::read_padded_zero(&end.encode_padded().body).ok(),
            Some(end)
        );

        let unknown_end = EndMatchBody {
            team_word: 1,
            reason: EndMatchReason::Unknown(99),
        };
        assert_eq!(
            EndMatchBody::read_padded_zero(&unknown_end.encode_padded().body).ok(),
            Some(unknown_end)
        );

        assert!(CrystalShatteredBody.encode().body.is_empty());
        assert!(StartMatchBody.encode().body.is_empty());
        assert_eq!(
            CrystalShatteredBody::read_exact(&[]).ok(),
            Some(CrystalShatteredBody)
        );
    }
}
