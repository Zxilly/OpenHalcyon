//! Full `0x03F3` (`HeroSpawn`, 750B) wire codec using `binrw`.
use crate::{EntitySpawnBody, Message, Opcode, invalid};
use binrw::{BinRead, BinWrite};
use std::io::{self, Cursor};

/// 50-byte core prefix of `0x03F3` (`Opcode::HeroSpawn`) up through `max_hp`.
#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct HeroSpawnPrefix {
    pub template: u32,
    pub skin: u32,
    pub actor: u32,
    pub team: u32,
    pub flags16: u16,
    pub position: [f32; 3],
    pub forward: [f32; 3],
    pub hp: f32,
    pub max_hp: f32,
}

/// Exact 750-byte `0x03F3` (`Opcode::HeroSpawn`) body preserving all reserved intervals
/// and the 4-byte alignment tail at `746..750`.
#[derive(Clone, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct HeroSpawnBody {
    /// Entity template ID (`0..4`; `u16` `EntityTemplate` raw IDs occupy bytes `2..4`).
    pub template: u32,
    /// Skin hash (`4..8`, Jenkins `lookup2`).
    pub skin: u32,
    /// Authoritative wire actor ID (`8..12`; `u16` `wire_id` occupies bytes `10..12`).
    pub actor: u32,
    /// One-based wire team (`12..16`: `1` Blue, `2` Red, `0` Neutral).
    pub team: u32,
    /// Fixed descriptor halfword (`16..18`, `0xffff` in standard declarations).
    pub flags16: u16,
    /// World `[x, y_or_height, z]` position (`18..30`).
    pub position: [f32; 3],
    /// Facing/forward vector `[fx, fy, fz]` (`30..42`; `34..38` carries `-0.0` on Solo opening).
    pub forward: [f32; 3],
    /// Current HP (`42..46`).
    pub hp: f32,
    /// Maximum HP (`46..50`).
    pub max_hp: f32,
    /// Unverified/reserved interval (`50..74`).
    pub reserved_50: [u8; 24],
    /// Movement speed (`74..78`).
    pub move_speed: f32,
    /// Unverified/reserved interval (`78..98`).
    pub reserved_78: [u8; 20],
    /// Attack speed (`98..102`).
    pub attack_speed: f32,
    /// Unverified/reserved interval (`102..122`).
    pub reserved_102: [u8; 20],
    /// Current energy (`122..126`).
    pub energy: f32,
    /// Maximum energy (`126..130`).
    pub max_energy: f32,
    /// Unverified/reserved interval (`130..190`).
    pub reserved_130: [u8; 60],
    /// Effective armor (`190..194`).
    pub armor: f32,
    /// Unverified/reserved interval (`194..202`).
    pub reserved_194: [u8; 8],
    /// Effective shield (`202..206`).
    pub shield: f32,
    /// Unverified/reserved interval (`206..214`).
    pub reserved_206: [u8; 8],
    /// Effective weapon power (`214..218`).
    pub weapon_power: f32,
    /// Unverified/reserved interval (`218..226`).
    pub reserved_218: [u8; 8],
    /// Unit spawn descriptor word (`226..230`, `0.5` when unit metadata is populated).
    pub word226: f32,
    /// Unverified/reserved interval (`230..286`).
    pub reserved_230: [u8; 56],
    /// Spendable gold baseline, resource `6` (`286..290`).
    pub gold: f32,
    /// Cumulative earned gold baseline, resource `7` (`290..294`).
    pub cumulative_gold: f32,
    /// Hero level, actor attribute `40` (`294..298`, `0x126`).
    pub level: f32,
    /// Unverified/reserved interval (`298..314`).
    pub reserved_298: [u8; 16],
    /// Unspent ability upgrade points, actor attribute `29` (`314..318`, `0x13a`).
    pub skill_points: f32,
    /// Current XP, actor resource `8` (`318..322`, `0x13e`).
    pub xp: f32,
    /// XP required for next level, actor attribute `39` (`322..326`, `0x142`).
    pub xp_required: f32,
    /// Alternate stat table selector mask (`326..330`, `0x146`); must stay `0` for standard stats.
    pub alt_stat_mask: u32,
    /// Unverified/reserved interval (`330..705`).
    pub reserved_330: [u8; 375],
    /// Active declaration flag (`705..709`, `0x2c1`, `1` on initialized hero spawns).
    pub active_flag: u32,
    /// Solo selector (`709..713`, `0x2c5`: `u32::MAX` for human, `team` for bot).
    pub selector: u32,
    /// Per-viewer initial visibility required flags (`713..721`, `0x2c9..0x2d1`).
    pub visibility_required: [u8; 8],
    /// Per-viewer initial visibility mask (`721..729`, `0x2d1..0x2d9`).
    pub visibility_mask: [u8; 8],
    /// Per-viewer auxiliary visibility flags (`729..737`, `0x2d9..0x2e1`).
    pub visibility_aux: [u8; 8],
    /// Unverified/reserved interval (`737..741`, `0x2e1..0x2e5`).
    pub reserved_737: [u8; 4],
    /// Trailing descriptor word (`741..745`, `0x2e5..0x2e9`, `u32::MAX` in Solo).
    pub word741: u32,
    /// Scene actor registration slot (`745`).
    pub slot: u8,
    /// Trailing 4-byte Blowfish alignment tail (`746..750`).
    pub reserved_746: [u8; 4],
}

