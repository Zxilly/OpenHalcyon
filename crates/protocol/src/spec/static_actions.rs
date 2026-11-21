//! Fixed Windows receiver primitives with matching Android frame lengths.
//! This table labels static action classes. It does not identify network inputs.
use crate::{AndroidLayout, Fields, Message};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StaticValue {
    Byte(u8),
    Be16(u16),
    Be32(u32),
    /// IEEE-754 payload bits, kept comparable even for NaNs and signed zero.
    BeFloatBits(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Primitive {
    Byte,
    Be16,
    Be32,
    Float,
}

impl Primitive {
    fn read(self, body: &[u8], offset: usize) -> StaticValue {
        match self {
            Self::Byte => StaticValue::Byte(body[offset]),
            Self::Be16 => StaticValue::Be16(u16::from_be_bytes(
                body[offset..offset + 2].try_into().unwrap(),
            )),
            Self::Be32 | Self::Float => {
                let bits = u32::from_be_bytes(body[offset..offset + 4].try_into().unwrap());
                if self == Self::Float {
                    StaticValue::BeFloatBits(bits)
                } else {
                    StaticValue::Be32(bits)
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Span {
    kind: Primitive,
    offset: usize,
    count: usize,
    stride: usize,
}

const fn span(kind: Primitive, offset: usize, count: usize, stride: usize) -> Span {
    Span {
        kind,
        offset,
        count,
        stride,
    }
}

const fn at(kind: Primitive, offset: usize) -> Span {
    span(kind, offset, 1, 0)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticActionLayout {
    pub opcode: u16,
    pub action_name: Option<&'static str>,
    fields: &'static [Span],
}

impl StaticActionLayout {
    pub fn for_opcode(opcode: u16) -> Option<Self> {
        let index = STATIC_ACTIONS
            .binary_search_by_key(&opcode, |row| row.opcode)
            .ok()?;
        Some(STATIC_ACTIONS[index])
    }

    pub fn has_fields(self) -> bool {
        !self.fields.is_empty()
    }

    /// Accepts the fixed-length body or the length obtained after block padding.
    /// Opaque fields, including nonzero padding, are borrowed without normalization.
    pub fn read(self, body: &[u8]) -> Option<StaticAction<'_>> {
        let AndroidLayout::Fixed { frame_bytes } = AndroidLayout::for_opcode(self.opcode) else {
            return None;
        };
        let body_bytes = frame_bytes - 4;
        let padded_bytes = (body_bytes + 2).next_multiple_of(8) - 2;
        if body.len() != body_bytes && body.len() != padded_bytes {
            return None;
        }
        Some(StaticAction {
            layout: self,
            body,
            body_bytes,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticAction<'a> {
    pub layout: StaticActionLayout,
    body: &'a [u8],
    body_bytes: usize,
}

impl<'a> StaticAction<'a> {
    pub fn payload(self) -> &'a [u8] {
        &self.body[..self.body_bytes]
    }

    pub fn padding(self) -> &'a [u8] {
        &self.body[self.body_bytes..]
    }

    pub fn primitives(self) -> impl Iterator<Item = (usize, StaticValue)> + 'a {
        let body = self.payload();
        let words = if self.layout.opcode == crate::Opcode::SetBuildPath.raw() {
            // The reader uses byte 96 as the number of words starting at byte 4.
            // Out-of-range counts leave the entire operand region opaque.
            let count = usize::from(body[96]);
            (count <= 23).then_some(span(Primitive::Be32, 4, count, 4))
        } else {
            None
        };
        self.layout
            .fields
            .iter()
            .copied()
            .chain(words)
            .flat_map(move |field| {
                (0..field.count).map(move |i| {
                    let offset = field.offset + i * field.stride;
                    (offset, field.kind.read(body, offset))
                })
            })
    }

    pub fn to_message(self) -> Message {
        Message::new(self.layout.opcode, self.body)
    }
}

pub(crate) fn fields(opcode: u16, body: &[u8]) -> Option<Fields<'_>> {
    let layout = StaticActionLayout::for_opcode(opcode)?;
    layout.has_fields().then_some(())?;
    Some(Fields::StaticAction(layout.read(body)?))
}

const fn entry(
    opcode: u16,
    action_name: Option<&'static str>,
    fields: &'static [Span],
) -> StaticActionLayout {
    StaticActionLayout {
        opcode,
        action_name,
        fields,
    }
}

use Primitive::{Be16 as H, Be32 as W, Byte as B, Float as F};

/// The intersection of fixed Android layouts and opcodes absent in both local corpora.
/// Empty primitive lists retain length and any evidenced static action name.
pub const STATIC_ACTIONS: &[StaticActionLayout] = &[
    entry(crate::wire_opcode::ClientHello, None, &[]),
    entry(crate::wire_opcode::ServerHelloAck, None, &[]),
    entry(crate::wire_opcode::PlayerDeserter, None, &[at(W, 0)]),
    entry(
        crate::wire_opcode::ShowMatchPrepSequence,
        Some("ActionShowMatchPrepSequence"),
        &[],
    ),
    entry(
        crate::wire_opcode::ResetMovement,
        Some("ActionResetMovement_Client"),
        &[span(F, 1, 4, 4)],
    ),
    entry(
        crate::wire_opcode::MoveToPredictive,
        Some("ActionMoveToPredictive_Client"),
        &[span(F, 1, 2, 4)],
    ),
    entry(
        crate::wire_opcode::MoveToAuthoritative,
        Some("ActionMoveToAuthoritative_Client"),
        &[span(F, 1, 4, 4)],
    ),
    entry(
        crate::wire_opcode::StopAuthoritative,
        Some("ActionStopAuthoritative_Client"),
        &[span(F, 1, 2, 4)],
    ),
    entry(
        crate::wire_opcode::StopNavigating,
        Some("ActionStopNavigating_Client"),
        &[span(F, 1, 2, 4)],
    ),
    entry(
        crate::wire_opcode::FaceDir,
        Some("ActionFaceDir"),
        &[at(W, 0), span(F, 4, 3, 4)],
    ),
    entry(
        crate::wire_opcode::AutoMoveToActorResponse,
        Some("ActionAutoMoveToActorResponse"),
        &[span(F, 1, 3, 4), span(B, 13, 2, 1)],
    ),
    entry(
        crate::wire_opcode::AutoMoveToLocation,
        Some("ActionAutoMoveToLocation_Client"),
        &[span(F, 1, 5, 4)],
    ),
    entry(
        crate::wire_opcode::AttachToActor,
        Some("ActionAttachToActor_Client"),
        &[at(W, 0)],
    ),
    entry(
        crate::wire_opcode::Teleport,
        Some("ActionTeleport_Client"),
        &[span(F, 1, 4, 4)],
    ),
    entry(
        crate::wire_opcode::PlayAbility,
        Some("ActionPlayAbility"),
        &[span(W, 0, 2, 4), span(F, 8, 3, 4)],
    ),
    entry(
        crate::wire_opcode::PlayVoiceOver,
        Some("ActionPlayVoiceOver"),
        &[span(W, 0, 2, 4)],
    ),
    entry(
        crate::wire_opcode::CreateZoneOfControlUnseen,
        Some("ActionCreateZoneOfControl"),
        &[span(W, 0, 2, 4), span(F, 8, 5, 4), at(B, 28)],
    ),
    entry(
        crate::wire_opcode::CreateRectZoneOfControl,
        Some("ActionCreateZoneOfControl"),
        &[span(W, 0, 2, 4), span(F, 8, 4, 4)],
    ),
    entry(
        crate::wire_opcode::PauseCooldown,
        Some("ActionPauseCooldown"),
        &[span(W, 0, 2, 4)],
    ),
    entry(
        crate::wire_opcode::ModifyAbilityCharges,
        Some("ActionModifyAbilityCharges"),
        &[span(W, 0, 2, 4)],
    ),
    entry(
        crate::wire_opcode::QuickBuyItem,
        Some("ActionQuickBuyItem"),
        &[at(W, 0)],
    ),
    entry(
        crate::wire_opcode::DowngradeAbility,
        Some("ActionDowngradeAbility"),
        &[span(W, 0, 2, 4)],
    ),
    entry(
        crate::wire_opcode::ModifyBuffMaxStacks,
        Some("ActionModifyBuffMaxStacks"),
        &[span(W, 0, 2, 4)],
    ),
    entry(
        crate::wire_opcode::ModifyItemStack,
        Some("ActionModifyItemStack"),
        &[span(W, 0, 2, 4)],
    ),
    entry(
        crate::wire_opcode::DraftRosterRanked,
        None,
        &[
            span(W, 4, 4, 4),
            span(W, 24, 60, 4),
            span(H, 273, 10, 166),
            span(H, 275, 10, 166),
            span(F, 2124, 2, 4),
        ],
    ),
    entry(
        crate::wire_opcode::PauseTutorial,
        Some("ActionPauseTutorial"),
        &[at(W, 0)],
    ),
    entry(
        crate::wire_opcode::SetSimulationSpeed,
        Some("ActionSetSimulationSpeed"),
        &[at(F, 0)],
    ),
    entry(
        crate::wire_opcode::SetPlayerAsSpectator,
        Some("ActionSetPlayerAsSpectator"),
        &[at(W, 0), at(B, 4)],
    ),
    entry(crate::wire_opcode::Unresolved047C, None, &[at(B, 0)]),
    entry(
        crate::wire_opcode::TutorialStateChangedResponse,
        Some("ActionTutorialStateChangedResponse"),
        &[],
    ),
    entry(
        crate::wire_opcode::StateChange,
        Some("ActionStateChange_Client"),
        &[at(W, 0)],
    ),
    entry(crate::wire_opcode::Unresolved047F, None, &[at(W, 0)]),
    entry(
        crate::wire_opcode::UnequipTalent,
        Some("ActionUnequipTalent"),
        &[at(W, 0)],
    ),
    entry(
        crate::wire_opcode::InvalidateTalent,
        Some("ActionInvalidateTalent"),
        &[at(W, 0)],
    ),
    entry(
        crate::wire_opcode::SetBuildPath,
        Some("ActionSetBuildPath"),
        &[at(W, 0), at(B, 96)],
    ),
    entry(
        crate::wire_opcode::SyncAbilityBehavior,
        Some("ActionSyncAbilityBehavior"),
        &[span(W, 0, 2, 4), span(B, 8, 2, 1)],
    ),
    entry(
        crate::wire_opcode::ModifyBuffVar,
        Some("ActionModifyBuffVar"),
        &[at(W, 0), at(W, 4), at(W, 12), at(B, 16)],
    ),
    entry(
        crate::wire_opcode::BroadcastSwapTarget,
        Some("ActionBroadcastSwapTarget"),
        &[span(W, 0, 2, 4)],
    ),
];
