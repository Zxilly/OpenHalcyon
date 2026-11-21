//! Big-endian `binrw` wire definitions for Lobby, Draft, Scene, Roster, and PlayerInfo packets.
use crate::{FixedAscii, Message, Opcode, invalid};
use binrw::{BinRead, BinWrite};
use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};

fn read_bin<T>(body: &[u8], expected_len: usize, reason: &'static str) -> io::Result<T>
where
    T: for<'a> BinRead<Args<'a> = ()>,
{
    if body.len() != expected_len {
        return Err(invalid(reason));
    }
    let mut cursor = Cursor::new(body);
    let value = T::read_be(&mut cursor).map_err(|_| invalid(reason))?;
    debug_assert_eq!(cursor.position(), expected_len as u64);
    Ok(value)
}

fn read_bin_variable<T>(body: &[u8], reason: &'static str) -> io::Result<T>
where
    T: for<'a> BinRead<Args<'a> = ()>,
{
    T::read_be(&mut Cursor::new(body)).map_err(|_| invalid(reason))
}

fn write_bin<T>(value: &T, dst: &mut [u8], len: usize) -> io::Result<usize>
where
    T: for<'a> BinWrite<Args<'a> = ()>,
{
    let out = dst
        .get_mut(..len)
        .ok_or_else(|| invalid("lobby wire output buffer is too small"))?;
    let mut writer = Cursor::new(out);
    value
        .write_be(&mut writer)
        .map_err(|_| invalid("invalid lobby wire body size"))?;
    debug_assert_eq!(writer.position(), len as u64);
    Ok(len)
}

fn encode_bin<T>(opcode: Opcode, value: &T, len: usize) -> Message
where
    T: for<'a> BinWrite<Args<'a> = ()>,
{
    let mut body = vec![0u8; len];
    write_bin(value, &mut body, len).expect("fixed lobby message fits allocated body");
    Message::known(opcode, body)
}

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

/// `0x0480` (`Opcode::AralRerollState`, 82B native / 86B cipher-padded) ARAL reroll state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct AralRerollStateBody {
    pub cost: u32,
    pub limit: u32,
    pub used: u32,
    pub reserved_12: [u8; 64],
    pub allow_override: u8,
    pub balance: u32,
    pub locked: u8,
    pub reserved_tail: [u8; 4],
}

impl Default for AralRerollStateBody {
    fn default() -> Self {
        Self {
            cost: 0,
            limit: 0,
            used: 0,
            reserved_12: [0; 64],
            allow_override: 0,
            balance: 0,
            locked: 0,
            reserved_tail: [0; 4],
        }
    }
}

impl AralRerollStateBody {
    pub const NATIVE_LEN: usize = 82;
    pub const BODY_LEN: usize = 86;

    pub fn new(
        cost: u32,
        limit: u32,
        used: u32,
        allow_override: u8,
        balance: u32,
        locked: u8,
    ) -> Self {
        Self {
            cost,
            limit,
            used,
            reserved_12: [0; 64],
            allow_override,
            balance,
            locked,
            reserved_tail: [0; 4],
        }
    }

    /// Reads either the 82-byte unpadded Android native body or the 86-byte padded body.
    pub fn read_body(body: &[u8]) -> io::Result<Self> {
        #[derive(BinRead)]
        #[br(big)]
        struct NativePrefix {
            cost: u32,
            limit: u32,
            used: u32,
            reserved_12: [u8; 64],
            allow_override: u8,
            balance: u32,
            locked: u8,
        }

        if body.len() == Self::NATIVE_LEN {
            let prefix: NativePrefix = read_bin(
                body,
                Self::NATIVE_LEN,
                "invalid native AralRerollState body",
            )?;
            return Ok(Self {
                cost: prefix.cost,
                limit: prefix.limit,
                used: prefix.used,
                reserved_12: prefix.reserved_12,
                allow_override: prefix.allow_override,
                balance: prefix.balance,
                locked: prefix.locked,
                reserved_tail: [0; 4],
            });
        }
        read_bin(body, Self::BODY_LEN, "invalid AralRerollState body")
    }

    pub fn write_body(&self, dst: &mut [u8]) -> io::Result<usize> {
        write_bin(self, dst, Self::BODY_LEN)
    }

    pub fn encode(&self) -> Message {
        encode_bin(Opcode::AralRerollState, self, Self::BODY_LEN)
    }
}

/// Single 6-byte entry inside `0x045C` (`Opcode::SceneReadinessRows`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct SceneReadinessRow {
    pub reference: u32,
    pub gate: u8,
    pub flag: u8,
}

