//! Opening 5v5 Solo compact structure declarations from the Android 147219 replay.
use crate::Message;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoloStructure {
    slot: u8,
    actor: u32,
    template: u16,
    team: u8,
    x: f32,
    z: f32,
}

impl SoloStructure {
    /// Native slot, with the hero registration block at 14–23 excluded.
    pub fn at_slot(slot: u8) -> Option<Self> {
        let (actor, template, team, x, z) = match slot {
            0 => (2000, 297, 1, -70.00, -11.00),
            1 => (2001, 297, 1, -63.00, 0.00),
            2 => (2002, 297, 1, -70.00, 11.00),
            3 => (2003, 297, 2, 70.00, -11.00),
            4 => (2004, 297, 2, 63.00, 0.00),
            5 => (2005, 297, 2, 70.00, 11.00),
            6 => (2006, 295, 1, -75.00, 0.00),
            7 => (2007, 296, 2, 75.00, 0.00),
            8 => (2008, 340, 1, -21.30, -57.50),
            9 => (2009, 340, 1, -17.50, 0.00),
            10 => (2010, 340, 1, -21.30, 57.50),
            11 => (2011, 340, 2, 21.30, -57.50),
            12 => (2012, 340, 2, 17.50, 0.00),
            13 => (2013, 340, 2, 21.30, 57.50),
            24 => (2046, 339, 1, -49.24, -37.84),
            25 => (2051, 341, 1, -63.00, -22.41),
            26 => (2060, 339, 1, -34.50, 0.00),
            27 => (2065, 341, 1, -53.04, 0.00),
            28 => (2074, 339, 1, -49.24, 37.84),
            29 => (2079, 341, 1, -63.00, 22.41),
            30 => (2088, 339, 2, 49.24, -37.84),
            31 => (2093, 341, 2, 63.00, -22.41),
            32 => (2102, 339, 2, 34.50, 0.00),
            33 => (2107, 341, 2, 53.04, 0.00),
            34 => (2116, 339, 2, 49.24, 37.84),
            35 => (2121, 341, 2, 63.00, 22.41),
            _ => return None,
        };
        Some(Self {
            slot,
            actor,
            template,
            team,
            x,
            z,
        })
    }

    /// Resolve a static actor without scanning the layout on the replication path.
    pub fn at_actor(actor: u32) -> Option<Self> {
        let slot = match actor {
            2000..=2013 => (actor - 2000) as u8,
            2046..=2121 => {
                let offset = (actor - 2046) as u8;
                24 + 2 * (offset / 14) + u8::from(offset % 14 == 5)
            }
            _ => return None,
        };
        Self::at_slot(slot).filter(|profile| profile.actor == actor)
    }

    /// Stable actor ID in the captured Solo map layout.
    pub fn actor(self) -> u32 {
        self.actor
    }

    /// Zero-based simulation team for this native structure.
    pub fn team_index(self) -> u8 {
        self.team - 1
    }

    pub fn template(self) -> u16 {
        self.template
    }
    pub fn slot(self) -> u8 {
        self.slot
    }

