//! Native attribute (0x041C) and resource (0x041D) operations.
use super::{Message, Opcode};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AttributeChange {
    pub actor: u32,
    pub source: u32,
    pub value: f32,
    pub index: u8,
    pub layer: u8,
    /// Zero adds and nonzero sets. Retain the original byte.
    pub set_mode: u8,
}

impl AttributeChange {
    pub fn read(msg: &Message) -> Option<Self> {
        let b = &msg.body;
        if msg.opcode != Opcode::StatModifier as u16 || !matches!(b.len(), 15 | 22) {
            return None;
        }
        Some(Self {
            actor: u32::from_be_bytes(b[..4].try_into().unwrap()),
            source: u32::from_be_bytes(b[4..8].try_into().unwrap()),
            value: f32::from_be_bytes(b[8..12].try_into().unwrap()),
            index: b[12],
            layer: b[13],
            set_mode: b[14],
        })
    }

    pub fn message(self) -> Message {
        let mut b = [0; 22];
        b[..4].copy_from_slice(&self.actor.to_be_bytes());
        b[4..8].copy_from_slice(&self.source.to_be_bytes());
        b[8..12].copy_from_slice(&self.value.to_be_bytes());
        b[12..15].copy_from_slice(&[self.index, self.layer, self.set_mode]);
        Message::new(Opcode::StatModifier as u16, b)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResourceChange {
    pub actor: u32,
    pub value: f32,
    pub index: u8,
    /// Zero adds and nonzero sets. The other two flag bytes are independent.
    pub set_mode: u8,
    pub flags: [u8; 2],
}

impl ResourceChange {
    pub fn read(msg: &Message) -> Option<Self> {
        let b = &msg.body;
        if msg.opcode != Opcode::EntityStat as u16 || !matches!(b.len(), 12 | 14) {
            return None;
        }
        Some(Self {
            actor: u32::from_be_bytes(b[..4].try_into().unwrap()),
            value: f32::from_be_bytes(b[4..8].try_into().unwrap()),
            index: b[8],
            set_mode: b[9],
            flags: [b[10], b[11]],
        })
    }

    pub fn message(self) -> Message {
        let mut b = [0; 14];
        b[..4].copy_from_slice(&self.actor.to_be_bytes());
        b[4..8].copy_from_slice(&self.value.to_be_bytes());
        b[8..12].copy_from_slice(&[self.index, self.set_mode, self.flags[0], self.flags[1]]);
        Message::new(Opcode::EntityStat as u16, b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_mutations_keep_full_actor_and_distinct_modes_layers_and_flags() {
        let resource = ResourceChange {
            actor: 0x12345678,
            value: -150.,
            index: 6,
            set_mode: 0,
            flags: [1, 0x81],
        };
        let mut wire = resource.message();
        assert_eq!(&wire.body[8..], &[6, 0, 1, 0x81, 0, 0]);
        assert_eq!(ResourceChange::read(&wire), Some(resource));
        wire.body.truncate(12);
        assert_eq!(ResourceChange::read(&wire), Some(resource));
        wire.body.pop();
        assert_eq!(ResourceChange::read(&wire), None);
        let attribute = AttributeChange {
            actor: 1500,
            source: u32::MAX,
            value: 90.,
            index: 39,
            layer: 0,
            set_mode: 1,
        };
        let mut wire = attribute.message();
        assert_eq!(&wire.body[12..15], &[39, 0, 1]);
        assert_eq!(AttributeChange::read(&wire), Some(attribute));
        wire.body.truncate(15);
        assert_eq!(AttributeChange::read(&wire), Some(attribute));
        wire.body.pop();
        assert_eq!(AttributeChange::read(&wire), None);
    }
}
