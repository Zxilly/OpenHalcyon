use crate::{Message, Opcode};

/// Values observed in the CE build-path selector. The special value's UI
/// meaning is unknown, so keep it distinct from the three numbered slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::FromRepr)]
#[repr(u8)]
pub enum RecommendedBuildPath {
    Slot0 = 0,
    Slot1 = 1,
    Slot2 = 2,
    SpecialFe = 0xfe,
}

impl RecommendedBuildPath {
    pub fn announcement(self) -> Message {
        Message::known(Opcode::RequestRecommendedBuildPath, [self as u8])
    }

    pub fn applied_to(self, actor: u32) -> Message {
        let mut body = [0; 5];
        body[..4].copy_from_slice(&actor.to_be_bytes());
        body[4] = self as u8;
        Message::known(Opcode::SetRecommendedBuildPath, body)
    }
}