impl SceneReadinessRow {
    pub const BODY_LEN: usize = 6;
}

/// `0x045C` (`Opcode::SceneReadinessRows`, 96B) 16-row scene readiness table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct SceneReadinessRowsBody {
    pub rows: [SceneReadinessRow; 16],
}

impl SceneReadinessRowsBody {
    pub const ROW_COUNT: usize = 16;
    pub const BODY_LEN: usize = 96;

    pub fn read_body(body: &[u8]) -> io::Result<Self> {
        read_bin(body, Self::BODY_LEN, "invalid SceneReadinessRows body")
    }

    /// Reads the 96-byte readiness table from any `96+`-byte `SceneReadinessRows` body.
    pub fn read_prefix(body: &[u8]) -> io::Result<Self> {
        read_bin_variable(body, "truncated control entries")
    }

    pub fn write_body(&self, dst: &mut [u8]) -> io::Result<usize> {
        write_bin(self, dst, Self::BODY_LEN)
    }

    pub fn encode(&self) -> Message {
        encode_bin(Opcode::SceneReadinessRows, self, Self::BODY_LEN)
    }
}

/// Three-byte draft preference block at offsets `12..15` inside each `DraftSlotBody`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct DraftPreferenceFlags {
    pub reserved_12: u8,
    pub position: u8,
    pub build_path: u8,
}

impl Default for DraftPreferenceFlags {
    fn default() -> Self {
        Self {
            reserved_12: u8::MAX,
            position: u8::MAX,
            build_path: u8::MAX,
        }
    }
}

impl DraftPreferenceFlags {
    pub const BODY_LEN: usize = 3;

    pub fn new(position: Option<u8>, build_path: Option<u8>) -> Self {
        Self {
            reserved_12: u8::MAX,
            position: position.unwrap_or(u8::MAX),
            build_path: build_path.unwrap_or(u8::MAX),
        }
    }

    pub fn read_body(body: &[u8]) -> io::Result<Self> {
        read_bin(body, Self::BODY_LEN, "invalid DraftPreferenceFlags body")
    }

    pub fn write_body(&self, dst: &mut [u8]) -> io::Result<usize> {
        write_bin(self, dst, Self::BODY_LEN)
    }
}

/// 161-byte seat slot row inside `0x0459` (`DraftRosterFull`) and `0x045A` (`DraftRosterAral`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct DraftSlotBody {
    /// Bytes `0..8`. On slot `0`, bytes `0..8` carry the draft `remaining_seconds` and
    /// `allotted_seconds` floats on the wire.
    pub prefix_0: [u8; 8],
    pub occupied: u8,
    pub slot: u8,
    pub selected: u8,
    pub locked: u8,
    pub reserved_12: u8,
    pub position: u8,
    pub build_path: u8,
    pub reserved_15: u8,
    pub team: u8,
    pub template: u16,
    pub entity: u16,
    pub skin: u32,
    pub display_name: FixedAscii<32>,
    pub reserved_57: [u8; 48],
    pub identity: FixedAscii<36>,
    pub reserved_141: [u8; 20],
}

impl DraftSlotBody {
    pub const BODY_LEN: usize = 161;
    pub const ARAL_EMPTY_SKIN: u32 = 0x10c2_bad9;

