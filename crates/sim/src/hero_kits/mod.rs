//! Registry of hero kit specifications across the 61 heroes in `abilities.json`.
//!
//! Ownership split:
//! - `batch_a`: `abilities.json` indices `0..30` (`Amael` ..= `Leo`, including `Idris` and `Karas`)
//! - `batch_b`: `abilities.json` indices `30..61` (`Lorelai` ..= `LanceBall_Lance`)

mod batch_a;
mod batch_b;
#[cfg(test)]
mod tests;

use crate::hero_kit::HeroKitSpec;

/// Look up a hero kit spec by numeric `Entity::type_id`.
pub(crate) fn kit_spec(type_id: u16) -> Option<&'static HeroKitSpec> {
    batch_a::specs()
        .iter()
        .chain(batch_b::specs().iter())
        .copied()
        .find(|s| s.type_id == type_id)
}

/// Look up a hero kit spec by exact/case-insensitive `abilities.json` `hero_id`.
pub(crate) fn kit_spec_by_hero_id(hero_id: &str) -> Option<&'static HeroKitSpec> {
    batch_a::specs()
        .iter()
        .chain(batch_b::specs().iter())
        .copied()
        .find(|s| s.hero_id.eq_ignore_ascii_case(hero_id))
}

/// Collect all currently registered hero kit specifications.
#[allow(dead_code)]
pub(crate) fn all_specs() -> Vec<&'static HeroKitSpec> {
    let mut out = batch_a::specs().to_vec();
    out.extend_from_slice(batch_b::specs());
    out
}
