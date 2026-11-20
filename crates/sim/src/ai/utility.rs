//! Layer 4 zero-allocation Utility AI, smooth response curves,
//! 61-hero data-driven Ability Role Classifier, and active-item intent scoring.

use super::super::*;
use super::macro_bt::MacroMode;
use super::threat::ThreatAssessment;
use halcyon_data::{
    AbilityForm, GameData, all_hero_ability_slots,
    combat_content::{Ability, AbilityVariable},
};

/// Clamps `x` into `[0.0, 1.0]`, mapping non-finite inputs safely to `0.0`.
#[inline]
pub fn clamp01(x: f32) -> f32 {
    if x.is_finite() {
        x.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Linear response curve normalized over `[min, max]`.
#[inline]
pub fn linear_curve(x: f32, min: f32, max: f32) -> f32 {
    if (max - min).abs() <= f32::EPSILON {
        return f32::from(x >= max);
    }
    clamp01((x - min) / (max - min))
}

/// Smooth Hermite S-curve rising from `0.0` at `edge0` to `1.0` at `edge1`.
#[inline]
pub fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = linear_curve(x, edge0, edge1);
    t * t * (3.0 - 2.0 * t)
}

/// Smooth inverse Hermite curve falling from `1.0` at `edge0` to `0.0` at `edge1`.
#[inline]
pub fn inv_smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    1.0 - smoothstep(edge0, edge1, x)
}

/// Smooth logistic response curve centered at `midpoint` with steepness `k`.
#[inline]
pub fn logistic_curve(x: f32, midpoint: f32, k: f32) -> f32 {
    if !x.is_finite() {
        return 0.0;
    }
    let exp_arg = (-k * (x - midpoint)).clamp(-24.0, 24.0);
    1.0 / (1.0 + exp_arg.exp())
}

/// Power curve `(clamp01(x))^exponent`.
#[inline]
pub fn power_curve(x: f32, exponent: f32) -> f32 {
    clamp01(x).powf(exponent.max(0.05))
}

/// High-level tactical role inferred from `abilities.json` metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum AbilityRole {
    Engage = 0,
    Poke = 1,
    Peel = 2,
    Burst = 3,
    Heal = 4,
    Barrier = 5,
    Escape = 6,
    CrowdControl = 7,
    Buff = 8,
    ZoneControl = 9,
}

impl AbilityRole {
    pub const ALL: [Self; 10] = [
        Self::Engage,
        Self::Poke,
        Self::Peel,
        Self::Burst,
        Self::Heal,
        Self::Barrier,
        Self::Escape,
        Self::CrowdControl,
        Self::Buff,
        Self::ZoneControl,
    ];

    const fn bit(self) -> u16 {
        1_u16 << (self as u8)
    }
}

/// Compact bitset of all inferred roles carried by an ability.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AbilityRoleFlags(u16);

impl AbilityRoleFlags {
    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn with(self, role: AbilityRole) -> Self {
        Self(self.0 | role.bit())
    }

    pub const fn contains(self, role: AbilityRole) -> bool {
        (self.0 & role.bit()) != 0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// Normalized target selection shape required by `Room::can_cast`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetPattern {
    SelfOnly,
    AllyHeroOrSelf,
    EnemyEntity,
    PointOrDirection,
}

/// Pre-classified ability metadata profile for a hero slot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClassifiedAbility {
    pub slot: crate::AbilitySlot,
    pub form: AbilityForm,
    pub primary_role: AbilityRole,
    pub roles: AbilityRoleFlags,
    pub targeting: TargetPattern,
    pub estimated_range: f32,
    pub estimated_radius: f32,
    pub has_damage: bool,
    pub is_dash_or_leap: bool,
    pub has_hard_cc: bool,
    pub has_execute: bool,
}

impl ClassifiedAbility {
    fn fallback(slot: crate::AbilitySlot, form: AbilityForm, hero_range: f32) -> Self {
        match slot {
            crate::AbilitySlot::A => Self {
                slot,
                form,
                primary_role: AbilityRole::Poke,
                roles: AbilityRoleFlags::empty().with(AbilityRole::Poke),
                targeting: TargetPattern::EnemyEntity,
                estimated_range: (hero_range + 1.5).max(7.0),
                estimated_radius: 1.5,
                has_damage: true,
                is_dash_or_leap: false,
                has_hard_cc: false,
                has_execute: false,
            },
            crate::AbilitySlot::B => Self {
                slot,
                form,
                primary_role: AbilityRole::Buff,
                roles: AbilityRoleFlags::empty().with(AbilityRole::Buff),
                targeting: TargetPattern::SelfOnly,
                estimated_range: hero_range.max(5.5),
                estimated_radius: 3.0,
                has_damage: false,
                is_dash_or_leap: false,
                has_hard_cc: false,
                has_execute: false,
            },
            crate::AbilitySlot::C => Self {
                slot,
                form,
                primary_role: AbilityRole::Burst,
                roles: AbilityRoleFlags::empty().with(AbilityRole::Burst),
                targeting: TargetPattern::EnemyEntity,
                estimated_range: (hero_range + 4.5).max(10.0),
                estimated_radius: 2.5,
                has_damage: true,
                is_dash_or_leap: false,
                has_hard_cc: false,
                has_execute: false,
            },
        }
    }
}

/// Complete classified primary (and optional alternate-form) kit for one hero template.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeroKitIntent {
    pub template: u16,
    pub primary: [ClassifiedAbility; 3],
    pub alternate: Option<[ClassifiedAbility; 3]>,
}

/// Precomputed 61-hero ability intent table, sorted by `EntityTemplate` raw ID for zero-alloc lookup.
#[derive(Clone, Debug, Default)]
pub struct HeroIntentCatalog {
    entries: Vec<(u16, HeroKitIntent)>,
}

