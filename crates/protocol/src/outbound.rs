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
        let operands = match crate::Opcode::try_from(self.opcode).ok() {
            Some(crate::Opcode::Unseen03F6) => OutboundOperands::MoveAlternate {
                coordinate_bits: [word(0), word(4)],
            },
            Some(crate::Opcode::RequestActivateAbilityUnseen) => {
                OutboundOperands::AbilityAlternate {
                    words: [word(0), word(4), word(8), word(12)],
                    byte: payload[16],
                }
            }
            Some(crate::Opcode::RequestActivateItemTarget) => {
                OutboundOperands::Words([word(0), word(4)])
            }
            Some(
                crate::Opcode::DraftSelectTeamOrSlot
                | crate::Opcode::DraftSelectBan
                | crate::Opcode::RequestLevelLogicEvent,
            ) => OutboundOperands::Word(word(0)),
            Some(
                crate::Opcode::DraftReadyAccept
                | crate::Opcode::DraftConfirmBan
                | crate::Opcode::SpectatorExitMatchRequest
                | crate::Opcode::Unresolved047B,
            ) => OutboundOperands::Byte(payload[0]),
            Some(crate::Opcode::DraftSwapRequest | crate::Opcode::DraftSwapResponse) => {
                OutboundOperands::Bytes([payload[0], payload[1]])
            }
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
    entry(
        crate::wire_opcode::RequestMoveJoystick,
        Some(8),
        true,
        Some("ActionRequestMoveTo_Client"),
    ),
    entry(
        crate::wire_opcode::RequestMovePoint,
        Some(8),
        true,
        Some("ActionRequestMoveTo_Client"),
    ),
    entry(
        crate::wire_opcode::Unseen03F6,
        Some(8),
        true,
        Some("ActionRequestMoveTo_Client"),
    ),
    entry(crate::wire_opcode::Unseen03F7, Some(1), false, None),
    entry(crate::wire_opcode::ReservedNoop040C, None, false, None),
    entry(
        crate::wire_opcode::RequestTargetAbility,
        Some(5),
        true,
        Some("ActionRequestActivateAbility"),
    ),
    entry(
        crate::wire_opcode::RequestPointAbility,
        Some(13),
        true,
        Some("ActionRequestActivateAbility"),
    ),
    entry(
        crate::wire_opcode::RequestActivateAbilityUnseen,
        Some(17),
        true,
        Some("ActionRequestActivateAbility"),
    ),
    entry(
        crate::wire_opcode::RequestCancelAbility,
        Some(2),
        true,
        Some("ActionRequestCancelAbility_Client"),
    ),
    entry(
        crate::wire_opcode::RequestModifyBasicAttackTarget,
        Some(5),
        true,
        Some("ActionRequestModifyBasicAttackTarget"),
    ),
    entry(crate::wire_opcode::ReservedNoop0426, None, false, None),
    entry(crate::wire_opcode::ReservedNoop042A, None, false, None),
    entry(crate::wire_opcode::ReservedNoop042D, None, false, None),
    entry(
        crate::wire_opcode::RequestUpgradeAbility,
        Some(1),
        true,
        Some("ActionRequestUpgradeAbility_Client"),
    ),
    entry(
        crate::wire_opcode::ReorderItem,
        Some(6),
        true,
        Some("ActionReorderItem"),
    ),
    entry(
        crate::wire_opcode::RequestActivateItemSelf,
        Some(4),
        true,
        Some("ActionRequestActivateItem"),
    ),
    entry(
        crate::wire_opcode::RequestActivateItemTarget,
        Some(8),
        true,
        Some("ActionRequestActivateItem"),
    ),
    entry(
        crate::wire_opcode::RequestActivateItemPoint,
        Some(16),
        true,
        Some("ActionRequestActivateItem"),
    ),
    entry(crate::wire_opcode::Unresolved0450, Some(1), false, None),
    entry(crate::wire_opcode::DraftReadyAccept, Some(1), true, None),
    entry(crate::wire_opcode::DraftRerollHero, Some(1), true, None),
    entry(crate::wire_opcode::Unresolved0457, Some(1), false, None),
    entry(crate::wire_opcode::DraftRefreshRequest, Some(1), true, None),
    entry(
        crate::wire_opcode::DraftSelectTeamOrSlot,
        Some(4),
        true,
        None,
    ),
    entry(crate::wire_opcode::DraftSelectHero, Some(8), true, None),
    entry(crate::wire_opcode::DraftSelectSkin, Some(4), true, None),
    entry(crate::wire_opcode::DraftSelectRole, Some(1), false, None),
    entry(crate::wire_opcode::DraftSelectPosition, Some(1), true, None),
    entry(
        crate::wire_opcode::DraftSelectBuildPath,
        Some(1),
        true,
        None,
    ),
    entry(crate::wire_opcode::DraftLockHero, Some(1), true, None),
    entry(crate::wire_opcode::DraftSelectBan, Some(4), true, None),
    entry(crate::wire_opcode::DraftConfirmBan, Some(1), true, None),
    entry(crate::wire_opcode::DraftSkipBan, Some(4), true, None),
    entry(crate::wire_opcode::DraftSwapRequest, Some(2), true, None),
    entry(crate::wire_opcode::DraftSwapResponse, Some(2), true, None),
    entry(crate::wire_opcode::DraftSwapCancel, Some(2), false, None),
    entry(crate::wire_opcode::DraftSwapDecline, Some(2), false, None),
    entry(crate::wire_opcode::DraftReadyControl, Some(1), true, None),
    entry(crate::wire_opcode::SceneReadyConfirm, Some(1), true, None),
    entry(
        crate::wire_opcode::RequestSceneSnapshot,
        Some(1),
        true,
        None,
    ),
    entry(
        crate::wire_opcode::SetSurrenderStateRequest,
        Some(1),
        true,
        Some("ActionSetSurrenderStateRequest"),
    ),
    entry(
        crate::wire_opcode::SpectatorExitMatchRequest,
        Some(1),
        true,
        Some("ActionSpectatorExitMatchRequest"),
    ),
    entry(crate::wire_opcode::ReservedNoop047A, None, false, None),
    entry(crate::wire_opcode::Unresolved047B, Some(1), true, None),
    entry(
        crate::wire_opcode::RequestEquipTalent,
        Some(4),
        true,
        Some("ActionRequestEquipTalent"),
    ),
    entry(
        crate::wire_opcode::RequestRecommendedBuildPath,
        Some(1),
        true,
        Some("ActionRequestRecommendedBuildPath"),
    ),
    entry(
        crate::wire_opcode::RequestLevelLogicEvent,
        Some(4),
        true,
        Some("ActionRequestLevelLogicEvent_Client"),
    ),
    entry(crate::wire_opcode::Unresolved048E, Some(8), false, None),
];
