//! Recorded item action layouts. A serializer prefix by itself proves neither
//! the network sender's identity nor that the server applied the command.
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

impl<'a> ItemAction<'a> {
    /// Rebuild a reorder record in its original six-byte body layout.
    /// Both bytes are native zero-based slot indices for a symmetric swap.
    /// Mode-specific capacity and occupied slots are outside this codec.
    pub fn reorder_message(&self) -> Option<crate::Message> {
        let Self::Reorder { actor, slots } = self else {
            return None;
        };
        let mut body = [0; 6];
        body[..4].copy_from_slice(&actor.to_be_bytes());
        body[4..].copy_from_slice(slots);
        Some(crate::Message::known(crate::Opcode::ReorderItem, body))
    }

    pub(super) fn decode(op: u16, body: &'a [u8]) -> Option<Self> {
        let prefix: usize = match op {
            crate::wire_opcode::ReorderItem => 6,
            crate::wire_opcode::RequestActivateItem => 4,
            crate::wire_opcode::RequestActivateItemVariantB | crate::wire_opcode::SellItem => 8,
            crate::wire_opcode::RequestActivateItemVariantC => 16,
            crate::wire_opcode::ItemActivated => 21,
            _ => return None,
        };
        let padded = (prefix + 2).next_multiple_of(8) - 2;
        if body.len() != prefix && body.len() != padded {
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

    /// Only the mode-zero receiver uses the target word.
    pub fn active_target(&self) -> Option<u32> {
        match *self {
            Self::Activation {
                mode: 0,
                target_word,
                ..
            } => Some(target_word),
            _ => None,
        }
    }

    /// Mode one consumes the coordinate pair. Preserve all IEEE-754 bit patterns.
    pub fn active_position_bits(&self) -> Option<[u32; 2]> {
        match *self {
            Self::Activation {
                mode: 1,
                coordinate_bits,
                ..
            } => Some(coordinate_bits),
            _ => None,
        }
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
            }
            for size in [prefix - 1, padded + 1] {
                assert!(ItemAction::decode(op, &bytes[..size]).is_none());
            }
        }
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
