//! Android packers for tags ignored by its inbound dispatcher.
use crate::{Fields, Message};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutboundLayout {
    opcode: u16,
    body_bytes: Option<usize>,
    /// A separate Windows formatter has the same plain body length.
    pub windows_width: bool,
    pub action_name: Option<&'static str>,
}

impl OutboundLayout {
    pub fn for_opcode(opcode: u16) -> Option<Self> {
        let index = OUTBOUND_LAYOUTS
            .binary_search_by_key(&opcode, |row| row.opcode)
            .ok()?;
        Some(OUTBOUND_LAYOUTS[index])
    }

    pub fn opcode(self) -> u16 {
        self.opcode
    }

    pub fn body_bytes(self) -> Option<usize> {
        self.body_bytes
    }

    pub fn read(self, body: &[u8]) -> Option<OutboundAction<'_>> {
        let meaningful = self.body_bytes?;
        let padded = (meaningful + 2).next_multiple_of(8) - 2;
        if body.len() != meaningful && body.len() != padded {
            return None;
        }
        let (payload, padding) = body.split_at(meaningful);
        let word = |offset: usize| {
            let bytes = &payload[offset..offset + 4];
            u32::from_be_bytes(bytes.try_into().unwrap())
        };
        let operands = match self.opcode {
            0x03f6 => OutboundOperands::MoveAlternate {
                coordinate_bits: [word(0), word(4)],
            },
            0x0413 => OutboundOperands::AbilityAlternate {
                words: [word(0), word(4), word(8), word(12)],
                byte: payload[16],
            },
            0x0449 => OutboundOperands::Words([word(0), word(4)]),
            0x045d | 0x0464 | 0x0486 => OutboundOperands::Word(word(0)),
            0x0455 | 0x0465 | 0x0474 | 0x047b => OutboundOperands::Byte(payload[0]),
            0x0467 | 0x0468 => OutboundOperands::Bytes([payload[0], payload[1]]),
            _ => OutboundOperands::Opaque,
        };
        Some(OutboundAction {
            layout: self,
            operands,
            payload,
            padding,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutboundOperands {
    MoveAlternate {
        coordinate_bits: [u32; 2],
    },
    AbilityAlternate {
        words: [u32; 4],
        byte: u8,
    },
    Word(u32),
    Words([u32; 2]),
    Byte(u8),
    Bytes([u8; 2]),
    /// The width is known, but no cross-checked operand interpretation is used.
    Opaque,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutboundAction<'a> {
    pub layout: OutboundLayout,
    pub operands: OutboundOperands,
    payload: &'a [u8],
    padding: &'a [u8],
}

impl<'a> OutboundAction<'a> {
    pub fn payload(self) -> &'a [u8] {
        self.payload
    }
    pub fn padding(self) -> &'a [u8] {
        self.padding
    }

    pub fn to_message(self) -> Message {
        Message::new(self.layout.opcode, [self.payload, self.padding].concat())
    }
}

pub(super) fn fields(opcode: u16, body: &[u8]) -> Option<Fields<'_>> {
    let action = OutboundLayout::for_opcode(opcode)?.read(body)?;
    (action.operands != OutboundOperands::Opaque).then_some(Fields::Outbound(action))
}

const fn entry(
    opcode: u16,
    body_bytes: Option<usize>,
    windows_width: bool,
    action_name: Option<&'static str>,
) -> OutboundLayout {
    OutboundLayout {
        opcode,
        body_bytes,
        windows_width,
        action_name,
    }
}

/// Receiver-ignore entries and independently checked writer metadata.
pub const OUTBOUND_LAYOUTS: &[OutboundLayout] = &[
    entry(0x03f4, Some(8), true, Some("ActionRequestMoveTo_Client")),
    entry(0x03f5, Some(8), true, Some("ActionRequestMoveTo_Client")),
    entry(0x03f6, Some(8), true, Some("ActionRequestMoveTo_Client")),
    entry(0x03f7, Some(1), false, None),
    entry(0x040c, None, false, None),
    entry(0x0411, Some(5), true, Some("ActionRequestActivateAbility")),
    entry(0x0412, Some(13), true, Some("ActionRequestActivateAbility")),
    entry(0x0413, Some(17), true, Some("ActionRequestActivateAbility")),
    entry(
        0x0414,
        Some(2),
        true,
        Some("ActionRequestCancelAbility_Client"),
    ),
    entry(
        0x0424,
        Some(5),
        true,
        Some("ActionRequestModifyBasicAttackTarget"),
    ),
    entry(0x0426, None, false, None),
    entry(0x042a, None, false, None),
    entry(0x042d, None, false, None),
    entry(
        0x0436,
        Some(1),
        true,
        Some("ActionRequestUpgradeAbility_Client"),
    ),
    entry(0x0438, Some(6), true, Some("ActionReorderItem")),
    entry(0x0448, Some(4), true, Some("ActionRequestActivateItem")),
    entry(0x0449, Some(8), true, Some("ActionRequestActivateItem")),
    entry(0x044a, Some(16), true, Some("ActionRequestActivateItem")),
    entry(0x0450, Some(1), false, None),
    entry(0x0455, Some(1), true, None),
    entry(0x0456, Some(1), true, None),
    entry(0x0457, Some(1), false, None),
    entry(0x0458, Some(1), true, None),
    entry(0x045d, Some(4), true, None),
    entry(0x045e, Some(8), true, None),
    entry(0x045f, Some(4), true, None),
    entry(0x0460, Some(1), false, None),
    entry(0x0461, Some(1), true, None),
    entry(0x0462, Some(1), true, None),
    entry(0x0463, Some(1), true, None),
    entry(0x0464, Some(4), true, None),
    entry(0x0465, Some(1), true, None),
    entry(0x0466, Some(4), true, None),
    entry(0x0467, Some(2), true, None),
    entry(0x0468, Some(2), true, None),
    entry(0x0469, Some(2), false, None),
    entry(0x046a, Some(2), false, None),
    entry(0x046b, Some(1), true, None),
    entry(0x046d, Some(1), true, None),
    entry(0x046e, Some(1), true, None),
    entry(
        0x0472,
        Some(1),
        true,
        Some("ActionSetSurrenderStateRequest"),
    ),
    entry(
        0x0474,
        Some(1),
        true,
        Some("ActionSpectatorExitMatchRequest"),
    ),
    entry(0x047a, None, false, None),
    entry(0x047b, Some(1), true, None),
    entry(0x0484, Some(4), true, Some("ActionRequestEquipTalent")),
    entry(
        0x0485,
        Some(1),
        true,
        Some("ActionRequestRecommendedBuildPath"),
    ),
    entry(
        0x0486,
        Some(4),
        true,
        Some("ActionRequestLevelLogicEvent_Client"),
    ),
    entry(0x048e, Some(8), false, None),
];
