use super::{Message, invalid};
use std::io;

/// Native ActionApplyBuff prefix. Actor references and the definition index
/// are scoped to the recording/build; the duration retains its binary16 bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuffPrefix {
    pub target: u32,
    pub source: u32,
    pub duration_bits: u16,
    pub instance: u32,
    pub definition: u16,
}

/// Build 147219 effect definitions used by the native infusion and reboot lifecycles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum BuffDefinition {
    RevealToBestowersTeam = 62,
    WeaponInfusion = 283,
    CrystalInfusion = 284,
    Rebooting = 683,
    RebootCooldown = 684,
}

impl BuffPrefix {
    pub fn timed_self(actor: u32, instance: u32, definition: BuffDefinition, seconds: f32) -> Self {
        Self {
            target: actor,
            source: actor,
            duration_bits: half::f16::from_f32(seconds).to_bits(),
            instance,
            definition: definition as u16,
        }
    }

    pub fn duration(self) -> f32 {
        half::f16::from_bits(self.duration_bits).to_f32()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuffExtras {
    pub stacks: u16,
    /// Two formatter float operands and two further words. Their gameplay
    /// meanings are unresolved; preserve all four original bit patterns.
    pub words: [u32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuffApplication<'a> {
    pub prefix: BuffPrefix,
    pub extras: Option<BuffExtras>,
    /// Longer archive variants carry bytes beyond the native formatter.
    pub suffix: &'a [u8],
}

impl<'a> BuffApplication<'a> {
    pub(super) fn decode(opcode: u16, body: &'a [u8]) -> Option<Self> {
        let size = match (opcode, body.len()) {
            (crate::wire_opcode::ApplyBuffCompact, 16 | 22) => 16,
            (crate::wire_opcode::ApplyBuffExtended, 34 | 38) => 34,
            _ => return None,
        };
        let word = |i: usize| u32::from_be_bytes(body[i..i + 4].try_into().unwrap());
        let half = |i: usize| u16::from_be_bytes(body[i..i + 2].try_into().unwrap());
        Some(Self {
            prefix: BuffPrefix {
                target: word(0),
                source: word(4),
                duration_bits: half(8),
                instance: word(10),
                definition: half(14),
            },
            extras: (size == 34).then(|| BuffExtras {
                stacks: half(16),
                words: std::array::from_fn(|i| word(18 + 4 * i)),
            }),
            suffix: &body[size..],
        })
    }

    pub fn to_message(self) -> io::Result<Message> {
        let (op, size, extended) = if self.extras.is_some() {
            (crate::wire_opcode::ApplyBuffExtended, 34, 4)
        } else {
            (crate::wire_opcode::ApplyBuffCompact, 16, 6)
        };
        if !self.suffix.is_empty() && self.suffix.len() != extended {
            return Err(invalid("unsupported buff application suffix length"));
        }
        let mut body = Vec::with_capacity(size + self.suffix.len());
        body.extend(self.prefix.target.to_be_bytes());
        body.extend(self.prefix.source.to_be_bytes());
        body.extend(self.prefix.duration_bits.to_be_bytes());
        body.extend(self.prefix.instance.to_be_bytes());
        body.extend(self.prefix.definition.to_be_bytes());
        if let Some(extras) = self.extras {
            body.extend(extras.stacks.to_be_bytes());
            for word in extras.words {
                body.extend(word.to_be_bytes());
            }
        }
        body.extend(self.suffix);
        Ok(Message::new(op, body))
    }
}

/// Recorded mutations address the active application by its target and instance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BuffChange {
    Duration(f32),
    Cancel,
    Timeout,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BuffMutation<'a> {
    pub target: u32,
    pub instance: u32,
    pub change: BuffChange,
    pub padding: &'a [u8],
}

/// Native ActionModifyBuffStack. `instance` identifies a buff on this actor.
/// Preserve the full unsigned count and any capture-format suffix.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuffStack<'a> {
    pub target: u32,
    pub instance: u32,
    pub count: u32,
    pub suffix: &'a [u8],
}

impl BuffStack<'_> {
    pub fn to_message(self) -> io::Result<Message> {
        if !matches!(self.suffix.len(), 0 | 2) {
            return Err(invalid("unsupported buff stack suffix length"));
        }
        let mut body = Vec::with_capacity(12 + self.suffix.len());
        body.extend(self.target.to_be_bytes());
        body.extend(self.instance.to_be_bytes());
        body.extend(self.count.to_be_bytes());
        body.extend(self.suffix);
        Ok(Message::known(crate::Opcode::ModifyBuffStack, body))
    }
}

