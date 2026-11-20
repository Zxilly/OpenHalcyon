use super::{Message, Opcode};

/// Recorded actor-anchored orbit coordinates, preserving original float bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrbitPosition {
    pub x_bits: u32,
    pub y_bits: u32,
    pub z_bits: u32,
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

impl Message {
    pub fn orbit_action(&self) -> Option<OrbitAction<'_>> {
        let n = match self.opcode {
            crate::wire_opcode::AutoOrbit => 32,
            crate::wire_opcode::CancelAutoOrbit => 16,
            _ => return None,
        };
        let (operands, tail) = self.body.split_at_checked(n)?;
        let padding = match tail {
            [] => None,
            data => Some(data.try_into().ok()?),
        };
        let word = |w: &[u8; 4]| u32::from_be_bytes(*w);
        let (header, coords) = operands.as_chunks::<4>().0.split_last_chunk::<3>()?;
        let [x_bits, y_bits, z_bits] = coords.map(u32::from_be_bytes);
        let position = OrbitPosition {
            x_bits,
            y_bits,
            z_bits,
        };
        match header {
            [actor, a, b, center, parameter] => Some(OrbitAction::Start {
                actor: word(actor),
                unknown: [word(a), word(b)],
                center: word(center),
                parameter: word(parameter),
                position,
                padding,
            }),
            [actor] => Some(OrbitAction::Cancel {
                actor: word(actor),
                position,
                padding,
            }),
            _ => None,
        }
    }
}

impl OrbitAction<'_> {
    pub fn to_message(&self) -> Message {
        let mut body = Vec::new();
        let (opcode, position, padding) = match *self {
            Self::Start {
                actor,
                unknown: [a, b],
                center,
                parameter,
                position,
                padding,
            } => {
                body.reserve_exact(32 + padding.map_or(0, |v| v.len()));
                for w in [actor, a, b, center, parameter] {
                    body.extend(w.to_be_bytes());
                }
                (Opcode::AutoOrbit, position, padding)
            }
            Self::Cancel {
                actor,
                position,
                padding,
            } => {
                body.reserve_exact(16 + padding.map_or(0, |v| v.len()));
                body.extend(actor.to_be_bytes());
                (Opcode::CancelAutoOrbit, position, padding)
            }
        };
        for coord in [position.x_bits, position.y_bits, position.z_bits] {
            body.extend(coord.to_be_bytes());
        }
        if let Some(bytes) = padding {
            body.extend(bytes);
        }
        Message::known(opcode, body)
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
