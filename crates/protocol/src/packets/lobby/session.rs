//! Admission UUID, scene entry, scene owner confirmation, and PlayerInfo packets.

use super::{encode_bin, read_bin, read_bin_variable, write_bin};
use crate::{FixedAscii, Message, Opcode};
use binrw::{BinRead, BinWrite};
use std::io::{self, Read, Seek, SeekFrom, Write};

/// `0x03E8` (`Opcode::RegisterPlayerUuid`, 64B) player UUID admission / notification frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct RegisterPlayerUuidBody {
    pub player_uuid: FixedAscii<64>,
}

impl RegisterPlayerUuidBody {
    pub const BODY_LEN: usize = 64;

    pub fn from_str_trunc(uuid: &str) -> Self {
        Self {
            player_uuid: FixedAscii::from_str_trunc(uuid),
        }
    }

    pub fn read_body(body: &[u8]) -> io::Result<Self> {
        read_bin(body, Self::BODY_LEN, "invalid RegisterPlayerUuid body")
    }

    pub fn write_body(&self, dst: &mut [u8]) -> io::Result<usize> {
        write_bin(self, dst, Self::BODY_LEN)
    }

    pub fn encode(&self) -> Message {
        encode_bin(Opcode::RegisterPlayerUuid, self, Self::BODY_LEN)
    }
}

/// `0x03ED` (`Opcode::PlayerLoadedScene`, 72B native / 78B padded) peer scene-entry notice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct PlayerLoadedSceneBody {
    pub player_name: FixedAscii<64>,
    pub seat: u32,
    pub actor: u32,
}

impl PlayerLoadedSceneBody {
    pub const BODY_LEN: usize = 72;
    pub const PADDED_LEN: usize = 78;

    pub fn new(player_name: &str, seat: u32, actor: u32) -> Self {
        Self {
            player_name: FixedAscii::from_str_trunc(player_name),
            seat,
            actor,
        }
    }

    pub fn read_body(body: &[u8]) -> io::Result<Self> {
        read_bin(body, Self::BODY_LEN, "invalid PlayerLoadedScene body")
    }

    /// Reads the 72-byte prefix from any `PlayerLoadedScene` body with optional trailing bytes.
    pub fn read_prefix(body: &[u8]) -> io::Result<Self> {
        read_bin_variable(body, "truncated PlayerLoadedScene body")
    }

    /// Reads the 78-byte cipher-padded frame, returning `(body, trailing_pad)`.
    pub fn read_padded(body: &[u8]) -> io::Result<(Self, [u8; 6])> {
        #[derive(BinRead)]
        #[br(big)]
        struct Padded {
            inner: PlayerLoadedSceneBody,
            pad: [u8; 6],
        }
        let parsed: Padded = read_bin(
            body,
            Self::PADDED_LEN,
            "invalid padded PlayerLoadedScene body",
        )?;
        Ok((parsed.inner, parsed.pad))
    }

    pub fn write_body(&self, dst: &mut [u8]) -> io::Result<usize> {
        write_bin(self, dst, Self::BODY_LEN)
    }

    pub fn encode(&self) -> Message {
        encode_bin(Opcode::PlayerLoadedScene, self, Self::BODY_LEN)
    }

    pub fn encode_padded(&self) -> Message {
        let mut body = vec![0u8; Self::PADDED_LEN];
        self.write_body(&mut body)
            .expect("PlayerLoadedScene fits padded buffer");
        Message::known(Opcode::PlayerLoadedScene, body)
    }
}

/// `0x0471` (`Opcode::ConfirmSceneOwner`, 5B native / 6B cipher-padded) scene-owner confirmation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct ConfirmSceneOwnerBody {
    pub actor: u32,
    pub flag: u8,
}

impl ConfirmSceneOwnerBody {
    pub const BODY_LEN: usize = 5;
    pub const PADDED_LEN: usize = 6;

    pub fn read_body(body: &[u8]) -> io::Result<Self> {
        read_bin(body, Self::BODY_LEN, "invalid ConfirmSceneOwner body")
    }

    /// Reads the 5-byte prefix from any `ConfirmSceneOwner` body with optional trailing bytes.
    pub fn read_prefix(body: &[u8]) -> io::Result<Self> {
        read_bin_variable(body, "truncated ConfirmSceneOwner body")
    }

