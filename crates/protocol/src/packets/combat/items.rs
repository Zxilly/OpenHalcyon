//! Recorded item action layouts, shop grant/buy/consume/reject packets, and recommended build paths.

use crate::codec::fixed_body::{self, fixed_packet};
use crate::codec::invalid;
use crate::{Message, Opcode};
use binrw::{BinRead, BinWrite};
use std::io;

const REORDER_PREFIX_LEN: usize = 6;
const ACTIVATE_SELF_PREFIX_LEN: usize = 4;
const ACTIVATE_TARGET_PREFIX_LEN: usize = 8;
const SELL_PREFIX_LEN: usize = 8;
const ACTIVATE_POINT_PREFIX_LEN: usize = 16;
const ACTIVATION_PREFIX_LEN: usize = 21;

/// The activation receiver selects which operand it consumes. Unknown selectors
/// and the inactive operands are retained in the wire view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemActivationMode {
    Target,
    Position,
    Untargeted,
    Unknown(u8),
}

impl From<u8> for ItemActivationMode {
    fn from(raw: u8) -> Self {
        match raw {
            0 => Self::Target,
            1 => Self::Position,
            2 => Self::Untargeted,
            other => Self::Unknown(other),
        }
    }
}

impl From<ItemActivationMode> for u8 {
    fn from(mode: ItemActivationMode) -> Self {
        match mode {
            ItemActivationMode::Target => 0,
            ItemActivationMode::Position => 1,
            ItemActivationMode::Untargeted => 2,
            ItemActivationMode::Unknown(value) => value,
        }
    }
}

impl BinRead for ItemActivationMode {
    type Args<'a> = ();

    fn read_options<R: io::Read + io::Seek>(
        reader: &mut R,
        endian: binrw::Endian,
        args: Self::Args<'_>,
    ) -> binrw::BinResult<Self> {
        u8::read_options(reader, endian, args).map(Self::from)
    }
}

impl BinWrite for ItemActivationMode {
    type Args<'a> = ();

