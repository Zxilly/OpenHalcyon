//! Fixed binary match-event, ability-request, cooldown, entity-state, and announcement packets.
//!
//! All wire layouts follow the crate-wide big-endian `binrw` conventions and preserve trailing
//! frame padding losslessly via [`crate::fixed_body`].

use crate::fixed_body::fixed_packet;
use crate::opcode::Opcode;
use binrw::{BinRead, BinWrite};

pub use crate::abilities::{
    GrantAbilityBody, ModifyCooldownBody, PlayPointAbilityBody, PlayTargetAbilityBody,
};
pub use crate::native_motion::{ActorBounceBody, MoveAndFaceBody, ServerMoveBody, StopActorBody};
pub use crate::statistics::StatisticsWireBody;
pub use crate::surrender::{SurrenderPhase, SurrenderVote};

/// Typed alias for [`crate::ResourceChange`] (`0x041d ModifyActorResource`).
pub type EntityStatBody = crate::ResourceChange;

/// Typed alias for [`crate::AttributeChange`] (`0x041c ModifyActorAttribute`).
pub type ActorAttributeBody = crate::AttributeChange;

/// `0x03f8 MoveTo`: 9-byte actor movement target update.
#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct MoveToBody {
    /// Wire slot or selector byte.
    pub selector: u8,
    /// Destination X world coordinate.
    pub x: f32,
    /// Destination Y world coordinate.
    pub y: f32,
}

fixed_packet!(MoveToBody, Opcode::MoveTo, 9);

/// `0x03f5 RequestMovePoint`: 8-byte client ground movement request.
#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct MovePointRequestBody {
    /// Requested X world coordinate.
    pub x: f32,
    /// Requested Y world coordinate.
    pub y: f32,
}

fixed_packet!(MovePointRequestBody, Opcode::RequestMovePoint, 8);

/// `0x0409 TeleportTo`: 17-byte immediate actor relocation packet.
#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct TeleportToBody {
    /// Wire actor identifier.
    pub actor: u32,
    /// Destination world coordinates `[x, y, z]`.
    pub position: [f32; 3],
    /// Trailing teleport mode/snap flag byte.
    pub flag: u8,
}

fixed_packet!(TeleportToBody, Opcode::TeleportTo, 17);

/// `0x040b DestroyEntity`: 4-byte entity removal packet (often padded to 6 bytes on the wire).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct DestroyEntityBody {
    /// Wire actor identifier to destroy.
    pub actor: u32,
}

fixed_packet!(DestroyEntityBody, Opcode::DestroyEntity, 4);

/// `0x042f ActorUndead`: 4-byte actor undeath/survival transition packet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct ActorUndeadBody {
    /// Wire actor identifier.
    pub actor: u32,
}

fixed_packet!(ActorUndeadBody, Opcode::ActorUndead, 4);

/// `0x0431 ActorDead`: 4-byte actor death notification (often padded to 6 bytes on the wire).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct ActorDeadBody {
    /// Wire actor identifier that died.
    pub actor: u32,
}

fixed_packet!(ActorDeadBody, Opcode::ActorDead, 4);

/// `0x0432 ActorRespawn`: 16-byte actor respawn packet (often padded to 22 bytes on the wire).
#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct ActorRespawnBody {
    /// Wire actor identifier respawning.
    pub actor: u32,
    /// Respawn world coordinates `[x, y, z]`.
    pub position: [f32; 3],
}

fixed_packet!(ActorRespawnBody, Opcode::ActorRespawn, 16);

/// `0x0433 ActorRespawnTimer`: 8-byte actor respawn countdown packet.
#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct RespawnTimerBody {
    /// Wire actor identifier.
    pub actor: u32,
    /// Remaining respawn duration in seconds.
    pub seconds: f32,
}

fixed_packet!(RespawnTimerBody, Opcode::ActorRespawnTimer, 8);

/// `0x0434 LevelUp`: 8-byte actor level-up packet (often padded to 14 bytes on the wire).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct LevelUpBody {
    /// Wire actor identifier.
    pub actor: u32,
    /// Level/points operand.
    pub operand: u32,
}

fixed_packet!(LevelUpBody, Opcode::LevelUp, 8);

/// `0x041f ModifyGameModeVar`: 8-byte game mode variable assignment (often padded to 14 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct ModifyGameModeVarBody {
    /// Variable key hash/identifier.
    pub key: u32,
    /// Assigned integer value.
    pub value: u32,
}