    /// Reads the 6-byte cipher-padded frame, returning `(body, trailing_pad_byte)`.
    pub fn read_padded(body: &[u8]) -> io::Result<(Self, u8)> {
        #[derive(BinRead)]
        #[br(big)]
        struct Padded {
            inner: ConfirmSceneOwnerBody,
            pad: u8,
        }
        let parsed: Padded = read_bin(
            body,
            Self::PADDED_LEN,
            "invalid padded ConfirmSceneOwner body",
        )?;
        Ok((parsed.inner, parsed.pad))
    }

    pub fn write_body(&self, dst: &mut [u8]) -> io::Result<usize> {
        write_bin(self, dst, Self::BODY_LEN)
    }

    pub fn encode(&self) -> Message {
        encode_bin(Opcode::ConfirmSceneOwner, self, Self::BODY_LEN)
    }

    pub fn encode_padded(&self) -> Message {
        let mut body = [0u8; Self::PADDED_LEN];
        self.write_body(&mut body)
            .expect("ConfirmSceneOwner fits padded buffer");
        Message::known(Opcode::ConfirmSceneOwner, body)
    }
}

/// 172-byte identity & hero summary prefix of `0x03EE` (`Opcode::PlayerInfo`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct PlayerInfoSummaryPrefix {
    pub player_name: FixedAscii<64>,
    pub player_uuid: FixedAscii<64>,
    pub reserved_128: [u8; 32],
    pub actor_id: u32,
    pub hero_template: u32,
    pub skin_hash: u32,
}

/// Common 216-byte prefix of `0x03EE` (`Opcode::PlayerInfo`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct PlayerInfoPrefix {
    pub player_name: FixedAscii<64>,
    pub player_uuid: FixedAscii<64>,
    pub reserved_128: [u8; 32],
    pub actor_id: u32,
    pub hero_template: u32,
    pub skin_hash: u32,
    pub word_172: u32,
    pub fnv_words_176: [u32; 7],
    pub flags_204: [u8; 4],
    pub reserved_208: u8,
    pub mode_tag_209: u8,
    pub team: u8,
    pub reserved_211: u8,
    pub is_bot: u8,
    pub reserved_213: [u8; 2],
    pub is_human: u8,
}

impl PlayerInfoPrefix {
    pub const BODY_LEN: usize = 216;
    pub const CONST_WORD_172: u32 = 0x9241_f10e;
    pub const FNV_WORD_176: u32 = 0x811c_9dc5;
    pub const DEFAULT_MODE_TAG: u8 = 2;

    /// Builds a compact `PlayerInfoPrefix` (zeroed FNV words and UUID) matching `messages::info`.
    pub fn compact(
        player_name: &str,
        actor_id: u32,
        hero_template: u32,
        skin_hash: u32,
        team: u8,
        bot: bool,
    ) -> Self {
        Self {
            player_name: FixedAscii::from_str_trunc(player_name),
            player_uuid: FixedAscii::default(),
            reserved_128: [0; 32],
            actor_id,
            hero_template,
            skin_hash,
            word_172: Self::CONST_WORD_172,
            fnv_words_176: [0; 7],
            flags_204: [0; 4],
            reserved_208: 0,
            mode_tag_209: Self::DEFAULT_MODE_TAG,
            team,
            reserved_211: 0,
            is_bot: u8::from(bot),
            reserved_213: [0; 2],
            is_human: u8::from(!bot),
        }
    }

    /// Builds a full `PlayerInfoPrefix` matching `messages::info(..., true)`.
    pub fn full(
        player_name: &str,
        player_uuid: &str,
        actor_id: u32,
        hero_template: u32,
        skin_hash: u32,
        team: u8,
        bot: bool,
    ) -> Self {
        let mut prefix = Self::compact(player_name, actor_id, hero_template, skin_hash, team, bot);
        prefix.player_uuid = FixedAscii::from_str_trunc(player_uuid);
        prefix.fnv_words_176 = [Self::FNV_WORD_176; 7];
        prefix.flags_204 = [0xff, 0xff, if bot { 0xff } else { 0 }, 0xff];
        prefix
    }
}

/// `0x03EE` (`Opcode::PlayerInfo`, 216B compact or 222B full/padded) player identity & loadout frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerInfoBody {
    Compact(PlayerInfoPrefix),
    Full {
        info: PlayerInfoPrefix,
        trailer: [u8; 6],
    },
}

impl BinRead for PlayerInfoBody {
    type Args<'a> = ();

