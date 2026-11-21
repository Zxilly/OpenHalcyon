//! Typed decoders and encoders for 5v5 draft actions (`0x045D`–`0x046B`)
//! and Ranked Ban/Pick/Swap draft state snapshots (`0x045B`).

use crate::codec::invalid;
use crate::{AndroidLayout, Message, Opcode, wire_opcode};
use binrw::{BinRead, BinWrite};
use serde::{Deserialize, Serialize};
use std::io::{self, Cursor, Read, Seek, Write};

/// Wire lane/position preference (`0x0461`), preserving the client's 1-byte wire codes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, BinRead, BinWrite)]
#[brw(big, repr = u8)]
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
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, BinRead, BinWrite)]
#[brw(big, repr = u8)]
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

#[derive(BinRead, BinWrite)]
#[brw(big)]
struct DraftWordPayload {
    word: u32,
}

#[derive(BinRead, BinWrite)]
#[brw(big)]
struct DraftHeroSkinPayload {
    id: u32,
    flags: u16,
}

#[derive(BinRead, BinWrite)]
#[brw(big)]
struct DraftBytePayload {
    value: u8,
}

#[derive(BinRead, BinWrite)]
#[brw(big)]
struct DraftPairPayload {
    first: u8,
    second: u8,
}

fn accepts_body_len(opcode: u16, actual: usize, fallback_body: usize) -> Option<usize> {
    let body_bytes = match AndroidLayout::for_opcode(opcode) {
        AndroidLayout::Fixed { frame_bytes } => frame_bytes.checked_sub(4)?,
        _ => fallback_body,
    };
    let padded_bytes = (body_bytes + 2).next_multiple_of(8) - 2;
    (actual == body_bytes || actual == padded_bytes).then_some(body_bytes)
}

fn read_prefix<T>(body: &[u8]) -> Option<T>
where
    T: for<'a> BinRead<Args<'a> = ()>,
{
    T::read_be(&mut Cursor::new(body)).ok()
}

fn encode_padded_6<T>(opcode: Opcode, payload: &T) -> Message
where
    T: for<'a> BinWrite<Args<'a> = ()>,
{
    let mut body = [0u8; 6];
    let mut writer = Cursor::new(&mut body[..]);
    payload
        .write_be(&mut writer)
        .expect("draft action fits 6-byte wire frame");
    Message::known(opcode, body)
}

impl Message {
    pub fn draft_action(&self) -> Option<DraftAction> {
        DraftAction::decode(self)
    }
}

impl DraftAction {
    pub fn decode(message: &Message) -> Option<Self> {
        Self::decode_raw(message.opcode, &message.body)
    }

