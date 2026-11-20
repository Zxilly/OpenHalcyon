//! Native movement actions carry either a scene registration slot or full actor IDs.
use crate::{Fields, Message};

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
                (0x03f9, values, Some(flag), remaining)
            }
            Self::MoveAndFace {
                actor,
                reference,
                values,
                remaining,
            } if matches!(remaining.len(), 0 | 2) => {
                body.extend(actor.to_be_bytes());
                body.extend(reference.to_be_bytes());
                (0x03fa, values, None, remaining)
            }
            Self::StopActor {
                actor,
                values,
                flag,
                remaining,
            } if matches!(remaining.len(), 0 | 5) => {
                body.extend(actor.to_be_bytes());
                (0x03fb, values, Some(flag), remaining)
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
    fn native_and_transport_lengths_keep_flags_signed_zero_and_padding() {
        let bits = [0xbf800000_u32, 0x80000000, 0x7fc00109];
        let xyz = bits.map(u32::to_be_bytes).concat();
        for (op, prefix, flag, padding) in [
            (0x3f9, vec![19], Some(0x7c), vec![]),
            (
                0x3fa,
                vec![0, 0, 5, 220, 0, 0, 5, 221],
                None,
                vec![0xaa, 0xbb],
            ),
            (0x3fb, vec![0, 0, 5, 220], Some(0x38), vec![1, 2, 3, 4, 5]),
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
