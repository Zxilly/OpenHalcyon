//! Deterministic straight-lane validation map. Coordinates are project world units.
//! Vision, spawn offsets, windup, attack cadence, retreat and respawn are project rules.

mod ai;
mod combat;
mod ecs;
mod heroes;
mod items;
pub mod maps;
mod nav;

pub use ai::{
    AbilityRole, ActiveItemCategory, ActiveItemIntent, BotDiagnostics, BotHeroDiagnostics,
    ClassifiedAbility, HeroIntentCatalog, HeroKitIntent, MacroMode, ObjectiveInfo, ObjectiveKind,
    TargetPattern, ThreatAssessment,
};
pub use combat::native_learn::{
    known_native_learn_operand, native_learn_operand, native_learn_request,
};
pub use combat::{AbilitySlot, CastTarget};
pub use ecs::actors::Actors;
pub use ecs::rules::{SOLO_CRYSTAL_START_INSTANCES, WorldRules};
pub use ecs::score::{PlayerScore, TeamScore};
pub use ecs::world::{
    Command, Entity, Event, ImpactEffect, Kind, Position, Room, RoomState, TICK_HZ, World,
    rules_digest,
};
pub use heroes::native::amael::KickKind;
pub use items::actives::ItemTarget;
pub use items::aral_inventory::{ARAL_ITEM_CAPACITY, AralInventory, AralItem, AralStarter};
pub use items::shop::{BasicItem, HeldItem, ItemSale, ItemStats, NativeItemStat, Purchase};
pub use items::vision_devices::{VisionDevice, VisionDeviceKind};
pub use maps::blitz::BlitzTalentSlot;
pub use maps::sovereigns_5v5::{
    CampKind5v5, CampVisibilityState5v5, CampVisibilityTransition5v5, Hero5v5Status,
};
pub use maps::{blitz as map_blitz, fold_3v3 as map_3v3, sovereigns_5v5 as map_5v5};