    /// Builds an unoccupied draft slot with `slot = index`, unset `0xff` preferences/IDs,
    /// and the caller-supplied default `skin` hash (`0x10c2bad9` for ARAL, `0` for 5v5/3v3).
    pub fn empty_with_skin(index: u8, skin: u32) -> Self {
        Self {
            prefix_0: [0; 8],
            occupied: 0,
            slot: index,
            selected: 0,
            locked: 0,
            reserved_12: u8::MAX,
            position: u8::MAX,
            build_path: u8::MAX,
            reserved_15: 0,
            team: 0,
            template: u16::MAX,
            entity: u16::MAX,
            skin,
            display_name: FixedAscii::default(),
            reserved_57: [0; 48],
            identity: FixedAscii::default(),
            reserved_141: [0; 20],
        }
    }

    pub fn empty(index: u8) -> Self {
        Self::empty_with_skin(index, 0)
    }

    pub const fn preferences(&self) -> DraftPreferenceFlags {
        DraftPreferenceFlags {
            reserved_12: self.reserved_12,
            position: self.position,
            build_path: self.build_path,
        }
    }

    pub fn set_preferences(&mut self, prefs: DraftPreferenceFlags) {
        self.reserved_12 = prefs.reserved_12;
        self.position = prefs.position;
        self.build_path = prefs.build_path;
    }

    pub fn read_body(body: &[u8]) -> io::Result<Self> {
        read_bin(body, Self::BODY_LEN, "invalid DraftSlotBody")
    }

    pub fn write_body(&self, dst: &mut [u8]) -> io::Result<usize> {
        write_bin(self, dst, Self::BODY_LEN)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
struct DraftRosterTimers {
    remaining: f32,
    allotted: f32,
}

#[derive(BinRead, BinWrite)]
#[brw(big)]
struct DraftRosterWire {
    slots: [DraftSlotBody; 16],
    trailer: [u8; 8],
}

/// Wire payload for `0x045A` (`DraftRosterAral`, 2584B) and `0x0459` (`DraftRosterFull`, 2590B).
///
/// On the wire, the 8-byte draft timer header `(remaining_seconds, allotted_seconds)` overlays
/// bytes `0..8` of `slots[0]` (whose semantic fields begin at offset `8`), followed by `slots[1..16]`
/// through offset `2576`, an 8-byte structural `trailer` (`2576..2584`), and optional 6-byte
/// transport `padding` (`2584..2590`).
#[derive(Clone, Debug, PartialEq)]
pub struct DraftRosterBody {
    pub remaining_seconds: f32,
    pub allotted_seconds: f32,
    pub slots: [DraftSlotBody; 16],
    pub trailer: [u8; 8],
    pub padding: Option<[u8; 6]>,
}

impl BinRead for DraftRosterBody {
    type Args<'a> = ();

    fn read_options<R: Read + Seek>(
        reader: &mut R,
        endian: binrw::Endian,
        _args: Self::Args<'_>,
    ) -> binrw::BinResult<Self> {
        let wire = DraftRosterWire::read_options(reader, endian, ())?;
        let pos = reader.stream_position()?;
        let end = reader.seek(SeekFrom::End(0))?;
        reader.seek(SeekFrom::Start(pos))?;
        let padding = match end - pos {
            0 => None,
            6 => Some(<[u8; 6]>::read_options(reader, endian, ())?),
            _ => {
                return Err(binrw::Error::AssertFail {
                    pos,
                    message: "invalid DraftRoster length (expected 2584 or 2590 bytes)".into(),
                });
            }
        };
        let (remaining_seconds, allotted_seconds) = Self::read_timers(&wire.slots[0].prefix_0)
            .map_err(|err| binrw::Error::AssertFail {
                pos: 0,
                message: err.to_string(),
            })?;
        Ok(Self {
            remaining_seconds,
            allotted_seconds,
            slots: wire.slots,
            trailer: wire.trailer,
            padding,
        })
    }
}

impl BinWrite for DraftRosterBody {
    type Args<'a> = ();

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        endian: binrw::Endian,
        _args: Self::Args<'_>,
    ) -> binrw::BinResult<()> {
        let mut slots = self.slots;
        Self::write_timers_to(
            self.remaining_seconds,
            self.allotted_seconds,
            &mut slots[0].prefix_0,
        );
        DraftRosterWire {
            slots,
            trailer: self.trailer,
        }
        .write_options(writer, endian, ())?;
        if let Some(pad) = self.padding {
            pad.write_options(writer, endian, ())?;
        }
        Ok(())
    }
}

impl DraftRosterBody {
    pub const SLOT_COUNT: usize = 16;
    pub const SLOT_BYTES: usize = DraftSlotBody::BODY_LEN;
    pub const BODY_LEN: usize = 2584;
    pub const PADDED_LEN: usize = 2590;

