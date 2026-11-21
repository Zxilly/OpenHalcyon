//! Scene settings, match settings, draft mode announcements, and catalog entry packets.

use super::{encode_bin, read_bin, read_bin_variable, write_bin};
use crate::codec::invalid;
use crate::{FixedAscii, Message, Opcode};
use binrw::{BinRead, BinWrite};
use std::io;

/// `0x046F` (`Opcode::SceneSettings`, 69B) scene mode asset declaration.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct SceneSettingsBody {
    pub mode: FixedAscii<64>,
    pub scene_word: u32,
    pub flag: u8,
}

impl SceneSettingsBody {
    pub const BODY_LEN: usize = 69;

    pub fn new(mode: &str, flag: u8) -> Self {
        Self {
            mode: FixedAscii::from_str_trunc(mode),
            scene_word: 0,
            flag,
        }
    }

    pub fn read_body(body: &[u8]) -> io::Result<Self> {
        read_bin(body, Self::BODY_LEN, "invalid SceneSettings body")
    }

    pub fn write_body(&self, dst: &mut [u8]) -> io::Result<usize> {
        write_bin(self, dst, Self::BODY_LEN)
    }

    pub fn encode(&self) -> Message {
        encode_bin(Opcode::SceneSettings, self, Self::BODY_LEN)
    }
}

/// 100-byte prefix of `0x03E9` (`Opcode::MatchSettings`) preceding the trailing `team` byte.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct MatchSettingsPrefix {
    pub local_actor: u32,
    pub state_input: u32,
    pub hero_template: u32,
    pub skin_hash: u32,
    pub team_sizes: [u8; 2],
    pub reserved_18: [u8; 14],
    pub mode: FixedAscii<64>,
    pub word96: u32,
}

/// `0x03E9` (`Opcode::MatchSettings`, 101B) local seat, mode, and loading/draft configuration.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct MatchSettingsBody {
    pub local_actor: u32,
    pub state_input: u32,
    pub hero_template: u32,
    pub skin_hash: u32,
    pub team_sizes: [u8; 2],
    pub reserved_18: [u8; 14],
    pub mode: FixedAscii<64>,
    pub word96: u32,
    pub team: u8,
}

impl MatchSettingsBody {
    pub const BODY_LEN: usize = 101;
    pub const PADDED_LEN: usize = 102;
    pub const PREFIX_LEN: usize = 100;
    pub const UNSET_HERO_TEMPLATE: u32 = u16::MAX as u32;

    /// Reads a 101-byte `MatchSettings` body or a 102-byte Blowfish-aligned wire body.
    pub fn read_padded(body: &[u8]) -> io::Result<(Self, &[u8])> {
        if !matches!(body.len(), Self::BODY_LEN | Self::PADDED_LEN) {
            return Err(invalid("invalid padded MatchSettings body"));
        }
        let parsed = Self::read_body(&body[..Self::BODY_LEN])?;
        Ok((parsed, &body[Self::BODY_LEN..]))
    }

    /// Reads the 100-byte prefix from any `MatchSettings` body with optional trailing bytes.
    pub fn read_prefix(body: &[u8]) -> io::Result<MatchSettingsPrefix> {
        read_bin_variable(body, "truncated game setup")
    }

    /// Builds a draft-phase `MatchSettings` (`state_input = 1`, `hero_template = 0xffff`).
    pub fn draft(local_actor: u32, team_size: u8, mode: &str, word96: f32, team: u8) -> Self {
        Self {
            local_actor,
            state_input: 1,
            hero_template: Self::UNSET_HERO_TEMPLATE,
            skin_hash: 0,
            team_sizes: [team_size, team_size],
            reserved_18: [0; 14],
            mode: FixedAscii::from_str_trunc(mode),
            word96: word96.to_bits(),
            team,
        }
    }

