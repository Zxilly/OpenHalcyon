//! Zones of control, actor-anchored orbits, and tower aggro state wire codecs.

use crate::codec::fixed_body::{self, fixed_packet};
use crate::{Message, Opcode};
use binrw::{BinRead, BinWrite};
use std::io;

const ZONE_CREATE_PREFIX_LEN: usize = 25;
const ZONE_RECT_PREFIX_LEN: usize = 24;
const ZONE_DESTROY_PREFIX_LEN: usize = 4;
const ORBIT_START_PREFIX_LEN: usize = 32;
const ORBIT_CANCEL_PREFIX_LEN: usize = 16;
const OBSERVED_RANGE_BITS: u32 = 9_f32.to_bits();

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

/// Recorded actor-anchored orbit coordinates, preserving original float bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct OrbitPosition {
    pub x_bits: u32,
    pub y_bits: u32,
    pub z_bits: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
struct OrbitStartWire {
    actor: u32,
    unknown: [u32; 2],
    center: u32,
    parameter: u32,
    position: OrbitPosition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
struct OrbitCancelWire {
    actor: u32,
    position: OrbitPosition,
}

#[derive(Debug, PartialEq, Eq)]
pub enum OrbitAction<'a> {
    Start {
        actor: u32,
        unknown: [u32; 2],
        center: u32,
        parameter: u32,
        position: OrbitPosition,
        padding: Option<&'a [u8; 6]>,
    },
    Cancel {
        actor: u32,
        position: OrbitPosition,
        padding: Option<&'a [u8; 6]>,
    },
}

fn opt_padding(tail: &[u8]) -> Option<Option<&[u8; 6]>> {
    match tail {
        [] => Some(None),
        data => Some(Some(data.try_into().ok()?)),
    }
}

impl OrbitAction<'_> {
    pub fn to_message(&self) -> Message {
        match *self {
            Self::Start {
                actor,
                unknown,
                center,
                parameter,
                position,
                padding,
            } => {
                let suffix = padding.map_or(&[][..], |v| &v[..]);
                fixed_body::message_with_padding(
                    Opcode::AutoOrbit,
                    &OrbitStartWire {
                        actor,
                        unknown,
                        center,
                        parameter,
                        position,
                    },
                    ORBIT_START_PREFIX_LEN,
                    suffix,
                )
                .expect("orbit start padding is 0 or 6 bytes")
            }
            Self::Cancel {
                actor,
                position,
                padding,
            } => {
                let suffix = padding.map_or(&[][..], |v| &v[..]);
                fixed_body::message_with_padding(
                    Opcode::CancelAutoOrbit,
                    &OrbitCancelWire { actor, position },
                    ORBIT_CANCEL_PREFIX_LEN,
                    suffix,
                )
                .expect("orbit cancel padding is 0 or 6 bytes")
            }
        }
    }
}

/// Observed tower form of ActionSendAggroState_Client in ARAL and Android Solo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TowerAggro {
    pub tower_slot: u8,
    pub target_slot: Option<u8>,
}

/// Raw 8-byte `0x0417` (`Opcode::SendAggroState`) wire prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct SendAggroStateBody {
    pub tower_slot: u8,
    pub target_slot: u8,
    pub range_bits: u32,
    pub has_target: u8,
    pub reserved: u8,
}
fixed_packet!(SendAggroStateBody, Opcode::SendAggroState, 8);

impl SendAggroStateBody {
    /// Returns the big-endian float word at bytes `2..6`.
    pub const fn value(self) -> f32 {
        f32::from_bits(self.range_bits)
    }
}

