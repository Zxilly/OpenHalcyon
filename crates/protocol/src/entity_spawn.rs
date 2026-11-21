//! Compact `0x03F2` (`EntitySpawn`, 122B) wire codec using `binrw`.
use crate::{Message, Opcode, invalid};
use binrw::{BinRead, BinWrite};
use std::io::{self, Cursor};

/// Closed lossless enum for byte `117` (`flag117` / stat mask) in `0x03F2`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityStatMask {
    /// Full health and stat fields are populated (`0`).
    Full,
    /// Pristine template-default birth (`1`), omitting health/stat fields.
    Pristine,
    /// Any other wire selector byte preserved losslessly.
    Unknown(u8),
}

impl EntityStatMask {
    pub const fn from_wire(raw: u8) -> Self {
        match raw {
            0 => Self::Full,
            1 => Self::Pristine,
            other => Self::Unknown(other),
        }
    }

    pub const fn raw(self) -> u8 {
        match self {
            Self::Full => 0,
            Self::Pristine => 1,
            Self::Unknown(other) => other,
        }
    }
}

impl From<u8> for EntityStatMask {
    fn from(raw: u8) -> Self {
        Self::from_wire(raw)
    }
}

impl From<EntityStatMask> for u8 {
    fn from(mask: EntityStatMask) -> Self {
        mask.raw()
    }
}

/// One-based wire team index at byte `121` of `0x03F2` (`EntitySpawn`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireTeam {
    /// Neutral / jungle actor (`0`).
    Neutral,
    /// Blue / left team (`1`, simulation team `0`).
    Blue,
    /// Red / right team (`2`, simulation team `1`).
    Red,
    /// Unobserved team byte preserved losslessly.
    Unknown(u8),
}

impl WireTeam {
    pub const fn from_wire(raw: u8) -> Self {
        match raw {
            0 => Self::Neutral,
            1 => Self::Blue,
            2 => Self::Red,
            other => Self::Unknown(other),
        }
    }

    pub const fn from_sim_team(team: u8) -> Self {
        match team {
            0 => Self::Blue,
            1 => Self::Red,
            _ => Self::Neutral,
        }
    }

    pub const fn raw(self) -> u8 {
        match self {
            Self::Neutral => 0,
            Self::Blue => 1,
            Self::Red => 2,
            Self::Unknown(other) => other,
        }
    }
}

impl From<u8> for WireTeam {
    fn from(raw: u8) -> Self {
        Self::from_wire(raw)
    }
}

impl From<WireTeam> for u8 {
    fn from(team: WireTeam) -> Self {
        team.raw()
    }
}

/// Exact 122-byte `0x03F2` (`Opcode::EntitySpawn`) body preserving all reserved bytes.
#[derive(Clone, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct EntitySpawnBody {
    /// Entity template ID (`0..4`; `u16` `EntityTemplate` raw IDs occupy bytes `2..4`).
    pub template: u32,
    /// Skin hash (`4..8`, Jenkins `lookup2`).
    pub skin: u32,
    /// Authoritative wire actor ID (`8..12`).
    pub actor: u32,
    /// World `[x, y_or_height, z]` position (`12..24`).
    pub position: [f32; 3],
    /// Facing/forward vector `[fx, fy, fz]` (`24..36`).
    pub forward: [f32; 3],
    /// Current HP (`36..40`).
    pub hp: f32,
    /// Maximum HP (`40..44`).
    pub max_hp: f32,
    /// Unverified/reserved bytes (`44..52`).
    pub reserved_44: [u8; 8],
    /// Collision/interaction radius (`52..56`).
    pub radius: f32,
    /// Unverified/reserved bytes (`56..64`).
    pub reserved_56: [u8; 8],
    /// Descriptor float word at offset `64` (`64..68`).
    pub word64: f32,
    /// Unverified/reserved bytes (`68..84`).
    pub reserved_68: [u8; 16],
    /// Descriptor float word at offset `84` (`84..88`).
    pub word84: f32,
    /// Per-viewer initial visibility required flags (`88..96`).
    pub visibility_required: [u8; 8],
    /// Per-viewer initial visibility masks (`96..104`).
    pub visibility_mask: [u8; 8],
    /// Per-viewer auxiliary visibility flags (`104..112`).
    pub visibility_aux: [u8; 8],
    /// Owner/parent actor reference or `u32::MAX` sentinel (`112..116`).
    pub word112: u32,
    /// Scene actor registration slot (`116`).
    pub slot: u8,
    /// Stat presence selector (`117`: `0` full stats, `1` pristine template-default birth).
    pub flag117: u8,
    /// Additional spawn flag byte (`118`).
    pub flag118: u8,
    /// Entity level (`119`).
    pub level: u8,
    /// Lane/route index (`120`, `255` when unassigned).
    pub lane_index: u8,
    /// One-based wire team index (`121`: `0` neutral, `1` blue, `2` red).
    pub team_index: u8,
}