impl HeroIntentCatalog {
    /// Builds the 61-hero intent table deterministically from loaded `GameData`.
    pub fn from_data(data: &GameData) -> Self {
        let mut entries = Vec::with_capacity(61);
        for slots in all_hero_ability_slots() {
            let hero_range = data
                .combat_content()
                .units()
                .find(|u| u.entity_type_id == Some(slots.template.as_raw()))
                .map_or(5.5, |u| u.range);
            let classify_slot = |slot: crate::AbilitySlot, form: AbilityForm| {
                let d_slot = to_data_slot(slot);
                data.ability_for_form(slots.template, d_slot, form)
                    .map_or_else(
                        || ClassifiedAbility::fallback(slot, form, hero_range),
                        |ability| classify_ability(ability, slot, form, hero_range),
                    )
            };
            let primary = [
                classify_slot(crate::AbilitySlot::A, AbilityForm::Primary),
                classify_slot(crate::AbilitySlot::B, AbilityForm::Primary),
                classify_slot(crate::AbilitySlot::C, AbilityForm::Primary),
            ];
            let alternate = slots.is_dual_form().then(|| {
                [
                    classify_slot(crate::AbilitySlot::A, AbilityForm::Alternate),
                    classify_slot(crate::AbilitySlot::B, AbilityForm::Alternate),
                    classify_slot(crate::AbilitySlot::C, AbilityForm::Alternate),
                ]
            });
            entries.push((
                slots.template.as_raw(),
                HeroKitIntent {
                    template: slots.template.as_raw(),
                    primary,
                    alternate,
                },
            ));
        }
        entries.sort_unstable_by_key(|(id, _)| *id);
        Self { entries }
    }

    /// Zero-allocation O(log N) lookup for a hero's classified ability kit.
    pub fn kit_for(&self, type_id: u16) -> Option<&HeroKitIntent> {
        self.entries
            .binary_search_by_key(&type_id, |(id, _)| *id)
            .ok()
            .map(|idx| &self.entries[idx].1)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

const fn to_data_slot(slot: crate::AbilitySlot) -> halcyon_data::AbilitySlot {
    match slot {
        crate::AbilitySlot::A => halcyon_data::AbilitySlot::A,
        crate::AbilitySlot::B => halcyon_data::AbilitySlot::B,
        crate::AbilitySlot::C => halcyon_data::AbilitySlot::C,
    }
}

fn contains_ascii_ci(haystack: &str, needle: &str) -> bool {
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || n.len() > h.len() {
        return false;
    }
    h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n))
}

fn first_rank_value(var: &AbilityVariable) -> f32 {
    var.ranks
        .as_deref()
        .and_then(|r| r.first().copied())
        .filter(|v| v.is_finite() && *v > 0.0)
        .unwrap_or(var.base)
}

fn is_heal_variable(name: &str) -> bool {
    if contains_ascii_ci(name, "health_cost")
        || contains_ascii_ci(name, "health cost")
        || contains_ascii_ci(name, "fortified")
        || contains_ascii_ci(name, "fort_health")
        || contains_ascii_ci(name, "health_ratio")
        || contains_ascii_ci(name, "healthratio")
        || contains_ascii_ci(name, "health_threshold")
        || contains_ascii_ci(name, "wolf")
    {
        // Note: `Burst_Heal_Health_Ratio` and `HealPerSecondHealthRatio` also contain "heal"
        // before "health"; permit those explicit heal ratios.
        return contains_ascii_ci(name, "burst_heal") || contains_ascii_ci(name, "healpersecond");
    }
    contains_ascii_ci(name, "heal")
        || contains_ascii_ci(name, "hps")
        || contains_ascii_ci(name, "lifesteal")
}