impl Default for HeroSpawnBody {
    fn default() -> Self {
        Self {
            template: 0,
            skin: 0,
            actor: 0,
            team: 0,
            flags16: 0,
            position: [0.0; 3],
            forward: [0.0; 3],
            hp: 0.0,
            max_hp: 0.0,
            reserved_50: [0; 24],
            move_speed: 0.0,
            reserved_78: [0; 20],
            attack_speed: 0.0,
            reserved_102: [0; 20],
            energy: 0.0,
            max_energy: 0.0,
            reserved_130: [0; 60],
            armor: 0.0,
            reserved_194: [0; 8],
            shield: 0.0,
            reserved_206: [0; 8],
            weapon_power: 0.0,
            reserved_218: [0; 8],
            word226: 0.0,
            reserved_230: [0; 56],
            gold: 0.0,
            cumulative_gold: 0.0,
            level: 0.0,
            reserved_298: [0; 16],
            skill_points: 0.0,
            xp: 0.0,
            xp_required: 0.0,
            alt_stat_mask: 0,
            reserved_330: [0; 375],
            active_flag: 0,
            selector: 0,
            visibility_required: [0; 8],
            visibility_mask: [0; 8],
            visibility_aux: [0; 8],
            reserved_737: [0; 4],
            word741: 0,
            slot: 0,
            reserved_746: [0; 4],
        }
    }
}

impl HeroSpawnBody {
    /// Exact Blowfish-aligned `0x03F3` body length in bytes.
    pub const BODY_LEN: usize = 750;
    /// Unpadded Android `0x03F3` struct size in bytes before the 4-byte alignment tail.
    pub const UNPADDED_LEN: usize = 746;
    /// Default descriptor halfword at bytes `16..18`.
    pub const DEFAULT_FLAGS16: u16 = 0xffff;
    /// Measured Solo opening ground Y coordinate bits (`0x3d75_be29`).
    pub const GROUND_HEIGHT_BITS: u32 = EntitySpawnBody::GROUND_HEIGHT_BITS;
    /// Active hero declaration marker written at bytes `705..709`.
    pub const ACTIVE_FLAG: u32 = 1;

    /// Constructs a standard baseline `HeroSpawnBody` (`flags16 = 0xffff`, `forward = [1.0, 0.0, 0.0]`).
    pub fn new(template: u32, skin: u32, actor: u32, team: u32, slot: u8) -> Self {
        Self {
            template,
            skin,
            actor,
            team,
            flags16: Self::DEFAULT_FLAGS16,
            forward: [1.0, 0.0, 0.0],
            slot,
            ..Default::default()
        }
    }

