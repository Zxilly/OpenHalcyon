//! Typed decoders and encoders for 5v5 draft actions (`0x045D`–`0x046A`, `0x046B`)
//! and Ranked Ban/Pick/Swap draft state snapshots (`0x045B`).

use crate::{AndroidLayout, Message, Opcode, wire_opcode};
use serde::{Deserialize, Serialize};
use std::io;

/// Wire lane/position preference (`0x0461`), preserving the client's 1-byte wire codes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum DraftPosition {
    Top = 0,
    Jungle = 1,
    Mid = 2,
    Bot = 3,
    Carry = 4,
    Captain = 5,
}

impl DraftPosition {
    pub const fn from_wire(raw: u8) -> Option<Self> {
        match raw {
            0 => Some(Self::Top),
            1 => Some(Self::Jungle),
            2 => Some(Self::Mid),
            3 => Some(Self::Bot),
            4 => Some(Self::Carry),
            5 => Some(Self::Captain),
            _ => None,
        }
    }

    pub const fn to_wire(self) -> u8 {
        self as u8
    }
}

/// Wire build-path preference (`0x0462`: Weapon / Crystal / Utility).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum DraftBuildPath {
    Weapon = 0,
    Crystal = 1,
    Utility = 2,
}

impl DraftBuildPath {
    pub const fn from_wire(raw: u8) -> Option<Self> {
        match raw {
            0 => Some(Self::Weapon),
            1 => Some(Self::Crystal),
            2 => Some(Self::Utility),
            _ => None,
        }
    }

    pub const fn to_wire(self) -> u8 {
        self as u8
    }
}

/// Closed vocabulary for C->S / echoed draft protocol messages (`0x045D`–`0x046B`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum DraftAction {
    /// `0x045D`: select/cycle team or roster slot (`u32 BE`, low byte carries 1-based slot/team).
    SelectSlotOrTeam { slot: u8 },
    /// `0x045E`: hover/select hero template (`u32 BE hero_id`, `u16 BE flags`).
    SelectHero { hero_id: u32, flags: u16 },
    /// `0x045F`: select skin hash (`u32 BE skin_id`, `u16 BE flags`).
    SelectSkin { skin_id: u32, flags: u16 },
    /// `0x0460`: select role/selector byte (`u8`).
    SelectRole { selector: u8 },
    /// `0x0461`: select lane/position (`None` for `0xFF` clear).
    SelectPosition { position: Option<DraftPosition> },
    /// `0x0462`: select build path (`None` for `0xFF` clear).
    SelectBuildPath { build_path: Option<DraftBuildPath> },
    /// `0x0463`: lock currently hovered hero pick (`u8`).
    LockHero { confirm: u8 },
    /// `0x0464`: hover/select hero template to ban (`u32 BE hero_id`, `0` clears).
    SelectBan { hero_id: u32 },
    /// `0x0465`: confirm/lock active ban (`u8`).
    ConfirmBan { confirm: u8 },
    /// `0x0466`: skip active ban turn (`u32 BE flags`).
    SkipBan { flags: u32 },
    /// `0x0467`: propose hero swap (`[requester_seat, target_seat]`).
    SwapRequest { first: u8, second: u8 },
    /// `0x0468`: respond to pending hero swap (`[actor_seat, partner_seat]`).
    SwapResponse { first: u8, second: u8 },
    /// `0x0469`: cancel or secondary swap control (`[first, second]`).
    SwapCancel { first: u8, second: u8 },
    /// `0x046A`: decline or auxiliary draft option (`[first, second]`).
    SwapDecline { first: u8, second: u8 },
    /// `0x046B`: draft ready / phase control byte (`u8`).
    ReadyControl { control: u8 },
}

fn accepts_body_len(opcode: u16, actual: usize, fallback_body: usize) -> Option<usize> {
    let body_bytes = match AndroidLayout::for_opcode(opcode) {
        AndroidLayout::Fixed { frame_bytes } => frame_bytes.checked_sub(4)?,
        _ => fallback_body,
    };
    let padded_bytes = (body_bytes + 2).next_multiple_of(8) - 2;
    (actual == body_bytes || actual == padded_bytes).then_some(body_bytes)
}

impl DraftAction {
    pub fn decode(message: &Message) -> Option<Self> {
        Self::decode_raw(message.opcode, &message.body)
    }

