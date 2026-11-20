//! Passive T2/T3 item mechanics, damage-triggered passives, and per-tick stacking/decay.
use super::*;
use crate::item_actives::{
    ActiveRules, CONTRACT_ARM_WINDOW, DamageContext, DamageOrigin, PulseweaveBurst,
    STORMGUARD_COOLDOWN_KEY, SpellfireBurn, apply_slow, level_lerp, opt_frac, opt_pos, opt_ticks,
};
use halcyon_data::items::{FullItem, ItemCatalog};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PassiveRules {
    pub bp_max_stacks: u8,
    pub bp_wp_per_stack: f32,
    pub bp_damage_to_stack: f32,
    pub bp_step_melee: f32,
    pub bp_step_ranged: f32,
    pub bp_time_to_decay: u64,
    pub bp_decay_per_sec: f32,
    pub de_cp_per_stack: f32,
    pub de_amp_duration: u64,
    pub de_falloff_per_sec: f32,
    pub de_max_stacks: u8,
    pub de_stack_interval: u64,
    pub aftershock_cooldown: u64,
    pub aftershock_pct_max_hp: f32,
    pub aftershock_non_hero_cap: f32,
    pub aftershock_vamp: f32,
    pub spellfire_ratio: f32,
    pub spellfire_duration: u64,
    pub spellfire_tps: f32,
    pub spellfire_base: f32,
    pub spellfire_max: f32,
    pub frostburn_slow: f32,
    pub frostburn_duration: u64,
    pub shiv_hits_to_proc: u8,
    pub shiv_mortal_duration: u64,
    pub clockwork_lockout: u64,
    pub clockwork_reduction: f32,
    pub clockwork_cap_ticks: u64,
    pub eve_vamp: f32,
    pub eve_cap_min: f32,
    pub eve_cap_max: f32,
    pub eve_recharge_ticks: u64,
    pub eve_omni_vamp: f32,
    pub serpent_vamp: f32,
    pub serpent_cap_min: f32,
    pub serpent_cap_max: f32,
    pub serpent_recharge_ticks: u64,
    pub husk_threshold: f32,
    pub husk_burst_window: u64,
    pub husk_fortify_duration: u64,
    pub husk_lockout: u64,
    pub seraphic_magic_reduction: f32,
    pub seraphic_cooldown: u64,
    pub seraphic_block_duration: u64,
    pub spellsword_non_hero_energy: f32,
    pub spellsword_hero_energy: f32,
    pub stormcrown_base: f32,
    pub stormcrown_per_level: f32,
    pub stormcrown_non_jungle_mult: f32,
    pub stormcrown_cooldown: u64,
    pub stormguard_base: f32,
    pub stormguard_per_level: f32,
    pub stormguard_cooldown: u64,
    pub bonesaw_shred: f32,
    pub bonesaw_duration: u64,
    pub bonesaw_max_stacks: u8,
    pub pulseweave_cooldown: u64,
    pub pulseweave_trigger_time: u64,
    pub pulseweave_ms: f32,
    pub pulseweave_damage_base: f32,
    pub pulseweave_damage_bhp: f32,
    pub pulseweave_slow_base: f32,
    pub pulseweave_slow_bhp: f32,
    pub pulseweave_slow_duration: u64,
    pub pulseweave_radius: f32,
    pub pulseweave_dps_factor: f32,
    pub tornado_ms: f32,
    pub tornado_duration: u64,
    pub lifespring_heal_pct: f32,
    pub lifespring_ooc_ticks: u64,
    pub metal_jacket_basic_reduction: f32,
}

impl Default for PassiveRules {
    fn default() -> Self {
        Self {
            bp_max_stacks: 35,
            bp_wp_per_stack: 5.0,
            bp_damage_to_stack: 100.0,
            bp_step_melee: 5.0,
            bp_step_ranged: 10.0,
            bp_time_to_decay: 3 * TICK_HZ,
            bp_decay_per_sec: 5.0,
            de_cp_per_stack: 10.0,
            de_amp_duration: 5 * TICK_HZ,
            de_falloff_per_sec: 3.0,
            de_max_stacks: 15,
            de_stack_interval: TICK_HZ,
            aftershock_cooldown: TICK_HZ,
            aftershock_pct_max_hp: 0.10,
            aftershock_non_hero_cap: 300.0,
            aftershock_vamp: 0.25,
            spellfire_ratio: 0.5,
            spellfire_duration: 3 * TICK_HZ,
            spellfire_tps: 2.0,
            spellfire_base: 7.0,
            spellfire_max: 40.0,
            frostburn_slow: 0.25,
            frostburn_duration: 3 * TICK_HZ / 2,
            shiv_hits_to_proc: 2,
            shiv_mortal_duration: 2 * TICK_HZ,
            clockwork_lockout: 3 * TICK_HZ,
            clockwork_reduction: 0.1,
            clockwork_cap_ticks: 3 * TICK_HZ,
            eve_vamp: 0.25,
            eve_cap_min: 400.0,
            eve_cap_max: 800.0,
            eve_recharge_ticks: 120 * TICK_HZ,
            eve_omni_vamp: 0.15,
            serpent_vamp: 0.25,
            serpent_cap_min: 400.0,
            serpent_cap_max: 800.0,
            serpent_recharge_ticks: 120 * TICK_HZ,
            husk_threshold: 0.25,
            husk_burst_window: TICK_HZ,
            husk_fortify_duration: 3 * TICK_HZ,
            husk_lockout: 30 * TICK_HZ,
            seraphic_magic_reduction: 1.0,
            seraphic_cooldown: 35 * TICK_HZ,
            seraphic_block_duration: TICK_HZ / 2,
            spellsword_non_hero_energy: 4.0,
            spellsword_hero_energy: 12.0,
            stormcrown_base: 500.0,
            stormcrown_per_level: 22.727,
            stormcrown_non_jungle_mult: 0.25,
            stormcrown_cooldown: 6 * TICK_HZ,
            stormguard_base: 250.0,
            stormguard_per_level: 4.545,
            stormguard_cooldown: 6 * TICK_HZ,
            bonesaw_shred: 0.1,
            bonesaw_duration: 3 * TICK_HZ,
            bonesaw_max_stacks: 5,
            pulseweave_cooldown: 45 * TICK_HZ,
            pulseweave_trigger_time: 3 * TICK_HZ,
            pulseweave_ms: 2.0,
            pulseweave_damage_base: 50.0,
            pulseweave_damage_bhp: 0.15,
            pulseweave_slow_base: 0.05,
            pulseweave_slow_bhp: 0.0003,
            pulseweave_slow_duration: 3 * TICK_HZ,
            pulseweave_radius: 2.7,
            pulseweave_dps_factor: 0.5,
            tornado_ms: 0.1,
            tornado_duration: 24,
            lifespring_heal_pct: 0.015,
            lifespring_ooc_ticks: 5 * TICK_HZ,
            metal_jacket_basic_reduction: 0.15,
        }
    }
}