    pub fn decode_raw(opcode: u16, body: &[u8]) -> Option<Self> {
        match opcode {
            wire_opcode::DraftSelectTeamOrSlot
                if accepts_body_len(opcode, body.len(), 4).is_some() =>
            {
                let payload: DraftWordPayload = read_prefix(body)?;
                Some(Self::SelectSlotOrTeam {
                    slot: (payload.word & 0xff) as u8,
                })
            }
            wire_opcode::DraftSelectHero if matches!(body.len(), 6 | 8) => {
                let payload: DraftHeroSkinPayload = read_prefix(body)?;
                Some(Self::SelectHero {
                    hero_id: payload.id,
                    flags: payload.flags,
                })
            }
            wire_opcode::DraftSelectSkin if matches!(body.len(), 4 | 6 | 8) => {
                if body.len() >= 6 {
                    let payload: DraftHeroSkinPayload = read_prefix(body)?;
                    Some(Self::SelectSkin {
                        skin_id: payload.id,
                        flags: payload.flags,
                    })
                } else {
                    let payload: DraftWordPayload = read_prefix(body)?;
                    Some(Self::SelectSkin {
                        skin_id: payload.word,
                        flags: 0,
                    })
                }
            }
            wire_opcode::DraftSelectRole if accepts_body_len(opcode, body.len(), 1).is_some() => {
                let payload: DraftBytePayload = read_prefix(body)?;
                Some(Self::SelectRole {
                    selector: payload.value,
                })
            }
            wire_opcode::DraftSelectPosition
                if accepts_body_len(opcode, body.len(), 1).is_some() =>
            {
                let payload: DraftBytePayload = read_prefix(body)?;
                let position = if payload.value == u8::MAX {
                    None
                } else {
                    Some(DraftPosition::from_wire(payload.value)?)
                };
                Some(Self::SelectPosition { position })
            }
            wire_opcode::DraftSelectBuildPath
                if accepts_body_len(opcode, body.len(), 1).is_some() =>
            {
                let payload: DraftBytePayload = read_prefix(body)?;
                let build_path = if payload.value == u8::MAX {
                    None
                } else {
                    Some(DraftBuildPath::from_wire(payload.value)?)
                };
                Some(Self::SelectBuildPath { build_path })
            }
            wire_opcode::DraftLockHero if accepts_body_len(opcode, body.len(), 1).is_some() => {
                let payload: DraftBytePayload = read_prefix(body)?;
                Some(Self::LockHero {
                    confirm: payload.value,
                })
            }
            wire_opcode::DraftSelectBan if accepts_body_len(opcode, body.len(), 4).is_some() => {
                let payload: DraftWordPayload = read_prefix(body)?;
                Some(Self::SelectBan {
                    hero_id: payload.word,
                })
            }
            wire_opcode::DraftConfirmBan if accepts_body_len(opcode, body.len(), 1).is_some() => {
                let payload: DraftBytePayload = read_prefix(body)?;
                Some(Self::ConfirmBan {
                    confirm: payload.value,
                })
            }
            wire_opcode::DraftSkipBan if accepts_body_len(opcode, body.len(), 4).is_some() => {
                let payload: DraftWordPayload = read_prefix(body)?;
                Some(Self::SkipBan {
                    flags: payload.word,
                })
            }
            wire_opcode::DraftSwapRequest if accepts_body_len(opcode, body.len(), 2).is_some() => {
                let payload: DraftPairPayload = read_prefix(body)?;
                Some(Self::SwapRequest {
                    first: payload.first,
                    second: payload.second,
                })
            }
            wire_opcode::DraftSwapResponse if accepts_body_len(opcode, body.len(), 2).is_some() => {
                let payload: DraftPairPayload = read_prefix(body)?;
                Some(Self::SwapResponse {
                    first: payload.first,
                    second: payload.second,
                })
            }
            wire_opcode::DraftSwapCancel if accepts_body_len(opcode, body.len(), 2).is_some() => {
                let payload: DraftPairPayload = read_prefix(body)?;
                Some(Self::SwapCancel {
                    first: payload.first,
                    second: payload.second,
                })
            }
            wire_opcode::DraftSwapDecline if accepts_body_len(opcode, body.len(), 2).is_some() => {
                let payload: DraftPairPayload = read_prefix(body)?;
                Some(Self::SwapDecline {
                    first: payload.first,
                    second: payload.second,
                })
            }
            wire_opcode::DraftReadyControl if accepts_body_len(opcode, body.len(), 1).is_some() => {
                let payload: DraftBytePayload = read_prefix(body)?;
                Some(Self::ReadyControl {
                    control: payload.value,
                })
            }
            _ => None,
        }
    }

