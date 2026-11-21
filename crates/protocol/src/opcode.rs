use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::{fmt, str::FromStr};

macro_rules! opcodes {
    ($( $name:ident = $raw:literal => $wire_name:literal, )*) => {
        /// CE opcodes. Some identifiers retain early capture labels for API compatibility.
        /// In particular, `PlayerReconnected` clears two actor flags. Use `Message::actor_flags`
        /// to decode the receiver operations for 0x03EA through 0x03EC.
        #[derive(Debug, Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
        #[repr(u16)]
        pub enum Opcode { $( $name = $raw, )* }

        impl Opcode {
            pub const fn raw(self) -> u16 { self as u16 }
            pub const fn as_str(self) -> &'static str { match self { $( Self::$name => $wire_name, )* } }
            pub fn from_name(name: &str) -> Option<Self> {
                match name { $( $wire_name => Some(Self::$name), )* _ => None }
            }
            pub const ALL: &'static [Self] = &[$(Self::$name,)*];
        }
        /// Raw constants for pattern matching on legacy APIs carrying unrecognized opcodes.
        #[allow(non_upper_case_globals)]
        pub mod wire_opcode {
            $(pub const $name: u16 = super::Opcode::$name.raw();)*
        }

        impl TryFrom<u16> for Opcode {
            type Error = u16;
            fn try_from(raw: u16) -> Result<Self, Self::Error> {
                match raw { $( $raw => Ok(Self::$name), )* other => Err(other) }
            }
        }
    };
}

impl fmt::Display for Opcode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for Opcode {
    type Err = crate::UnknownOpcodeName;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        Self::from_name(name).ok_or(UnknownOpcodeName)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnknownOpcodeName;

impl fmt::Display for UnknownOpcodeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("unknown CE opcode name")
    }
}

impl std::error::Error for UnknownOpcodeName {}

impl From<Opcode> for u16 {
    fn from(opcode: Opcode) -> Self {
        opcode.raw()
    }
}

impl Serialize for Opcode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            serializer.serialize_str(self.as_str())
        } else {
            serializer.serialize_u16(self.raw())
        }
    }
}

impl<'de> Deserialize<'de> for Opcode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl de::Visitor<'_> for Visitor {
            type Value = Opcode;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a registered CE opcode name or unsigned 16-bit opcode")
            }
            fn visit_str<E: de::Error>(self, name: &str) -> Result<Opcode, E> {
                Opcode::from_name(name)
                    .ok_or_else(|| E::invalid_value(de::Unexpected::Str(name), &self))
            }
            fn visit_u64<E: de::Error>(self, raw: u64) -> Result<Opcode, E> {
                u16::try_from(raw)
                    .ok()
                    .and_then(|n| Opcode::try_from(n).ok())
                    .ok_or_else(|| E::invalid_value(de::Unexpected::Unsigned(raw), &self))
            }
        }
        if deserializer.is_human_readable() {
            deserializer.deserialize_any(Visitor)
        } else {
            deserializer.deserialize_u16(Visitor)
        }
    }
}

