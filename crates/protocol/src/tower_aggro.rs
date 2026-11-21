//! Observed tower form of ActionSendAggroState_Client in ARAL and Android Solo.
use crate::{Message, Opcode, fixed_body::fixed_packet};
use binrw::{BinRead, BinWrite};

const OBSERVED_RANGE_BITS: u32 = 9_f32.to_bits();

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

#[cfg(test)]
mod tests {
    use super::*;
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