/// Data-driven ability role classifier driven solely by `halcyon_data::Ability` metadata.
pub fn classify_ability(
    ability: &Ability,
    slot: crate::AbilitySlot,
    form: AbilityForm,
    hero_range: f32,
) -> ClassifiedAbility {
    let mut roles = AbilityRoleFlags::empty();
    let mut estimated_range = 0.0_f32;
    let mut estimated_radius = 0.0_f32;
    let mut has_heal = false;
    let mut has_barrier = false;
    let mut has_hard_cc = false;
    let mut has_slow = false;
    let mut has_knock = false;
    let mut is_dash_or_leap = false;
    let mut is_escape_dash = false;
    let mut has_stealth = false;
    let mut has_zone = false;
    let mut has_buff = false;
    let mut has_execute = false;
    let mut has_damage_var = !ability.damage_variables.is_empty();
    let mut has_projectile = false;

    for v in &ability.variables {
        let name = v.name.as_str();
        if contains_ascii_ci(name, "range")
            && !contains_ascii_ci(name, "notrange")
            && !contains_ascii_ci(name, "basicattackrange")
            && !contains_ascii_ci(name, "attack_range")
            && !contains_ascii_ci(name, "outofrange")
        {
            let val = first_rank_value(v);
            if val > 1.5 && val < 60.0 {
                estimated_range = estimated_range.max(val);
            }
        }
        if contains_ascii_ci(name, "radius") || contains_ascii_ci(name, "aoe") {
            let val = first_rank_value(v);
            if val > 0.2 && val < 40.0 {
                estimated_radius = estimated_radius.max(val);
            }
        }
        if contains_ascii_ci(name, "shot_speed")
            || contains_ascii_ci(name, "shot speed")
            || contains_ascii_ci(name, "shotspeed")
            || contains_ascii_ci(name, "projectile")
        {
            has_projectile = true;
        }
        if is_heal_variable(name) {
            has_heal = true;
        }
        if contains_ascii_ci(name, "barrier")
            || contains_ascii_ci(name, "fortified")
            || contains_ascii_ci(name, "fort_health")
            || contains_ascii_ci(name, "damage_reduction")
            || contains_ascii_ci(name, "damagereduction")
            || contains_ascii_ci(name, "passive_dr")
            || contains_ascii_ci(name, "active_dr")
            || contains_ascii_ci(name, "immunity")
            || contains_ascii_ci(name, "dodge")
            || contains_ascii_ci(name, "submerge")
            || contains_ascii_ci(name, "guard")
        {
            has_barrier = true;
        }
        if contains_ascii_ci(name, "stun")
            || contains_ascii_ci(name, "root")
            || contains_ascii_ci(name, "silence")
            || contains_ascii_ci(name, "taunt")
            || contains_ascii_ci(name, "immobilize")
            || contains_ascii_ci(name, "fear")
            || contains_ascii_ci(name, "sleep")
            || contains_ascii_ci(name, "pull")
            || contains_ascii_ci(name, "tether")
        {
            has_hard_cc = true;
        }
        if contains_ascii_ci(name, "knockback")
            || contains_ascii_ci(name, "knockaway")
            || contains_ascii_ci(name, "knockup")
            || contains_ascii_ci(name, "fling")
        {
            has_hard_cc = true;
            has_knock = true;
        }
        if contains_ascii_ci(name, "slow") && !contains_ascii_ci(name, "selfslow") {
            has_slow = true;
        }
        if contains_ascii_ci(name, "backwards_dash") || contains_ascii_ci(name, "leapback") {
            is_escape_dash = true;
            is_dash_or_leap = true;
        } else if contains_ascii_ci(name, "dash")
            || contains_ascii_ci(name, "leap")
            || contains_ascii_ci(name, "lunge")
            || contains_ascii_ci(name, "blink")
            || contains_ascii_ci(name, "roll distance")
            || contains_ascii_ci(name, "jump_delay")
        {
            is_dash_or_leap = true;
        }
        if contains_ascii_ci(name, "stealth") || contains_ascii_ci(name, "kaku") {
            has_stealth = true;
        }
        if contains_ascii_ci(name, "field")
            || contains_ascii_ci(name, "pool")
            || contains_ascii_ci(name, "trap")
            || contains_ascii_ci(name, "tripwire")
            || contains_ascii_ci(name, "arena")
            || contains_ascii_ci(name, "cloud")
            || contains_ascii_ci(name, "portal")
            || contains_ascii_ci(name, "munion")
            || contains_ascii_ci(name, "seed")
            || contains_ascii_ci(name, "turret")
            || contains_ascii_ci(name, "wolf")
            || contains_ascii_ci(name, "burnradius")
            || contains_ascii_ci(name, "effectradius")
            || contains_ascii_ci(name, "explosionradius")
            || contains_ascii_ci(name, "impact_radius")
            || contains_ascii_ci(name, "damage_radius")
            || contains_ascii_ci(name, "aoe")
        {
            has_zone = true;
        }
        if contains_ascii_ci(name, "aspd")
            || contains_ascii_ci(name, "attack_speed")
            || contains_ascii_ci(name, "attackspeed")
            || contains_ascii_ci(name, "mspd")
            || contains_ascii_ci(name, "speedboost")
            || contains_ascii_ci(name, "speed_boost")
            || contains_ascii_ci(name, "movespeed")
            || contains_ascii_ci(name, "bonus_damage")
            || contains_ascii_ci(name, "bonusdamage")
            || contains_ascii_ci(name, "empower")
            || contains_ascii_ci(name, "stance")
            || contains_ascii_ci(name, "ammo")
            || contains_ascii_ci(name, "cleave")
        {
            has_buff = true;
        }
        if contains_ascii_ci(name, "execute") {
            has_execute = true;
        }
        if contains_ascii_ci(name, "damage") && !contains_ascii_ci(name, "reduction") {
            has_damage_var = true;
        }
    }

    let tt = ability.target_type.as_deref().unwrap_or("");
    let is_buff_ally = tt == "LABEL_ABILITY_TYPE_BUFF_ALLY";
    let is_buff_self = tt == "LABEL_ABILITY_TYPE_BUFF_SELF";
    let is_radius = tt == "LABEL_ABILITY_TYPE_RADIUS";
    let is_targeted = tt == "LABEL_ABILITY_TYPE_TARGETED";
    let is_untargeted = tt == "LABEL_ABILITY_TYPE_UNTARGETED";
    let is_directional =
        tt == "LABEL_ABILITY_TYPE_DIRECTIONAL" || tt == "LABEL_ABILITY_TYPE_CENTER_ON_TARGET";

    if has_heal {
        roles = roles.with(AbilityRole::Heal);
    }
    if has_barrier {
        roles = roles.with(AbilityRole::Barrier);
    }
    if has_hard_cc || has_slow {
        roles = roles.with(AbilityRole::CrowdControl);
    }
    if has_knock || (is_buff_ally && (has_heal || has_barrier)) || (is_radius && has_hard_cc) {
        roles = roles.with(AbilityRole::Peel);
    }
    if has_stealth || is_escape_dash || (has_barrier && has_buff && !has_damage_var) {
        roles = roles.with(AbilityRole::Escape);
    }
    if is_dash_or_leap && (has_damage_var || has_hard_cc || is_targeted || is_directional) {
        roles = roles.with(AbilityRole::Engage);
    }
    if has_zone || (is_radius && !is_buff_self && estimated_radius >= 2.0) {
        roles = roles.with(AbilityRole::ZoneControl);
    }
    if has_buff || is_buff_self {
        roles = roles.with(AbilityRole::Buff);
    }
    if has_execute || (slot == crate::AbilitySlot::C && has_damage_var) {
        roles = roles.with(AbilityRole::Burst);
    }
    if has_damage_var && (is_directional || is_targeted || has_projectile || estimated_range >= 5.5)
    {
        if estimated_range >= 6.0 || has_projectile || (is_directional && !is_dash_or_leap) {
            roles = roles.with(AbilityRole::Poke);
        }
        roles = roles.with(AbilityRole::Burst);
    }

    if roles.is_empty() {
        let fallback = ClassifiedAbility::fallback(slot, form, hero_range);
        roles = fallback.roles;
    }

    let primary_role =
        if has_heal && (is_buff_ally || !has_damage_var || slot != crate::AbilitySlot::C) {
            AbilityRole::Heal
        } else if has_barrier && (is_buff_ally || is_buff_self || !has_damage_var) {
            AbilityRole::Barrier
        } else if has_stealth || is_escape_dash {
            AbilityRole::Escape
        } else if has_hard_cc && (has_knock || is_radius || is_buff_self) {
            AbilityRole::Peel
        } else if is_dash_or_leap && has_damage_var {
            AbilityRole::Engage
        } else if has_hard_cc {
            AbilityRole::CrowdControl
        } else if has_execute || (slot == crate::AbilitySlot::C && has_damage_var) {
            AbilityRole::Burst
        } else if has_zone && !is_dash_or_leap && !has_projectile {
            AbilityRole::ZoneControl
        } else if roles.contains(AbilityRole::Poke) {
            AbilityRole::Poke
        } else if has_buff || is_buff_self {
            AbilityRole::Buff
        } else {
            AbilityRole::Burst
        };

    let targeting = if (is_buff_ally && (has_heal || has_barrier || has_buff))
        || (is_targeted && has_heal && !has_damage_var)
    {
        TargetPattern::AllyHeroOrSelf
    } else if is_buff_self || is_untargeted || (is_radius && estimated_range <= 0.1) {
        TargetPattern::SelfOnly
    } else if is_targeted {
        TargetPattern::EnemyEntity
    } else if is_directional || is_radius || estimated_range > 1.5 {
        TargetPattern::PointOrDirection
    } else {
        TargetPattern::SelfOnly
    };

    if estimated_range <= 0.1 {
        estimated_range = match targeting {
            TargetPattern::SelfOnly => estimated_radius.max(hero_range).max(3.5),
            TargetPattern::AllyHeroOrSelf => 7.0,
            TargetPattern::EnemyEntity => hero_range.max(5.5),
            TargetPattern::PointOrDirection => (hero_range + 2.5).max(8.0),
        };
    }
    if estimated_radius <= 0.1 {
        estimated_radius = 2.5;
    }

    ClassifiedAbility {
        slot,
        form,
        primary_role,
        roles,
        targeting,
        estimated_range,
        estimated_radius,
        has_damage: has_damage_var,
        is_dash_or_leap,
        has_hard_cc,
        has_execute,
    }
}

