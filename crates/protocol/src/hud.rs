//! Map pings and HUD quick messages from the recorded server stream.
use crate::{
    Message, Opcode,
    fixed_body::{self, fixed_packet},
};
use binrw::{BinRead, BinWrite};

#[derive(Debug, Clone, Copy, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct MapPing {
    pub actor: u32,
    #[br(map = target)]
    #[bw(map = |value: &Option<u32>| value.unwrap_or(u32::MAX))]
    pub target: Option<u32>,
    pub position: [f32; 3],
    pub selector: u8,
    pub qualifier: u8,
}
fixed_packet!(MapPing, Opcode::Ping, 22);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HudQuickMessage {
    pub actor: u32,
    pub selector: u8,
    pub target: Option<u32>,
    pub value: f32,
    pub operand: u32,
    pub padding: Option<[u8; 5]>,
}

#[derive(Debug, Clone, Copy, PartialEq, BinRead, BinWrite)]
#[brw(big)]
struct HudQuickWire {
    actor: u32,
    selector: u8,
    #[br(map = target)]
    #[bw(map = |value: &Option<u32>| value.unwrap_or(u32::MAX))]
    target: Option<u32>,
    value: f32,
    operand: u32,
}

const HUD_QUICK_PREFIX_LEN: usize = 17;

fn target(raw: u32) -> Option<u32> {
    (raw != u32::MAX).then_some(raw)
}

impl MapPing {
    pub(super) fn decode(body: &[u8]) -> Option<Self> {
        Self::read_exact(body).ok()
    }
    pub fn message(&self) -> Message {
        self.encode()
    }
}

impl HudQuickMessage {
    /// In the two recorded item-message forms, the final operand names an item
    /// template. Keep all other selectors and operands available to the caller.
    pub fn item_template(&self) -> Option<u32> {
        match (self.selector, self.target) {
            (0, Some(_)) if self.value == 0.0 => Some(self.operand),
            (1, None) if self.value.is_finite() && self.value > 0.0 => Some(self.operand),
            _ => None,
        }
    }

    pub(super) fn decode(body: &[u8]) -> Option<Self> {
        let (wire, tail): (HudQuickWire, _) =
            fixed_body::read_with_padding(body, HUD_QUICK_PREFIX_LEN).ok()?;
        let padding = match tail {
            [] => None,
            data => Some(data.try_into().ok()?),
        };
        Some(Self {
            actor: wire.actor,
            selector: wire.selector,
            target: wire.target,
            value: wire.value,
            operand: wire.operand,
            padding,
        })
    }
    pub fn message(&self) -> Message {
        let wire = HudQuickWire {
            actor: self.actor,
            selector: self.selector,
            target: self.target,
            value: self.value,
            operand: self.operand,
        };
        let suffix = self.padding.as_ref().map_or(&[][..], |pad| &pad[..]);
        fixed_body::message_with_padding(
            Opcode::HUDQuickMessage,
            &wire,
            HUD_QUICK_PREFIX_LEN,
            suffix,
        )
        .expect("HUD quick padding is 0 or 5 bytes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Fields, LiveFields};
    #[test]
    fn only_item_forms_expose_a_template() {
        let mut q = HudQuickMessage {
            actor: 1501,
            selector: 0,
            target: Some(1502),
            value: 0.,
            operand: 463,
            padding: None,
        };
        assert_eq!(q.item_template(), Some(463));
        q.target = None;
        assert_eq!(q.item_template(), None);
        q.selector = 1;
        q.value = 3.5;
        assert_eq!(q.item_template(), Some(463));
        q.selector = 2;
        assert_eq!(q.item_template(), None);
    }
    #[test]
    fn ping_coordinates_and_unaligned_hud_operands_roundtrip() {
        let ping = Message::known(
            crate::Opcode::Ping,
            [
                0, 0, 5, 220, 255, 255, 255, 255, 0xc0, 0, 0, 0, 0x80, 0, 0, 0, 0x3f, 0x80, 0, 0,
                5, 1,
            ],
        );
        let Fields::Live(LiveFields::MapPing(p)) = ping.fields().unwrap() else {
            panic!()
        };
        assert_eq!(
            (p.actor, p.target, p.selector, p.qualifier),
            (1500, None, 5, 1)
        );
        assert_eq!(
            p.position.map(f32::to_bits),
            [(-2f32).to_bits(), (-0f32).to_bits(), 1f32.to_bits()]
        );
        assert_eq!(p.message(), ping);
        let mut raw = vec![0, 0, 5, 221, 1, 0, 0, 5, 220, 0x7f, 0xc0, 1, 9, 0, 0, 2, 14];
        for padded in [false, true] {
            if padded {
                raw.extend([1, 3, 5, 7, 9]);
            }
            let m = Message::known(crate::Opcode::HUDQuickMessage, raw.clone());
            let Fields::Live(LiveFields::HudQuickMessage(q)) = m.fields().unwrap() else {
                panic!()
            };
            assert_eq!(
                (q.actor, q.selector, q.target, q.value.to_bits(), q.operand),
                (1501, 1, Some(1500), 0x7fc00109, 526)
            );
            assert_eq!(q.message(), m);
        }
    }
}