impl Default for EntitySpawnBody {
    fn default() -> Self {
        Self {
            template: 0,
            skin: 0,
            actor: 0,
            position: [0.0; 3],
            forward: [0.0; 3],
            hp: 0.0,
            max_hp: 0.0,
            reserved_44: [0; 8],
            radius: 0.0,
            reserved_56: [0; 8],
            word64: 0.0,
            reserved_68: [0; 16],
            word84: 0.0,
            visibility_required: [0; 8],
            visibility_mask: [0; 8],
            visibility_aux: [0; 8],
            word112: 0,
            slot: 0,
            flag117: 0,
            flag118: 0,
            level: 0,
            lane_index: 0,
            team_index: 0,
        }
    }
}

impl EntitySpawnBody {
    /// Exact unpadded `0x03F2` body size in bytes.
    pub const BODY_LEN: usize = 122;
    /// Blowfish 8-byte aligned `0x03F2` body size in bytes (4 trailing padding bytes).
    pub const PADDED_LEN: usize = 126;

    /// Verified skin hash for structures (crystals, turrets, beacons, base shops).
    pub const STRUCTURE_SKIN: u32 = 0xc10b_41da;
    /// Verified skin hash for 5v5 Solo armories (`VainNode`).
    pub const ARMORY_SKIN: u32 = 0x4a2e_ed42;
    /// Verified skin hash for lane minions.
    pub const MINION_SKIN: u32 = 0xeb39_ce55;
    /// Verified skin hash for jungle creeps, sentries, miners, and jungle shops.
    pub const JUNGLE_MINION_SKIN: u32 = 0x4dd5_b7d0;
    /// Verified skin hash for 5v5 shopkeepers (`329`/`330`).
    pub const SHOPKEEPER_SKIN: u32 = 0xb535_88b5;
    /// Measured ground terrain Y coordinate bits (`0x3d75_be29`).
    pub const GROUND_HEIGHT_BITS: u32 = 0x3d75_be29;
    /// All eight viewer indices marked required (`[1; 8]`).
    pub const VISIBILITY_ALL_REQUIRED: [u8; 8] = [1; 8];
    /// Standard active visibility mask (`15`).
    pub const VISIBILITY_ACTIVE_MASK: u8 = 15;
    /// Pristine wave birth visibility mask (`1`).
    pub const VISIBILITY_PRISTINE_MASK: u8 = 1;
    /// Sentinel lane index when no lane route applies (`255`).
    pub const NO_LANE_INDEX: u8 = u8::MAX;

    /// Constructs a baseline `EntitySpawnBody` with `word112 = u32::MAX`, `level = 1`,
    /// `lane_index = NO_LANE_INDEX`, and zeroed numeric/reserved fields.
    pub fn new(template: u32, skin: u32, actor: u32, slot: u8, team_index: u8) -> Self {
        Self {
            template,
            skin,
            actor,
            word112: u32::MAX,
            slot,
            level: 1,
            lane_index: Self::NO_LANE_INDEX,
            team_index,
            ..Default::default()
        }
    }

    /// Sets `visibility_required = [1; 8]`, fills `visibility_mask` with `fill`, and writes
    /// `team_mask` at `visibility_mask[team_index]` when `team_index < 8`.
    pub fn set_team_visibility(&mut self, fill: u8, team_mask: u8) {
        self.visibility_required = Self::VISIBILITY_ALL_REQUIRED;
        self.visibility_mask = [fill; 8];
        if let Some(entry) = self.visibility_mask.get_mut(usize::from(self.team_index)) {
            *entry = team_mask;
        }
    }

    /// Builder variant of [`Self::set_team_visibility`].
    pub fn with_team_visibility(mut self, fill: u8, team_mask: u8) -> Self {
        self.set_team_visibility(fill, team_mask);
        self
    }

