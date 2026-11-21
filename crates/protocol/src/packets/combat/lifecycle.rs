//! Actor lifecycle, respawn, level-up, visibility, and camp minimap packets.

use crate::codec::fixed_body::{fixed_packet, fixed_prefix};
use crate::{Message, Opcode};
use binrw::{BinRead, BinWrite};

#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct VisibilityBody {
    pub actor: u32,
    pub index: u8,
    pub state: u8,
    pub masks: [u8; 2],
}
fixed_prefix!(VisibilityBody, 8);

/// Observed `0x042C` camp minimap state byte (`(populated << 1) | visible`).
///
/// Android 147219 (`0x82aee0`) extracts only the low 2 bits via
/// `bfxil w3, w21, #0, #2`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CampVisibilityState {
    /// Camp has no living monsters (countdown or cleared) and is hidden from this index (`0`).
    EmptyHidden,
    /// Camp has no living monsters (countdown or cleared) and is visible to this index (`1`).
    EmptyVisible,
    /// Camp has living monsters and is hidden in fog from this index (`2`).
    PopulatedHidden,
    /// Camp has living monsters and is visible to this index (`3`).
    PopulatedVisible,
    /// Unobserved state byte preserved losslessly.
    Unknown(u8),
}

impl CampVisibilityState {
    pub const fn from_wire(raw: u8) -> Self {
        match raw {
            0 => Self::EmptyHidden,
            1 => Self::EmptyVisible,
            2 => Self::PopulatedHidden,
            3 => Self::PopulatedVisible,
            other => Self::Unknown(other),
        }
    }

    pub const fn raw(self) -> u8 {
        match self {
            Self::EmptyHidden => 0,
            Self::EmptyVisible => 1,
            Self::PopulatedHidden => 2,
            Self::PopulatedVisible => 3,
            Self::Unknown(other) => other,
        }
    }
}

impl From<u8> for CampVisibilityState {
    fn from(raw: u8) -> Self {
        Self::from_wire(raw)
    }
}

impl From<CampVisibilityState> for u8 {
    fn from(state: CampVisibilityState) -> Self {
        state.raw()
    }
}

/// Six-byte body of `0x042C` (`Opcode::CampVisibility`).
///
/// Android 147219 (`vt[3]` at `0x8844dc`) filters strictly on
/// `index == GetLocalTeamId()` (`0xbedf10`, `[local_player + 0xc2] & 0xf`) before
/// storing `state` into `[minimap_entry + 0x6c]` via `0x820a7c`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct CampVisibilityBody {
    pub actor: u32,
    pub index: u8,
    #[br(map = CampVisibilityState::from_wire)]
    #[bw(map = |state: &CampVisibilityState| state.raw())]
    pub state: CampVisibilityState,
}
fixed_prefix!(CampVisibilityBody, 6);

/// `0x040b DestroyEntity`: 4-byte entity removal packet (often padded to 6 bytes on the wire).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct DestroyEntityBody {
    /// Wire actor identifier to destroy.
    pub actor: u32,
}
fixed_packet!(DestroyEntityBody, Opcode::DestroyEntity, 4);

/// `0x042f ActorUndead`: 4-byte actor undeath/survival transition packet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct ActorUndeadBody {
    /// Wire actor identifier.
    pub actor: u32,
}
fixed_packet!(ActorUndeadBody, Opcode::ActorUndead, 4);

/// `0x0431 ActorDead`: 4-byte actor death notification (often padded to 6 bytes on the wire).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct ActorDeadBody {
    /// Wire actor identifier that died.
    pub actor: u32,
}
fixed_packet!(ActorDeadBody, Opcode::ActorDead, 4);

/// `0x0432 ActorRespawn`: 16-byte actor respawn packet (often padded to 22 bytes on the wire).
#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct ActorRespawnBody {
    /// Wire actor identifier respawning.
    pub actor: u32,
    /// Respawn world coordinates `[x, y, z]`.
    pub position: [f32; 3],
}
fixed_packet!(ActorRespawnBody, Opcode::ActorRespawn, 16);

/// `0x0433 ActorRespawnTimer`: 8-byte actor respawn countdown packet.
#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct RespawnTimerBody {
    /// Wire actor identifier.
    pub actor: u32,
    /// Remaining respawn duration in seconds.
    pub seconds: f32,
}
fixed_packet!(RespawnTimerBody, Opcode::ActorRespawnTimer, 8);

/// `0x0434 LevelUp`: 8-byte actor level-up packet (often padded to 14 bytes on the wire).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct LevelUpBody {
    /// Wire actor identifier.
    pub actor: u32,
    /// Level/points operand.
    pub operand: u32,
}
fixed_packet!(LevelUpBody, Opcode::LevelUp, 8);

impl Message {
    pub fn actor_visibility(actor: u32, index: u8, state: u8, masks: [u8; 2]) -> Self {
        VisibilityBody {
            actor,
            index,
            state,
            masks,
        }
        .message(Opcode::ModifyVisibility, 14)
    }

    pub fn camp_visibility(actor: u32, index: u8, state: CampVisibilityState) -> Self {
        CampVisibilityBody {
            actor,
            index,
            state,
        }
        .message(Opcode::CampVisibility, CampVisibilityBody::BODY_LEN)
    }

    pub fn actor_respawn_timer(actor: u32, seconds: f32) -> Self {
        RespawnTimerBody { actor, seconds }.encode_padded()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_golden_layouts() {
        let dead = ActorDeadBody { actor: 1504 };
        let padded_dead = dead.encode_padded();
        assert_eq!(padded_dead.body, [0, 0, 5, 224, 0, 0]);
        assert_eq!(
            ActorDeadBody::read_padded_zero(&padded_dead.body).ok(),
            Some(dead)
        );

        let destroy = DestroyEntityBody { actor: 2001 };
        let padded_destroy = destroy.encode_padded();
        assert_eq!(
            DestroyEntityBody::read_with_padding(&padded_destroy.body).ok(),
            Some((destroy, &[0u8, 0][..]))
        );

        let undead = ActorUndeadBody { actor: 1501 };
        assert_eq!(undead.encode().body, 1501u32.to_be_bytes());

        let respawn = ActorRespawnBody {
            actor: 1500,
            position: [10.0, 0.0, -5.0],
        };
        let respawn_padded = respawn.encode_padded();
        assert_eq!(respawn_padded.body.len(), 22);
        assert_eq!(
            ActorRespawnBody::read_padded_zero(&respawn_padded.body).ok(),
            Some(respawn)
        );

        let timer = RespawnTimerBody {
            actor: 1500,
            seconds: 12.5,
        };
        assert_eq!(
            RespawnTimerBody::read_exact(&timer.encode().body).ok(),
            Some(timer)
        );

        let level = LevelUpBody {
            actor: 1500,
            operand: 4,
        };
        let level_padded = level.encode_padded();
        assert_eq!(level_padded.body.len(), 14);
        assert_eq!(
            LevelUpBody::read_padded_zero(&level_padded.body).ok(),
            Some(level)
        );
    }
}
