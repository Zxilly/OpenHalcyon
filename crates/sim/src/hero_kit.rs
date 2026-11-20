//! Unified, deterministic Hero Ability, Heroic Perk, and Talent execution engine.
//!
//! Keeps inline `Entity` bloat to a single `Option<Box<HeroKitState>>` (8 bytes)
//! and executes active multi-tick zones, projectiles, traps, segment tripwires,
//! summoned pets, dashes, channels, tethers, and attachments through one sparse
//! `KitEffect` ECS `TimedSpell` family.

use std::collections::BTreeMap;

use bevy_ecs::component::Component;
use halcyon_data::GameData;
use serde::Serialize;

use crate::combat::{AbilitySlot, CastTarget};
use crate::persistent_spells::TimedSpell;
use crate::score::DamageSchool;
use crate::talents_runtime::{
    self, ActiveTalentState, LoadedTalent, TalentSpecial, find_talent, load_talents_from_data,
};
use crate::{Event, Kind, Position, Room, TICK_HZ, shop};

/// Canonical deterministic mark IDs used across `HeroKitState::marks` (shared with Batch B).
#[allow(dead_code)]
pub(crate) mod marks {
    pub const ARCANE_FIRE: u8 = 0;
    pub const HEARTTHROB: u8 = 1;
    pub const CORE_OVERLOAD: u8 = 2;
    pub const SOUL_FRAGMENT: u8 = 3;
    pub const LIVING_ARMOR: u8 = 4;
    pub const BLOODSONG: u8 = 5;
    pub const ADRENALINE: u8 = 6;
    pub const TRACER_SHOTS: u8 = 7;
    pub const WEAKNESS: u8 = 8;
    pub const KEEN_EYE: u8 = 9;
    pub const RESONANCE: u8 = 10;
    pub const FIRESTARTER: u8 = 11;
    pub const ARCANE_MARK: u8 = 12;
    pub const SPIN_UP: u8 = 13;
    pub const HEAT: u8 = 14;
    pub const KENSHO_STANCE: u8 = 15;
    pub const PUMP_IT_UP: u8 = 16;
    pub const LAW_OF_THE_CLAW: u8 = 17;
    pub const FORTRESS_MARK: u8 = 18;
    pub const LEO_EMPOWERED: u8 = 19;
    pub const BANISHING_MARK: u8 = 20;
    pub const CHARGED_KINETIC: u8 = 21;
    pub const CHURNWALKER_TETHER: u8 = 22;
    pub const ORDAINED_TETHER: u8 = 23;
    pub const BOOMSTICK_READY: u8 = 24;
    pub const DOUBLE_SHOT_READY: u8 = 25;
    pub const GOOP: u8 = 26;
    pub const MORTAL_WOUND: u8 = 27;
    pub const BLEED: u8 = 28;
    pub const TREANT_EMPOWER: u8 = 29;
    pub const STANCE_COUNTER: u8 = 30;
    pub const UNGUARDED: u8 = 31;
    pub const BLINDSIDE: u8 = 32;
    pub const RETRIBUTION_READY: u8 = 33;
    pub const WATER_POOL_BUFF: u8 = 34;
    pub const LIVING_BLADE_ASPD: u8 = 35;
    pub const ISHTAR_INDOMITABLE: u8 = 36;
    pub const KI: u8 = 37;
    pub const MELODIOUS_TUNE: u8 = 38;
    pub const AFTERBURN_PUNT: u8 = 39;
    pub const VANGUARD_GUARDED: u8 = 40;
    pub const BAPTISTE_EMPOWERED: u8 = 41;
    pub const ANKA_MARKED: u8 = 42;
    pub const WARHAWK_GRENADE_OUT: u8 = 43;
}

/// Resource bar model used by a hero.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ResourceKind {
    Energy,
    Stamina,
    Focus,
    Vengeance,
    Bloodrage,
    /// Caine ammo / Alpha reboot / cooldown-only (`0.0` energy bar).
    None,
}

/// Logical slot including recast, secondary form, and chord variants.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum HeroSlot {
    A,
    B,
    C,
    A2,
    B2,
    C2,
    C3,
    C4,
}

impl HeroSlot {
    pub(crate) const fn base_slot(self) -> AbilitySlot {
        match self {
            Self::A | Self::A2 => AbilitySlot::A,
            Self::B | Self::B2 => AbilitySlot::B,
            Self::C | Self::C2 | Self::C3 | Self::C4 => AbilitySlot::C,
        }
    }
}

/// How player targets resolve on cast validation and range checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum CastTargeting {
    InstantSelf,
    TargetEnemy,
    TargetAlly,
    TargetAnyHero,
    TargetAllyOrSelf,
    GroundPoint,
    Directional,
    TargetLocationOrEntity,
}

/// Damage mitigation school mapping cleanly onto `DamageSchool`.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum DamageKind {
    Weapon,
    Crystal,
    True,
    PercentMaxHp,
    PercentMissingHp,
    PercentCurrentHp,
}

impl DamageKind {
    pub(crate) const fn school(self) -> DamageSchool {
        match self {
            Self::Weapon => DamageSchool::Weapon,
            Self::Crystal => DamageSchool::Crystal,
            Self::True | Self::PercentMaxHp | Self::PercentMissingHp | Self::PercentCurrentHp => {
                DamageSchool::True
            }
        }
    }
}

/// Precondition checked before deducting cost/cooldown and beginning a cast.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum CastGate {
    None,
    RequiresTetheredTarget,
    RequiresLockedTarget,
    RequiresFullResource,
    RequiresRebootReady,
    RequiresNotRecentlyDamaged,
    RequiresMusicalChord3,
    RequiresEmpowered,
    RequiresOutOfCombat,
}

/// Mark selector used in declarative `PrimitiveEffect`s.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum MarkKind {
    Custom(u8),
}

impl MarkKind {
    pub(crate) const fn id(self) -> u8 {
        match self {
            Self::Custom(id) => id,
        }
    }
}

/// Stat buffs tracked deterministically inside `HeroKitState`.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub(crate) enum BuffStatKind {
    MoveSpeedFlat,
    MoveSpeedPct,
    AttackSpeed,
    Armor,
    Shield,
    WeaponPowerFlat,
    WeaponPowerPct,
    CrystalPowerFlat,
    CrystalPowerPct,
    Lifesteal,
    DamageReduction,
    FortifiedHealth,
    AttackRange,
    CooldownReduction,
    CritChance,
    DamageAmp,
}

/// Data-bound rank + scaling formula with compiled fallback from `abilities.json`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ValueSpec {
    pub var_name: Option<&'static str>,
    pub dmg_index: Option<u8>,
    pub ranks: [f32; 5],
    pub cp_ratio: f32,
    pub wp_ratio: f32,
    pub bonus_wp_ratio: f32,
    pub bonus_hp_ratio: f32,
    pub alt_ratio: f32,
    pub bonus_as_ratio: f32,
    pub clamp_min: f32,
    pub clamp_max: f32,
}

#[allow(dead_code)]
impl ValueSpec {
    pub(crate) const fn zero() -> Self {
        Self {
            var_name: None,
            dmg_index: None,
            ranks: [0.0; 5],
            cp_ratio: 0.0,
            wp_ratio: 0.0,
            bonus_wp_ratio: 0.0,
            bonus_hp_ratio: 0.0,
            alt_ratio: 0.0,
            bonus_as_ratio: 0.0,
            clamp_min: 0.0,
            clamp_max: 0.0,
        }
    }

    pub(crate) const fn constant(v: f32) -> Self {
        Self {
            ranks: [v; 5],
            ..Self::zero()
        }
    }

    pub(crate) const fn ranks(ranks: [f32; 5]) -> Self {
        Self {
            ranks,
            ..Self::zero()
        }
    }

    pub(crate) const fn ranks3(r: [f32; 3]) -> Self {
        Self::ranks([r[0], r[1], r[2], r[2], r[2]])
    }

    pub(crate) const fn var(var_name: &'static str, ranks: [f32; 5]) -> Self {
        Self {
            var_name: Some(var_name),
            ranks,
            ..Self::zero()
        }
    }

    pub(crate) const fn var3(var_name: &'static str, r: [f32; 3]) -> Self {
        Self::var(var_name, [r[0], r[1], r[2], r[2], r[2]])
    }

    pub(crate) const fn var_scaled(
        var_name: &'static str,
        ranks: [f32; 5],
        cp_ratio: f32,
        wp_ratio: f32,
        bonus_wp_ratio: f32,
        bonus_hp_ratio: f32,
    ) -> Self {
        Self {
            var_name: Some(var_name),
            ranks,
            cp_ratio,
            wp_ratio,
            bonus_wp_ratio,
            bonus_hp_ratio,
            ..Self::zero()
        }
    }

    pub(crate) const fn dmg(
        dmg_index: u8,
        ranks: [f32; 5],
        cp_ratio: f32,
        wp_ratio: f32,
        bonus_wp_ratio: f32,
    ) -> Self {
        Self {
            dmg_index: Some(dmg_index),
            ranks,
            cp_ratio,
            wp_ratio,
            bonus_wp_ratio,
            ..Self::zero()
        }
    }

    pub(crate) const fn dmg3(
        dmg_index: u8,
        r: [f32; 3],
        cp_ratio: f32,
        wp_ratio: f32,
        bonus_wp_ratio: f32,
    ) -> Self {
        Self::dmg(
            dmg_index,
            [r[0], r[1], r[2], r[2], r[2]],
            cp_ratio,
            wp_ratio,
            bonus_wp_ratio,
        )
    }

    pub(crate) const fn var3_scaled(var_name: &'static str, r: [f32; 3], cp_ratio: f32) -> Self {
        Self::var_scaled(
            var_name,
            [r[0], r[1], r[2], r[2], r[2]],
            cp_ratio,
            0.0,
            0.0,
            0.0,
        )
    }

    pub(crate) const fn with_wp(mut self, wp_ratio: f32) -> Self {
        self.wp_ratio = wp_ratio;
        self
    }

    pub(crate) fn eval_fallback(self, rank: u8, inputs: EvalInputs) -> f32 {
        let idx = usize::from(rank.saturating_sub(1)).min(4);
        let raw = self.ranks[idx]
            + self.cp_ratio * inputs.cp
            + self.wp_ratio * inputs.wp
            + self.bonus_wp_ratio * inputs.bonus_wp
            + self.bonus_hp_ratio * inputs.bonus_hp
            + self.alt_ratio * inputs.alt
            + self.bonus_as_ratio * inputs.bonus_as.max(0.0);
        let min_clamped = if self.clamp_min > 0.0 {
            raw.max(self.clamp_min)
        } else {
            raw
        };
        if self.clamp_max > 0.0 {
            min_clamped.min(self.clamp_max)
        } else {
            min_clamped
        }
    }
}

/// Stat snapshot used to evaluate `ValueSpec` and `GameData` formulas deterministically.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct EvalInputs {
    pub cp: f32,
    pub wp: f32,
    pub bonus_wp: f32,
    pub bonus_hp: f32,
    pub alt: f32,
    pub bonus_as: f32,
}

/// Reusable composable effect primitives powering all 61 heroes' kits (shared with Batch B).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub(crate) enum PrimitiveEffect {
    TargetedStrike {
        damage: ValueSpec,
        school: DamageKind,
        bonus_if_marked: Option<(MarkKind, ValueSpec)>,
        triggers_basic_effects: bool,
        resets_attack_cd: bool,
        stun_seconds: ValueSpec,
        slow_pct: ValueSpec,
        slow_seconds: ValueSpec,
    },
    SkillshotProjectile {
        damage: ValueSpec,
        school: DamageKind,
        range: ValueSpec,
        speed: ValueSpec,
        radius: ValueSpec,
        piercing: bool,
        return_to_caster: bool,
        repeat_hit_mult: f32,
        splash_damage: Option<ValueSpec>,
        splash_radius: f32,
        stun_seconds: ValueSpec,
        root_seconds: ValueSpec,
        silence_seconds: ValueSpec,
        slow_pct: ValueSpec,
        slow_seconds: ValueSpec,
        minion_mult: f32,
        resets_cast_cd_on_hit: bool,
        applies_mark: Option<(MarkKind, u8, f32)>,
        destroys_enemy_projectiles: bool,
    },
    GroundAoE {
        radius: ValueSpec,
        duration: ValueSpec,
        ticks_per_sec: f32,
        dps: ValueSpec,
        hps: ValueSpec,
        school: DamageKind,
        slow_pct: ValueSpec,
        end_damage: Option<ValueSpec>,
        end_root_seconds: ValueSpec,
        end_stun_seconds: ValueSpec,
        drifts_toward_aim: bool,
        minion_mult: f32,
        applies_mark: Option<(MarkKind, u8, f32)>,
        zone_kind: u8,
    },
    DelayedBlast {
        delay: ValueSpec,
        radius: ValueSpec,
        inner_radius: f32,
        damage: ValueSpec,
        outer_mult: f32,
        school: DamageKind,
        stun_seconds: ValueSpec,
        root_seconds: ValueSpec,
        sleep_seconds: ValueSpec,
        slow_pct: ValueSpec,
        slow_seconds: ValueSpec,
        reveal_seconds: f32,
        bonus_if_marked: Option<(MarkKind, f32)>,
        minion_mult: f32,
        applies_mark: Option<(MarkKind, u8, f32)>,
    },
    ConeWave {
        range: ValueSpec,
        half_angle_deg: f32,
        damage: ValueSpec,
        school: DamageKind,
        multi_bolts: u8,
        extra_hit_mult: f32,
        slow_pct: ValueSpec,
        slow_seconds: ValueSpec,
        stun_seconds: ValueSpec,
        fear_seconds: ValueSpec,
        silence_seconds: ValueSpec,
        knockback_dist: f32,
        reveal_seconds: f32,
        minion_mult: f32,
        applies_mark: Option<(MarkKind, u8, f32)>,
    },
    DashBlink {
        range: ValueSpec,
        duration: ValueSpec,
        instant_blink: bool,
        invuln_during: bool,
        unstoppable_during: bool,
        path_radius: f32,
        path_damage: ValueSpec,
        arrival_radius: f32,
        arrival_damage: ValueSpec,
        school: DamageKind,
        slow_pct: ValueSpec,
        slow_seconds: ValueSpec,
        stun_seconds: ValueSpec,
        knockback_dist: f32,
        backward_hop: bool,
        applies_mark: Option<(MarkKind, u8, f32)>,
    },
    LungeHook {
        range: ValueSpec,
        damage: ValueSpec,
        school: DamageKind,
        pull_distance: ValueSpec,
        knockback_distance: ValueSpec,
        stun_on_wall_seconds: ValueSpec,
        tether_duration: ValueSpec,
        tether_break_range: ValueSpec,
        tether_dps_pct_hp: ValueSpec,
        devour_seconds: ValueSpec,
        suppress_untargetable: bool,
    },
    SelfBuff {
        stat: BuffStatKind,
        amount: ValueSpec,
        duration: ValueSpec,
        empowered_attacks: u8,
        empowered_bonus_damage: ValueSpec,
        empowered_stun_seconds: f32,
        empowered_slow_pct: f32,
        empowered_slow_seconds: f32,
        empowered_knockback: f32,
        resets_attack_cd: bool,
    },
    AllyHealBarrier {
        radius: ValueSpec,
        include_self: bool,
        target_only: bool,
        burst_heal: ValueSpec,
        hot_per_sec: ValueSpec,
        hot_duration: ValueSpec,
        barrier: ValueSpec,
        barrier_duration: ValueSpec,
        fortified: ValueSpec,
        fortified_duration: ValueSpec,
        self_cast_mult: f32,
        grants_attack_buff_stacks: u8,
        grants_attack_bonus_dmg: ValueSpec,
    },
    ChannelBeam {
        duration: ValueSpec,
        ticks_per_sec: f32,
        range: ValueSpec,
        width: f32,
        dps: ValueSpec,
        school: DamageKind,
        lock_caster: bool,
        slow_pct: ValueSpec,
        stun_target: bool,
        global_all_heroes: bool,
        shield_pierce: ValueSpec,
        structure_mult: f32,
        minion_mult: f32,
    },
    SummonPet {
        count: u8,
        max_active: u8,
        duration: ValueSpec,
        pet_hp: ValueSpec,
        pet_damage: ValueSpec,
        school: DamageKind,
        hit_ticks: u8,
        applies_mark: Option<(MarkKind, u8, f32)>,
        replace_existing: bool,
        pet_kind: u8,
    },
    TrapZone {
        arm_delay: ValueSpec,
        duration: ValueSpec,
        trigger_radius: ValueSpec,
        blast_radius: ValueSpec,
        damage: ValueSpec,
        school: DamageKind,
        root_seconds: ValueSpec,
        stun_seconds: ValueSpec,
        slow_pct: ValueSpec,
        max_traps: u8,
        reveals_enemies: bool,
    },
    SegmentBarrier {
        radius_or_span: ValueSpec,
        duration: ValueSpec,
        arm_delay: ValueSpec,
        dps_or_hit_damage: ValueSpec,
        school: DamageKind,
        stun_seconds: ValueSpec,
        silence_seconds: ValueSpec,
        slow_pct: ValueSpec,
        max_stakes: u8,
        is_ring_arena: bool,
    },
    StealthMode {
        pre_delay: ValueSpec,
        duration: ValueSpec,
        speed_bonus: ValueSpec,
        affect_allies: bool,
        break_on_damage: bool,
        break_on_attack: bool,
        re_stealth_delay: f32,
    },
    MarkStacks {
        mark: MarkKind,
        on_self: bool,
        add_stacks: i8,
        max_stacks: u8,
        duration: f32,
        consume_all: bool,
        damage_per_stack: ValueSpec,
        heal_per_stack: ValueSpec,
        school: DamageKind,
        mortal_wound_seconds: f32,
    },
    ExecuteDamage {
        threshold_instant_kill_pct: ValueSpec,
        missing_hp_pct: ValueSpec,
        school: DamageKind,
        nonhero_cap: f32,
    },
    CleanseImmune {
        cleanse_cc: bool,
        cc_immune_seconds: ValueSpec,
        untargetable_seconds: ValueSpec,
    },
    FormSwap {
        to_form: u8,
        empowers_next_attack: bool,
        bonus_damage: ValueSpec,
        slow_pct: f32,
        slow_seconds: f32,
    },
    AmmoPool {
        slot: HeroSlot,
        max_charges: ValueSpec,
        reload_seconds: ValueSpec,
        grant_charges: i8,
        grant_resource: f32,
    },
    RefreshCooldown {
        slot: HeroSlot,
        reduce_pct: f32,
        reduce_flat_sec: f32,
        overdrive_only: bool,
    },
    OpenRecast {
        slot: HeroSlot,
        window_seconds: ValueSpec,
        stage: u8,
    },
}

/// Ability execution callback signature for custom per-hero mechanics.
/// Return `Ok(true)` if handled completely, or `Ok(false)` to also execute `spec.effects`.
pub(crate) type AbilityExecFn =
    fn(&mut HeroKitContext<'_, '_>, AbilitySlot, u8, CastTarget) -> Result<bool, &'static str>;

/// Periodic 20Hz perk callback signature.
pub(crate) type PerkTickFn = fn(&mut HeroKitContext<'_, '_>);

/// Basic-attack perk callback: returns `(bonus_damage, bonus_school)`.
pub(crate) type PerkAttackFn =
    fn(&mut HeroKitContext<'_, '_>, usize, f32, bool) -> (f32, DamageSchool);

/// Ability cast perk callback.
pub(crate) type PerkCastFn = fn(&mut HeroKitContext<'_, '_>, AbilitySlot, u8, CastTarget);

/// Ability damage dealt perk callback.
pub(crate) type PerkAbilityHitFn = fn(&mut HeroKitContext<'_, '_>, usize, f32);

/// Incoming damage perk callback: returns incoming damage multiplier (`1.0` = unchanged).
pub(crate) type PerkDamagedFn =
    fn(&mut HeroKitContext<'_, '_>, Option<usize>, f32, DamageSchool) -> f32;

/// Kill / assist perk callback: `(victim_idx, is_killer, is_hero)`.
pub(crate) type PerkKillAssistFn = fn(&mut HeroKitContext<'_, '_>, usize, bool, bool);

/// Declarative spec for one ability or sub-ability variant.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AbilityKitSpec {
    pub ability_id: &'static str,
    pub targeting: CastTargeting,
    pub max_rank: u8,
    pub startup: ValueSpec,
    pub range: ValueSpec,
    pub cooldown: ValueSpec,
    pub cost: ValueSpec,
    pub charges: ValueSpec,
    pub between_charges_cd: ValueSpec,
    pub gate: CastGate,
    pub castable_while_cc: bool,
    pub castable_during_movement: bool,
    pub effects: &'static [PrimitiveEffect],
    pub passive_bonus: Option<(BuffStatKind, ValueSpec)>,
    pub custom_exec: Option<AbilityExecFn>,
}

impl AbilityKitSpec {
    pub(crate) const DEFAULT: Self = Self {
        ability_id: "",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::zero(),
        range: ValueSpec::constant(6.0),
        cooldown: ValueSpec::constant(8.0),
        cost: ValueSpec::zero(),
        charges: ValueSpec::zero(),
        between_charges_cd: ValueSpec::zero(),
        gate: CastGate::None,
        castable_while_cc: false,
        castable_during_movement: false,
        effects: &[],
        passive_bonus: None,
        custom_exec: None,
    };
}

/// Alternate-form, recast, or chord ability variant bound to a `HeroSlot`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AbilityVariantSpec {
    pub slot: HeroSlot,
    pub spec: AbilityKitSpec,
}

/// Heroic Perk declarative attributes + optional runtime hooks.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct HeroicPerkSpec {
    pub id: &'static str,
    pub basic_attack_is_crystal: bool,
    pub basic_attack_damage_mult: f32,
    pub attack_speed_efficiency: f32,
    pub basic_cleave_radius: f32,
    pub basic_cleave_fraction: f32,
    pub unstoppable_cc_downgrade: bool,
    pub healing_received_mult_base: f32,
    pub healing_received_mult_per_level: f32,
    pub on_tick: Option<PerkTickFn>,
    pub on_basic_attack: Option<PerkAttackFn>,
    pub on_ability_cast: Option<PerkCastFn>,
    pub on_ability_damage: Option<PerkAbilityHitFn>,
    pub on_take_damage: Option<PerkDamagedFn>,
    pub on_kill_assist: Option<PerkKillAssistFn>,
}

