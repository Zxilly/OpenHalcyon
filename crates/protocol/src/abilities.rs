use crate::{Message, Opcode, fixed_body::fixed_packet};
use binrw::{BinRead, BinWrite};

#[derive(Debug, Clone, Copy, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big, repr = u8)]
#[repr(u8)]
pub enum CooldownMode {
    Adjust = 0,
    Set = 1,
}

impl CooldownMode {
    pub const fn from_wire(raw: u8) -> Option<Self> {
        match raw {
            0 => Some(Self::Adjust),
            1 => Some(Self::Set),
            _ => None,
        }
    }

    pub const fn raw(self) -> u8 {
        self as u8
    }
}

/// 13-byte body of `0x0428` (`Opcode::ModifyCooldown`), padded to 14 bytes when aligned.
#[derive(Debug, Clone, Copy, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct ModifyCooldownBody {
    pub owner: u32,
    pub key: u32,
    pub value: f32,
    pub mode: u8,
}
fixed_packet!(ModifyCooldownBody, Opcode::ModifyCooldown, 13);

impl ModifyCooldownBody {
    pub const fn from_mode(owner: u32, key: u32, value: f32, mode: CooldownMode) -> Self {
        Self {
            owner,
            key,
            value,
            mode: mode.raw(),
        }
    }

    pub const fn cooldown_mode(self) -> Option<CooldownMode> {
        CooldownMode::from_wire(self.mode)
    }
}

/// 8-byte body of `0x043A` (`Opcode::GrantAbility`), padded to 14 bytes when aligned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct GrantAbilityBody {
    pub actor: u32,
    pub operand: u32,
}
fixed_packet!(GrantAbilityBody, Opcode::GrantAbility, 8);

/// 9-byte body of `0x0415` (`Opcode::PlayTargetAbility`), padded to 14 bytes when aligned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct PlayTargetAbilityBody {
    pub actor: u32,
    pub target: u32,
    pub selector: u8,
}
fixed_packet!(PlayTargetAbilityBody, Opcode::PlayTargetAbility, 9);

/// 17-byte body of `0x0416` (`Opcode::PlayPointAbility`), padded to 22 bytes when aligned.
#[derive(Debug, Clone, Copy, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct PlayPointAbilityBody {
    pub actor: u32,
    pub coordinates: [f32; 3],
    pub selector: u8,
}
fixed_packet!(PlayPointAbilityBody, Opcode::PlayPointAbility, 17);

impl Message {
    pub fn cooldown_adjustment(owner: u32, key: u32, value: f32, mode: CooldownMode) -> Self {
        ModifyCooldownBody::from_mode(owner, key, value, mode).encode()
    }

    /// Observed 0x043A actor and learned ability operand.
    pub fn ability_learned(actor: u32, operand: u32) -> Self {
        GrantAbilityBody { actor, operand }.encode()
    }

    /// Observed 0x0415 caster, target actor and ability selector.
    pub fn ability_target_used(actor: u32, target: u32, selector: u8) -> Self {
        PlayTargetAbilityBody {
            actor,
            target,
            selector,
        }
        .encode()
    }

    /// Observed 0x0416 actor, three world coordinates and ability selector.
    pub fn ability_point_used(actor: u32, coordinates: [f32; 3], selector: u8) -> Self {
        PlayPointAbilityBody {
            actor,
            coordinates,
            selector,
        }
        .encode()
    }
}

#[cfg(test)]
mod tests {
    use crate::{Fields, LiveFields, MatchCipher, Message};

    #[test]
    fn observed_idris_points_round_trip_and_preserve_signed_zero() {
        let cipher = MatchCipher::new("synthetic-round-trip");
        for coordinates in [
            [-40.8939_f32, 0.0, 1.4944],
            [-58.2402, -0.0, 14.9417],
            [-44.1787, -0.0, 3.5794],
            [-34.1510, 0.0, -0.7185],
        ] {
            let mut body = coordinates
                .into_iter()
                .flat_map(f32::to_be_bytes)
                .collect::<Vec<_>>();
            body.push(3);
            let request = Message::known(crate::Opcode::RequestPointAbility, &body[..]);
            let wire = cipher.encode(&request).unwrap();
            let decoded = cipher.decode(&wire[2..]).unwrap();
            assert_eq!(decoded.body[13..], [0]);
            assert_eq!(
                decoded.fields().unwrap(),
                Fields::Live(LiveFields::AbilityPoint {
                    vector: coordinates,
                    selector: 3
                })
            );
            let response = Message::ability_point_used(1505, coordinates, 3);
            let wire = cipher.encode(&response).unwrap();
            let decoded = cipher.decode(&wire[2..]).unwrap();
            assert_eq!(decoded.opcode, crate::wire_opcode::PlayPointAbility);
            assert_eq!(&decoded.body[..4], &1505u32.to_be_bytes());
            assert_eq!(&decoded.body[4..17], &body);
            assert_eq!(decoded.body[17..], [0; 5]);
        }
    }
}
