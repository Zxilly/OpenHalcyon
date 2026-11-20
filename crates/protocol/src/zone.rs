use super::{Message, invalid};
use std::io;

/// Native actor-anchored and rectangular zone declarations and removal.
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
    /// Two X/Z endpoints (`p0_xz_bits`, `p1_xz_bits`). In Android 147219,
    /// `0x82bccc` -> `0xc00e64` -> `0xc7e1e0` stores `p0_xz` (`+0x60`) and `p1_xz`
    /// (`+0x68`) with `flags |= 4` (`+0x78`), and movement collision (`0xc7dd80`,
    /// branch `0xc7de74`/`0xc7df48`) invokes 2D line-segment vs. line-segment
    /// intersection (`0x18bb8ec`, `t, u in [0, 1]`), establishing an oriented 2D
    /// line-segment barrier `(p0_x, p0_z) -> (p1_x, p1_z)` rather than an AABB.
    /// Original IEEE-754 bits are retained without assuming endpoint order.
    Rectangle {
        id: u32,
        owner: u32,
        p0_xz_bits: [u32; 2],
        p1_xz_bits: [u32; 2],
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
            (crate::wire_opcode::CreateZoneOfControl, 25 | 30) => Some(ZoneAction::Create {
                id: word(0),
                source: word(4),
                anchor: word(8),
                parameters: [word(12), word(16), word(20)],
                flag: body[24],
                suffix: &body[25..],
            }),
            (crate::wire_opcode::CreateRectZoneOfControl, 24 | 30) => Some(ZoneAction::Rectangle {
                id: word(0),
                owner: word(4),
                p0_xz_bits: [word(8), word(12)],
                p1_xz_bits: [word(16), word(20)],
                suffix: &body[24..],
            }),
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
                Ok(Message::known(crate::Opcode::CreateZoneOfControl, body))
            }
            Self::Rectangle {
                id,
                owner,
                p0_xz_bits,
                p1_xz_bits,
                suffix,
            } if matches!(suffix.len(), 0 | 6) => {
                let mut body = Vec::with_capacity(24 + suffix.len());
                for word in [id, owner].into_iter().chain(p0_xz_bits).chain(p1_xz_bits) {
                    body.extend(word.to_be_bytes());
                }
                body.extend_from_slice(suffix);
                Ok(Message::known(crate::Opcode::CreateRectZoneOfControl, body))
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
    fn rectangle_preserves_coordinates_and_padding_as_received() {
        let body = [
            0, 0, 8, 1, 0, 0, 5, 220, 0x80, 0, 0, 0, 0x7f, 0x80, 0, 1, 0xbf, 0x80, 0, 0, 0x40, 0,
            0, 0, 1, 2, 3, 4, 5, 6,
        ];
        let m = Message::known(crate::Opcode::CreateRectZoneOfControl, body);
        let expected = ZoneAction::Rectangle {
            id: 2049,
            owner: 1500,
            p0_xz_bits: [0x80000000, 0x7f800001],
            p1_xz_bits: [0xbf800000, 0x40000000],
            suffix: &body[24..],
        };
        assert_eq!(expected.to_message().unwrap(), m);
        assert_eq!(m.fields().unwrap(), crate::Fields::Zone(expected));
        let invalid = ZoneAction::Rectangle {
            id: 1,
            owner: 2,
            p0_xz_bits: [0; 2],
            p1_xz_bits: [0; 2],
            suffix: &[0],
        };
        assert!(invalid.to_message().is_err());
    }

    #[test]
    fn native_and_aligned_zone_actions_retain_all_bits() {
        for (opcode, lengths) in [
            (crate::wire_opcode::CreateZoneOfControl, [25, 30]),
            (crate::wire_opcode::CreateRectZoneOfControl, [24, 30]),
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