/// Stack-only situational input passed to `score_classified_ability`.
#[derive(Clone, Copy, Debug)]
pub struct UtilityContext {
    pub hero_hp_ratio: f32,
    pub hero_energy_ratio: f32,
    pub hero_attack_range: f32,
    pub macro_mode: MacroMode,
    pub threat: ThreatAssessment,
    pub enemy_kind: Kind,
    pub enemy_dist: f32,
    pub enemy_hp_ratio: f32,
    pub enemy_cluster_count: u8,
    pub lowest_ally_hp_ratio: f32,
    pub lowest_ally_dist: f32,
}

/// Evaluates a smooth response-curve utility score in `[0.0, 1.0]` for casting `ability`.
pub fn score_classified_ability(ability: &ClassifiedAbility, ctx: &UtilityContext) -> f32 {
    if ctx.enemy_dist > ability.estimated_range + 1.5
        && !ability.roles.contains(AbilityRole::Heal)
        && !ability.roles.contains(AbilityRole::Barrier)
        && !ability.roles.contains(AbilityRole::Escape)
    {
        return 0.0;
    }

    let missing_hp = 1.0 - clamp01(ctx.hero_hp_ratio);
    let ally_missing_hp = 1.0 - clamp01(ctx.lowest_ally_hp_ratio);
    let enemy_missing_hp = 1.0 - clamp01(ctx.enemy_hp_ratio);
    let is_hero_target = ctx.enemy_kind == Kind::Hero;
    let is_objective = matches!(ctx.enemy_kind, Kind::Neutral);
    let cluster_bonus = linear_curve(f32::from(ctx.enemy_cluster_count), 1.0, 3.0) * 0.22;
    let threat_pressure = clamp01(ctx.threat.total_threat);

    let mut score = match ability.primary_role {
        AbilityRole::Heal => {
            let need = ally_missing_hp.max(missing_hp);
            if ctx.lowest_ally_dist <= ability.estimated_range + 1.0 {
                smoothstep(0.12, 0.72, need) * (0.78 + 0.22 * threat_pressure)
            } else {
                smoothstep(0.20, 0.80, missing_hp) * 0.85
            }
        }
        AbilityRole::Barrier => {
            let need = if ability.targeting == TargetPattern::AllyHeroOrSelf {
                ally_missing_hp.max(missing_hp)
            } else {
                missing_hp
            };
            let combat = if is_hero_target { 0.45 } else { 0.15 };
            clamp01(0.45 * threat_pressure + 0.40 * smoothstep(0.10, 0.75, need) + combat * 0.45)
        }
        AbilityRole::Escape => {
            if ctx.macro_mode == MacroMode::Retreat {
                0.70 + 0.25 * smoothstep(0.25, 0.90, threat_pressure)
            } else if threat_pressure > 0.65 && ctx.hero_hp_ratio < 0.45 {
                0.65
            } else if is_hero_target && ctx.enemy_dist <= ability.estimated_range {
                0.22
            } else {
                0.05
            }
        }
        AbilityRole::Peel => {
            let enemy_close = inv_smoothstep(2.0, 6.5, ctx.enemy_dist);
            let rescue = smoothstep(0.15, 0.65, ally_missing_hp.max(missing_hp));
            let base = if is_hero_target { 0.48 } else { 0.18 };
            clamp01(base + 0.30 * enemy_close + 0.24 * rescue + cluster_bonus)
        }
        AbilityRole::Engage => {
            if ctx.macro_mode == MacroMode::Retreat
                || (ctx.threat.in_enemy_turret_range && !ctx.threat.allied_minion_tanking_turret)
            {
                0.05
            } else if is_hero_target {
                let health_ok = smoothstep(0.25, 0.65, ctx.hero_hp_ratio);
                let gap = linear_curve(
                    ctx.enemy_dist,
                    ctx.hero_attack_range * 0.6,
                    ability.estimated_range,
                );
                let kill_drive = smoothstep(0.35, 0.90, enemy_missing_hp) * 0.25;
                clamp01((0.46 + 0.30 * gap + kill_drive) * health_ok + cluster_bonus * 0.5)
            } else {
                0.22
            }
        }
        AbilityRole::CrowdControl => {
            if is_hero_target {
                let range_fit = inv_smoothstep(
                    ability.estimated_range * 0.7,
                    ability.estimated_range + 1.0,
                    ctx.enemy_dist,
                );
                clamp01(0.55 + 0.22 * range_fit + 0.15 * enemy_missing_hp + cluster_bonus)
            } else {
                0.15
            }
        }
        AbilityRole::Burst => {
            if is_hero_target {
                let execute_drive = if ability.has_execute {
                    logistic_curve(enemy_missing_hp, 0.58, 7.0) * 0.40
                } else {
                    power_curve(enemy_missing_hp, 1.35) * 0.28
                };
                clamp01(0.54 + execute_drive + cluster_bonus)
            } else if is_objective {
                0.52
            } else if ability.slot == crate::AbilitySlot::C {
                0.08
            } else {
                0.26
            }
        }
        AbilityRole::Poke => {
            let energy_ok = smoothstep(0.12, 0.45, ctx.hero_energy_ratio);
            let standoff = linear_curve(
                ctx.enemy_dist,
                ctx.hero_attack_range * 0.5,
                ability.estimated_range,
            );
            if is_hero_target {
                clamp01(
                    (0.52 + 0.25 * standoff + 0.15 * enemy_missing_hp) * (0.45 + 0.55 * energy_ok),
                )
            } else if is_objective {
                0.44 * energy_ok
            } else {
                0.22 * energy_ok
            }
        }
        AbilityRole::ZoneControl => {
            if is_hero_target {
                clamp01(0.50 + cluster_bonus + 0.15 * threat_pressure)
            } else if is_objective {
                0.46
            } else {
                0.20
            }
        }
        AbilityRole::Buff => {
            if is_hero_target && ctx.enemy_dist <= ctx.hero_attack_range + 3.0 {
                0.56 + 0.14 * smoothstep(0.20, 0.80, enemy_missing_hp)
            } else if matches!(ctx.enemy_kind, Kind::Tower | Kind::Crystal | Kind::Neutral)
                && ctx.enemy_dist <= ctx.hero_attack_range + 1.5
            {
                0.44
            } else {
                0.12
            }
        }
    };

    // Preserve ultimate discipline: avoid burning slot C on non-heroes unless desperate/objective.
    if ability.slot == crate::AbilitySlot::C
        && !is_hero_target
        && !is_objective
        && !ability.roles.contains(AbilityRole::Heal)
        && !ability.roles.contains(AbilityRole::Escape)
    {
        score *= 0.25;
    }

    // In Retreat mode, allow Escape/Heal/Barrier/Peel while suppressing pure offensive pokes.
    if ctx.macro_mode == MacroMode::Retreat
        && !ability.roles.contains(AbilityRole::Escape)
        && !ability.roles.contains(AbilityRole::Heal)
        && !ability.roles.contains(AbilityRole::Barrier)
        && !ability.roles.contains(AbilityRole::Peel)
    {
        score *= 0.20;
    }

    clamp01(score)
}

