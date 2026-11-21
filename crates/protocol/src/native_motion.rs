//! Native movement actions carry either a scene registration slot or full actor IDs.
use crate::{Fields, Message, Opcode, fixed_body::fixed_packet};
use binrw::{BinRead, BinWrite};

/// Supported 0403 displacement profiles seen on the ARAL wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big, repr = u8)]
#[repr(u8)]
pub enum ForcedMoveMode {
    Dash = 0,
    Knockback = 1,
}

#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct ForcedMove {
    pub actor: u32,
    pub speed: f32,
    pub destination: [f32; 3],
    pub mode: ForcedMoveMode,
}
fixed_packet!(ForcedMove, Opcode::AutoMoveTo, 21);

/// 16-byte body of `0x0402` (`Opcode::AutoActorBounce`), padded to 22 bytes when aligned.
#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct ActorBounceBody {
    pub actor: u32,
    pub height: f32,
    pub duration: f32,
    pub enabled: u32,
}
fixed_packet!(ActorBounceBody, Opcode::AutoActorBounce, 16);

/// 14-byte body of `0x03F9` (`Opcode::MoveToServerAuthoritative`).
#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct ServerMoveBody {
    pub selector: u8,
    pub values: [f32; 3],
    pub flag: u8,
}
fixed_packet!(ServerMoveBody, Opcode::MoveToServerAuthoritative, 14);

/// 20-byte body of `0x03FA` (`Opcode::MoveAndFace`), padded to 22 bytes when aligned.
#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct MoveAndFaceBody {
    pub actor: u32,
    pub reference: u32,
    pub values: [f32; 3],
}
fixed_packet!(MoveAndFaceBody, Opcode::MoveAndFace, 20);

/// 17-byte body of `0x03FB` (`Opcode::StopActor`), padded to 22 bytes when aligned.
#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct StopActorBody {
    pub actor: u32,
    pub values: [f32; 3],
    pub flag: u8,
}
fixed_packet!(StopActorBody, Opcode::StopActor, 17);

impl ForcedMove {
    pub fn message(self) -> Message {
        self.encode()
    }
}

impl Message {
    /// Karas dash and knockback captures place speed before the destination.
    /// The imported operator layout retains its original labels in spec_view().
    pub fn forced_move(&self) -> Option<ForcedMove> {
        self.is(Opcode::AutoMoveTo)
            .then(|| ForcedMove::read_exact_or_padded(&self.body).ok())
            .flatten()
    }

    /// Native enabled bounce applied to an affected actor.
    pub fn actor_bounce(actor: u32, height: f32, duration: f32) -> Self {
        ActorBounceBody {
            actor,
            height,
            duration,
            enabled: 1,
        }
        .encode()
    }
}

impl Fields<'_> {
    /// Rebuild one of the three native movement actions at its original body
    /// length, including any caller-preserved transport padding.
    pub fn movement_message(&self) -> Option<Message> {
        match *self {
            Self::ServerMove {
                selector,
                values,
                flag,
                remaining,
            } => ServerMoveBody {
                selector,
                values,
                flag,
            }
            .encode_with_padding(remaining)
            .ok(),
            Self::MoveAndFace {
                actor,
                reference,
                values,
                remaining,
            } => MoveAndFaceBody {
                actor,
                reference,
                values,
            }
            .encode_with_padding(remaining)
            .ok(),
            Self::StopActor {
                actor,
                values,
                flag,
                remaining,
            } => StopActorBody {
                actor,
                values,
                flag,
            }
            .encode_with_padding(remaining)
            .ok(),
            _ => None,
        }
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
            (
                crate::wire_opcode::MoveToServerAuthoritative,
                vec![19],
                Some(0x7c),
                vec![],
            ),
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