    /// Encodes a pristine opening board using the authoritative server actor ID.
    pub fn message(self, actor: u32) -> Message {
        let mut body = [0; 122];
        let mut word = |offset: usize, value: u32| {
            body[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        };
        word(0, u32::from(self.template));
        word(
            4,
            if self.template == 297 {
                0x4a2e_ed42
            } else {
                0xc10b_41da
            },
        );
        word(8, actor);
        word(12, self.x.to_bits());
        word(16, if self.slot == 8 { 0 } else { 0x3d75_be29 });
        word(20, self.z.to_bits());
        for (i, component) in self.heading_bits().into_iter().enumerate() {
            word(24 + i * 4, component);
        }
        let hp: f32 = match self.template {
            295 | 296 => 20_000.,
            297 => 4_350.,
            339 => 4_000.,
            340 => 3_000.,
            341 => 5_000.,
            _ => unreachable!("validated by at_slot"),
        };
        word(36, hp.to_bits());
        word(40, hp.to_bits());
        if matches!(self.template, 339..=341) {
            word(64, 1f32.to_bits());
        }
        word(84, 1f32.to_bits());
        word(112, u32::MAX);
        body[88..104].fill(1);
        body[96 + usize::from(self.team)] = 15;
        body[116] = self.slot;
        body[119..122].copy_from_slice(&[1, 255, self.team]);
        Message::known(crate::Opcode::EntitySpawn, body)
    }

    // Native headings retain signed zero and slightly asymmetric rotation rounding.
    fn heading_bits(self) -> [u32; 3] {
        match self.slot {
            0 => [0x3f12_d5e8, 0, 0xbf51_b3f3],
            1 => [1f32.to_bits(), 0, 0xb33b_bd2e],
            2 => [0x3f12_d5e8, (-0.0f32).to_bits(), 0x3f51_b3f3],
            3 => [0xbf12_d5e8, 0, 0xbf51_b3f3],
            4 => [(-1f32).to_bits(), 0, 0xb33b_bd2e],
            5 => [0xbf12_d5e8, 0, 0x3f51_b3f3],
            6 | 8 | 9 | 10 | 26 | 27 => [1f32.to_bits(), (-0.0f32).to_bits(), 0],
            7 | 11 | 12 | 13 | 32 | 33 => [(-1f32).to_bits(), 0, 0],
            24 | 25 => [0x3f35_04f4, 0, 0xbf35_04f3],
            28 | 29 => [0x3f35_04f4, (-0.0f32).to_bits(), 0x3f35_04f2],
            30 | 31 => [0xbf35_04f2, 0, 0xbf35_04f5],
            34 | 35 => [0xbf35_04f4, 0, 0x3f35_04f2],
            _ => unreachable!("validated by at_slot"),
        }
    }
}

const SHOPKEEPER_SKIN: u32 = 0xb535_88b5;
const SHOPKEEPER_HP: f32 = 450.0;
const BUFF_INVULNERABLE: u16 = 27;
const BUFF_SHOP_CAN_ICON: u16 = 173;
const BUFF_SHOP_ALLOW_PURCHASE: u16 = 174;
const BUFF_SHOP_GRANT_5V5_BASE: u16 = 177;
const STORE_5V5_OPERAND: u32 = 0x01c3;
const PERMANENT_DURATION_BITS: u16 = 0xbc00;
const SHOP_ALLOW_DURATION_BITS: u16 = 0x3e00;

impl SoloStructure {
    pub const BASE_SHOP_LEFT_ACTOR: u32 = 6530;
    pub const BASE_SHOP_RIGHT_ACTOR: u32 = 6531;
    pub const RIVER_SHOP_LEFT_ACTOR: u32 = 6532;
    pub const RIVER_SHOP_RIGHT_ACTOR: u32 = 6533;
    pub const BASE_SHOP_LEFT_SLOT: u8 = 36;
    pub const BASE_SHOP_RIGHT_SLOT: u8 = 37;

    /// Compact `0x03F2` declaration for team `0`/`1`'s 5v5 Base Shopkeeper (`329`).
    pub fn base_shopkeeper(team: u8) -> Option<(u32, u8, Message)> {
        let (actor, slot, wire_team, x_bits, z_bits) = match team {
            0 => (
                Self::BASE_SHOP_LEFT_ACTOR,
                Self::BASE_SHOP_LEFT_SLOT,
                1,
                0xc2bf_d1ec,
                0xbec2_8f5c,
            ),
            1 => (
                Self::BASE_SHOP_RIGHT_ACTOR,
                Self::BASE_SHOP_RIGHT_SLOT,
                2,
                0x42bf_d1ec,
                0x3ec2_8f5c,
            ),
            _ => return None,
        };
        let heading = [0, (-0.0f32).to_bits(), 1.0f32.to_bits()];
        let msg = Self::encode_shopkeeper(
            actor,
            329,
            slot,
            wire_team,
            [x_bits, 0x3d75_be29, z_bits],
            heading,
        );
        Some((actor, slot, msg))
    }