    fn read_options<R: Read + Seek>(
        reader: &mut R,
        endian: binrw::Endian,
        _args: Self::Args<'_>,
    ) -> binrw::BinResult<Self> {
        let info = PlayerInfoPrefix::read_options(reader, endian, ())?;
        let pos = reader.stream_position()?;
        let end = reader.seek(SeekFrom::End(0))?;
        reader.seek(SeekFrom::Start(pos))?;
        match end - pos {
            0 => Ok(Self::Compact(info)),
            6 => {
                let trailer = <[u8; 6]>::read_options(reader, endian, ())?;
                Ok(Self::Full { info, trailer })
            }
            _ => Err(binrw::Error::AssertFail {
                pos,
                message: "invalid PlayerInfo length (expected 216 or 222 bytes)".into(),
            }),
        }
    }
}

impl BinWrite for PlayerInfoBody {
    type Args<'a> = ();

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        endian: binrw::Endian,
        _args: Self::Args<'_>,
    ) -> binrw::BinResult<()> {
        match self {
            Self::Compact(info) => info.write_options(writer, endian, ()),
            Self::Full { info, trailer } => {
                info.write_options(writer, endian, ())?;
                trailer.write_options(writer, endian, ())
            }
        }
    }
}

impl PlayerInfoBody {
    pub const COMPACT_LEN: usize = PlayerInfoPrefix::BODY_LEN;
    pub const FULL_LEN: usize = 222;

    pub fn compact(info: PlayerInfoPrefix) -> Self {
        Self::Compact(info)
    }

    pub fn full(info: PlayerInfoPrefix) -> Self {
        Self::Full {
            info,
            trailer: [0; 6],
        }
    }

    pub const fn wire_len(&self) -> usize {
        match self {
            Self::Compact(_) => Self::COMPACT_LEN,
            Self::Full { .. } => Self::FULL_LEN,
        }
    }

    pub const fn prefix(&self) -> &PlayerInfoPrefix {
        match self {
            Self::Compact(info) | Self::Full { info, .. } => info,
        }
    }

    pub fn prefix_mut(&mut self) -> &mut PlayerInfoPrefix {
        match self {
            Self::Compact(info) | Self::Full { info, .. } => info,
        }
    }

    /// Clears private human handle and UUID fields while leaving bot records intact.
    pub fn redact(&mut self) {
        let info = self.prefix_mut();
        if info.is_bot == 0 {
            info.player_name = FixedAscii::default();
            info.player_uuid = FixedAscii::default();
        }
    }

    pub fn read_body(body: &[u8]) -> io::Result<Self> {
        read_bin_variable(body, "invalid PlayerInfo body")
    }

    /// Reads the 172-byte identity & hero summary prefix from any `172+`-byte `PlayerInfo` body.
    pub fn read_summary_prefix(body: &[u8]) -> io::Result<PlayerInfoSummaryPrefix> {
        read_bin_variable(body, "truncated PlayerInfo summary")
    }

    pub fn write_body(&self, dst: &mut [u8]) -> io::Result<usize> {
        write_bin(self, dst, self.wire_len())
    }