    /// Builds a 3-per-side (Halcyon Fold / Blitz / ARAL) draft `MatchSettings`.
    pub fn draft_three(local_actor: u32, mode: &str, team: u8) -> Self {
        Self::draft(local_actor, 3, mode, 5.0, team)
    }

    /// Builds a 5v5 draft `MatchSettings`.
    pub fn draft_five(local_actor: u32, mode: &str) -> Self {
        Self::draft(local_actor, 5, mode, 1.0, 1)
    }

    /// Builds a world/loading `MatchSettings` (`state_input = 0`, `team = 1`).
    pub fn world(local_actor: u32, mode: &str) -> Self {
        Self {
            local_actor,
            state_input: 0,
            hero_template: 0,
            skin_hash: 0,
            team_sizes: [0, 0],
            reserved_18: [0; 14],
            mode: FixedAscii::from_str_trunc(mode),
            word96: 0,
            team: 1,
        }
    }

    pub fn read_body(body: &[u8]) -> io::Result<Self> {
        read_bin(body, Self::BODY_LEN, "invalid MatchSettings body")
    }

    pub fn write_body(&self, dst: &mut [u8]) -> io::Result<usize> {
        write_bin(self, dst, Self::BODY_LEN)
    }

    pub fn encode(&self) -> Message {
        encode_bin(Opcode::MatchSettings, self, Self::BODY_LEN)
    }
}

/// `0x0454` (`Opcode::DraftModeAnnouncement`, 68B) draft mode & team size banner.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct DraftModeAnnouncementBody {
    pub team_size: u32,
    pub mode: FixedAscii<64>,
}

impl DraftModeAnnouncementBody {
    pub const BODY_LEN: usize = 68;

    pub fn new(team_size: u32, mode: &str) -> Self {
        Self {
            team_size,
            mode: FixedAscii::from_str_trunc(mode),
        }
    }

    pub fn read_body(body: &[u8]) -> io::Result<Self> {
        read_bin(body, Self::BODY_LEN, "invalid DraftModeAnnouncement body")
    }

    pub fn write_body(&self, dst: &mut [u8]) -> io::Result<usize> {
        write_bin(self, dst, Self::BODY_LEN)
    }

    pub fn encode(&self) -> Message {
        encode_bin(Opcode::DraftModeAnnouncement, self, Self::BODY_LEN)
    }
}

/// `0x0453` (`Opcode::DraftCatalogEntry`, 32B) hero or skin asset name entry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct DraftCatalogEntryBody {
    pub name: FixedAscii<32>,
}

impl DraftCatalogEntryBody {
    pub const BODY_LEN: usize = 32;

    pub fn new(name: &str) -> Self {
        Self {
            name: FixedAscii::from_str_trunc(name),
        }
    }

    pub fn read_body(body: &[u8]) -> io::Result<Self> {
        read_bin(body, Self::BODY_LEN, "invalid DraftCatalogEntry body")
    }

    pub fn write_body(&self, dst: &mut [u8]) -> io::Result<usize> {
        write_bin(self, dst, Self::BODY_LEN)
    }