    /// Returns the typed representation of `flag117`.
    pub const fn stat_mask(&self) -> EntityStatMask {
        EntityStatMask::from_wire(self.flag117)
    }

    /// Sets `flag117` from [`EntityStatMask`].
    pub fn set_stat_mask(&mut self, mask: EntityStatMask) {
        self.flag117 = mask.raw();
    }

    /// Returns the typed representation of `team_index`.
    pub const fn wire_team(&self) -> WireTeam {
        WireTeam::from_wire(self.team_index)
    }

    /// Sets `team_index` from [`WireTeam`].
    pub fn set_wire_team(&mut self, team: WireTeam) {
        self.team_index = team.raw();
    }

    /// Reads an exact 122-byte `0x03F2` body in big-endian order.
    pub fn read_body(body: &[u8]) -> io::Result<Self> {
        if body.len() != Self::BODY_LEN {
            return Err(invalid("invalid EntitySpawn body length"));
        }
        let mut cursor = Cursor::new(body);
        Self::read_be(&mut cursor).map_err(|_| invalid("invalid EntitySpawn body"))
    }

    /// Reads either a 122-byte native body or a 126-byte Blowfish-padded body,
    /// returning the parsed body and any trailing padding slice.
    pub fn read_padded(body: &[u8]) -> io::Result<(Self, &[u8])> {
        if !matches!(body.len(), Self::BODY_LEN | Self::PADDED_LEN) {
            return Err(invalid("invalid padded EntitySpawn body length"));
        }
        let parsed = Self::read_body(&body[..Self::BODY_LEN])?;
        Ok((parsed, &body[Self::BODY_LEN..]))
    }

    /// Writes the 122-byte big-endian body into `dst[..122]` without touching trailing bytes.
    pub fn write_body(&self, dst: &mut [u8]) -> io::Result<()> {
        let out = dst
            .get_mut(..Self::BODY_LEN)
            .ok_or_else(|| invalid("EntitySpawn output buffer too small"))?;
        let mut cursor = Cursor::new(out);
        self.write_be(&mut cursor)
            .map_err(|_| invalid("failed to write EntitySpawn body"))?;
        debug_assert_eq!(cursor.position(), Self::BODY_LEN as u64);
        Ok(())
    }

    /// Serializes the body into an exact 122-byte array.
    pub fn to_bytes(&self) -> [u8; Self::BODY_LEN] {
        let mut out = [0u8; Self::BODY_LEN];
        self.write_body(&mut out)
            .expect("EntitySpawnBody fits BODY_LEN");
        out
    }

    /// Encodes as an `Opcode::EntitySpawn` (`0x03F2`) [`Message`].
    pub fn encode(&self) -> Message {
        Message::known(Opcode::EntitySpawn, self.to_bytes())
    }