    pub fn encode(self) -> Message {
        match self {
            Self::SelectSlotOrTeam { slot } => encode_padded_6(
                Opcode::DraftSelectTeamOrSlot,
                &DraftWordPayload {
                    word: u32::from(slot),
                },
            ),
            Self::SelectHero { hero_id, flags } => encode_padded_6(
                Opcode::DraftSelectHero,
                &DraftHeroSkinPayload { id: hero_id, flags },
            ),
            Self::SelectSkin { skin_id, flags } => encode_padded_6(
                Opcode::DraftSelectSkin,
                &DraftHeroSkinPayload { id: skin_id, flags },
            ),
            Self::SelectRole { selector } => encode_padded_6(
                Opcode::DraftSelectRole,
                &DraftBytePayload { value: selector },
            ),
            Self::SelectPosition { position } => encode_padded_6(
                Opcode::DraftSelectPosition,
                &DraftBytePayload {
                    value: position.map_or(u8::MAX, DraftPosition::to_wire),
                },
            ),
            Self::SelectBuildPath { build_path } => encode_padded_6(
                Opcode::DraftSelectBuildPath,
                &DraftBytePayload {
                    value: build_path.map_or(u8::MAX, DraftBuildPath::to_wire),
                },
            ),
            Self::LockHero { confirm } => {
                encode_padded_6(Opcode::DraftLockHero, &DraftBytePayload { value: confirm })
            }
            Self::SelectBan { hero_id } => {
                encode_padded_6(Opcode::DraftSelectBan, &DraftWordPayload { word: hero_id })
            }
            Self::ConfirmBan { confirm } => encode_padded_6(
                Opcode::DraftConfirmBan,
                &DraftBytePayload { value: confirm },
            ),
            Self::SkipBan { flags } => {
                encode_padded_6(Opcode::DraftSkipBan, &DraftWordPayload { word: flags })
            }
            Self::SwapRequest { first, second } => encode_padded_6(
                Opcode::DraftSwapRequest,
                &DraftPairPayload { first, second },
            ),
            Self::SwapResponse { first, second } => encode_padded_6(
                Opcode::DraftSwapResponse,
                &DraftPairPayload { first, second },
            ),
            Self::SwapCancel { first, second } => {
                encode_padded_6(Opcode::DraftSwapCancel, &DraftPairPayload { first, second })
            }
            Self::SwapDecline { first, second } => encode_padded_6(
                Opcode::DraftSwapDecline,
                &DraftPairPayload { first, second },
            ),
            Self::ReadyControl { control } => encode_padded_6(
                Opcode::DraftReadyControl,
                &DraftBytePayload { value: control },
            ),
        }
    }
}

/// Phase marker serialized inside Ranked Ban/Pick/Swap `0x045B` snapshots.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, BinRead, BinWrite)]
#[brw(big, repr = u8)]
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

#[derive(Clone, Copy, BinRead, BinWrite)]
#[brw(big)]
struct RankedDraftSeatWire {
    present: u8,
    seat_index: u8,
    selected: u8,
    locked: u8,
    reserved_4: u8,
    position: u8,
    build_path: u8,
    reserved_7: u8,
    team: u8,
    actor_wire_id: u16,
    hero_template: u16,
    reserved_13: [u8; 153],
}

impl From<RankedDraftSeat> for RankedDraftSeatWire {
    fn from(seat: RankedDraftSeat) -> Self {
        Self {
            present: u8::from(seat.present),
            seat_index: seat.seat_index,
            selected: u8::from(seat.selected),
            locked: u8::from(seat.locked),
            reserved_4: 0,
            position: seat.position,
            build_path: seat.build_path,
            reserved_7: 0,
            team: seat.team,
            actor_wire_id: seat.actor_wire_id,
            hero_template: seat.hero_template,
            reserved_13: [0; 153],
        }
    }
}

impl From<RankedDraftSeatWire> for RankedDraftSeat {
    fn from(wire: RankedDraftSeatWire) -> Self {
        Self {
            present: wire.present != 0,
            seat_index: wire.seat_index,
            selected: wire.selected != 0,
            locked: wire.locked != 0,
            position: wire.position,
            build_path: wire.build_path,
            team: wire.team,
            actor_wire_id: wire.actor_wire_id,
            hero_template: wire.hero_template,
        }
    }
}

impl BinRead for RankedDraftSeat {
    type Args<'a> = ();

    fn read_options<R: Read + Seek>(
        reader: &mut R,
        endian: binrw::Endian,
        _args: Self::Args<'_>,
    ) -> binrw::BinResult<Self> {
        RankedDraftSeatWire::read_options(reader, endian, ()).map(Into::into)
    }
}

impl BinWrite for RankedDraftSeat {
    type Args<'a> = ();

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        endian: binrw::Endian,
        _args: Self::Args<'_>,
    ) -> binrw::BinResult<()> {
        RankedDraftSeatWire::from(*self).write_options(writer, endian, ())
    }
}