    /// Reads an exact 750-byte `0x03F3` body in big-endian order.
    pub fn read_body(body: &[u8]) -> io::Result<Self> {
        if body.len() != Self::BODY_LEN {
            return Err(invalid("invalid HeroSpawn body length"));
        }
        let mut cursor = Cursor::new(body);
        Self::read_be(&mut cursor).map_err(|_| invalid("invalid HeroSpawn body"))
    }

    /// Reads the 50-byte core `HeroSpawn` prefix (`template..=max_hp`) from any `50+`-byte body.
    pub fn read_summary_prefix(body: &[u8]) -> io::Result<HeroSpawnPrefix> {
        HeroSpawnPrefix::read_be(&mut Cursor::new(body))
            .map_err(|_| invalid("truncated HeroSpawn prefix"))
    }

    /// Reads either an unpadded 746-byte replay body or a 750-byte aligned body.
    pub fn read_unpadded_or_padded(body: &[u8]) -> io::Result<Self> {
        match body.len() {
            Self::BODY_LEN => Self::read_body(body),
            Self::UNPADDED_LEN => {
                let mut padded = [0u8; Self::BODY_LEN];
                padded[..Self::UNPADDED_LEN].copy_from_slice(body);
                Self::read_body(&padded)
            }
            _ => Err(invalid("invalid HeroSpawn body length")),
        }
    }

    /// Writes the 750-byte big-endian body into `dst[..750]` without modifying trailing bytes.
    pub fn write_body(&self, dst: &mut [u8]) -> io::Result<()> {
        let out = dst
            .get_mut(..Self::BODY_LEN)
            .ok_or_else(|| invalid("HeroSpawn output buffer too small"))?;
        let mut cursor = Cursor::new(out);
        self.write_be(&mut cursor)
            .map_err(|_| invalid("failed to write HeroSpawn body"))?;
        debug_assert_eq!(cursor.position(), Self::BODY_LEN as u64);
        Ok(())
    }

    /// Serializes the body into an exact 750-byte vector.
    pub fn to_vec(&self) -> Vec<u8> {
        let mut out = vec![0u8; Self::BODY_LEN];
        self.write_body(&mut out)
            .expect("HeroSpawnBody fits BODY_LEN");
        out
    }

    /// Encodes as an `Opcode::HeroSpawn` (`0x03F3`) [`Message`].
    pub fn encode(&self) -> Message {
        Message::known(Opcode::HeroSpawn, self.to_vec())
    }

    /// Compares two bodies byte-for-byte on the wire, distinguishing `-0.0` and NaN payloads.
    pub fn bit_eq(&self, other: &Self) -> bool {
        self.to_vec() == other.to_vec()
    }

    /// Returns `(gold, cumulative_gold)` when the standard stat table is active (`alt_stat_mask == 0`).
    pub fn gold_baseline(&self) -> Option<(f32, f32)> {
        (self.alt_stat_mask == 0).then_some((self.gold, self.cumulative_gold))
    }

    /// Validates `alt_stat_mask == 0` and finite non-negative gold baselines before updating.
    pub fn set_gold(&mut self, gold: f32, cumulative: f32) -> io::Result<()> {
        if self.alt_stat_mask != 0
            || !gold.is_finite()
            || !cumulative.is_finite()
            || gold < 0.0
            || cumulative < 0.0
        {
            return Err(invalid("invalid spawn gold baseline"));
        }
        self.gold = gold;
        self.cumulative_gold = cumulative;
        Ok(())
    }