    pub fn decode_raw(opcode: u16, body: &[u8]) -> Option<Self> {
        let be32 = |off: usize| u32::from_be_bytes(body[off..off + 4].try_into().unwrap());
        let be16 = |off: usize| u16::from_be_bytes(body[off..off + 2].try_into().unwrap());
        match opcode {
            wire_opcode::Unresolved045D if accepts_body_len(opcode, body.len(), 4).is_some() => {
                let word = be32(0);
                Some(Self::SelectSlotOrTeam {
                    slot: (word & 0xff) as u8,
                })
            }
            wire_opcode::DraftSelectHero if matches!(body.len(), 6 | 8) => Some(Self::SelectHero {
                hero_id: be32(0),
                flags: be16(4),
            }),
            wire_opcode::DraftSelectSkin if matches!(body.len(), 4 | 6 | 8) => {
                Some(Self::SelectSkin {
                    skin_id: be32(0),
                    flags: if body.len() >= 6 { be16(4) } else { 0 },
                })
            }
            wire_opcode::Unresolved0460 if accepts_body_len(opcode, body.len(), 1).is_some() => {
                Some(Self::SelectRole { selector: body[0] })
            }
            wire_opcode::DraftSelectPosition
                if accepts_body_len(opcode, body.len(), 1).is_some() =>
            {
                let position = if body[0] == u8::MAX {
                    None
                } else {
                    Some(DraftPosition::from_wire(body[0])?)
                };
                Some(Self::SelectPosition { position })
            }
            wire_opcode::DraftSelectBuildPath
                if accepts_body_len(opcode, body.len(), 1).is_some() =>
            {
                let build_path = if body[0] == u8::MAX {
                    None
                } else {
                    Some(DraftBuildPath::from_wire(body[0])?)
                };
                Some(Self::SelectBuildPath { build_path })
            }
            wire_opcode::DraftLockHero if accepts_body_len(opcode, body.len(), 1).is_some() => {
                Some(Self::LockHero { confirm: body[0] })
            }
            wire_opcode::Unresolved0464 if accepts_body_len(opcode, body.len(), 4).is_some() => {
                Some(Self::SelectBan { hero_id: be32(0) })
            }
            wire_opcode::Unresolved0465 if accepts_body_len(opcode, body.len(), 1).is_some() => {
                Some(Self::ConfirmBan { confirm: body[0] })
            }
            wire_opcode::Unresolved0466 if accepts_body_len(opcode, body.len(), 4).is_some() => {
                Some(Self::SkipBan { flags: be32(0) })
            }
            wire_opcode::Unresolved0467 if accepts_body_len(opcode, body.len(), 2).is_some() => {
                Some(Self::SwapRequest {
                    first: body[0],
                    second: body[1],
                })
            }
            wire_opcode::Unresolved0468 if accepts_body_len(opcode, body.len(), 2).is_some() => {
                Some(Self::SwapResponse {
                    first: body[0],
                    second: body[1],
                })
            }
            wire_opcode::Unresolved0469 if accepts_body_len(opcode, body.len(), 2).is_some() => {
                Some(Self::SwapCancel {
                    first: body[0],
                    second: body[1],
                })
            }
            wire_opcode::Unresolved046A if accepts_body_len(opcode, body.len(), 2).is_some() => {
                Some(Self::SwapDecline {
                    first: body[0],
                    second: body[1],
                })
            }
            wire_opcode::DraftReadyControl if accepts_body_len(opcode, body.len(), 1).is_some() => {
                Some(Self::ReadyControl { control: body[0] })
            }
            _ => None,
        }
    }

    pub fn encode(self) -> Message {
        let padded_one = |opcode: Opcode, v: u8| {
            let mut b = [0u8; 6];
            b[0] = v;
            Message::known(opcode, b)
        };
        match self {
            Self::SelectSlotOrTeam { slot } => {
                let mut b = [0u8; 6];
                b[..4].copy_from_slice(&u32::from(slot).to_be_bytes());
                Message::known(Opcode::Unresolved045D, b)
            }
            Self::SelectHero { hero_id, flags } => {
                let mut b = [0u8; 6];
                b[..4].copy_from_slice(&hero_id.to_be_bytes());
                b[4..6].copy_from_slice(&flags.to_be_bytes());
                Message::known(Opcode::DraftSelectHero, b)
            }
            Self::SelectSkin { skin_id, flags } => {
                let mut b = [0u8; 6];
                b[..4].copy_from_slice(&skin_id.to_be_bytes());
                b[4..6].copy_from_slice(&flags.to_be_bytes());
                Message::known(Opcode::DraftSelectSkin, b)
            }
            Self::SelectRole { selector } => padded_one(Opcode::Unresolved0460, selector),
            Self::SelectPosition { position } => padded_one(
                Opcode::DraftSelectPosition,
                position.map_or(u8::MAX, DraftPosition::to_wire),
            ),
            Self::SelectBuildPath { build_path } => padded_one(
                Opcode::DraftSelectBuildPath,
                build_path.map_or(u8::MAX, DraftBuildPath::to_wire),
            ),
            Self::LockHero { confirm } => padded_one(Opcode::DraftLockHero, confirm),
            Self::SelectBan { hero_id } => {
                let mut b = [0u8; 6];
                b[..4].copy_from_slice(&hero_id.to_be_bytes());
                Message::known(Opcode::Unresolved0464, b)
            }
            Self::ConfirmBan { confirm } => padded_one(Opcode::Unresolved0465, confirm),
            Self::SkipBan { flags } => {
                let mut b = [0u8; 6];
                b[..4].copy_from_slice(&flags.to_be_bytes());
                Message::known(Opcode::Unresolved0466, b)
            }
            Self::SwapRequest { first, second } => {
                let mut b = [0u8; 6];
                b[0] = first;
                b[1] = second;
                Message::known(Opcode::Unresolved0467, b)
            }
            Self::SwapResponse { first, second } => {
                let mut b = [0u8; 6];
                b[0] = first;
                b[1] = second;
                Message::known(Opcode::Unresolved0468, b)
            }
            Self::SwapCancel { first, second } => {
                let mut b = [0u8; 6];
                b[0] = first;
                b[1] = second;
                Message::known(Opcode::Unresolved0469, b)
            }
            Self::SwapDecline { first, second } => {
                let mut b = [0u8; 6];
                b[0] = first;
                b[1] = second;
                Message::known(Opcode::Unresolved046A, b)
            }
            Self::ReadyControl { control } => padded_one(Opcode::DraftReadyControl, control),
        }
    }
}