opcodes! {
    ClockSync = 0x0000 => "ClockSync",
    TransportAck = 0x0001 => "TransportKeepAlive_or_Ack",
    ClientHello = 0x0005 => "ClientHello_Plaintext",
    ServerHelloAck = 0x0006 => "ServerHelloAck_Plaintext",
    RegisterPlayerUuid = 0x03e8 => "ActionRegisterPlayer_UUID",
    MatchSettings = 0x03e9 => "ActionMatchSettings",
    PlayerAfk = 0x03ea => "ActionPlayerAFK",
    PlayerDeserter = 0x03eb => "ActionPlayerDeserter",
    PlayerReconnected = 0x03ec => "ActionPlayerReconnected",
    PlayerLoadedScene = 0x03ed => "ActionPlayerLoadedScene",
    PlayerInfo = 0x03ee => "ActionPlayerInfo",
    StartMatch = 0x03ef => "ActionStartMatch",
    ShowMatchPrepSequence = 0x03f0 => "ActionShowMatchPrepSequence",
    EndMatch = 0x03f1 => "ActionEndMatch",
    EntitySpawn = 0x03f2 => "ActionEntitySpawn",
    HeroSpawn = 0x03f3 => "ActionHeroSpawn",
    RequestMoveJoystick = 0x03f4 => "ActionRequestMoveJoystick_Client",
    RequestMovePoint = 0x03f5 => "ActionRequestMovePoint_Client",
    Unseen03F6 = 0x03f6 => "Action_Unseen_03F6",
    Unseen03F7 = 0x03f7 => "Action_Unseen_03F7",
    MoveTo = 0x03f8 => "ActionMoveTo",
    MoveToServerAuthoritative = 0x03f9 => "ActionMoveToServerAuthoritative",
    MoveAndFace = 0x03fa => "ActionMoveAndFaceTarget",
    StopActor = 0x03fb => "ActionStopActor",
    ResetMovement = 0x03fc => "ActionResetMovement_Client",
    MoveToPredictive = 0x03fd => "ActionMoveToPredictive_Client",
    MoveToAuthoritative = 0x03fe => "ActionMoveToAuthoritative_Client",
    StopAuthoritative = 0x03ff => "ActionStopAuthoritative_Client",
    StopNavigating = 0x0400 => "ActionStopNavigating_Client",
    FaceDir = 0x0401 => "ActionFaceDir",
    AutoActorBounce = 0x0402 => "ActionAutoActorBounce",
    AutoMoveTo = 0x0403 => "ActionAutoMoveTo",
    AutoMoveToActorResponse = 0x0404 => "ActionAutoMoveToActorResponse",
    AutoMoveToLocation = 0x0405 => "ActionAutoMoveToLocation_Client",
    AutoOrbit = 0x0406 => "ActionAutoOrbit",
    CancelAutoOrbit = 0x0407 => "ActionCancelAutoOrbit",
    AttachToActor = 0x0408 => "ActionAttachToActor_Client",
    TeleportTo = 0x0409 => "ActionTeleportTo",
    Teleport = 0x040a => "ActionTeleport_Client",
    DestroyEntity = 0x040b => "ActionEntityDestroy",
    ReservedNoop040C = 0x040c => "Reserved_Noop_040C",
    FireProjectile = 0x040d => "ActionFireProjectile_Homing",
    FirePointProjectile = 0x040e => "ActionFireProjectile_Point",
    FireDirectedProjectile = 0x040f => "ActionFireProjectile_FullVector",
    DetonateProjectile = 0x0410 => "ActionDetonateProjectile",
    RequestTargetAbility = 0x0411 => "ActionRequestActivateAbility_Target",
    RequestPointAbility = 0x0412 => "ActionRequestActivateAbility_Point",
    RequestActivateAbilityUnseen = 0x0413 => "ActionRequestActivateAbility_Unseen",
    RequestCancelAbility = 0x0414 => "ActionRequestCancelAbility_Client",
    PlayTargetAbility = 0x0415 => "ActionPlayAbility_Target",
    PlayPointAbility = 0x0416 => "ActionPlayAbility_Point",
    PlayAbility = 0x0417 => "ActionPlayAbility",
    PlayVoiceOver = 0x0418 => "ActionPlayVoiceOver",
    PlayPfxAtLocation = 0x0419 => "ActionPlayEffect_ByHash",
    OverrideAbility = 0x041a => "ActionOverrideAbility",
    ClearAbilityOverride = 0x041b => "ActionClearAbilityOverride",
    ModifyActorAttribute = 0x041c => "ActionModifyActorAttribute",
    ModifyActorResource = 0x041d => "ActionModifyActorResource",
    HealthImpact = 0x041e => "ActionImpactHealth",
    ModifyGameModeVar = 0x041f => "ActionModifyGameModeVar",
    CreateZoneOfControlUnseen = 0x0420 => "Action_Unseen_0420",
    CreateZoneOfControl = 0x0421 => "ActionCreateZoneOfControl",
    CreateRectZoneOfControl = 0x0422 => "ActionCreateZoneOfControl_Rect",
    DestroyZoneOfControl = 0x0423 => "ActionDestroyZoneOfControl",
    RequestModifyBasicAttackTarget = 0x0424 => "ActionRequestModifyBasicAttackTarget",
    ModifyBasicAttackTarget = 0x0425 => "ActionModifyBasicAttackTarget",
    ReservedNoop0426 = 0x0426 => "Reserved_Noop_0426",
    PauseCooldown = 0x0427 => "ActionPauseCooldown",
    ModifyCooldown = 0x0428 => "ActionModifyCooldown",
    ModifyAbilityCharges = 0x0429 => "ActionModifyAbilityCharges",
    ReservedNoop042A = 0x042a => "Reserved_Noop_042A",
    ModifyVisibility = 0x042b => "ActionModifyVisibility",
    CampVisibility = 0x042c => "ActionCampVisibility",
    ReservedNoop042D = 0x042d => "Reserved_Noop_042D",
    MoveToCorrection = 0x042e => "ActionMoveToCorrection",
    ActorUndead = 0x042f => "ActionActorUndead",
    ActorDeath = 0x0430 => "ActionActorDie",
    ActorDead = 0x0431 => "ActionActorDead",
    ActorRespawn = 0x0432 => "ActionActorRespawn",
    ActorRespawnTimer = 0x0433 => "ActionStartRespawnTimer",
    LevelUp = 0x0434 => "ActionLevelUp",
    MakeAnnouncement = 0x0435 => "ActionAnnouncement",
    RequestUpgradeAbility = 0x0436 => "ActionRequestUpgradeAbility_Client",
    QuickBuyItem = 0x0437 => "ActionQuickBuyItem",
    ReorderItem = 0x0438 => "ActionReorderItem",
    BuyItem = 0x0439 => "ActionBuyItem",
    GrantAbility = 0x043a => "ActionGrantAbility",
    DowngradeAbility = 0x043b => "ActionDowngradeAbility",
    CancelAbility = 0x043c => "ActionCancelAbility",
    ItemGrant = 0x043d => "ActionGrantItem",
    ApplyBuffCompact = 0x043e => "ActionApplyBuff_Compact",
    ApplyBuffExtended = 0x043f => "ActionApplyBuff_Extended",
    ShowFlyoutText = 0x0440 => "ActionShowFlyoutText",
    ModifyBuffDuration = 0x0441 => "ActionModifyBuffDuration",
    ModifyBuffMaxStacks = 0x0442 => "ActionModifyBuffMaxStacks",
    ModifyBuffStack = 0x0443 => "ActionModifyBuffStack",
    ModifyItemStack = 0x0444 => "ActionModifyItemStack",
    CancelBuff = 0x0445 => "ActionCancelBuff",
    TimeoutBuff = 0x0446 => "ActionTimeoutBuff",
    DecrementBuffStack = 0x0447 => "ActionDecrementBuffStack",
    RequestActivateItemSelf = 0x0448 => "ActionRequestActivateItem_Self",
    RequestActivateItemTarget = 0x0449 => "ActionRequestActivateItem_Target",
    RequestActivateItemPoint = 0x044a => "ActionRequestActivateItem_Point",
    ConsumeItem = 0x044b => "ActionConsumeItem",
    ItemActivated = 0x044c => "ActionItemActivated",
    SellItem = 0x044d => "ActionSellItem",
    Ping = 0x044e => "ActionPing",
    HUDQuickMessage = 0x044f => "ActionHUDQuickMessage",
    Unresolved0450 = 0x0450 => "Action_Unseen_0450",
    SyncWorldClock = 0x0451 => "ActionSyncWorldClock",
    CrystalShattered = 0x0452 => "ActionCrystalShattered",
    DraftCatalogEntry = 0x0453 => "ActionDraftCatalogEntry",
    DraftModeAnnouncement = 0x0454 => "ActionDraftModeAnnouncement",
    DraftReadyAccept = 0x0455 => "ActionDraftReadyAccept",
    DraftRerollHero = 0x0456 => "ActionDraftRerollHero",
    Unresolved0457 = 0x0457 => "Action_Unseen_0457",
    DraftRefreshRequest = 0x0458 => "ActionDraftRefreshRequest",
    DraftRosterFull = 0x0459 => "ActionDraftRosterFull",
    DraftRosterAral = 0x045a => "ActionDraftRosterARAL",
    DraftRosterRanked = 0x045b => "ActionDraftRosterRanked",
    SceneReadinessRows = 0x045c => "ActionSceneReadinessRows",
    DraftSelectTeamOrSlot = 0x045d => "ActionDraftSelectTeamOrSlot",
    DraftSelectHero = 0x045e => "ActionDraftSelectHero",
    DraftSelectSkin = 0x045f => "ActionDraftSelectSkin",
    DraftSelectRole = 0x0460 => "ActionDraftSelectRole",
    DraftSelectPosition = 0x0461 => "ActionDraftSelectPosition",
    DraftSelectBuildPath = 0x0462 => "ActionDraftSelectBuildPath",
    DraftLockHero = 0x0463 => "ActionDraftLockHero",
    DraftSelectBan = 0x0464 => "ActionDraftSelectBan",
    DraftConfirmBan = 0x0465 => "ActionDraftConfirmBan",
    DraftSkipBan = 0x0466 => "ActionDraftSkipBan",
    DraftSwapRequest = 0x0467 => "ActionDraftSwapRequest",
    DraftSwapResponse = 0x0468 => "ActionDraftSwapResponse",
    DraftSwapCancel = 0x0469 => "ActionDraftSwapCancel",
    DraftSwapDecline = 0x046a => "ActionDraftSwapDecline",
    DraftReadyControl = 0x046b => "ActionDraftReadyControl",
    DraftTransitionToLoading = 0x046c => "ActionDraftTransitionToLoading",
    SceneReadyConfirm = 0x046d => "ActionSceneReadyConfirm",
    RequestSceneSnapshot = 0x046e => "ActionRequestSceneSnapshot",
    SceneSettings = 0x046f => "ActionSceneSettings",
    SceneLoadedDismiss = 0x0470 => "ActionSceneLoadedDismiss",
    ConfirmSceneOwner = 0x0471 => "ActionConfirmSceneOwner",
    SetSurrenderStateRequest = 0x0472 => "ActionRequestSurrenderVote",
    TeamSurrenderStateChanged = 0x0473 => "ActionTeamSurrenderStateChanged",
    SpectatorExitMatchRequest = 0x0474 => "ActionSpectatorExitMatchRequest",
    PauseTutorial = 0x0475 => "ActionPauseTutorial",
    SetSimulationSpeed = 0x0476 => "ActionSetSimulationSpeed",
    SetPlayerAsSpectator = 0x0477 => "ActionSetPlayerAsSpectator",
    SetMinimapSpawnPhase = 0x0478 => "ActionSetMinimapSpawnPhase",
    KickPlayer = 0x0479 => "ActionKickPlayer_ForceDisconnect",
    ReservedNoop047A = 0x047a => "Reserved_Noop_047A",
    Unresolved047B = 0x047b => "Action_Unseen_047B",
    Unresolved047C = 0x047c => "Action_Unseen_047C",
    TutorialStateChangedResponse = 0x047d => "ActionTutorialStateChangedResponse",
    StateChange = 0x047e => "ActionStateChange_Client",
    Unresolved047F = 0x047f => "Action_Unseen_047F",
    AralRerollState = 0x0480 => "ActionAralRerollState",
    EquipTalent = 0x0481 => "ActionEquipTalent",
    UnequipTalent = 0x0482 => "ActionUnequipTalent",
    InvalidateTalent = 0x0483 => "ActionInvalidateTalent",
    RequestEquipTalent = 0x0484 => "ActionRequestEquipTalent",
    RequestRecommendedBuildPath = 0x0485 => "ActionRequestRecommendedBuildPath",
    RequestLevelLogicEvent = 0x0486 => "ActionRequestLevelLogicEvent_Client",
    SetBuildPath = 0x0487 => "ActionSetBuildPath",
    SetRecommendedBuildPath = 0x0488 => "ActionSetRecommendedBuildPath",
    SyncAbilityBehavior = 0x0489 => "ActionSyncAbilityBehavior",
    InitCooldown = 0x048a => "ActionInitCooldown",
    ModifyBuffVar = 0x048b => "ActionModifyBuffVar",
    SendAggroState = 0x048c => "ActionSendAggroState_Client",
    PostMatchStats = 0x048d => "ActionPostMatchStats",
    Unresolved048E = 0x048e => "Action_Unseen_048E",
    RejectBuyItem = 0x048f => "ActionRejectBuyItem_Client",
    BroadcastSwapTarget = 0x0490 => "ActionBroadcastSwapTarget",
}

#[cfg(test)]
mod tests {
    use super::Opcode;
    use crate::{Message, READ_SIZES};

    #[test]
    fn every_reader_has_one_named_opcode_and_unknown_values_round_trip() {
        assert!(
            Opcode::ALL
                .windows(2)
                .all(|pair| pair[0].raw() < pair[1].raw())
        );
        for &(wire, _) in READ_SIZES {
            let opcode = Opcode::try_from(wire).expect("known Android receiver");
            assert_eq!(Message::known(opcode, []).opcode, wire);
        }
        for &opcode in Opcode::ALL {
            assert_eq!(Opcode::try_from(opcode.raw()), Ok(opcode));
        }
        let raw = [0xfa, 0xce, 0xab, 0xcd];
        let message = Message::from_plain(&raw).unwrap();
        assert_eq!(message.known_opcode(), None);
        assert_eq!(message.opcode, u16::from_be_bytes([0xfa, 0xce]));
        assert_eq!(message.body, [0xab, 0xcd]);
    }
}
