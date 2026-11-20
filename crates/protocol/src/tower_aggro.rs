//! Observed tower form of ActionSendAggroState_Client in ARAL and Android Solo.
use crate::Message;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TowerAggro {
    pub tower_slot: u8,
    pub target_slot: Option<u8>,
}

impl TowerAggro {
    pub fn decode(message: &Message) -> Option<Self> {
        let b = &message.body;
        if !message.is(crate::Opcode::AggroState)
            || !matches!(b.len(), 8 | 14)
            || b[2..6] != 9_f32.to_be_bytes()
            || b[7..].iter().any(|&byte| byte != 0)
            || b[0] == u8::MAX
        {
            return None;
        }
        let target_slot = match (b[1], b[6]) {
            (255, 0) => None,
            (slot @ 0..=254, 1) => Some(slot),
            _ => return None,
        };
        Some(Self {
            tower_slot: b[0],
            target_slot,
        })
    }

    pub fn message(self) -> Option<Message> {
        if self.tower_slot == 255 || self.target_slot == Some(255) {
            return None;
        }
        let mut body = [0; 14];
        body[0] = self.tower_slot;
        body[1] = self.target_slot.unwrap_or(255);
        body[2..6].copy_from_slice(&9_f32.to_be_bytes());
        body[6] = u8::from(self.target_slot.is_some());
        Some(Message::known(crate::Opcode::AggroState, body))
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