fixed_packet!(ModifyGameModeVarBody, Opcode::ModifyGameModeVar, 8);

/// `0x0451 SyncWorldClock`: 4-byte authoritative match clock synchronization packet.
#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct WorldClockBody {
    /// Elapsed world time in seconds.
    pub seconds: f32,
}

fixed_packet!(WorldClockBody, Opcode::SyncWorldClock, 4);

/// `0x0000 ClockSync`: 4-byte heartbeat/clock synchronization packet.
#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct ClockSyncBody {
    /// Clock value in seconds.
    pub seconds: f32,
}

fixed_packet!(ClockSyncBody, Opcode::ClockSync, 4);

/// `0x043d ItemGrant`: 12-byte inventory item grant packet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct ItemGrantBody {
    /// Receiving wire actor identifier.
    pub actor: u32,
    /// Item definition identifier.
    pub item: u32,
    /// Newly assigned item instance identifier.
    pub instance: u32,
}

fixed_packet!(ItemGrantBody, Opcode::ItemGrant, 12);

/// `0x044b ConsumeItem`: 10-byte inventory item removal/consumption packet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct ConsumeItemBody {
    /// Owning wire actor identifier.
    pub actor: u32,
    /// Item instance identifier being removed.
    pub instance: u32,
    /// Trailing removal/consumption flags.
    pub flags: u16,
}

fixed_packet!(ConsumeItemBody, Opcode::ConsumeItem, 10);

/// `0x0439 BuyItem`: 8-byte item purchase request packet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct BuyItemBody {
    /// Purchasing wire actor identifier.
    pub actor: u32,
    /// Item definition identifier.
    pub item: u32,
}

fixed_packet!(BuyItemBody, Opcode::BuyItem, 8);

/// 18-byte preset payload inside `0x048a InitCooldown` (`key`, `value`, `config_value`, `flags`).
#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct InitCooldownPayload {
    /// Cooldown slot/definition key.
    pub key: u32,
    /// Initial current remaining cooldown seconds.
    pub value: f32,
    /// Configured base cooldown duration seconds.
    pub config_value: f32,
    /// Trailing preset flag bytes.
    pub flags: [u8; 6],
}

impl InitCooldownPayload {
    /// Fixed byte width of the cooldown preset payload.
    pub const LEN: usize = 18;

    /// Serializes this 18-byte preset payload using big-endian `binrw`.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; Self::LEN] {
        let mut out = [0; Self::LEN];
        crate::fixed_body::write(self, &mut out, Self::LEN)
            .expect("18-byte InitCooldownPayload fits output array");
        out
    }

    /// Reads an 18-byte preset payload using big-endian `binrw`.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; Self::LEN]) -> Self {
        crate::fixed_body::read(bytes).expect("18-byte slice is valid InitCooldownPayload")
    }
}

/// `0x048a InitCooldown`: 22-byte initial ability/item cooldown slot registration.
#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct InitCooldownBody {
    /// Wire actor identifier.
    pub owner: u32,
    /// Cooldown slot/definition key.
    pub key: u32,
    /// Initial current remaining cooldown seconds.
    pub value: f32,
    /// Configured base cooldown duration seconds.
    pub config_value: f32,
    /// Trailing preset flag bytes.
    pub flags: [u8; 6],
}

fixed_packet!(InitCooldownBody, Opcode::InitCooldown, 22);

impl InitCooldownBody {
    /// Raw offset where the 18-byte preset payload begins after [`Self::owner`].
    pub const PAYLOAD_OFFSET: usize = 4;

    /// Returns the 18-byte [`InitCooldownPayload`] view of this cooldown entry.
    #[must_use]
    pub const fn payload(&self) -> InitCooldownPayload {
        InitCooldownPayload {
            key: self.key,
            value: self.value,
            config_value: self.config_value,
            flags: self.flags,
        }
    }

    /// Serializes only the 18-byte preset payload `(key, value, config_value, flags)`.
    #[must_use]
    pub fn encode_payload(&self) -> [u8; InitCooldownPayload::LEN] {
        self.payload().to_bytes()
    }

    /// Reconstructs an [`InitCooldownBody`] from an owner wire ID and an 18-byte preset payload.
    #[must_use]
    pub fn from_payload(owner: u32, bytes: [u8; InitCooldownPayload::LEN]) -> Self {
        let payload = InitCooldownPayload::from_bytes(&bytes);
        Self {
            owner,
            key: payload.key,
            value: payload.value,
            config_value: payload.config_value,
            flags: payload.flags,
        }
    }
}

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