fn opt_count(v: Option<f32>) -> Result<Option<u8>, String> {
    let Some(x) = v else { return Ok(None) };
    if !x.is_finite() || x.fract() != 0. || !(1.0..=255.0).contains(&x) {
        return Err(format!("invalid passive stack limit: {x}"));
    }
    Ok(Some(x as u8))
}

fn damage_extra_float(item: &FullItem) -> Option<f32> {
    item.passive_variables
        .iter()
        .find(|v| v.label == "DAMAGE")
        .and_then(|v| v.extra_floats.first().copied())
        .filter(|v| v.is_finite() && *v >= 0.)
}

impl PassiveRules {
    pub(super) fn load(catalog: Option<&ItemCatalog>) -> Result<Self, String> {
        let mut r = Self::default();
        let Some(cat) = catalog else { return Ok(r) };

        if let Some(it) = cat.item("Item_BreakingPoint") {
            if let Some(v) = opt_count(it.passive_value("MAX_STACKS"))? {
                r.bp_max_stacks = v;
            }
            if let Some(v) = opt_pos(it.passive_value("WEAPON"))? {
                r.bp_wp_per_stack = v;
            }
            if let Some(v) = opt_pos(it.passive_value("DAMAGE_TO_STACK"))? {
                r.bp_damage_to_stack = v;
            }
            if let Some(v) = opt_pos(it.passive_value("STACK_STEP"))? {
                r.bp_step_melee = v;
            }
            if let Some(v) = opt_pos(it.passive_value("RANGED_STACK_STEP"))? {
                r.bp_step_ranged = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("TIME_TO_DECAY"))? {
                r.bp_time_to_decay = v;
            }
            if let Some(v) = opt_pos(it.passive_value("STACKS_TO_DECAY"))? {
                r.bp_decay_per_sec = v;
            }
        }
        if let Some(it) = cat.item("Item_DragonsEye") {
            if let Some(v) = opt_pos(it.passive_value("BONUS_CRYSTAL"))? {
                r.de_cp_per_stack = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("AMP_DURATION"))? {
                r.de_amp_duration = v;
            }
            if let Some(v) = opt_pos(it.passive_value("FALLOFF_PER_SEC"))? {
                r.de_falloff_per_sec = v;
            }
            if let Some(v) = opt_count(it.passive_value("MAX_STACKS"))? {
                r.de_max_stacks = v;
            }
        }
        if let Some(it) = cat.item("Item_Aftershock")
            && let Some(v) = opt_ticks(it.passive_value("COOLDOWN"))?
        {
            r.aftershock_cooldown = v;
        }
        if let Some(it) = cat.item("Item_Spellfire") {
            if let Some(v) = opt_pos(it.passive_value("DAMAGE_RATIO"))? {
                r.spellfire_ratio = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("DAMAGE_DURATION"))? {
                r.spellfire_duration = v;
            }
            if let Some(v) = opt_pos(it.passive_value("TICKS_PER_SECOND"))? {
                r.spellfire_tps = v;
            }
            if let Some(v) = opt_pos(it.passive_value("DAMAGE_BASE"))? {
                r.spellfire_base = v;
            }
            if let Some(v) = opt_pos(it.passive_value("DAMAGE_MAX"))? {
                r.spellfire_max = v.max(r.spellfire_base);
            }
        }
        if let Some(it) = cat.item("Item_Frostburn") {
            if let Some(v) = opt_frac(it.passive_value("SLOW_STRENGTH"))? {
                r.frostburn_slow = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("SLOW_DURATION"))? {
                r.frostburn_duration = v;
            }
        }
        if let Some(it) = cat.item("Item_PoisonedShiv") {
            if let Some(v) = opt_count(it.passive_value("Hits_To_Proc"))? {
                r.shiv_hits_to_proc = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("Mortal_Wound_Duration"))? {
                r.shiv_mortal_duration = v;
            }
        }
        if let Some(it) = cat.item("Item_Clockwork") {
            if let Some(v) = opt_ticks(it.passive_value("LOCKOUT_DURATION"))? {
                r.clockwork_lockout = v;
            }
            if let Some(v) = opt_frac(it.passive_value("REDUCTION_AMOUNT"))? {
                r.clockwork_reduction = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("REDUCTION_CAP"))? {
                r.clockwork_cap_ticks = v;
            }
        }
        if let Some(it) = cat.item("Item_EveOfHarvest") {
            if let Some(v) = opt_frac(it.passive_value("VAMP"))? {
                r.eve_vamp = v;
            }
            if let Some(v) = opt_pos(it.passive_value("CAP_MIN"))? {
                r.eve_cap_min = v;
            }
            if let Some(v) = opt_pos(it.passive_value("CAP_MAX"))? {
                r.eve_cap_max = v.max(r.eve_cap_min);
            }
            if let Some(v) = opt_ticks(it.passive_value("RECHARGE_TIME"))? {
                r.eve_recharge_ticks = v;
            }
            if let Some(v) = opt_frac(it.passive_value("OMNI_VAMP"))? {
                r.eve_omni_vamp = v;
            }
        }
        if let Some(it) = cat.item("Item_SerpentMask") {
            if let Some(v) = opt_frac(it.passive_value("VAMP"))? {
                r.serpent_vamp = v;
            }
            if let Some(v) = opt_pos(it.passive_value("CAP_MIN"))? {
                r.serpent_cap_min = v;
            }
            if let Some(v) = opt_pos(it.passive_value("CAP_MAX"))? {
                r.serpent_cap_max = v.max(r.serpent_cap_min);
            }
            if let Some(v) = opt_ticks(it.passive_value("RECHARGE_TIME"))? {
                r.serpent_recharge_ticks = v;
            }
        }
        if let Some(it) = cat.item("Item_SlumberingHusk") {
            if let Some(v) = opt_frac(it.passive_value("Trigger_Threshold"))? {
                r.husk_threshold = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("Burst_Window"))? {
                r.husk_burst_window = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("Fortified_Health_Duration"))? {
                r.husk_fortify_duration = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("Lockout_Duration"))? {
                r.husk_lockout = v;
            }
        }
        if let Some(it) = cat.item("Item_SeraphicShell") {
            if let Some(v) = opt_frac(it.passive_value("MAGIC_DAMAGE_REDUCTION"))? {
                r.seraphic_magic_reduction = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("COOLDOWN"))? {
                r.seraphic_cooldown = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("BLOCK_DURATION"))? {
                r.seraphic_block_duration = v;
            }
        }
        if let Some(it) = cat.item("Item_Spellsword")
            && let Some(v) = opt_pos(it.passive_value("NON_HERO_ENERGY_RECHARGE"))?
        {
            r.spellsword_non_hero_energy = v;
        }
        if let Some(it) = cat.item("Item_Stormcrown") {
            if let Some(v) = opt_pos(it.passive_value("DAMAGE"))? {
                r.stormcrown_base = v;
            }
            if let Some(v) = damage_extra_float(it) {
                r.stormcrown_per_level = v;
            }
            if let Some(v) = opt_frac(it.passive_value("DAMAGE_HEROES"))? {
                r.stormcrown_non_jungle_mult = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("COOLDOWN"))? {
                r.stormcrown_cooldown = v;
            }
        }
        if let Some(it) = cat.item("Item_StormguardBanner") {
            if let Some(v) = opt_pos(it.passive_value("DAMAGE"))? {
                r.stormguard_base = v;
            }
            if let Some(v) = damage_extra_float(it) {
                r.stormguard_per_level = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("COOLDOWN"))? {
                r.stormguard_cooldown = v;
            }
        }
        if let Some(it) = cat.item("Item_Bonesaw") {
            if let Some(v) = opt_frac(it.passive_value("SHRED"))? {
                r.bonesaw_shred = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("DURATION"))? {
                r.bonesaw_duration = v;
            }
            if let Some(v) = opt_count(it.passive_value("MAX"))? {
                r.bonesaw_max_stacks = v;
            }
        }
        if let Some(it) = cat.item("Item_Pulseweave") {
            if let Some(v) = opt_ticks(it.passive_value("COOLDOWN"))? {
                r.pulseweave_cooldown = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("TRIGGER_TIME"))? {
                r.pulseweave_trigger_time = v;
            }
            if let Some(v) = opt_pos(it.passive_value("MSPD_STRENGTH"))? {
                r.pulseweave_ms = v;
            }
            if let Some(v) = opt_pos(it.passive_value("DAMAGE_BHP_SCALING"))? {
                r.pulseweave_damage_bhp = v;
            }
            if let Some(v) = opt_frac(it.passive_value("SLOW_BASE_STRENGTH"))? {
                r.pulseweave_slow_base = v;
            }
            if let Some(v) = opt_pos(it.passive_value("SLOW_BHP_SCALING"))? {
                r.pulseweave_slow_bhp = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("SLOW_DURATION"))? {
                r.pulseweave_slow_duration = v;
            }
            if let Some(v) = opt_pos(it.passive_value("BURN_RADIUS"))? {
                r.pulseweave_radius = v;
            }
            if let Some(v) = opt_pos(it.passive_value("DAMAGE_PER_SECOND"))? {
                r.pulseweave_dps_factor = v;
            }
        }
        if let Some(it) = cat.item("Item_TornadoTrigger") {
            if let Some(v) = opt_pos(it.passive_value("MSPD_STRENGTH"))? {
                r.tornado_ms = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("MSPD_DURATION"))? {
                r.tornado_duration = v;
            }
        }
        if let Some(it) = cat.item("Item_Lifespring") {
            if let Some(v) = opt_frac(it.passive_value("PCT_MISSING_HEALTH_HEAL"))? {
                r.lifespring_heal_pct = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("TIME_OUT_OF_COMBAT"))? {
                r.lifespring_ooc_ticks = v;
            }
        }
        if let Some(it) = cat.item("Item_MetalJacket")
            && let Some(v) = opt_frac(it.passive_value("BASIC_ATTACK_REDUCTION"))?
        {
            r.metal_jacket_basic_reduction = v;
        }
        Ok(r)
    }
}

