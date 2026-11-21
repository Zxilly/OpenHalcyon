//! Unified, deterministic Hero Ability, Heroic Perk, and Talent execution engine.
//!
//! Keeps inline `Entity` bloat to a single `Option<Box<HeroKitState>>` (8 bytes)
//! and executes active multi-tick zones, projectiles, traps, segment tripwires,
//! summoned pets, dashes, channels, tethers, and attachments through one sparse
//! `KitEffect` ECS `TimedSpell` family.

use std::collections::BTreeMap;

use bevy_ecs::component::Component;
use serde::Serialize;

use crate::combat::{AbilitySlot, CastTarget};
use crate::ecs::score::DamageSchool;
use crate::heroes::talents::ActiveTalentState;
use crate::{Position, TICK_HZ};

pub(crate) mod context;
pub(crate) mod effects;
pub(crate) mod eval;

pub(crate) use context::HeroKitContext;
pub(crate) use eval::{EvalInputs, HeroKitBook, ValueSpec};

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

pub(super) fn clamp_map_pos(p: Position) -> Position {
    Position {
        x: p.x.clamp(-120.0, 120.0),
        y: p.y.clamp(-80.0, 80.0),
    }
}

pub(super) fn dir_between(from: Position, to: Position) -> (f32, f32) {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let d = dx.hypot(dy);
    if d <= 0.0001 {
        (1.0, 0.0)
    } else {
        (dx / d, dy / d)
    }
}

pub(super) fn point_to_segment_dist(p: Position, a: Position, b: Position) -> f32 {
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

pub(super) fn apply_stat_to_bonus(b: &mut KitStatBonus, stat: BuffStatKind, val: f32) {
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