/// Phase marker serialized inside Ranked Ban/Pick/Swap `0x045B` snapshots.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[repr(u8)]
pub enum RankedWirePhase {
    Ban = 1,
    Pick = 2,
    Swap = 3,
    Complete = 4,
}

impl RankedWirePhase {
    pub const fn from_wire(raw: u8) -> Option<Self> {
        match raw {
            1 => Some(Self::Ban),
            2 => Some(Self::Pick),
            3 => Some(Self::Swap),
            4 => Some(Self::Complete),
            _ => None,
        }
    }
}

/// Per-seat row inside the 2134-byte Ranked draft snapshot (`0x045B`, 166B stride from `+264`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RankedDraftSeat {
    pub present: bool,
    pub seat_index: u8,
    pub selected: bool,
    pub locked: bool,
    pub position: u8,
    pub build_path: u8,
    pub team: u8,
    pub actor_wire_id: u16,
    pub hero_template: u16,
}

impl Default for RankedDraftSeat {
    fn default() -> Self {
        Self {
            present: false,
            seat_index: u8::MAX,
            selected: false,
            locked: false,
            position: u8::MAX,
            build_path: u8::MAX,
            team: 0,
            actor_wire_id: 0xffff,
            hero_template: 0xffff,
        }
    }
}

/// Authoritative Ranked Ban/Pick/Swap snapshot encoded to `0x045B` (`Unresolved045B`, 2134B).
///
/// Layout matches `sub_82D514` (`+4..+20` four BE32 words, `+24..+264` sixty BE32 words,
/// ten 166-byte seat rows at `+264..+1924` with paired BE16 fields at row `+9` and `+11`,
/// and two BE32 timers at `+2124` and `+2128`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RankedDraftStatePacket {
    pub phase: RankedWirePhase,
    pub active_turn: u8,
    pub bans: [u32; 4],
    pub seats: [RankedDraftSeat; 10],
    pub timer_remaining_seconds: f32,
    pub timer_total_seconds: f32,
}

impl RankedDraftStatePacket {
    pub const BODY_LEN: usize = 2134;
    const SEAT_BASE: usize = 264;
    const SEAT_STRIDE: usize = 166;

    pub fn encode(&self) -> Message {
        let mut body = vec![0u8; Self::BODY_LEN];
        body[4..8].copy_from_slice(&u32::from(self.phase as u8).to_be_bytes());
        body[8..12].copy_from_slice(&u32::from(self.active_turn).to_be_bytes());
        for (i, &ban) in self.bans.iter().enumerate() {
            let off = 24 + i * 4;
            body[off..off + 4].copy_from_slice(&ban.to_be_bytes());
        }
        for (i, seat) in self.seats.iter().enumerate() {
            let base = Self::SEAT_BASE + i * Self::SEAT_STRIDE;
            body[base] = u8::from(seat.present);
            body[base + 1] = seat.seat_index;
            body[base + 2] = u8::from(seat.selected);
            body[base + 3] = u8::from(seat.locked);
            body[base + 5] = seat.position;
            body[base + 6] = seat.build_path;
            body[base + 8] = seat.team;
            body[base + 9..base + 11].copy_from_slice(&seat.actor_wire_id.to_be_bytes());
            body[base + 11..base + 13].copy_from_slice(&seat.hero_template.to_be_bytes());
        }
        body[2124..2128].copy_from_slice(&self.timer_remaining_seconds.to_be_bytes());
        body[2128..2132].copy_from_slice(&self.timer_total_seconds.to_be_bytes());
        Message::known(Opcode::Unresolved045B, body)
    }