    pub fn encode(&self) -> Message {
        encode_bin(Opcode::PlayerInfo, self, self.wire_len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_info_compact_and_full_preserve_offsets_and_redact() {
        let human_full = PlayerInfoBody::full(PlayerInfoPrefix::full(
            "Guest",
            "11111111-2222-4333-8444-555555555555",
            1500,
            243,
            0xf240_ebfe,
            1,
            false,
        ));
        let msg = human_full.encode();
        assert_eq!(msg.opcode, Opcode::PlayerInfo.raw());
        assert_eq!(msg.body.len(), 222);
        assert_eq!(&msg.body[..6], b"Guest\0");
        assert_eq!(&msg.body[64..100], b"11111111-2222-4333-8444-555555555555");
        assert_eq!(&msg.body[160..164], &1500u32.to_be_bytes());
        assert_eq!(&msg.body[164..168], &243u32.to_be_bytes());
        assert_eq!(&msg.body[168..172], &0xf240_ebfeu32.to_be_bytes());
        assert_eq!(&msg.body[172..176], &0x9241_f10eu32.to_be_bytes());
        for chunk in msg.body[176..204].as_chunks::<4>().0 {
            assert_eq!(chunk, &0x811c_9dc5u32.to_be_bytes());
        }
        assert_eq!(&msg.body[204..208], &[0xff, 0xff, 0x00, 0xff]);
        assert_eq!(msg.body[209], 2);
        assert_eq!(msg.body[210], 1);
        assert_eq!(msg.body[212], 0);
        assert_eq!(msg.body[215], 1);
        assert_eq!(PlayerInfoBody::read_body(&msg.body).unwrap(), human_full);

        let mut redacted = human_full;
        redacted.redact();
        let redacted_msg = redacted.encode();
        assert_eq!(&redacted_msg.body[..128], &[0u8; 128]);
        assert_eq!(&redacted_msg.body[128..], &msg.body[128..]);

        let bot_compact = PlayerInfoBody::compact(PlayerInfoPrefix::compact(
            "Ringo",
            1501,
            243,
            0x1122_3344,
            2,
            true,
        ));
        let compact_msg = bot_compact.encode();
        assert_eq!(compact_msg.body.len(), 216);
        assert_eq!(&compact_msg.body[..6], b"Ringo\0");
        assert_eq!(&compact_msg.body[64..128], &[0u8; 64]);
        assert_eq!(&compact_msg.body[176..208], &[0u8; 32]);
        assert_eq!(compact_msg.body[212], 1);
        assert_eq!(compact_msg.body[215], 0);
        assert_eq!(
            PlayerInfoBody::read_body(&compact_msg.body).unwrap(),
            bot_compact
        );
        let mut bot_after_redact = bot_compact;
        bot_after_redact.redact();
        assert_eq!(bot_after_redact, bot_compact);

        assert!(PlayerInfoBody::read_body(&vec![0u8; 218]).is_err());
    }

    #[test]
    fn admission_scene_loaded_and_confirm_owner_roundtrip() {
        let reg = RegisterPlayerUuidBody::from_str_trunc("11111111-2222-4333-8444-555555555555");
        let reg_msg = reg.encode();
        assert_eq!(reg_msg.opcode, Opcode::RegisterPlayerUuid.raw());
        assert_eq!(reg_msg.body.len(), 64);
        assert_eq!(
            RegisterPlayerUuidBody::read_body(&reg_msg.body).unwrap(),
            reg
        );

        let loaded = PlayerLoadedSceneBody::new("Guest", 2, 1502);
        let unpadded_msg = loaded.encode();
        assert_eq!(unpadded_msg.opcode, Opcode::PlayerLoadedScene.raw());
        assert_eq!(unpadded_msg.body.len(), 72);
        assert_eq!(
            PlayerLoadedSceneBody::read_body(&unpadded_msg.body).unwrap(),
            loaded
        );

        let mut loaded_msg = loaded.encode_padded();
        assert_eq!(loaded_msg.opcode, Opcode::PlayerLoadedScene.raw());
        assert_eq!(loaded_msg.body.len(), 78);
        assert_eq!(&loaded_msg.body[..6], b"Guest\0");
        assert_eq!(&loaded_msg.body[64..68], &2u32.to_be_bytes());
        assert_eq!(&loaded_msg.body[68..72], &1502u32.to_be_bytes());
        assert_eq!(&loaded_msg.body[72..78], &[0; 6]);
        assert!(PlayerLoadedSceneBody::read_body(&loaded_msg.body).is_err());
        loaded_msg.body[72..78].copy_from_slice(&[1, 2, 3, 4, 5, 6]);
        assert_eq!(
            PlayerLoadedSceneBody::read_padded(&loaded_msg.body).unwrap(),
            (loaded, [1, 2, 3, 4, 5, 6])
        );

        let owner = ConfirmSceneOwnerBody {
            actor: 1500,
            flag: 1,
        };
        let owner_msg = owner.encode_padded();
        assert_eq!(owner_msg.opcode, Opcode::ConfirmSceneOwner.raw());
        assert_eq!(owner_msg.body, [0, 0, 5, 220, 1, 0]);
        assert_eq!(
            ConfirmSceneOwnerBody::read_padded(&owner_msg.body).unwrap(),
            (owner, 0)
        );
        assert_eq!(
            ConfirmSceneOwnerBody::read_body(&owner.encode().body).unwrap(),
            owner
        );
    }

    #[test]
    fn session_decoders_reject_short_and_oversized_slices() {
        super::super::assert_rejects(
            RegisterPlayerUuidBody::BODY_LEN,
            RegisterPlayerUuidBody::read_body,
        );
        super::super::assert_rejects(
            PlayerLoadedSceneBody::BODY_LEN,
            PlayerLoadedSceneBody::read_body,
        );
        super::super::assert_rejects(
            PlayerLoadedSceneBody::PADDED_LEN,
            PlayerLoadedSceneBody::read_padded,
        );
        super::super::assert_rejects(
            ConfirmSceneOwnerBody::BODY_LEN,
            ConfirmSceneOwnerBody::read_body,
        );
        super::super::assert_rejects(
            ConfirmSceneOwnerBody::PADDED_LEN,
            ConfirmSceneOwnerBody::read_padded,
        );
        for bad_len in [215, 217, 221, 223] {
            assert!(PlayerInfoBody::read_body(&vec![0u8; bad_len]).is_err());
        }
    }
}