/// Known `0x03f1 EndMatch` completion reason codes, preserving unknown bytes losslessly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, BinRead, BinWrite)]
#[brw(big)]
pub enum EndMatchReason {
    /// Vain crystal shattered (`0`).
    #[brw(magic = 0u8)]
    CrystalShattered,
    /// Match ended by team surrender (`2`).
    #[brw(magic = 2u8)]
    Surrender,
    /// Match ended by score or time limit (`3`).
    #[brw(magic = 3u8)]
    ScoreOrTimeLimit,
    /// Match ended during draft/early termination (`8`).
    #[brw(magic = 8u8)]
    DraftSync,
    /// Uncatalogued match end reason byte.
    Unknown(u8),
}

impl EndMatchReason {
    /// Converts a raw wire byte into [`EndMatchReason`].
    #[must_use]
    pub const fn from_wire(raw: u8) -> Self {
        match raw {
            0 => Self::CrystalShattered,
            2 => Self::Surrender,
            3 => Self::ScoreOrTimeLimit,
            8 => Self::DraftSync,
            other => Self::Unknown(other),
        }
    }

    /// Converts [`EndMatchReason`] into its wire byte.
    #[must_use]
    pub const fn to_wire(self) -> u8 {
        match self {
            Self::CrystalShattered => 0,
            Self::Surrender => 2,
            Self::ScoreOrTimeLimit => 3,
            Self::DraftSync => 8,
            Self::Unknown(other) => other,
        }
    }
}

/// `0x03f1 EndMatch`: 5-byte match result frame (often padded to 6 bytes on the wire).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct EndMatchBody {
    /// Winning wire team word (`1` = Left, `2` = Right).
    pub team_word: u32,
    /// Match termination reason code.
    pub reason: EndMatchReason,
}

fixed_packet!(EndMatchBody, Opcode::EndMatch, 5);

/// `0x0429 ModifyAbilityCharges`: 13-byte ability charge update frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct ModifyAbilityChargesBody {
    /// Owning wire actor identifier.
    pub owner: u32,
    /// Target ability slot or key identifier.
    pub ability_slot: u32,
    /// Charge count or delta operand.
    pub operand: u32,
    /// Update mode byte.
    pub mode: u8,
}

fixed_packet!(ModifyAbilityChargesBody, Opcode::ModifyAbilityCharges, 13);

/// `0x0447 DecrementBuffStack`: 12-byte buff stack decrement frame (often padded to 14 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct DecrementBuffStackBody {
    /// Target wire actor identifier.
    pub target: u32,
    /// Active buff instance identifier.
    pub instance: u32,
    /// Number of stacks to remove.
    pub count: u32,
}

fixed_packet!(DecrementBuffStackBody, Opcode::DecrementBuffStack, 12);

/// `0x041a OverrideAbility`: 13-byte ability slot override frame (often padded to 14 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct OverrideAbilityBody {
    /// Owning wire actor identifier.
    pub actor: u32,
    /// Ability key/slot identifier being overridden.
    pub key: u32,
    /// Replacement ability key/identifier.
    pub replacement: u32,
    /// Override flag byte.
    pub flag: u8,
}

fixed_packet!(OverrideAbilityBody, Opcode::OverrideAbility, 13);

/// `0x041b ClearAbilityOverride`: 8-byte ability slot override clear frame (often padded to 14B).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct ClearAbilityOverrideBody {
    /// Owning wire actor identifier.
    pub actor: u32,
    /// Ability key/slot identifier to clear.
    pub key: u32,
}

fixed_packet!(ClearAbilityOverrideBody, Opcode::ClearAbilityOverride, 8);

/// `0x043c CancelAbility`: 10-byte server ability/item cast cancellation frame (up to 14B padded).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct CancelAbilityBody {
    /// Casting wire actor identifier.
    pub actor: u32,
    /// Selector or item instance reference being cancelled.
    pub reference: u32,
    /// Non-zero when cancelling an item activation rather than an ability slot.
    pub is_item: u8,
    /// Non-zero when shared channel state should also be cancelled.
    pub cancel_shared: u8,
}

fixed_packet!(CancelAbilityBody, Opcode::CancelAbility, 10);

/// `0x0414 RequestCancelAbility`: 2-byte client cast-cancel request (padded to 6 bytes on wire).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct RequestCancelAbilityBody {
    /// Ability slot selector byte.
    pub ability_slot: u8,
    /// Client cancel mode flag byte.
    pub cancel_flag: u8,
}