impl HeroicPerkSpec {
    pub(crate) const DEFAULT: Self = Self {
        id: "",
        basic_attack_is_crystal: false,
        basic_attack_damage_mult: 1.0,
        attack_speed_efficiency: 1.0,
        basic_cleave_radius: 0.0,
        basic_cleave_fraction: 0.0,
        unstoppable_cc_downgrade: false,
        healing_received_mult_base: 1.0,
        healing_received_mult_per_level: 0.0,
        on_tick: None,
        on_basic_attack: None,
        on_ability_cast: None,
        on_ability_damage: None,
        on_take_damage: None,
        on_kill_assist: None,
    };
}

/// Complete static kit specification for a hero across all 61 `abilities.json` entries.
#[derive(Clone, Copy, Debug)]
pub(crate) struct HeroKitSpec {
    pub hero_id: &'static str,
    pub type_id: u16,
    pub resource: ResourceKind,
    pub resource_max: f32,
    pub resource_initial: f32,
    pub resource_regen_per_sec: f32,
    pub resource_energy_conversion: (f32, f32),
    pub ult_from_level_one: bool,
    pub ult_max_rank: u8,
    pub perk: HeroicPerkSpec,
    pub a: AbilityKitSpec,
    pub b: AbilityKitSpec,
    pub c: AbilityKitSpec,
    pub variants: &'static [AbilityVariantSpec],
}

impl HeroKitSpec {
    pub(crate) const DEFAULT: Self = Self {
        hero_id: "",
        type_id: 0,
        resource: ResourceKind::Energy,
        resource_max: 0.0,
        resource_initial: 0.0,
        resource_regen_per_sec: 0.0,
        resource_energy_conversion: (0.0, 0.0),
        ult_from_level_one: false,
        ult_max_rank: 3,
        perk: HeroicPerkSpec::DEFAULT,
        a: AbilityKitSpec::DEFAULT,
        b: AbilityKitSpec::DEFAULT,
        c: AbilityKitSpec::DEFAULT,
        variants: &[],
    };

    pub(crate) fn variant(&'static self, slot: HeroSlot) -> Option<&'static AbilityKitSpec> {
        match slot {
            HeroSlot::A => Some(&self.a),
            HeroSlot::B => Some(&self.b),
            HeroSlot::C => Some(&self.c),
            _ => self
                .variants
                .iter()
                .find(|v| v.slot == slot)
                .map(|v| &v.spec),
        }
    }
}

/// Loaded `GameData` variable formula matching `DATA_SPEC.md`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct LoadedVarFormula {
    pub base: f32,
    pub per_rank: f32,
    pub overdrive: f32,
    pub cp_ratio: f32,
    pub wp_ratio: f32,
    pub alt_ratio: f32,
    pub bonus_as_ratio: f32,
    pub clamp: f32,
    pub ranks: Vec<f32>,
}

impl LoadedVarFormula {
    pub(crate) fn eval(
        &self,
        rank: u8,
        max_rank: u8,
        inputs: EvalInputs,
        use_bonus_wp: bool,
    ) -> f32 {
        let idx = usize::from(rank.saturating_sub(1));
        let base_val = self.ranks.get(idx).copied().unwrap_or_else(|| {
            let od = if max_rank == 5 && rank == 5 {
                self.overdrive
            } else {
                0.0
            };
            self.base + f32::from(rank.saturating_sub(1)) * self.per_rank + od
        });
        let wp_term = if use_bonus_wp {
            self.wp_ratio * inputs.bonus_wp
        } else {
            self.wp_ratio * inputs.wp
        };
        let v = base_val
            + self.cp_ratio * inputs.cp
            + wp_term
            + self.alt_ratio * inputs.alt
            + self.bonus_as_ratio * inputs.bonus_as.max(0.0);
        if self.clamp > 0.0 {
            if self.per_rank < 0.0 || self.clamp < self.base {
                v.max(self.clamp)
            } else {
                v.min(self.clamp)
            }
        } else {
            v
        }
    }
}

/// Loaded `GameData` damage variable formula.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct LoadedDmgFormula {
    pub header_u32: u32,
    pub tooltip_operand: Option<String>,
    pub base: f32,
    pub per_rank: f32,
    pub overdrive: f32,
    pub cp_ratio: f32,
    pub wp_ratio: f32,
    pub f2: f32,
    pub f6: f32,
    pub f7: f32,
    pub ranks: Vec<f32>,
}

impl LoadedDmgFormula {
    pub(crate) fn eval(
        &self,
        rank: u8,
        max_rank: u8,
        inputs: EvalInputs,
        use_bonus_wp: bool,
    ) -> f32 {
        let idx = usize::from(rank.saturating_sub(1));
        let base_val = self.ranks.get(idx).copied().unwrap_or_else(|| {
            let od = if max_rank == 5 && rank == 5 {
                self.overdrive
            } else {
                0.0
            };
            self.base + f32::from(rank.saturating_sub(1)) * self.per_rank + od
        });
        let wp_coeff = if self.wp_ratio != 0.0 {
            self.wp_ratio
        } else {
            self.f6
        };
        let wp_term = if use_bonus_wp && self.f6 == 0.0 {
            wp_coeff * inputs.bonus_wp
        } else {
            wp_coeff * inputs.wp
        };
        base_val
            + self.cp_ratio * inputs.cp
            + wp_term
            + self.f2 * inputs.alt
            + self.f7 * inputs.bonus_hp
    }
}

/// Loaded ability record from `abilities.json`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct LoadedAbility {
    pub max_rank: u8,
    pub vars: BTreeMap<String, LoadedVarFormula>,
    pub dmg_vars: Vec<LoadedDmgFormula>,
}

/// Loaded hero kit parameters from `abilities.json`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct LoadedHero {
    pub hero_id: String,
    pub type_id: u16,
    pub abilities: BTreeMap<String, LoadedAbility>,
}

/// Deterministic data-driven book attached optionally to `AbilityBook::hero_kits`.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub(crate) struct HeroKitBook {
    pub heroes: BTreeMap<String, LoadedHero>,
    pub by_type_id: BTreeMap<u16, String>,
    pub talents: BTreeMap<String, LoadedTalent>,
}

impl HeroKitBook {
    pub(crate) fn load(data: &GameData) -> Result<Option<Self>, String> {
        let content = data.extra.combat_content();
        if content.heroes().len() == 0 && content.talents().len() == 0 {
            return Ok(None);
        }
        let mut book = Self::default();
        let unit_type_ids: BTreeMap<String, u16> = content
            .units()
            .filter_map(|u| {
                u.entity_type_id
                    .map(|tid| (u.name.to_ascii_lowercase(), tid))
            })
            .collect();

        for hero in content.heroes() {
            let tid = crate::hero_kits::kit_spec_by_hero_id(&hero.hero_id)
                .map(|s| s.type_id)
                .or_else(|| data.type_id(&hero.hero_id))
                .or_else(|| {
                    unit_type_ids
                        .get(&hero.hero_id.to_ascii_lowercase())
                        .copied()
                })
                .unwrap_or(0);
            let mut abilities = BTreeMap::new();
            for ab in &hero.abilities {
                let mut vars = BTreeMap::new();
                for v in &ab.variables {
                    vars.insert(
                        v.name.clone(),
                        LoadedVarFormula {
                            base: v.base,
                            per_rank: v.per_rank,
                            overdrive: v.overdrive,
                            cp_ratio: v.cp_ratio,
                            wp_ratio: v.wp_ratio,
                            alt_ratio: v.alt_ratio,
                            bonus_as_ratio: v.bonus_as_ratio,
                            clamp: v.clamp,
                            ranks: v.ranks.clone().unwrap_or_default(),
                        },
                    );
                }
                let dmg_vars = ab
                    .damage_variables
                    .iter()
                    .map(|d| LoadedDmgFormula {
                        header_u32: d.header_u32,
                        tooltip_operand: d.tooltip_operand.clone(),
                        base: d.base,
                        per_rank: d.per_rank,
                        overdrive: d.overdrive,
                        cp_ratio: d.cp_ratio,
                        wp_ratio: d.wp_ratio,
                        f2: d.f2,
                        f6: d.f6,
                        f7: d.f7,
                        ranks: d.ranks.clone(),
                    })
                    .collect();
                abilities.insert(
                    ab.ability_id.clone(),
                    LoadedAbility {
                        max_rank: (ab.max_rank_guess.max(1) as u8).min(5),
                        vars,
                        dmg_vars,
                    },
                );
            }
            if tid != 0 {
                book.by_type_id.insert(tid, hero.hero_id.clone());
            }
            book.heroes.insert(
                hero.hero_id.clone(),
                LoadedHero {
                    hero_id: hero.hero_id.clone(),
                    type_id: tid,
                    abilities,
                },
            );
        }
        book.talents = load_talents_from_data(data);
        Ok(Some(book))
    }

    #[allow(dead_code)]
    pub(crate) fn builtin() -> Self {
        Self::default()
    }

    pub(crate) fn hero_by_type(&self, type_id: u16) -> Option<&LoadedHero> {
        if let Some(id) = self.by_type_id.get(&type_id)
            && let Some(h) = self.heroes.get(id)
        {
            return Some(h);
        }
        let spec = crate::hero_kits::kit_spec(type_id)?;
        self.heroes.get(spec.hero_id)
    }

    pub(crate) fn eval_value(
        &self,
        type_id: u16,
        ability_id: &str,
        rank: u8,
        fallback: ValueSpec,
        inputs: EvalInputs,
    ) -> f32 {
        if let Some(hero) = self.hero_by_type(type_id)
            && let Some(ab) = hero.abilities.get(ability_id)
        {
            if let Some(var_name) = fallback.var_name
                && let Some(v) = ab.vars.get(var_name).or_else(|| {
                    ab.vars
                        .iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case(var_name))
                        .map(|(_, v)| v)
                })
            {
                let use_bwp = fallback.bonus_wp_ratio > 0.0 && fallback.wp_ratio == 0.0;
                let mut val = v.eval(rank, ab.max_rank, inputs, use_bwp);
                if v.cp_ratio == 0.0 && fallback.cp_ratio != 0.0 {
                    val += fallback.cp_ratio * inputs.cp;
                }
                if fallback.bonus_hp_ratio != 0.0 {
                    val += fallback.bonus_hp_ratio * inputs.bonus_hp;
                }
                return val;
            }
            if let Some(dmg_idx) = fallback.dmg_index
                && let Some(d) = ab.dmg_vars.get(usize::from(dmg_idx))
            {
                let use_bwp = fallback.bonus_wp_ratio > 0.0 && fallback.wp_ratio == 0.0;
                return d.eval(rank, ab.max_rank, inputs, use_bwp);
            }
        }
        fallback.eval_fallback(rank, inputs)
    }
}

/// Active timed mark instance stored in `HeroKitState::marks`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub(crate) struct MarkInstance {
    pub stacks: u8,
    pub until: u64,
    pub source_id: u32,
}

/// Timed stat buff stored in `HeroKitState::buffs`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub(crate) struct TimedBuff {
    pub stat: BuffStatKind,
    pub amount: f32,
    pub until: u64,
}

/// Sparse boxed state on `Entity::hero_kit` (`Option<Box<HeroKitState>>`).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub(crate) struct HeroKitState {
    pub resource: f32,
    pub resource_initialized: bool,
    pub form: u8,
    pub step_stage: [u8; 3],
    pub recast_until: [u64; 3],
    pub recast_target: [u32; 3],
    pub charges: [u8; 3],
    pub charges_initialized: [bool; 3],
    pub charge_regen_at: [u64; 3],
    pub marks: BTreeMap<u8, MarkInstance>,
    pub buffs: Vec<TimedBuff>,
    pub fortified_health: f32,
    pub fortified_until: u64,
    pub unstoppable_until: u64,
    pub untargetable_until: u64,
    pub stealth_until: u64,
    pub stealth_grace_until: u64,
    pub stealth_speed: f32,
    pub fear_until: u64,
    pub fear_source: u32,
    pub sleep_until: u64,
    pub sleep_unwaking: bool,
    pub taunt_until: u64,
    pub taunt_target: u32,
    pub mortal_wound_until: u64,
    pub empowered_attacks: u8,
    pub empowered_until: u64,
    pub empowered_bonus_damage: f32,
    pub empowered_is_crystal: bool,
    pub empowered_stun_sec: f32,
    pub empowered_slow_pct: f32,
    pub empowered_slow_sec: f32,
    pub empowered_knockback: f32,
    pub talent: Option<ActiveTalentState>,
    pub last_damaged_tick: u64,
    pub last_attack_tick: u64,
    pub stationary_ticks: u16,
    pub lock_target_id: u32,
    pub lock_target_until: u64,
    pub musical_notes: [u8; 3],
    pub musical_note_count: u8,
    pub counters: [u32; 4],
    pub timers: [u64; 4],
    pub values: [f32; 4],
}

impl HeroKitState {
    /// True when no runtime state or talent differs from `Default::default()`, allowing pruning to `None`.
    pub(crate) fn is_inert_default(&self) -> bool {
        !self.resource_initialized
            && self.resource == 0.0
            && self.form == 0
            && self.step_stage == [0; 3]
            && self.recast_until == [0; 3]
            && self.recast_target == [0; 3]
            && self.charges == [0; 3]
            && self.charges_initialized == [false; 3]
            && self.charge_regen_at == [0; 3]
            && self.marks.is_empty()
            && self.buffs.is_empty()
            && self.fortified_health == 0.0
            && self.fortified_until == 0
            && self.unstoppable_until == 0
            && self.untargetable_until == 0
            && self.stealth_until == 0
            && self.stealth_grace_until == 0
            && self.stealth_speed == 0.0
            && self.fear_until == 0
            && self.fear_source == 0
            && self.sleep_until == 0
            && !self.sleep_unwaking
            && self.taunt_until == 0
            && self.taunt_target == 0
            && self.mortal_wound_until == 0
            && self.empowered_attacks == 0
            && self.empowered_until == 0
            && self.empowered_bonus_damage == 0.0
            && !self.empowered_is_crystal
            && self.empowered_stun_sec == 0.0
            && self.empowered_slow_pct == 0.0
            && self.empowered_slow_sec == 0.0
            && self.empowered_knockback == 0.0
            && self.talent.is_none()
            && self.last_damaged_tick == 0
            && self.last_attack_tick == 0
            && self.stationary_ticks == 0
            && self.lock_target_id == 0
            && self.lock_target_until == 0
            && self.musical_notes == [0; 3]
            && self.musical_note_count == 0
            && self.counters == [0; 4]
            && self.timers == [0; 4]
            && self.values == [0.0; 4]
    }
}

/// Net aggregated bonuses on an `Entity` from `HeroKitState` + learned rank passives + talents.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct KitStatBonus {
    pub move_speed_flat: f32,
    pub move_speed_pct: f32,
    pub attack_speed: f32,
    pub armor_flat: f32,
    pub armor_pct: f32,
    pub shield_flat: f32,
    pub shield_pct: f32,
    pub weapon_flat: f32,
    pub weapon_pct: f32,
    pub crystal_flat: f32,
    pub crystal_pct: f32,
    pub lifesteal: f32,
    pub damage_reduction: f32,
    pub attack_range: f32,
    pub cooldown_accel: f32,
    pub crit_chance: f32,
    pub crit_damage: f32,
    pub damage_amp: f32,
    pub armor_pierce: f32,
    pub shield_pierce: f32,
    pub hp_regen_per_sec: f32,
    pub max_resource_bonus: f32,
}

/// Pending ability release queued on an `Entity` or scheduled via `KitEffect`.
#[allow(dead_code)]
#[derive(Clone, Debug, Serialize)]
pub(crate) struct KitPendingCast {
    pub slot: AbilitySlot,
    pub rank: u8,
    pub target: CastTarget,
    pub release: u64,
    pub sub_slot: u8,
}

/// Active multi-tick effect stored as sparse ECS `TimedSpell`.
#[derive(Component, Clone, Debug, Serialize)]
pub(crate) struct KitEffect {
    pub owner_id: u32,
    pub team: u8,
    pub position: Position,
    pub goal: Position,
    pub kind: KitEffectKind,
}

/// Behavior payload for `KitEffect`.
#[derive(Clone, Debug, Serialize)]
pub(crate) enum KitEffectKind {
    Projectile {
        ability_slot: u8,
        rank: u8,
        speed_per_tick: f32,
        radius: f32,
        damage: f32,
        school: DamageSchool,
        piercing: bool,
        returning: bool,
        return_to_caster: bool,
        repeat_mult: f32,
        splash_damage: f32,
        splash_radius: f32,
        stun_ticks: u64,
        root_ticks: u64,
        silence_ticks: u64,
        slow_pct: f32,
        slow_ticks: u64,
        minion_mult: f32,
        resets_cd_on_hit: bool,
        applies_mark: Option<(u8, u8, u64)>,
        destroys_projectiles: bool,
        hit_actors: Vec<u32>,
    },
    DelayedBlast {
        ability_slot: u8,
        release_tick: u64,
        radius: f32,
        inner_radius: f32,
        damage: f32,
        outer_mult: f32,
        school: DamageSchool,
        stun_ticks: u64,
        root_ticks: u64,
        sleep_ticks: u64,
        sleep_unwaking: bool,
        slow_pct: f32,
        slow_ticks: u64,
        reveal_ticks: u64,
        bonus_mark: Option<(u8, f32)>,
        minion_mult: f32,
        applies_mark: Option<(u8, u8, u64)>,
    },
    GroundZone {
        ability_slot: u8,
        zone_kind: u8,
        radius: f32,
        ticks_interval: u64,
        next_tick: u64,
        end_tick: u64,
        tick_damage: f32,
        tick_heal: f32,
        school: DamageSchool,
        slow_pct: f32,
        ally_speed_bonus: f32,
        end_damage: f32,
        end_root_ticks: u64,
        end_stun_ticks: u64,
        drift_dx: f32,
        drift_dy: f32,
        follow_caster: bool,
        ignited: bool,
        minion_mult: f32,
        applies_mark: Option<(u8, u8, u64)>,
    },
    TrapZone {
        ability_slot: u8,
        armed_at: u64,
        end_tick: u64,
        trigger_radius: f32,
        blast_radius: f32,
        damage: f32,
        school: DamageSchool,
        root_ticks: u64,
        stun_ticks: u64,
        slow_pct: f32,
        reveals: bool,
        detonate_on_caster_damage: bool,
    },
    SegmentBarrier {
        ability_slot: u8,
        armed_at: u64,
        end_tick: u64,
        end_pos: Position,
        radius: f32,
        is_ring_arena: bool,
        dps_or_hit_damage: f32,
        school: DamageSchool,
        stun_ticks: u64,
        silence_ticks: u64,
        slow_pct: f32,
        next_tick: u64,
        struck_recently: Vec<(u32, u64)>,
    },
    DashMotion {
        ability_slot: u8,
        start: Position,
        start_tick: u64,
        end_tick: u64,
        invuln: bool,
        unstoppable: bool,
        path_radius: f32,
        path_damage: f32,
        arrival_radius: f32,
        arrival_damage: f32,
        school: DamageSchool,
        slow_pct: f32,
        slow_ticks: u64,
        stun_ticks: u64,
        knockback_dist: f32,
        applies_mark: Option<(u8, u8, u64)>,
        hit_actors: Vec<u32>,
    },
    ChannelBeam {
        ability_slot: u8,
        target_id: u32,
        ticks_interval: u64,
        next_tick: u64,
        end_tick: u64,
        range: f32,
        width: f32,
        tick_damage: f32,
        tick_heal: f32,
        school: DamageSchool,
        lock_caster: bool,
        slow_pct: f32,
        stun_target: bool,
        global_all_heroes: bool,
        shield_pierce: f32,
        structure_mult: f32,
        minion_mult: f32,
    },
    SummonPet {
        ability_slot: u8,
        pet_kind: u8,
        hp: f32,
        hit_ticks_remaining: u8,
        damage: f32,
        school: DamageSchool,
        attack_interval: u64,
        next_attack: u64,
        end_tick: u64,
        applies_mark: Option<(u8, u8, u64)>,
        locked_target: u32,
    },
    Tether {
        ability_slot: u8,
        target_id: u32,
        anchor_pos: Position,
        break_range: f32,
        dps_pct_hp: f32,
        break_damage: f32,
        break_stun_ticks: u64,
        anchor_follows_caster: bool,
        next_tick: u64,
        end_tick: u64,
    },
    Attachment {
        ability_slot: u8,
        host_id: u32,
        is_devour: bool,
        dps: f32,
        end_damage: f32,
        end_radius: f32,
        school: DamageSchool,
        next_tick: u64,
        end_tick: u64,
    },
    DoT {
        ability_slot: u8,
        target_id: u32,
        tick_damage: f32,
        school: DamageSchool,
        ticks_interval: u64,
        next_tick: u64,
        end_tick: u64,
        minion_mult: f32,
    },
}

/// Convert ability index `0/1/2` back to `AbilitySlot`.
pub(crate) const fn slot_from_idx(idx: u8) -> AbilitySlot {
    match idx {
        0 => AbilitySlot::A,
        1 => AbilitySlot::B,
        _ => AbilitySlot::C,
    }
}

/// Convert `AbilitySlot` to array index `0..3`.
#[allow(dead_code)]
pub(crate) const fn slot_idx(slot: AbilitySlot) -> usize {
    match slot {
        AbilitySlot::A => 0,
        AbilitySlot::B => 1,
        AbilitySlot::C => 2,
    }
}

/// Convert seconds into deterministic 20Hz simulation ticks (`0` for `<= 0`).
pub(crate) fn seconds_to_ticks(sec: f32) -> u64 {
    if sec <= 0.0 {
        0
    } else {
        (sec * TICK_HZ as f32).round().max(1.0) as u64
    }
}

fn clamp_map_pos(p: Position) -> Position {
    Position {
        x: p.x.clamp(-120.0, 120.0),
        y: p.y.clamp(-80.0, 80.0),
    }
}

fn dir_between(from: Position, to: Position) -> (f32, f32) {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let d = dx.hypot(dy);
    if d <= 0.0001 {
        (1.0, 0.0)
    } else {
        (dx / d, dy / d)
    }
}

