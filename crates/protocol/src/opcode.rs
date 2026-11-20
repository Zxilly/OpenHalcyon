macro_rules! opcodes {
    ($( $name:ident = $raw:literal, )*) => {
        /// CE opcodes. Some identifiers retain early capture labels for API compatibility.
        /// In particular, `PosUpdate` clears two actor flags. Use `Message::actor_flags`
        /// to decode the receiver operations for 0x03EA through 0x03EC.
        #[derive(Debug, Clone, Copy, Eq, PartialEq)]
        #[repr(u16)]
        pub enum Opcode { $( $name = $raw, )* }

        impl Opcode {
            pub const fn raw(self) -> u16 { self as u16 }
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

opcodes! {
    ClockSync = 0x0000,
    Hello = 0x0005,
    Heartbeat = 0x0006,
    PlayerUuid = 0x03e8,
    GameSetup = 0x03e9,
    EntityActA = 0x03ea,
    EntityActB = 0x03eb,
    PosUpdate = 0x03ec,
    PlayerHandle = 0x03ed,
    PlayerInfo = 0x03ee,
    TickSyncA = 0x03ef,
    TickSyncB = 0x03f0,
    TickSyncC = 0x03f1,
    Snapshot = 0x03f2,
    EntitySpawn = 0x03f3,
    MoveCmd = 0x03f4,
    StateUpdA = 0x03f5,
    StateUpdB = 0x03f6,
    StateUpdC = 0x03f7,
    EntityFloat = 0x03f8,
    EntityAct2 = 0x03f9,
    MoveAndFace = 0x03fa,
    StopActor = 0x03fb,
    ResetMovement = 0x03fc,
    MoveToPredictive = 0x03fd,
    MoveToAuthoritative = 0x03fe,
    StopAuthoritative = 0x03ff,
    StopNavigating = 0x0400,
    FaceDir = 0x0401,
    AutoActorBounce = 0x0402,
    AutoMoveTo = 0x0403,
    AutoMoveToActorResponse = 0x0404,
    AutoMoveToLocation = 0x0405,
    AutoOrbit = 0x0406,
    CancelAutoOrbit = 0x0407,
    AttachToActor = 0x0408,
    TeleportTo = 0x0409,
    Teleport = 0x040a,
    DestroyEntity = 0x040b,
    Unresolved040C = 0x040c,
    FireProjectile = 0x040d,
    FirePointProjectile = 0x040e,
    FireDirectedProjectile = 0x040f,
    DetonateProjectile = 0x0410,
    RequestTargetAbility = 0x0411,
    RequestPointAbility = 0x0412,
    RequestAbilityVariant = 0x0413,
    RequestCancelAbility = 0x0414,
    PlayAbility = 0x0415,
    PlayPointAbility = 0x0416,
    PlayAbilityVariant = 0x0417,
    PlayVoiceOver = 0x0418,
    PlayPfxAtLocation = 0x0419,
    OverrideAbility = 0x041a,
    ClearAbilityOverride = 0x041b,
    StatModifier = 0x041c,
    EntityStat = 0x041d,
    HealthImpact = 0x041e,
    ModifyGameModeVar = 0x041f,
    CreateZoneOfControl = 0x0420,
    CreateZoneOfControlVariantB = 0x0421,
    CreateZoneOfControlVariantC = 0x0422,
    DestroyZoneOfControl = 0x0423,
    RequestModifyBasicAttackTarget = 0x0424,
    ModifyBasicAttackTarget = 0x0425,
    Unresolved0426 = 0x0426,
    PauseCooldown = 0x0427,
    CooldownValue = 0x0428,
    ModifyAbilityCharges = 0x0429,
    Unresolved042A = 0x042a,
    EntityState = 0x042b,
    CampVisibility = 0x042c,
    Unresolved042D = 0x042d,
    Position = 0x042e,
    ActorUndead = 0x042f,
    ActorDeath = 0x0430,
    ActorDead = 0x0431,
    ActorRespawn = 0x0432,
    ActorRespawnTimer = 0x0433,
    LevelUp = 0x0434,
    MakeAnnouncement = 0x0435,
    RequestUpgradeAbility = 0x0436,
    QuickBuyItem = 0x0437,
    ReorderItem = 0x0438,
    ShopRequest = 0x0439,
    GrantAbility = 0x043a,
    DowngradeAbility = 0x043b,
    CancelAbility = 0x043c,
    ItemGrant = 0x043d,
    EntityProp = 0x043e,
    EntityData = 0x043f,
    ShowFlyoutText = 0x0440,
    ModifyBuffDuration = 0x0441,
    ModifyBuffMaxStacks = 0x0442,
    ModifyBuffStack = 0x0443,
    ModifyItemStack = 0x0444,
    CancelBuff = 0x0445,
    TimeoutBuff = 0x0446,
    DecrementBuffStack = 0x0447,
    RequestActivateItem = 0x0448,
    RequestActivateItemVariantB = 0x0449,
    RequestActivateItemVariantC = 0x044a,
    RemoveItem = 0x044b,
    ItemActivated = 0x044c,
    SellItem = 0x044d,
    Ping = 0x044e,
    HUDQuickMessage = 0x044f,
    Unresolved0450 = 0x0450,
    OpeningWorldClock = 0x0451,
    EnterPostgame = 0x0452,
    HeroCatalog = 0x0453,
    GameMode = 0x0454,
    HeroLock = 0x0455,
    Unresolved0456 = 0x0456,
    Unresolved0457 = 0x0457,
    SetupA = 0x0458,
    Snapshot5V5 = 0x0459,
    SnapshotAral = 0x045a,
    Unresolved045B = 0x045b,
    SceneReadiness = 0x045c,
    Unresolved045D = 0x045d,
    HeroSelection = 0x045e,
    SkinSelection = 0x045f,
    Unresolved0460 = 0x0460,
    Unresolved0461 = 0x0461,
    Unresolved0462 = 0x0462,
    LockSelection = 0x0463,
    Unresolved0464 = 0x0464,
    Unresolved0465 = 0x0465,
    Unresolved0466 = 0x0466,
    Unresolved0467 = 0x0467,
    Unresolved0468 = 0x0468,
    Unresolved0469 = 0x0469,
    Unresolved046A = 0x046a,
    SetupC = 0x046b,
    BeginLoading = 0x046c,
    SceneConfirmed = 0x046d,
    RequestScene = 0x046e,
    ModeName = 0x046f,
    SceneTransition = 0x0470,
    SceneOwner = 0x0471,
    SetSurrenderStateRequest = 0x0472,
    TeamSurrenderStateChanged = 0x0473,
    SpectatorExitMatchRequest = 0x0474,
    PauseTutorial = 0x0475,
    SetSimulationSpeed = 0x0476,
    SetPlayerAsSpectator = 0x0477,
    SetMinimapSpawnPhase = 0x0478,
    Unresolved0479 = 0x0479,
    Unresolved047A = 0x047a,
    Unresolved047B = 0x047b,
    Unresolved047C = 0x047c,
    TutorialStateChangedResponse = 0x047d,
    StateChange = 0x047e,
    Unresolved047F = 0x047f,
    AralDraftMarker = 0x0480,
    EquipTalent = 0x0481,
    UnequipTalent = 0x0482,
    InvalidateTalent = 0x0483,
    RequestEquipTalent = 0x0484,
    RequestRecommendedBuildPath = 0x0485,
    RequestLevelLogicEvent = 0x0486,
    SetBuildPath = 0x0487,
    SetRecommendedBuildPath = 0x0488,
    SyncAbilityBehavior = 0x0489,
    CooldownSync = 0x048a,
    ModifyBuffVar = 0x048b,
    AggroState = 0x048c,
    StatisticsReport = 0x048d,
    Unresolved048E = 0x048e,
    RejectBuyItem = 0x048f,
    BroadcastSwapTarget = 0x0490,
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
