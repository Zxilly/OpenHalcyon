use super::{Message, Opcode, fixed_body};
use binrw::{BinRead, BinWrite};

const ORBIT_START_PREFIX_LEN: usize = 32;
const ORBIT_CANCEL_PREFIX_LEN: usize = 16;

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

impl Message {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