pub(super) fn sync_after_inventory_change(
    e: &mut Entity,
    purchased: Option<BasicItem>,
    tick: u64,
    rules: &PassiveRules,
) {
    if e.kind != Kind::Hero {
        return;
    }
    let needs = e
        .inventory
        .iter()
        .any(|item| Entity::item_needs_runtime(item.kind));
    if !needs && e.item_runtime.is_none() {
        e.status.spell_shield_up = false;
        return;
    }
    if purchased == Some(BasicItem::MinionsFoot) {
        e.rt_mut().minions_foot_ready = true;
    } else if !e.has_item(BasicItem::MinionsFoot)
        && let Some(rt) = e.item_runtime.as_deref_mut()
    {
        rt.minions_foot_ready = false;
    }

    if !e.has_item(BasicItem::BreakingPoint)
        && let Some(rt) = e.item_runtime.as_deref_mut()
        && (rt.bp_applied_wp != 0. || rt.bp_stacks != 0 || rt.bp_accum != 0.)
    {
        e.stats.weapon = (e.stats.weapon - rt.bp_applied_wp).max(0.);
        e.item_stats.bonus_weapon = (e.item_stats.bonus_weapon - rt.bp_applied_wp).max(0.);
        rt.bp_applied_wp = 0.;
        rt.bp_stacks = 0;
        rt.bp_accum = 0.;
        rt.bp_decay_accum = 0.;
    }
    if !e.has_item(BasicItem::DragonsEye)
        && let Some(rt) = e.item_runtime.as_deref_mut()
        && (rt.de_applied_cp != 0. || rt.de_stacks != 0)
    {
        e.item_stats.crystal = (e.item_stats.crystal - rt.de_applied_cp).max(0.);
        rt.de_applied_cp = 0.;
        rt.de_stacks = 0;
        rt.de_decay_accum = 0.;
    }

    if e.has_item(BasicItem::SerpentMask) {
        let cap = level_lerp(rules.serpent_cap_min, rules.serpent_cap_max, e.level);
        let rt = e.rt_mut();
        if !rt.serpent_initialized {
            rt.serpent_initialized = true;
            rt.serpent_pool = cap;
        }
    }
    if e.has_item(BasicItem::EveOfHarvest) {
        let cap = level_lerp(rules.eve_cap_min, rules.eve_cap_max, e.level);
        let rt = e.rt_mut();
        if !rt.eve_initialized {
            rt.eve_initialized = true;
            rt.eve_pool = cap;
        }
    }
    if e.has_item(BasicItem::SeraphicShell) {
        let alive = e.hp > 0.;
        let up = {
            let rt = e.rt_mut();
            alive && (tick >= rt.seraphic_ready || tick < rt.seraphic_window_until)
        };
        e.status.spell_shield_up = up;
    } else {
        e.status.spell_shield_up = false;
    }
}

