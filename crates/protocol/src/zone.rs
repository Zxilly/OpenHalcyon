use super::{Message, Opcode, fixed_body};
use binrw::{BinRead, BinWrite};
use std::io;

const ZONE_CREATE_PREFIX_LEN: usize = 25;
const ZONE_RECT_PREFIX_LEN: usize = 24;
const ZONE_DESTROY_PREFIX_LEN: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
struct ZoneCreateWire {
    id: u32,
    source: u32,
    anchor: u32,
    parameters: [u32; 3],
    flag: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
struct ZoneRectWire {
    id: u32,
    owner: u32,
    p0_xz_bits: [u32; 2],
    p1_xz_bits: [u32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
struct ZoneDestroyWire {
    id: u32,
}

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
        match self.known_opcode()? {
            Opcode::CreateZoneOfControl => {
                let (wire, suffix): (ZoneCreateWire, _) =
                    fixed_body::read_with_padding(&self.body, ZONE_CREATE_PREFIX_LEN).ok()?;
                Some(ZoneAction::Create {
                    id: wire.id,
                    source: wire.source,
                    anchor: wire.anchor,
                    parameters: wire.parameters,
                    flag: wire.flag,
                    suffix,
                })
            }
            Opcode::CreateRectZoneOfControl => {
                let (wire, suffix): (ZoneRectWire, _) =
                    fixed_body::read_with_padding(&self.body, ZONE_RECT_PREFIX_LEN).ok()?;
                Some(ZoneAction::Rectangle {
                    id: wire.id,
                    owner: wire.owner,
                    p0_xz_bits: wire.p0_xz_bits,
                    p1_xz_bits: wire.p1_xz_bits,
                    suffix,
                })
            }
            Opcode::DestroyZoneOfControl => {
                let (wire, suffix): (ZoneDestroyWire, _) =
                    fixed_body::read_with_padding(&self.body, ZONE_DESTROY_PREFIX_LEN).ok()?;
                Some(ZoneAction::Destroy {
                    id: wire.id,
                    suffix,
                })
            }
            _ => None,
        }
    }
}

impl ZoneAction<'_> {
    pub fn to_message(&self) -> io::Result<Message> {
        match *self {
            Self::Create {
                id,
                source,
                anchor,
                parameters,
                flag,
                suffix,
            } => fixed_body::message_with_padding(
                Opcode::CreateZoneOfControl,
                &ZoneCreateWire {
                    id,
                    source,
                    anchor,
                    parameters,
                    flag,
                },
                ZONE_CREATE_PREFIX_LEN,
                suffix,
            ),
            Self::Rectangle {
                id,
                owner,
                p0_xz_bits,
                p1_xz_bits,
                suffix,
            } => fixed_body::message_with_padding(
                Opcode::CreateRectZoneOfControl,
                &ZoneRectWire {
                    id,
                    owner,
                    p0_xz_bits,
                    p1_xz_bits,
                },
                ZONE_RECT_PREFIX_LEN,
                suffix,
            ),
            Self::Destroy { id, suffix } => fixed_body::message_with_padding(
                Opcode::DestroyZoneOfControl,
                &ZoneDestroyWire { id },
                ZONE_DESTROY_PREFIX_LEN,
                suffix,
            ),
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