    /// Builds a 2584-byte `DraftRosterAral` body (`padding = None`).
    pub fn aral(remaining_seconds: f32, allotted_seconds: f32, slots: [DraftSlotBody; 16]) -> Self {
        let mut out = Self {
            remaining_seconds,
            allotted_seconds,
            slots,
            trailer: [0; 8],
            padding: None,
        };
        out.sync_slot0_timer_prefix();
        out
    }

    /// Builds a 2590-byte `DraftRosterFull` body (`padding = Some([0; 6])`).
    pub fn full(remaining_seconds: f32, allotted_seconds: f32, slots: [DraftSlotBody; 16]) -> Self {
        let mut out = Self {
            remaining_seconds,
            allotted_seconds,
            slots,
            trailer: [0; 8],
            padding: Some([0; 6]),
        };
        out.sync_slot0_timer_prefix();
        out
    }

    fn write_timers_to(remaining: f32, allotted: f32, dst: &mut [u8; 8]) {
        write_bin(
            &DraftRosterTimers {
                remaining,
                allotted,
            },
            dst,
            8,
        )
        .expect("8-byte timer header fits prefix_0");
    }

    fn sync_slot0_timer_prefix(&mut self) {
        Self::write_timers_to(
            self.remaining_seconds,
            self.allotted_seconds,
            &mut self.slots[0].prefix_0,
        );
    }

    /// Clears admission UUID identities for the first `count` slots (observer snapshots).
    pub fn redact_identities(&mut self, count: usize) {
        for slot in self.slots.iter_mut().take(count) {
            slot.identity = FixedAscii::default();
        }
    }

    /// Clears display names and UUID identities on all non-bot slots (replay recording sanitization).
    pub fn redact_non_bots(&mut self) {
        for slot in &mut self.slots {
            if !slot
                .identity
                .as_bytes()
                .starts_with(b"__Kindred_Player_Bot__")
            {
                slot.display_name = FixedAscii::default();
                slot.identity = FixedAscii::default();
            }
        }
    }

    pub const fn wire_len(&self) -> usize {
        if self.padding.is_some() {
            Self::PADDED_LEN
        } else {
            Self::BODY_LEN
        }
    }

    pub fn read_timers(body: &[u8]) -> io::Result<(f32, f32)> {
        let timers: DraftRosterTimers = read_bin(body, 8, "invalid DraftRoster timer header")?;
        Ok((timers.remaining, timers.allotted))
    }

    /// Reads the 8-byte draft timer header from any `8+`-byte body.
    pub fn read_timers_prefix(body: &[u8]) -> io::Result<(f32, f32)> {
        let timers: DraftRosterTimers =
            read_bin_variable(body, "invalid DraftRoster timer header")?;
        Ok((timers.remaining, timers.allotted))
    }

    pub fn read_body(body: &[u8]) -> io::Result<Self> {
        read_bin_variable(body, "invalid DraftRoster body")
    }

    pub fn write_body(&self, dst: &mut [u8]) -> io::Result<usize> {
        write_bin(self, dst, self.wire_len())
    }