#[derive(BinRead, BinWrite)]
#[brw(big)]
struct RankedDraftStateWire {
    reserved_0: u32,
    phase_word: u32,
    active_turn_word: u32,
    reserved_12: [u32; 3],
    bans: [u32; 4],
    reserved_40: [u8; 224],
    seats: [RankedDraftSeatWire; 10],
    reserved_1924: [u8; 200],
    timer_remaining_seconds: f32,
    timer_total_seconds: f32,
    reserved_2132: [u8; 2],
}

/// Authoritative Ranked Ban/Pick/Swap snapshot encoded to `0x045B` (`DraftRosterRanked`, 2134B).
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

impl BinRead for RankedDraftStatePacket {
    type Args<'a> = ();

    fn read_options<R: Read + Seek>(
        reader: &mut R,
        endian: binrw::Endian,
        _args: Self::Args<'_>,
    ) -> binrw::BinResult<Self> {
        let pos = reader.stream_position()?;
        let wire = RankedDraftStateWire::read_options(reader, endian, ())?;
        let phase =
            RankedWirePhase::from_wire((wire.phase_word & 0xff) as u8).ok_or_else(|| {
                binrw::Error::AssertFail {
                    pos,
                    message: "invalid ranked draft phase".into(),
                }
            })?;
        Ok(Self {
            phase,
            active_turn: (wire.active_turn_word & 0xff) as u8,
            bans: wire.bans,
            seats: wire.seats.map(Into::into),
            timer_remaining_seconds: wire.timer_remaining_seconds,
            timer_total_seconds: wire.timer_total_seconds,
        })
    }
}

impl BinWrite for RankedDraftStatePacket {
    type Args<'a> = ();

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        endian: binrw::Endian,
        _args: Self::Args<'_>,
    ) -> binrw::BinResult<()> {
        RankedDraftStateWire {
            reserved_0: 0,
            phase_word: u32::from(self.phase as u8),
            active_turn_word: u32::from(self.active_turn),
            reserved_12: [0; 3],
            bans: self.bans,
            reserved_40: [0; 224],
            seats: self.seats.map(Into::into),
            reserved_1924: [0; 200],
            timer_remaining_seconds: self.timer_remaining_seconds,
            timer_total_seconds: self.timer_total_seconds,
            reserved_2132: [0; 2],
        }
        .write_options(writer, endian, ())
    }
}

impl RankedDraftStatePacket {
    pub const BODY_LEN: usize = 2134;

    pub fn encode(&self) -> Message {
        let mut body = vec![0u8; Self::BODY_LEN];
        let mut writer = Cursor::new(&mut body[..]);
        self.write_be(&mut writer)
            .expect("ranked draft state fits 2134-byte buffer");
        debug_assert_eq!(writer.position(), Self::BODY_LEN as u64);
        Message::known(Opcode::DraftRosterRanked, body)
    }

    pub fn decode(body: &[u8]) -> io::Result<Self> {
        if body.len() != Self::BODY_LEN {
            return Err(invalid("invalid 0x045B ranked draft length"));
        }
        Self::read_be(&mut Cursor::new(body)).map_err(|_| invalid("invalid ranked draft phase"))
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
        assert_eq!(msg.opcode, wire_opcode::DraftRosterRanked);
        assert_eq!(msg.body.len(), RankedDraftStatePacket::BODY_LEN);
        assert_eq!(&msg.body[4..8], &2u32.to_be_bytes());
        assert_eq!(&msg.body[8..12], &2u32.to_be_bytes());
        assert_eq!(&msg.body[24..28], &110u32.to_be_bytes());
        assert_eq!(&msg.body[264 + 9..264 + 11], &1500u16.to_be_bytes());
        assert_eq!(&msg.body[264 + 11..264 + 13], &100u16.to_be_bytes());
        assert_eq!(&msg.body[2124..2128], &24.5f32.to_be_bytes());
        assert_eq!(&msg.body[2128..2132], &30.0f32.to_be_bytes());
        assert!(StaticActionLayout::for_opcode(msg.opcode).is_some());
        let decoded = RankedDraftStatePacket::decode(&msg.body).unwrap();
        assert_eq!(decoded, packet);
    }
}