    /// Compares two bodies byte-for-byte on the wire, distinguishing `-0.0` and NaN payloads.
    pub fn bit_eq(&self, other: &Self) -> bool {
        self.to_bytes() == other.to_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_real_vgr_turret_and_arbitrary_reserved_bytes_losslessly() {
        // Real 0x03F2 ArmoryTurret5v5 (341, actor 2121, slot 35, team 2) capture body.
        let mut raw = [0u8; EntitySpawnBody::BODY_LEN];
        raw[0..4].copy_from_slice(&341u32.to_be_bytes());
        raw[4..8].copy_from_slice(&EntitySpawnBody::STRUCTURE_SKIN.to_be_bytes());
        raw[8..12].copy_from_slice(&2121u32.to_be_bytes());
        raw[12..16].copy_from_slice(&0x427c_0000u32.to_be_bytes()); // 63.0
        raw[16..20].copy_from_slice(&EntitySpawnBody::GROUND_HEIGHT_BITS.to_be_bytes());
        raw[20..24].copy_from_slice(&0x41b3_47aeu32.to_be_bytes()); // 22.41
        raw[24..28].copy_from_slice(&0xbf35_04f4u32.to_be_bytes());
        raw[28..32].copy_from_slice(&0x8000_0000u32.to_be_bytes()); // -0.0 preserved
        raw[32..36].copy_from_slice(&0x3f35_04f2u32.to_be_bytes());
        raw[36..40].copy_from_slice(&5000.0f32.to_be_bytes());
        raw[40..44].copy_from_slice(&5000.0f32.to_be_bytes());
        raw[64..68].copy_from_slice(&1.0f32.to_be_bytes());
        raw[84..88].copy_from_slice(&1.0f32.to_be_bytes());
        raw[88..96].copy_from_slice(&[1; 8]);
        raw[96..104].copy_from_slice(&[1, 1, 15, 1, 1, 1, 1, 1]);
        raw[112..116].copy_from_slice(&u32::MAX.to_be_bytes());
        raw[116..122].copy_from_slice(&[35, 0, 0, 1, 255, 2]);

        let parsed = EntitySpawnBody::read_body(&raw).unwrap();
        assert_eq!(parsed.template, 341);
        assert_eq!(parsed.skin, EntitySpawnBody::STRUCTURE_SKIN);
        assert_eq!(parsed.actor, 2121);
        assert_eq!(parsed.position[0], 63.0);
        assert_eq!(
            parsed.position[1].to_bits(),
            EntitySpawnBody::GROUND_HEIGHT_BITS
        );
        assert_eq!(parsed.forward[1].to_bits(), 0x8000_0000);
        assert_eq!(parsed.hp, 5000.0);
        assert_eq!(parsed.max_hp, 5000.0);
        assert_eq!(parsed.word64, 1.0);
        assert_eq!(parsed.word84, 1.0);
        assert_eq!(parsed.slot, 35);
        assert_eq!(parsed.stat_mask(), EntityStatMask::Full);
        assert_eq!(parsed.wire_team(), WireTeam::Red);
        assert_eq!(parsed.encode().body, raw);

        // Mutate every reserved/float payload bit (including signaling NaN) and check lossless round-trip.
        let mut noisy: [u8; EntitySpawnBody::BODY_LEN] =
            std::array::from_fn(|i| (i as u8).wrapping_mul(37).wrapping_add(11));
        noisy[52..56].copy_from_slice(&0x7f80_0001u32.to_be_bytes());
        noisy[28..32].copy_from_slice(&0x8000_0000u32.to_be_bytes());
        let parsed_noisy = EntitySpawnBody::read_body(&noisy).unwrap();
        assert_eq!(parsed_noisy.encode().body, noisy);

        let mut padded = vec![0u8; EntitySpawnBody::PADDED_LEN];
        padded[..EntitySpawnBody::BODY_LEN].copy_from_slice(&noisy);
        padded[EntitySpawnBody::BODY_LEN..].copy_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd]);
        let (parsed_padded, pad) = EntitySpawnBody::read_padded(&padded).unwrap();
        assert!(parsed_padded.bit_eq(&parsed_noisy));
        assert_eq!(pad, &[0xaa, 0xbb, 0xcc, 0xdd]);
    }

    #[test]
    fn rejects_wrong_lengths_and_leaves_short_output_untouched() {
        let valid = [0x42u8; EntitySpawnBody::BODY_LEN];
        let body = EntitySpawnBody::read_body(&valid).unwrap();
        for bad_len in [0, 1, 121, 123, 126] {
            assert!(EntitySpawnBody::read_body(&vec![0u8; bad_len]).is_err());
        }
        assert!(EntitySpawnBody::read_padded(&[0u8; 125]).is_err());
        let mut short = [0x5au8; 121];
        assert!(body.write_body(&mut short).is_err());
        assert_eq!(short, [0x5au8; 121]);

        let mut longer = [0x99u8; 126];
        body.write_body(&mut longer).unwrap();
        assert_eq!(&longer[..122], &valid);
        assert_eq!(&longer[122..], &[0x99; 4]);
    }

    #[test]
    fn enum_vocabularies_preserve_unknown_bytes() {
        assert_eq!(EntityStatMask::from_wire(0), EntityStatMask::Full);
        assert_eq!(EntityStatMask::from_wire(1), EntityStatMask::Pristine);
        assert_eq!(EntityStatMask::from_wire(7), EntityStatMask::Unknown(7));
        assert_eq!(EntityStatMask::Unknown(7).raw(), 7);

        assert_eq!(WireTeam::from_wire(0), WireTeam::Neutral);
        assert_eq!(WireTeam::from_wire(1), WireTeam::Blue);
        assert_eq!(WireTeam::from_wire(2), WireTeam::Red);
        assert_eq!(WireTeam::from_wire(9), WireTeam::Unknown(9));
        assert_eq!(WireTeam::from_sim_team(0), WireTeam::Blue);
        assert_eq!(WireTeam::from_sim_team(1), WireTeam::Red);
        assert_eq!(WireTeam::from_sim_team(2), WireTeam::Neutral);
    }
}