    pub fn encode_aral(&self) -> Message {
        if self.padding.is_none() {
            encode_bin(Opcode::DraftRosterAral, self, Self::BODY_LEN)
        } else {
            let mut unpadded = self.clone();
            unpadded.padding = None;
            encode_bin(Opcode::DraftRosterAral, &unpadded, Self::BODY_LEN)
        }
    }

    pub fn encode_full(&self) -> Message {
        if self.padding.is_some() {
            encode_bin(Opcode::DraftRosterFull, self, Self::PADDED_LEN)
        } else {
            let mut padded = self.clone();
            padded.padding = Some([0; 6]);
            encode_bin(Opcode::DraftRosterFull, &padded, Self::PADDED_LEN)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AralRoster, DraftRoster, Fields, LiveFields};

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
    }

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
    fn aral_reroll_state_and_readiness_rows_roundtrip() {
        let reroll = AralRerollStateBody::new(1, 5, 0, 1, 3, 0);
        let msg = reroll.encode();
        assert_eq!(msg.opcode, Opcode::AralRerollState.raw());
        assert_eq!(msg.body.len(), 86);
        let mut expected = [0u8; 86];
        for (i, v) in [(3, 1), (7, 5), (76, 1), (80, 3)] {
            expected[i] = v;
        }
        assert_eq!(msg.body, expected);
        assert_eq!(AralRerollStateBody::read_body(&msg.body).unwrap(), reroll);
        assert_eq!(
            AralRerollStateBody::read_body(&expected[..82]).unwrap(),
            reroll
        );
        let Fields::Live(LiveFields::AralRerollState {
            cost,
            limit,
            used,
            allow_override,
            balance,
            locked,
        }) = msg.fields().unwrap()
        else {
            panic!("expected AralRerollState")
        };
        assert_eq!(
            (cost, limit, used, allow_override, balance, locked),
            (1, 5, 0, 1, 3, 0)
        );

        let mut table = SceneReadinessRowsBody::default();
        table.rows[0] = SceneReadinessRow {
            reference: 1500,
            gate: 1,
            flag: 0,
        };
        table.rows[15] = SceneReadinessRow {
            reference: 1515,
            gate: 1,
            flag: 1,
        };
        let table_msg = table.encode();
        assert_eq!(table_msg.opcode, Opcode::SceneReadinessRows.raw());
        assert_eq!(table_msg.body.len(), 96);
        assert_eq!(&table_msg.body[..6], &[0, 0, 5, 220, 1, 0]);
        assert_eq!(
            SceneReadinessRowsBody::read_body(&table_msg.body).unwrap(),
            table
        );
    }