impl TowerAggro {
    pub fn decode(message: &Message) -> Option<Self> {
        if !message.is(Opcode::SendAggroState) {
            return None;
        }
        let (wire, tail) = SendAggroStateBody::read_with_padding(&message.body).ok()?;
        if wire.range_bits != OBSERVED_RANGE_BITS
            || wire.reserved != 0
            || tail.iter().any(|&byte| byte != 0)
            || wire.tower_slot == u8::MAX
        {
            return None;
        }
        let target_slot = match (wire.target_slot, wire.has_target) {
            (255, 0) => None,
            (slot @ 0..=254, 1) => Some(slot),
            _ => return None,
        };
        Some(Self {
            tower_slot: wire.tower_slot,
            target_slot,
        })
    }

    pub fn message(self) -> Option<Message> {
        if self.tower_slot == 255 || self.target_slot == Some(255) {
            return None;
        }
        Some(
            SendAggroStateBody {
                tower_slot: self.tower_slot,
                target_slot: self.target_slot.unwrap_or(255),
                range_bits: OBSERVED_RANGE_BITS,
                has_target: u8::from(self.target_slot.is_some()),
                reserved: 0,
            }
            .encode_padded(),
        )
    }
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

    pub fn orbit_action(&self) -> Option<OrbitAction<'_>> {
        match self.known_opcode()? {
            Opcode::AutoOrbit => {
                let (wire, tail): (OrbitStartWire, _) =
                    fixed_body::read_with_padding(&self.body, ORBIT_START_PREFIX_LEN).ok()?;
                Some(OrbitAction::Start {
                    actor: wire.actor,
                    unknown: wire.unknown,
                    center: wire.center,
                    parameter: wire.parameter,
                    position: wire.position,
                    padding: opt_padding(tail)?,
                })
            }
            Opcode::CancelAutoOrbit => {
                let (wire, tail): (OrbitCancelWire, _) =
                    fixed_body::read_with_padding(&self.body, ORBIT_CANCEL_PREFIX_LEN).ok()?;
                Some(OrbitAction::Cancel {
                    actor: wire.actor,
                    position: wire.position,
                    padding: opt_padding(tail)?,
                })
            }
            _ => None,
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
        let m = Message::known(Opcode::CreateRectZoneOfControl, body);
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

    #[test]
    fn orbit_actions_preserve_actor_roles_coordinates_and_aligned_tails() {
        for (opcode, prefix) in [(Opcode::AutoOrbit, 32), (Opcode::CancelAutoOrbit, 16)] {
            for len in 0..=40 {
                let body: Vec<_> = (0..len).map(|n| 0xff - n as u8).collect();
                let m = Message::known(opcode, body);
                let action = m.orbit_action();
                if len != prefix && len != prefix + 6 {
                    assert!(action.is_none());
                    continue;
                }
                let action = action.unwrap();
                match action {
                    OrbitAction::Start {
                        actor,
                        center,
                        position,
                        ..
                    } => {
                        assert_eq!(
                            (actor, center, position.x_bits),
                            (0xfffefdfc, 0xf3f2f1f0, 0xebeae9e8)
                        );
                    }
                    OrbitAction::Cancel {
                        actor, position, ..
                    } => {
                        assert_eq!((actor, position.z_bits), (0xfffefdfc, 0xf3f2f1f0));
                    }
                }
                assert_eq!(action.to_message(), m);
            }
        }
    }

    #[test]
    fn requires_canonical_target_flag_and_trailing_bytes() {
        for target in [None, Some(17)] {
            let state = TowerAggro {
                tower_slot: 7,
                target_slot: target,
            };
            let mut frame = state.message().unwrap();
            assert_eq!(TowerAggro::decode(&frame), Some(state));
            frame.body[6] ^= 1;
            assert!(TowerAggro::decode(&frame).is_none());
            frame.body[6] ^= 1;
            frame.body[8] = 1;
            assert!(TowerAggro::decode(&frame).is_none());
            frame.body.truncate(8);
            assert_eq!(TowerAggro::decode(&frame), Some(state));
            frame.body[7] = 1;
            assert!(TowerAggro::decode(&frame).is_none());
        }
    }
}