    /// Applies Android Solo full-declaration selectors and opening visibility flags.
    pub fn apply_solo_profile(&mut self, bot: bool, sim_team: u8, tick: u64) {
        self.selector = if bot { u32::from(sim_team) } else { u32::MAX };
        self.word741 = u32::MAX;
        if tick == 0 {
            self.position[1] = f32::from_bits(Self::GROUND_HEIGHT_BITS);
            self.forward[1] = -0.0_f32;
            self.visibility_required = EntitySpawnBody::VISIBILITY_ALL_REQUIRED;
            if let Some(entry) = self.visibility_mask.get_mut(usize::from(sim_team) + 1) {
                *entry = EntitySpawnBody::VISIBILITY_ACTIVE_MASK;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_observed_hero_spawn_and_arbitrary_reserved_bytes_losslessly() {
        let mut raw = [0u8; HeroSpawnBody::BODY_LEN];
        raw[0..4].copy_from_slice(&261u32.to_be_bytes());
        raw[4..8].copy_from_slice(&0x5b65_dad4u32.to_be_bytes());
        raw[8..12].copy_from_slice(&1523u32.to_be_bytes());
        raw[12..16].copy_from_slice(&2u32.to_be_bytes());
        raw[16..18].copy_from_slice(&HeroSpawnBody::DEFAULT_FLAGS16.to_be_bytes());
        raw[18..22].copy_from_slice(&94.0f32.to_be_bytes());
        raw[22..26].copy_from_slice(&HeroSpawnBody::GROUND_HEIGHT_BITS.to_be_bytes());
        raw[26..30].copy_from_slice(&6.0f32.to_be_bytes());
        raw[30..34].copy_from_slice(&1.0f32.to_be_bytes());
        raw[34..38].copy_from_slice(&(-0.0f32).to_be_bytes());
        raw[42..46].copy_from_slice(&679.0f32.to_be_bytes());
        raw[46..50].copy_from_slice(&679.0f32.to_be_bytes());
        raw[74..78].copy_from_slice(&3.6f32.to_be_bytes());
        raw[98..102].copy_from_slice(&1.0f32.to_be_bytes());
        raw[122..126].copy_from_slice(&320.0f32.to_be_bytes());
        raw[126..130].copy_from_slice(&320.0f32.to_be_bytes());
        raw[190..194].copy_from_slice(&26.0f32.to_be_bytes());
        raw[202..206].copy_from_slice(&21.0f32.to_be_bytes());
        raw[214..218].copy_from_slice(&71.0f32.to_be_bytes());
        raw[226..230].copy_from_slice(&0.5f32.to_be_bytes());
        raw[286..290].copy_from_slice(&600.0f32.to_be_bytes());
        raw[290..294].copy_from_slice(&600.0f32.to_be_bytes());
        raw[294..298].copy_from_slice(&1.0f32.to_be_bytes());
        raw[314..318].copy_from_slice(&1.0f32.to_be_bytes());
        raw[322..326].copy_from_slice(&68.0f32.to_be_bytes());
        raw[705..709].copy_from_slice(&1u32.to_be_bytes());
        raw[709..713].copy_from_slice(&1u32.to_be_bytes());
        raw[713..721].copy_from_slice(&[1; 8]);
        raw[721..729].copy_from_slice(&[0, 0, 15, 0, 0, 0, 0, 0]);
        raw[741..745].copy_from_slice(&u32::MAX.to_be_bytes());
        raw[745] = 23;

        let parsed = HeroSpawnBody::read_body(&raw).unwrap();
        assert_eq!(parsed.template, 261);
        assert_eq!(parsed.skin, 0x5b65_dad4);
        assert_eq!(parsed.actor, 1523);
        assert_eq!(parsed.team, 2);
        assert_eq!(parsed.flags16, 0xffff);
        assert_eq!(parsed.position[0], 94.0);
        assert_eq!(
            parsed.position[1].to_bits(),
            HeroSpawnBody::GROUND_HEIGHT_BITS
        );
        assert_eq!(parsed.position[2], 6.0);
        assert_eq!(parsed.forward[0], 1.0);
        assert_eq!(parsed.forward[1].to_bits(), 0x8000_0000);
        assert_eq!(parsed.hp, 679.0);
        assert_eq!(parsed.max_hp, 679.0);
        assert_eq!(parsed.move_speed, 3.6);
        assert_eq!(parsed.attack_speed, 1.0);
        assert_eq!(parsed.energy, 320.0);
        assert_eq!(parsed.max_energy, 320.0);
        assert_eq!(parsed.armor, 26.0);
        assert_eq!(parsed.shield, 21.0);
        assert_eq!(parsed.weapon_power, 71.0);
        assert_eq!(parsed.word226, 0.5);
        assert_eq!(parsed.gold_baseline(), Some((600.0, 600.0)));
        assert_eq!(parsed.level, 1.0);
        assert_eq!(parsed.skill_points, 1.0);
        assert_eq!(parsed.xp, 0.0);
        assert_eq!(parsed.xp_required, 68.0);
        assert_eq!(parsed.alt_stat_mask, 0);
        assert_eq!(parsed.active_flag, 1);
        assert_eq!(parsed.selector, 1);
        assert_eq!(parsed.word741, u32::MAX);
        assert_eq!(parsed.slot, 23);
        assert_eq!(parsed.encode().body, raw);

        // Mutate every byte across all reserved intervals + sNaN + -0.0 and verify 100% round-trip.
        let mut noisy: [u8; HeroSpawnBody::BODY_LEN] =
            std::array::from_fn(|i| (i as u8).wrapping_mul(53).wrapping_add(7));
        noisy[42..46].copy_from_slice(&0x7f80_0001u32.to_be_bytes());
        noisy[34..38].copy_from_slice(&0x8000_0000u32.to_be_bytes());
        let parsed_noisy = HeroSpawnBody::read_body(&noisy).unwrap();
        assert_eq!(parsed_noisy.encode().body, noisy);
    }

    #[test]
    fn rejects_wrong_lengths_and_validates_gold_and_short_output() {
        for bad_len in [0, 1, 745, 746, 749, 751] {
            assert!(HeroSpawnBody::read_body(&vec![0u8; bad_len]).is_err());
        }
        let unpadded = [0x33u8; HeroSpawnBody::UNPADDED_LEN];
        let decoded_unpadded = HeroSpawnBody::read_unpadded_or_padded(&unpadded).unwrap();
        assert_eq!(&decoded_unpadded.to_vec()[..746], &unpadded[..]);
        assert_eq!(&decoded_unpadded.to_vec()[746..], &[0; 4]);
        assert!(HeroSpawnBody::read_unpadded_or_padded(&[0u8; 747]).is_err());

        let mut body = HeroSpawnBody::default();
        assert!(body.set_gold(250.0, 2000.0).is_ok());
        assert_eq!(body.gold_baseline(), Some((250.0, 2000.0)));
        assert!(body.set_gold(-1.0, 2000.0).is_err());
        assert!(body.set_gold(f32::NAN, 2000.0).is_err());
        body.alt_stat_mask = 1;
        assert_eq!(body.gold_baseline(), None);
        assert!(body.set_gold(250.0, 2000.0).is_err());

        let mut short = [0x5au8; 749];
        assert!(body.write_body(&mut short).is_err());
        assert_eq!(short, [0x5au8; 749]);

        let mut longer = [0x77u8; 754];
        body.write_body(&mut longer).unwrap();
        assert_eq!(&longer[..750], &body.to_vec()[..]);
        assert_eq!(&longer[750..], &[0x77; 4]);
    }

    #[test]
    fn solo_profile_populates_opening_and_late_selectors_faithfully() {
        let mut opening_human = HeroSpawnBody::new(261, 0x5b65_dad4, 1500, 1, 14);
        opening_human.apply_solo_profile(false, 0, 0);
        assert_eq!(opening_human.selector, u32::MAX);
        assert_eq!(opening_human.word741, u32::MAX);
        assert_eq!(
            opening_human.position[1].to_bits(),
            HeroSpawnBody::GROUND_HEIGHT_BITS
        );
        assert_eq!(opening_human.forward[1].to_bits(), (-0.0_f32).to_bits());
        assert_eq!(
            opening_human.visibility_required,
            EntitySpawnBody::VISIBILITY_ALL_REQUIRED
        );
        assert_eq!(opening_human.visibility_mask, [0, 15, 0, 0, 0, 0, 0, 0]);

        let mut late_bot = HeroSpawnBody::new(261, 0x5b65_dad4, 1501, 2, 19);
        late_bot.apply_solo_profile(true, 1, 90);
        assert_eq!(late_bot.selector, 1);
        assert_eq!(late_bot.word741, u32::MAX);
        assert_eq!(late_bot.visibility_required, [0; 8]);
        assert_eq!(late_bot.visibility_mask, [0; 8]);
    }
}