impl Message {
    /// Reads the Android native prefix and the observed extended capture form.
    pub fn buff_stack(&self) -> Option<BuffStack<'_>> {
        if !self.is(crate::Opcode::ModifyBuffStack) || !matches!(self.body.len(), 12 | 14) {
            return None;
        }
        let word = |i| u32::from_be_bytes(self.body[i..i + 4].try_into().unwrap());
        Some(BuffStack {
            target: word(0),
            instance: word(4),
            count: word(8),
            suffix: &self.body[12..],
        })
    }

    /// Distinguishes a duration update, explicit cancellation and timeout.
    pub fn buff_mutation(&self) -> Option<BuffMutation<'_>> {
        BuffMutation::decode(self.opcode, &self.body)
    }
}

impl<'a> BuffMutation<'a> {
    fn decode(op: u16, body: &'a [u8]) -> Option<Self> {
        let size = match op {
            crate::wire_opcode::ModifyBuffDuration => 12,
            crate::wire_opcode::CancelBuff | crate::wire_opcode::TimeoutBuff => 8,
            _ => return None,
        };
        if body.len() != size && body.len() != 14 {
            return None;
        }
        let change = match op {
            crate::wire_opcode::ModifyBuffDuration => {
                BuffChange::Duration(f32::from_be_bytes(body[8..12].try_into().unwrap()))
            }
            crate::wire_opcode::CancelBuff => BuffChange::Cancel,
            _ => BuffChange::Timeout,
        };
        Some(Self {
            target: u32::from_be_bytes(body[..4].try_into().unwrap()),
            instance: u32::from_be_bytes(body[4..8].try_into().unwrap()),
            change,
            padding: &body[size..],
        })
    }