fn point_to_segment_dist(p: Position, a: Position, b: Position) -> f32 {
    let abx = b.x - a.x;
    let aby = b.y - a.y;
    let len_sq = abx * abx + aby * aby;
    if len_sq <= 1e-6 {
        return p.dist(a);
    }
    let t = (((p.x - a.x) * abx + (p.y - a.y) * aby) / len_sq).clamp(0.0, 1.0);
    let proj = Position {
        x: a.x + abx * t,
        y: a.y + aby * t,
    };
    p.dist(proj)
}

impl TimedSpell for KitEffect {
    fn expired_before_step(&self, tick: u64) -> bool {
        match &self.kind {
            KitEffectKind::GroundZone { end_tick, .. }
            | KitEffectKind::TrapZone { end_tick, .. }
            | KitEffectKind::SegmentBarrier { end_tick, .. }
            | KitEffectKind::ChannelBeam { end_tick, .. }
            | KitEffectKind::SummonPet { end_tick, .. }
            | KitEffectKind::Tether { end_tick, .. }
            | KitEffectKind::Attachment { end_tick, .. }
            | KitEffectKind::DoT { end_tick, .. } => tick > *end_tick,
            KitEffectKind::DelayedBlast { .. }
            | KitEffectKind::Projectile { .. }
            | KitEffectKind::DashMotion { .. } => false,
        }
    }

    fn begin_impact(&mut self) -> Self {
        self.clone()
    }

    fn advance(&mut self, world: &mut Room<'_>, events: &mut Vec<Event>) -> bool {
        let Some(owner_idx) = world.index(self.owner_id) else {
            return false;
        };
        let tick = world.tick;
        let owner_alive = world.entities[owner_idx].hp > 0.;
        if !owner_alive
            && !matches!(
                self.kind,
                KitEffectKind::DelayedBlast { .. } | KitEffectKind::DoT { .. }
            )
        {
            return false;
        }

        match &mut self.kind {
            KitEffectKind::DelayedBlast {
                ability_slot,
                release_tick,
                radius,
                inner_radius,
                damage,
                outer_mult,
                school,
                stun_ticks,
                root_ticks,
                sleep_ticks,
                sleep_unwaking,
                slow_pct,
                slow_ticks,
                reveal_ticks,
                bonus_mark,
                minion_mult,
                applies_mark,
            } => {
                if tick < *release_tick {
                    return true;
                }
                let center = self.position;
                let mut ctx = HeroKitContext::new(world, events, owner_idx);
                let hits = ctx.enemies_in_radius(center, *radius, false);
                for j in hits {
                    let dist = ctx.room.entities[j].position.dist(center);
                    let mut raw = *damage;
                    if *inner_radius > 0.0 && dist > *inner_radius {
                        raw *= *outer_mult;
                    }
                    if let Some((mark_id, mult)) = bonus_mark
                        && ctx.mark_stacks(j, *mark_id) > 0
                    {
                        raw *= 1.0 + *mult;
                    }
                    if !matches!(ctx.room.entities[j].kind, Kind::Hero) {
                        raw *= *minion_mult;
                    }
                    let slot = slot_from_idx(*ability_slot);
                    let mult = ctx.talent_dmg_mult(slot);
                    ctx.deal_damage(j, raw * mult, *school, 0.0, false);
                    if *stun_ticks > 0 {
                        ctx.apply_stun_ticks(j, *stun_ticks);
                    }
                    if *root_ticks > 0 {
                        ctx.apply_root_ticks(j, *root_ticks);
                    }
                    if *sleep_ticks > 0 {
                        ctx.apply_sleep_ticks(j, *sleep_ticks, *sleep_unwaking);
                    }
                    if *slow_ticks > 0 && *slow_pct > 0.0 {
                        ctx.apply_slow_ticks(j, *slow_pct, *slow_ticks);
                    }
                    if *reveal_ticks > 0 {
                        let until = tick + *reveal_ticks;
                        ctx.room.entities[j].status.reveal_until =
                            ctx.room.entities[j].status.reveal_until.max(until);
                    }
                    if let Some((m, add, dur_ticks)) = applies_mark {
                        ctx.add_mark_ticks(j, *m, *add, 10, *dur_ticks);
                    }
                }
                false
            }
            KitEffectKind::Projectile {
                ability_slot,
                rank: _,
                speed_per_tick,
                radius,
                damage,
                school,
                piercing,
                returning,
                return_to_caster,
                repeat_mult,
                splash_damage,
                splash_radius,
                stun_ticks,
                root_ticks,
                silence_ticks,
                slow_pct,
                slow_ticks,
                minion_mult,
                resets_cd_on_hit,
                applies_mark,
                destroys_projectiles: _,
                hit_actors,
            } => {
                if *returning {
                    self.goal = world.entities[owner_idx].position;
                }
                let prev = self.position;
                let next = prev.advance(self.goal, *speed_per_tick);
                self.position = next;

                let mut hit_targets = Vec::new();
                for (j, e) in world.entities.iter().enumerate() {
                    if e.team == self.team
                        || e.hp <= 0.0
                        || !matches!(e.kind, Kind::Hero | Kind::Minion | Kind::Neutral)
                    {
                        continue;
                    }
                    if hit_actors.contains(&e.id) {
                        continue;
                    }
                    if point_to_segment_dist(e.position, prev, next) <= *radius + 0.55 {
                        hit_targets.push(j);
                    }
                }

                let mut stop_now = false;
                if !hit_targets.is_empty() {
                    let slot = slot_from_idx(*ability_slot);
                    let mut ctx = HeroKitContext::new(world, events, owner_idx);
                    let dmg_mult = ctx.talent_dmg_mult(slot);
                    for j in hit_targets {
                        let target_id = ctx.room.entities[j].id;
                        hit_actors.push(target_id);
                        let is_hero = ctx.room.entities[j].kind == Kind::Hero;
                        let mut raw = *damage * dmg_mult;
                        if !is_hero {
                            raw *= *minion_mult;
                        }
                        if hit_actors.len() > 1 {
                            raw *= *repeat_mult;
                        }
                        ctx.deal_damage(j, raw, *school, 0.0, false);
                        if *stun_ticks > 0 && is_hero {
                            ctx.apply_stun_ticks(j, *stun_ticks);
                        }
                        if *root_ticks > 0 {
                            ctx.apply_root_ticks(j, *root_ticks);
                        }
                        if *silence_ticks > 0 {
                            ctx.apply_silence_ticks(j, *silence_ticks);
                        }
                        if *slow_ticks > 0 && *slow_pct > 0.0 {
                            ctx.apply_slow_ticks(j, *slow_pct, *slow_ticks);
                        }
                        if let Some((m, add, dur_ticks)) = applies_mark {
                            ctx.add_mark_ticks(j, *m, *add, 10, *dur_ticks);
                        }
                        if *resets_cd_on_hit && is_hero {
                            ctx.reset_slot_cd(slot);
                        }
                        if *splash_damage > 0.0 {
                            let splash_pos = ctx.room.entities[j].position;
                            let splash_hits =
                                ctx.enemies_in_radius(splash_pos, *splash_radius, false);
                            for sj in splash_hits {
                                if sj != j {
                                    ctx.deal_damage(
                                        sj,
                                        *splash_damage * dmg_mult,
                                        *school,
                                        0.0,
                                        false,
                                    );
                                }
                            }
                        }
                        if !*piercing && is_hero {
                            stop_now = true;
                            break;
                        }
                    }
                }
                if stop_now || next.dist(self.goal) <= 0.05 {
                    if *return_to_caster && !*returning {
                        *returning = true;
                        hit_actors.clear();
                        return true;
                    }
                    return false;
                }
                true
            }
            KitEffectKind::GroundZone {
                ability_slot,
                zone_kind: _,
                radius,
                ticks_interval,
                next_tick,
                end_tick,
                tick_damage,
                tick_heal,
                school,
                slow_pct,
                ally_speed_bonus,
                end_damage,
                end_root_ticks,
                end_stun_ticks,
                drift_dx,
                drift_dy,
                follow_caster,
                ignited,
                minion_mult,
                applies_mark,
            } => {
                if *follow_caster {
                    self.position = world.entities[owner_idx].position;
                } else if *drift_dx != 0.0 || *drift_dy != 0.0 {
                    self.position = clamp_map_pos(Position {
                        x: self.position.x + *drift_dx,
                        y: self.position.y + *drift_dy,
                    });
                }
                let center = self.position;
                let slot = slot_from_idx(*ability_slot);
                let mut ctx = HeroKitContext::new(world, events, owner_idx);
                if *ally_speed_bonus > 0.0 {
                    let allies = ctx.allies_in_radius(center, *radius, true, true);
                    for a in allies {
                        ctx.add_buff_ticks(a, BuffStatKind::MoveSpeedFlat, *ally_speed_bonus, 3);
                    }
                }
                if *slow_pct > 0.0 {
                    let enemies = ctx.enemies_in_radius(center, *radius, false);
                    for e in enemies {
                        ctx.apply_slow_ticks(e, *slow_pct, 3);
                    }
                }
                if tick >= *next_tick {
                    *next_tick = tick + (*ticks_interval).max(1);
                    let per_tick = (*tick_damage)
                        * ((*ticks_interval).max(1) as f32 / TICK_HZ as f32)
                        * ctx.talent_dmg_mult(slot);
                    if per_tick > 0.0 || *ignited {
                        let eff_dmg = if *ignited && per_tick <= 0.0 {
                            40.0
                        } else {
                            per_tick
                        };
                        let enemies = ctx.enemies_in_radius(center, *radius, false);
                        for e in enemies {
                            let is_hero = ctx.room.entities[e].kind == Kind::Hero;
                            let d = if is_hero {
                                eff_dmg
                            } else {
                                eff_dmg * *minion_mult
                            };
                            ctx.deal_damage(e, d, *school, 0.0, false);
                            if let Some((m, add, dur)) = applies_mark {
                                ctx.add_mark_ticks(e, *m, *add, 10, *dur);
                            }
                        }
                    }
                    if *tick_heal > 0.0 {
                        let heal_amt = (*tick_heal)
                            * ((*ticks_interval).max(1) as f32 / TICK_HZ as f32)
                            * ctx.talent_heal_mult(slot);
                        let allies = ctx.allies_in_radius(center, *radius, true, true);
                        for a in allies {
                            ctx.heal_unit(a, heal_amt);
                        }
                    }
                }
                if tick >= *end_tick {
                    if *end_damage > 0.0 || *end_root_ticks > 0 || *end_stun_ticks > 0 {
                        let enemies = ctx.enemies_in_radius(center, *radius, false);
                        let dmg = *end_damage * ctx.talent_dmg_mult(slot);
                        for e in enemies {
                            if dmg > 0.0 {
                                let m = if ctx.room.entities[e].kind == Kind::Hero {
                                    1.0
                                } else {
                                    *minion_mult
                                };
                                ctx.deal_damage(e, dmg * m, *school, 0.0, false);
                            }
                            if *end_root_ticks > 0 {
                                ctx.apply_root_ticks(e, *end_root_ticks);
                            }
                            if *end_stun_ticks > 0 {
                                ctx.apply_stun_ticks(e, *end_stun_ticks);
                            }
                        }
                    }
                    return false;
                }
                true
            }
            KitEffectKind::TrapZone {
                ability_slot,
                armed_at,
                end_tick,
                trigger_radius,
                blast_radius,
                damage,
                school,
                root_ticks,
                stun_ticks,
                slow_pct,
                reveals,
                detonate_on_caster_damage: _,
            } => {
                if tick >= *end_tick {
                    return false;
                }
                if tick < *armed_at {
                    return true;
                }
                let center = self.position;
                let triggered = world.entities.iter().any(|e| {
                    e.team != self.team
                        && e.hp > 0.0
                        && e.kind == Kind::Hero
                        && e.position.dist(center) <= *trigger_radius
                });
                if !triggered {
                    return true;
                }
                let slot = slot_from_idx(*ability_slot);
                let mut ctx = HeroKitContext::new(world, events, owner_idx);
                let dmg = *damage * ctx.talent_dmg_mult(slot);
                let enemies = ctx.enemies_in_radius(center, *blast_radius, false);
                for e in enemies {
                    ctx.deal_damage(e, dmg, *school, 0.0, false);
                    if *root_ticks > 0 {
                        ctx.apply_root_ticks(e, *root_ticks);
                    }
                    if *stun_ticks > 0 {
                        ctx.apply_stun_ticks(e, *stun_ticks);
                    }
                    if *slow_pct > 0.0 {
                        ctx.apply_slow_ticks(e, *slow_pct, TICK_HZ * 2);
                    }
                    if *reveals {
                        ctx.room.entities[e].status.reveal_until = ctx.room.entities[e]
                            .status
                            .reveal_until
                            .max(tick + TICK_HZ * 3);
                    }
                }
                false
            }
            KitEffectKind::SegmentBarrier {
                ability_slot,
                armed_at,
                end_tick,
                end_pos,
                radius,
                is_ring_arena,
                dps_or_hit_damage,
                school,
                stun_ticks,
                silence_ticks,
                slow_pct,
                next_tick,
                struck_recently,
            } => {
                if tick >= *end_tick {
                    return false;
                }
                if *is_ring_arena
                    && world.entities[owner_idx].position.dist(self.position) > *radius + 0.5
                {
                    // Ardan leaves Gauntlet perimeter -> barrier immediately collapses.
                    return false;
                }
                if tick < *armed_at {
                    return true;
                }
                struck_recently.retain(|(_, until)| *until > tick);
                let start_pos = self.position;
                let end_p = *end_pos;
                let rad = *radius;
                let ring = *is_ring_arena;
                let slot = slot_from_idx(*ability_slot);
                let mut ctx = HeroKitContext::new(world, events, owner_idx);
                let mut crossed = Vec::new();
                for (j, e) in ctx.room.entities.iter().enumerate() {
                    if e.team == self.team || e.hp <= 0.0 || e.kind != Kind::Hero {
                        continue;
                    }
                    if struck_recently.iter().any(|(id, _)| *id == e.id) {
                        continue;
                    }
                    let hit = if ring {
                        let d = e.position.dist(start_pos);
                        (d - rad).abs() <= 0.9
                    } else {
                        point_to_segment_dist(e.position, start_pos, end_p) <= 0.9
                    };
                    if hit {
                        crossed.push(j);
                    }
                }
                let do_dmg_tick = tick >= *next_tick;
                if do_dmg_tick {
                    *next_tick = tick + TICK_HZ / 2;
                }
                for j in crossed {
                    let eid = ctx.room.entities[j].id;
                    if *dps_or_hit_damage > 0.0 && (*stun_ticks > 0 || do_dmg_tick) {
                        let dmg = *dps_or_hit_damage * ctx.talent_dmg_mult(slot);
                        ctx.deal_damage(j, dmg, *school, 0.0, false);
                    }
                    if *stun_ticks > 0 {
                        ctx.apply_stun_ticks(j, *stun_ticks);
                        struck_recently.push((eid, tick + TICK_HZ * 2));
                    }
                    if *silence_ticks > 0 {
                        ctx.apply_silence_ticks(j, *silence_ticks);
                        struck_recently.push((eid, tick + TICK_HZ));
                    }
                    if *slow_pct > 0.0 {
                        ctx.apply_slow_ticks(j, *slow_pct, TICK_HZ);
                    }
                }
                true
            }
            KitEffectKind::DashMotion {
                ability_slot,
                start,
                start_tick,
                end_tick,
                invuln,
                unstoppable,
                path_radius,
                path_damage,
                arrival_radius,
                arrival_damage,
                school,
                slow_pct,
                slow_ticks,
                stun_ticks,
                knockback_dist,
                applies_mark,
                hit_actors,
            } => {
                let total = (*end_tick).saturating_sub(*start_tick).max(1);
                let elapsed = tick.saturating_sub(*start_tick).min(total);
                let frac = elapsed as f32 / total as f32;
                let target_pos = Position {
                    x: start.x + (self.goal.x - start.x) * frac,
                    y: start.y + (self.goal.y - start.y) * frac,
                };
                if world.spell_motion_clear(world.entities[owner_idx].position, target_pos) {
                    world.place_spell_actor(owner_idx, target_pos, events);
                }
                let cur_pos = world.entities[owner_idx].position;
                let slot = slot_from_idx(*ability_slot);
                let mut ctx = HeroKitContext::new(world, events, owner_idx);
                if *invuln {
                    ctx.set_untargetable(owner_idx, 0.15);
                }
                if *unstoppable {
                    ctx.set_unstoppable(owner_idx, 0.15);
                }
                if *path_damage > 0.0 {
                    let mult = ctx.talent_dmg_mult(slot);
                    let enemies = ctx.enemies_in_radius(cur_pos, *path_radius, false);
                    for j in enemies {
                        let id = ctx.room.entities[j].id;
                        if !hit_actors.contains(&id) {
                            hit_actors.push(id);
                            ctx.deal_damage(j, *path_damage * mult, *school, 0.0, false);
                            if *slow_ticks > 0 && *slow_pct > 0.0 {
                                ctx.apply_slow_ticks(j, *slow_pct, *slow_ticks);
                            }
                            if let Some((m, add, dur)) = applies_mark {
                                ctx.add_mark_ticks(j, *m, *add, 10, *dur);
                            }
                        }
                    }
                }
                if tick >= *end_tick {
                    if *arrival_damage > 0.0 || *stun_ticks > 0 || *knockback_dist > 0.0 {
                        let mult = ctx.talent_dmg_mult(slot);
                        let enemies = ctx.enemies_in_radius(cur_pos, *arrival_radius, false);
                        for j in enemies {
                            let id = ctx.room.entities[j].id;
                            if *arrival_damage > 0.0 && !hit_actors.contains(&id) {
                                hit_actors.push(id);
                                ctx.deal_damage(j, *arrival_damage * mult, *school, 0.0, false);
                            }
                            if *stun_ticks > 0 && ctx.room.entities[j].kind == Kind::Hero {
                                ctx.apply_stun_ticks(j, *stun_ticks);
                            }
                            if *slow_ticks > 0 && *slow_pct > 0.0 {
                                ctx.apply_slow_ticks(j, *slow_pct, *slow_ticks);
                            }
                            if *knockback_dist > 0.0 {
                                ctx.knockback(j, cur_pos, *knockback_dist, 0.0);
                            }
                        }
                    }
                    return false;
                }
                true
            }
            KitEffectKind::ChannelBeam {
                ability_slot,
                target_id,
                ticks_interval,
                next_tick,
                end_tick,
                range,
                width,
                tick_damage,
                tick_heal,
                school,
                lock_caster,
                slow_pct,
                stun_target,
                global_all_heroes,
                shield_pierce,
                structure_mult,
                minion_mult,
            } => {
                if tick >= *end_tick {
                    return false;
                }
                let slot = slot_from_idx(*ability_slot);
                if *lock_caster {
                    world.entities[owner_idx].order = crate::Command::Stop;
                }
                if tick < *next_tick {
                    return true;
                }
                *next_tick = tick + (*ticks_interval).max(1);
                let origin = world.entities[owner_idx].position;
                let end_pt = origin.advance(self.goal, *range);
                let mut ctx = HeroKitContext::new(world, events, owner_idx);
                let dmg_mult = ctx.talent_dmg_mult(slot);
                let per_tick_dmg =
                    (*tick_damage) * ((*ticks_interval).max(1) as f32 / TICK_HZ as f32) * dmg_mult;

                if *target_id != 0 {
                    if let Some(tj) = ctx.room.index(*target_id) {
                        if *tick_heal > 0.0 {
                            let heal_amt = (*tick_heal)
                                * ((*ticks_interval).max(1) as f32 / TICK_HZ as f32)
                                * ctx.talent_heal_mult(slot);
                            ctx.heal_unit(tj, heal_amt);
                        }
                        if per_tick_dmg > 0.0 {
                            ctx.deal_damage(tj, per_tick_dmg, *school, *shield_pierce, false);
                        }
                        if *stun_target {
                            if ctx.room.entities[tj].status.blocks_debuffs(tick) {
                                // Koshka C cancels if stun blocked by Reflex Block.
                                return false;
                            }
                            ctx.apply_stun_ticks(tj, (*ticks_interval).max(1) + 1);
                        }
                    } else {
                        return false;
                    }
                    return true;
                }

                if *global_all_heroes {
                    let targets: Vec<usize> = ctx
                        .room
                        .entities
                        .iter()
                        .enumerate()
                        .filter_map(|(j, e)| {
                            (e.team != self.team && e.hp > 0.0 && e.kind == Kind::Hero).then_some(j)
                        })
                        .collect();
                    for j in targets {
                        ctx.deal_damage(j, per_tick_dmg, *school, *shield_pierce, false);
                    }
                    return true;
                }

                let mut hits = Vec::new();
                for (j, e) in ctx.room.entities.iter().enumerate() {
                    if e.team == self.team || e.hp <= 0.0 {
                        continue;
                    }
                    if point_to_segment_dist(e.position, origin, end_pt) <= *width {
                        hits.push(j);
                    }
                }
                for j in hits {
                    let mult = match ctx.room.entities[j].kind {
                        Kind::Hero => 1.0,
                        Kind::Tower | Kind::Crystal => *structure_mult,
                        _ => *minion_mult,
                    };
                    if per_tick_dmg > 0.0 {
                        ctx.deal_damage(j, per_tick_dmg * mult, *school, *shield_pierce, false);
                    }
                    if *slow_pct > 0.0 {
                        ctx.apply_slow_ticks(j, *slow_pct, (*ticks_interval).max(1) + 2);
                    }
                }
                true
            }
            KitEffectKind::SummonPet {
                ability_slot: _,
                pet_kind: _,
                hp: _,
                hit_ticks_remaining,
                damage,
                school,
                attack_interval,
                next_attack,
                end_tick,
                applies_mark,
                locked_target,
            } => {
                if tick >= *end_tick || *hit_ticks_remaining == 0 {
                    return false;
                }
                let owner_pos = world.entities[owner_idx].position;
                if self.position.dist(owner_pos) > 22.0 {
                    // Pets despawn when straying too far from master.
                    return false;
                }
                let target_idx = if *locked_target != 0 {
                    world
                        .index(*locked_target)
                        .filter(|&j| world.entities[j].hp > 0.0)
                } else {
                    None
                }
                .or_else(|| {
                    world
                        .entities
                        .iter()
                        .enumerate()
                        .filter(|(_, e)| {
                            e.team != self.team
                                && e.hp > 0.0
                                && matches!(e.kind, Kind::Hero | Kind::Minion | Kind::Neutral)
                        })
                        .min_by(|(_, a), (_, b)| {
                            let ha = if a.kind == Kind::Hero { 0 } else { 1 };
                            let hb = if b.kind == Kind::Hero { 0 } else { 1 };
                            ha.cmp(&hb).then_with(|| {
                                a.position
                                    .dist(self.position)
                                    .total_cmp(&b.position.dist(self.position))
                            })
                        })
                        .map(|(j, _)| j)
                });
                if let Some(tj) = target_idx {
                    *locked_target = world.entities[tj].id;
                    let tpos = world.entities[tj].position;
                    self.position = self.position.advance(tpos, 0.35);
                    if self.position.dist(tpos) <= 2.2 && tick >= *next_attack {
                        *next_attack = tick + (*attack_interval).max(4);
                        let mut ctx = HeroKitContext::new(world, events, owner_idx);
                        ctx.deal_damage(tj, *damage, *school, 0.0, false);
                        if let Some((m, add, dur)) = applies_mark {
                            ctx.add_mark_ticks(tj, *m, *add, 10, *dur);
                        }
                    }
                } else {
                    self.position = self.position.advance(owner_pos, 0.3);
                }
                true
            }
            KitEffectKind::Tether {
                ability_slot: _,
                target_id,
                anchor_pos,
                break_range,
                dps_pct_hp,
                break_damage,
                break_stun_ticks,
                anchor_follows_caster,
                next_tick,
                end_tick,
            } => {
                if tick >= *end_tick {
                    return false;
                }
                let Some(tj) = world.index(*target_id) else {
                    return false;
                };
                if world.entities[tj].hp <= 0.0 {
                    return false;
                }
                if *anchor_follows_caster {
                    *anchor_pos = world.entities[owner_idx].position;
                }
                let dist = world.entities[tj].position.dist(*anchor_pos);
                if dist > *break_range {
                    let mut ctx = HeroKitContext::new(world, events, owner_idx);
                    if *break_damage > 0.0 {
                        ctx.deal_damage(tj, *break_damage, DamageSchool::Crystal, 0.0, false);
                    }
                    if *break_stun_ticks > 0 {
                        ctx.apply_stun_ticks(tj, *break_stun_ticks);
                    }
                    return false;
                }
                if *dps_pct_hp > 0.0 && tick >= *next_tick {
                    *next_tick = tick + TICK_HZ / 2;
                    let max_hp = world.entities[tj].max_hp;
                    let dmg = max_hp * *dps_pct_hp * 0.5;
                    let mut ctx = HeroKitContext::new(world, events, owner_idx);
                    ctx.deal_damage(tj, dmg, DamageSchool::Crystal, 0.0, false);
                }
                true
            }
            KitEffectKind::Attachment {
                ability_slot: _,
                host_id,
                is_devour,
                dps,
                end_damage,
                end_radius,
                school,
                next_tick,
                end_tick,
            } => {
                let Some(hj) = world.index(*host_id) else {
                    return false;
                };
                if world.entities[hj].hp <= 0.0 {
                    return false;
                }
                if *is_devour {
                    let belly_pos = world.entities[owner_idx].position;
                    world.place_spell_actor(hj, belly_pos, events);
                    let mut ctx = HeroKitContext::new(world, events, owner_idx);
                    ctx.apply_stun_ticks(hj, 2);
                    ctx.apply_silence_ticks(hj, 2);
                } else {
                    let host_pos = world.entities[hj].position;
                    world.place_spell_actor(owner_idx, host_pos, events);
                    let mut ctx = HeroKitContext::new(world, events, owner_idx);
                    ctx.set_untargetable(owner_idx, 0.15);
                }
                if *dps > 0.0 && tick >= *next_tick {
                    *next_tick = tick + TICK_HZ / 2;
                    let mut ctx = HeroKitContext::new(world, events, owner_idx);
                    ctx.deal_damage(hj, *dps * 0.5, *school, 0.0, false);
                }
                if tick >= *end_tick {
                    if *end_damage > 0.0 {
                        let center = world.entities[owner_idx].position;
                        let mut ctx = HeroKitContext::new(world, events, owner_idx);
                        for j in ctx.enemies_in_radius(center, (*end_radius).max(1.5), false) {
                            ctx.deal_damage(j, *end_damage, *school, 0.0, false);
                        }
                    }
                    return false;
                }
                true
            }
            KitEffectKind::DoT {
                ability_slot: _,
                target_id,
                tick_damage,
                school,
                ticks_interval,
                next_tick,
                end_tick,
                minion_mult,
            } => {
                if tick >= *end_tick {
                    return false;
                }
                if tick < *next_tick {
                    return true;
                }
                *next_tick = tick + (*ticks_interval).max(1);
                if let Some(tj) = world.index(*target_id)
                    && world.entities[tj].hp > 0.0
                {
                    let m = if world.entities[tj].kind == Kind::Hero {
                        1.0
                    } else {
                        *minion_mult
                    };
                    let mut ctx = HeroKitContext::new(world, events, owner_idx);
                    ctx.deal_damage(tj, *tick_damage * m, *school, 0.0, false);
                    return true;
                }
                false
            }
        }
    }
}

