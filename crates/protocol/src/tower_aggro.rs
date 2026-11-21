//! Observed tower form of ActionSendAggroState_Client in ARAL and Android Solo.
use crate::{Message, Opcode, fixed_body::fixed_packet};
use binrw::{BinRead, BinWrite};

const OBSERVED_RANGE_BITS: u32 = 9_f32.to_bits();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TowerAggro {
    pub tower_slot: u8,
    pub target_slot: Option<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
struct TowerAggroWire {
    tower_slot: u8,
    target_slot: u8,
    range_bits: u32,
    has_target: u8,
    reserved: u8,
}
fixed_packet!(TowerAggroWire, Opcode::SendAggroState, 8);

impl TowerAggro {
    pub fn decode(message: &Message) -> Option<Self> {
        if !message.is(Opcode::SendAggroState) {
            return None;
        }
        let (wire, tail) = TowerAggroWire::read_with_padding(&message.body).ok()?;
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
            TowerAggroWire {
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