    pub fn decode(body: &[u8]) -> io::Result<Self> {
        if body.len() != Self::BODY_LEN {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid 0x045B ranked draft length",
            ));
        }
        let word = |off: usize| u32::from_be_bytes(body[off..off + 4].try_into().unwrap());
        let half = |off: usize| u16::from_be_bytes(body[off..off + 2].try_into().unwrap());
        let phase = RankedWirePhase::from_wire((word(4) & 0xff) as u8).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "invalid ranked draft phase")
        })?;
        let active_turn = (word(8) & 0xff) as u8;
        let bans = std::array::from_fn(|i| word(24 + i * 4));
        let seats = std::array::from_fn(|i| {
            let base = Self::SEAT_BASE + i * Self::SEAT_STRIDE;
            RankedDraftSeat {
                present: body[base] != 0,
                seat_index: body[base + 1],
                selected: body[base + 2] != 0,
                locked: body[base + 3] != 0,
                position: body[base + 5],
                build_path: body[base + 6],
                team: body[base + 8],
                actor_wire_id: half(base + 9),
                hero_template: half(base + 11),
            }
        });
        let timer_remaining_seconds = f32::from_bits(word(2124));
        let timer_total_seconds = f32::from_bits(word(2128));
        Ok(Self {
            phase,
            active_turn,
            bans,
            seats,
            timer_remaining_seconds,
            timer_total_seconds,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StaticActionLayout;

    #[test]
    fn draft_actions_roundtrip_all_opcodes_045d_through_046b() {
        let cases = [
            DraftAction::SelectSlotOrTeam { slot: 7 },
            DraftAction::SelectHero {
                hero_id: 141,
                flags: 1,
            },
            DraftAction::SelectSkin {
                skin_id: 0x12345678,
                flags: 2,
            },
            DraftAction::SelectRole { selector: 3 },
            DraftAction::SelectPosition {
                position: Some(DraftPosition::Mid),
            },
            DraftAction::SelectPosition { position: None },
            DraftAction::SelectBuildPath {
                build_path: Some(DraftBuildPath::Crystal),
            },
            DraftAction::SelectBuildPath { build_path: None },
            DraftAction::LockHero { confirm: 1 },
            DraftAction::SelectBan { hero_id: 235 },
            DraftAction::ConfirmBan { confirm: 1 },
            DraftAction::SkipBan { flags: 1 },
            DraftAction::SwapRequest {
                first: 0,
                second: 3,
            },
            DraftAction::SwapResponse {
                first: 3,
                second: 0,
            },
            DraftAction::SwapCancel {
                first: 0,
                second: 3,
            },
            DraftAction::SwapDecline {
                first: 3,
                second: 0,
            },
            DraftAction::ReadyControl { control: 1 },
        ];
        for action in cases {
            let msg = action.encode();
            let decoded = DraftAction::decode(&msg).unwrap_or_else(|| {
                panic!(
                    "failed to decode opcode {:#06x} body {:?}",
                    msg.opcode, msg.body
                )
            });
            assert_eq!(decoded, action);
        }
        assert!(
            DraftAction::decode_raw(wire_opcode::DraftSelectPosition, &[9, 0, 0, 0, 0, 0])
                .is_none()
        );
        assert!(
            DraftAction::decode_raw(wire_opcode::DraftSelectBuildPath, &[9, 0, 0, 0, 0, 0])
                .is_none()
        );
    }

    #[test]
    fn ranked_draft_state_045b_matches_static_layout_and_roundtrips() {
        let mut seats = [RankedDraftSeat::default(); 10];
        for (i, seat) in seats.iter_mut().enumerate() {
            seat.present = true;
            seat.seat_index = i as u8;
            seat.selected = i < 4;
            seat.locked = i < 2;
            seat.position = (i % 5) as u8;
            seat.build_path = (i % 3) as u8;
            seat.team = u8::from(i >= 5) + 1;
            seat.actor_wire_id = 1500 + i as u16;
            seat.hero_template = if i < 4 { 100 + i as u16 } else { u16::MAX };
        }
        let packet = RankedDraftStatePacket {
            phase: RankedWirePhase::Pick,
            active_turn: 2,
            bans: [110, 141, 200, 0],
            seats,
            timer_remaining_seconds: 24.5,
            timer_total_seconds: 30.0,
        };
        let msg = packet.encode();
        assert_eq!(msg.opcode, wire_opcode::Unresolved045B);
        assert_eq!(msg.body.len(), RankedDraftStatePacket::BODY_LEN);
        assert!(StaticActionLayout::for_opcode(msg.opcode).is_some());
        let decoded = RankedDraftStatePacket::decode(&msg.body).unwrap();
        assert_eq!(decoded, packet);
    }
}