pub(super) fn on_ability_cast(
    e: &mut Entity,
    tick: u64,
    active: &ActiveRules,
    passive: &PassiveRules,
) {
    item_actives::arm_on_ability_cast(e, tick, active);
    if e.hp > 0.
        && e.kind == Kind::Hero
        && e.has_item(BasicItem::Aftershock)
        && e.rt().is_none_or(|rt| tick >= rt.aftershock_ready)
    {
        let _ = passive;
        e.rt_mut().aftershock_armed_until = tick + CONTRACT_ARM_WINDOW;
    }
}

fn adjust_bp_stacks(e: &mut Entity, new_stacks: u8, wp_per_stack: f32) {
    let target_wp = f32::from(new_stacks) * wp_per_stack;
    let prev_wp = e.rt().map_or(0., |rt| rt.bp_applied_wp);
    let delta = target_wp - prev_wp;
    if delta != 0. {
        e.stats.weapon = (e.stats.weapon + delta).max(0.);
        e.item_stats.bonus_weapon = (e.item_stats.bonus_weapon + delta).max(0.);
    }
    let rt = e.rt_mut();
    rt.bp_stacks = new_stacks;
    rt.bp_applied_wp = target_wp;
}

fn adjust_de_stacks(e: &mut Entity, new_stacks: u8, cp_per_stack: f32) {
    let target_cp = f32::from(new_stacks) * cp_per_stack;
    let prev_cp = e.rt().map_or(0., |rt| rt.de_applied_cp);
    let delta = target_cp - prev_cp;
    if delta != 0. {
        e.item_stats.crystal = (e.item_stats.crystal + delta).max(0.);
    }
    let rt = e.rt_mut();
    rt.de_stacks = new_stacks;
    rt.de_applied_cp = target_cp;
}

/// Apply incoming passive damage modifiers (`MetalJacket`, `AtlasPauldron`, `SeraphicShell`, `SlumberingHusk` fortify).
/// Returns `None` if negated completely by Celestial Shroud (`SeraphicShell`).
pub(super) fn filter_incoming_damage(
    entities: &mut actors::Actors<'_>,
    ctx: DamageContext,
    tick: u64,
    rules: &PassiveRules,
) -> Option<f32> {
    let DamageContext {
        src,
        target_idx,
        amount: raw_amount,
        school,
        origin,
        ..
    } = ctx;
    let src_entity = entities.iter().find(|e| e.id == src);
    let src_is_hero = src_entity.is_some_and(|e| e.kind == Kind::Hero);
    let src_team = src_entity.map(|e| e.team);

    // Atlas Pauldron reduces source WP damage by 30%.
    let mut amount = raw_amount;
    if matches!(school, DamageSchool::Weapon)
        && let Some(se) = src_entity
    {
        amount *= item_actives::atlas_weapon_multiplier(se, tick);
    }

    let target = &mut entities[target_idx];
    // Celestial Shroud (SeraphicShell): blocks enemy hero ability damage & opens 0.5s burst immunity window.
    if origin == DamageOrigin::Ability
        && src_is_hero
        && src_team != Some(target.team)
        && target.kind == Kind::Hero
        && target.has_item(BasicItem::SeraphicShell)
    {
        let rt = target.rt_mut();
        let active_shield = tick >= rt.seraphic_ready || tick < rt.seraphic_window_until;
        if active_shield {
            if rt.seraphic_window_until == 0 {
                rt.seraphic_window_until = tick + rules.seraphic_block_duration;
            }
            target.status.spell_shield_up = true;
            let blocked = amount * (1. - rules.seraphic_magic_reduction);
            if blocked <= 0. {
                return None;
            }
            amount = blocked;
        }
    }

    // Metal Jacket: take 15% less damage from basic attacks.
    if origin == DamageOrigin::BasicAttack
        && target.kind == Kind::Hero
        && target.has_item(BasicItem::MetalJacket)
    {
        amount *= (1. - rules.metal_jacket_basic_reduction).max(0.);
    }

    Some(amount)
}

pub(super) fn apply_slumbering_fortify(target: &mut Entity, remaining: f32, tick: u64) -> f32 {
    let Some(rt) = target.item_runtime.as_deref_mut() else {
        return remaining;
    };
    if tick < rt.slumbering_fortify_until && rt.slumbering_fortified > 0. {
        let saved = rt.slumbering_fortified.min(remaining * 0.5);
        rt.slumbering_fortified -= saved;
        (remaining - saved).max(0.)
    } else {
        remaining
    }
}