/// Active item tactical intent category.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ActiveItemCategory {
    DefensiveBarrierCleanse,
    TeamHeal,
    SelfHeal,
    SpeedEngageEscape,
    AttackSpeedSlow,
    TargetSlow,
}

/// Scored active-item candidate returned by the Layer 4 active-item utility evaluator.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActiveItemIntent {
    pub item: BasicItem,
    pub instance: u32,
    pub category: ActiveItemCategory,
    pub score: f32,
    pub executable_now: bool,
}

pub fn classify_active_item(item: BasicItem) -> Option<ActiveItemCategory> {
    use BasicItem::*;
    Some(match item {
        ReflexBlock | Aegis | Crucible => ActiveItemCategory::DefensiveBarrierCleanse,
        FountainOfRenewal => ActiveItemCategory::TeamHeal,
        SprintBoots | TravelBoots | JourneyBoots | HalcyonChargers | WarTreads => {
            ActiveItemCategory::SpeedEngageEscape
        }
        AtlasPauldron => ActiveItemCategory::AttackSpeedSlow,
        Shiversteel => ActiveItemCategory::TargetSlow,
        _ => return None,
    })
}

impl Room<'_> {
    /// Looks up the precomputed or fallback classified kit for `hero_template`.
    pub(crate) fn classified_kit_for(&self, hero_template: u16, hero_range: f32) -> HeroKitIntent {
        if let Some(kit) = self.ability_intents.kit_for(hero_template) {
            return *kit;
        }
        HeroKitIntent {
            template: hero_template,
            primary: [
                ClassifiedAbility::fallback(
                    crate::AbilitySlot::A,
                    AbilityForm::Primary,
                    hero_range,
                ),
                ClassifiedAbility::fallback(
                    crate::AbilitySlot::B,
                    AbilityForm::Primary,
                    hero_range,
                ),
                ClassifiedAbility::fallback(
                    crate::AbilitySlot::C,
                    AbilityForm::Primary,
                    hero_range,
                ),
            ],
            alternate: None,
        }
    }

    /// Evaluates active-item utility scores across the hero's inventory and ARAL Healing Flask
    /// using zero heap allocations (returns up to 7 stack-allocated intent records).
    pub fn evaluate_active_item_intents(
        &self,
        caster: usize,
        threat: &ThreatAssessment,
        mode: MacroMode,
        move_goal_dist: f32,
    ) -> [Option<ActiveItemIntent>; 7] {
        let mut out = [None; 7];
        let mut count = 0usize;
        let hero = &self.entities[caster];
        let hp_ratio = (hero.hp / hero.max_hp.max(1.0)).clamp(0.0, 1.0);
        let missing_hp = 1.0 - hp_ratio;
        let ally_missing = 1.0 - threat.lowest_ally_hp_ratio;
        let has_cc_debuff = hero.status.slow_until > self.tick
            || hero.status.bulwark_slow_until > self.tick
            || hero.status.water_slow_until > self.tick
            || hero.status.root_until > self.tick
            || hero.status.silence_until > self.tick;

        // 1. ARAL Healing Flask starter slot.
        if self.flask_available(caster)
            && let Some(instance) = self.flask_instance(caster)
        {
            let flask_score = if hp_ratio < 0.30 {
                0.92
            } else if hp_ratio < 0.50 && threat.total_threat > 0.35 {
                0.72
            } else {
                smoothstep(0.35, 0.85, missing_hp) * 0.70
            };
            out[count] = Some(ActiveItemIntent {
                item: BasicItem::Oakheart, // sentinel for ARAL HealingFlask command path
                instance,
                category: ActiveItemCategory::SelfHeal,
                score: clamp01(flask_score),
                executable_now: true,
            });
            count += 1;
        }

        // 2. Equipped inventory items (max 6 slots).
        for held in hero.inventory.iter().take(6) {
            let Some(category) = classify_active_item(held.kind) else {
                continue;
            };
            let (score, executable_now) = match held.kind {
                BasicItem::ReflexBlock => {
                    let ready = hero
                        .item_stats
                        .reflex_ready
                        .is_none_or(|until| self.tick >= until)
                        && hero.status.silence_until <= self.tick;
                    let s = if !ready || threat.nearby_enemy_heroes == 0 {
                        0.0
                    } else {
                        let danger = smoothstep(0.35, 0.85, threat.total_threat);
                        let urgency = smoothstep(0.20, 0.70, missing_hp);
                        let cc_bonus = if has_cc_debuff { 0.25 } else { 0.0 };
                        clamp01(0.35 * danger + 0.45 * urgency + cc_bonus)
                    };
                    (s, ready)
                }
                BasicItem::Aegis => {
                    let s = if threat.nearby_enemy_heroes == 0 {
                        0.0
                    } else {
                        clamp01(
                            0.40 * smoothstep(0.35, 0.85, threat.total_threat)
                                + 0.45 * smoothstep(0.20, 0.70, missing_hp)
                                + if has_cc_debuff { 0.25 } else { 0.0 },
                        )
                    };
                    (s, false)
                }
                BasicItem::Crucible => {
                    let s = if threat.nearby_enemy_heroes == 0 {
                        0.0
                    } else {
                        let team_need = ally_missing.max(missing_hp);
                        clamp01(
                            0.35 * smoothstep(0.30, 0.85, threat.total_threat)
                                + 0.45 * smoothstep(0.20, 0.75, team_need)
                                + linear_curve(f32::from(threat.nearby_allied_heroes), 0.0, 2.0)
                                    * 0.20,
                        )
                    };
                    (s, false)
                }
                BasicItem::FountainOfRenewal => {
                    let team_need = ally_missing.max(missing_hp);
                    let s = if threat.nearby_enemy_heroes == 0 && team_need < 0.45 {
                        0.0
                    } else {
                        clamp01(
                            smoothstep(0.25, 0.72, team_need)
                                * (0.75
                                    + 0.25
                                        * linear_curve(
                                            f32::from(threat.nearby_allied_heroes),
                                            0.0,
                                            2.0,
                                        )),
                        )
                    };
                    (s, false)
                }
                BasicItem::SprintBoots => {
                    let ready = self.tick >= hero.item_stats.sprint_ready;
                    let s = if !ready {
                        0.0
                    } else if mode == MacroMode::Retreat && threat.nearby_enemy_heroes > 0 {
                        0.85
                    } else if move_goal_dist > 8.0 {
                        0.68
                    } else if mode == MacroMode::Teamfight && threat.nearby_enemy_heroes > 0 {
                        0.58
                    } else {
                        0.10
                    };
                    (s, ready)
                }
                BasicItem::TravelBoots
                | BasicItem::JourneyBoots
                | BasicItem::HalcyonChargers
                | BasicItem::WarTreads => {
                    let s = if mode == MacroMode::Retreat && threat.nearby_enemy_heroes > 0 {
                        0.82
                    } else if matches!(mode, MacroMode::Teamfight | MacroMode::ObjectiveContest)
                        && (move_goal_dist > 7.5 || threat.nearby_allied_heroes > 0)
                    {
                        0.66
                    } else if move_goal_dist > 10.0 {
                        0.60
                    } else {
                        0.10
                    };
                    (s, false)
                }
                BasicItem::AtlasPauldron => {
                    let s = if threat.enemy_heroes_in_attack_range > 0 {
                        clamp01(
                            0.55 + 0.25
                                * linear_curve(
                                    f32::from(threat.enemy_heroes_in_attack_range),
                                    1.0,
                                    3.0,
                                )
                                + 0.20 * threat.total_threat,
                        )
                    } else {
                        0.0
                    };
                    (s, false)
                }
                BasicItem::Shiversteel => {
                    let s = if threat.enemy_heroes_in_attack_range > 0 && mode != MacroMode::Retreat
                    {
                        0.62
                    } else {
                        0.0
                    };
                    (s, false)
                }
                _ => (0.0, false),
            };
            if count < out.len() {
                out[count] = Some(ActiveItemIntent {
                    item: held.kind,
                    instance: held.instance,
                    category,
                    score,
                    executable_now,
                });
                count += 1;
            }
        }
        out
    }

    /// Picks the highest-scoring currently executable active item command (`>= 0.60` threshold).
    pub(crate) fn best_executable_active_item(
        &self,
        caster: usize,
        threat: &ThreatAssessment,
        mode: MacroMode,
        move_goal_dist: f32,
    ) -> Option<Command> {
        let intents = self.evaluate_active_item_intents(caster, threat, mode, move_goal_dist);
        let best = intents
            .into_iter()
            .flatten()
            .filter(|intent| intent.executable_now && intent.score >= 0.60)
            .max_by(|a, b| {
                a.score
                    .total_cmp(&b.score)
                    .then_with(|| b.instance.cmp(&a.instance))
            })?;
        match best.category {
            ActiveItemCategory::SelfHeal => Some(Command::ActivateHealingFlask {
                instance: best.instance,
            }),
            ActiveItemCategory::DefensiveBarrierCleanse => Some(Command::ActivateReflexBlock {
                instance: best.instance,
            }),
            ActiveItemCategory::SpeedEngageEscape => Some(Command::ActivateSprintBoots {
                instance: best.instance,
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use halcyon_data::combat_content::DamageVariable;
    use std::collections::BTreeSet;
    use std::path::Path;

    fn var(name: &str, base: f32) -> AbilityVariable {
        AbilityVariable {
            name: name.into(),
            base,
            ranks: Some(vec![base]),
            ..Default::default()
        }
    }

    fn ability(
        id: &str,
        target_type: Option<&str>,
        dmgs: usize,
        vars: Vec<AbilityVariable>,
    ) -> Ability {
        Ability {
            ability_id: id.into(),
            max_rank_guess: 5,
            target_type: target_type.map(str::to_owned),
            damage_variables: vec![DamageVariable::default(); dmgs],
            variables: vars,
            ..Default::default()
        }
    }

    #[test]
    fn response_curves_stay_bounded_and_monotonic() {
        for x in [
            -10.0_f32,
            0.0,
            0.25,
            0.5,
            0.75,
            1.0,
            5.0,
            f32::NAN,
            f32::INFINITY,
        ] {
            assert!((0.0..=1.0).contains(&clamp01(x)));
            assert!((0.0..=1.0).contains(&linear_curve(x, 0.2, 0.8)));
            assert!((0.0..=1.0).contains(&smoothstep(0.2, 0.8, x)));
            assert!((0.0..=1.0).contains(&inv_smoothstep(0.2, 0.8, x)));
            assert!((0.0..=1.0).contains(&logistic_curve(x, 0.5, 6.0)));
            assert!((0.0..=1.0).contains(&power_curve(x, 1.5)));
        }
        assert!(smoothstep(0.0, 1.0, 0.7) > smoothstep(0.0, 1.0, 0.3));
        assert!(inv_smoothstep(0.0, 1.0, 0.7) < inv_smoothstep(0.0, 1.0, 0.3));
        assert!(logistic_curve(0.8, 0.5, 6.0) > logistic_curve(0.2, 0.5, 6.0));
        assert!(power_curve(0.8, 1.5) > power_curve(0.4, 1.5));
    }

    #[test]
    fn data_driven_classifier_and_utility_scorer_cover_all_61_hero_templates() {
        // Verify synthetic ability classification across all 10 AbilityRole categories.
        let samples = [
            (
                ability(
                    "Sample__Engage",
                    Some("LABEL_ABILITY_TYPE_TARGETED"),
                    1,
                    vec![var("Range", 7.0), var("DashSpeed", 14.0)],
                ),
                crate::AbilitySlot::A,
                AbilityRole::Engage,
            ),
            (
                ability(
                    "Sample__Poke",
                    Some("LABEL_ABILITY_TYPE_DIRECTIONAL"),
                    1,
                    vec![var("Range", 9.5), var("SHOT_SPEED", 20.0)],
                ),
                crate::AbilitySlot::A,
                AbilityRole::Poke,
            ),
            (
                ability(
                    "Sample__Peel",
                    Some("LABEL_ABILITY_TYPE_RADIUS"),
                    1,
                    vec![var("Radius", 4.0), var("KnockbackDistance", 5.0)],
                ),
                crate::AbilitySlot::B,
                AbilityRole::Peel,
            ),
            (
                ability(
                    "Sample__Burst",
                    Some("LABEL_ABILITY_TYPE_DIRECTIONAL"),
                    2,
                    vec![var("Range", 11.0), var("EXECUTE_THRESHOLD", 0.25)],
                ),
                crate::AbilitySlot::C,
                AbilityRole::Burst,
            ),
            (
                ability(
                    "Sample__Heal",
                    Some("LABEL_ABILITY_TYPE_BUFF_ALLY"),
                    0,
                    vec![var("Range", 7.0), var("HealPerSecond", 120.0)],
                ),
                crate::AbilitySlot::A,
                AbilityRole::Heal,
            ),
            (
                ability(
                    "Sample__Barrier",
                    Some("LABEL_ABILITY_TYPE_BUFF_SELF"),
                    0,
                    vec![var("BarrierStrength", 350.0)],
                ),
                crate::AbilitySlot::B,
                AbilityRole::Barrier,
            ),
            (
                ability(
                    "Sample__Escape",
                    Some("LABEL_ABILITY_TYPE_BUFF_SELF"),
                    0,
                    vec![var("StealthDuration", 2.5)],
                ),
                crate::AbilitySlot::B,
                AbilityRole::Escape,
            ),
            (
                ability(
                    "Sample__CC",
                    Some("LABEL_ABILITY_TYPE_DIRECTIONAL"),
                    1,
                    vec![var("Range", 8.0), var("StunDuration", 1.5)],
                ),
                crate::AbilitySlot::A,
                AbilityRole::CrowdControl,
            ),
            (
                ability(
                    "Sample__Buff",
                    Some("LABEL_ABILITY_TYPE_BUFF_SELF"),
                    0,
                    vec![var("BonusASPDStrength", 0.6), var("BonusMSPDStrength", 1.2)],
                ),
                crate::AbilitySlot::B,
                AbilityRole::Buff,
            ),
            (
                ability(
                    "Sample__Zone",
                    Some("LABEL_ABILITY_TYPE_DIRECTIONAL"),
                    0,
                    vec![
                        var("Range", 8.0),
                        var("FieldRadius", 4.5),
                        var("Duration", 6.0),
                    ],
                ),
                crate::AbilitySlot::B,
                AbilityRole::ZoneControl,
            ),
        ];

        let mut seen_roles = BTreeSet::new();
        for (ab, slot, expected) in &samples {
            let classified = classify_ability(ab, *slot, AbilityForm::Primary, 5.5);
            assert_eq!(classified.primary_role, *expected, "{}", ab.ability_id);
            seen_roles.insert(classified.primary_role);
        }
        assert_eq!(seen_roles.len(), AbilityRole::ALL.len());

        // Verify fallback classification + utility scoring across all 61 hero templates.
        let w = World::new(1, 0);
        let ctx_duel = UtilityContext {
            hero_hp_ratio: 0.85,
            hero_energy_ratio: 0.80,
            hero_attack_range: 5.5,
            macro_mode: MacroMode::Teamfight,
            threat: ThreatAssessment {
                nearby_enemy_heroes: 2,
                total_threat: 0.45,
                ..ThreatAssessment::default()
            },
            enemy_kind: Kind::Hero,
            enemy_dist: 5.0,
            enemy_hp_ratio: 0.28,
            enemy_cluster_count: 2,
            lowest_ally_hp_ratio: 0.40,
            lowest_ally_dist: 4.5,
        };
        assert_eq!(all_hero_ability_slots().len(), 61);
        for slots in all_hero_ability_slots() {
            let kit = w.classified_kit_for(slots.template.as_raw(), 5.5);
            assert_eq!(kit.template, slots.template.as_raw());
            for ab in &kit.primary {
                let s = score_classified_ability(ab, &ctx_duel);
                assert!(s.is_finite() && (0.0..=1.0).contains(&s));
            }
        }
    }

    #[test]
    fn active_item_intent_scoring_covers_barrier_heal_speed_and_slow() {
        let mut w = World::new(7, 6);
        w.use_six_human_roster().unwrap();
        w.use_aral_lane().unwrap();
        w.entities[0].bot = true;
        w.entities[0].gold = 10_000;
        w.purchase(1, BasicItem::ReflexBlock).unwrap();
        w.purchase(1, BasicItem::SprintBoots).unwrap();
        for (offset, kind) in [
            BasicItem::FountainOfRenewal,
            BasicItem::AtlasPauldron,
            BasicItem::Shiversteel,
            BasicItem::Crucible,
        ]
        .into_iter()
        .enumerate()
        {
            w.entities[0].inventory.push(HeldItem {
                kind,
                instance: 50_000 + offset as u32,
            });
        }
        w.entities[0].hp = w.entities[0].max_hp * 0.25;
        w.entities[0].status.slow_until = w.tick + 10;
        w.entities[1].position = w.entities[0].position;

        let threat = ThreatAssessment {
            nearby_enemy_heroes: 2,
            enemy_heroes_in_attack_range: 1,
            nearby_allied_heroes: 1,
            lowest_ally_hp_ratio: 0.30,
            total_threat: 0.85,
            ..ThreatAssessment::default()
        };
        let intents = w.evaluate_active_item_intents(0, &threat, MacroMode::Retreat, 15.0);
        let find_score = |kind: BasicItem| {
            intents
                .iter()
                .flatten()
                .find(|i| i.item == kind)
                .map(|i| i.score)
                .unwrap()
        };
        assert!(find_score(BasicItem::ReflexBlock) >= 0.70);
        assert!(find_score(BasicItem::FountainOfRenewal) >= 0.65);
        assert!(find_score(BasicItem::SprintBoots) >= 0.80);
        assert!(find_score(BasicItem::AtlasPauldron) >= 0.65);
        assert!(find_score(BasicItem::Crucible) >= 0.60);
        // Shiversteel is suppressed during Retreat.
        assert_eq!(find_score(BasicItem::Shiversteel), 0.0);

        // Verify executable command path picks ReflexBlock (1.0 under CC+threat),
        // then HealingFlask (0.92), then SprintBoots (0.85) cleanly.
        let reflex_cmd = w
            .best_executable_active_item(0, &threat, MacroMode::Retreat, 15.0)
            .expect("executable reflex");
        assert!(matches!(reflex_cmd, Command::ActivateReflexBlock { .. }));

        let reflex_inst = w.entities[0].inventory[0].instance;
        w.activate_reflex(0, reflex_inst).unwrap();
        let flask_cmd = w
            .best_executable_active_item(0, &threat, MacroMode::Retreat, 15.0)
            .expect("executable flask");
        assert!(matches!(flask_cmd, Command::ActivateHealingFlask { .. }));

        let flask_inst = w.flask_instance(0).unwrap();
        w.activate_healing_flask(1, flask_inst).unwrap();
        let sprint_cmd = w
            .best_executable_active_item(0, &threat, MacroMode::Retreat, 15.0)
            .expect("executable sprint");
        assert!(matches!(sprint_cmd, Command::ActivateSprintBoots { .. }));
    }

    #[test]
    #[ignore = "requires delivery and server-data supplement for all 61 heroes"]
    fn supplemental_61_heroes_277_abilities_classify_and_score_cleanly() {
        let base = std::env::var_os("HALCYON_DELIVERY").unwrap();
        let extra_dir = std::env::var_os("HALCYON_DATA_EXTRA").unwrap();
        let data = GameData::load_with_extra(base, Some(Path::new(&extra_dir))).unwrap();
        let catalog = HeroIntentCatalog::from_data(&data);
        assert_eq!(catalog.len(), 61);

        let malene = catalog
            .kit_for(halcyon_data::EntityTemplate::Malene.as_raw())
            .expect("Malene kit");
        assert!(malene.alternate.is_some());

        let scenarios = [
            (
                "duel",
                UtilityContext {
                    hero_hp_ratio: 0.85,
                    hero_energy_ratio: 0.90,
                    hero_attack_range: 5.5,
                    macro_mode: MacroMode::LanePush,
                    threat: ThreatAssessment {
                        nearby_enemy_heroes: 1,
                        total_threat: 0.35,
                        ..ThreatAssessment::default()
                    },
                    enemy_kind: Kind::Hero,
                    enemy_dist: 5.2,
                    enemy_hp_ratio: 0.30,
                    enemy_cluster_count: 1,
                    lowest_ally_hp_ratio: 0.90,
                    lowest_ally_dist: 6.0,
                },
            ),
            (
                "retreat",
                UtilityContext {
                    hero_hp_ratio: 0.18,
                    hero_energy_ratio: 0.50,
                    hero_attack_range: 5.5,
                    macro_mode: MacroMode::Retreat,
                    threat: ThreatAssessment {
                        nearby_enemy_heroes: 2,
                        total_threat: 0.90,
                        ..ThreatAssessment::default()
                    },
                    enemy_kind: Kind::Hero,
                    enemy_dist: 4.5,
                    enemy_hp_ratio: 0.85,
                    enemy_cluster_count: 2,
                    lowest_ally_hp_ratio: 0.85,
                    lowest_ally_dist: 8.0,
                },
            ),
            (
                "teamfight_rescue",
                UtilityContext {
                    hero_hp_ratio: 0.70,
                    hero_energy_ratio: 0.75,
                    hero_attack_range: 5.5,
                    macro_mode: MacroMode::Teamfight,
                    threat: ThreatAssessment {
                        nearby_enemy_heroes: 3,
                        nearby_allied_heroes: 2,
                        total_threat: 0.65,
                        ..ThreatAssessment::default()
                    },
                    enemy_kind: Kind::Hero,
                    enemy_dist: 5.8,
                    enemy_hp_ratio: 0.55,
                    enemy_cluster_count: 3,
                    lowest_ally_hp_ratio: 0.25,
                    lowest_ally_dist: 4.0,
                },
            ),
        ];

        let mut role_counts = [0usize; 10];
        for slots in all_hero_ability_slots() {
            let kit = catalog.kit_for(slots.template.as_raw()).expect("hero kit");
            for ab in kit.primary.iter().chain(kit.alternate.iter().flatten()) {
                assert!(!ab.roles.is_empty(), "empty roles on {}", slots.hero_id);
                assert!(
                    ab.estimated_range.is_finite() && ab.estimated_range > 0.0,
                    "{} range",
                    slots.hero_id
                );
                assert!(
                    ab.estimated_radius.is_finite() && ab.estimated_radius > 0.0,
                    "{} radius",
                    slots.hero_id
                );
                role_counts[ab.primary_role as usize] += 1;
                for (name, ctx) in &scenarios {
                    let score = score_classified_ability(ab, ctx);
                    assert!(
                        score.is_finite() && (0.0..=1.0).contains(&score),
                        "{} {name} score={score}",
                        slots.hero_id
                    );
                }
            }
        }
        // Verify rich diversity across roles (not collapsing into a single fallback role).
        assert!(role_counts.iter().filter(|&&c| c > 0).count() >= 8);
    }
}
