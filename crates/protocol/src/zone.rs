use super::{Message, invalid};
use std::io;

/// Actor-anchored zone records. The other create variants have different layouts.
#[derive(Debug, PartialEq, Eq)]
pub enum ZoneAction<'a> {
    Create {
        id: u32,
        source: u32,
        anchor: u32,
        /// The original scalar words, preserving unknown meanings and float bits.
        parameters: [u32; 3],
        flag: u8,
        suffix: &'a [u8],
    },
    Destroy {
        id: u32,
        suffix: &'a [u8],
    },
}

impl Message {
    /// Decode the native prefix or its transport-aligned form.
    pub fn zone_action(&self) -> Option<ZoneAction<'_>> {
        let body = &self.body;
        let word = |i: usize| u32::from_be_bytes(body[i..i + 4].try_into().unwrap());
        match (self.opcode, body.len()) {
            (crate::wire_opcode::CreateZoneOfControlVariantB, 25 | 30) => {
                Some(ZoneAction::Create {
                    id: word(0),
                    source: word(4),
                    anchor: word(8),
                    parameters: [word(12), word(16), word(20)],
                    flag: body[24],
                    suffix: &body[25..],
                })
            }
            (crate::wire_opcode::DestroyZoneOfControl, 4 | 6) => Some(ZoneAction::Destroy {
                id: word(0),
                suffix: &body[4..],
            }),
            _ => None,
        }
    }
}

impl ZoneAction<'_> {
    pub fn to_message(&self) -> io::Result<Message> {
        match self {
            Self::Create {
                id,
                source,
                anchor,
                parameters,
                flag,
                suffix,
            } if matches!(suffix.len(), 0 | 5) => {
                let mut body = Vec::with_capacity(25 + suffix.len());
                for word in [id, source, anchor].into_iter().chain(parameters) {
                    body.extend(word.to_be_bytes());
                }
                body.push(*flag);
                body.extend_from_slice(suffix);
                Ok(Message::known(
                    crate::Opcode::CreateZoneOfControlVariantB,
                    body,
                ))
            }
            Self::Destroy { id, suffix } if matches!(suffix.len(), 0 | 2) => {
                let mut body = Vec::with_capacity(4 + suffix.len());
                body.extend(id.to_be_bytes());
                body.extend_from_slice(suffix);
                Ok(Message::known(crate::Opcode::DestroyZoneOfControl, body))
            }
            _ => Err(invalid("unsupported zone action suffix length")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_and_aligned_zone_actions_retain_all_bits() {
        for (opcode, lengths) in [
            (crate::wire_opcode::CreateZoneOfControlVariantB, [25, 30]),
            (crate::wire_opcode::DestroyZoneOfControl, [4, 6]),
        ] {
            for len in lengths {
                let body: Vec<_> = (0..len).map(|n| 0xff - n as u8).collect();
                let message = Message::new(opcode, body);
                assert_eq!(
                    message.zone_action().unwrap().to_message().unwrap(),
                    message
                );
            }
            assert!(
                Message::new(opcode, vec![0; lengths[0] + 1])
                    .zone_action()
                    .is_none()
            );
        }
    }
}
