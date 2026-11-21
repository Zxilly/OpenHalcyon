//! Native attribute (0x041C) and resource (0x041D) operations.
use super::{Message, Opcode, fixed_body::fixed_packet};
use binrw::{BinRead, BinWrite};

#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct AttributeChange {
    pub actor: u32,
    pub source: u32,
    pub value: f32,
    pub index: u8,
    pub layer: u8,
    /// Zero adds and nonzero sets. Retain the original byte.
    pub set_mode: u8,
}
fixed_packet!(AttributeChange, Opcode::ModifyActorAttribute, 15);

impl AttributeChange {
    /// Reads the 14-byte `Fields::StatModifier` prefix (`actor`, `source`, `value`, `index`, `layer`).
    pub fn read_stat_modifier(body: &[u8]) -> std::io::Result<StatModifierPrefix> {
        crate::fixed_body::read(body)
    }

    pub fn read(msg: &Message) -> Option<Self> {
        msg.is(Opcode::ModifyActorAttribute)
            .then(|| Self::read_exact_or_padded(&msg.body).ok())
            .flatten()
    }

    pub fn message(self) -> Message {
        self.encode_padded()
    }
}

/// 14-byte prefix of `0x041C` (`Opcode::ModifyActorAttribute`) excluding `set_mode`.
#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct StatModifierPrefix {
    pub actor: u32,
    pub source: u32,
    pub value: f32,
    pub index: u8,
    pub layer: u8,
}

/// 9-byte prefix of `0x041D` (`Opcode::ModifyActorResource`) up through the resource `index`.
#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct EntityStatPrefix {
    pub actor: u32,
    pub value: f32,
    pub index: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct ResourceChange {
    pub actor: u32,
    pub value: f32,
    pub index: u8,
    /// Zero adds and nonzero sets. The other two flag bytes are independent.
    pub set_mode: u8,
    pub flags: [u8; 2],
}
fixed_packet!(ResourceChange, Opcode::ModifyActorResource, 12);

impl ResourceChange {
    /// Reads the 9-byte `Fields::EntityStat` prefix (`actor`, `value`, `index`).
    pub fn read_entity_stat_prefix(body: &[u8]) -> std::io::Result<EntityStatPrefix> {
        crate::fixed_body::read(body)
    }

    pub fn read(msg: &Message) -> Option<Self> {
        msg.is(Opcode::ModifyActorResource)
            .then(|| Self::read_exact_or_padded(&msg.body).ok())
            .flatten()
    }

    pub fn message(self) -> Message {
        self.encode_padded()
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