    #[test]
    fn draft_roster_matches_aral_and_full_wire_layouts_losslessly() {
        let mut slots = std::array::from_fn(|i| {
            DraftSlotBody::empty_with_skin(i as u8, DraftSlotBody::ARAL_EMPTY_SKIN)
        });
        slots[0].occupied = 1;
        slots[0].set_preferences(DraftPreferenceFlags::new(Some(5), Some(2)));
        slots[0].selected = 1;
        slots[0].locked = 0;
        slots[0].team = 1;
        slots[0].template = 243;
        slots[0].entity = 1500;
        slots[0].skin = 0xf240_ebfe;
        slots[0].display_name = FixedAscii::from_str_trunc("Guest");
        slots[0].identity = FixedAscii::from_str_trunc("11111111-2222-4333-8444-555555555555");
        slots[0].reserved_141[19] = 0x55;

        let mut aral = DraftRosterBody::aral(12.5, 20.0, slots);
        aral.trailer[7] = 0x77;
        let aral_msg = aral.encode_aral();
        assert_eq!(aral_msg.opcode, Opcode::DraftRosterAral.raw());
        assert_eq!(aral_msg.body.len(), 2584);
        assert_eq!(&aral_msg.body[..4], &12.5f32.to_be_bytes());
        assert_eq!(&aral_msg.body[4..8], &20.0f32.to_be_bytes());
        assert_eq!(aral_msg.body[8], 1);
        assert_eq!(aral_msg.body[9], 0);
        assert_eq!(&aral_msg.body[12..15], &[0xff, 5, 2]);
        assert_eq!(&aral_msg.body[17..19], &243u16.to_be_bytes());
        assert_eq!(&aral_msg.body[19..21], &1500u16.to_be_bytes());
        assert_eq!(&aral_msg.body[21..25], &0xf240_ebfeu32.to_be_bytes());
        assert_eq!(&aral_msg.body[25..31], b"Guest\0");
        assert_eq!(
            &aral_msg.body[105..141],
            b"11111111-2222-4333-8444-555555555555"
        );
        assert_eq!(aral_msg.body[160], 0x55);
        assert_eq!(aral_msg.body[161 + 9], 1);
        assert_eq!(
            &aral_msg.body[161 + 21..161 + 25],
            &DraftSlotBody::ARAL_EMPTY_SKIN.to_be_bytes()
        );
        assert_eq!(aral_msg.body[2583], 0x77);
        assert_eq!(DraftRosterBody::read_body(&aral_msg.body).unwrap(), aral);

        let aral_view = AralRoster::new(&aral_msg.body).unwrap();
        assert_eq!(aral_view.remaining_seconds(), 12.5);
        assert_eq!(aral_view.allotted_seconds(), 20.0);
        assert_eq!(aral_view.slot(0).unwrap().entity, 1500);
        assert_eq!(aral_view.trailer()[7], 0x77);

        let mut full = DraftRosterBody::full(300.0, 300.0, slots);
        full.padding = Some([1, 2, 3, 4, 5, 6]);
        let full_msg = full.encode_full();
        assert_eq!(full_msg.opcode, Opcode::DraftRosterFull.raw());
        assert_eq!(full_msg.body.len(), 2590);
        assert_eq!(&full_msg.body[2584..2590], &[1, 2, 3, 4, 5, 6]);
        assert_eq!(DraftRosterBody::read_body(&full_msg.body).unwrap(), full);

        let full_view = DraftRoster::new(&full_msg.body).unwrap();
        assert_eq!(full_view.remaining_seconds(), 300.0);
        assert_eq!(full_view.slot(0).unwrap().display_name, b"Guest");
        assert_eq!(aral_view.decode_body().unwrap(), aral);
        assert!(DraftRosterBody::read_body(&full_msg.body[..2589]).is_err());

        // `encode_aral` always emits the unpadded 2584B ARAL roster even if padding was set.
        let mut aral_with_pad = aral.clone();
        aral_with_pad.padding = Some([1, 2, 3, 4, 5, 6]);
        let forced_aral = aral_with_pad.encode_aral();
        assert_eq!(forced_aral.opcode, Opcode::DraftRosterAral.raw());
        assert_eq!(forced_aral.body.len(), DraftRosterBody::BODY_LEN);
        assert_eq!(forced_aral.body, aral_msg.body);

        // Timer bit patterns (including -0.0 and signaling NaN) must round-trip losslessly.
        let mut raw_nan = vec![0u8; DraftRosterBody::BODY_LEN];
        raw_nan[..4].copy_from_slice(&0x7f80_0051u32.to_be_bytes());
        raw_nan[4..8].copy_from_slice(&0x8000_0000u32.to_be_bytes());
        let decoded_nan = DraftRosterBody::read_body(&raw_nan).unwrap();
        assert_eq!(decoded_nan.encode_aral().body, raw_nan);
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

        let five = MatchSettingsBody::draft_five(1500, "*GameMode_5v5_Casual*");
        assert_eq!(five.team_sizes, [5, 5]);
        assert_eq!(five.word96, 1.0f32.to_bits());
    }

