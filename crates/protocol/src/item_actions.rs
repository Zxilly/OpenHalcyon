//! Recorded item action layouts. A serializer prefix by itself proves neither
//! the network sender's identity nor that the server applied the command.
use std::io;

use crate::{Message, Opcode, invalid};

/// The activation receiver selects which operand it consumes. Unknown selectors
/// and the inactive operands are retained in the wire view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemActivationMode {
    Target,
    Position,
    Unknown(u8),
}
impl From<u8> for ItemActivationMode {
    fn from(raw: u8) -> Self {
        match raw {
            0 => Self::Target,
            1 => Self::Position,
            other => Self::Unknown(other),
        }
    }
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
        mode: u8,
        suffix: &'a [u8],
    },
}

fn prefix_len(op: Opcode) -> Option<usize> {
    Some(match op {
        Opcode::ReorderItem => 6,
        Opcode::RequestActivateItem => 4,
        Opcode::RequestActivateItemVariantB | Opcode::SellItem => 8,
        Opcode::RequestActivateItemVariantC => 16,
        Opcode::ItemActivated => 21,
        _ => return None,
    })
}

fn supported_len(prefix: usize, len: usize) -> bool {
    len == prefix || len == (prefix + 2).next_multiple_of(8) - 2
}

impl<'a> ItemAction<'a> {
    pub fn opcode(&self) -> Opcode {
        match self {
            Self::Sell { .. } => Opcode::SellItem,
            Self::Reorder { .. } => Opcode::ReorderItem,
            Self::Activate { .. } => Opcode::RequestActivateItem,
            Self::TargetRequest { .. } => Opcode::RequestActivateItemVariantB,
            Self::VectorRequest { .. } => Opcode::RequestActivateItemVariantC,
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
        let (prefix, len) = self.lengths()?;
        let body = dst
            .get_mut(..len)
            .ok_or_else(|| invalid("item action output is too small"))?;
        let (head, tail) = body.split_at_mut(prefix);
        match *self {
            Self::Sell {
                actor, instance, ..
            } => put_words(head, &[actor, instance]),
            Self::Reorder { actor, slots } => {
                put_words(head, &[actor]);
                head[4..].copy_from_slice(&slots);
            }
            Self::Activate { instance, .. } => put_words(head, &[instance]),
            Self::TargetRequest { arguments, .. } => put_words(head, &arguments),
            Self::VectorRequest {
                vector_bits,
                argument,
                ..
            } => {
                put_words(
                    head,
                    &[vector_bits[0], vector_bits[1], vector_bits[2], argument],
                );
            }
            Self::Activation {
                actor,
                instance,
                target_word,
                coordinate_bits,
                mode,
                ..
            } => {
                put_words(
                    head,
                    &[
                        actor,
                        instance,
                        target_word,
                        coordinate_bits[0],
                        coordinate_bits[1],
                    ],
                );
                head[20] = mode;
            }
        }
        tail.copy_from_slice(self.suffix());
        Ok(len)
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
    pub(super) fn decode(op: u16, body: &'a [u8]) -> Option<Self> {
        let prefix = prefix_len(Opcode::try_from(op).ok()?)?;
        if !supported_len(prefix, body.len()) {
            return None;
        }
        let word = |offset: usize| u32::from_be_bytes(body[offset..offset + 4].try_into().unwrap());
        let suffix = &body[prefix..];
        Some(match op {
            crate::wire_opcode::SellItem => Self::Sell {
                actor: word(0),
                instance: word(4),
                suffix,
            },
            crate::wire_opcode::ReorderItem => Self::Reorder {
                actor: word(0),
                slots: [body[4], body[5]],
            },
            crate::wire_opcode::RequestActivateItem => Self::Activate {
                instance: word(0),
                suffix,
            },
            crate::wire_opcode::RequestActivateItemVariantB => Self::TargetRequest {
                arguments: [word(0), word(4)],
                suffix,
            },
            crate::wire_opcode::RequestActivateItemVariantC => Self::VectorRequest {
                vector_bits: [word(0), word(4), word(8)],
                argument: word(12),
                suffix,
            },
            crate::wire_opcode::ItemActivated => Self::Activation {
                actor: word(0),
                instance: word(4),
                target_word: word(8),
                coordinate_bits: [word(12), word(16)],
                mode: body[20],
                suffix,
            },
            _ => unreachable!(),
        })
    }

    pub fn activation_mode(&self) -> Option<ItemActivationMode> {
        match self {
            Self::Activation { mode, .. } => Some((*mode).into()),
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

fn put_words(dst: &mut [u8], words: &[u32]) {
    for (chunk, value) in dst.as_chunks_mut::<4>().0.iter_mut().zip(words) {
        *chunk = value.to_be_bytes();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Fields, LiveFields, Message};

    #[test]
    fn reorder_preserves_actor_and_both_zero_based_slot_bytes() {
        let cipher = crate::MatchCipher::new("reorder fixtures");
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
            (crate::wire_opcode::RequestActivateItem, 4, 6),
            (crate::wire_opcode::RequestActivateItemVariantB, 8, 14),
            (crate::wire_opcode::RequestActivateItemVariantC, 16, 22),
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
                    crate::wire_opcode::RequestActivateItem => ItemAction::Activate {
                        instance: 0x80818283,
                        suffix,
                    },
                    crate::wire_opcode::RequestActivateItemVariantB => ItemAction::TargetRequest {
                        arguments: [0x80818283, 0x84858687],
                        suffix,
                    },
                    crate::wire_opcode::RequestActivateItemVariantC => ItemAction::VectorRequest {
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
                        mode: 0x94,
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
        let cipher = crate::MatchCipher::new("item fixture");
        let action = ItemAction::Activation {
            actor: 2,
            instance: 259,
            target_word: 0x7f800011,
            coordinate_bits: [0x80000000, 0x7fc00012],
            mode: 0xfe,
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
}
