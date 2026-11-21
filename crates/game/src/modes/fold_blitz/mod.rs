//! Game-room lifecycle, scene declarations, 0x041F mode variables, Blitz flyouts/talents,
//! and protocol integration for 3v3 Halcyon Fold (`CasualFold` / `RankedFold`) and `Blitz`.

mod opening;
mod setup;
mod tick_sync;

pub(crate) use opening::{
    allocate_aral_solo_declaration, allocate_declaration, append_opening, append_opening_snapshot,
    solo_opening,
};
pub(crate) use setup::{
    hero_slot, is_blitz, is_managed, prepare_world, trim_to_three_per_side_roster,
};
pub(crate) use tick_sync::{publish_tick, sync_shops, talent_response};

#[cfg(test)]
use setup::non_hero_skin;

#[cfg(test)]
#[path = "modes_3v3_blitz_tests.rs"]
mod tests;