fixed_packet!(RequestCancelAbilityBody, Opcode::RequestCancelAbility, 2);

/// `0x0436 RequestUpgradeAbility`: 1-byte client ability upgrade request (padded to 6 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct RequestUpgradeAbilityBody {
    /// Ability slot selector byte.
    pub ability_slot: u8,
}

fixed_packet!(RequestUpgradeAbilityBody, Opcode::RequestUpgradeAbility, 1);

/// `0x0412 RequestPointAbility`: 13-byte client point-targeted cast request (padded to 14 bytes).
#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct RequestPointAbilityBody {
    /// World coordinates `[x, y, z]`.
    pub coordinates: [f32; 3],
    /// Ability selector byte.
    pub selector: u8,
}

fixed_packet!(RequestPointAbilityBody, Opcode::RequestPointAbility, 13);

/// `0x0411 RequestTargetAbility`: 5-byte client unit-targeted cast request (padded to 6 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct RequestTargetAbilityBody {
    /// Target wire actor identifier.
    pub target: u32,
    /// Ability selector byte.
    pub selector: u8,
}

fixed_packet!(RequestTargetAbilityBody, Opcode::RequestTargetAbility, 5);

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

/// `0x0472 SetSurrenderStateRequest`: 1-byte client surrender vote packet (often 6 bytes padded).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct SurrenderRequestBody {
    /// Wire vote byte (`1` = yes, `2` = no).
    pub vote: u8,
}

fixed_packet!(SurrenderRequestBody, Opcode::SetSurrenderStateRequest, 1);

impl SurrenderRequestBody {
    /// Returns the typed [`SurrenderVote`] corresponding to [`Self::vote`].
    #[must_use]
    pub const fn typed_vote(self) -> SurrenderVote {
        SurrenderVote::from_wire(self.vote)
    }
}

/// `0x0452 CrystalShattered`: 0-byte vain-crystal destruction marker frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct CrystalShatteredBody;

fixed_packet!(CrystalShatteredBody, Opcode::CrystalShattered, 0);

/// `0x03ef StartMatch`: 0-byte match start marker frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct StartMatchBody;

fixed_packet!(StartMatchBody, Opcode::StartMatch, 0);

/// `0x048f RejectBuyItem`: 0-byte rejected item purchase notification.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct RejectBuyItemBody;

