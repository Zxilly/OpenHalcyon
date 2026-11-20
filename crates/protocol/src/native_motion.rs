//! Native movement actions carry either a scene registration slot or full actor IDs.
use crate::{Fields, Message, Opcode};

/// Supported 0403 displacement profiles seen on the ARAL wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ForcedMoveMode {
    Dash = 0,
    Knockback = 1,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ForcedMove {
    pub actor: u32,
    pub speed: f32,
    pub destination: [f32; 3],
    pub mode: ForcedMoveMode,
}

impl ForcedMove {
    pub fn message(self) -> Message {
        let mut body = [0; 21];
        body[..4].copy_from_slice(&self.actor.to_be_bytes());
        for (bytes, value) in body[4..20].chunks_exact_mut(4).zip([
            self.speed,
            self.destination[0],
            self.destination[1],
            self.destination[2],
        ]) {
            bytes.copy_from_slice(&value.to_be_bytes());
        }
        body[20] = self.mode as u8;
        Message::known(Opcode::AutoMoveTo, body)
    }
}

impl Message {
    /// Karas dash and knockback captures place speed before the destination.
    /// The imported operator layout retains its original labels in spec_view().
    pub fn forced_move(&self) -> Option<ForcedMove> {
        let b = &self.body;
        if !self.is(Opcode::AutoMoveTo) || !matches!(b.len(), 21 | 22) {
            return None;
        }
        let word = |i| u32::from_be_bytes(b[i..i + 4].try_into().unwrap());
        Some(ForcedMove {
            actor: word(0),
            speed: f32::from_bits(word(4)),
            destination: std::array::from_fn(|i| f32::from_bits(word(8 + 4 * i))),
            mode: match b[20] {
                0 => ForcedMoveMode::Dash,
                1 => ForcedMoveMode::Knockback,
                _ => return None,
            },
        })
    }

    /// Native enabled bounce applied to an affected actor.
    pub fn actor_bounce(actor: u32, height: f32, duration: f32) -> Self {
        let mut body = [0; 16];
        for (bytes, word) in
            body.chunks_exact_mut(4)
                .zip([actor, height.to_bits(), duration.to_bits(), 1])
        {
            bytes.copy_from_slice(&word.to_be_bytes());
        }
        Self::known(Opcode::AutoActorBounce, body)
    }
}

impl Fields<'_> {
    /// Rebuild one of the three native movement actions at its original body
    /// length, including any caller-preserved transport padding.
    pub fn movement_message(&self) -> Option<Message> {
        let mut body = Vec::with_capacity(22);
        let (opcode, position, flag, padding) = match self {
            Self::ServerMove {
                selector,
                values,
                flag,
                remaining,
            } if remaining.is_empty() => {
                body.push(*selector);
                (
                    crate::wire_opcode::EntityAct2,
                    values,
                    Some(flag),
                    remaining,
                )
            }
            Self::MoveAndFace {
                actor,
                reference,
                values,
                remaining,
            } if matches!(remaining.len(), 0 | 2) => {
                body.extend(actor.to_be_bytes());
                body.extend(reference.to_be_bytes());
                (crate::wire_opcode::MoveAndFace, values, None, remaining)
            }
            Self::StopActor {
                actor,
                values,
                flag,
                remaining,
            } if matches!(remaining.len(), 0 | 5) => {
                body.extend(actor.to_be_bytes());
                (crate::wire_opcode::StopActor, values, Some(flag), remaining)
            }
            _ => return None,
        };
        for v in position {
            body.extend(v.to_be_bytes());
        }
        if let Some(flag) = flag {
            body.push(*flag);
        }
        body.extend_from_slice(padding);
        Some(Message::new(opcode, body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn forced_motion_matches_native_capture_operands() {
        let capture = Message::known(
            Opcode::AutoMoveTo,
            [
                0, 0, 5, 0xe0, 0x41, 0x5b, 0x22, 0xfa, 0x41, 0xb3, 0x84, 5, 0x3d, 0x19, 0x2e, 0x1f,
                0x40, 0x81, 0x75, 0xfa, 0, 0,
            ],
        );
        let displacement = capture.forced_move().unwrap();
        assert_eq!(displacement.mode, ForcedMoveMode::Dash);
        assert_eq!(displacement.speed.to_bits(), 0x415b22fa);
        assert_eq!(displacement.destination[1].to_bits(), 0x3d192e1f);
        assert_eq!(displacement.message().body, capture.body[..21]);
        let bounce = Message::actor_bounce(1503, 6., 5.);
        assert_eq!(
            bounce.body,
            [
                0, 0, 5, 0xdf, 0x40, 0xc0, 0, 0, 0x40, 0xa0, 0, 0, 0, 0, 0, 1
            ]
        );
    }

    #[test]
    fn native_and_transport_lengths_keep_flags_signed_zero_and_padding() {
        let bits = [0xbf800000_u32, 0x80000000, 0x7fc00109];
        let xyz = bits.map(u32::to_be_bytes).concat();
        for (op, prefix, flag, padding) in [
            (crate::wire_opcode::EntityAct2, vec![19], Some(0x7c), vec![]),
            (
                crate::wire_opcode::MoveAndFace,
                vec![0, 0, 5, 220, 0, 0, 5, 221],
                None,
                vec![0xaa, 0xbb],
            ),
            (
                crate::wire_opcode::StopActor,
                vec![0, 0, 5, 220],
                Some(0x38),
                vec![1, 2, 3, 4, 5],
            ),
        ] {
            let mut raw = prefix;
            raw.extend(&xyz);
            raw.extend(flag);
            for pad in [&[][..], padding.as_slice()] {
                let mut body = raw.clone();
                body.extend(pad);
                let m = Message::new(op, body);
                let fields = m.fields().unwrap();
                match &fields {
                    Fields::ServerMove {
                        selector,
                        values,
                        flag,
                        remaining,
                    } => {
                        assert_eq!((*selector, *flag, *remaining), (19, 0x7c, pad));
                        assert_eq!(values.map(f32::to_bits), bits);
                    }
                    Fields::MoveAndFace {
                        actor,
                        reference,
                        values,
                        remaining,
                    } => {
                        assert_eq!((*actor, *reference, *remaining), (1500, 1501, pad));
                        assert_eq!(values.map(f32::to_bits), bits);
                    }
                    Fields::StopActor {
                        actor,
                        values,
                        flag,
                        remaining,
                    } => {
                        assert_eq!((*actor, *flag, *remaining), (1500, 0x38, pad));
                        assert_eq!(values.map(f32::to_bits), bits);
                    }
                    _ => panic!("missing typed movement view"),
                }
                assert_eq!(fields.movement_message(), Some(m));
            }
        }
    }
}