    fn write_options<W: io::Write + io::Seek>(
        &self,
        writer: &mut W,
        endian: binrw::Endian,
        args: Self::Args<'_>,
    ) -> binrw::BinResult<()> {
        u8::from(*self).write_options(writer, endian, args)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
struct SellItemWire {
    actor: u32,
    instance: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
struct ReorderItemWire {
    actor: u32,
    slots: [u8; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
struct ActivateItemSelfWire {
    instance: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
struct ActivateItemTargetWire {
    arguments: [u32; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
struct ActivateItemPointWire {
    vector_bits: [u32; 3],
    argument: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
struct ItemActivatedWire {
    actor: u32,
    instance: u32,
    target_word: u32,
    coordinate_bits: [u32; 2],
    mode: ItemActivationMode,
}

#[derive(Debug, PartialEq)]
pub enum ItemAction<'a> {
    Sell {
        actor: u32,
        instance: u32,
        suffix: &'a [u8],
    },
    Reorder {
        actor: u32,
        slots: [u8; 2],
    },
    Activate {
        instance: u32,
        suffix: &'a [u8],
    },
    TargetRequest {
        arguments: [u32; 2],
        suffix: &'a [u8],
    },
    VectorRequest {
        vector_bits: [u32; 3],
        argument: u32,
        suffix: &'a [u8],
    },
    Activation {
        actor: u32,
        instance: u32,
        target_word: u32,
        coordinate_bits: [u32; 2],
        mode: ItemActivationMode,
        suffix: &'a [u8],
    },
}

fn prefix_len(op: Opcode) -> Option<usize> {
    Some(match op {
        Opcode::ReorderItem => REORDER_PREFIX_LEN,
        Opcode::RequestActivateItemSelf => ACTIVATE_SELF_PREFIX_LEN,
        Opcode::RequestActivateItemTarget => ACTIVATE_TARGET_PREFIX_LEN,
        Opcode::SellItem => SELL_PREFIX_LEN,
        Opcode::RequestActivateItemPoint => ACTIVATE_POINT_PREFIX_LEN,
        Opcode::ItemActivated => ACTIVATION_PREFIX_LEN,
        _ => return None,
    })
}

fn supported_len(prefix: usize, len: usize) -> bool {
    len == prefix || len == fixed_body::padded_len(prefix)
}

impl<'a> ItemAction<'a> {
    pub fn opcode(&self) -> Opcode {
        match self {
            Self::Sell { .. } => Opcode::SellItem,
            Self::Reorder { .. } => Opcode::ReorderItem,
            Self::Activate { .. } => Opcode::RequestActivateItemSelf,
            Self::TargetRequest { .. } => Opcode::RequestActivateItemTarget,
            Self::VectorRequest { .. } => Opcode::RequestActivateItemPoint,
            Self::Activation { .. } => Opcode::ItemActivated,
        }
    }

    fn suffix(&self) -> &[u8] {
        match self {
            Self::Reorder { .. } => &[],
            Self::Sell { suffix, .. }
            | Self::Activate { suffix, .. }
            | Self::TargetRequest { suffix, .. }
            | Self::VectorRequest { suffix, .. }
            | Self::Activation { suffix, .. } => suffix,
        }
    }

    fn lengths(&self) -> io::Result<(usize, usize)> {
        let prefix = prefix_len(self.opcode()).expect("item action always has a fixed prefix");
        let len = prefix
            .checked_add(self.suffix().len())
            .filter(|&len| supported_len(prefix, len))
            .ok_or_else(|| invalid("unsupported item action suffix length"))?;
        Ok((prefix, len))
    }

    /// Writes the original native or block-aligned body. All region and suffix
    /// lengths are checked before writing. Bytes beyond the result are untouched.
    pub fn write_body(&self, dst: &mut [u8]) -> io::Result<usize> {
        let result = match *self {
            Self::Sell {
                actor,
                instance,
                suffix,
            } => fixed_body::write_with_padding(
                &SellItemWire { actor, instance },
                SELL_PREFIX_LEN,
                suffix,
                dst,
            ),
            Self::Reorder { actor, slots } => fixed_body::write_with_padding(
                &ReorderItemWire { actor, slots },
                REORDER_PREFIX_LEN,
                &[],
                dst,
            ),
            Self::Activate { instance, suffix } => fixed_body::write_with_padding(
                &ActivateItemSelfWire { instance },
                ACTIVATE_SELF_PREFIX_LEN,
                suffix,
                dst,
            ),
            Self::TargetRequest { arguments, suffix } => fixed_body::write_with_padding(
                &ActivateItemTargetWire { arguments },
                ACTIVATE_TARGET_PREFIX_LEN,
                suffix,
                dst,
            ),
            Self::VectorRequest {
                vector_bits,
                argument,
                suffix,
            } => fixed_body::write_with_padding(
                &ActivateItemPointWire {
                    vector_bits,
                    argument,
                },
                ACTIVATE_POINT_PREFIX_LEN,
                suffix,
                dst,
            ),
            Self::Activation {
                actor,
                instance,
                target_word,
                coordinate_bits,
                mode,
                suffix,
            } => fixed_body::write_with_padding(
                &ItemActivatedWire {
                    actor,
                    instance,
                    target_word,
                    coordinate_bits,
                    mode,
                },
                ACTIVATION_PREFIX_LEN,
                suffix,
                dst,
            ),
        };
        result.map_err(|err| {
            if err.to_string().contains("suffix") {
                invalid("unsupported item action suffix length")
            } else {
                invalid("item action output is too small")
            }
        })
    }

    pub fn to_message(&self) -> io::Result<Message> {
        let (_, len) = self.lengths()?;
        let mut body = vec![0; len];
        self.write_body(&mut body)?;
        Ok(Message::known(self.opcode(), body))
    }

    /// Both native zero-based reorder slots are preserved for a symmetric swap.
    pub fn reorder_message(&self) -> Option<Message> {
        matches!(self, Self::Reorder { .. })
            .then(|| self.to_message().expect("six-byte reorder body"))
    }

    pub(crate) fn decode(op: u16, body: &'a [u8]) -> Option<Self> {
        Some(match Opcode::try_from(op).ok()? {
            Opcode::SellItem => {
                let (wire, suffix): (SellItemWire, _) =
                    fixed_body::read_with_padding(body, SELL_PREFIX_LEN).ok()?;
                Self::Sell {
                    actor: wire.actor,
                    instance: wire.instance,
                    suffix,
                }
            }
            Opcode::ReorderItem => {
                let wire: ReorderItemWire =
                    fixed_body::read_exact(body, REORDER_PREFIX_LEN).ok()?;
                Self::Reorder {
                    actor: wire.actor,
                    slots: wire.slots,
                }
            }
            Opcode::RequestActivateItemSelf => {
                let (wire, suffix): (ActivateItemSelfWire, _) =
                    fixed_body::read_with_padding(body, ACTIVATE_SELF_PREFIX_LEN).ok()?;
                Self::Activate {
                    instance: wire.instance,
                    suffix,
                }
            }
            Opcode::RequestActivateItemTarget => {
                let (wire, suffix): (ActivateItemTargetWire, _) =
                    fixed_body::read_with_padding(body, ACTIVATE_TARGET_PREFIX_LEN).ok()?;
                Self::TargetRequest {
                    arguments: wire.arguments,
                    suffix,
                }
            }
            Opcode::RequestActivateItemPoint => {
                let (wire, suffix): (ActivateItemPointWire, _) =
                    fixed_body::read_with_padding(body, ACTIVATE_POINT_PREFIX_LEN).ok()?;
                Self::VectorRequest {
                    vector_bits: wire.vector_bits,
                    argument: wire.argument,
                    suffix,
                }
            }
            Opcode::ItemActivated => {
                let (wire, suffix): (ItemActivatedWire, _) =
                    fixed_body::read_with_padding(body, ACTIVATION_PREFIX_LEN).ok()?;
                Self::Activation {
                    actor: wire.actor,
                    instance: wire.instance,
                    target_word: wire.target_word,
                    coordinate_bits: wire.coordinate_bits,
                    mode: wire.mode,
                    suffix,
                }
            }
            _ => return None,
        })
    }

    pub fn activation_mode(&self) -> Option<ItemActivationMode> {
        match self {
            Self::Activation { mode, .. } => Some(*mode),
            _ => None,
        }
    }

    pub fn active_target(&self) -> Option<u32> {
        match (self, self.activation_mode()?) {
            (Self::Activation { target_word, .. }, ItemActivationMode::Target) => {
                Some(*target_word)
            }
            _ => None,
        }
    }

    pub fn active_position_bits(&self) -> Option<[u32; 2]> {
        match (self, self.activation_mode()?) {
            (
                Self::Activation {
                    coordinate_bits, ..
                },
                ItemActivationMode::Position,
            ) => Some(*coordinate_bits),
            _ => None,
        }
    }
}

/// In-game build-path selection. Draft deselection uses a different sentinel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::FromRepr, BinRead, BinWrite)]
#[brw(big, repr = u8)]
#[repr(u8)]
pub enum RecommendedBuildPath {
    Slot0 = 0,
    Slot1 = 1,
    Slot2 = 2,
    NoBuildPath = 0xfe,
}

/// `0x0485 RequestRecommendedBuildPath`: 1-byte client build-path announcement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct RequestRecommendedBuildPathBody {
    pub path: RecommendedBuildPath,
}
fixed_packet!(
    RequestRecommendedBuildPathBody,
    Opcode::RequestRecommendedBuildPath,
    1
);

/// `0x0488 SetRecommendedBuildPath`: 5-byte server build-path assignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct SetRecommendedBuildPathBody {
    pub actor: u32,
    pub path: RecommendedBuildPath,
}
fixed_packet!(
    SetRecommendedBuildPathBody,
    Opcode::SetRecommendedBuildPath,
    5
);

impl RecommendedBuildPath {
    pub fn announcement(self) -> Message {
        RequestRecommendedBuildPathBody { path: self }.encode()
    }

    pub fn applied_to(self, actor: u32) -> Message {
        SetRecommendedBuildPathBody { actor, path: self }.encode()
    }
}

/// `0x043d ItemGrant`: 12-byte inventory item grant packet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct ItemGrantBody {
    /// Receiving wire actor identifier.
    pub actor: u32,
    /// Item definition identifier.
    pub item: u32,
    /// Newly assigned item instance identifier.
    pub instance: u32,
}
fixed_packet!(ItemGrantBody, Opcode::ItemGrant, 12);

/// `0x044b ConsumeItem`: 10-byte inventory item removal/consumption packet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct ConsumeItemBody {
    /// Owning wire actor identifier.
    pub actor: u32,
    /// Item instance identifier being removed.
    pub instance: u32,
    /// Trailing removal/consumption flags.
    pub flags: u16,
}
fixed_packet!(ConsumeItemBody, Opcode::ConsumeItem, 10);

/// `0x0439 BuyItem`: 8-byte item purchase request packet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct BuyItemBody {
    /// Purchasing wire actor identifier.
    pub actor: u32,
    /// Item definition identifier.
    pub item: u32,
}
fixed_packet!(BuyItemBody, Opcode::BuyItem, 8);

/// `0x048f RejectBuyItem`: 0-byte rejected item purchase notification.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct RejectBuyItemBody;
fixed_packet!(RejectBuyItemBody, Opcode::RejectBuyItem, 0);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Fields, LiveFields, MatchCipher};

    #[test]
    fn reorder_preserves_actor_and_both_zero_based_slot_bytes() {
        let cipher = MatchCipher::new("reorder fixtures");
        for slots in [[4, 3], [3, 2], [6, 5], [5, 6], [0, 255]] {
            let input = ItemAction::Reorder { actor: 1500, slots };
            let message = input.reorder_message().unwrap();
            let frame = cipher.encode(&message).unwrap();
            let decoded = cipher.decode(&frame[2..]).unwrap();
            assert_eq!(decoded.body.len(), 6);
            let Fields::Live(LiveFields::Item(parsed)) = decoded.fields().unwrap() else {
                panic!("missing reorder view");
            };
            assert_eq!(parsed, input);
            assert_eq!(parsed.reorder_message(), Some(message));
        }
        assert!(ItemAction::decode(crate::wire_opcode::ReorderItem, &[0; 5]).is_none());
        assert!(ItemAction::decode(crate::wire_opcode::ReorderItem, &[0; 7]).is_none());
    }

    #[test]
    fn compact_operands_and_opaque_suffixes_survive_dispatch() {
        let bytes: Vec<u8> = (0x80..=0xa0).collect();
        for (op, prefix, padded) in [
            (crate::wire_opcode::ReorderItem, 6, 6),
            (crate::wire_opcode::RequestActivateItemSelf, 4, 6),
            (crate::wire_opcode::RequestActivateItemTarget, 8, 14),
            (crate::wire_opcode::RequestActivateItemPoint, 16, 22),
            (crate::wire_opcode::ItemActivated, 21, 22),
            (crate::wire_opcode::SellItem, 8, 14),
        ] {
            for size in [prefix, padded] {
                let m = Message::new(op, &bytes[..size]);
                let Fields::Live(LiveFields::Item(action)) = m.fields().unwrap() else {
                    panic!("{op:x}")
                };
                let suffix = &bytes[prefix..size];
                let expected = match op {
                    crate::wire_opcode::ReorderItem => ItemAction::Reorder {
                        actor: 0x80818283,
                        slots: [0x84, 0x85],
                    },
                    crate::wire_opcode::RequestActivateItemSelf => ItemAction::Activate {
                        instance: 0x80818283,
                        suffix,
                    },
                    crate::wire_opcode::RequestActivateItemTarget => ItemAction::TargetRequest {
                        arguments: [0x80818283, 0x84858687],
                        suffix,
                    },
                    crate::wire_opcode::RequestActivateItemPoint => ItemAction::VectorRequest {
                        vector_bits: [0x80818283, 0x84858687, 0x88898a8b],
                        argument: 0x8c8d8e8f,
                        suffix,
                    },
                    crate::wire_opcode::SellItem => ItemAction::Sell {
                        actor: 0x80818283,
                        instance: 0x84858687,
                        suffix,
                    },
                    _ => ItemAction::Activation {
                        actor: 0x80818283,
                        instance: 0x84858687,
                        target_word: 0x88898a8b,
                        coordinate_bits: [0x8c8d8e8f, 0x90919293],
                        mode: ItemActivationMode::Unknown(0x94),
                        suffix,
                    },
                };
                assert_eq!(action, expected);
                assert_eq!(expected.to_message().unwrap(), m);
                let mut out = [0xc7; 32];
                assert_eq!(expected.write_body(&mut out).unwrap(), size);
                assert_eq!(out[..size], bytes[..size]);
                assert!(out[size..].iter().all(|&b| b == 0xc7));
            }
            for size in [prefix - 1, padded + 1] {
                assert!(ItemAction::decode(op, &bytes[..size]).is_none());
            }
        }
    }

    #[test]
    fn arbitrary_mode_and_float_bits_survive_encryption_and_checked_writes() {
        let raw = [
            0, 0, 0, 2, 0, 0, 1, 3, 0x7f, 0x80, 0, 0x11, 0x80, 0, 0, 0, 0x7f, 0xc0, 0, 0x12, 0xfe,
            0xb5,
        ];
        let cipher = MatchCipher::new("item fixture");
        let action = ItemAction::Activation {
            actor: 2,
            instance: 259,
            target_word: 0x7f800011,
            coordinate_bits: [0x80000000, 0x7fc00012],
            mode: ItemActivationMode::Unknown(0xfe),
            suffix: &[0xb5],
        };
        assert_eq!(
            action.activation_mode(),
            Some(ItemActivationMode::Unknown(0xfe))
        );
        let msg = action.to_message().unwrap();
        assert_eq!(msg.body, raw);
        let encrypted = cipher.encode(&msg).unwrap();
        let decrypted = cipher.decode(&encrypted[2..]).unwrap();
        let Fields::Live(LiveFields::Item(decoded)) = decrypted.fields().unwrap() else {
            panic!()
        };
        assert_eq!(action, decoded);
        assert!(action.reorder_message().is_none());
        let mut short = [0xa7; 21];
        assert!(action.write_body(&mut short).is_err());
        assert_eq!(short, [0xa7; 21]);
        let bad = ItemAction::Sell {
            actor: 1,
            instance: 2,
            suffix: &[1, 2],
        };
        assert!(bad.to_message().is_err());
        assert!(bad.write_body(&mut short).is_err());
        assert_eq!(short, [0xa7; 21]);
    }

    #[test]
    fn activation_mode_controls_active_operands() {
        let mut body = [0xff; 22];
        body[8..12].copy_from_slice(&1510u32.to_be_bytes());
        body[12..16].copy_from_slice(&0x7fc0_4321u32.to_be_bytes());
        for mode in [0, 1, 2, 255] {
            body[20] = mode;
            let action = ItemAction::decode(crate::wire_opcode::ItemActivated, &body).unwrap();
            assert_eq!(action.active_target(), (mode == 0).then_some(1510));
            assert_eq!(
                action.active_position_bits(),
                (mode == 1).then_some([0x7fc04321, u32::MAX])
            );
        }
    }

    #[test]
    fn recommended_build_path_bodies_roundtrip_golden_bytes() {
        for path in [
            RecommendedBuildPath::Slot0,
            RecommendedBuildPath::Slot1,
            RecommendedBuildPath::Slot2,
            RecommendedBuildPath::NoBuildPath,
        ] {
            let req = path.announcement();
            assert_eq!(req.body, [path as u8]);
            assert_eq!(
                RequestRecommendedBuildPathBody::read_exact(&req.body)
                    .unwrap()
                    .path,
                path
            );

            let applied = path.applied_to(1502);
            assert_eq!(applied.body, [0, 0, 5, 222, path as u8]);
            let decoded = SetRecommendedBuildPathBody::read_exact(&applied.body).unwrap();
            assert_eq!(decoded.actor, 1502);
            assert_eq!(decoded.path, path);
        }
        assert!(RequestRecommendedBuildPathBody::read_exact(&[3]).is_err());
        assert!(SetRecommendedBuildPathBody::read_exact(&[0, 0, 5, 222, 3]).is_err());
    }

    #[test]
    fn item_grant_consume_buy_and_reject_roundtrip() {
        let grant = ItemGrantBody {
            actor: 1500,
            item: 201,
            instance: 9001,
        };
        assert_eq!(
            ItemGrantBody::read_exact(&grant.encode().body).ok(),
            Some(grant)
        );

        let consume = ConsumeItemBody {
            actor: 1500,
            instance: 9001,
            flags: 1,
        };
        assert_eq!(
            ConsumeItemBody::read_exact(&consume.encode().body).ok(),
            Some(consume)
        );

        let buy = BuyItemBody {
            actor: 1500,
            item: 201,
        };
        assert_eq!(BuyItemBody::read_exact(&buy.encode().body).ok(), Some(buy));
        assert!(RejectBuyItemBody.encode().body.is_empty());
    }
}