pub(super) fn on_damage_landed(room: &mut Room<'_>, ctx: DamageContext, events: &mut Vec<Event>) {
    let DamageContext {
        src,
        target_idx,
        amount: pre_barrier_mitigated,
        hp_loss,
        school,
        origin,
    } = ctx;
    let tick = room.state.tick;
    let rules = room.state.shop.effect_rules.passive;
    let active_rules = room.state.shop.effect_rules.active;
    let src_idx = room.entities.iter().position(|e| e.id == src);
    let src_is_hero = src_idx.is_some_and(|si| room.entities[si].kind == Kind::Hero);
    let target_kind = room.entities[target_idx].kind;
    let target_team = room.entities[target_idx].team;

    // 1. Defender triggers when hurt by an enemy hero.
    if src_is_hero
        && let Some(si) = src_idx
        && room.entities[si].team != target_team
        && target_kind == Kind::Hero
    {
        let target_alive = room.entities[target_idx].hp > 0.;
        if hp_loss > 0. && room.entities[target_idx].item_runtime.is_some()
            || room.entities[target_idx].has_item(BasicItem::Lifespring)
            || room.entities[target_idx].has_item(BasicItem::FountainOfRenewal)
            || room.entities[target_idx].has_item(BasicItem::Pulseweave)
            || room.entities[target_idx].has_travel_boots()
            || room.entities[target_idx].has_item(BasicItem::SlumberingHusk)
        {
            let target_max_hp = room.entities[target_idx].max_hp;
            let target_hp = room.entities[target_idx].hp;
            let has_husk = room.entities[target_idx].has_item(BasicItem::SlumberingHusk);
            let has_pw = room.entities[target_idx].has_item(BasicItem::Pulseweave);
            let rt = room.entities[target_idx].rt_mut();
            rt.last_hero_combat_tick = Some(tick);
            rt.last_enemy_hero_damage_taken_tick = Some(tick);

            // Slumbering Husk burst detection.
            if has_husk && hp_loss > 0. {
                let cutoff = tick.saturating_sub(rules.husk_burst_window);
                rt.slumbering_window_hits.retain(|&(t, _)| t >= cutoff);
                if let Some(last) = rt
                    .slumbering_window_hits
                    .last_mut()
                    .filter(|(t, _)| *t == tick)
                {
                    last.1 += hp_loss;
                } else {
                    rt.slumbering_window_hits.push((tick, hp_loss));
                }
                let burst: f32 = rt.slumbering_window_hits.iter().map(|&(_, d)| d).sum();
                if target_alive
                    && burst >= target_max_hp * rules.husk_threshold
                    && tick >= rt.slumbering_ready
                {
                    rt.slumbering_fortify_until = tick + rules.husk_fortify_duration;
                    rt.slumbering_fortified = target_hp;
                    rt.slumbering_ready = tick + rules.husk_lockout;
                    rt.slumbering_window_hits.clear();
                }
            }

            // Pulseweave speed + delayed burst trigger.
            if has_pw
                && hp_loss > 0.
                && target_alive
                && tick >= rt.pulseweave_ready
                && rt.pulseweave_burst.is_none()
            {
                rt.pulseweave_ready = tick + rules.pulseweave_cooldown;
                rt.pulseweave_burst = Some(PulseweaveBurst {
                    detonate_tick: tick + rules.pulseweave_trigger_time,
                    ms_bonus: rules.pulseweave_ms,
                });
            }
        }
    }

    // 2. Attacker triggers (Hero vs enemy).
    let Some(si) = src_idx else {
        return;
    };
    if !src_is_hero || room.entities[si].team == target_team {
        return;
    }

    // Hero vs Hero combat timers (Lifespring OOC, Journey Boots CD reduction, Dragon's Eye, Breaking Point).
    if target_kind == Kind::Hero {
        let atk = &room.entities[si];
        let has_lifespring = atk.has_item(BasicItem::Lifespring)
            || atk.has_item(BasicItem::FountainOfRenewal)
            || atk.has_item(BasicItem::Pulseweave);
        if has_lifespring || atk.item_runtime.is_some() {
            room.entities[si].rt_mut().last_hero_combat_tick = Some(tick);
        }
        if room.entities[si].has_item(BasicItem::JourneyBoots) {
            let cap = tick + active_rules.journey_combat_cooldown;
            if room.entities[si].item_stats.sprint_ready > cap {
                room.entities[si].item_stats.sprint_ready = cap;
            }
        }
        if matches!(origin, DamageOrigin::BasicAttack | DamageOrigin::Ability)
            && pre_barrier_mitigated > 0.
            && room.entities[si].has_item(BasicItem::DragonsEye)
        {
            let gain_stack = {
                let rt = room.entities[si].rt_mut();
                rt.de_last_damage_tick = tick;
                rt.de_stacks < rules.de_max_stacks
                    && (rt.de_stacks == 0
                        || tick >= rt.de_last_stack_tick + rules.de_stack_interval)
            };
            if gain_stack {
                let new_stacks = room.entities[si].rt().map_or(1, |rt| rt.de_stacks + 1);
                room.entities[si].rt_mut().de_last_stack_tick = tick;
                adjust_de_stacks(&mut room.entities[si], new_stacks, rules.de_cp_per_stack);
            }
        }
        if matches!(school, DamageSchool::Weapon)
            && matches!(
                origin,
                DamageOrigin::BasicAttack | DamageOrigin::Ability | DamageOrigin::ItemProc
            )
            && hp_loss > 0.
            && room.entities[si].has_item(BasicItem::BreakingPoint)
        {
            let ranged = room.entities[si].stats.range >= 4.;
            let step = if ranged {
                rules.bp_step_ranged
            } else {
                rules.bp_step_melee
            };
            let (mut stacks, mut accum) = {
                let rt = room.entities[si].rt();
                (
                    rt.map_or(0, |r| r.bp_stacks),
                    rt.map_or(0., |r| r.bp_accum) + hp_loss,
                )
            };
            let mut gained = false;
            while stacks < rules.bp_max_stacks {
                let threshold = rules.bp_damage_to_stack + f32::from(stacks) * step;
                if accum + 1e-4 >= threshold {
                    accum = (accum - threshold).max(0.);
                    stacks += 1;
                    gained = true;
                } else {
                    break;
                }
            }
            {
                let rt = room.entities[si].rt_mut();
                rt.bp_accum = accum;
                if gained {
                    rt.bp_last_stack_tick = tick;
                }
            }
            if gained {
                adjust_bp_stacks(&mut room.entities[si], stacks, rules.bp_wp_per_stack);
            }
        }
    }

    // Ability-only offensive passives (Spellfire, Frostburn, Clockwork, Eve of Harvest).
    if origin == DamageOrigin::Ability && pre_barrier_mitigated > 0. {
        // Clockwork cooldown reduction on enemy hero hit.
        if target_kind == Kind::Hero
            && room.entities[si].has_item(BasicItem::Clockwork)
            && room.entities[si]
                .rt()
                .is_none_or(|rt| tick >= rt.clockwork_ready)
        {
            let atk = &mut room.entities[si];
            for cd in &mut atk.cooldown {
                if *cd > tick {
                    let rem = *cd - tick;
                    let cut = ((rem as f32) * rules.clockwork_reduction)
                        .round()
                        .min(rules.clockwork_cap_ticks as f32) as u64;
                    *cd = tick + rem.saturating_sub(cut);
                }
            }
            atk.rt_mut().clockwork_ready = tick + rules.clockwork_lockout;
        }

        // Frostburn slow on enemy heroes.
        if target_kind == Kind::Hero && room.entities[si].has_item(BasicItem::Frostburn) {
            apply_slow(
                &mut room.entities[target_idx],
                tick,
                rules.frostburn_slow,
                rules.frostburn_duration,
            );
        }

        // Spellfire DoT + Mortal Wounds on non-lane-minion, non-structure enemies.
        if matches!(target_kind, Kind::Hero | Kind::Neutral)
            && room.entities[si].has_item(BasicItem::Spellfire)
        {
            let total_raw = level_lerp(
                rules.spellfire_base,
                rules.spellfire_max,
                room.entities[si].level,
            ) + rules.spellfire_ratio * room.entities[si].item_stats.crystal;
            let num_ticks = ((rules.spellfire_duration as f32 / TICK_HZ as f32)
                * rules.spellfire_tps)
                .round()
                .max(1.) as u8;
            let interval = (rules.spellfire_duration / u64::from(num_ticks)).max(1);
            let pierce = room.entities[si].item_stats.shield_pierce;
            let team = room.entities[si].team;
            let t_rt = room.entities[target_idx].rt_mut();
            if target_kind == Kind::Hero {
                t_rt.mortal_wounds_until = t_rt
                    .mortal_wounds_until
                    .max(tick + rules.spellfire_duration);
            }
            t_rt.spellfire_burn = Some(SpellfireBurn {
                source: src,
                team,
                raw_per_tick: total_raw / f32::from(num_ticks),
                shield_pierce: pierce,
                next_tick: tick + interval,
                remaining: num_ticks,
            });
        }

        // Eve of Harvest crystal/ability vamp + rechargeable hero pool.
        if hp_loss > 0.
            && matches!(school, DamageSchool::Crystal | DamageSchool::True)
            && room.entities[si].has_item(BasicItem::EveOfHarvest)
        {
            let mut heal_amount = hp_loss * rules.eve_omni_vamp;
            if target_kind == Kind::Hero {
                let cap = level_lerp(
                    rules.eve_cap_min,
                    rules.eve_cap_max,
                    room.entities[si].level,
                );
                let rt = room.entities[si].rt_mut();
                if !rt.eve_initialized {
                    rt.eve_initialized = true;
                    rt.eve_pool = cap;
                }
                let drained = hp_loss.min(rt.eve_pool.max(0.));
                rt.eve_pool = (rt.eve_pool - drained).max(0.);
                heal_amount += drained * rules.eve_vamp;
            }
            if heal_amount > 0. {
                room.heal(src, si, heal_amount, events);
            }
        }
    }
}