    #[test]
    fn decoders_reject_short_and_oversized_slices() {
        fn assert_rejects<T>(len: usize, f: impl Fn(&[u8]) -> io::Result<T>) {
            assert!(f(&vec![0u8; len.saturating_sub(1)]).is_err());
            assert!(f(&vec![0u8; len + 1]).is_err());
        }

        assert_rejects(
            RegisterPlayerUuidBody::BODY_LEN,
            RegisterPlayerUuidBody::read_body,
        );
        assert_rejects(
            PlayerLoadedSceneBody::BODY_LEN,
            PlayerLoadedSceneBody::read_body,
        );
        assert_rejects(
            PlayerLoadedSceneBody::PADDED_LEN,
            PlayerLoadedSceneBody::read_padded,
        );
        assert_rejects(
            ConfirmSceneOwnerBody::BODY_LEN,
            ConfirmSceneOwnerBody::read_body,
        );
        assert_rejects(
            ConfirmSceneOwnerBody::PADDED_LEN,
            ConfirmSceneOwnerBody::read_padded,
        );
        assert_rejects(SceneSettingsBody::BODY_LEN, SceneSettingsBody::read_body);
        assert_rejects(MatchSettingsBody::BODY_LEN, MatchSettingsBody::read_body);
        assert_rejects(
            DraftModeAnnouncementBody::BODY_LEN,
            DraftModeAnnouncementBody::read_body,
        );
        assert_rejects(
            DraftCatalogEntryBody::BODY_LEN,
            DraftCatalogEntryBody::read_body,
        );
        assert_rejects(
            SceneReadinessRowsBody::BODY_LEN,
            SceneReadinessRowsBody::read_body,
        );
        assert_rejects(
            DraftPreferenceFlags::BODY_LEN,
            DraftPreferenceFlags::read_body,
        );
        assert_rejects(DraftSlotBody::BODY_LEN, DraftSlotBody::read_body);
        assert_rejects(8, DraftRosterBody::read_timers);

        for bad_len in [215, 217, 221, 223] {
            assert!(PlayerInfoBody::read_body(&vec![0u8; bad_len]).is_err());
        }
        for bad_len in [81, 83, 85, 87] {
            assert!(AralRerollStateBody::read_body(&vec![0u8; bad_len]).is_err());
        }
        for bad_len in [2583, 2585, 2589, 2591] {
            assert!(DraftRosterBody::read_body(&vec![0u8; bad_len]).is_err());
        }
    }

    #[test]
    fn roster_redaction_clears_human_identities_and_preserves_bot_sentinel() {
        let mut slots = std::array::from_fn(|i| DraftSlotBody::empty(i as u8));
        slots[0].occupied = 1;
        slots[0].display_name = FixedAscii::from_str_trunc("Guest");
        slots[0].identity = FixedAscii::from_str_trunc("11111111-2222-4333-8444-555555555555");
        slots[1].occupied = 1;
        slots[1].display_name = FixedAscii::from_str_trunc("Ringo Bot");
        slots[1].identity = FixedAscii::from_str_trunc("__Kindred_Player_Bot__");

        let mut observer = DraftRosterBody::full(120.0, 300.0, slots);
        observer.redact_identities(1);
        assert_eq!(observer.slots[0].display_name.trimmed_bytes(), b"Guest");
        assert!(observer.slots[0].identity.trimmed_bytes().is_empty());
        assert_eq!(
            observer.slots[1].identity.trimmed_bytes(),
            b"__Kindred_Player_Bot__"
        );

        let mut recorded = DraftRosterBody::full(120.0, 300.0, slots);
        recorded.redact_non_bots();
        assert!(recorded.slots[0].display_name.trimmed_bytes().is_empty());
        assert!(recorded.slots[0].identity.trimmed_bytes().is_empty());
        assert_eq!(recorded.slots[1].display_name.trimmed_bytes(), b"Ringo Bot");
        assert_eq!(
            recorded.slots[1].identity.trimmed_bytes(),
            b"__Kindred_Player_Bot__"
        );
    }
}
