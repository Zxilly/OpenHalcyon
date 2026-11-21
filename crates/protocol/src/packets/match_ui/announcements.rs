//! Match announcements (`0x0435`) and player kick/disconnect notices (`0x0479`).

use crate::Opcode;
use crate::codec::fixed_body::fixed_packet;
use binrw::{BinRead, BinWrite};

/// Known `0x0435 MakeAnnouncement` event key identifiers, preserving unknown wire keys losslessly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, BinRead, BinWrite)]
#[brw(big)]
pub enum AnnouncementKey {
    /// Minions have spawned (`0x140be4b6`).
    #[brw(magic = 0x140b_e4b6u32)]
    FirstWave,
    /// First blood kill announcement (`0x46482c5d`).
    #[brw(magic = 0x4648_2c5du32)]
    FirstBlood,
    /// Uncatalogued wire announcement key.
    Unknown(u32),
}

impl AnnouncementKey {
    /// Wire constant for [`Self::FirstWave`].
    pub const FIRST_WAVE_WIRE: u32 = 0x140b_e4b6;
    /// Wire constant for [`Self::FirstBlood`].
    pub const FIRST_BLOOD_WIRE: u32 = 0x4648_2c5d;

    /// Converts a raw big-endian wire key into [`AnnouncementKey`].
    #[must_use]
    pub const fn from_wire(raw: u32) -> Self {
        match raw {
            Self::FIRST_WAVE_WIRE => Self::FirstWave,
            Self::FIRST_BLOOD_WIRE => Self::FirstBlood,
            other => Self::Unknown(other),
        }
    }

    /// Converts [`AnnouncementKey`] back to its 32-bit wire identifier.
    #[must_use]
    pub const fn to_wire(self) -> u32 {
        match self {
            Self::FirstWave => Self::FIRST_WAVE_WIRE,
            Self::FirstBlood => Self::FIRST_BLOOD_WIRE,
            Self::Unknown(other) => other,
        }
    }
}

/// `0x0435 MakeAnnouncement`: 24-byte match announcement frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct AnnouncementBody {
    /// Target recipient wire actor ID or [`Self::NONE`] (`0xffff_ffff`).
    pub recipient: u32,
    /// Subject wire actor ID or [`Self::NONE`].
    pub subject: u32,
    /// Announcement event key.
    pub key: AnnouncementKey,
    /// Primary numeric argument (or [`Self::NONE`]).
    pub arg: u32,
    /// Secondary parameter word (or [`Self::NONE`]).
    pub param1: u32,
    /// Tertiary parameter word (or [`Self::NONE`]).
    pub param2: u32,
}

impl AnnouncementBody {
    /// Sentinel value (`0xffff_ffff`) denoting an absent actor or optional argument.
    pub const NONE: u32 = u32::MAX;
}

fixed_packet!(AnnouncementBody, Opcode::MakeAnnouncement, 24);

/// `0x0479 KickPlayer`: 66-byte player disconnect notification frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct KickPlayerBody {
    /// Zero-padded ASCII UUID buffer of the kicked player.
    pub player_uuid: [u8; 64],
    /// Wire disconnect reason code.
    pub disconnect_code: u16,
}

fixed_packet!(KickPlayerBody, Opcode::KickPlayer, 66);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn announcements_and_kick_roundtrips() {
        let ann = AnnouncementBody {
            recipient: AnnouncementBody::NONE,
            subject: 1501,
            key: AnnouncementKey::FirstBlood,
            arg: 125,
            param1: AnnouncementBody::NONE,
            param2: AnnouncementBody::NONE,
        };
        let ann_msg = ann.encode();
        assert_eq!(ann_msg.body.len(), 24);
        assert_eq!(
            &ann_msg.body[8..12],
            &AnnouncementKey::FIRST_BLOOD_WIRE.to_be_bytes()
        );
        assert_eq!(AnnouncementBody::read_exact(&ann_msg.body).ok(), Some(ann));

        let unknown_ann = AnnouncementBody {
            key: AnnouncementKey::Unknown(0xdead_beef),
            ..ann
        };
        assert_eq!(
            AnnouncementBody::read_exact(&unknown_ann.encode().body).ok(),
            Some(unknown_ann)
        );

        let mut player_uuid = [0u8; 64];
        player_uuid[..4].copy_from_slice(b"uuid");
        let kick = KickPlayerBody {
            player_uuid,
            disconnect_code: 4,
        };
        let kick_msg = kick.encode();
        assert_eq!(kick_msg.body.len(), 66);
        assert_eq!(KickPlayerBody::read_exact(&kick_msg.body).ok(), Some(kick));
    }
}
