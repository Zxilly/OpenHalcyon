//! Vainglory CE game transport. Decrypted messages preserve every byte,
//! including ambiguous padding, so unknown opcodes can be re-encoded exactly.

pub mod ce;
mod codec;
mod opcode;
mod packets;
pub mod player_actions;
mod skin;
mod solo_structure;
pub mod spec;

pub use codec::async_frame;
pub use codec::cipher::{
    HelloEndpoint, MatchCipher, hello, hello_with_port, parse_hello, parse_hello_endpoint,
};
pub use codec::message::{
    ActionWords, ControlEntry, DraftHeroChoice, Fields, LaneMinionSpawn, Message,
};
pub use codec::replay::{ReplayReader, ReplayRecord, ReplayWriter};
pub use codec::wire_text::{self, FixedAscii};
pub use codec::{ACK_FRAME, MAX_PAYLOAD, read_frame};

pub use opcode::{Opcode, UnknownOpcodeName, wire_opcode};
pub use skin::skin_hash;
pub use solo_structure::SoloStructure;

pub use spec::live::LiveFields;
pub use spec::metadata::{AndroidLayout, READ_SIZES};
pub use spec::outbound::{OUTBOUND_LAYOUTS, OutboundAction, OutboundLayout, OutboundOperands};
pub use spec::static_actions::{STATIC_ACTIONS, StaticAction, StaticActionLayout, StaticValue};

pub use packets::combat::{
    ActorAttributeBody, ActorBounceBody, ActorDeadBody, ActorFlagChange, ActorFlags,
    ActorRespawnBody, ActorUndeadBody, AttributeChange, BuffApplication, BuffChange,
    BuffDefinition, BuffExtras, BuffMutation, BuffPrefix, BuffStack, BuyItemBody,
    CampVisibilityBody, CampVisibilityState, CancelAbilityBody, ClearAbilityOverrideBody,
    ConsumeItemBody, CooldownMode, DeathBody, DecrementBuffStackBody, DestroyEntityBody,
    EntityStatBody, EntityStatPrefix, EquippedTalent, ForcedMove, ForcedMoveMode, GrantAbilityBody,
    ImpactBody, InitCooldownBody, InitCooldownPayload, ItemAction, ItemActivationMode,
    ItemGrantBody, LevelUpBody, ModifyAbilityChargesBody, ModifyBasicAttackTargetBody,
    ModifyCooldownBody, MoveAndFaceBody, MoveCmd, MovePointRequestBody, MoveToBody, OrbitAction,
    OrbitPosition, OverrideAbilityBody, PlayPfxAtLocation, PlayPointAbilityBody,
    PlayTargetAbilityBody, PositionBody, ProjectileAction, ProjectileFire, RecommendedBuildPath,
    RejectBuyItemBody, RequestCancelAbilityBody, RequestModifyBasicAttackTargetBody,
    RequestPointAbilityBody, RequestRecommendedBuildPathBody, RequestTargetAbilityBody,
    RequestUpgradeAbilityBody, ResourceChange, RespawnTimerBody, SendAggroStateBody,
    ServerMoveBody, SetRecommendedBuildPathBody, StatModifierPrefix, StopActorBody,
    TalentSelection, TeleportToBody, TowerAggro, VisibilityBody, ZoneAction,
};
pub use packets::lobby::draft_5v5 as draft;
pub use packets::lobby::draft_5v5::{
    DraftAction, DraftBuildPath, DraftPosition, RankedDraftSeat, RankedDraftStatePacket,
    RankedWirePhase,
};
pub use packets::lobby::{
    self, AralRerollStateBody, AralRoster, ConfirmSceneOwnerBody, DraftCatalogEntryBody,
    DraftModeAnnouncementBody, DraftPreferenceFlags, DraftRoster, DraftRosterBody, DraftSlot,
    DraftSlotBody, MatchSettingsBody, MatchSettingsPrefix, PlayerInfoBody, PlayerInfoPrefix,
    PlayerInfoSummaryPrefix, PlayerLoadedSceneBody, RegisterPlayerUuidBody, SceneReadinessRow,
    SceneReadinessRowsBody, SceneSettingsBody,
};
pub use packets::match_ui::{
    AnnouncementBody, AnnouncementKey, AralSurrenderBoard, ClockSyncBody, CrystalShatteredBody,
    EndMatchBody, EndMatchReason, Flyout, FlyoutText, HudQuickMessage, KickPlayerBody,
    MAX_SURRENDER_SLOTS, MapPing, MatchStatistics, MatchStatisticsMut, ModifyGameModeVarBody,
    PlayerStatistics, StartMatchBody, StatisticsReport, StatisticsWireBody, SurrenderPhase,
    SurrenderRequestBody, SurrenderVote, TeamStatistics, TeamSurrenderBoard, WorldClockBody,
};
pub use packets::spawn::{
    EntitySpawnBody, EntityStatMask, HeroSpawnBody, HeroSpawnPrefix, WireTeam,
};
