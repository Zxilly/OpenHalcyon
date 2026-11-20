//! Map pings and HUD quick messages from the recorded server stream.
use crate::Message;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapPing {
    pub actor: u32,
    pub target: Option<u32>,
    pub position: [f32; 3],
    pub selector: u8,
    pub qualifier: u8,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HudQuickMessage {
    pub actor: u32,
    pub selector: u8,
    pub target: Option<u32>,
    pub value: f32,
    pub operand: u32,
    pub padding: Option<[u8; 5]>,
}

fn target(raw: u32) -> Option<u32> {
    (raw != u32::MAX).then_some(raw)
}

fn word(body: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(body[offset..offset + 4].try_into().unwrap())
}

impl MapPing {
    pub(super) fn decode(body: &[u8]) -> Option<Self> {
        if body.len() != 22 {
            return None;
        }
        Some(Self {
            actor: word(body, 0),
            target: target(word(body, 4)),
            position: std::array::from_fn(|i| f32::from_bits(word(body, 8 + 4 * i))),
            selector: body[20],
            qualifier: body[21],
        })
    }
    pub fn message(&self) -> Message {
        let mut body = [0; 22];
        body[..4].copy_from_slice(&self.actor.to_be_bytes());
        body[4..8].copy_from_slice(&self.target.unwrap_or(u32::MAX).to_be_bytes());
        for (dest, value) in body[8..20]
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(self.position)
        {
            *dest = value.to_be_bytes();
        }
        body[20] = self.selector;
        body[21] = self.qualifier;
        Message::known(crate::Opcode::Ping, body)
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
        if !matches!(body.len(), 17 | 22) {
            return None;
        }
        Some(Self {
            actor: word(body, 0),
            selector: body[4],
            target: target(word(body, 5)),
            value: f32::from_bits(word(body, 9)),
            operand: word(body, 13),
            padding: body[17..].try_into().ok(),
        })
    }
    pub fn message(&self) -> Message {
        let mut body = Vec::with_capacity(22);
        body.extend(self.actor.to_be_bytes());
        body.push(self.selector);
        body.extend(self.target.unwrap_or(u32::MAX).to_be_bytes());
        body.extend(self.value.to_be_bytes());
        body.extend(self.operand.to_be_bytes());
        if let Some(pad) = self.padding {
            body.extend(pad);
        }
        Message::known(crate::Opcode::HUDQuickMessage, body)
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