    pub fn encode(&self) -> Message {
        encode_bin(Opcode::DraftCatalogEntry, self, Self::BODY_LEN)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Fields;

    #[test]
    fn scene_settings_and_announcements_match_golden_bytes() {
        let scene = SceneSettingsBody::new("*GameMode_5v5_SoloBots*", 1);
        let msg = scene.encode();
        assert_eq!(msg.opcode, Opcode::SceneSettings.raw());
        assert_eq!(msg.body.len(), 69);
        assert_eq!(&msg.body[..24], b"*GameMode_5v5_SoloBots*\0");
        assert_eq!(&msg.body[64..68], &[0; 4]);
        assert_eq!(msg.body[68], 1);
        assert_eq!(SceneSettingsBody::read_body(&msg.body).unwrap(), scene);
        let Fields::NameField {
            kind,
            text,
            remaining,
        } = msg.fields().unwrap()
        else {
            panic!("expected NameField")
        };
        assert_eq!(kind, Opcode::SceneSettings);
        assert_eq!(text, b"*GameMode_5v5_SoloBots*");
        assert_eq!(remaining, &[0, 0, 0, 0, 1]);

        let mode = DraftModeAnnouncementBody::new(3, "*GameMode_Aral_Casual*");
        let mode_msg = mode.encode();
        assert_eq!(mode_msg.opcode, Opcode::DraftModeAnnouncement.raw());
        assert_eq!(mode_msg.body.len(), 68);
        assert_eq!(&mode_msg.body[..4], &3u32.to_be_bytes());
        assert_eq!(&mode_msg.body[4..26], b"*GameMode_Aral_Casual*");
        assert_eq!(
            DraftModeAnnouncementBody::read_body(&mode_msg.body).unwrap(),
            mode
        );

        let entry = DraftCatalogEntryBody::new("*Ringo*");
        let entry_msg = entry.encode();
        assert_eq!(entry_msg.opcode, Opcode::DraftCatalogEntry.raw());
        assert_eq!(entry_msg.body.len(), 32);
        assert_eq!(&entry_msg.body[..8], b"*Ringo*\0");
        assert_eq!(
            DraftCatalogEntryBody::read_body(&entry_msg.body).unwrap(),
            entry
        );
    }

    #[test]
    fn match_settings_matches_draft_and_world_offsets() {
        let three = MatchSettingsBody::draft_three(1502, "*GameMode_Aral_Casual*", 2);
        let msg = three.encode();
        assert_eq!(msg.opcode, Opcode::MatchSettings.raw());
        assert_eq!(msg.body.len(), 101);
        assert_eq!(&msg.body[..4], &1502u32.to_be_bytes());
        assert_eq!(&msg.body[4..8], &[0, 0, 0, 1]);
        assert_eq!(&msg.body[8..12], &[0, 0, 0xff, 0xff]);
        assert_eq!(&msg.body[12..16], &[0; 4]);
        assert_eq!(&msg.body[16..18], &[3, 3]);
        assert_eq!(&msg.body[32..55], b"*GameMode_Aral_Casual*\0");
        assert_eq!(&msg.body[96..100], &5.0f32.to_be_bytes());
        assert_eq!(msg.body[100], 2);
        assert_eq!(MatchSettingsBody::read_body(&msg.body).unwrap(), three);

        let mut world = MatchSettingsBody::world(1500, "*GameMode_5v5_SoloBots*");
        world.hero_template = 243;
        world.skin_hash = 0xf240_ebfe;
        world.team = 1;
        let world_msg = world.encode();
        let Fields::GameSetup {
            first_word,
            state_input,
            word8,
            word12,
            word96,
            mode,
            remaining,
        } = world_msg.fields().unwrap()
        else {
            panic!("expected GameSetup")
        };
        assert_eq!(
            (first_word, state_input, word8, word12, word96),
            (1500, 0, 243, 0xf240_ebfe, 0)
        );
        assert_eq!(mode, b"*GameMode_5v5_SoloBots*");
        assert_eq!(remaining, &[1]);

        let five = MatchSettingsBody::draft_five(1500, "*GameMode_5v5_Casual*");
        assert_eq!(five.team_sizes, [5, 5]);
        assert_eq!(five.word96, 1.0f32.to_bits());
    }

    #[test]
    fn settings_decoders_reject_short_and_oversized_slices() {
        super::super::assert_rejects(SceneSettingsBody::BODY_LEN, SceneSettingsBody::read_body);
        super::super::assert_rejects(MatchSettingsBody::BODY_LEN, MatchSettingsBody::read_body);
        super::super::assert_rejects(
            DraftModeAnnouncementBody::BODY_LEN,
            DraftModeAnnouncementBody::read_body,
        );
        super::super::assert_rejects(
            DraftCatalogEntryBody::BODY_LEN,
            DraftCatalogEntryBody::read_body,
        );
    }
}
