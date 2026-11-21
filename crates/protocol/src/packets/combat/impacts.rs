//! Health impacts, actor deaths, stat/resource mutations, gold baselines, target locks, and AFK/deserter flags.

use crate::codec::fixed_body::{self, fixed_packet, fixed_prefix};
use crate::codec::invalid;
use crate::{Message, Opcode};
use binrw::{BinRead, BinWrite};
use std::io;

#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct DeathBody {
    pub victim: u32,
    pub source: u32,
}
fixed_prefix!(DeathBody, 8);

#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct ImpactBody {
    pub target: u32,
    pub source: u32,
    pub operand: f32,
    pub word: u16,
    pub byte: u8,
    pub flag: u8,
}
fixed_prefix!(ImpactBody, 16);

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

/// Typed alias for [`AttributeChange`] (`0x041c ModifyActorAttribute`).
pub type ActorAttributeBody = AttributeChange;

impl AttributeChange {
    /// Reads the 14-byte `Fields::StatModifier` prefix (`actor`, `source`, `value`, `index`, `layer`).
    pub fn read_stat_modifier(body: &[u8]) -> io::Result<StatModifierPrefix> {
        fixed_body::read(body)
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

/// Typed alias for [`ResourceChange`] (`0x041d ModifyActorResource`).
pub type EntityStatBody = ResourceChange;

impl ResourceChange {
    /// Reads the 9-byte `Fields::EntityStat` prefix (`actor`, `value`, `index`).
    pub fn read_entity_stat_prefix(body: &[u8]) -> io::Result<EntityStatPrefix> {
        fixed_body::read(body)
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

/// `0x0425 ModifyBasicAttackTarget`: 4-byte basic-attack target lock packet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct ModifyBasicAttackTargetBody {
    /// Locked wire actor target, or `None` (`0xffff_ffff`) to clear target lock.
    #[br(map = |raw: u32| (raw != u32::MAX).then_some(raw))]
    #[bw(map = |target: &Option<u32>| target.unwrap_or(u32::MAX))]
    pub target: Option<u32>,
}
fixed_packet!(
    ModifyBasicAttackTargetBody,
    Opcode::ModifyBasicAttackTarget,
    4
);

/// `0x0424 RequestModifyBasicAttackTarget`: 5-byte client basic-attack target request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct RequestModifyBasicAttackTargetBody {
    /// Target wire actor identifier (or `u32::MAX`).
    pub target: u32,
    /// Trailing request mode flag byte.
    pub flag: u8,
}
fixed_packet!(
    RequestModifyBasicAttackTargetBody,
    Opcode::RequestModifyBasicAttackTarget,
    5
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActorFlagChange {
    Afk,
    Deserter,
    Reconnected,
}

impl ActorFlagChange {
    pub const fn opcode(self) -> Opcode {
        match self {
            Self::Afk => Opcode::PlayerAfk,
            Self::Deserter => Opcode::PlayerDeserter,
            Self::Reconnected => Opcode::PlayerReconnected,
        }
    }

    pub const fn from_opcode(opcode: Opcode) -> Option<Self> {
        match opcode {
            Opcode::PlayerAfk => Some(Self::Afk),
            Opcode::PlayerDeserter => Some(Self::Deserter),
            Opcode::PlayerReconnected => Some(Self::Reconnected),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
struct ActorFlagWire {
    actor: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorFlags<'a> {
    pub actor: u32,
    pub change: ActorFlagChange,
    padding: &'a [u8],
}

impl ActorFlags<'_> {
    pub fn new(actor: u32, change: ActorFlagChange) -> Self {
        Self {
            actor,
            change,
            padding: &[],
        }
    }

    pub fn to_message(self) -> Message {
        fixed_body::message_with_padding(
            self.change.opcode(),
            &ActorFlagWire { actor: self.actor },
            4,
            self.padding,
        )
        .expect("actor flag padding validated at decode")
    }
}

const GOLD_OFFSET: usize = 286;
const RESOURCE_MASK_OFFSET: usize = 326;
const FULL_SPAWN_COMPACT_LEN: usize = 746;
const FULL_SPAWN_PADDED_LEN: usize = 750;

#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
struct SpawnGoldFields {
    gold: f32,
    cumulative: f32,
}

impl Message {
    pub fn entity_stat(entity: u16, value: f32, stat: u8) -> Self {
        ResourceChange {
            actor: u32::from(entity),
            value,
            index: stat,
            set_mode: 0,
            flags: [0, 0],
        }
        .message()
    }

    pub fn actor_death(victim: u32, source: u32) -> Self {
        DeathBody { victim, source }.message(Opcode::ActorDeath, 14)
    }

    pub fn health_impact(
        target: u32,
        source: u32,
        operand: f32,
        word: u16,
        byte: u8,
        flag: u8,
    ) -> Self {
        ImpactBody {
            target,
            source,
            operand,
            word,
            byte,
            flag,
        }
        .message(Opcode::HealthImpact, 22)
    }

    pub fn actor_flags(&self) -> Option<ActorFlags<'_>> {
        let change = ActorFlagChange::from_opcode(self.known_opcode()?)?;
        let (wire, padding): (ActorFlagWire, _) =
            fixed_body::read_with_padding(&self.body, 4).ok()?;
        Some(ActorFlags {
            actor: wire.actor,
            change,
            padding,
        })
    }

    fn has_spawn_resources(&self) -> bool {
        self.is(Opcode::HeroSpawn)
            && matches!(
                self.body.len(),
                FULL_SPAWN_COMPACT_LEN | FULL_SPAWN_PADDED_LEN
            )
            && self.body[RESOURCE_MASK_OFFSET..RESOURCE_MASK_OFFSET + 4] == [0; 4]
    }

    /// Resources 6 and 7 in a full actor declaration: spendable gold and cumulative gold.
    pub fn spawn_gold(&self) -> Option<(f32, f32)> {
        self.has_spawn_resources().then(|| {
            let fields: SpawnGoldFields =
                fixed_body::read(&self.body[GOLD_OFFSET..GOLD_OFFSET + 8])
                    .expect("verified spawn gold slice");
            (fields.gold, fields.cumulative)
        })
    }

    /// Patch just the two resource baselines. The alternate stat mask omits these fields.
    pub fn set_spawn_gold(&mut self, gold: f32, cumulative: f32) -> io::Result<()> {
        if !self.has_spawn_resources()
            || !gold.is_finite()
            || !cumulative.is_finite()
            || gold < 0.
            || cumulative < 0.
        {
            return Err(invalid("invalid spawn gold baseline"));
        }
        fixed_body::write(
            &SpawnGoldFields { gold, cumulative },
            &mut self.body[GOLD_OFFSET..GOLD_OFFSET + 8],
            8,
        )?;
        Ok(())
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

    #[test]
    fn resource_mask_and_bounds_preserve_the_other_fields() {
        for length in [746, 750] {
            let mut m = Message::known(Opcode::HeroSpawn, vec![0x31; length]);
            assert!(m.spawn_gold().is_none());
            assert!(m.set_spawn_gold(250., 2000.).is_err());
            m.body[326] = 0;
            assert!(m.spawn_gold().is_none());
            m.body[326..330].fill(0);
            let before = m.clone();
            m.set_spawn_gold(250., 2000.).unwrap();
            assert_eq!(m.spawn_gold(), Some((250., 2000.)));
            assert_eq!(m.body[..286], before.body[..286]);
            assert_eq!(m.body[294..], before.body[294..]);
        }
    }

    #[test]
    fn decodes_both_setters_and_the_shared_clear() {
        for (opcode, change) in [
            (crate::wire_opcode::PlayerAfk, ActorFlagChange::Afk),
            (
                crate::wire_opcode::PlayerDeserter,
                ActorFlagChange::Deserter,
            ),
            (
                crate::wire_opcode::PlayerReconnected,
                ActorFlagChange::Reconnected,
            ),
        ] {
            for padding in [&[][..], &[0x35, 0x78]] {
                let message = Message::new(opcode, [&1502u32.to_be_bytes()[..], padding].concat());
                let flags = message.actor_flags().unwrap();
                assert_eq!((flags.actor, flags.change), (1502, change));
                assert_eq!(flags.to_message(), message);
            }
        }
        assert!(
            Message::known(Opcode::PlayerReconnected, [0; 5])
                .actor_flags()
                .is_none()
        );
        assert!(
            Message::known(Opcode::PlayerLoadedScene, [0; 6])
                .actor_flags()
                .is_none()
        );
    }

    #[test]
    fn basic_attack_target_roundtrips() {
        let lock = ModifyBasicAttackTargetBody { target: Some(1502) };
        assert_eq!(lock.encode().body, 1502u32.to_be_bytes());
        assert_eq!(
            ModifyBasicAttackTargetBody::read_exact(&lock.encode().body).ok(),
            Some(lock)
        );
        let clear = ModifyBasicAttackTargetBody { target: None };
        assert_eq!(clear.encode().body, u32::MAX.to_be_bytes());
        assert_eq!(
            ModifyBasicAttackTargetBody::read_exact(&clear.encode().body).ok(),
            Some(clear)
        );

        let req_target = RequestModifyBasicAttackTargetBody {
            target: 1502,
            flag: 1,
        };
        assert_eq!(
            RequestModifyBasicAttackTargetBody::read_exact(&req_target.encode().body).ok(),
            Some(req_target)
        );
    }
}
