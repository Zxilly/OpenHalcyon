//! Compact entity (`0x03F2`) and full hero (`0x03F3`) spawn codecs.

mod entity_spawn;
mod hero_spawn;

pub use entity_spawn::{EntitySpawnBody, EntityStatMask, WireTeam};
pub use hero_spawn::{HeroSpawnBody, HeroSpawnPrefix};