    /// Compact `0x03F2` declaration for left (`right == false`) or right River Shopkeeper (`330`).
    pub fn river_shopkeeper(right: bool, slot: u8) -> (u32, Message) {
        let (actor, pos, heading) = if right {
            (
                Self::RIVER_SHOP_RIGHT_ACTOR,
                [16.3f32.to_bits(), 1.7f32.to_bits(), 33.6f32.to_bits()],
                [
                    std::f32::consts::FRAC_1_SQRT_2.to_bits(),
                    0,
                    std::f32::consts::FRAC_1_SQRT_2.to_bits(),
                ],
            )
        } else {
            (
                Self::RIVER_SHOP_LEFT_ACTOR,
                [(-16.3f32).to_bits(), 1.7f32.to_bits(), (-33.6f32).to_bits()],
                [
                    (-std::f32::consts::FRAC_1_SQRT_2).to_bits(),
                    0,
                    (-std::f32::consts::FRAC_1_SQRT_2).to_bits(),
                ],
            )
        };
        (
            actor,
            Self::encode_shopkeeper(actor, 330, slot, 0, pos, heading),
        )
    }

    fn encode_shopkeeper(
        actor: u32,
        template: u16,
        slot: u8,
        wire_team: u8,
        pos_bits: [u32; 3],
        heading_bits: [u32; 3],
    ) -> Message {
        let mut body = [0u8; 122];
        let mut word = |offset: usize, value: u32| {
            body[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        };
        word(0, u32::from(template));
        word(4, SHOPKEEPER_SKIN);
        word(8, actor);
        for (i, comp) in pos_bits.into_iter().enumerate() {
            word(12 + i * 4, comp);
        }
        for (i, comp) in heading_bits.into_iter().enumerate() {
            word(24 + i * 4, comp);
        }
        word(36, SHOPKEEPER_HP.to_bits());
        word(40, SHOPKEEPER_HP.to_bits());
        word(52, 2.5f32.to_bits());
        word(64, 1.0f32.to_bits());
        word(84, 1.0f32.to_bits());
        word(112, u32::MAX);
        body[88..96].fill(1);
        body[96 + usize::from(wire_team)] = 15;
        body[116] = slot;
        body[119..122].copy_from_slice(&[1, 255, wire_team]);
        Message::known(crate::Opcode::EntitySpawn, body)
    }

    /// Opening `Buff_Shop_GrantStoreAccess_5v5Base` (`177`) and `Buff_Invulnerable` (`27`)
    /// applied to each declared 5v5 shopkeeper actor.
    pub fn shopkeeper_opening_buffs(actor: u32) -> [Message; 2] {
        let first_instance = match actor {
            Self::BASE_SHOP_LEFT_ACTOR => 6592,
            Self::BASE_SHOP_RIGHT_ACTOR => 6599,
            Self::RIVER_SHOP_LEFT_ACTOR => 6606,
            _ => 6608,
        };
        [BUFF_SHOP_GRANT_5V5_BASE, BUFF_INVULNERABLE]
            .into_iter()
            .enumerate()
            .map(|(offset, definition)| {
                crate::BuffApplication {
                    prefix: crate::BuffPrefix {
                        target: actor,
                        source: actor,
                        duration_bits: PERMANENT_DURATION_BITS,
                        instance: first_instance + offset as u32,
                        definition,
                    },
                    extras: None,
                    suffix: &[],
                }
                .to_message()
                .expect("verified 5v5 shopkeeper opening buff")
            })
            .collect::<Vec<_>>()
            .try_into()
            .expect("two shopkeeper opening buffs")
    }

    /// Client shop HUD icon unlock buff (`173` `Buff_Shop_CanShopIcon`).
    pub fn shop_can_icon_message(hero_wire_id: u32, instance: u32) -> Message {
        crate::BuffApplication {
            prefix: crate::BuffPrefix {
                target: hero_wire_id,
                source: hero_wire_id,
                duration_bits: PERMANENT_DURATION_BITS,
                instance,
                definition: BUFF_SHOP_CAN_ICON,
            },
            extras: Some(crate::BuffExtras {
                stacks: 1,
                words: [0; 4],
            }),
            suffix: &[],
        }
        .to_message()
        .expect("verified shop icon buff")
    }

    /// Client store purchase unlock buff (`174` `Buff_Shop_AllowStorePurchase`, 1.5s TTL).
    pub fn shop_allow_purchase_message(
        hero_wire_id: u32,
        keeper_wire_id: u32,
        instance: u32,
    ) -> Message {
        crate::BuffApplication {
            prefix: crate::BuffPrefix {
                target: hero_wire_id,
                source: keeper_wire_id,
                duration_bits: SHOP_ALLOW_DURATION_BITS,
                instance,
                definition: BUFF_SHOP_ALLOW_PURCHASE,
            },
            extras: Some(crate::BuffExtras {
                stacks: 1,
                words: [0, 0, STORE_5V5_OPERAND, 0],
            }),
            suffix: &[],
        }
        .to_message()
        .expect("verified store purchase buff")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use md5::{Digest, Md5};

    #[test]
    fn all_opening_structures_preserve_native_fields_in_slot_order() {
        assert!(SoloStructure::at_actor(2047).is_none());
        assert!(SoloStructure::at_actor(2014).is_none());
        let mut digest = Md5::new();
        let mut count = 0;
        for slot in 0..=u8::MAX {
            if let Some(profile) = SoloStructure::at_slot(slot) {
                assert_eq!(SoloStructure::at_actor(profile.actor()), Some(profile));
                let frame = profile.message(0);
                assert_eq!(frame.opcode, crate::wire_opcode::EntitySpawn);
                digest.update(&frame.body);
                count += 1;
            }
        }
        assert_eq!(count, 26);
        // Reference bodies use zero in the four-byte runtime actor field.
        assert_eq!(
            format!("{:x}", digest.finalize()),
            "e0fb95a82162bfccbe25d9797530425e"
        );
    }

    #[test]
    fn base_shopkeeper_329_matches_recorded_5v5_snapshot_wire_bytes() {
        let (_, _, left) = SoloStructure::base_shopkeeper(0).unwrap();
        let (_, _, right) = SoloStructure::base_shopkeeper(1).unwrap();
        assert_eq!(left.opcode, crate::wire_opcode::EntitySpawn);
        assert_eq!(left.body.len(), 122);
        assert_eq!(
            &left.body[..44],
            &[
                0x00, 0x00, 0x01, 0x49, 0xb5, 0x35, 0x88, 0xb5, 0x00, 0x00, 0x19, 0x82, 0xc2, 0xbf,
                0xd1, 0xec, 0x3d, 0x75, 0xbe, 0x29, 0xbe, 0xc2, 0x8f, 0x5c, 0x00, 0x00, 0x00, 0x00,
                0x80, 0x00, 0x00, 0x00, 0x3f, 0x80, 0x00, 0x00, 0x43, 0xe1, 0x00, 0x00, 0x43, 0xe1,
                0x00, 0x00,
            ]
        );
        assert_eq!(&left.body[52..56], &2.5f32.to_be_bytes());
        assert_eq!(&left.body[64..68], &1.0f32.to_be_bytes());
        assert_eq!(&left.body[84..88], &1.0f32.to_be_bytes());
        assert_eq!(&left.body[96..104], &[0, 15, 0, 0, 0, 0, 0, 0]);
        assert_eq!(&left.body[116..122], &[36, 0, 0, 1, 255, 1]);
        assert_eq!(&right.body[96..104], &[0, 0, 15, 0, 0, 0, 0, 0]);
        assert_eq!(&right.body[116..122], &[37, 0, 0, 1, 255, 2]);

        let [grant, invuln] =
            SoloStructure::shopkeeper_opening_buffs(SoloStructure::BASE_SHOP_LEFT_ACTOR);
        assert_eq!(grant.opcode, crate::wire_opcode::ApplyBuffCompact);
        assert_eq!(grant.body[14..16], 177u16.to_be_bytes());
        assert_eq!(invuln.body[14..16], 27u16.to_be_bytes());

        let allow = SoloStructure::shop_allow_purchase_message(
            1500,
            SoloStructure::BASE_SHOP_LEFT_ACTOR,
            4369,
        );
        assert_eq!(allow.opcode, crate::wire_opcode::ApplyBuffExtended);
        assert_eq!(
            &allow.body[..10],
            &[0, 0, 5, 220, 0, 0, 0x19, 0x82, 0x3e, 0x00]
        );
        assert_eq!(allow.body[14..16], 174u16.to_be_bytes());
        assert_eq!(allow.body[26..30], 451u32.to_be_bytes());
    }
}