    pub fn to_message(self) -> io::Result<Message> {
        let (op, size) = match self.change {
            BuffChange::Duration(_) => (crate::wire_opcode::ModifyBuffDuration, 12),
            BuffChange::Cancel => (crate::wire_opcode::CancelBuff, 8),
            BuffChange::Timeout => (crate::wire_opcode::TimeoutBuff, 8),
        };
        if !self.padding.is_empty() && self.padding.len() != 14 - size {
            return Err(invalid("unsupported buff mutation padding length"));
        }
        let mut body = Vec::with_capacity(size + self.padding.len());
        body.extend(self.target.to_be_bytes());
        body.extend(self.instance.to_be_bytes());
        if let BuffChange::Duration(seconds) = self.change {
            body.extend(seconds.to_be_bytes());
        }
        body.extend(self.padding);
        Ok(Message::new(op, body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Fields;

    #[test]
    fn buff_stack_preserves_both_native_shapes_and_the_existing_word_view() {
        let bytes = [
            0, 0, 5, 220, 0x12, 0x34, 0x56, 0x78, 0, 0, 3, 32, 0xAB, 0xCD,
        ];
        for len in [12, 14] {
            let msg = Message::known(crate::Opcode::ModifyBuffStack, &bytes[..len]);
            let change = msg.buff_stack().unwrap();
            assert_eq!(
                change,
                BuffStack {
                    target: 1500,
                    instance: 0x12345678,
                    count: 800,
                    suffix: &bytes[12..len]
                }
            );
            assert_eq!(change.to_message().unwrap(), msg);
            assert!(matches!(msg.fields(), Ok(Fields::ActionWords(_))));
            let zero = BuffStack { count: 0, ..change }.to_message().unwrap();
            assert_eq!(zero.buff_stack().unwrap().count, 0);
            assert!(
                BuffStack {
                    suffix: &[0],
                    ..change
                }
                .to_message()
                .is_err()
            );
        }
        for n in [11, 13, 15] {
            assert!(
                Message::known(crate::Opcode::ModifyBuffStack, vec![0; n])
                    .buff_stack()
                    .is_none()
            );
        }
        assert!(
            Message::known(crate::Opcode::ModifyItemStack, bytes)
                .buff_stack()
                .is_none()
        );
    }

    #[test]
    fn all_four_lengths_preserve_references_half_bits_and_unknown_words() {
        let prefix = BuffPrefix {
            target: 0x10203040,
            source: 0x802030ff,
            duration_bits: 0x7e51,
            instance: 0xfe123456,
            definition: 0x8492,
        };
        let extras = BuffExtras {
            stacks: 0xfedc,
            words: [0x80000000, 0x7fc04201, 0x01020304, 0xffffffff],
        };
        for (ext, suffix) in [
            (None, &[][..]),
            (None, &[1, 2, 3, 4, 5, 6]),
            (Some(extras), &[]),
            (Some(extras), &[9, 8, 7, 6]),
        ] {
            let original = BuffApplication {
                prefix,
                extras: ext,
                suffix,
            };
            let message = original.to_message().unwrap();
            assert_eq!(
                &message.body[8..16],
                &[0x7e, 0x51, 0xfe, 0x12, 0x34, 0x56, 0x84, 0x92]
            );
            if ext.is_some() {
                assert_eq!(&message.body[16..22], &[0xfe, 0xdc, 0x80, 0, 0, 0]);
            }
            let Fields::BuffApplication(decoded) = message.fields().unwrap() else {
                panic!()
            };
            assert_eq!(decoded, original);
            assert_eq!(decoded.to_message().unwrap(), message);
            for suffix in [&[0][..], &[0; 7]] {
                assert!(BuffApplication { suffix, ..original }.to_message().is_err());
            }
            assert!(
                BuffApplication::decode(message.opcode, &message.body[..message.body.len() - 1])
                    .is_none()
            );
        }
        for (bits, value) in [
            (0xbc00, -1.),
            (0x3e00, 1.5),
            (0x8000, -0.),
            (1, 2f32.powi(-24)),
            (0x7c00, f32::INFINITY),
        ] {
            assert_eq!(
                BuffPrefix {
                    duration_bits: bits,
                    ..prefix
                }
                .duration()
                .to_bits(),
                value.to_bits()
            );
        }
        assert!(prefix.duration().is_nan());
    }

    #[test]
    fn duration_cancel_and_timeout_keep_the_actor_instance_key() {
        for (opcode, prefix) in [
            (crate::wire_opcode::ModifyBuffDuration, 12),
            (crate::wire_opcode::CancelBuff, 8),
            (crate::wire_opcode::TimeoutBuff, 8),
        ] {
            let mut body = Vec::from(1500u32.to_be_bytes());
            body.extend(0x12345678u32.to_be_bytes());
            if prefix == 12 {
                body.extend(8f32.to_be_bytes());
            }
            for size in [prefix, 14] {
                body.resize(size, 0xab);
                let message = Message::new(opcode, body.clone());
                let action = message.buff_mutation().unwrap();
                assert_eq!((action.target, action.instance), (1500, 0x12345678));
                let expected = match opcode {
                    crate::wire_opcode::ModifyBuffDuration => BuffChange::Duration(8.),
                    crate::wire_opcode::CancelBuff => BuffChange::Cancel,
                    _ => BuffChange::Timeout,
                };
                assert_eq!(action.change, expected);
                assert_eq!(action.padding, &body[prefix..]);
                assert_eq!(action.to_message().unwrap(), message);
                assert!(
                    BuffMutation {
                        padding: &[7],
                        ..action
                    }
                    .to_message()
                    .is_err()
                );
            }
        }
    }
}