/// Safe, ergonomic runtime execution context handed to declarative primitives and custom callbacks.
pub(crate) struct HeroKitContext<'a, 'w> {
    pub room: &'a mut Room<'w>,
    pub events: &'a mut Vec<Event>,
    pub caster: usize,
    pub tick: u64,
}

impl<'a, 'w> HeroKitContext<'a, 'w> {
    pub(crate) fn new(room: &'a mut Room<'w>, events: &'a mut Vec<Event>, caster: usize) -> Self {
        let tick = room.tick;
        Self {
            room,
            events,
            caster,
            tick,
        }
    }

    pub(crate) fn caster_id(&self) -> u32 {
        self.room.entities[self.caster].id
    }

    pub(crate) fn caster_team(&self) -> u8 {
        self.room.entities[self.caster].team
    }

    pub(crate) fn caster_pos(&self) -> Position {
        self.room.entities[self.caster].position
    }

    pub(crate) fn caster_type(&self) -> u16 {
        self.room.entities[self.caster].type_id
    }

    pub(crate) fn spec(&self) -> Option<&'static HeroKitSpec> {
        crate::hero_kits::kit_spec(self.caster_type())
    }

    pub(crate) fn eval_inputs(&self) -> EvalInputs {
        let e = &self.room.entities[self.caster];
        let bonus = self.room.kit_bonus_stats(self.caster);
        let cp =
            e.item_stats.crystal + bonus.crystal_flat + e.item_stats.crystal * bonus.crystal_pct;
        let base_wp = e.stats.weapon;
        let wp = (base_wp + bonus.weapon_flat) * (1.0 + bonus.weapon_pct);
        let bwp = e.item_stats.bonus_weapon + bonus.weapon_flat + base_wp * bonus.weapon_pct;
        EvalInputs {
            cp,
            wp,
            bonus_wp: bwp.max(0.0),
            bonus_hp: e.bonus_hp,
            alt: (e.max_energy - 200.0).max(0.0),
            bonus_as: (e.item_stats.attack_speed + bonus.attack_speed).max(0.0),
        }
    }

    pub(crate) fn eval_var(&self, ability_id: &str, rank: u8, fallback: ValueSpec) -> f32 {
        let inputs = self.eval_inputs();
        if let Some(book) = &self.room.abilities.hero_kits {
            book.eval_value(self.caster_type(), ability_id, rank, fallback, inputs)
        } else {
            fallback.eval_fallback(rank, inputs)
        }
    }

    pub(crate) fn resolve_target_idx(&self, target: CastTarget) -> Option<usize> {
        match target {
            CastTarget::SelfCast => Some(self.caster),
            CastTarget::Entity(id) => self.room.index(id),
            CastTarget::Point(_) => None,
        }
    }

    pub(crate) fn resolve_target_pos(&self, target: CastTarget, max_range: f32) -> Position {
        let origin = self.caster_pos();
        let raw = match target {
            CastTarget::SelfCast => origin,
            CastTarget::Entity(id) => self
                .room
                .index(id)
                .map_or(origin, |j| self.room.entities[j].position),
            CastTarget::Point(p) => p,
        };
        if max_range <= 0.0 || origin.dist(raw) <= max_range {
            clamp_map_pos(raw)
        } else {
            let (dx, dy) = dir_between(origin, raw);
            clamp_map_pos(Position {
                x: origin.x + dx * max_range,
                y: origin.y + dy * max_range,
            })
        }
    }

    pub(crate) fn active_talent(&self) -> Option<&ActiveTalentState> {
        self.room.entities[self.caster]
            .hero_kit
            .as_deref()
            .and_then(|k| k.talent.as_ref())
    }

    pub(crate) fn has_talent(&self, id: &str) -> bool {
        self.active_talent().is_some_and(|t| t.id == id)
    }

    pub(crate) fn talent_special(&self) -> TalentSpecial {
        self.active_talent()
            .and_then(|t| find_talent(&t.id))
            .map_or(TalentSpecial::None, |s| s.special)
    }

    pub(crate) fn talent_mod(&self, label: &str) -> Option<f32> {
        let t = self.active_talent()?;
        let inputs = self.eval_inputs();
        let loaded = self.room.abilities.hero_kits.as_ref().map(|b| &b.talents);
        talents_runtime::eval_talent_mod(&t.id, label, t.level, inputs.cp, inputs.wp, loaded)
    }

    pub(crate) fn talent_dmg_mult(&self, slot: AbilitySlot) -> f32 {
        let Some(t) = self.active_talent() else {
            return 1.0;
        };
        let inputs = self.eval_inputs();
        let loaded = self.room.abilities.hero_kits.as_ref().map(|b| &b.talents);
        talents_runtime::talent_damage_multiplier(t, slot, inputs.cp, inputs.wp, loaded)
    }

    pub(crate) fn talent_barrier_mult(&self, slot: AbilitySlot) -> f32 {
        let Some(t) = self.active_talent() else {
            return 1.0;
        };
        let inputs = self.eval_inputs();
        let loaded = self.room.abilities.hero_kits.as_ref().map(|b| &b.talents);
        talents_runtime::talent_barrier_multiplier(t, slot, inputs.cp, inputs.wp, loaded)
    }

    pub(crate) fn talent_heal_mult(&self, slot: AbilitySlot) -> f32 {
        let Some(t) = self.active_talent() else {
            return 1.0;
        };
        let inputs = self.eval_inputs();
        let loaded = self.room.abilities.hero_kits.as_ref().map(|b| &b.talents);
        talents_runtime::talent_heal_multiplier(t, slot, inputs.cp, inputs.wp, loaded)
    }

    pub(crate) fn deal_damage(
        &mut self,
        target_idx: usize,
        raw: f32,
        school: DamageSchool,
        extra_pierce: f32,
        triggers_basic: bool,
    ) {
        if raw <= 0.0 || target_idx >= self.room.entities.len() {
            return;
        }
        if self.room.entities[target_idx]
            .hero_kit
            .as_deref()
            .is_some_and(|k| self.tick < k.untargetable_until)
        {
            return;
        }
        let src_id = self.caster_id();
        let team = self.caster_team();
        let caster_bonus = self.room.kit_bonus_stats(self.caster);
        let target_bonus = self.room.kit_bonus_stats(target_idx);
        let target = &self.room.entities[target_idx];
        let caster = &self.room.entities[self.caster];
        let amp = (1.0 + caster_bonus.damage_amp + target_bonus.damage_amp).max(0.1);
        let effective_raw = raw * amp;
        let mitigated = match school {
            DamageSchool::Weapon => {
                let defense =
                    (target.stats.armor + target_bonus.armor_flat) * (1.0 + target_bonus.armor_pct);
                let pierce =
                    (caster.item_stats.armor_pierce + caster_bonus.armor_pierce + extra_pierce)
                        .clamp(0.0, 1.0);
                shop::mitigated(effective_raw, defense, pierce)
            }
            DamageSchool::Crystal => {
                let defense = (target.stats.shield + target_bonus.shield_flat)
                    * (1.0 + target_bonus.shield_pct);
                let pierce =
                    (caster.item_stats.shield_pierce + caster_bonus.shield_pierce + extra_pierce)
                        .clamp(0.0, 1.0);
                shop::mitigated(effective_raw, defense, pierce)
            }
            DamageSchool::True => effective_raw,
        };
        let hp_before = self.room.entities[target_idx].hp;
        self.room
            .hurt(src_id, team, target_idx, mitigated, school, self.events);
        let actual_dmg = (hp_before - self.room.entities[target_idx].hp).max(0.0);
        if caster_bonus.lifesteal > 0.0 && actual_dmg > 0.0 {
            self.heal_unit(self.caster, actual_dmg * caster_bonus.lifesteal);
        }
        if let Some(spec) = self.spec()
            && let Some(cb) = spec.perk.on_ability_damage
        {
            cb(self, target_idx, mitigated);
        }
        if triggers_basic {
            let _ = self
                .room
                .item_stats_trigger_basic_placeholder(self.caster, target_idx);
        }
    }

    pub(crate) fn heal_unit(&mut self, target_idx: usize, amount: f32) {
        if amount <= 0.0 || target_idx >= self.room.entities.len() {
            return;
        }
        let mut adjusted = amount;
        if let Some(spec) = crate::hero_kits::kit_spec(self.room.entities[target_idx].type_id) {
            let lvl = f32::from(self.room.entities[target_idx].level.saturating_sub(1));
            adjusted *= spec.perk.healing_received_mult_base
                + lvl * spec.perk.healing_received_mult_per_level;
        }
        if self.room.entities[target_idx]
            .hero_kit
            .as_deref()
            .is_some_and(|k| self.tick < k.mortal_wound_until)
        {
            adjusted *= 0.67;
        }
        let src_id = self.caster_id();
        self.room.heal(src_id, target_idx, adjusted, self.events);
    }

    pub(crate) fn grant_barrier(
        &mut self,
        target_idx: usize,
        amount: f32,
        duration_sec: f32,
        stack_cap: Option<(f32, u8)>,
    ) {
        if amount <= 0.0 || target_idx >= self.room.entities.len() {
            return;
        }
        let mut scaled = amount;
        if let Some(spec) = crate::hero_kits::kit_spec(self.room.entities[target_idx].type_id) {
            let lvl = f32::from(self.room.entities[target_idx].level.saturating_sub(1));
            scaled *= spec.perk.healing_received_mult_base
                + lvl * spec.perk.healing_received_mult_per_level;
        }
        let until = self.tick + seconds_to_ticks(duration_sec).max(1);
        let t = &mut self.room.entities[target_idx];
        let prev = if t.status.barrier_until > self.tick {
            t.status.barrier
        } else {
            0.0
        };
        if let Some((cap_mult, _)) = stack_cap {
            t.status.barrier = (prev + scaled).min(scaled * cap_mult);
        } else {
            t.status.barrier = prev.max(scaled);
        }
        t.status.barrier_until = t.status.barrier_until.max(until);
    }

    pub(crate) fn grant_fortified(&mut self, target_idx: usize, amount: f32, duration_sec: f32) {
        if amount <= 0.0 || target_idx >= self.room.entities.len() {
            return;
        }
        let mut scaled = amount;
        if let Some(spec) = crate::hero_kits::kit_spec(self.room.entities[target_idx].type_id) {
            let lvl = f32::from(self.room.entities[target_idx].level.saturating_sub(1));
            scaled *= spec.perk.healing_received_mult_base
                + lvl * spec.perk.healing_received_mult_per_level;
        }
        let until = self.tick + seconds_to_ticks(duration_sec).max(1);
        let state = self.room.entities[target_idx]
            .hero_kit
            .get_or_insert_with(Box::default);
        let prev = if self.tick < state.fortified_until {
            state.fortified_health
        } else {
            0.0
        };
        state.fortified_health = prev.max(scaled);
        state.fortified_until = state.fortified_until.max(until);
    }

    pub(crate) fn target_blocks_debuffs(&self, target_idx: usize) -> bool {
        let e = &self.room.entities[target_idx];
        e.status.blocks_debuffs(self.tick)
            || e.hero_kit.as_deref().is_some_and(|k| {
                self.tick < k.unstoppable_until || self.tick < k.untargetable_until
            })
    }

    pub(crate) fn apply_stun(&mut self, target_idx: usize, sec: f32) {
        self.apply_stun_ticks(target_idx, seconds_to_ticks(sec));
    }

    pub(crate) fn apply_stun_ticks(&mut self, target_idx: usize, dur: u64) {
        if dur == 0
            || target_idx >= self.room.entities.len()
            || self.target_blocks_debuffs(target_idx)
        {
            return;
        }
        let phinn = crate::hero_kits::kit_spec(self.room.entities[target_idx].type_id)
            .is_some_and(|s| s.perk.unstoppable_cc_downgrade);
        let t = &mut self.room.entities[target_idx];
        if phinn {
            t.status.silence_until = t.status.silence_until.max(self.tick + dur);
        } else {
            t.status.stun_until = t.status.stun_until.max(self.tick + dur);
            t.cast = None;
        }
    }

    pub(crate) fn apply_slow(&mut self, target_idx: usize, factor: f32, sec: f32) {
        self.apply_slow_ticks(target_idx, factor, seconds_to_ticks(sec));
    }

    pub(crate) fn apply_slow_ticks(&mut self, target_idx: usize, slow_pct: f32, dur: u64) {
        if dur == 0 || slow_pct <= 0.0 || target_idx >= self.room.entities.len() {
            return;
        }
        let e = &self.room.entities[target_idx];
        if self.target_blocks_debuffs(target_idx) || e.status.blocks_movement_impairment(self.tick)
        {
            return;
        }
        let phinn =
            crate::hero_kits::kit_spec(e.type_id).is_some_and(|s| s.perk.unstoppable_cc_downgrade);
        let eff_slow = if phinn {
            (slow_pct * 0.5).min(0.4)
        } else {
            slow_pct.clamp(0.05, 0.85)
        };
        let speed_factor = (1.0 - eff_slow).clamp(0.15, 0.95);
        let t = &mut self.room.entities[target_idx];
        let existing_factor = if self.tick < t.status.slow_until {
            t.status.slow_factor.min(speed_factor)
        } else {
            speed_factor
        };
        t.status.slow_until = t.status.slow_until.max(self.tick + dur);
        t.status.slow_factor = existing_factor;
    }

    pub(crate) fn apply_root(&mut self, target_idx: usize, sec: f32) {
        self.apply_root_ticks(target_idx, seconds_to_ticks(sec));
    }

    pub(crate) fn apply_root_ticks(&mut self, target_idx: usize, dur: u64) {
        if dur == 0
            || target_idx >= self.room.entities.len()
            || self.target_blocks_debuffs(target_idx)
        {
            return;
        }
        let e = &self.room.entities[target_idx];
        if e.status.blocks_movement_impairment(self.tick) {
            return;
        }
        let phinn =
            crate::hero_kits::kit_spec(e.type_id).is_some_and(|s| s.perk.unstoppable_cc_downgrade);
        let t = &mut self.room.entities[target_idx];
        if phinn {
            t.status.slow_until = t.status.slow_until.max(self.tick + dur);
            t.status.slow_factor = t.status.slow_factor.max(0.35);
        } else {
            t.status.root_until = t.status.root_until.max(self.tick + dur);
        }
    }

    pub(crate) fn apply_silence(&mut self, target_idx: usize, sec: f32) {
        self.apply_silence_ticks(target_idx, seconds_to_ticks(sec));
    }

    pub(crate) fn apply_silence_ticks(&mut self, target_idx: usize, dur: u64) {
        if dur == 0
            || target_idx >= self.room.entities.len()
            || self.target_blocks_debuffs(target_idx)
        {
            return;
        }
        let t = &mut self.room.entities[target_idx];
        t.status.silence_until = t.status.silence_until.max(self.tick + dur);
    }

    pub(crate) fn apply_fear(&mut self, target_idx: usize, sec: f32) {
        let dur = seconds_to_ticks(sec);
        if dur == 0
            || target_idx >= self.room.entities.len()
            || self.target_blocks_debuffs(target_idx)
        {
            return;
        }
        if crate::hero_kits::kit_spec(self.room.entities[target_idx].type_id)
            .is_some_and(|s| s.perk.unstoppable_cc_downgrade)
        {
            self.apply_slow_ticks(target_idx, 0.35, dur);
            return;
        }
        let src = self.caster_id();
        let until = self.tick + dur;
        let state = self.room.entities[target_idx]
            .hero_kit
            .get_or_insert_with(Box::default);
        state.fear_until = state.fear_until.max(until);
        state.fear_source = src;
    }

    pub(crate) fn apply_sleep_ticks(&mut self, target_idx: usize, dur: u64, unwaking: bool) {
        if dur == 0
            || target_idx >= self.room.entities.len()
            || self.target_blocks_debuffs(target_idx)
        {
            return;
        }
        if crate::hero_kits::kit_spec(self.room.entities[target_idx].type_id)
            .is_some_and(|s| s.perk.unstoppable_cc_downgrade)
        {
            self.apply_silence_ticks(target_idx, dur);
            return;
        }
        let until = self.tick + dur;
        let state = self.room.entities[target_idx]
            .hero_kit
            .get_or_insert_with(Box::default);
        state.sleep_until = state.sleep_until.max(until);
        state.sleep_unwaking |= unwaking;
    }

    pub(crate) fn apply_taunt(&mut self, target_idx: usize, sec: f32) {
        let dur = seconds_to_ticks(sec);
        if dur == 0
            || target_idx >= self.room.entities.len()
            || self.target_blocks_debuffs(target_idx)
        {
            return;
        }
        let src = self.caster_id();
        let until = self.tick + dur;
        let state = self.room.entities[target_idx]
            .hero_kit
            .get_or_insert_with(Box::default);
        state.taunt_until = state.taunt_until.max(until);
        state.taunt_target = src;
    }

    pub(crate) fn apply_mortal_wound(&mut self, target_idx: usize, sec: f32) {
        let dur = seconds_to_ticks(sec);
        if dur == 0 || target_idx >= self.room.entities.len() {
            return;
        }
        let until = self.tick + dur;
        let state = self.room.entities[target_idx]
            .hero_kit
            .get_or_insert_with(Box::default);
        state.mortal_wound_until = state.mortal_wound_until.max(until);
    }

    pub(crate) fn apply_reveal(&mut self, target_idx: usize, sec: f32) {
        let dur = seconds_to_ticks(sec);
        if dur == 0 || target_idx >= self.room.entities.len() {
            return;
        }
        let until = self.tick + dur;
        self.room.entities[target_idx].status.reveal_until = self.room.entities[target_idx]
            .status
            .reveal_until
            .max(until);
    }

    pub(crate) fn cleanse_cc(&mut self, target_idx: usize) {
        if target_idx >= self.room.entities.len() {
            return;
        }
        let t = &mut self.room.entities[target_idx];
        t.status.stun_until = 0;
        t.status.root_until = 0;
        t.status.slow_until = 0;
        t.status.bulwark_slow_until = 0;
        t.status.silence_until = 0;
        if let Some(k) = t.hero_kit.as_deref_mut() {
            k.fear_until = 0;
            k.sleep_until = 0;
            k.taunt_until = 0;
        }
    }

    pub(crate) fn set_unstoppable(&mut self, target_idx: usize, sec: f32) {
        let dur = seconds_to_ticks(sec);
        if dur == 0 || target_idx >= self.room.entities.len() {
            return;
        }
        let until = self.tick + dur;
        let k = self.room.entities[target_idx]
            .hero_kit
            .get_or_insert_with(Box::default);
        k.unstoppable_until = k.unstoppable_until.max(until);
    }

    pub(crate) fn set_untargetable(&mut self, target_idx: usize, sec: f32) {
        let dur = seconds_to_ticks(sec);
        if dur == 0 || target_idx >= self.room.entities.len() {
            return;
        }
        let until = self.tick + dur;
        let k = self.room.entities[target_idx]
            .hero_kit
            .get_or_insert_with(Box::default);
        k.untargetable_until = k.untargetable_until.max(until);
    }

    pub(crate) fn set_stealth(&mut self, target_idx: usize, sec: f32, speed: f32, grace_sec: f32) {
        let dur = seconds_to_ticks(sec);
        if dur == 0 || target_idx >= self.room.entities.len() {
            return;
        }
        let until = self.tick + dur;
        let grace = self.tick + seconds_to_ticks(grace_sec);
        let k = self.room.entities[target_idx]
            .hero_kit
            .get_or_insert_with(Box::default);
        k.stealth_until = k.stealth_until.max(until);
        k.stealth_grace_until = k.stealth_grace_until.max(grace);
        k.stealth_speed = k.stealth_speed.max(speed);
    }

    pub(crate) fn knockback(
        &mut self,
        target_idx: usize,
        from: Position,
        distance: f32,
        wall_stun_sec: f32,
    ) -> bool {
        if distance <= 0.0 || target_idx >= self.room.entities.len() {
            return false;
        }
        if crate::hero_kits::kit_spec(self.room.entities[target_idx].type_id)
            .is_some_and(|s| s.perk.unstoppable_cc_downgrade)
        {
            return false;
        }
        let tpos = self.room.entities[target_idx].position;
        let (dx, dy) = dir_between(from, tpos);
        let dest = clamp_map_pos(Position {
            x: tpos.x + dx * distance,
            y: tpos.y + dy * distance,
        });
        let clear = self.room.spell_motion_clear(tpos, dest);
        if clear {
            self.room.place_spell_actor(target_idx, dest, self.events);
            false
        } else {
            // Stop at half step if blocked; stun if wall stun is configured.
            let half = clamp_map_pos(Position {
                x: tpos.x + dx * (distance * 0.4),
                y: tpos.y + dy * (distance * 0.4),
            });
            if self.room.spell_motion_clear(tpos, half) {
                self.room.place_spell_actor(target_idx, half, self.events);
            }
            if wall_stun_sec > 0.0 {
                self.apply_stun(target_idx, wall_stun_sec);
            }
            true
        }
    }

    pub(crate) fn pull_toward(&mut self, target_idx: usize, toward: Position, distance: f32) {
        if distance <= 0.0 || target_idx >= self.room.entities.len() {
            return;
        }
        if crate::hero_kits::kit_spec(self.room.entities[target_idx].type_id)
            .is_some_and(|s| s.perk.unstoppable_cc_downgrade)
        {
            return;
        }
        let tpos = self.room.entities[target_idx].position;
        let next = tpos.advance(toward, distance);
        if self.room.spell_motion_clear(tpos, next) {
            self.room.place_spell_actor(target_idx, next, self.events);
        }
    }

    pub(crate) fn add_buff(
        &mut self,
        target_idx: usize,
        stat: BuffStatKind,
        amount: f32,
        duration_sec: f32,
    ) {
        self.add_buff_ticks(
            target_idx,
            stat,
            amount,
            seconds_to_ticks(duration_sec).max(1),
        );
    }

    pub(crate) fn add_buff_ticks(
        &mut self,
        target_idx: usize,
        stat: BuffStatKind,
        amount: f32,
        dur: u64,
    ) {
        if dur == 0 || target_idx >= self.room.entities.len() {
            return;
        }
        let until = self.tick + dur;
        let k = self.room.entities[target_idx]
            .hero_kit
            .get_or_insert_with(Box::default);
        if let Some(existing) = k
            .buffs
            .iter_mut()
            .find(|b| b.stat == stat && b.until > self.tick)
        {
            existing.amount = existing.amount.max(amount);
            existing.until = existing.until.max(until);
        } else {
            k.buffs.push(TimedBuff {
                stat,
                amount,
                until,
            });
        }
    }

    pub(crate) fn add_mark(
        &mut self,
        target_idx: usize,
        mark: u8,
        add: u8,
        max: u8,
        duration_sec: f32,
    ) -> u8 {
        self.add_mark_ticks(
            target_idx,
            mark,
            add,
            max,
            seconds_to_ticks(duration_sec).max(1),
        )
    }

    pub(crate) fn add_mark_ticks(
        &mut self,
        target_idx: usize,
        mark: u8,
        add: u8,
        max: u8,
        dur: u64,
    ) -> u8 {
        if target_idx >= self.room.entities.len() {
            return 0;
        }
        let src_id = self.caster_id();
        let until = self.tick + dur;
        let k = self.room.entities[target_idx]
            .hero_kit
            .get_or_insert_with(Box::default);
        let entry = k.marks.entry(mark).or_insert(MarkInstance {
            stacks: 0,
            until,
            source_id: src_id,
        });
        if entry.until <= self.tick {
            entry.stacks = 0;
        }
        entry.stacks = entry.stacks.saturating_add(add).min(max.max(1));
        entry.until = until;
        entry.source_id = src_id;
        entry.stacks
    }

    pub(crate) fn mark_stacks(&self, target_idx: usize, mark: u8) -> u8 {
        self.room
            .entities
            .get(target_idx)
            .and_then(|e| e.hero_kit.as_deref())
            .and_then(|k| k.marks.get(&mark))
            .filter(|m| m.until > self.tick)
            .map_or(0, |m| m.stacks)
    }

    pub(crate) fn consume_mark(&mut self, target_idx: usize, mark: u8) -> u8 {
        let Some(k) = self
            .room
            .entities
            .get_mut(target_idx)
            .and_then(|e| e.hero_kit.as_deref_mut())
        else {
            return 0;
        };
        if let Some(m) = k.marks.remove(&mark)
            && m.until > self.tick
        {
            m.stacks
        } else {
            0
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn empower_next_attacks(
        &mut self,
        count: u8,
        bonus_dmg: f32,
        is_crystal: bool,
        stun_sec: f32,
        slow_pct: f32,
        slow_sec: f32,
        knockback: f32,
        duration_sec: f32,
    ) {
        let until = self.tick + seconds_to_ticks(duration_sec).max(1);
        let k = self.room.entities[self.caster]
            .hero_kit
            .get_or_insert_with(Box::default);
        k.empowered_attacks = count.max(1);
        k.empowered_until = until;
        k.empowered_bonus_damage = bonus_dmg;
        k.empowered_is_crystal = is_crystal;
        k.empowered_stun_sec = stun_sec;
        k.empowered_slow_pct = slow_pct;
        k.empowered_slow_sec = slow_sec;
        k.empowered_knockback = knockback;
    }

    pub(crate) fn reset_attack_cd(&mut self) {
        self.room.entities[self.caster].next_attack = self.tick;
    }

    pub(crate) fn reset_slot_cd(&mut self, slot: AbilitySlot) {
        self.room.entities[self.caster].cooldown[slot.idx()] = self.tick;
    }

    pub(crate) fn reduce_slot_cd(&mut self, slot: AbilitySlot, seconds: f32) {
        let ticks = seconds_to_ticks(seconds);
        let cur = self.room.entities[self.caster].cooldown[slot.idx()];
        self.room.entities[self.caster].cooldown[slot.idx()] =
            cur.saturating_sub(ticks).max(self.tick);
    }

    pub(crate) fn open_recast(
        &mut self,
        slot: AbilitySlot,
        stage: u8,
        target_id: u32,
        window_sec: f32,
    ) {
        let until = self.tick + seconds_to_ticks(window_sec).max(1);
        let k = self.room.entities[self.caster]
            .hero_kit
            .get_or_insert_with(Box::default);
        k.step_stage[slot.idx()] = stage;
        k.recast_until[slot.idx()] = until;
        k.recast_target[slot.idx()] = target_id;
    }

    pub(crate) fn clear_recast(&mut self, slot: AbilitySlot) {
        if let Some(k) = self.room.entities[self.caster].hero_kit.as_deref_mut() {
            k.step_stage[slot.idx()] = 0;
            k.recast_until[slot.idx()] = 0;
            k.recast_target[slot.idx()] = 0;
        }
    }

    pub(crate) fn swap_form(&mut self, new_form: u8) {
        let k = self.room.entities[self.caster]
            .hero_kit
            .get_or_insert_with(Box::default);
        k.form = if new_form == 0xFF {
            u8::from(k.form == 0)
        } else {
            new_form
        };
    }

    pub(crate) fn resource(&self) -> f32 {
        self.room.kit_resource_value(self.caster)
    }

    pub(crate) fn resource_max(&self) -> f32 {
        self.room.kit_resource_max(self.caster)
    }

    pub(crate) fn add_resource(&mut self, delta: f32) {
        let max = self.resource_max();
        let cur = self.resource();
        let next = (cur + delta).clamp(0.0, max);
        let k = self.room.entities[self.caster]
            .hero_kit
            .get_or_insert_with(Box::default);
        k.resource = next;
        k.resource_initialized = true;
    }

    pub(crate) fn spend_resource(&mut self, amount: f32) -> bool {
        let cur = self.resource();
        if cur + 0.001 < amount {
            return false;
        }
        let k = self.room.entities[self.caster]
            .hero_kit
            .get_or_insert_with(Box::default);
        k.resource = (cur - amount).max(0.0);
        k.resource_initialized = true;
        true
    }

    pub(crate) fn spawn_effect(&mut self, kind: KitEffectKind, position: Position, goal: Position) {
        let owner_id = self.caster_id();
        let team = self.caster_team();
        self.room.entities.spawn_spell(KitEffect {
            owner_id,
            team,
            position,
            goal,
            kind,
        });
    }

    pub(crate) fn enemies_in_radius(
        &self,
        center: Position,
        radius: f32,
        heroes_only: bool,
    ) -> Vec<usize> {
        let team = self.caster_team();
        self.room
            .entities
            .iter()
            .enumerate()
            .filter_map(|(j, e)| {
                let valid_kind = if heroes_only {
                    e.kind == Kind::Hero
                } else {
                    matches!(e.kind, Kind::Hero | Kind::Minion | Kind::Neutral)
                };
                (e.team != team && e.hp > 0.0 && valid_kind && e.position.dist(center) <= radius)
                    .then_some(j)
            })
            .collect()
    }

    pub(crate) fn allies_in_radius(
        &self,
        center: Position,
        radius: f32,
        include_self: bool,
        heroes_only: bool,
    ) -> Vec<usize> {
        let team = self.caster_team();
        self.room
            .entities
            .iter()
            .enumerate()
            .filter_map(|(j, e)| {
                if e.team != team || e.hp <= 0.0 || (!include_self && j == self.caster) {
                    return None;
                }
                let valid_kind = if heroes_only {
                    e.kind == Kind::Hero
                } else {
                    matches!(e.kind, Kind::Hero | Kind::Minion)
                };
                (valid_kind && e.position.dist(center) <= radius).then_some(j)
            })
            .collect()
    }

    /// Execute a slice of declarative `PrimitiveEffect`s for `ability_id`.
    pub(crate) fn exec_primitives(
        &mut self,
        ability_id: &str,
        slot: AbilitySlot,
        rank: u8,
        target: CastTarget,
        effects: &[PrimitiveEffect],
    ) {
        for &effect in effects {
            self.exec_one_primitive(ability_id, slot, rank, target, effect);
        }
    }

    fn exec_one_primitive(
        &mut self,
        ability_id: &str,
        slot: AbilitySlot,
        rank: u8,
        target: CastTarget,
        effect: PrimitiveEffect,
    ) {
        let dmg_mult = self.talent_dmg_mult(slot);
        let barrier_mult = self.talent_barrier_mult(slot);
        let heal_mult = self.talent_heal_mult(slot);
        let origin = self.caster_pos();
        match effect {
            PrimitiveEffect::TargetedStrike {
                damage,
                school,
                bonus_if_marked,
                triggers_basic_effects,
                resets_attack_cd,
                stun_seconds,
                slow_pct,
                slow_seconds,
            } => {
                if resets_attack_cd {
                    self.reset_attack_cd();
                }
                if let Some(tj) = self.resolve_target_idx(target) {
                    let mut raw = self.eval_var(ability_id, rank, damage) * dmg_mult;
                    if let Some((mk, extra)) = bonus_if_marked
                        && self.mark_stacks(tj, mk.id()) > 0
                    {
                        raw += self.eval_var(ability_id, rank, extra) * dmg_mult;
                    }
                    let actual_raw = match school {
                        DamageKind::PercentMaxHp => self.room.entities[tj].max_hp * raw,
                        DamageKind::PercentMissingHp => {
                            (self.room.entities[tj].max_hp - self.room.entities[tj].hp).max(0.0)
                                * raw
                        }
                        DamageKind::PercentCurrentHp => self.room.entities[tj].hp * raw,
                        _ => raw,
                    };
                    self.deal_damage(tj, actual_raw, school.school(), 0.0, triggers_basic_effects);
                    let stun = self.eval_var(ability_id, rank, stun_seconds);
                    if stun > 0.0 {
                        self.apply_stun(tj, stun);
                    }
                    let slow = self.eval_var(ability_id, rank, slow_pct);
                    let slow_sec = self.eval_var(ability_id, rank, slow_seconds);
                    if slow > 0.0 && slow_sec > 0.0 {
                        self.apply_slow(tj, slow, slow_sec);
                    }
                }
            }
            PrimitiveEffect::SkillshotProjectile {
                damage,
                school,
                range,
                speed,
                radius,
                piercing,
                return_to_caster,
                repeat_hit_mult,
                splash_damage,
                splash_radius,
                stun_seconds,
                root_seconds,
                silence_seconds,
                slow_pct,
                slow_seconds,
                minion_mult,
                resets_cast_cd_on_hit,
                applies_mark,
                destroys_enemy_projectiles,
            } => {
                let r = self.eval_var(ability_id, rank, range).max(1.0);
                let goal = self.resolve_target_pos(target, r);
                let (dx, dy) = dir_between(origin, goal);
                let end = clamp_map_pos(Position {
                    x: origin.x + dx * r,
                    y: origin.y + dy * r,
                });
                let spd = self.eval_var(ability_id, rank, speed).max(5.0) / TICK_HZ as f32;
                let rad = self.eval_var(ability_id, rank, radius).max(0.6);
                let raw_dmg = self.eval_var(ability_id, rank, damage);
                let splash = splash_damage.map_or(0.0, |s| self.eval_var(ability_id, rank, s));
                let stun_t = seconds_to_ticks(self.eval_var(ability_id, rank, stun_seconds));
                let root_t = seconds_to_ticks(self.eval_var(ability_id, rank, root_seconds));
                let sil_t = seconds_to_ticks(self.eval_var(ability_id, rank, silence_seconds));
                let slow_p = self.eval_var(ability_id, rank, slow_pct);
                let slow_t = seconds_to_ticks(self.eval_var(ability_id, rank, slow_seconds));
                let mark = applies_mark.map(|(m, add, sec)| (m.id(), add, seconds_to_ticks(sec)));
                self.spawn_effect(
                    KitEffectKind::Projectile {
                        ability_slot: slot.idx() as u8,
                        rank,
                        speed_per_tick: spd,
                        radius: rad,
                        damage: raw_dmg,
                        school: school.school(),
                        piercing,
                        returning: false,
                        return_to_caster,
                        repeat_mult: repeat_hit_mult,
                        splash_damage: splash,
                        splash_radius,
                        stun_ticks: stun_t,
                        root_ticks: root_t,
                        silence_ticks: sil_t,
                        slow_pct: slow_p,
                        slow_ticks: slow_t,
                        minion_mult,
                        resets_cd_on_hit: resets_cast_cd_on_hit,
                        applies_mark: mark,
                        destroys_projectiles: destroys_enemy_projectiles,
                        hit_actors: Vec::new(),
                    },
                    origin,
                    end,
                );
            }
            PrimitiveEffect::GroundAoE {
                radius,
                duration,
                ticks_per_sec,
                dps,
                hps,
                school,
                slow_pct,
                end_damage,
                end_root_seconds,
                end_stun_seconds,
                drifts_toward_aim,
                minion_mult,
                applies_mark,
                zone_kind,
            } => {
                let rad = self.eval_var(ability_id, rank, radius).max(1.0);
                let dur = self.eval_var(ability_id, rank, duration).max(0.2);
                let center = self.resolve_target_pos(target, 20.0);
                let interval = (TICK_HZ as f32 / ticks_per_sec.max(1.0)).round().max(1.0) as u64;
                let (drift_dx, drift_dy) = if drifts_toward_aim {
                    let (dx, dy) = dir_between(origin, center);
                    (dx * 0.1, dy * 0.1)
                } else {
                    (0.0, 0.0)
                };
                self.spawn_effect(
                    KitEffectKind::GroundZone {
                        ability_slot: slot.idx() as u8,
                        zone_kind,
                        radius: rad,
                        ticks_interval: interval,
                        next_tick: self.tick,
                        end_tick: self.tick + seconds_to_ticks(dur),
                        tick_damage: self.eval_var(ability_id, rank, dps),
                        tick_heal: self.eval_var(ability_id, rank, hps),
                        school: school.school(),
                        slow_pct: self.eval_var(ability_id, rank, slow_pct),
                        ally_speed_bonus: 0.0,
                        end_damage: end_damage.map_or(0.0, |v| self.eval_var(ability_id, rank, v)),
                        end_root_ticks: seconds_to_ticks(self.eval_var(
                            ability_id,
                            rank,
                            end_root_seconds,
                        )),
                        end_stun_ticks: seconds_to_ticks(self.eval_var(
                            ability_id,
                            rank,
                            end_stun_seconds,
                        )),
                        drift_dx,
                        drift_dy,
                        follow_caster: false,
                        ignited: false,
                        minion_mult,
                        applies_mark: applies_mark
                            .map(|(m, add, sec)| (m.id(), add, seconds_to_ticks(sec))),
                    },
                    center,
                    center,
                );
            }
            PrimitiveEffect::DelayedBlast {
                delay,
                radius,
                inner_radius,
                damage,
                outer_mult,
                school,
                stun_seconds,
                root_seconds,
                sleep_seconds,
                slow_pct,
                slow_seconds,
                reveal_seconds,
                bonus_if_marked,
                minion_mult,
                applies_mark,
            } => {
                let d_ticks = seconds_to_ticks(self.eval_var(ability_id, rank, delay));
                let center = self.resolve_target_pos(target, 200.0);
                self.spawn_effect(
                    KitEffectKind::DelayedBlast {
                        ability_slot: slot.idx() as u8,
                        release_tick: self.tick + d_ticks,
                        radius: self.eval_var(ability_id, rank, radius).max(1.0),
                        inner_radius,
                        damage: self.eval_var(ability_id, rank, damage),
                        outer_mult,
                        school: school.school(),
                        stun_ticks: seconds_to_ticks(self.eval_var(ability_id, rank, stun_seconds)),
                        root_ticks: seconds_to_ticks(self.eval_var(ability_id, rank, root_seconds)),
                        sleep_ticks: seconds_to_ticks(self.eval_var(
                            ability_id,
                            rank,
                            sleep_seconds,
                        )),
                        sleep_unwaking: false,
                        slow_pct: self.eval_var(ability_id, rank, slow_pct),
                        slow_ticks: seconds_to_ticks(self.eval_var(ability_id, rank, slow_seconds)),
                        reveal_ticks: seconds_to_ticks(reveal_seconds),
                        bonus_mark: bonus_if_marked.map(|(m, mult)| (m.id(), mult)),
                        minion_mult,
                        applies_mark: applies_mark
                            .map(|(m, add, sec)| (m.id(), add, seconds_to_ticks(sec))),
                    },
                    center,
                    center,
                );
            }
            PrimitiveEffect::ConeWave {
                range,
                half_angle_deg,
                damage,
                school,
                multi_bolts,
                extra_hit_mult,
                slow_pct,
                slow_seconds,
                stun_seconds,
                fear_seconds,
                silence_seconds,
                knockback_dist,
                reveal_seconds,
                minion_mult,
                applies_mark,
            } => {
                let r = self.eval_var(ability_id, rank, range).max(1.0);
                let aim = self.resolve_target_pos(target, r);
                let (ax, ay) = dir_between(origin, aim);
                let cos_thresh = half_angle_deg.to_radians().cos();
                let raw = self.eval_var(ability_id, rank, damage) * dmg_mult;
                let total_mult = if multi_bolts > 1 {
                    1.0 + f32::from(multi_bolts - 1) * extra_hit_mult
                } else {
                    1.0
                };
                let hits = self.enemies_in_radius(origin, r, false);
                let stun = self.eval_var(ability_id, rank, stun_seconds);
                let fear = self.eval_var(ability_id, rank, fear_seconds);
                let sil = self.eval_var(ability_id, rank, silence_seconds);
                let slow_p = self.eval_var(ability_id, rank, slow_pct);
                let slow_s = self.eval_var(ability_id, rank, slow_seconds);
                for j in hits {
                    let tpos = self.room.entities[j].position;
                    let (tx, ty) = dir_between(origin, tpos);
                    if origin.dist(tpos) > 0.6 && (ax * tx + ay * ty) < cos_thresh {
                        continue;
                    }
                    let is_hero = self.room.entities[j].kind == Kind::Hero;
                    let mult = if is_hero { 1.0 } else { minion_mult };
                    self.deal_damage(j, raw * total_mult * mult, school.school(), 0.0, false);
                    if stun > 0.0 && is_hero {
                        self.apply_stun(j, stun);
                    }
                    if fear > 0.0 && is_hero {
                        self.apply_fear(j, fear);
                    }
                    if sil > 0.0 {
                        self.apply_silence(j, sil);
                    }
                    if slow_p > 0.0 && slow_s > 0.0 {
                        self.apply_slow(j, slow_p, slow_s);
                    }
                    if knockback_dist > 0.0 && is_hero {
                        self.knockback(j, origin, knockback_dist, 0.0);
                    }
                    if reveal_seconds > 0.0 {
                        self.apply_reveal(j, reveal_seconds);
                    }
                    if let Some((m, add, sec)) = applies_mark {
                        self.add_mark(j, m.id(), add, 10, sec);
                    }
                }
            }
            PrimitiveEffect::DashBlink {
                range,
                duration,
                instant_blink,
                invuln_during,
                unstoppable_during,
                path_radius,
                path_damage,
                arrival_radius,
                arrival_damage,
                school,
                slow_pct,
                slow_seconds,
                stun_seconds,
                knockback_dist,
                backward_hop,
                applies_mark,
            } => {
                let r = self.eval_var(ability_id, rank, range).max(1.0);
                let raw_goal = self.resolve_target_pos(target, r);
                let goal = if backward_hop {
                    let (dx, dy) = dir_between(origin, raw_goal);
                    clamp_map_pos(Position {
                        x: origin.x - dx * r,
                        y: origin.y - dy * r,
                    })
                } else {
                    raw_goal
                };
                let dur = self.eval_var(ability_id, rank, duration);
                if instant_blink || dur <= 0.05 {
                    if self.room.spell_motion_clear(origin, goal) {
                        self.room.place_spell_actor(self.caster, goal, self.events);
                    }
                    let arr_dmg = self.eval_var(ability_id, rank, arrival_damage) * dmg_mult;
                    let stun = self.eval_var(ability_id, rank, stun_seconds);
                    let slow_p = self.eval_var(ability_id, rank, slow_pct);
                    let slow_s = self.eval_var(ability_id, rank, slow_seconds);
                    if arr_dmg > 0.0 || stun > 0.0 || knockback_dist > 0.0 {
                        let center = self.caster_pos();
                        for j in self.enemies_in_radius(center, arrival_radius.max(1.5), false) {
                            if arr_dmg > 0.0 {
                                self.deal_damage(j, arr_dmg, school.school(), 0.0, false);
                            }
                            if stun > 0.0 {
                                self.apply_stun(j, stun);
                            }
                            if slow_p > 0.0 && slow_s > 0.0 {
                                self.apply_slow(j, slow_p, slow_s);
                            }
                            if knockback_dist > 0.0 {
                                self.knockback(j, center, knockback_dist, 0.0);
                            }
                            if let Some((m, add, sec)) = applies_mark {
                                self.add_mark(j, m.id(), add, 10, sec);
                            }
                        }
                    }
                } else {
                    let dur_ticks = seconds_to_ticks(dur).max(1);
                    self.spawn_effect(
                        KitEffectKind::DashMotion {
                            ability_slot: slot.idx() as u8,
                            start: origin,
                            start_tick: self.tick,
                            end_tick: self.tick + dur_ticks,
                            invuln: invuln_during,
                            unstoppable: unstoppable_during,
                            path_radius: path_radius.max(1.2),
                            path_damage: self.eval_var(ability_id, rank, path_damage),
                            arrival_radius: arrival_radius.max(1.5),
                            arrival_damage: self.eval_var(ability_id, rank, arrival_damage),
                            school: school.school(),
                            slow_pct: self.eval_var(ability_id, rank, slow_pct),
                            slow_ticks: seconds_to_ticks(self.eval_var(
                                ability_id,
                                rank,
                                slow_seconds,
                            )),
                            stun_ticks: seconds_to_ticks(self.eval_var(
                                ability_id,
                                rank,
                                stun_seconds,
                            )),
                            knockback_dist,
                            applies_mark: applies_mark
                                .map(|(m, add, sec)| (m.id(), add, seconds_to_ticks(sec))),
                            hit_actors: Vec::new(),
                        },
                        origin,
                        goal,
                    );
                }
            }
            PrimitiveEffect::LungeHook {
                range,
                damage,
                school,
                pull_distance,
                knockback_distance,
                stun_on_wall_seconds,
                tether_duration,
                tether_break_range,
                tether_dps_pct_hp,
                devour_seconds,
                suppress_untargetable,
            } => {
                let r = self.eval_var(ability_id, rank, range).max(1.5);
                let aim = self.resolve_target_pos(target, r);
                let target_idx = self
                    .resolve_target_idx(target)
                    .or_else(|| self.enemies_in_radius(aim, 2.5, true).into_iter().next());
                if let Some(tj) = target_idx {
                    let raw = self.eval_var(ability_id, rank, damage) * dmg_mult;
                    if raw > 0.0 {
                        self.deal_damage(tj, raw, school.school(), 0.0, false);
                    }
                    let pull = self.eval_var(ability_id, rank, pull_distance);
                    if pull > 0.0 {
                        self.pull_toward(tj, origin, pull);
                    }
                    let kb = self.eval_var(ability_id, rank, knockback_distance);
                    let wall_stun = self.eval_var(ability_id, rank, stun_on_wall_seconds);
                    if kb > 0.0 {
                        self.knockback(tj, origin, kb, wall_stun);
                    }
                    let tether_dur = self.eval_var(ability_id, rank, tether_duration);
                    if tether_dur > 0.0 {
                        let host_id = self.room.entities[tj].id;
                        let anchor = self.room.entities[tj].position;
                        self.spawn_effect(
                            KitEffectKind::Tether {
                                ability_slot: slot.idx() as u8,
                                target_id: host_id,
                                anchor_pos: anchor,
                                break_range: self
                                    .eval_var(ability_id, rank, tether_break_range)
                                    .max(3.5),
                                dps_pct_hp: self.eval_var(ability_id, rank, tether_dps_pct_hp),
                                break_damage: 0.0,
                                break_stun_ticks: seconds_to_ticks(wall_stun),
                                anchor_follows_caster: false,
                                next_tick: self.tick + TICK_HZ / 2,
                                end_tick: self.tick + seconds_to_ticks(tether_dur),
                            },
                            anchor,
                            anchor,
                        );
                    }
                    let dev_sec = self.eval_var(ability_id, rank, devour_seconds);
                    if dev_sec > 0.0 || suppress_untargetable {
                        let host_id = self.room.entities[tj].id;
                        let is_enemy = self.room.entities[tj].team != self.caster_team();
                        let pos = self.room.entities[tj].position;
                        self.spawn_effect(
                            KitEffectKind::Attachment {
                                ability_slot: slot.idx() as u8,
                                host_id,
                                is_devour: is_enemy,
                                dps: 0.0,
                                end_damage: 0.0,
                                end_radius: 3.0,
                                school: school.school(),
                                next_tick: self.tick + TICK_HZ / 2,
                                end_tick: self.tick + seconds_to_ticks(dev_sec.max(1.5)),
                            },
                            pos,
                            pos,
                        );
                    }
                }
            }
            PrimitiveEffect::SelfBuff {
                stat,
                amount,
                duration,
                empowered_attacks,
                empowered_bonus_damage,
                empowered_stun_seconds,
                empowered_slow_pct,
                empowered_slow_seconds,
                empowered_knockback,
                resets_attack_cd,
            } => {
                if resets_attack_cd {
                    self.reset_attack_cd();
                }
                let amt = self.eval_var(ability_id, rank, amount);
                let dur = self.eval_var(ability_id, rank, duration).max(0.5);
                if amt != 0.0 {
                    self.add_buff(self.caster, stat, amt, dur);
                }
                if empowered_attacks > 0 {
                    let bonus = self.eval_var(ability_id, rank, empowered_bonus_damage) * dmg_mult;
                    self.empower_next_attacks(
                        empowered_attacks,
                        bonus,
                        false,
                        empowered_stun_seconds,
                        empowered_slow_pct,
                        empowered_slow_seconds,
                        empowered_knockback,
                        dur,
                    );
                }
            }
            PrimitiveEffect::AllyHealBarrier {
                radius,
                include_self,
                target_only,
                burst_heal,
                hot_per_sec,
                hot_duration,
                barrier,
                barrier_duration,
                fortified,
                fortified_duration,
                self_cast_mult,
                grants_attack_buff_stacks,
                grants_attack_bonus_dmg,
            } => {
                let rad = self.eval_var(ability_id, rank, radius);
                let center = self.resolve_target_pos(target, rad.max(10.0));
                let targets = if target_only {
                    self.resolve_target_idx(target)
                        .filter(|&j| self.room.entities[j].team == self.caster_team())
                        .into_iter()
                        .collect()
                } else {
                    self.allies_in_radius(center, rad.max(1.0), include_self, true)
                };
                let bh = self.eval_var(ability_id, rank, burst_heal) * heal_mult;
                let hot_sec = self.eval_var(ability_id, rank, hot_duration);
                let hot = self.eval_var(ability_id, rank, hot_per_sec) * hot_sec * heal_mult;
                let bar = self.eval_var(ability_id, rank, barrier) * barrier_mult;
                let bar_dur = self.eval_var(ability_id, rank, barrier_duration).max(1.0);
                let fort = self.eval_var(ability_id, rank, fortified);
                let fort_dur = self.eval_var(ability_id, rank, fortified_duration).max(1.0);
                let grant_bonus_dmg = if grants_attack_buff_stacks > 0 {
                    self.eval_var(ability_id, rank, grants_attack_bonus_dmg)
                } else {
                    0.0
                };
                for j in targets {
                    let sm = if j == self.caster && self_cast_mult > 0.0 {
                        self_cast_mult
                    } else {
                        1.0
                    };
                    if bh + hot > 0.0 {
                        self.heal_unit(j, (bh + hot) * sm);
                    }
                    if bar > 0.0 {
                        self.grant_barrier(j, bar * sm, bar_dur, None);
                    }
                    if fort > 0.0 {
                        self.grant_fortified(j, fort * sm, fort_dur);
                    }
                    if grants_attack_buff_stacks > 0 {
                        let k = self.room.entities[j]
                            .hero_kit
                            .get_or_insert_with(Box::default);
                        k.empowered_attacks = grants_attack_buff_stacks;
                        k.empowered_until = self.tick + seconds_to_ticks(bar_dur);
                        k.empowered_bonus_damage = grant_bonus_dmg;
                        k.empowered_is_crystal = true;
                    }
                }
            }
            PrimitiveEffect::ChannelBeam {
                duration,
                ticks_per_sec,
                range,
                width,
                dps,
                school,
                lock_caster,
                slow_pct,
                stun_target,
                global_all_heroes,
                shield_pierce,
                structure_mult,
                minion_mult,
            } => {
                let dur = self.eval_var(ability_id, rank, duration).max(0.2);
                let r = self.eval_var(ability_id, rank, range).max(1.0);
                let aim = self.resolve_target_pos(target, r);
                let target_id = if let CastTarget::Entity(id) = target {
                    if stun_target { id } else { 0 }
                } else {
                    0
                };
                let interval = (TICK_HZ as f32 / ticks_per_sec.max(1.0)).round().max(1.0) as u64;
                self.spawn_effect(
                    KitEffectKind::ChannelBeam {
                        ability_slot: slot.idx() as u8,
                        target_id,
                        ticks_interval: interval,
                        next_tick: self.tick,
                        end_tick: self.tick + seconds_to_ticks(dur),
                        range: r,
                        width,
                        tick_damage: self.eval_var(ability_id, rank, dps),
                        tick_heal: 0.0,
                        school: school.school(),
                        lock_caster,
                        slow_pct: self.eval_var(ability_id, rank, slow_pct),
                        stun_target,
                        global_all_heroes,
                        shield_pierce: self.eval_var(ability_id, rank, shield_pierce),
                        structure_mult,
                        minion_mult,
                    },
                    origin,
                    aim,
                );
            }
            PrimitiveEffect::SummonPet {
                count,
                max_active,
                duration,
                pet_hp,
                pet_damage,
                school,
                hit_ticks,
                applies_mark,
                replace_existing,
                pet_kind,
            } => {
                let owner_id = self.caster_id();
                if replace_existing {
                    let to_remove: Vec<_> = self
                        .room
                        .entities
                        .ecs
                        .query::<(bevy_ecs::entity::Entity, &KitEffect)>()
                        .iter(&self.room.entities.ecs)
                        .filter_map(|(eh, eff)| {
                            (eff.owner_id == owner_id
                                && matches!(
                                    eff.kind,
                                    KitEffectKind::SummonPet { pet_kind: pk, .. } if pk == pet_kind
                                ))
                            .then_some(eh)
                        })
                        .collect();
                    for eh in to_remove {
                        self.room.entities.remove_spell_handle::<KitEffect>(eh);
                    }
                }
                let active_count = self
                    .room
                    .entities
                    .spells::<KitEffect>()
                    .filter(|eff| {
                        eff.owner_id == owner_id
                            && matches!(
                                eff.kind,
                                KitEffectKind::SummonPet { pet_kind: pk, .. } if pk == pet_kind
                            )
                    })
                    .count();
                let allowed = usize::from(max_active).saturating_sub(active_count);
                let spawn_n = usize::from(count).min(allowed);
                let dur = self.eval_var(ability_id, rank, duration).max(1.0);
                let hp = self.eval_var(ability_id, rank, pet_hp).max(100.0);
                let dmg = self.eval_var(ability_id, rank, pet_damage);
                for offset in 0..spawn_n {
                    let spawn_pos = Position {
                        x: origin.x + (offset as f32 - 1.0) * 0.8,
                        y: origin.y + 0.5,
                    };
                    self.spawn_effect(
                        KitEffectKind::SummonPet {
                            ability_slot: slot.idx() as u8,
                            pet_kind,
                            hp,
                            hit_ticks_remaining: hit_ticks.max(1),
                            damage: dmg,
                            school: school.school(),
                            attack_interval: TICK_HZ / 2,
                            next_attack: self.tick + TICK_HZ / 2,
                            end_tick: self.tick + seconds_to_ticks(dur),
                            applies_mark: applies_mark
                                .map(|(m, add, sec)| (m.id(), add, seconds_to_ticks(sec))),
                            locked_target: 0,
                        },
                        spawn_pos,
                        spawn_pos,
                    );
                }
            }
            PrimitiveEffect::TrapZone {
                arm_delay,
                duration,
                trigger_radius,
                blast_radius,
                damage,
                school,
                root_seconds,
                stun_seconds,
                slow_pct,
                max_traps,
                reveals_enemies,
            } => {
                let owner_id = self.caster_id();
                let mut existing: Vec<_> = self
                    .room
                    .entities
                    .ecs
                    .query::<(bevy_ecs::entity::Entity, &KitEffect)>()
                    .iter(&self.room.entities.ecs)
                    .filter_map(|(eh, eff)| {
                        (eff.owner_id == owner_id
                            && matches!(eff.kind, KitEffectKind::TrapZone { .. }))
                        .then_some(eh)
                    })
                    .collect();
                while existing.len() >= usize::from(max_traps.max(1)) {
                    let oldest = existing.remove(0);
                    self.room.entities.remove_spell_handle::<KitEffect>(oldest);
                }
                let center = self.resolve_target_pos(target, 12.0);
                let arm = self.eval_var(ability_id, rank, arm_delay);
                let dur = self.eval_var(ability_id, rank, duration).max(2.0);
                self.spawn_effect(
                    KitEffectKind::TrapZone {
                        ability_slot: slot.idx() as u8,
                        armed_at: self.tick + seconds_to_ticks(arm),
                        end_tick: self.tick + seconds_to_ticks(dur),
                        trigger_radius: self.eval_var(ability_id, rank, trigger_radius).max(0.8),
                        blast_radius: self.eval_var(ability_id, rank, blast_radius).max(1.5),
                        damage: self.eval_var(ability_id, rank, damage),
                        school: school.school(),
                        root_ticks: seconds_to_ticks(self.eval_var(ability_id, rank, root_seconds)),
                        stun_ticks: seconds_to_ticks(self.eval_var(ability_id, rank, stun_seconds)),
                        slow_pct: self.eval_var(ability_id, rank, slow_pct),
                        reveals: reveals_enemies,
                        detonate_on_caster_damage: false,
                    },
                    center,
                    center,
                );
            }
            PrimitiveEffect::SegmentBarrier {
                radius_or_span,
                duration,
                arm_delay,
                dps_or_hit_damage,
                school,
                stun_seconds,
                silence_seconds,
                slow_pct,
                max_stakes: _,
                is_ring_arena,
            } => {
                let rad = self.eval_var(ability_id, rank, radius_or_span).max(2.0);
                let dur = self.eval_var(ability_id, rank, duration).max(1.0);
                let arm = self.eval_var(ability_id, rank, arm_delay);
                let center = self.resolve_target_pos(target, 12.0);
                let (end_pos, anchor) = if is_ring_arena {
                    (center, center)
                } else {
                    // Connect to previous recent stake if within span, otherwise form perpendicular barrier segment.
                    let owner_id = self.caster_id();
                    let prev_stake = self
                        .room
                        .entities
                        .spells::<KitEffect>()
                        .filter_map(|eff| {
                            (eff.owner_id == owner_id
                                && matches!(
                                    eff.kind,
                                    KitEffectKind::SegmentBarrier {
                                        is_ring_arena: false,
                                        ..
                                    }
                                )
                                && eff.position.dist(center) <= rad * 2.0)
                                .then_some(eff.position)
                        })
                        .last();
                    let other = prev_stake.unwrap_or(Position {
                        x: center.x,
                        y: center.y + 0.1,
                    });
                    (other, center)
                };
                self.spawn_effect(
                    KitEffectKind::SegmentBarrier {
                        ability_slot: slot.idx() as u8,
                        armed_at: self.tick + seconds_to_ticks(arm),
                        end_tick: self.tick + seconds_to_ticks(dur),
                        end_pos,
                        radius: rad,
                        is_ring_arena,
                        dps_or_hit_damage: self.eval_var(ability_id, rank, dps_or_hit_damage),
                        school: school.school(),
                        stun_ticks: seconds_to_ticks(self.eval_var(ability_id, rank, stun_seconds)),
                        silence_ticks: seconds_to_ticks(self.eval_var(
                            ability_id,
                            rank,
                            silence_seconds,
                        )),
                        slow_pct: self.eval_var(ability_id, rank, slow_pct),
                        next_tick: self.tick,
                        struck_recently: Vec::new(),
                    },
                    anchor,
                    end_pos,
                );
            }
            PrimitiveEffect::StealthMode {
                pre_delay: _,
                duration,
                speed_bonus,
                affect_allies,
                break_on_damage: _,
                break_on_attack: _,
                re_stealth_delay: _,
            } => {
                let dur = self.eval_var(ability_id, rank, duration).max(1.0);
                let spd = self.eval_var(ability_id, rank, speed_bonus);
                if affect_allies {
                    let allies = self.allies_in_radius(origin, 200.0, true, true);
                    for a in allies {
                        self.set_stealth(a, dur, spd, 0.5);
                    }
                } else {
                    self.set_stealth(self.caster, dur, spd, 0.25);
                }
            }
            PrimitiveEffect::MarkStacks {
                mark,
                on_self,
                add_stacks,
                max_stacks,
                duration,
                consume_all,
                damage_per_stack,
                heal_per_stack,
                school,
                mortal_wound_seconds,
            } => {
                let idx = if on_self {
                    Some(self.caster)
                } else {
                    self.resolve_target_idx(target)
                };
                if let Some(j) = idx {
                    if add_stacks > 0 {
                        self.add_mark(j, mark.id(), add_stacks as u8, max_stacks, duration);
                    }
                    if mortal_wound_seconds > 0.0 {
                        self.apply_mortal_wound(j, mortal_wound_seconds);
                    }
                    if consume_all {
                        let stacks = self.consume_mark(j, mark.id());
                        if stacks > 0 {
                            let d = self.eval_var(ability_id, rank, damage_per_stack)
                                * f32::from(stacks)
                                * dmg_mult;
                            if d > 0.0 {
                                self.deal_damage(j, d, school.school(), 0.0, false);
                            }
                            let h = self.eval_var(ability_id, rank, heal_per_stack)
                                * f32::from(stacks)
                                * heal_mult;
                            if h > 0.0 {
                                self.heal_unit(self.caster, h);
                            }
                        }
                    }
                }
            }
            PrimitiveEffect::ExecuteDamage {
                threshold_instant_kill_pct,
                missing_hp_pct,
                school,
                nonhero_cap,
            } => {
                if let Some(tj) = self.resolve_target_idx(target) {
                    let thr = self.eval_var(ability_id, rank, threshold_instant_kill_pct);
                    let e = &self.room.entities[tj];
                    if thr > 0.0 && e.max_hp > 0.0 && (e.hp / e.max_hp) <= thr {
                        let kill_dmg = e.hp + 9999.0;
                        self.deal_damage(tj, kill_dmg, DamageSchool::True, 1.0, false);
                    } else {
                        let pct = self.eval_var(ability_id, rank, missing_hp_pct);
                        if pct > 0.0 {
                            let missing = (e.max_hp - e.hp).max(0.0);
                            let mut dmg = missing * pct * dmg_mult;
                            if e.kind != Kind::Hero && nonhero_cap > 0.0 {
                                dmg = dmg.min(nonhero_cap);
                            }
                            self.deal_damage(tj, dmg, school.school(), 0.0, false);
                        }
                    }
                }
            }
            PrimitiveEffect::CleanseImmune {
                cleanse_cc,
                cc_immune_seconds,
                untargetable_seconds,
            } => {
                if cleanse_cc {
                    self.cleanse_cc(self.caster);
                }
                let imm = self.eval_var(ability_id, rank, cc_immune_seconds);
                if imm > 0.0 {
                    self.set_unstoppable(self.caster, imm);
                }
                let untarg = self.eval_var(ability_id, rank, untargetable_seconds);
                if untarg > 0.0 {
                    self.set_untargetable(self.caster, untarg);
                }
            }
            PrimitiveEffect::FormSwap {
                to_form,
                empowers_next_attack,
                bonus_damage,
                slow_pct,
                slow_seconds,
            } => {
                self.swap_form(to_form);
                if empowers_next_attack {
                    let bonus = self.eval_var(ability_id, rank, bonus_damage) * dmg_mult;
                    self.empower_next_attacks(
                        1,
                        bonus,
                        true,
                        0.0,
                        slow_pct,
                        slow_seconds,
                        0.0,
                        4.0,
                    );
                }
            }
            PrimitiveEffect::AmmoPool {
                slot: pool_slot,
                max_charges,
                reload_seconds: _,
                grant_charges,
                grant_resource,
            } => {
                let max_c = self
                    .eval_var(ability_id, rank, max_charges)
                    .round()
                    .max(1.0) as u8;
                let s_idx = pool_slot.base_slot().idx();
                if grant_charges != 0 {
                    let cur = self
                        .room
                        .kit_ammo_charges(self.caster, pool_slot.base_slot());
                    let next = (i16::from(cur) + i16::from(grant_charges))
                        .clamp(0, i16::from(max_c)) as u8;
                    let k = self.room.entities[self.caster]
                        .hero_kit
                        .get_or_insert_with(Box::default);
                    k.charges[s_idx] = next;
                    k.charges_initialized[s_idx] = true;
                }
                if grant_resource != 0.0 {
                    self.add_resource(grant_resource);
                }
            }
            PrimitiveEffect::RefreshCooldown {
                slot: target_slot,
                reduce_pct,
                reduce_flat_sec,
                overdrive_only,
            } => {
                if !overdrive_only || rank >= 5 {
                    let s = target_slot.base_slot();
                    if reduce_pct >= 1.0 {
                        self.reset_slot_cd(s);
                    } else {
                        if reduce_pct > 0.0 {
                            let rem = self.room.entities[self.caster].cooldown[s.idx()]
                                .saturating_sub(self.tick)
                                as f32
                                / TICK_HZ as f32;
                            self.reduce_slot_cd(s, rem * reduce_pct);
                        }
                        if reduce_flat_sec > 0.0 {
                            self.reduce_slot_cd(s, reduce_flat_sec);
                        }
                    }
                }
            }
            PrimitiveEffect::OpenRecast {
                slot: recast_slot,
                window_seconds,
                stage,
            } => {
                let target_id = match target {
                    CastTarget::Entity(id) => id,
                    CastTarget::SelfCast | CastTarget::Point(_) => 0,
                };
                let win = self.eval_var(ability_id, rank, window_seconds).max(1.0);
                self.open_recast(recast_slot.base_slot(), stage, target_id, win);
            }
        }
    }
}

impl Room<'_> {
    pub(crate) fn hero_kit_enabled(&self) -> bool {
        self.abilities.hero_kits.is_some()
    }

    #[allow(dead_code)]
    pub(crate) fn enable_builtin_hero_kits(&mut self) {
        let book = std::sync::Arc::make_mut(&mut self.abilities);
        if book.hero_kits.is_none() {
            book.hero_kits = Some(HeroKitBook::builtin());
        }
    }

    /// Resolve active `AbilityKitSpec` for `(caster_idx, slot)`, honoring forms, recasts, and Viola chords.
    pub(crate) fn active_ability_spec(
        &self,
        caster_idx: usize,
        slot: AbilitySlot,
    ) -> Option<&'static AbilityKitSpec> {
        let e = &self.entities[caster_idx];
        let spec = crate::hero_kits::kit_spec(e.type_id)?;
        let kit = e.hero_kit.as_deref();
        let recast_active = kit.is_some_and(|k| self.tick < k.recast_until[slot.idx()]);
        let stage = kit.map_or(0, |k| k.step_stage[slot.idx()]);
        let form = kit.map_or(0, |k| k.form);

        let variant = match slot {
            AbilitySlot::A => {
                if recast_active && stage == 2 {
                    spec.variant(HeroSlot::C2)
                        .or_else(|| spec.variant(HeroSlot::A2))
                } else if recast_active || form == 1 {
                    spec.variant(HeroSlot::A2)
                } else {
                    None
                }
            }
            AbilitySlot::B => {
                if recast_active || form == 1 {
                    spec.variant(HeroSlot::B2)
                } else {
                    None
                }
            }
            AbilitySlot::C => {
                if spec.hero_id == "Viola"
                    && let Some(k) = kit
                    && k.musical_note_count >= 3
                {
                    let b_count = k.musical_notes.iter().filter(|&&n| n == 2).count();
                    let chord = match b_count {
                        0 => HeroSlot::C,  // AAA -> Fleeting Song
                        1 => HeroSlot::C2, // AAB -> Tempo
                        2 => HeroSlot::C3, // ABB -> Dissonance
                        _ => HeroSlot::C4, // BBB -> Lullaby
                    };
                    spec.variant(chord).or(Some(&spec.c))
                } else if recast_active || form == 1 {
                    spec.variant(HeroSlot::C2)
                } else {
                    None
                }
            }
        };
        Some(variant.unwrap_or(match slot {
            AbilitySlot::A => &spec.a,
            AbilitySlot::B => &spec.b,
            AbilitySlot::C => &spec.c,
        }))
    }

    pub(crate) fn kit_resource_max(&self, i: usize) -> f32 {
        let e = &self.entities[i];
        let Some(spec) = crate::hero_kits::kit_spec(e.type_id) else {
            return e.max_energy;
        };
        if spec.resource == ResourceKind::Energy {
            return e.max_energy;
        }
        let talent_bonus = e
            .hero_kit
            .as_deref()
            .and_then(|k| k.talent.as_ref())
            .map_or(0.0, |t| {
                let loaded = self.abilities.hero_kits.as_ref().map(|b| &b.talents);
                talents_runtime::talent_passive_stats(
                    t,
                    e.item_stats.crystal,
                    e.stats.weapon,
                    loaded,
                )
                .max_resource_bonus
            });
        let bonus_energy = (e.max_energy - 200.0).max(0.0);
        spec.resource_max + bonus_energy * spec.resource_energy_conversion.0 + talent_bonus
    }

    pub(crate) fn kit_resource_value(&self, i: usize) -> f32 {
        let e = &self.entities[i];
        let Some(spec) = crate::hero_kits::kit_spec(e.type_id) else {
            return e.energy;
        };
        if spec.resource == ResourceKind::Energy {
            return e.energy;
        }
        match &e.hero_kit {
            Some(k) if k.resource_initialized => k.resource,
            _ => spec.resource_initial,
        }
    }

    pub(crate) fn kit_ammo_charges(&self, i: usize, slot: AbilitySlot) -> u8 {
        let e = &self.entities[i];
        let max_c = self.kit_max_charges(i, slot);
        if let Some(k) = &e.hero_kit
            && k.charges_initialized[slot.idx()]
        {
            return k.charges[slot.idx()].min(max_c);
        }
        max_c
    }

    pub(crate) fn kit_max_charges(&self, i: usize, slot: AbilitySlot) -> u8 {
        let e = &self.entities[i];
        let Some(ab) = self.active_ability_spec(i, slot) else {
            return 0;
        };
        let rank = e.ability_ranks[slot.idx()].max(1);
        let mut base = ab
            .charges
            .eval_fallback(rank, EvalInputs::default())
            .round() as u8;
        if let Some(t) = e.hero_kit.as_deref().and_then(|k| k.talent.as_ref()) {
            let loaded = self.abilities.hero_kits.as_ref().map(|b| &b.talents);
            base = base.saturating_add(talents_runtime::talent_bonus_charges(
                t,
                slot,
                e.item_stats.crystal,
                e.stats.weapon,
                loaded,
            ));
        }
        base
    }

    pub(crate) fn kit_can_learn(&self, i: usize, slot: AbilitySlot) -> bool {
        if !self.hero_kit_enabled() {
            return false;
        }
        let e = &self.entities[i];
        let Some(spec) = crate::hero_kits::kit_spec(e.type_id) else {
            return false;
        };
        let rank = e.ability_ranks[slot.idx()];
        let max_rank = self.active_ability_spec(i, slot).map_or(
            if slot == AbilitySlot::C {
                spec.ult_max_rank
            } else {
                5
            },
            |ab| {
                if slot == AbilitySlot::C {
                    ab.max_rank.min(spec.ult_max_rank)
                } else {
                    ab.max_rank.min(5)
                }
            },
        );
        if e.skill_points == 0 || rank >= max_rank {
            return false;
        }
        let needed = if slot == AbilitySlot::C {
            if spec.ult_from_level_one {
                1 + 2 * rank
            } else {
                6 + 3 * rank
            }
        } else {
            1 + 2 * rank
        };
        e.level >= needed
    }

    pub(crate) fn kit_castable(
        &self,
        i: usize,
        slot: AbilitySlot,
        target: CastTarget,
    ) -> Result<(u8, f32, f32, u64, bool), &'static str> {
        if !self.hero_kit_enabled() {
            return Err("ability unavailable");
        }
        let e = &self.entities[i];
        let Some(spec) = crate::hero_kits::kit_spec(e.type_id) else {
            return Err("ability unavailable");
        };
        let ab = self
            .active_ability_spec(i, slot)
            .ok_or("ability unavailable")?;
        let rank = e.ability_ranks[slot.idx()];
        let allow_unlearned_ult = slot == AbilitySlot::C && spec.ult_from_level_one && rank == 0;
        if rank == 0 && !allow_unlearned_ult {
            return Err("ability unlearned");
        }
        let eval_rank = rank.max(1);

        if !ab.castable_while_cc
            && (e.status.silence_until > self.tick
                || e.status.stun_until > self.tick
                || e.hero_kit
                    .as_deref()
                    .is_some_and(|k| self.tick < k.fear_until || self.tick < k.sleep_until))
        {
            return Err("crowd controlled");
        }

        let is_recast = e
            .hero_kit
            .as_deref()
            .is_some_and(|k| self.tick < k.recast_until[slot.idx()]);

        let max_c = self.kit_max_charges(i, slot);
        if !is_recast {
            if max_c > 0 {
                if self.kit_ammo_charges(i, slot) == 0 || self.tick < e.cooldown[slot.idx()] {
                    return Err("cooldown");
                }
            } else if self.tick < e.cooldown[slot.idx()] {
                return Err("cooldown");
            }
        }

        // Check preconditions.
        match ab.gate {
            CastGate::None => {}
            CastGate::RequiresFullResource => {
                if self.kit_resource_value(i) + 0.01 < self.kit_resource_max(i) {
                    return Err("insufficient resource");
                }
            }
            CastGate::RequiresTetheredTarget => {
                let owner_id = e.id;
                let has_tether = self.entities.spells::<KitEffect>().any(|eff| {
                    eff.owner_id == owner_id && matches!(eff.kind, KitEffectKind::Tether { .. })
                });
                if !has_tether {
                    return Err("no tethered target");
                }
            }
            CastGate::RequiresLockedTarget => {
                let has_lock = e
                    .hero_kit
                    .as_deref()
                    .is_some_and(|k| k.lock_target_id != 0 && self.tick < k.lock_target_until);
                if !has_lock {
                    return Err("no locked target");
                }
            }
            CastGate::RequiresRebootReady => {
                let ready = match e.reboot {
                    None => true,
                    Some(crate::reboot::Phase::Recharging { until }) => self.tick >= until,
                    Some(crate::reboot::Phase::Rebooting { .. }) => false,
                };
                if !ready {
                    return Err("reboot on cooldown");
                }
            }
            CastGate::RequiresNotRecentlyDamaged => {
                let last_dmg = e.hero_kit.as_deref().map_or(0, |k| k.last_damaged_tick);
                let lock_ticks = (seconds_to_ticks(3.5))
                    .saturating_sub(((e.item_stats.bonus_weapon * 0.015) * TICK_HZ as f32) as u64);
                if last_dmg > 0 && self.tick < last_dmg + lock_ticks {
                    return Err("recently damaged");
                }
            }
            CastGate::RequiresMusicalChord3 => {
                let ready = e
                    .hero_kit
                    .as_deref()
                    .is_some_and(|k| k.musical_note_count >= 3);
                if !ready {
                    return Err("requires 3 musical notes");
                }
            }
            CastGate::RequiresEmpowered => {
                let ready = e
                    .hero_kit
                    .as_deref()
                    .is_some_and(|k| k.empowered_attacks > 0 || self.tick < k.empowered_until);
                if !ready {
                    return Err("requires empowerment");
                }
            }
            CastGate::RequiresOutOfCombat => {
                let last = e
                    .hero_kit
                    .as_deref()
                    .map_or(0, |k| k.last_damaged_tick.max(k.last_attack_tick));
                if last > 0 && self.tick < last + TICK_HZ * 3 {
                    return Err("in combat");
                }
            }
        }

        let bonus = self.kit_bonus_stats(i);
        let inputs = EvalInputs {
            cp: e.item_stats.crystal + bonus.crystal_flat,
            wp: e.stats.weapon + bonus.weapon_flat,
            bonus_wp: e.item_stats.bonus_weapon + bonus.weapon_flat,
            bonus_hp: e.bonus_hp,
            alt: (e.max_energy - 200.0).max(0.0),
            bonus_as: (e.item_stats.attack_speed + bonus.attack_speed).max(0.0),
        };
        let eval_f = |vs: ValueSpec| -> f32 {
            if let Some(book) = &self.abilities.hero_kits {
                book.eval_value(e.type_id, ab.ability_id, eval_rank, vs, inputs)
            } else {
                vs.eval_fallback(eval_rank, inputs)
            }
        };

        let mut cost = if is_recast { 0.0 } else { eval_f(ab.cost) };
        if let Some(t) = e.hero_kit.as_deref().and_then(|k| k.talent.as_ref()) {
            let loaded = self.abilities.hero_kits.as_ref().map(|b| &b.talents);
            if let Some(em) = talents_runtime::eval_talent_mod(
                &t.id,
                "ENERGY_MODIFIER",
                t.level,
                inputs.cp,
                inputs.wp,
                loaded,
            ) && find_talent(&t.id).is_some_and(|s| s.scope.matches(slot))
            {
                cost *= em.max(0.0);
            }
        }

        match spec.resource {
            ResourceKind::Energy => {
                if e.energy + 0.01 < cost {
                    return Err("insufficient energy");
                }
            }
            ResourceKind::None => {}
            _ => {
                if self.kit_resource_value(i) + 0.01 < cost {
                    return Err("insufficient resource");
                }
            }
        }

        let mut range = eval_f(ab.range);
        if let Some(t) = e.hero_kit.as_deref().and_then(|k| k.talent.as_ref()) {
            let loaded = self.abilities.hero_kits.as_ref().map(|b| &b.talents);
            range += talents_runtime::talent_bonus_range(t, slot, inputs.cp, inputs.wp, loaded);
        }
        range = range.max(1.5);

        match (ab.targeting, target) {
            (CastTargeting::TargetEnemy, CastTarget::Entity(id)) => {
                let j = self.index(id).ok_or("unknown target")?;
                let t = &self.entities[j];
                if t.team == e.team || t.hp <= 0.0 {
                    return Err("invalid target");
                }
                if e.position.dist(t.position) > range + 0.75 {
                    return Err("cast out of range");
                }
            }
            (CastTargeting::TargetEnemy, _) => {
                return Err("target required");
            }
            (
                CastTargeting::TargetAlly | CastTargeting::TargetAllyOrSelf,
                CastTarget::Entity(id),
            ) => {
                let j = self.index(id).ok_or("unknown target")?;
                let t = &self.entities[j];
                if t.team != e.team || t.hp <= 0.0 {
                    return Err("invalid ally target");
                }
                if ab.targeting == CastTargeting::TargetAlly && j == i {
                    return Err("cannot target self");
                }
                if e.position.dist(t.position) > range + 0.75 {
                    return Err("cast out of range");
                }
            }
            (CastTargeting::TargetAnyHero, CastTarget::Entity(id)) => {
                let j = self.index(id).ok_or("unknown target")?;
                let t = &self.entities[j];
                if t.kind != Kind::Hero || t.hp <= 0.0 {
                    return Err("invalid hero target");
                }
                if e.position.dist(t.position) > range + 0.75 {
                    return Err("cast out of range");
                }
            }
            (CastTargeting::GroundPoint, CastTarget::Point(p))
            | (CastTargeting::TargetLocationOrEntity, CastTarget::Point(p)) => {
                if range < 180.0 && e.position.dist(p) > range + 0.75 {
                    return Err("cast out of range");
                }
            }
            (CastTargeting::Directional | CastTargeting::InstantSelf, _)
            | (
                CastTargeting::TargetAllyOrSelf
                | CastTargeting::TargetAnyHero
                | CastTargeting::GroundPoint
                | CastTargeting::TargetLocationOrEntity,
                _,
            ) => {}
            (CastTargeting::TargetAlly, _) => {
                return Err("ally target required");
            }
        }

        let mut cd_sec = eval_f(ab.cooldown);
        if let Some(t) = e.hero_kit.as_deref().and_then(|k| k.talent.as_ref()) {
            let loaded = self.abilities.hero_kits.as_ref().map(|b| &b.talents);
            cd_sec = talents_runtime::apply_cooldown_modifier(
                t, slot, cd_sec, inputs.cp, inputs.wp, loaded,
            );
        }
        let cda = 1.0 + e.item_stats.cooldown + bonus.cooldown_accel;
        let cd_ticks = seconds_to_ticks((cd_sec / cda.max(0.2)).max(0.0));
        let windup_ticks = seconds_to_ticks(eval_f(ab.startup));

        Ok((
            eval_rank,
            cost,
            range,
            cd_ticks,
            windup_ticks == 0 || is_recast,
        ))
    }

    /// Execute a validated HeroKit cast on entity `i`.
    pub(crate) fn kit_begin_or_release_cast(
        &mut self,
        i: usize,
        slot: AbilitySlot,
        target: CastTarget,
        events: &mut Vec<Event>,
    ) -> Result<bool, &'static str> {
        let (eval_rank, cost, _range, cd_ticks, _immediate) = self.kit_castable(i, slot, target)?;
        let ab = self
            .active_ability_spec(i, slot)
            .ok_or("ability unavailable")?;
        let spec =
            crate::hero_kits::kit_spec(self.entities[i].type_id).ok_or("ability unavailable")?;

        let is_recast = self.entities[i]
            .hero_kit
            .as_deref()
            .is_some_and(|k| self.tick < k.recast_until[slot.idx()]);

        if is_recast {
            let mut ctx = HeroKitContext::new(self, events, i);
            let handled = if let Some(custom) = ab.custom_exec {
                custom(&mut ctx, slot, eval_rank, target)?
            } else {
                false
            };
            if !handled {
                ctx.exec_primitives(ab.ability_id, slot, eval_rank, target, ab.effects);
            }
            let still_recast = ctx.room.entities[i]
                .hero_kit
                .as_deref()
                .is_some_and(|k| ctx.tick < k.recast_until[slot.idx()]);
            if !still_recast {
                ctx.room.entities[i].cooldown[slot.idx()] = ctx.tick + cd_ticks;
            }
            return Ok(true);
        }

        // Spend cost and apply cooldown/charges cleanly.
        match spec.resource {
            ResourceKind::Energy => self.entities[i].energy -= cost,
            ResourceKind::None => {}
            _ => {
                let max = self.kit_resource_max(i);
                let cur = self.kit_resource_value(i);
                let k = self.entities[i].hero_kit.get_or_insert_with(Box::default);
                k.resource = (cur - cost).clamp(0.0, max);
                k.resource_initialized = true;
            }
        }

        let max_c = self.kit_max_charges(i, slot);
        if max_c > 0 {
            let cur_c = self.kit_ammo_charges(i, slot);
            let next_c = cur_c.saturating_sub(1);
            let tick = self.tick;
            let k = self.entities[i].hero_kit.get_or_insert_with(Box::default);
            k.charges[slot.idx()] = next_c;
            k.charges_initialized[slot.idx()] = true;
            if k.charge_regen_at[slot.idx()] <= tick {
                k.charge_regen_at[slot.idx()] = tick + cd_ticks.max(1);
            }
            let between = seconds_to_ticks(
                ab.between_charges_cd
                    .eval_fallback(eval_rank, EvalInputs::default()),
            );
            self.entities[i].cooldown[slot.idx()] = self.tick + between;
        } else {
            self.entities[i].cooldown[slot.idx()] = self.tick + cd_ticks;
        }

        self.kit_execute_release(i, slot, eval_rank, target, events)?;
        Ok(true)
    }

    pub(crate) fn kit_windup_ticks(&self, i: usize, slot: AbilitySlot) -> u64 {
        let Some(ab) = self.active_ability_spec(i, slot) else {
            return 0;
        };
        let rank = self.entities[i].ability_ranks[slot.idx()].max(1);
        let inputs = EvalInputs::default();
        let sec = if let Some(book) = &self.abilities.hero_kits {
            book.eval_value(
                self.entities[i].type_id,
                ab.ability_id,
                rank,
                ab.startup,
                inputs,
            )
        } else {
            ab.startup.eval_fallback(rank, inputs)
        };
        seconds_to_ticks(sec)
    }

    pub(crate) fn kit_execute_release(
        &mut self,
        i: usize,
        slot: AbilitySlot,
        rank: u8,
        target: CastTarget,
        events: &mut Vec<Event>,
    ) -> Result<(), &'static str> {
        let ab = self
            .active_ability_spec(i, slot)
            .ok_or("ability unavailable")?;
        let spec =
            crate::hero_kits::kit_spec(self.entities[i].type_id).ok_or("ability unavailable")?;
        let mut ctx = HeroKitContext::new(self, events, i);
        let handled = if let Some(custom) = ab.custom_exec {
            custom(&mut ctx, slot, rank, target)?
        } else {
            false
        };
        if !handled {
            ctx.exec_primitives(ab.ability_id, slot, rank, target, ab.effects);
        }
        if let Some(on_cast) = spec.perk.on_ability_cast {
            on_cast(&mut ctx, slot, rank, target);
        }
        Ok(())
    }

    /// Aggregate active temporary buffs, passive skill bonuses, and talent stats on entity `i`.
    pub(crate) fn kit_bonus_stats(&self, i: usize) -> KitStatBonus {
        let mut b = KitStatBonus::default();
        let e = &self.entities[i];
        if !self.hero_kit_enabled() && e.hero_kit.is_none() {
            return b;
        }
        if let Some(spec) = crate::hero_kits::kit_spec(e.type_id) {
            for (slot_idx, ab) in [(0usize, &spec.a), (1usize, &spec.b), (2usize, &spec.c)] {
                let r = e.ability_ranks[slot_idx];
                if r > 0
                    && let Some((stat, vs)) = ab.passive_bonus
                {
                    let val = vs.eval_fallback(r, EvalInputs::default());
                    apply_stat_to_bonus(&mut b, stat, val);
                }
            }
        }
        if let Some(k) = e.hero_kit.as_deref() {
            for buff in &k.buffs {
                if self.tick < buff.until {
                    apply_stat_to_bonus(&mut b, buff.stat, buff.amount);
                }
            }
            if self.tick < k.stealth_until {
                b.move_speed_flat += k.stealth_speed;
            }
            if let Some(t) = &k.talent {
                let loaded = self.abilities.hero_kits.as_ref().map(|bk| &bk.talents);
                let tb = talents_runtime::talent_passive_stats(
                    t,
                    e.item_stats.crystal,
                    e.stats.weapon,
                    loaded,
                );
                b.weapon_pct += tb.weapon_pct;
                b.weapon_flat += tb.weapon_flat;
                b.crystal_pct += tb.crystal_pct;
                b.crystal_flat += tb.crystal_flat;
                b.attack_speed += tb.attack_speed;
                b.armor_pct += tb.armor_pct;
                b.armor_flat += tb.armor_flat;
                b.shield_pct += tb.shield_pct;
                b.shield_flat += tb.shield_flat;
                b.move_speed_flat += tb.move_speed_flat;
                b.move_speed_pct += tb.move_speed_pct;
                b.attack_range += tb.attack_range;
                b.cooldown_accel += tb.cooldown_accel;
                b.crit_chance += tb.crit_chance;
                b.crit_damage += tb.crit_damage;
                b.armor_pierce += tb.armor_pierce;
                b.shield_pierce += tb.shield_pierce;
                b.lifesteal += tb.lifesteal;
                b.hp_regen_per_sec += tb.hp_regen_per_sec;
                b.max_resource_bonus += tb.max_resource_bonus;
            }
        }
        b
    }

    /// 20Hz per-actor HeroKit maintenance: resource regen, ammo recharge, HP regen talents, perk ticks.
    pub(crate) fn tick_hero_kits(&mut self, events: &mut Vec<Event>) {
        if !self.hero_kit_enabled() {
            return;
        }
        let actor_count = self.entities.len();
        for i in 0..actor_count {
            if self.entities[i].hp <= 0.0 || self.entities[i].kind != Kind::Hero {
                continue;
            }
            let type_id = self.entities[i].type_id;
            let Some(spec) = crate::hero_kits::kit_spec(type_id) else {
                continue;
            };

            // Prune expired buffs/marks cleanly.
            let tick = self.tick;
            let is_stopped = matches!(self.entities[i].order, crate::Command::Stop);
            if is_stopped && spec.perk.on_tick.is_some() {
                self.entities[i].hero_kit.get_or_insert_with(Box::default);
            }
            if let Some(k) = self.entities[i].hero_kit.as_deref_mut() {
                k.buffs.retain(|b| b.until > tick);
                k.marks.retain(|_, m| m.until > tick);
                if is_stopped {
                    k.stationary_ticks = k.stationary_ticks.saturating_add(1);
                } else {
                    k.stationary_ticks = 0;
                }
            }

            // Regenerate non-Energy resources (Focus / Stamina / Vengeance / Bloodrage decay).
            if spec.resource != ResourceKind::Energy && spec.resource != ResourceKind::None {
                let regen = spec.resource_regen_per_sec
                    + self.entities[i].item_stats.energy_regen * spec.resource_energy_conversion.1;
                if regen != 0.0 {
                    let max_r = self.kit_resource_max(i);
                    let cur_r = self.kit_resource_value(i);
                    let delta = regen / TICK_HZ as f32;
                    let next = (cur_r + delta).clamp(0.0, max_r);
                    if (next - cur_r).abs() > 1e-5 {
                        let k = self.entities[i].hero_kit.get_or_insert_with(Box::default);
                        k.resource = next;
                        k.resource_initialized = true;
                    }
                }
            }

            // Recharge ability ammo charges when needed.
            for slot in [AbilitySlot::A, AbilitySlot::B, AbilitySlot::C] {
                let max_c = self.kit_max_charges(i, slot);
                if max_c == 0 {
                    continue;
                }
                let cur_c = self.kit_ammo_charges(i, slot);
                if cur_c < max_c
                    && self.entities[i]
                        .hero_kit
                        .as_deref()
                        .is_some_and(|k| tick >= k.charge_regen_at[slot.idx()])
                {
                    let slot_rank = self.entities[i].ability_ranks[slot.idx()].max(1);
                    let next_cd = self.active_ability_spec(i, slot).map_or(3.0, |a| {
                        a.cooldown.eval_fallback(slot_rank, EvalInputs::default())
                    });
                    if let Some(k) = self.entities[i].hero_kit.as_deref_mut() {
                        k.charges[slot.idx()] = (cur_c + 1).min(max_c);
                        k.charges_initialized[slot.idx()] = true;
                        if k.charges[slot.idx()] < max_c {
                            k.charge_regen_at[slot.idx()] = tick + seconds_to_ticks(next_cd).max(1);
                        }
                    }
                }
            }

            // Passive HP regen from talents (e.g., Talent_BonusRegen, Phinn RaidBoss, Flicker FairyHealing).
            let bonus = self.kit_bonus_stats(i);
            let mut regen_hp = bonus.hp_regen_per_sec / TICK_HZ as f32;
            if let Some(t) = self.entities[i]
                .hero_kit
                .as_deref()
                .and_then(|k| k.talent.as_ref())
                && let Some(sp) = find_talent(&t.id).map(|s| s.special)
            {
                let loaded = self.abilities.hero_kits.as_ref().map(|b| &b.talents);
                let cp = self.entities[i].item_stats.crystal;
                let wp = self.entities[i].stats.weapon;
                match sp {
                    TalentSpecial::PhinnRapidRegen => {
                        if let Some(pct) = talents_runtime::eval_talent_mod(
                            &t.id,
                            "CURRENT_HEALTH_REGEN",
                            t.level,
                            cp,
                            wp,
                            loaded,
                        ) {
                            regen_hp += (self.entities[i].hp * pct) / TICK_HZ as f32;
                        }
                    }
                    TalentSpecial::FlickerStealthRegen => {
                        let stealthed = self.entities[i]
                            .hero_kit
                            .as_deref()
                            .is_some_and(|k| tick < k.stealth_until);
                        if stealthed
                            && let Some(pct) = talents_runtime::eval_talent_mod(
                                &t.id,
                                "MAX_HEALTH_TO_HEALING",
                                t.level,
                                cp,
                                wp,
                                loaded,
                            )
                        {
                            regen_hp += (self.entities[i].max_hp * pct) / TICK_HZ as f32;
                        }
                    }
                    _ => {}
                }
            }
            if regen_hp > 0.0 && self.entities[i].hp < self.entities[i].max_hp {
                let id = self.entities[i].id;
                self.heal(id, i, regen_hp, events);
            }

            // Heroic perk per-tick callback.
            if let Some(on_tick) = spec.perk.on_tick {
                let mut ctx = HeroKitContext::new(self, events, i);
                on_tick(&mut ctx);
            }
        }
    }

    /// Hook called inside `apply_damage` for HeroKit fortified HP absorption & perk reactions.
    pub(crate) fn kit_modify_incoming_damage(
        &mut self,
        source_id: u32,
        target_idx: usize,
        raw: f32,
        school: DamageSchool,
        events: &mut Vec<Event>,
    ) -> f32 {
        if target_idx >= self.entities.len() || raw <= 0.0 {
            return raw;
        }
        let src_idx = self.index(source_id);
        let mut amount = raw;

        // Apply active damage reduction from buffs/perks.
        let bonus = self.kit_bonus_stats(target_idx);
        if bonus.damage_reduction > 0.0 {
            amount *= (1.0 - bonus.damage_reduction.clamp(0.0, 0.85)).max(0.15);
        }

        // Tony Legendary C: immune to taunted attacker.
        if let Some(si) = src_idx
            && self.entities[si].hero_kit.as_deref().is_some_and(|k| {
                self.tick < k.taunt_until && k.taunt_target == self.entities[target_idx].id
            })
            && self.entities[target_idx]
                .hero_kit
                .as_deref()
                .and_then(|k| k.talent.as_ref())
                .and_then(|t| find_talent(&t.id))
                .is_some_and(|s| s.special == TalentSpecial::TonyImmuneToTaunted)
        {
            return 0.0;
        }

        // Fortified Health absorbs 50% of remaining damage.
        let tick = self.tick;
        if let Some(k) = self.entities[target_idx].hero_kit.as_deref_mut()
            && tick < k.fortified_until
            && k.fortified_health > 0.0
        {
            let saved = k.fortified_health.min(amount * 0.5);
            k.fortified_health -= saved;
            amount -= saved;
        }

        // Record combat timestamp & trigger on_take_damage perk.
        if let Some(k) = self.entities[target_idx].hero_kit.as_deref_mut() {
            k.last_damaged_tick = tick;
            if tick < k.sleep_until && !k.sleep_unwaking {
                k.sleep_until = 0;
            }
        }
        if self.hero_kit_enabled()
            && let Some(spec) = crate::hero_kits::kit_spec(self.entities[target_idx].type_id)
            && let Some(cb) = spec.perk.on_take_damage
        {
            let mut ctx = HeroKitContext::new(self, events, target_idx);
            let mult = cb(&mut ctx, src_idx, amount, school);
            amount *= mult.clamp(0.0, 2.0);
        }

        // Adagio Arcane Renewal: any damage to enemy burning with Arcane Fire restores energy to Adagio.
        if self.hero_kit_enabled()
            && let Some(mark) = self.entities[target_idx]
                .hero_kit
                .as_deref()
                .and_then(|k| k.marks.get(&marks::ARCANE_FIRE).copied())
            && self.tick < mark.until
            && let Some(adagio_idx) = self.index(mark.source_id)
            && adagio_idx != target_idx
            && self.entities[adagio_idx].type_id == 244
        {
            let adagio = &mut self.entities[adagio_idx];
            adagio.energy = (adagio.energy + amount * 0.35).min(adagio.max_energy);
        }

        // Churnwalker Futility of Life: heal Churnwalker & share damage with other tethered enemies.
        if self.hero_kit_enabled()
            && let Some(cw_id) = self.entities[target_idx]
                .hero_kit
                .as_deref()
                .and_then(|k| k.marks.get(&marks::CHURNWALKER_TETHER).copied())
                .filter(|m| self.tick < m.until)
                .map(|m| m.source_id)
            && source_id != cw_id
            && let Some(cw_idx) = self.index(cw_id)
            && self.entities[cw_idx].hp > 0.0
        {
            let (heal_ratio, share_ratio) = if self.entities[cw_idx]
                .hero_kit
                .as_deref()
                .and_then(|k| k.talent.as_ref())
                .and_then(|t| find_talent(&t.id))
                .is_some_and(|s| s.special == TalentSpecial::ChurnwalkerHealShareTrade)
            {
                (0.5, 0.15)
            } else {
                (0.25, 0.30)
            };
            self.heal(cw_id, cw_idx, amount * heal_ratio, events);
            let other_chained: Vec<usize> = self
                .entities
                .iter()
                .enumerate()
                .filter_map(|(j, e)| {
                    (j != target_idx
                        && e.hp > 0.0
                        && e.hero_kit
                            .as_deref()
                            .and_then(|k| k.marks.get(&marks::CHURNWALKER_TETHER))
                            .is_some_and(|m| self.tick < m.until && m.source_id == cw_id))
                    .then_some(j)
                })
                .collect();
            for oj in other_chained {
                let team = self.entities[cw_idx].team;
                self.hurt(
                    cw_id,
                    team,
                    oj,
                    amount * share_ratio,
                    DamageSchool::True,
                    events,
                );
            }
        }

        amount.max(0.0)
    }

    /// Placeholder invoked cleanly from `deal_damage(..., triggers_basic=true)` when an ability triggers basic-attack items.
    fn item_stats_trigger_basic_placeholder(&mut self, _caster: usize, _target: usize) -> bool {
        false
    }

    /// Telemetry helper for UI and unit tests: query active alternate form (`0` or `1`).
    pub fn kit_form(&self, actor_id: u32) -> u8 {
        self.index(actor_id)
            .and_then(|i| self.entities[i].hero_kit.as_deref())
            .map_or(0, |k| k.form)
    }

    /// Telemetry helper: query active charges on `slot` (or Caine's Peacemaker ammo clip on slot A).
    pub fn kit_ammo(&self, actor_id: u32, slot: AbilitySlot) -> u8 {
        self.index(actor_id).map_or(0, |i| {
            let e = &self.entities[i];
            if slot == AbilitySlot::A
                && crate::hero_kits::kit_spec(e.type_id).is_some_and(|s| s.hero_id == "Caine")
            {
                let has_double = e
                    .hero_kit
                    .as_deref()
                    .and_then(|k| k.talent.as_ref())
                    .and_then(|t| find_talent(&t.id))
                    .is_some_and(|s| s.special == TalentSpecial::CaineDoubleBullets);
                let cap = if has_double { 12 } else { 6 };
                return e.hero_kit.as_deref().map_or(cap, |k| {
                    if k.timers[0] == 0 {
                        cap
                    } else {
                        k.counters[0].min(255) as u8
                    }
                });
            }
            self.kit_ammo_charges(i, slot)
        })
    }

    /// Telemetry helper: query current non-energy resource (`Focus`, `Stamina`, `Vengeance`, `Bloodrage`).
    pub fn kit_resource(&self, actor_id: u32) -> f32 {
        self.index(actor_id)
            .map_or(0.0, |i| self.kit_resource_value(i))
    }

    /// Telemetry helper: query active mark stacks on an actor.
    pub fn kit_mark_stacks(&self, actor_id: u32, mark: u8) -> u8 {
        self.index(actor_id)
            .and_then(|i| self.entities[i].hero_kit.as_deref())
            .and_then(|k| k.marks.get(&mark))
            .filter(|m| self.tick < m.until)
            .map_or(0, |m| m.stacks)
    }

    /// Hook called when hero `attacker` releases a basic attack hitting `target_idx`.
    /// Handles empowered basic-attack charges, perk basic-attack callbacks, cleave, and Target Lock.
    pub(crate) fn kit_on_basic_attack(
        &mut self,
        attacker: usize,
        target_idx: usize,
        base_damage: f32,
        was_crit: bool,
        events: &mut Vec<Event>,
    ) {
        if !self.hero_kit_enabled()
            || attacker >= self.entities.len()
            || target_idx >= self.entities.len()
        {
            return;
        }
        let type_id = self.entities[attacker].type_id;
        let target_id = self.entities[target_idx].id;

        let tick = self.tick;
        if let Some(k) = self.entities[attacker].hero_kit.as_deref_mut() {
            k.last_attack_tick = tick;
            if tick >= k.stealth_grace_until {
                k.stealth_until = 0;
            }
            if k.lock_target_id == 0 || k.lock_target_id != target_id || tick >= k.lock_target_until
            {
                k.lock_target_id = target_id;
            }
            k.lock_target_until = tick + TICK_HZ * 4;
        }

        // Consume empowered basic attack charge if active.
        let emp = self.entities[attacker]
            .hero_kit
            .as_deref_mut()
            .and_then(|k| {
                if k.empowered_attacks > 0 && tick < k.empowered_until {
                    k.empowered_attacks -= 1;
                    Some((
                        k.empowered_bonus_damage,
                        k.empowered_is_crystal,
                        k.empowered_stun_sec,
                        k.empowered_slow_pct,
                        k.empowered_slow_sec,
                        k.empowered_knockback,
                    ))
                } else {
                    None
                }
            });
        let mut ctx = HeroKitContext::new(self, events, attacker);
        if let Some((bonus_dmg, is_cp, stun_sec, slow_pct, slow_sec, kb)) = emp {
            if bonus_dmg > 0.0 {
                let sch = if is_cp {
                    DamageSchool::Crystal
                } else {
                    DamageSchool::Weapon
                };
                ctx.deal_damage(target_idx, bonus_dmg, sch, 0.0, false);
            }
            if stun_sec > 0.0 {
                ctx.apply_stun(target_idx, stun_sec);
            }
            if slow_pct > 0.0 && slow_sec > 0.0 {
                ctx.apply_slow(target_idx, slow_pct, slow_sec);
            }
            if kb > 0.0 {
                let pos = ctx.caster_pos();
                ctx.knockback(target_idx, pos, kb, 0.0);
            }
        }

        // Trigger Heroic Perk basic attack hook & built-in cleave/splash.
        if let Some(spec) = crate::hero_kits::kit_spec(type_id) {
            if let Some(on_aa) = spec.perk.on_basic_attack {
                let (bonus_dmg, school) = on_aa(&mut ctx, target_idx, base_damage, was_crit);
                if bonus_dmg > 0.0 {
                    ctx.deal_damage(target_idx, bonus_dmg, school, 0.0, false);
                }
            }
            if spec.perk.basic_cleave_radius > 0.0 && spec.perk.basic_cleave_fraction > 0.0 {
                let center = ctx.room.entities[target_idx].position;
                let cleave_dmg = base_damage * spec.perk.basic_cleave_fraction;
                let nearby = ctx.enemies_in_radius(center, spec.perk.basic_cleave_radius, false);
                for oj in nearby {
                    if oj != target_idx {
                        let m = if ctx.room.entities[oj].kind == Kind::Hero {
                            1.0
                        } else {
                            0.5
                        };
                        ctx.deal_damage(oj, cleave_dmg * m, DamageSchool::Weapon, 0.0, false);
                    }
                }
            }
        }
    }

    /// Hook called on unit kill or assist to trigger Heroic Perks & kill/assist Talents.
    pub(crate) fn kit_on_kill_assist(
        &mut self,
        actor: usize,
        victim: usize,
        is_killer: bool,
        events: &mut Vec<Event>,
    ) {
        if !self.hero_kit_enabled() || actor >= self.entities.len() || victim >= self.entities.len()
        {
            return;
        }
        let is_hero = self.entities[victim].kind == Kind::Hero;
        let type_id = self.entities[actor].type_id;
        let mut ctx = HeroKitContext::new(self, events, actor);

        if is_hero
            && let Some(t) = ctx.active_talent().cloned()
            && let Some(s) = find_talent(&t.id)
        {
            match s.special {
                TalentSpecial::RingoAnotherRoundKillReset => {
                    ctx.reset_slot_cd(AbilitySlot::C);
                }
                TalentSpecial::AnkaRefreshAllOnKill => {
                    ctx.reset_slot_cd(AbilitySlot::A);
                    ctx.reset_slot_cd(AbilitySlot::B);
                    ctx.reset_slot_cd(AbilitySlot::C);
                }
                TalentSpecial::IshtarKillStackDefenses => {
                    ctx.add_mark(actor, marks::ISHTAR_INDOMITABLE, 1, 10, 3600.0);
                }
                _ => {}
            }
        }

        if let Some(spec) = crate::hero_kits::kit_spec(type_id)
            && let Some(cb) = spec.perk.on_kill_assist
        {
            cb(&mut ctx, victim, is_killer, is_hero);
        }
    }

    /// Clean temporary HeroKit combat state on death while retaining permanent stacks & talent.
    pub(crate) fn kit_on_death(&mut self, i: usize) {
        if i >= self.entities.len() {
            return;
        }
        let type_id = self.entities[i].type_id;
        let tick = self.tick;
        let Some(k) = self.entities[i].hero_kit.as_deref_mut() else {
            return;
        };
        k.fear_until = 0;
        k.fear_source = 0;
        k.sleep_until = 0;
        k.sleep_unwaking = false;
        k.taunt_until = 0;
        k.taunt_target = 0;
        k.stealth_until = 0;
        k.stealth_grace_until = 0;
        k.stealth_speed = 0.0;
        k.unstoppable_until = 0;
        k.untargetable_until = 0;
        k.fortified_health = 0.0;
        k.fortified_until = 0;
        k.mortal_wound_until = 0;
        k.empowered_attacks = 0;
        k.empowered_until = 0;
        k.recast_until = [0; 3];
        k.recast_target = [0; 3];
        k.step_stage = [0; 3];
        k.buffs.retain(|b| b.until >= tick + TICK_HZ * 600);
        if crate::hero_kits::kit_spec(type_id).is_some_and(|s| s.hero_id == "Ishtar")
            && let Some(m) = k.marks.get_mut(&marks::ISHTAR_INDOMITABLE)
        {
            m.stacks = m.stacks.saturating_sub(1);
            if m.stacks == 0 {
                k.marks.remove(&marks::ISHTAR_INDOMITABLE);
            }
        }
    }
}

fn apply_stat_to_bonus(b: &mut KitStatBonus, stat: BuffStatKind, val: f32) {
    match stat {
        BuffStatKind::MoveSpeedFlat => b.move_speed_flat += val,
        BuffStatKind::MoveSpeedPct => b.move_speed_pct += val,
        BuffStatKind::AttackSpeed => b.attack_speed += val,
        BuffStatKind::Armor => b.armor_flat += val,
        BuffStatKind::Shield => b.shield_flat += val,
        BuffStatKind::WeaponPowerFlat => b.weapon_flat += val,
        BuffStatKind::WeaponPowerPct => b.weapon_pct += val,
        BuffStatKind::CrystalPowerFlat => b.crystal_flat += val,
        BuffStatKind::CrystalPowerPct => b.crystal_pct += val,
        BuffStatKind::Lifesteal => b.lifesteal += val,
        BuffStatKind::DamageReduction => b.damage_reduction += val,
        BuffStatKind::FortifiedHealth => {}
        BuffStatKind::AttackRange => b.attack_range += val,
        BuffStatKind::CooldownReduction => b.cooldown_accel += val,
        BuffStatKind::CritChance => b.crit_chance += val,
        BuffStatKind::DamageAmp => b.damage_amp += val,
    }
}
