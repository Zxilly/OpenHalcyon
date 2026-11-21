//! Registry of hero kit specifications across all 61 heroes in `abilities.json`.

mod assassins_snipers;
mod captains_supports;
mod mages;
#[cfg(test)]
mod tests;
mod warriors_brawlers;

use crate::combat::AbilitySlot;
use crate::ecs::score::DamageSchool;
use crate::heroes::engine::{
    HeroKitContext, HeroKitSpec, KitEffect, KitEffectKind, seconds_to_ticks,
};
use crate::{Position, TICK_HZ};

use assassins_snipers::{
    ANKA, BARON, CAINE, GWEN, IDRIS, INARA, KESTREL, KINETIC, KOSHKA, MIHO, REZA, RINGO, SAW,
    SILVERNAIL, TAKA, VOX, YLVA,
};
use captains_supports::{
    ADAGIO, ARDAN, CATHERINE, CHURNWALKER, FLICKER, FORTRESS, GRACE, HERO057, HERO062, HERO999,
    LANCE, LANCEBALL_LANCE, LORELAI, LYRA, PHINN, VIOLA, YATES,
};
use mages::{CELESTE, ISHTAR, KARAS, MAGNUS, MALENE, PETAL, SAMUEL, SKAARF, SKYE, VARYA, WARHAWK};
use warriors_brawlers::{
    ALPHA, AMAEL, BAPTISTE, BLACKFEATHER, GLAIVE, GRUMPJAW, JOULE, KENSEI, KRUL, LEO, OZO, REIM,
    RONA, SANFENG, SHIN, TONY,
};

pub(super) fn clamp_pos(p: Position) -> Position {
    Position {
        x: p.x.clamp(-120.0, 120.0),
        y: p.y.clamp(-80.0, 80.0),
    }
}

pub(super) fn unit_dir(from: Position, to: Position) -> (f32, f32) {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let d = dx.hypot(dy);
    if d <= 0.0001 {
        (1.0, 0.0)
    } else {
        (dx / d, dy / d)
    }
}

pub(super) fn lerp_pos(a: Position, b: Position, t: f32) -> Position {
    Position {
        x: a.x + (b.x - a.x) * t,
        y: a.y + (b.y - a.y) * t,
    }
}

pub(super) fn move_toward_or_dash(ctx: &mut HeroKitContext<'_, '_>, goal: Position) {
    let origin = ctx.caster_pos();
    if ctx.room.spell_motion_clear(origin, goal) {
        ctx.room.place_spell_actor(ctx.caster, goal, ctx.events);
    }
}

pub(super) fn spawn_single_hit_dot(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    target_idx: usize,
    tick_dmg: f32,
    school: DamageSchool,
    dur_sec: f32,
    minion_mult: f32,
) {
    if target_idx >= ctx.room.entities.len() || tick_dmg <= 0.0 || dur_sec <= 0.0 {
        return;
    }
    let pos = ctx.room.entities[target_idx].position;
    let target_id = ctx.room.entities[target_idx].id;
    ctx.spawn_effect(
        KitEffectKind::DoT {
            ability_slot: slot.idx() as u8,
            target_id,
            tick_damage: tick_dmg,
            school,
            ticks_interval: TICK_HZ,
            next_tick: ctx.tick + TICK_HZ,
            end_tick: ctx.tick + seconds_to_ticks(dur_sec),
            minion_mult,
        },
        pos,
        pos,
    );
}

pub(super) fn has_nearby_active_zone(
    ctx: &HeroKitContext<'_, '_>,
    pos: Position,
    zone_kind: u8,
    pad: f32,
) -> bool {
    let owner_id = ctx.caster_id();
    ctx.room.entities.spells::<KitEffect>().any(|eff| {
        eff.owner_id == owner_id
            && matches!(
                eff.kind,
                KitEffectKind::GroundZone {
                    zone_kind: zk,
                    radius,
                    ..
                } if zk == zone_kind && eff.position.dist(pos) <= radius + pad
            )
    })
}

static ALL_SPECS: &[&HeroKitSpec] = &[
    &AMAEL,
    &ADAGIO,
    &ALPHA,
    &ANKA,
    &ARDAN,
    &BAPTISTE,
    &BARON,
    &BLACKFEATHER,
    &CAINE,
    &CATHERINE,
    &CELESTE,
    &CHURNWALKER,
    &FLICKER,
    &FORTRESS,
    &GLAIVE,
    &GRACE,
    &GRUMPJAW,
    &GWEN,
    &IDRIS,
    &INARA,
    &ISHTAR,
    &JOULE,
    &KARAS,
    &KENSEI,
    &KESTREL,
    &KINETIC,
    &KOSHKA,
    &KRUL,
    &LANCE,
    &LEO,
    &LORELAI,
    &LYRA,
    &MAGNUS,
    &MALENE,
    &MIHO,
    &OZO,
    &PETAL,
    &PHINN,
    &REIM,
    &REZA,
    &RINGO,
    &RONA,
    &SAMUEL,
    &SANFENG,
    &SAW,
    &SHIN,
    &SILVERNAIL,
    &SKAARF,
    &SKYE,
    &TAKA,
    &TONY,
    &VARYA,
    &VIOLA,
    &VOX,
    &WARHAWK,
    &YATES,
    &YLVA,
    &HERO057,
    &HERO062,
    &HERO999,
    &LANCEBALL_LANCE,
];

/// Look up a hero kit spec by numeric `Entity::type_id`.
pub(crate) fn kit_spec(type_id: u16) -> Option<&'static HeroKitSpec> {
    ALL_SPECS.iter().copied().find(|s| s.type_id == type_id)
}

/// Look up a hero kit spec by exact/case-insensitive `abilities.json` `hero_id`.
pub(crate) fn kit_spec_by_hero_id(hero_id: &str) -> Option<&'static HeroKitSpec> {
    ALL_SPECS
        .iter()
        .copied()
        .find(|s| s.hero_id.eq_ignore_ascii_case(hero_id))
}

/// Collect all currently registered hero kit specifications.
#[allow(dead_code)]
pub(crate) fn all_specs() -> Vec<&'static HeroKitSpec> {
    ALL_SPECS.to_vec()
}

#[cfg(test)]
pub(super) fn batch_b_specs() -> &'static [&'static HeroKitSpec] {
    &ALL_SPECS[30..]
}
