//! Active item mechanics, vision actives, contracts, boots, and compact per-hero item runtime state.
use super::*;
use halcyon_data::items::{FullItem, ItemActiveParameter, ItemCatalog};

/// Wire cooldown key `fnv1a_32("Ability__Item__HealingFlask")`.
pub(super) const HEALING_FLASK_COOLDOWN_KEY: u32 = 0x1b18_4adf;
/// Wire cooldown key `fnv1a_32("StormguardBanner")`.
pub(super) const STORMGUARD_COOLDOWN_KEY: u32 = 0xdb16_1949;
/// Out-of-combat window before Travel Boots regain full speed after hero damage.
const TRAVEL_BOOTS_COMBAT_WINDOW: u64 = 5 * TICK_HZ;
/// Window after ability cast during which Protector Contract / Rook's Decree / Aftershock stays primed.
pub(super) const CONTRACT_ARM_WINDOW: u64 = 5 * TICK_HZ;
/// Mortal Wounds healing and barrier reduction multiplier (`-33%`).
pub(super) const MORTAL_WOUNDS_MULTIPLIER: f32 = 0.67;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum ItemTarget {
    SelfCast,
    Entity(u32),
    Point(Position),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DamageOrigin {
    BasicAttack,
    Ability,
    ItemProc,
}

#[derive(Clone, Copy)]
pub(super) struct DamageContext {
    pub src: u32,
    pub target_idx: usize,
    pub amount: f32,
    pub hp_loss: f32,
    pub school: DamageSchool,
    pub origin: DamageOrigin,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ActiveRules {
    pub reflex_low: f32,
    pub reflex_high: f32,
    pub reflex_duration: u64,
    pub reflex_cooldown: u64,
    pub aegis_cooldown: u64,
    pub crucible_cooldown: u64,
    pub crucible_range: f32,
    pub fountain_cooldown: u64,
    pub fountain_heal_per_pct: f32,
    pub fountain_heal_ticks: u64,
    pub fountain_range: f32,
    pub fountain_travel_ticks: u64,
    pub atlas_cooldown: u64,
    pub atlas_as_slow: f32,
    pub atlas_wp_reduction: f32,
    pub atlas_duration: u64,
    pub atlas_range: f32,
    pub atlas_delay: u64,
    pub boot_travel_bonus: f32,
    pub travel_cooldown: u64,
    pub travel_speed: f32,
    pub travel_duration: u64,
    pub journey_cooldown: u64,
    pub journey_combat_cooldown: u64,
    pub journey_speed: f32,
    pub journey_duration: u64,
    pub halcyon_cooldown: u64,
    pub halcyon_speed: f32,
    pub halcyon_duration: u64,
    pub war_treads_cooldown: u64,
    pub war_treads_speed: f32,
    pub war_treads_duration: u64,
    pub war_treads_range: f32,
    pub teleport_cooldown: u64,
    pub teleport_range: f32,
    pub teleport_channel: u64,
    pub teleport_barrier: f32,
    pub teleport_arrival_speed: f32,
    pub teleport_arrival_duration: u64,
    pub capacitor_heal_amp: f32,
    pub capacitor_barrier_amp: f32,
    pub capacitor_ms: f32,
    pub capacitor_ms_duration: u64,
    pub capacitor_lockout: u64,
    pub protector_barrier: f32,
    pub protector_duration: u64,
    pub protector_cooldown: u64,
    pub rooks_barrier: f32,
    pub rooks_bonus_hp_ratio: f32,
    pub rooks_duration: u64,
    pub rooks_cooldown: u64,
    pub contract_range: f32,
    pub dragonblood_slow_melee: f32,
    pub dragonblood_slow_ranged: f32,
    pub dragonblood_duration: u64,
    pub dragonblood_cooldown: u64,
    pub shiversteel_slow_melee: f32,
    pub shiversteel_slow_ranged: f32,
    pub shiversteel_duration: u64,
    pub shiversteel_active_cooldown: u64,
    pub shiversteel_active_range: f32,
    pub flare_cooldown: u64,
    pub flare_range: f32,
    pub flare_duration: u64,
    pub flare_sight_radius: f32,
    pub flare_loader_charges: u8,
    pub flare_loader_recharge: u64,
    pub flare_loader_range: f32,
    pub flare_loader_duration: u64,
    pub flaregun_charges: u8,
    pub flaregun_recharge: u64,
    pub flaregun_cast_cd: u64,
    pub flaregun_range: f32,
    pub flaregun_duration: u64,
    pub contraption_charges: u8,
    pub contraption_recharge: u64,
    pub contraption_cast_cd: u64,
    pub contraption_splash: f32,
    pub scout_pak_bonus_charges: u8,
    pub scout_pak_cd_reduction: u64,
    pub scout_tuff_bonus_hits: u8,
    pub scout_tuff_bonus_lifespan: u64,
    pub super_scout_bonus_charges: u8,
    pub super_scout_cd_reduction: u64,
    pub super_scout_bonus_hits: u8,
    pub super_scout_bonus_lifespan: u64,
    pub trap_damage_base: f32,
    pub trap_damage_per_level: f32,
}

impl Default for ActiveRules {
    fn default() -> Self {
        Self {
            reflex_low: 100.,
            reflex_high: 600.,
            reflex_duration: 30,
            reflex_cooldown: 90 * TICK_HZ,
            aegis_cooldown: 45 * TICK_HZ,
            crucible_cooldown: 75 * TICK_HZ,
            crucible_range: 12.,
            fountain_cooldown: 75 * TICK_HZ,
            fountain_heal_per_pct: 2.5,
            fountain_heal_ticks: 3 * TICK_HZ,
            fountain_range: 12.,
            fountain_travel_ticks: TICK_HZ / 2,
            atlas_cooldown: 45 * TICK_HZ,
            atlas_as_slow: 0.5,
            atlas_wp_reduction: 0.3,
            atlas_duration: 4 * TICK_HZ,
            atlas_range: 4.5,
            atlas_delay: 16,
            boot_travel_bonus: 0.5,
            travel_cooldown: 90 * TICK_HZ,
            travel_speed: 2.0,
            travel_duration: 2 * TICK_HZ,
            journey_cooldown: 60 * TICK_HZ,
            journey_combat_cooldown: 30 * TICK_HZ,
            journey_speed: 2.0,
            journey_duration: 2 * TICK_HZ,
            halcyon_cooldown: 45 * TICK_HZ,
            halcyon_speed: 3.0,
            halcyon_duration: 3 * TICK_HZ,
            war_treads_cooldown: 60 * TICK_HZ,
            war_treads_speed: 3.0,
            war_treads_duration: 3 * TICK_HZ,
            war_treads_range: 12.,
            teleport_cooldown: 150 * TICK_HZ,
            teleport_range: 200.,
            teleport_channel: 50,
            teleport_barrier: 600.,
            teleport_arrival_speed: 2.0,
            teleport_arrival_duration: 2 * TICK_HZ,
            capacitor_heal_amp: 0.15,
            capacitor_barrier_amp: 0.15,
            capacitor_ms: 1.5,
            capacitor_ms_duration: 3 * TICK_HZ,
            capacitor_lockout: 15 * TICK_HZ,
            protector_barrier: 150.,
            protector_duration: 2 * TICK_HZ,
            protector_cooldown: 12 * TICK_HZ,
            rooks_barrier: 150.,
            rooks_bonus_hp_ratio: 0.15,
            rooks_duration: 2 * TICK_HZ,
            rooks_cooldown: 10 * TICK_HZ,
            contract_range: 12.,
            dragonblood_slow_melee: 0.35,
            dragonblood_slow_ranged: 0.20,
            dragonblood_duration: TICK_HZ,
            dragonblood_cooldown: 6 * TICK_HZ,
            shiversteel_slow_melee: 0.35,
            shiversteel_slow_ranged: 0.20,
            shiversteel_duration: TICK_HZ,
            shiversteel_active_cooldown: 18 * TICK_HZ,
            shiversteel_active_range: 4.5,
            flare_cooldown: 5 * TICK_HZ,
            flare_range: 15.,
            flare_duration: 5 * TICK_HZ,
            flare_sight_radius: 10.,
            flare_loader_charges: 3,
            flare_loader_recharge: 75 * TICK_HZ,
            flare_loader_range: 14.,
            flare_loader_duration: 3 * TICK_HZ,
            flaregun_charges: 2,
            flaregun_recharge: 20 * TICK_HZ,
            flaregun_cast_cd: 5 * TICK_HZ,
            flaregun_range: 15.,
            flaregun_duration: 5 * TICK_HZ,
            contraption_charges: 4,
            contraption_recharge: 20 * TICK_HZ,
            contraption_cast_cd: 5 * TICK_HZ,
            contraption_splash: 2.5,
            scout_pak_bonus_charges: 1,
            scout_pak_cd_reduction: 30 * TICK_HZ,
            scout_tuff_bonus_hits: 1,
            scout_tuff_bonus_lifespan: 30 * TICK_HZ,
            super_scout_bonus_charges: 3,
            super_scout_cd_reduction: 60 * TICK_HZ,
            super_scout_bonus_hits: 3,
            super_scout_bonus_lifespan: 60 * TICK_HZ,
            trap_damage_base: 135.,
            trap_damage_per_level: 45.,
        }
    }
}

pub(super) fn opt_ticks(seconds: Option<f32>) -> Result<Option<u64>, String> {
    let Some(s) = seconds else { return Ok(None) };
    if !s.is_finite() || s <= 0. || s > 86400. {
        return Err(format!("invalid item duration seconds: {s}"));
    }
    Ok(Some((s * TICK_HZ as f32).round().max(1.) as u64))
}

pub(super) fn opt_pos(v: Option<f32>) -> Result<Option<f32>, String> {
    let Some(x) = v else { return Ok(None) };
    if !x.is_finite() || x <= 0. {
        return Err(format!("invalid positive item scalar: {x}"));
    }
    Ok(Some(x))
}

pub(super) fn opt_frac(v: Option<f32>) -> Result<Option<f32>, String> {
    let Some(x) = v else { return Ok(None) };
    if !x.is_finite() || x <= 0. || x > 1. {
        return Err(format!("invalid fraction item scalar: {x}"));
    }
    Ok(Some(x))
}

fn opt_u8(v: Option<f32>) -> Result<Option<u8>, String> {
    let Some(x) = v else { return Ok(None) };
    if !x.is_finite() || x.fract() != 0. || !(1.0..=255.0).contains(&x) {
        return Err(format!("invalid item count scalar: {x}"));
    }
    Ok(Some(x as u8))
}

fn opt_act(item: &FullItem, p: ItemActiveParameter) -> Option<f32> {
    item.fixed_active(p)
}

impl ActiveRules {
    pub(super) fn load(catalog: Option<&ItemCatalog>) -> Result<Self, String> {
        let mut r = Self::default();
        let Some(cat) = catalog else { return Ok(r) };

        if let Some(it) = cat.item("Item_ReflexBlock") {
            if let Some(v) = opt_pos(it.passive_value("BARRIER_MIN"))? {
                r.reflex_low = v;
            }
            if let Some(v) = opt_pos(it.passive_value("BARRIER_MAX"))? {
                r.reflex_high = v.max(r.reflex_low);
            }
            if let Some(v) = opt_ticks(it.passive_value("TIME"))? {
                r.reflex_duration = v;
            }
        }
        if let Some(it) = cat.item("Item_Crucible") {
            if let Some(v) = opt_ticks(opt_act(it, ItemActiveParameter::Cooldown))? {
                r.crucible_cooldown = v;
            }
            if let Some(v) = opt_pos(it.passive_value("RANGE"))? {
                r.crucible_range = v;
            }
        }
        if let Some(it) = cat.item("Item_FountainOfRenewal") {
            if let Some(v) = opt_ticks(opt_act(it, ItemActiveParameter::Cooldown))? {
                r.fountain_cooldown = v;
            }
            if let Some(v) = opt_pos(it.passive_value("HEAL_PER_PCT_MISSING_HEALTH"))? {
                r.fountain_heal_per_pct = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("HEAL_TIME"))? {
                r.fountain_heal_ticks = v;
            }
            if let Some(v) = opt_pos(it.passive_value("RANGE"))? {
                r.fountain_range = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("Shot Travel Time"))? {
                r.fountain_travel_ticks = v;
            }
        }
        if let Some(it) = cat.item("Item_AtlasPauldron") {
            if let Some(v) = opt_ticks(opt_act(it, ItemActiveParameter::Cooldown))? {
                r.atlas_cooldown = v;
            }
            if let Some(v) = opt_frac(it.passive_value("AS_SLOW"))? {
                r.atlas_as_slow = v;
            }
            if let Some(v) = opt_frac(it.passive_value("WP_DAMAGE_REDUCTION"))? {
                r.atlas_wp_reduction = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("DURATION"))? {
                r.atlas_duration = v;
            }
            if let Some(v) = opt_pos(it.passive_value("RANGE"))? {
                r.atlas_range = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("DELAY"))? {
                r.atlas_delay = v;
            }
        }
        if let Some(it) = cat.item("Item_TravelBoots") {
            if let Some(v) = opt_ticks(opt_act(it, ItemActiveParameter::Cooldown))? {
                r.travel_cooldown = v;
            }
            if let Some(v) = opt_pos(it.passive_value("TRAVEL"))? {
                r.boot_travel_bonus = v;
            }
            if let Some(v) = opt_pos(it.passive_value("SPRINT"))? {
                r.travel_speed = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("SPRINT_DURATION"))? {
                r.travel_duration = v;
            }
        }
        if let Some(it) = cat.item("Item_JourneyBoots") {
            if let Some(v) = opt_ticks(opt_act(it, ItemActiveParameter::Cooldown))? {
                r.journey_cooldown = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("COMBAT_COOLDOWN"))? {
                r.journey_combat_cooldown = v;
            }
            if let Some(v) = opt_pos(it.passive_value("SPRINT"))? {
                r.journey_speed = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("SPRINT_DURATION"))? {
                r.journey_duration = v;
            }
        }
        if let Some(it) = cat.item("Item_HalcyonChargers") {
            if let Some(v) = opt_ticks(opt_act(it, ItemActiveParameter::Cooldown))? {
                r.halcyon_cooldown = v;
            }
            if let Some(v) = opt_pos(it.passive_value("SPRINT"))? {
                r.halcyon_speed = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("SPRINT_DURATION"))? {
                r.halcyon_duration = v;
            }
        }
        if let Some(it) = cat.item("Item_WarTreads") {
            if let Some(v) = opt_ticks(opt_act(it, ItemActiveParameter::Cooldown))? {
                r.war_treads_cooldown = v;
            }
            if let Some(v) = opt_pos(it.passive_value("SPEED"))? {
                r.war_treads_speed = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("BASE_DURATION"))? {
                r.war_treads_duration = v;
            }
            if let Some(v) = opt_pos(it.passive_value("RANGE"))? {
                r.war_treads_range = v;
            }
        }
        if let Some(it) = cat.item("Item_TeleportBoots") {
            if let Some(v) = opt_ticks(opt_act(it, ItemActiveParameter::Cooldown))? {
                r.teleport_cooldown = v;
            }
            if let Some(v) = opt_pos(opt_act(it, ItemActiveParameter::Range))? {
                r.teleport_range = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("CHANNEL_TIME"))? {
                r.teleport_channel = v;
            }
            if let Some(v) = opt_pos(it.passive_value("TARGET_BARRIER"))? {
                r.teleport_barrier = v;
            }
        }
        if let Some(it) = cat.item("Item_CapacitorPlate") {
            if let Some(v) = opt_frac(it.passive_value("HEAL_AMP"))? {
                r.capacitor_heal_amp = v;
            }
            if let Some(v) = opt_frac(it.passive_value("BARRIER_AMP"))? {
                r.capacitor_barrier_amp = v;
            }
            if let Some(v) = opt_pos(it.passive_value("MSPD_STRENGTH"))? {
                r.capacitor_ms = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("MSPD_DURATION"))? {
                r.capacitor_ms_duration = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("LOCKOUT_DURATION"))? {
                r.capacitor_lockout = v;
            }
        }
        if let Some(it) = cat.item("Item_ProtectorContract") {
            if let Some(v) = opt_pos(it.passive_value("BARRIER_STRENGTH"))? {
                r.protector_barrier = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("DURATION"))? {
                r.protector_duration = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("COOLDOWN"))? {
                r.protector_cooldown = v;
            }
        }
        if let Some(it) = cat.item("Item_RooksDecree") {
            if let Some(v) = opt_pos(it.passive_value("BARRIER_STRENGTH"))? {
                r.rooks_barrier = v;
            }
            if let Some(v) = opt_frac(it.passive_value("BONUS_HEALTH_RATIO"))? {
                r.rooks_bonus_hp_ratio = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("DURATION"))? {
                r.rooks_duration = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("COOLDOWN"))? {
                r.rooks_cooldown = v;
            }
        }
        if let Some(it) = cat.item("Item_DragonbloodContract") {
            if let Some(v) = opt_frac(it.passive_value("MELEE_SLOW"))? {
                r.dragonblood_slow_melee = v;
            }
            if let Some(v) = opt_frac(it.passive_value("RANGED_SLOW"))? {
                r.dragonblood_slow_ranged = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("SLOW_DURATION"))? {
                r.dragonblood_duration = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("COOLDOWN"))? {
                r.dragonblood_cooldown = v;
            }
        }
        if let Some(it) = cat.item("Item_Shiversteel") {
            if let Some(v) = opt_frac(it.passive_value("MELEE_SLOW"))? {
                r.shiversteel_slow_melee = v;
            }
            if let Some(v) = opt_frac(it.passive_value("RANGED_SLOW"))? {
                r.shiversteel_slow_ranged = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("SLOW_DURATION"))? {
                r.shiversteel_duration = v;
            }
        }
        if let Some(it) = cat.item("Item_Flare") {
            if let Some(v) = opt_ticks(opt_act(it, ItemActiveParameter::Cooldown))? {
                r.flare_cooldown = v;
            }
            if let Some(v) = opt_pos(opt_act(it, ItemActiveParameter::Range))? {
                r.flare_range = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("DURATION"))? {
                r.flare_duration = v;
            }
        }
        if let Some(it) = cat.item("Item_FlareLoader") {
            if let Some(v) = opt_u8(opt_act(it, ItemActiveParameter::Charges))? {
                r.flare_loader_charges = v;
            }
            if let Some(v) = opt_ticks(opt_act(it, ItemActiveParameter::Cooldown))? {
                r.flare_loader_recharge = v;
            }
            if let Some(v) = opt_pos(opt_act(it, ItemActiveParameter::Range))? {
                r.flare_loader_range = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("DURATION"))? {
                r.flare_loader_duration = v;
            }
        }
        if let Some(it) = cat.item("Item_Flaregun") {
            if let Some(v) = opt_u8(opt_act(it, ItemActiveParameter::Charges))? {
                r.flaregun_charges = v;
            }
            if let Some(v) = opt_ticks(opt_act(it, ItemActiveParameter::Cooldown))? {
                r.flaregun_recharge = v;
            }
            if let Some(v) = opt_ticks(it.fixed_active_seconds("Cooldown2"))? {
                r.flaregun_cast_cd = v;
            }
            if let Some(v) = opt_pos(opt_act(it, ItemActiveParameter::Range))? {
                r.flaregun_range = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("DURATION"))? {
                r.flaregun_duration = v;
            }
        }
        if let Some(it) = cat.item("Item_Contraption") {
            if let Some(v) = opt_u8(opt_act(it, ItemActiveParameter::Charges))? {
                r.contraption_charges = v;
            }
            if let Some(v) = opt_ticks(opt_act(it, ItemActiveParameter::Cooldown))? {
                r.contraption_recharge = v;
            }
            if let Some(v) = opt_ticks(it.fixed_active_seconds("Cooldown2"))? {
                r.contraption_cast_cd = v;
            }
            if let Some(v) = opt_pos(it.passive_value("SPLASH"))? {
                r.contraption_splash = v;
            }
        }
        if let Some(it) = cat.item("Item_ScoutPak") {
            if let Some(v) = opt_u8(it.passive_value("BONUS_CHARGES"))? {
                r.scout_pak_bonus_charges = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("COOLDOWN"))? {
                r.scout_pak_cd_reduction = v;
            }
        }
        if let Some(it) = cat.item("Item_ScoutTuff") {
            if let Some(v) = opt_u8(it.passive_value("BONUS_HEALTH"))? {
                r.scout_tuff_bonus_hits = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("BONUS_LIFESPAN"))? {
                r.scout_tuff_bonus_lifespan = v;
            }
        }
        if let Some(it) = cat.item("Item_SuperScout2000") {
            if let Some(v) = opt_u8(it.passive_value("BONUS_CHARGES"))? {
                r.super_scout_bonus_charges = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("COOLDOWN"))? {
                r.super_scout_cd_reduction = v;
            }
            if let Some(v) = opt_u8(it.passive_value("BONUS_HEALTH"))? {
                r.super_scout_bonus_hits = v;
            }
            if let Some(v) = opt_ticks(it.passive_value("BONUS_LIFESPAN"))? {
                r.super_scout_bonus_lifespan = v;
            }
        }
        if let Some(it) = cat.item("Item_ScoutTrap") {
            if let Some(v) = opt_pos(it.passive_value("DAMAGE"))? {
                r.trap_damage_base = v;
            }
            if let Some(v) = opt_pos(it.passive_value("DAMAGE_PER_LEVEL"))? {
                r.trap_damage_per_level = v;
            }
        }
        Ok(r)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct ItemEffectRules {
    pub active: ActiveRules,
    pub passive: item_passives::PassiveRules,
}

impl ItemEffectRules {
    pub(super) fn load(catalog: Option<&ItemCatalog>) -> Result<Self, String> {
        Ok(Self {
            active: ActiveRules::load(catalog)?,
            passive: item_passives::PassiveRules::load(catalog)?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub(super) struct FountainHot {
    pub source: u32,
    pub start_tick: u64,
    pub end_tick: u64,
    pub amp: f32,
    pub heal_per_pct_per_tick: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(super) struct TeleportChannel {
    pub target_id: u32,
    pub complete_tick: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub(super) struct PulseweaveBurst {
    pub detonate_tick: u64,
    pub ms_bonus: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub(super) struct SpellfireBurn {
    pub source: u32,
    pub team: u8,
    pub raw_per_tick: f32,
    pub shield_pierce: f32,
    pub next_tick: u64,
    pub remaining: u8,
}

/// Boxed per-entity item runtime state (`None` unless holding T2/T3 stateful items or active debuffs).
#[derive(Clone, Default, Debug, PartialEq, Serialize)]
pub(super) struct ItemRuntimeState {
    // Persistent item cooldowns & pools (survive death)
    pub crucible_ready: u64,
    pub fountain_ready: u64,
    pub atlas_ready: u64,
    pub war_treads_ready: u64,
    pub teleport_ready: u64,
    pub shiversteel_ready: u64,
    pub aftershock_ready: u64,
    pub clockwork_ready: u64,
    pub protector_ready: u64,
    pub rooks_ready: u64,
    pub dragonblood_ready: u64,
    pub stormguard_ready: u64,
    pub slumbering_ready: u64,
    pub seraphic_ready: u64,
    pub seraphic_window_until: u64,
    pub pulseweave_ready: u64,
    pub pulseweave_next_burn_tick: u64,
    pub flare_cast_ready: u64,
    pub flare_spent: u8,
    pub flare_recharge_at: u64,
    pub trap_ready: u64,
    pub serpent_initialized: bool,
    pub serpent_pool: f32,
    pub eve_initialized: bool,
    pub eve_pool: f32,
    pub minions_foot_ready: bool,
    pub capacitor_lockouts: Vec<(u32, u64)>,

    // Combat & stacking trackers (cleared or reset on death as needed)
    pub last_hero_combat_tick: Option<u64>,
    pub last_enemy_hero_damage_taken_tick: Option<u64>,
    pub bp_stacks: u8,
    pub bp_applied_wp: f32,
    pub bp_accum: f32,
    pub bp_last_stack_tick: u64,
    pub bp_decay_accum: f32,
    pub de_stacks: u8,
    pub de_applied_cp: f32,
    pub de_last_damage_tick: u64,
    pub de_last_stack_tick: u64,
    pub de_decay_accum: f32,
    pub shiv_hits: u8,
    pub aftershock_armed_until: u64,
    pub contract_armed_until: u64,
    pub slumbering_window_hits: Vec<(u64, f32)>,

    // Timed buffs, debuffs, and channels (cleared on death)
    pub pending_atlas_tick: Option<u64>,
    pub teleport_channel: Option<TeleportChannel>,
    pub fountain_hot: Option<FountainHot>,
    pub capacitor_ms_until: u64,
    pub capacitor_ms_bonus: f32,
    pub pulseweave_burst: Option<PulseweaveBurst>,
    pub tornado_ms_until: u64,
    pub tornado_ms_bonus: f32,
    pub atlas_debuff_until: u64,
    pub atlas_as_slow: f32,
    pub atlas_wp_reduction: f32,
    pub mortal_wounds_until: u64,
    pub spellfire_burn: Option<SpellfireBurn>,
    pub armor_shred_stacks: u8,
    pub armor_shred_until: u64,
    pub armor_shred_per_stack: f32,
    pub slumbering_fortify_until: u64,
    pub slumbering_fortified: f32,
}

impl Entity {
    pub(super) fn has_item(&self, kind: BasicItem) -> bool {
        self.inventory.iter().any(|item| item.kind == kind)
    }

    pub(super) fn has_travel_boots(&self) -> bool {
        self.inventory.iter().any(|item| {
            matches!(
                item.kind,
                BasicItem::TravelBoots
                    | BasicItem::JourneyBoots
                    | BasicItem::HalcyonChargers
                    | BasicItem::WarTreads
                    | BasicItem::TeleportBoots
            )
        })
    }

    pub(super) fn rt(&self) -> Option<&ItemRuntimeState> {
        self.item_runtime.as_deref()
    }

    pub(super) fn rt_mut(&mut self) -> &mut ItemRuntimeState {
        self.item_runtime
            .get_or_insert_with(|| Box::new(ItemRuntimeState::default()))
    }

    pub(super) fn item_needs_runtime(kind: BasicItem) -> bool {
        use BasicItem::*;
        matches!(
            kind,
            TravelBoots
                | JourneyBoots
                | HalcyonChargers
                | WarTreads
                | TeleportBoots
                | Aegis
                | Crucible
                | FountainOfRenewal
                | AtlasPauldron
                | CapacitorPlate
                | ProtectorContract
                | RooksDecree
                | DragonbloodContract
                | Shiversteel
                | Flare
                | Flaregun
                | FlareLoader
                | Contraption
                | ScoutTuff
                | SuperScout2000
                | BreakingPoint
                | DragonsEye
                | Aftershock
                | Spellfire
                | Frostburn
                | PoisonedShiv
                | Clockwork
                | EveOfHarvest
                | SerpentMask
                | SlumberingHusk
                | SeraphicShell
                | Spellsword
                | Stormcrown
                | StormguardBanner
                | Bonesaw
                | Pulseweave
                | TornadoTrigger
                | Lifespring
                | MinionsFoot
                | MetalJacket
        )
    }
}

pub(super) fn level_lerp(low: f32, high: f32, level: u8) -> f32 {
    low + f32::from(level.clamp(1, 12) - 1) * ((high - low) / 11.)
}

pub(super) fn extra_move_speed(e: &Entity, shop: &shop::Shop, tick: u64) -> f32 {
    let mut bonus = 0.;
    if e.has_travel_boots()
        && e.rt().is_none_or(|rt| {
            rt.last_enemy_hero_damage_taken_tick
                .is_none_or(|hit| tick >= hit + TRAVEL_BOOTS_COMBAT_WINDOW)
        })
    {
        bonus += shop.effect_rules.active.boot_travel_bonus;
    }
    if let Some(rt) = e.rt() {
        if tick < rt.capacitor_ms_until {
            bonus += rt.capacitor_ms_bonus;
        }
        if let Some(burst) = rt.pulseweave_burst
            && tick < burst.detonate_tick
        {
            bonus += burst.ms_bonus;
        }
        if tick < rt.tornado_ms_until {
            bonus += rt.tornado_ms_bonus;
        }
    }
    bonus
}

pub(super) fn atlas_attack_speed_multiplier(e: &Entity, tick: u64) -> f32 {
    e.rt()
        .filter(|rt| tick < rt.atlas_debuff_until)
        .map_or(1., |rt| (1. - rt.atlas_as_slow).clamp(0.1, 1.))
}

pub(super) fn atlas_weapon_multiplier(e: &Entity, tick: u64) -> f32 {
    e.rt()
        .filter(|rt| tick < rt.atlas_debuff_until)
        .map_or(1., |rt| (1. - rt.atlas_wp_reduction).clamp(0.1, 1.))
}

pub(super) fn mortal_wounds_multiplier(e: &Entity, tick: u64) -> f32 {
    if e.rt().is_some_and(|rt| tick < rt.mortal_wounds_until) {
        MORTAL_WOUNDS_MULTIPLIER
    } else {
        1.
    }
}

pub(super) fn armor_with_shred(target: &Entity, tick: u64) -> f32 {
    let base = target.stats.armor;
    let Some(rt) = target.rt() else {
        return base;
    };
    if rt.armor_shred_stacks == 0 || tick >= rt.armor_shred_until {
        return base;
    }
    let mult = (1. - f32::from(rt.armor_shred_stacks) * rt.armor_shred_per_stack).max(0.);
    (base * mult).max(0.)
}

pub(super) fn clear_ephemeral_on_death(e: &mut Entity) {
    let Some(rt) = e.item_runtime.as_deref_mut() else {
        return;
    };
    if rt.bp_applied_wp != 0. {
        e.stats.weapon = (e.stats.weapon - rt.bp_applied_wp).max(0.);
        e.item_stats.bonus_weapon = (e.item_stats.bonus_weapon - rt.bp_applied_wp).max(0.);
        rt.bp_applied_wp = 0.;
    }
    rt.bp_stacks = 0;
    rt.bp_accum = 0.;
    rt.bp_decay_accum = 0.;
    if rt.de_applied_cp != 0. {
        e.item_stats.crystal = (e.item_stats.crystal - rt.de_applied_cp).max(0.);
        rt.de_applied_cp = 0.;
    }
    rt.de_stacks = 0;
    rt.de_decay_accum = 0.;
    rt.aftershock_armed_until = 0;
    rt.contract_armed_until = 0;
    rt.slumbering_window_hits.clear();
    rt.pending_atlas_tick = None;
    rt.teleport_channel = None;
    rt.fountain_hot = None;
    rt.capacitor_ms_until = 0;
    rt.capacitor_ms_bonus = 0.;
    rt.pulseweave_burst = None;
    rt.tornado_ms_until = 0;
    rt.tornado_ms_bonus = 0.;
    rt.atlas_debuff_until = 0;
    rt.mortal_wounds_until = 0;
    rt.spellfire_burn = None;
    rt.armor_shred_stacks = 0;
    rt.armor_shred_until = 0;
    rt.slumbering_fortify_until = 0;
    rt.slumbering_fortified = 0.;
    e.status.spell_shield_up = false;
}

pub(super) fn cancel_teleport_channel(e: &mut Entity) {
    if let Some(rt) = e.item_runtime.as_deref_mut() {
        rt.teleport_channel = None;
    }
}

pub(super) fn apply_slow(target: &mut Entity, tick: u64, strength: f32, duration: u64) {
    if target.hp <= 0. || target.status.blocks_movement_impairment(tick) {
        return;
    }
    let factor = (1. - strength).clamp(0.1, 1.);
    if target.status.slow_until > tick {
        target.status.slow_factor = target.status.slow_factor.min(factor);
        target.status.slow_until = target.status.slow_until.max(tick + duration);
    } else {
        target.status.slow_factor = factor;
        target.status.slow_until = tick + duration;
    }
}

fn capacitor_heal_amp(source: &Entity, rules: &ActiveRules) -> f32 {
    if source.has_item(BasicItem::CapacitorPlate) {
        1. + rules.capacitor_heal_amp
    } else {
        1.
    }
}

fn capacitor_barrier_amp(source: &Entity, rules: &ActiveRules) -> f32 {
    if source.has_item(BasicItem::CapacitorPlate) {
        1. + rules.capacitor_barrier_amp
    } else {
        1.
    }
}

pub(super) fn on_heal_or_barrier_applied(
    entities: &mut actors::Actors<'_>,
    src_idx: Option<usize>,
    target_idx: usize,
    tick: u64,
    rules: &ActiveRules,
) {
    let Some(si) = src_idx else {
        return;
    };
    if si == target_idx
        || entities[target_idx].kind != Kind::Hero
        || entities[target_idx].hp <= 0.
        || !entities[si].has_item(BasicItem::CapacitorPlate)
    {
        return;
    }
    let target_id = entities[target_idx].id;
    let lockout_ok = {
        let rt = entities[si].rt_mut();
        match rt
            .capacitor_lockouts
            .binary_search_by_key(&target_id, |(id, _)| *id)
        {
            Ok(pos) => {
                if tick >= rt.capacitor_lockouts[pos].1 {
                    rt.capacitor_lockouts[pos].1 = tick + rules.capacitor_lockout;
                    true
                } else {
                    false
                }
            }
            Err(pos) => {
                rt.capacitor_lockouts
                    .insert(pos, (target_id, tick + rules.capacitor_lockout));
                true
            }
        }
    };
    if lockout_ok {
        let t_rt = entities[target_idx].rt_mut();
        t_rt.capacitor_ms_bonus = rules.capacitor_ms;
        t_rt.capacitor_ms_until = t_rt
            .capacitor_ms_until
            .max(tick + rules.capacitor_ms_duration);
    }
}

pub(super) fn adjusted_heal_amount(
    entities: &actors::Actors<'_>,
    src: u32,
    target_idx: usize,
    n: f32,
    tick: u64,
    rules: &ActiveRules,
) -> (Option<usize>, f32) {
    let src_idx = entities.iter().position(|e| e.id == src);
    let mut amount = n;
    if let Some(si) = src_idx {
        amount *= capacitor_heal_amp(&entities[si], rules);
    }
    amount *= mortal_wounds_multiplier(&entities[target_idx], tick);
    (src_idx, amount)
}

pub(super) fn grant_barrier(
    entities: &mut actors::Actors<'_>,
    src_idx: usize,
    target_idx: usize,
    base_amount: f32,
    duration: u64,
    tick: u64,
    rules: &ActiveRules,
) {
    if entities[target_idx].hp <= 0. || base_amount <= 0. {
        return;
    }
    let amp = capacitor_barrier_amp(&entities[src_idx], rules);
    let mw = mortal_wounds_multiplier(&entities[target_idx], tick);
    let final_amount = base_amount * amp * mw;
    let target = &mut entities[target_idx];
    if target.status.barrier_until > tick {
        target.status.barrier = target.status.barrier.max(final_amount);
        target.status.barrier_until = target.status.barrier_until.max(tick + duration);
    } else {
        target.status.barrier = final_amount;
        target.status.barrier_until = tick + duration;
    }
    on_heal_or_barrier_applied(entities, Some(src_idx), target_idx, tick, rules);
}

fn recharge_flare_charges(rt: &mut ItemRuntimeState, max_charges: u8, recharge: u64, tick: u64) {
    while rt.flare_spent > 0 && tick >= rt.flare_recharge_at {
        rt.flare_spent -= 1;
        rt.flare_recharge_at += recharge;
    }
    if rt.flare_spent > max_charges {
        rt.flare_spent = max_charges;
    }
}

fn apply_sprint_buff(target: &mut Entity, speed: f32, until: u64, tick: u64) {
    if target.item_stats.sprint_until > tick {
        if speed > target.item_stats.sprint_speed || until > target.item_stats.sprint_until {
            target.item_stats.sprint_speed = target.item_stats.sprint_speed.max(speed);
            target.item_stats.sprint_until = target.item_stats.sprint_until.max(until);
        }
    } else {
        target.item_stats.sprint_speed = speed;
        target.item_stats.sprint_until = until;
    }
}

fn allied_hero_indices(entities: &actors::Actors<'_>, caster_idx: usize, range: f32) -> Vec<usize> {
    let origin = entities[caster_idx].position;
    let team = entities[caster_idx].team;
    entities
        .iter()
        .enumerate()
        .filter(|&(_, e)| {
            e.kind == Kind::Hero && e.team == team && e.hp > 0. && origin.dist(e.position) <= range
        })
        .map(|(idx, _)| idx)
        .collect()
}

pub(super) fn arm_on_ability_cast(e: &mut Entity, tick: u64, rules: &ActiveRules) {
    if e.hp <= 0. || e.kind != Kind::Hero {
        return;
    }
    let has_rooks = e.has_item(BasicItem::RooksDecree);
    let has_protector = e.has_item(BasicItem::ProtectorContract);
    if !has_rooks && !has_protector {
        return;
    }
    let ready = {
        let rt = e.rt();
        if has_rooks {
            rt.is_none_or(|r| tick >= r.rooks_ready)
        } else {
            rt.is_none_or(|r| tick >= r.protector_ready)
        }
    };
    if ready {
        e.rt_mut().contract_armed_until = tick + CONTRACT_ARM_WINDOW;
        let _ = rules;
    }
}

pub(super) fn trigger_contract_on_hero_basic(
    entities: &mut actors::Actors<'_>,
    attacker_idx: usize,
    victim_kind: Kind,
    tick: u64,
    rules: &ActiveRules,
) {
    if victim_kind != Kind::Hero {
        return;
    }
    let has_rooks = entities[attacker_idx].has_item(BasicItem::RooksDecree);
    let has_protector = entities[attacker_idx].has_item(BasicItem::ProtectorContract);
    if !has_rooks && !has_protector {
        return;
    }
    let armed = entities[attacker_idx]
        .rt()
        .is_some_and(|rt| tick < rt.contract_armed_until);
    if !armed {
        return;
    }
    let bonus_hp = entities[attacker_idx].bonus_hp.max(0.);
    let (barrier, dur, cd, is_rooks) = if has_rooks {
        (
            rules.rooks_barrier + rules.rooks_bonus_hp_ratio * bonus_hp,
            rules.rooks_duration,
            rules.rooks_cooldown,
            true,
        )
    } else {
        (
            rules.protector_barrier,
            rules.protector_duration,
            rules.protector_cooldown,
            false,
        )
    };
    let ready = {
        let rt = entities[attacker_idx].rt();
        if is_rooks {
            rt.is_none_or(|r| tick >= r.rooks_ready)
        } else {
            rt.is_none_or(|r| tick >= r.protector_ready)
        }
    };
    if !ready {
        return;
    }
    {
        let rt = entities[attacker_idx].rt_mut();
        rt.contract_armed_until = 0;
        if is_rooks {
            rt.rooks_ready = tick + cd;
        } else {
            rt.protector_ready = tick + cd;
        }
    }
    for ally in allied_hero_indices(entities, attacker_idx, rules.contract_range) {
        grant_barrier(entities, attacker_idx, ally, barrier, dur, tick, rules);
    }
}

pub(super) fn trigger_slow_on_basic(
    entities: &mut actors::Actors<'_>,
    attacker_idx: usize,
    target_idx: usize,
    tick: u64,
    rules: &ActiveRules,
) {
    if attacker_idx == target_idx
        || entities[target_idx].hp <= 0.
        || !matches!(
            entities[target_idx].kind,
            Kind::Hero | Kind::Neutral | Kind::Minion
        )
    {
        return;
    }
    let ranged = entities[attacker_idx].stats.range >= 4.;
    if entities[attacker_idx].has_item(BasicItem::Shiversteel) {
        let slow = if ranged {
            rules.shiversteel_slow_ranged
        } else {
            rules.shiversteel_slow_melee
        };
        apply_slow(
            &mut entities[target_idx],
            tick,
            slow,
            rules.shiversteel_duration,
        );
        return;
    }
    if entities[attacker_idx].has_item(BasicItem::DragonbloodContract) {
        let ready = entities[attacker_idx]
            .rt()
            .is_none_or(|rt| tick >= rt.dragonblood_ready);
        if ready {
            entities[attacker_idx].rt_mut().dragonblood_ready = tick + rules.dragonblood_cooldown;
            let slow = if ranged {
                rules.dragonblood_slow_ranged
            } else {
                rules.dragonblood_slow_melee
            };
            apply_slow(
                &mut entities[target_idx],
                tick,
                slow,
                rules.dragonblood_duration,
            );
        }
    }
}

pub(super) fn advance_active_effects(room: &mut Room<'_>, events: &mut Vec<Event>) {
    let tick = room.state.tick;
    let rules = room.state.shop.effect_rules.active;
    let n = room.entities.len();

    // 1. Tick Fountain HoTs, Atlas delayed detonations, Teleport Boots channels, and Flare charges.
    for i in 0..n {
        if room.entities[i].kind != Kind::Hero && room.entities[i].item_runtime.is_none() {
            continue;
        }
        let Some(rt) = room.entities[i].item_runtime.as_deref_mut() else {
            continue;
        };

        // Fountain HoT tick.
        let fountain = rt.fountain_hot;
        if let Some(hot) = fountain {
            if tick >= hot.end_tick || room.entities[i].hp <= 0. {
                if let Some(r) = room.entities[i].item_runtime.as_deref_mut() {
                    r.fountain_hot = None;
                }
            } else if tick >= hot.start_tick {
                let e = &room.entities[i];
                let missing_pct = ((e.max_hp - e.hp).max(0.) / e.max_hp.max(1.)) * 100.;
                let mw = mortal_wounds_multiplier(e, tick);
                let step_heal = missing_pct * hot.heal_per_pct_per_tick * hot.amp * mw;
                if step_heal > 0. && !room.entities[i].is_rebooting() {
                    let e = &mut room.entities[i];
                    let before = e.hp;
                    e.hp = (e.hp + step_heal).min(e.max_hp);
                    if e.hp > before {
                        events.push(Event::Heal {
                            source: hot.source,
                            target: e.id,
                            amount: e.hp - before,
                            hp: e.hp,
                        });
                    }
                }
            }
        }

        // Atlas Pauldron delayed AoE detonation.
        let pending_atlas = room.entities[i]
            .rt()
            .and_then(|r| r.pending_atlas_tick)
            .filter(|&due| tick >= due);
        if pending_atlas.is_some() {
            if let Some(r) = room.entities[i].item_runtime.as_deref_mut() {
                r.pending_atlas_tick = None;
            }
            if room.entities[i].hp > 0. {
                let origin = room.entities[i].position;
                let team = room.entities[i].team;
                for j in 0..n {
                    let victim = &mut room.entities[j];
                    if victim.kind != Kind::Hero
                        || victim.team == team
                        || victim.hp <= 0.
                        || origin.dist(victim.position) > rules.atlas_range
                        || victim.status.blocks_debuffs(tick)
                    {
                        continue;
                    }
                    let rt = victim.rt_mut();
                    rt.atlas_debuff_until = rt.atlas_debuff_until.max(tick + rules.atlas_duration);
                    rt.atlas_as_slow = rules.atlas_as_slow;
                    rt.atlas_wp_reduction = rules.atlas_wp_reduction;
                }
            }
        }

        // Teleport Boots channel interruption / completion.
        let channel = room.entities[i].rt().and_then(|r| r.teleport_channel);
        if let Some(ch) = channel {
            let hero = &room.entities[i];
            if hero.hp <= 0.
                || hero.status.stun_until > tick
                || hero.status.silence_until > tick
                || hero.is_rebooting()
            {
                cancel_teleport_channel(&mut room.entities[i]);
            } else if tick >= ch.complete_tick {
                cancel_teleport_channel(&mut room.entities[i]);
                let dest = room
                    .index(ch.target_id)
                    .filter(|&j| room.entities[j].hp > 0.)
                    .map(|j| room.entities[j].position)
                    .or_else(|| {
                        room.vision_devices()
                            .find(|v| v.id == ch.target_id && v.team == room.entities[i].team)
                            .map(|v| v.point)
                    });
                if let Some(dest) = dest {
                    let id = room.entities[i].id;
                    let hero = &mut room.entities[i];
                    hero.position = dest;
                    hero.path = Default::default();
                    hero.order = Command::Stop;
                    apply_sprint_buff(
                        hero,
                        rules.teleport_arrival_speed,
                        tick + rules.teleport_arrival_duration,
                        tick,
                    );
                    if let Some(map) = &mut room.state.native_navigation {
                        map.forget_route(id);
                    }
                }
            }
        }
    }
}

impl Room<'_> {
    pub(super) fn activate_item(
        &mut self,
        i: usize,
        instance: u32,
        target: ItemTarget,
    ) -> Result<(), &'static str> {
        let result = self.activate_item_inner(i, instance, target);
        if result.is_ok() {
            self.interrupt_recall_from_command(i);
        }
        result
    }

    fn activate_item_inner(
        &mut self,
        i: usize,
        instance: u32,
        target: ItemTarget,
    ) -> Result<(), &'static str> {
        let hero = &self.entities[i];
        if hero.hp <= 0. || hero.kind != Kind::Hero || hero.is_rebooting() {
            return Err("actor cannot activate item");
        }
        if hero.status.silence_until > self.tick || hero.status.stun_until > self.tick {
            return Err("actor disabled");
        }
        let actor_id = hero.id;
        // Starter Vision Totem / Healing Flask may live outside store `inventory`.
        if instance == AralStarter::HealingFlask.grant().instance {
            return self.activate_healing_flask(actor_id, instance);
        }
        if instance == AralStarter::VisionTotem.grant().instance {
            let point = match target {
                ItemTarget::Point(p) => p,
                ItemTarget::SelfCast => hero.position,
                ItemTarget::Entity(id) => self
                    .index(id)
                    .map(|j| self.entities[j].position)
                    .ok_or("invalid totem target")?,
            };
            return self.deploy_device(i, instance, Some(point));
        }

        let held = *hero
            .inventory
            .iter()
            .find(|item| item.instance == instance)
            .ok_or("item not held")?;
        let tick = self.state.tick;
        let rules = self.state.shop.effect_rules.active;

        match held.kind {
            BasicItem::ReflexBlock => self.activate_reflex(i, instance),
            BasicItem::SprintBoots => self.activate_sprint_boots(actor_id, instance),
            BasicItem::WeaponInfusion | BasicItem::CrystalInfusion => {
                self.consume_infusion(i, instance)
            }
            BasicItem::ScoutTrap => self.deploy_device(i, instance, None),
            BasicItem::Aegis => {
                let hero = &mut self.entities[i];
                if hero.item_stats.reflex_ready.is_some_and(|end| tick < end) {
                    return Err("Aegis on cooldown");
                }
                let barrier = level_lerp(rules.reflex_low, rules.reflex_high, hero.level);
                let first_effect = self.next_id;
                self.next_id = first_effect.checked_add(3).ok_or("effect instance limit")?;
                let hero = &mut self.entities[i];
                let amp = capacitor_barrier_amp(hero, &rules);
                let mw = mortal_wounds_multiplier(hero, tick);
                let applied = barrier * amp * mw;
                hero.item_stats.reflex_ready = Some(tick + rules.aegis_cooldown);
                hero.status.reflex = Some(reflex::Active {
                    until: tick + rules.reflex_duration,
                    barrier: applied,
                    effect: first_effect + 1,
                });
                let actor_id = hero.id;
                self.state.pending_item_events.push(Event::ReflexActivated {
                    actor: actor_id,
                    item: instance,
                    first_effect,
                    barrier: applied,
                });
                self.state.pending_item_events.push(Event::ItemActivated {
                    actor: actor_id,
                    instance,
                    kind: BasicItem::Aegis,
                    target,
                    cooldown_ticks: rules.aegis_cooldown,
                });
                Ok(())
            }
            BasicItem::Crucible => {
                if self.entities[i]
                    .rt()
                    .is_some_and(|rt| tick < rt.crucible_ready)
                {
                    return Err("Crucible on cooldown");
                }
                let src_id = self.entities[i].id;
                self.entities[i].rt_mut().crucible_ready = tick + rules.crucible_cooldown;
                let amp = capacitor_barrier_amp(&self.entities[i], &rules);
                let allies = allied_hero_indices(&self.entities, i, rules.crucible_range);
                for ally in allies {
                    let mw = mortal_wounds_multiplier(&self.entities[ally], tick);
                    let base = level_lerp(
                        rules.reflex_low,
                        rules.reflex_high,
                        self.entities[ally].level,
                    );
                    let barrier = base * amp * mw;
                    let target_ent = &mut self.entities[ally];
                    let prev_effect = target_ent.status.reflex.map_or(0, |r| r.effect);
                    target_ent.status.reflex = Some(reflex::Active {
                        until: tick + rules.reflex_duration,
                        barrier,
                        effect: prev_effect,
                    });
                    on_heal_or_barrier_applied(&mut self.entities, Some(i), ally, tick, &rules);
                }
                self.state.pending_item_events.push(Event::ItemActivated {
                    actor: src_id,
                    instance,
                    kind: BasicItem::Crucible,
                    target,
                    cooldown_ticks: rules.crucible_cooldown,
                });
                Ok(())
            }
            BasicItem::FountainOfRenewal => {
                if self.entities[i]
                    .rt()
                    .is_some_and(|rt| tick < rt.fountain_ready)
                {
                    return Err("Fountain on cooldown");
                }
                let caster_id = self.entities[i].id;
                let origin = self.entities[i].position;
                self.entities[i].rt_mut().fountain_ready = tick + rules.fountain_cooldown;
                let amp = capacitor_heal_amp(&self.entities[i], &rules);
                let per_pct_per_tick = rules.fountain_heal_per_pct / TICK_HZ as f32;
                let allies = allied_hero_indices(&self.entities, i, rules.fountain_range);
                for ally in allies {
                    let dist = origin.dist(self.entities[ally].position);
                    let delay = ((dist / rules.fountain_range.max(1.)).clamp(0., 1.)
                        * rules.fountain_travel_ticks as f32)
                        .round() as u64;
                    let start_tick = tick + delay;
                    self.entities[ally].rt_mut().fountain_hot = Some(FountainHot {
                        source: caster_id,
                        start_tick,
                        end_tick: start_tick + rules.fountain_heal_ticks,
                        amp,
                        heal_per_pct_per_tick: per_pct_per_tick,
                    });
                    on_heal_or_barrier_applied(&mut self.entities, Some(i), ally, tick, &rules);
                }
                self.state.pending_item_events.push(Event::ItemActivated {
                    actor: caster_id,
                    instance,
                    kind: BasicItem::FountainOfRenewal,
                    target,
                    cooldown_ticks: rules.fountain_cooldown,
                });
                Ok(())
            }
            BasicItem::AtlasPauldron => {
                if self.entities[i]
                    .rt()
                    .is_some_and(|rt| tick < rt.atlas_ready)
                {
                    return Err("Atlas Pauldron on cooldown");
                }
                let src_id = self.entities[i].id;
                let rt = self.entities[i].rt_mut();
                rt.atlas_ready = tick + rules.atlas_cooldown;
                rt.pending_atlas_tick = Some(tick + rules.atlas_delay);
                self.state.pending_item_events.push(Event::ItemActivated {
                    actor: src_id,
                    instance,
                    kind: BasicItem::AtlasPauldron,
                    target,
                    cooldown_ticks: rules.atlas_cooldown,
                });
                Ok(())
            }
            BasicItem::TravelBoots | BasicItem::JourneyBoots | BasicItem::HalcyonChargers => {
                let hero = &mut self.entities[i];
                if tick < hero.item_stats.sprint_ready {
                    return Err("boots sprint on cooldown");
                }
                let (cd, speed, dur) = match held.kind {
                    BasicItem::TravelBoots => (
                        rules.travel_cooldown,
                        rules.travel_speed,
                        rules.travel_duration,
                    ),
                    BasicItem::JourneyBoots => (
                        rules.journey_cooldown,
                        rules.journey_speed,
                        rules.journey_duration,
                    ),
                    BasicItem::HalcyonChargers => (
                        rules.halcyon_cooldown,
                        rules.halcyon_speed,
                        rules.halcyon_duration,
                    ),
                    _ => unreachable!(),
                };
                hero.item_stats.sprint_ready = tick + cd;
                apply_sprint_buff(hero, speed, tick + dur, tick);
                let src_id = hero.id;
                self.state.pending_item_events.push(Event::ItemActivated {
                    actor: src_id,
                    instance,
                    kind: held.kind,
                    target,
                    cooldown_ticks: cd,
                });
                Ok(())
            }
            BasicItem::WarTreads => {
                if self.entities[i]
                    .rt()
                    .is_some_and(|rt| tick < rt.war_treads_ready)
                {
                    return Err("War Treads on cooldown");
                }
                let src_id = self.entities[i].id;
                self.entities[i].rt_mut().war_treads_ready = tick + rules.war_treads_cooldown;
                let until = tick + rules.war_treads_duration;
                for ally in allied_hero_indices(&self.entities, i, rules.war_treads_range) {
                    apply_sprint_buff(
                        &mut self.entities[ally],
                        rules.war_treads_speed,
                        until,
                        tick,
                    );
                }
                self.state.pending_item_events.push(Event::ItemActivated {
                    actor: src_id,
                    instance,
                    kind: BasicItem::WarTreads,
                    target,
                    cooldown_ticks: rules.war_treads_cooldown,
                });
                Ok(())
            }
            BasicItem::TeleportBoots => {
                if self.entities[i]
                    .rt()
                    .is_some_and(|rt| tick < rt.teleport_ready)
                {
                    return Err("Teleport Boots on cooldown");
                }
                let origin = self.entities[i].position;
                let team = self.entities[i].team;
                let valid_actor = |e: &Entity| {
                    e.team == team
                        && e.hp > 0.
                        && matches!(
                            e.kind,
                            Kind::Tower | Kind::Armory | Kind::Crystal | Kind::Minion
                        )
                        && origin.dist(e.position) <= rules.teleport_range
                };
                let (target_id, entity_idx) = match target {
                    ItemTarget::Entity(id) => {
                        if let Some(j) = self.index(id).filter(|&j| valid_actor(&self.entities[j]))
                        {
                            (id, Some(j))
                        } else if self.vision_devices().any(|v| {
                            v.id == id
                                && v.team == team
                                && v.kind == VisionDeviceKind::ScoutCam
                                && origin.dist(v.point) <= rules.teleport_range
                        }) {
                            (id, None)
                        } else {
                            return Err("invalid teleport target");
                        }
                    }
                    ItemTarget::Point(p) if p.x.is_finite() && p.y.is_finite() => {
                        let best_actor = self
                            .entities
                            .iter()
                            .enumerate()
                            .filter(|&(_, e)| valid_actor(e) && p.dist(e.position) <= 10.)
                            .min_by(|(_, a), (_, b)| {
                                p.dist(a.position)
                                    .total_cmp(&p.dist(b.position))
                                    .then_with(|| a.id.cmp(&b.id))
                            })
                            .map(|(j, e)| (p.dist(e.position), e.id, Some(j)));
                        let best_cam = self
                            .vision_devices()
                            .filter(|v| {
                                v.team == team
                                    && v.kind == VisionDeviceKind::ScoutCam
                                    && origin.dist(v.point) <= rules.teleport_range
                                    && p.dist(v.point) <= 10.
                            })
                            .min_by(|a, b| {
                                p.dist(a.point)
                                    .total_cmp(&p.dist(b.point))
                                    .then_with(|| a.id.cmp(&b.id))
                            })
                            .map(|v| (p.dist(v.point), v.id, None));
                        match (best_actor, best_cam) {
                            (Some(a), Some(b)) => {
                                if a.0 <= b.0 {
                                    (a.1, a.2)
                                } else {
                                    (b.1, b.2)
                                }
                            }
                            (Some(a), None) => (a.1, a.2),
                            (None, Some(b)) => (b.1, b.2),
                            (None, None) => return Err("no valid teleport target near point"),
                        }
                    }
                    _ => return Err("teleport requires allied structure, minion, or scout cam"),
                };
                if let Some(j) = entity_idx {
                    grant_barrier(
                        &mut self.entities,
                        i,
                        j,
                        rules.teleport_barrier,
                        rules.teleport_channel,
                        tick,
                        &rules,
                    );
                }
                let src_id = self.entities[i].id;
                let hero = &mut self.entities[i];
                hero.order = Command::Stop;
                hero.path = Default::default();
                hero.winding = None;
                let rt = hero.rt_mut();
                rt.teleport_ready = tick + rules.teleport_cooldown;
                rt.teleport_channel = Some(TeleportChannel {
                    target_id,
                    complete_tick: tick + rules.teleport_channel,
                });
                self.state.pending_item_events.push(Event::ItemActivated {
                    actor: src_id,
                    instance,
                    kind: BasicItem::TeleportBoots,
                    target: ItemTarget::Entity(target_id),
                    cooldown_ticks: rules.teleport_cooldown,
                });
                Ok(())
            }
            BasicItem::Shiversteel => {
                if self.entities[i]
                    .rt()
                    .is_some_and(|rt| tick < rt.shiversteel_ready)
                {
                    return Err("Shiversteel active on cooldown");
                }
                let hero = &mut self.entities[i];
                let ranged = hero.stats.range >= 4.;
                let slow = if ranged {
                    rules.shiversteel_slow_ranged
                } else {
                    rules.shiversteel_slow_melee
                };
                let origin = hero.position;
                let team = hero.team;
                let src_id = hero.id;
                hero.rt_mut().shiversteel_ready = tick + rules.shiversteel_active_cooldown;
                for j in 0..self.entities.len() {
                    let ent = &mut self.entities[j];
                    if j == i || ent.team == team || ent.hp <= 0. {
                        continue;
                    }
                    let hit = match target {
                        ItemTarget::Entity(id) => {
                            ent.id == id
                                && origin.dist(ent.position) <= rules.shiversteel_active_range
                        }
                        _ => {
                            ent.kind == Kind::Hero
                                && origin.dist(ent.position) <= rules.shiversteel_active_range
                        }
                    };
                    if hit {
                        apply_slow(ent, tick, slow, rules.shiversteel_duration);
                    }
                }
                self.state.pending_item_events.push(Event::ItemActivated {
                    actor: src_id,
                    instance,
                    kind: BasicItem::Shiversteel,
                    target,
                    cooldown_ticks: rules.shiversteel_active_cooldown,
                });
                Ok(())
            }
            BasicItem::Flare
            | BasicItem::FlareLoader
            | BasicItem::Flaregun
            | BasicItem::ScoutTuff
            | BasicItem::SuperScout2000
            | BasicItem::Contraption => self.activate_vision_item(i, held, target),
            _ => Err("item has no active effect"),
        }
    }

    fn activate_vision_item(
        &mut self,
        i: usize,
        held: HeldItem,
        target: ItemTarget,
    ) -> Result<(), &'static str> {
        let tick = self.state.tick;
        let rules = self.state.shop.effect_rules.active;
        let hero_pos = self.entities[i].position;
        let point = match target {
            ItemTarget::Point(p) => {
                if !p.x.is_finite() || !p.y.is_finite() {
                    return Err("invalid flare point");
                }
                p
            }
            ItemTarget::SelfCast => hero_pos,
            ItemTarget::Entity(id) => self
                .index(id)
                .map(|j| self.entities[j].position)
                .ok_or("unknown flare target")?,
        };

        // Contraption places a ScoutTrap on SelfCast or point within SPLASH radius.
        if held.kind == BasicItem::Contraption
            && (matches!(target, ItemTarget::SelfCast)
                || hero_pos.dist(point) <= rules.contraption_splash)
        {
            return self.deploy_charge_device(i, held, VisionDeviceKind::ScoutTrap, hero_pos);
        }

        let (range, duration, charges, recharge, cast_cd, consume) = match held.kind {
            BasicItem::Flare => (
                rules.flare_range,
                rules.flare_duration,
                1u8,
                0u64,
                rules.flare_cooldown,
                true,
            ),
            BasicItem::FlareLoader | BasicItem::ScoutTuff | BasicItem::SuperScout2000 => (
                rules.flare_loader_range,
                rules.flare_loader_duration,
                rules.flare_loader_charges,
                rules.flare_loader_recharge,
                0,
                false,
            ),
            BasicItem::Flaregun => (
                rules.flaregun_range,
                rules.flaregun_duration,
                rules.flaregun_charges,
                rules.flaregun_recharge,
                rules.flaregun_cast_cd,
                false,
            ),
            BasicItem::Contraption => (
                rules.flaregun_range,
                rules.flaregun_duration,
                rules.contraption_charges,
                rules.contraption_recharge,
                rules.contraption_cast_cd,
                false,
            ),
            _ => return Err("not a flare item"),
        };
        if hero_pos.dist(point) > range {
            return Err("outside flare range");
        }
        if !consume {
            if self.vision_device_ids.len() >= 256 {
                return Err("vision limit");
            }
            {
                let rt = self.entities[i].rt_mut();
                if tick < rt.flare_cast_ready {
                    return Err("flare on cast cooldown");
                }
                recharge_flare_charges(rt, charges, recharge, tick);
                if rt.flare_spent >= charges {
                    return Err("no flare charges");
                }
                if rt.flare_spent == 0 {
                    rt.flare_recharge_at = tick + recharge;
                }
                rt.flare_spent += 1;
                rt.flare_cast_ready = tick + cast_cd;
            }
            self.spawn_flare_device(i, held.instance, point, duration, &rules)?;
            self.state.pending_item_events.push(Event::ItemActivated {
                actor: self.entities[i].id,
                instance: held.instance,
                kind: held.kind,
                target: ItemTarget::Point(point),
                cooldown_ticks: cast_cd.max(recharge),
            });
            Ok(())
        } else {
            if self.vision_device_ids.len() >= 256 {
                return Err("vision limit");
            }
            if self.entities[i]
                .rt()
                .is_some_and(|rt| tick < rt.flare_cast_ready)
            {
                return Err("flare on cooldown");
            }
            let pos = self.entities[i]
                .inventory
                .iter()
                .position(|h| h.instance == held.instance)
                .ok_or("flare not held")?;
            let h = &mut self.entities[i];
            if let Some(aral) = &mut h.aral_inventory {
                let _ = aral.remove_held(held);
            }
            h.inventory.remove(pos);
            h.rt_mut().flare_cast_ready = tick + cast_cd;
            let src_id = h.id;
            self.spawn_flare_device(i, held.instance, point, duration, &rules)?;
            self.state.pending_item_events.push(Event::ItemActivated {
                actor: src_id,
                instance: held.instance,
                kind: held.kind,
                target: ItemTarget::Point(point),
                cooldown_ticks: cast_cd,
            });
            Ok(())
        }
    }

    fn deploy_charge_device(
        &mut self,
        i: usize,
        held: HeldItem,
        kind: VisionDeviceKind,
        point: Position,
    ) -> Result<(), &'static str> {
        let tick = self.state.tick;
        let rules = self.state.shop.effect_rules.active;
        if self.vision_device_ids.len() >= 256 {
            return Err("vision limit");
        }
        {
            let rt = self.entities[i].rt_mut();
            if tick < rt.flare_cast_ready {
                return Err("contraption on cast cooldown");
            }
            recharge_flare_charges(
                rt,
                rules.contraption_charges,
                rules.contraption_recharge,
                tick,
            );
            if rt.flare_spent >= rules.contraption_charges {
                return Err("no contraption charges");
            }
            if rt.flare_spent == 0 {
                rt.flare_recharge_at = tick + rules.contraption_recharge;
            }
            rt.flare_spent += 1;
            rt.flare_cast_ready = tick + rules.contraption_cast_cd;
        }
        let id = self.next_id;
        self.state.next_id = id.checked_add(1).ok_or("entity id exhausted")?;
        let owner = self.entities[i].id;
        let team = self.entities[i].team;
        let device = vision_devices::VisionDevice::new(id, owner, team, kind, point, tick);
        let handle = self.entities.ecs.spawn(device).id();
        self.vision_device_ids.push(handle);
        self.pending_item_events.push(Event::VisionDeployed {
            device,
            instance: held.instance,
        });
        self.state.pending_item_events.push(Event::ItemActivated {
            actor: owner,
            instance: held.instance,
            kind: held.kind,
            target: ItemTarget::SelfCast,
            cooldown_ticks: rules.contraption_cast_cd,
        });
        Ok(())
    }

    fn spawn_flare_device(
        &mut self,
        i: usize,
        instance: u32,
        point: Position,
        duration: u64,
        rules: &ActiveRules,
    ) -> Result<(), &'static str> {
        let id = self.next_id;
        self.state.next_id = id.checked_add(1).ok_or("entity id exhausted")?;
        let tick = self.state.tick;
        let owner = self.entities[i].id;
        let team = self.entities[i].team;
        let expires_at = tick + duration;
        let device = vision_devices::VisionDevice::new(
            id,
            owner,
            team,
            VisionDeviceKind::Flare,
            point,
            tick,
        )
        .with_extras(0, expires_at);
        let handle = self.entities.ecs.spawn(device).id();
        self.vision_device_ids.push(handle);
        // Permanently reveal enemy ScoutTraps and temporarily reveal enemy heroes in Flare radius.
        self.reveal_in_flare(team, point, rules.flare_sight_radius, expires_at);
        self.pending_item_events
            .push(Event::VisionDeployed { device, instance });
        Ok(())
    }
}