/// Trigger passive basic-attack item effects on impact (after primary hit, before optional follow-up procs).
/// Returns extra Serpent Mask hero-pool heal to add to attacker restoration.
pub(super) fn on_basic_hit(
    room: &mut Room<'_>,
    attacker_idx: usize,
    target_idx: usize,
    primary_weapon_loss: f32,
    events: &mut Vec<Event>,
) -> f32 {
    let tick = room.state.tick;
    let rules = room.state.shop.effect_rules.passive;
    let active_rules = room.state.shop.effect_rules.active;
    let src_id = room.entities[attacker_idx].id;
    let team = room.entities[attacker_idx].team;
    let victim_kind = room.entities[target_idx].kind;

    // 1. Spellsword energy recharge on any valid target (including towers/objectives).
    if room.entities[attacker_idx].has_item(BasicItem::Spellsword) {
        let gain = if victim_kind == Kind::Hero {
            rules.spellsword_hero_energy
        } else {
            rules.spellsword_non_hero_energy
        };
        let atk = &mut room.entities[attacker_idx];
        atk.energy = (atk.energy + gain).min(atk.max_energy);
    }

    // 2. Tornado Trigger short MS boost on basic attacks.
    if room.entities[attacker_idx].has_item(BasicItem::TornadoTrigger) {
        let rt = room.entities[attacker_idx].rt_mut();
        rt.tornado_ms_bonus = rules.tornado_ms;
        rt.tornado_ms_until = rt.tornado_ms_until.max(tick + rules.tornado_duration);
    }

    // 3. Poisoned Shiv Mortal Wounds every `Hits_To_Proc` basic hits on living Hero/Neutral.
    if room.entities[target_idx].hp > 0.
        && matches!(victim_kind, Kind::Hero | Kind::Neutral)
        && room.entities[attacker_idx].has_item(BasicItem::PoisonedShiv)
    {
        let proc_now = {
            let rt = room.entities[attacker_idx].rt_mut();
            rt.shiv_hits = rt.shiv_hits.saturating_add(1);
            if rt.shiv_hits >= rules.shiv_hits_to_proc {
                rt.shiv_hits = 0;
                true
            } else {
                false
            }
        };
        if proc_now && victim_kind == Kind::Hero {
            let t_rt = room.entities[target_idx].rt_mut();
            t_rt.mortal_wounds_until = t_rt
                .mortal_wounds_until
                .max(tick + rules.shiv_mortal_duration);
        }
    }

    // 4. Shiversteel / Dragonblood Contract slow.
    if room.entities[target_idx].hp > 0. {
        item_actives::trigger_slow_on_basic(
            &mut room.entities,
            attacker_idx,
            target_idx,
            tick,
            &active_rules,
        );
    }

    // 5. Protector Contract / Rook's Decree primed barrier.
    item_actives::trigger_contract_on_hero_basic(
        &mut room.entities,
        attacker_idx,
        victim_kind,
        tick,
        &active_rules,
    );

    // 6. Bonesaw armor shred stack.
    if room.entities[target_idx].hp > 0.
        && matches!(victim_kind, Kind::Hero | Kind::Neutral | Kind::Minion)
        && room.entities[attacker_idx].has_item(BasicItem::Bonesaw)
    {
        let t_rt = room.entities[target_idx].rt_mut();
        if tick >= t_rt.armor_shred_until {
            t_rt.armor_shred_stacks = 1;
        } else {
            t_rt.armor_shred_stacks = (t_rt.armor_shred_stacks + 1).min(rules.bonesaw_max_stacks);
        }
        t_rt.armor_shred_per_stack = rules.bonesaw_shred;
        t_rt.armor_shred_until = tick + rules.bonesaw_duration;
    }

    // 7. Stormguard (Stormcrown overrides StormguardBanner, non-stacking).
    let has_crown = room.entities[attacker_idx].has_item(BasicItem::Stormcrown);
    let has_banner = room.entities[attacker_idx].has_item(BasicItem::StormguardBanner);
    if (has_crown || has_banner)
        && room.entities[target_idx].hp > 0.
        && room.entities[attacker_idx]
            .rt()
            .is_none_or(|rt| tick >= rt.stormguard_ready)
    {
        let level = f32::from(room.entities[attacker_idx].level.clamp(1, 12) - 1);
        let (base, per_lvl, cd) = if has_crown {
            (
                rules.stormcrown_base,
                rules.stormcrown_per_level,
                rules.stormcrown_cooldown,
            )
        } else {
            (
                rules.stormguard_base,
                rules.stormguard_per_level,
                rules.stormguard_cooldown,
            )
        };
        let mult = if victim_kind == Kind::Neutral {
            1.
        } else {
            rules.stormcrown_non_jungle_mult
        };
        let dmg = (base + level * per_lvl) * mult;
        room.entities[attacker_idx].rt_mut().stormguard_ready = tick + cd;
        room.hurt_item(src_id, team, target_idx, dmg, DamageSchool::True, events);
        events.push(Event::ItemCooldownUpdated {
            actor: src_id,
            key: STORMGUARD_COOLDOWN_KEY,
            remaining_seconds: cd as f32 / TICK_HZ as f32,
        });
    }

    // 8. Aftershock bonus crystal damage + lifesteal if armed & off cooldown.
    if room.entities[target_idx].hp > 0.
        && room.entities[attacker_idx].has_item(BasicItem::Aftershock)
        && room.entities[attacker_idx]
            .rt()
            .is_some_and(|rt| tick < rt.aftershock_armed_until && tick >= rt.aftershock_ready)
    {
        let target_max_hp = room.entities[target_idx].max_hp;
        let mut raw = target_max_hp * rules.aftershock_pct_max_hp;
        if victim_kind != Kind::Hero {
            raw = raw.min(rules.aftershock_non_hero_cap);
        }
        let shield = room.entities[target_idx].stats.shield;
        let pierce = room.entities[attacker_idx].item_stats.shield_pierce;
        let mitigated_dmg = shop::mitigated(raw, shield, pierce);
        {
            let rt = room.entities[attacker_idx].rt_mut();
            rt.aftershock_armed_until = 0;
            rt.aftershock_ready = tick + rules.aftershock_cooldown;
        }
        let before = room.entities[target_idx].hp;
        room.hurt_item(
            src_id,
            team,
            target_idx,
            mitigated_dmg,
            DamageSchool::Crystal,
            events,
        );
        let dealt = (before - room.entities[target_idx].hp).max(0.);
        if dealt > 0. {
            room.heal(src_id, attacker_idx, dealt * rules.aftershock_vamp, events);
        }
    }

    // 9. Serpent Mask extra hero lifesteal pool.
    if victim_kind == Kind::Hero
        && primary_weapon_loss > 0.
        && room.entities[attacker_idx].has_item(BasicItem::SerpentMask)
    {
        let cap = level_lerp(
            rules.serpent_cap_min,
            rules.serpent_cap_max,
            room.entities[attacker_idx].level,
        );
        let rt = room.entities[attacker_idx].rt_mut();
        if !rt.serpent_initialized {
            rt.serpent_initialized = true;
            rt.serpent_pool = cap;
        }
        let drained = primary_weapon_loss.min(rt.serpent_pool.max(0.));
        rt.serpent_pool = (rt.serpent_pool - drained).max(0.);
        return drained * rules.serpent_vamp;
    }
    0.
}