fixed_packet!(RejectBuyItemBody, Opcode::RejectBuyItem, 0);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_to_roundtrip_and_float_bits() {
        let pkt = MoveToBody {
            selector: 3,
            x: -0.0,
            y: f32::from_bits(0x7fc0_1234),
        };
        let msg = pkt.encode();
        assert_eq!(msg.known_opcode(), Some(Opcode::MoveTo));
        assert_eq!(msg.body.len(), MoveToBody::BODY_LEN);
        let mut expected = vec![3];
        expected.extend_from_slice(&(-0.0f32).to_be_bytes());
        expected.extend_from_slice(&0x7fc0_1234u32.to_be_bytes());
        assert_eq!(msg.body, expected);

        let decoded = MoveToBody::read_exact(&msg.body).unwrap();
        assert_eq!(decoded.selector, 3);
        assert_eq!(decoded.x.to_bits(), (-0.0f32).to_bits());
        assert_eq!(decoded.y.to_bits(), 0x7fc0_1234);
        assert!(MoveToBody::read_exact(&msg.body[..8]).is_err());
    }

    #[test]
    fn move_point_and_teleport_roundtrip() {
        let move_req = MovePointRequestBody { x: 12.5, y: -4.25 };
        assert_eq!(
            MovePointRequestBody::read_exact(&move_req.encode().body).ok(),
            Some(move_req)
        );

        let teleport = TeleportToBody {
            actor: 1502,
            position: [1.0, 2.0, -3.5],
            flag: 1,
        };
        let msg = teleport.encode();
        assert_eq!(msg.body.len(), 17);
        assert_eq!(TeleportToBody::read_exact(&msg.body).ok(), Some(teleport));
    }

    #[test]
    fn lifecycle_and_world_clock_golden_layouts() {
        let dead = ActorDeadBody { actor: 1504 };
        let padded_dead = dead.encode_padded();
        assert_eq!(padded_dead.body, [0, 0, 5, 224, 0, 0]);
        assert_eq!(
            ActorDeadBody::read_padded_zero(&padded_dead.body).ok(),
            Some(dead)
        );

        let destroy = DestroyEntityBody { actor: 2001 };
        let padded_destroy = destroy.encode_padded();
        assert_eq!(
            DestroyEntityBody::read_with_padding(&padded_destroy.body).ok(),
            Some((destroy, &[0u8, 0][..]))
        );

        let undead = ActorUndeadBody { actor: 1501 };
        assert_eq!(undead.encode().body, 1501u32.to_be_bytes());

        let respawn = ActorRespawnBody {
            actor: 1500,
            position: [10.0, 0.0, -5.0],
        };
        let respawn_padded = respawn.encode_padded();
        assert_eq!(respawn_padded.body.len(), 22);
        assert_eq!(
            ActorRespawnBody::read_padded_zero(&respawn_padded.body).ok(),
            Some(respawn)
        );

        let timer = RespawnTimerBody {
            actor: 1500,
            seconds: 12.5,
        };
        assert_eq!(
            RespawnTimerBody::read_exact(&timer.encode().body).ok(),
            Some(timer)
        );

        let level = LevelUpBody {
            actor: 1500,
            operand: 4,
        };
        let level_padded = level.encode_padded();
        assert_eq!(level_padded.body.len(), 14);
        assert_eq!(
            LevelUpBody::read_padded_zero(&level_padded.body).ok(),
            Some(level)
        );

        let clock = WorldClockBody { seconds: 42.25 };
        assert_eq!(clock.encode().body, 42.25f32.to_be_bytes());
        assert_eq!(
            WorldClockBody::read_exact(&clock.encode().body).ok(),
            Some(clock)
        );

        let sync = ClockSyncBody { seconds: 7.5 };
        assert_eq!(
            ClockSyncBody::read_exact(&sync.encode().body).ok(),
            Some(sync)
        );

        let mode_var = ModifyGameModeVarBody {
            key: 0x1234_5678,
            value: 5,
        };
        let mode_msg = mode_var.encode_padded();
        assert_eq!(mode_msg.body.len(), 14);
        assert_eq!(
            ModifyGameModeVarBody::read_padded_zero(&mode_msg.body).ok(),
            Some(mode_var)
        );
    }

    #[test]
    fn items_cooldowns_and_charges_roundtrip() {
        let grant = ItemGrantBody {
            actor: 1500,
            item: 201,
            instance: 9001,
        };
        assert_eq!(
            ItemGrantBody::read_exact(&grant.encode().body).ok(),
            Some(grant)
        );

        let consume = ConsumeItemBody {
            actor: 1500,
            instance: 9001,
            flags: 1,
        };
        assert_eq!(
            ConsumeItemBody::read_exact(&consume.encode().body).ok(),
            Some(consume)
        );

        let buy = BuyItemBody {
            actor: 1500,
            item: 201,
        };
        assert_eq!(BuyItemBody::read_exact(&buy.encode().body).ok(), Some(buy));

        let cd = InitCooldownBody {
            owner: 1500,
            key: 77,
            value: 0.0,
            config_value: 14.0,
            flags: [1, 0, 2, 0, 3, 4],
        };
        let cd_msg = cd.encode();
        assert_eq!(cd_msg.body.len(), 22);
        assert_eq!(InitCooldownBody::read_exact(&cd_msg.body).ok(), Some(cd));
        let payload_bytes = cd.encode_payload();
        assert_eq!(
            &payload_bytes[..],
            &cd_msg.body[InitCooldownBody::PAYLOAD_OFFSET..]
        );
        assert_eq!(
            payload_bytes,
            [
                0, 0, 0, 77, // key = 77
                0, 0, 0, 0, // value = 0.0
                0x41, 0x60, 0, 0, // config_value = 14.0
                1, 0, 2, 0, 3, 4, // flags
            ]
        );
        assert_eq!(
            InitCooldownPayload::from_bytes(&payload_bytes),
            cd.payload()
        );
        assert_eq!(InitCooldownBody::from_payload(1500, payload_bytes), cd);

        let charges = ModifyAbilityChargesBody {
            owner: 1500,
            ability_slot: 2,
            operand: 1,
            mode: 3,
        };
        assert_eq!(
            ModifyAbilityChargesBody::read_exact(&charges.encode().body).ok(),
            Some(charges)
        );

        let dec = DecrementBuffStackBody {
            target: 2000,
            instance: 44,
            count: 1,
        };
        let dec_padded = dec.encode_padded();
        assert_eq!(dec_padded.body.len(), 14);
        assert_eq!(
            DecrementBuffStackBody::read_padded_zero(&dec_padded.body).ok(),
            Some(dec)
        );
        assert!(DecrementBuffStackBody::read_padded_zero(&dec.encode().body).is_err());
        assert_eq!(
            DecrementBuffStackBody::read_exact_or_padded(&dec.encode().body).ok(),
            Some(dec)
        );
    }

    #[test]
    fn announcements_end_match_and_target_roundtrips() {
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

        let end = EndMatchBody {
            team_word: 2,
            reason: EndMatchReason::ScoreOrTimeLimit,
        };
        assert_eq!(end.encode_padded().body, [0, 0, 0, 2, 3, 0]);
        assert_eq!(
            EndMatchBody::read_padded_zero(&end.encode_padded().body).ok(),
            Some(end)
        );

        let unknown_end = EndMatchBody {
            team_word: 1,
            reason: EndMatchReason::Unknown(99),
        };
        assert_eq!(
            EndMatchBody::read_padded_zero(&unknown_end.encode_padded().body).ok(),
            Some(unknown_end)
        );
    }

    #[test]
    fn ability_overrides_requests_kick_and_markers_roundtrip() {
        let ov = OverrideAbilityBody {
            actor: 1500,
            key: 8,
            replacement: 3,
            flag: 0,
        };
        let ov_msg = ov.encode_padded();
        assert_eq!(ov_msg.body.len(), 14);
        assert_eq!(
            OverrideAbilityBody::read_padded_zero(&ov_msg.body).ok(),
            Some(ov)
        );

        let clr = ClearAbilityOverrideBody {
            actor: 1500,
            key: 3,
        };
        let clr_msg = clr.encode_padded();
        assert_eq!(clr_msg.body.len(), 14);
        assert_eq!(
            ClearAbilityOverrideBody::read_padded_zero(&clr_msg.body).ok(),
            Some(clr)
        );

        let cancel = CancelAbilityBody {
            actor: 1500,
            reference: 2,
            is_item: 0,
            cancel_shared: 0,
        };
        assert_eq!(cancel.encode().body.len(), 10);
        assert_eq!(
            CancelAbilityBody::read_exact(&cancel.encode().body).ok(),
            Some(cancel)
        );
        assert_eq!(
            CancelAbilityBody::read_padded_zero(&cancel.encode_padded().body).ok(),
            Some(cancel)
        );

        let req_cancel = RequestCancelAbilityBody {
            ability_slot: 1,
            cancel_flag: 0,
        };
        assert_eq!(req_cancel.encode_padded().body, [1, 0, 0, 0, 0, 0]);
        assert_eq!(
            RequestCancelAbilityBody::read_padded_zero(&req_cancel.encode_padded().body).ok(),
            Some(req_cancel)
        );

        let req_up = RequestUpgradeAbilityBody { ability_slot: 2 };
        assert_eq!(req_up.encode_padded().body, [2, 0, 0, 0, 0, 0]);
        assert_eq!(
            RequestUpgradeAbilityBody::read_padded_zero(&req_up.encode_padded().body).ok(),
            Some(req_up)
        );

        let req_pt = RequestPointAbilityBody {
            coordinates: [1.5, 0.0, -2.5],
            selector: 1,
        };
        let req_pt_msg = req_pt.encode_padded();
        assert_eq!(req_pt_msg.body.len(), 14);
        assert_eq!(
            RequestPointAbilityBody::read_padded_zero(&req_pt_msg.body).ok(),
            Some(req_pt)
        );

        let req_tgt = RequestTargetAbilityBody {
            target: 1501,
            selector: 2,
        };
        assert_eq!(req_tgt.encode_padded().body, [0, 0, 5, 221, 2, 0]);
        assert_eq!(
            RequestTargetAbilityBody::read_padded_zero(&req_tgt.encode_padded().body).ok(),
            Some(req_tgt)
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

        let surr_req = SurrenderRequestBody { vote: 1 };
        assert_eq!(surr_req.typed_vote(), SurrenderVote::Yes);
        assert_eq!(
            SurrenderRequestBody::read_padded_zero(&surr_req.encode_padded().body).ok(),
            Some(surr_req)
        );

        assert!(CrystalShatteredBody.encode().body.is_empty());
        assert!(StartMatchBody.encode().body.is_empty());
        assert!(RejectBuyItemBody.encode().body.is_empty());
        assert_eq!(
            CrystalShatteredBody::read_exact(&[]).ok(),
            Some(CrystalShatteredBody)
        );
    }
}