pub(super) fn advance_passives(room: &mut Room<'_>, events: &mut Vec<Event>) {
    let tick = room.state.tick;
    let rules = room.state.shop.effect_rules.passive;
    let dt = 1. / TICK_HZ as f32;
    let n = room.entities.len();

    for i in 0..n {
        if room.entities[i].kind != Kind::Hero && room.entities[i].item_runtime.is_none() {
            continue;
        }

        // Spellfire burn tick on any afflicted entity (Hero or Neutral).
        let burn_due = room.entities[i]
            .rt()
            .and_then(|rt| rt.spellfire_burn)
            .filter(|b| tick >= b.next_tick);
        if let Some(mut burn) = burn_due {
            if room.entities[i].hp <= 0. || burn.remaining == 0 {
                if let Some(rt) = room.entities[i].item_runtime.as_deref_mut() {
                    rt.spellfire_burn = None;
                }
            } else {
                let dmg = shop::mitigated(
                    burn.raw_per_tick,
                    room.entities[i].stats.shield,
                    burn.shield_pierce,
                );
                let interval = (rules.spellfire_duration
                    / u64::from(
                        ((rules.spellfire_duration as f32 / TICK_HZ as f32) * rules.spellfire_tps)
                            .round()
                            .max(1.) as u8,
                    ))
                .max(1);
                burn.remaining -= 1;
                if burn.remaining == 0 {
                    if let Some(rt) = room.entities[i].item_runtime.as_deref_mut() {
                        rt.spellfire_burn = None;
                    }
                } else {
                    burn.next_tick = tick + interval;
                    if let Some(rt) = room.entities[i].item_runtime.as_deref_mut() {
                        rt.spellfire_burn = Some(burn);
                    }
                }
                room.hurt_item(
                    burn.source,
                    burn.team,
                    i,
                    dmg,
                    DamageSchool::Crystal,
                    events,
                );
            }
        }

        if room.entities[i].kind != Kind::Hero {
            continue;
        }

        let alive = room.entities[i].hp > 0. && !room.entities[i].is_rebooting();

        // SeraphicShell (Celestial Shroud) state transitions.
        if room.entities[i].has_item(BasicItem::SeraphicShell) {
            let rt = room.entities[i].rt_mut();
            if rt.seraphic_window_until > 0 && tick >= rt.seraphic_window_until {
                rt.seraphic_window_until = 0;
                rt.seraphic_ready = tick + rules.seraphic_cooldown;
            }
            let up = alive && (tick >= rt.seraphic_ready || tick < rt.seraphic_window_until);
            room.entities[i].status.spell_shield_up = up;
        } else {
            room.entities[i].status.spell_shield_up = false;
        }

        // Serpent Mask & Eve of Harvest pool recharge (even while respawning).
        if room.entities[i].has_item(BasicItem::SerpentMask) {
            let cap = level_lerp(
                rules.serpent_cap_min,
                rules.serpent_cap_max,
                room.entities[i].level,
            );
            let rate = cap / (rules.serpent_recharge_ticks.max(1) as f32);
            let rt = room.entities[i].rt_mut();
            if !rt.serpent_initialized {
                rt.serpent_initialized = true;
                rt.serpent_pool = cap;
            } else if rt.serpent_pool < cap {
                rt.serpent_pool = (rt.serpent_pool + rate).min(cap);
            }
        }
        if room.entities[i].has_item(BasicItem::EveOfHarvest) {
            let cap = level_lerp(rules.eve_cap_min, rules.eve_cap_max, room.entities[i].level);
            let rate = cap / (rules.eve_recharge_ticks.max(1) as f32);
            let rt = room.entities[i].rt_mut();
            if !rt.eve_initialized {
                rt.eve_initialized = true;
                rt.eve_pool = cap;
            } else if rt.eve_pool < cap {
                rt.eve_pool = (rt.eve_pool + rate).min(cap);
            }
        }

        if !alive {
            continue;
        }

        // Breaking Point decay.
        if room.entities[i].has_item(BasicItem::BreakingPoint) {
            let decay_step = room.entities[i]
                .rt()
                .filter(|rt| {
                    rt.bp_stacks > 0 && tick >= rt.bp_last_stack_tick + rules.bp_time_to_decay
                })
                .map(|_| rules.bp_decay_per_sec * dt);
            if let Some(inc) = decay_step {
                let (stacks, new_stacks) = {
                    let rt = room.entities[i].rt_mut();
                    rt.bp_accum = 0.;
                    rt.bp_decay_accum += inc;
                    let drop = rt.bp_decay_accum.floor() as u8;
                    if drop > 0 {
                        rt.bp_decay_accum -= f32::from(drop);
                        (true, rt.bp_stacks.saturating_sub(drop))
                    } else {
                        (false, rt.bp_stacks)
                    }
                };
                if stacks {
                    adjust_bp_stacks(&mut room.entities[i], new_stacks, rules.bp_wp_per_stack);
                }
            }
        }

        // Dragon's Eye decay.
        if room.entities[i].has_item(BasicItem::DragonsEye) {
            let decay_step = room.entities[i]
                .rt()
                .filter(|rt| {
                    rt.de_stacks > 0 && tick >= rt.de_last_damage_tick + rules.de_amp_duration
                })
                .map(|_| rules.de_falloff_per_sec * dt);
            if let Some(inc) = decay_step {
                let (drop_now, new_stacks) = {
                    let rt = room.entities[i].rt_mut();
                    rt.de_decay_accum += inc;
                    let drop = rt.de_decay_accum.floor() as u8;
                    if drop > 0 {
                        rt.de_decay_accum -= f32::from(drop);
                        (true, rt.de_stacks.saturating_sub(drop))
                    } else {
                        (false, rt.de_stacks)
                    }
                };
                if drop_now {
                    adjust_de_stacks(&mut room.entities[i], new_stacks, rules.de_cp_per_stack);
                }
            }
        }

        // Lifespring (non-stacking across Lifespring, FountainOfRenewal, Pulseweave).
        let has_lifespring = room.entities[i].has_item(BasicItem::Lifespring)
            || room.entities[i].has_item(BasicItem::FountainOfRenewal)
            || room.entities[i].has_item(BasicItem::Pulseweave);
        if has_lifespring && room.entities[i].hp < room.entities[i].max_hp {
            let ooc = room.entities[i].rt().is_none_or(|rt| {
                rt.last_hero_combat_tick
                    .is_none_or(|hit| tick >= hit + rules.lifespring_ooc_ticks)
            });
            if ooc {
                let missing = (room.entities[i].max_hp - room.entities[i].hp).max(0.);
                let amount = missing * rules.lifespring_heal_pct * dt;
                if amount > 0. {
                    let id = room.entities[i].id;
                    room.heal(id, i, amount, events);
                }
            }
        }

        // Pulseweave passive burn & triggered burst detonation.
        if room.entities[i].has_item(BasicItem::Pulseweave) {
            let burst_due = room.entities[i]
                .rt()
                .and_then(|rt| rt.pulseweave_burst)
                .filter(|b| tick >= b.detonate_tick);
            let bonus_hp = room.entities[i].bonus_hp.max(0.);
            let origin = room.entities[i].position;
            let team = room.entities[i].team;
            let src_id = room.entities[i].id;
            let pierce = room.entities[i].item_stats.shield_pierce;

            if burst_due.is_some() {
                if let Some(rt) = room.entities[i].item_runtime.as_deref_mut() {
                    rt.pulseweave_burst = None;
                }
                let raw_dmg = rules.pulseweave_damage_base + rules.pulseweave_damage_bhp * bonus_hp;
                let slow_strength =
                    (rules.pulseweave_slow_base + rules.pulseweave_slow_bhp * bonus_hp).min(0.85);
                for j in 0..n {
                    let victim = &room.entities[j];
                    if victim.team == team
                        || victim.hp <= 0.
                        || !matches!(victim.kind, Kind::Hero | Kind::Neutral | Kind::Minion)
                        || origin.dist(victim.position) > rules.pulseweave_radius
                    {
                        continue;
                    }
                    let mitigated_dmg = shop::mitigated(raw_dmg, victim.stats.shield, pierce);
                    room.hurt_item(
                        src_id,
                        team,
                        j,
                        mitigated_dmg,
                        DamageSchool::Crystal,
                        events,
                    );
                    if room.entities[j].hp > 0. {
                        apply_slow(
                            &mut room.entities[j],
                            tick,
                            slow_strength,
                            rules.pulseweave_slow_duration,
                        );
                    }
                }
            } else if room.entities[i].rt().is_none_or(|rt| {
                tick >= rt.pulseweave_ready
                    && rt.pulseweave_burst.is_none()
                    && tick >= rt.pulseweave_next_burn_tick
            }) {
                let has_enemy_in_range = room.entities.iter().any(|e| {
                    e.team != team
                        && e.hp > 0.
                        && matches!(e.kind, Kind::Hero | Kind::Neutral | Kind::Minion)
                        && origin.dist(e.position) <= rules.pulseweave_radius
                });
                if has_enemy_in_range {
                    room.entities[i].rt_mut().pulseweave_next_burn_tick = tick + TICK_HZ;
                    let raw_dps = (rules.pulseweave_damage_base
                        + rules.pulseweave_damage_bhp * bonus_hp)
                        * rules.pulseweave_dps_factor;
                    for j in 0..n {
                        let victim = &room.entities[j];
                        if victim.team == team
                            || victim.hp <= 0.
                            || !matches!(victim.kind, Kind::Hero | Kind::Neutral | Kind::Minion)
                            || origin.dist(victim.position) > rules.pulseweave_radius
                        {
                            continue;
                        }
                        let dmg = shop::mitigated(raw_dps, victim.stats.shield, pierce);
                        room.hurt_item(src_id, team, j, dmg, DamageSchool::Crystal, events);
                    }
                }
            }
        }
    }
}
