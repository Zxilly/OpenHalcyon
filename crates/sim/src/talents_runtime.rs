//! Runtime evaluation, validation, and combat effects for all 189 Vainglory talents.

use std::collections::BTreeMap;
use std::num::NonZeroU8;

use halcyon_data::GameData;
use serde::Serialize;

use crate::combat::AbilitySlot;

/// Which ability slot(s) a talent's numerical modifiers target.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TalentSlotScope {
    None,
    SlotA,
    SlotB,
    SlotC,
    SlotAB,
    SlotBC,
    SlotAC,
    AllAbilities,
    Perk,
}

impl TalentSlotScope {
    pub(crate) const fn matches(self, slot: AbilitySlot) -> bool {
        matches!(
            (self, slot),
            (Self::SlotA, AbilitySlot::A)
                | (Self::SlotB, AbilitySlot::B)
                | (Self::SlotC, AbilitySlot::C)
                | (Self::SlotAB, AbilitySlot::A | AbilitySlot::B)
                | (Self::SlotBC, AbilitySlot::B | AbilitySlot::C)
                | (Self::SlotAC, AbilitySlot::A | AbilitySlot::C)
                | (Self::AllAbilities, _)
        )
    }
}

/// Rarity classification matching `talents_full.json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TalentRarity {
    Rare,
    Epic,
    Legendary,
    Generic,
}

/// Compile-time modifier specification from `talents_full.json`.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct TalentModSpec {
    pub label: &'static str,
    pub base: f32,
    pub per_level: f32,
    pub cp_ratio: f32,
    pub wp_ratio: f32,
    pub clamp: f32,
    pub is_percent_label: bool,
}

impl TalentModSpec {
    pub(crate) const fn new(
        label: &'static str,
        base: f32,
        per_level: f32,
        cp_ratio: f32,
        wp_ratio: f32,
        clamp: f32,
    ) -> Self {
        Self {
            label,
            base,
            per_level,
            cp_ratio,
            wp_ratio,
            clamp,
            is_percent_label: false,
        }
    }

    pub(crate) fn value_at(self, level: NonZeroU8, cp: f32, wp: f32) -> f32 {
        let lvl = f32::from(level.get().saturating_sub(1));
        let raw = self.base + lvl * self.per_level + self.cp_ratio * cp + self.wp_ratio * wp;
        if self.clamp > 0.0 {
            if self.per_level < 0.0 || self.clamp < self.base {
                raw.max(self.clamp)
            } else {
                raw.min(self.clamp)
            }
        } else {
            raw
        }
    }
}

/// Behavioral rules beyond simple scalar multipliers for specific talents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TalentSpecial {
    None,
    /// Legacy modeled talents handled directly by dedicated combat modules.
    ModeledLongShot,
    ModeledMobileBulwark,
    ModeledSpeedLoader,
    /// Generic 17 ARAL bonus stat talents (`Talent_Bonus*`).
    BonusAttackRange,
    BonusAttackSpeed,
    BonusAttackSpeedCooldown,
    BonusCooldown,
    BonusCritChance,
    BonusCritChanceDamage,
    BonusCritDamage,
    BonusCrystal,
    BonusCrystalPierce,
    BonusCrystalVamp,
    BonusDamageAttackSpeed,
    BonusDefense,
    BonusMovementSpeed,
    BonusRegen,
    BonusWeapon,
    BonusWeaponPierce,
    BonusWeaponVamp,
    /// Hero-specific special behaviors across all 61 heroes:
    AdagioGiftFortified,
    AdagioHealingVerseAllies,
    AlphaAccelerationMoveSpeed,
    AlphaPrimeTimeFortified,
    AnkaRefreshAllOnKill,
    ArdanFullSelfVanguard,
    ArdanKnockoutPush,
    BaptisteArmyDualWave,
    BaptisteReaperOverflowSouls,
    BaronTurboJetsTripleShot,
    BlackfeatherPointGuardAllies,
    CaineDoubleBullets,
    CaineOnceMoreInvuln,
    CaineHigherExecuteNoRefresh,
    CatherineAdvancingGuardPower,
    CatherineQuickPursuitCdr,
    CatherineShockwaveKnockbackStun,
    CelesteFourCollapses,
    CelesteChainSupernova,
    CelesteYoungStarDoT,
    ChurnwalkerHealShareTrade,
    ChurnwalkerTormentCloud,
    ChurnwalkerGrapplePull,
    FlickerDustRootAndSpeed,
    FlickerStealthRegen,
    FortressWolvesCritChance,
    FortressSpawnWolvesOnAb,
    GlaiveBloodsongStun,
    GlaiveTwistedDash,
    GlaiveAfterburnPuntStun,
    GraceUnitedFrontSelfHeal,
    GrumpjawEatMultiple,
    GrumpjawGrumpyPathDamage,
    GrumpjawHangryLifesteal,
    GwenThreeCards,
    IdrisTripleChakram,
    IdrisBothPaths,
    IdrisShimmerHeal,
    InaraDanceHealAllies,
    InaraInvulnOnDash,
    IshtarKillStackDefenses,
    IshtarWhirlRoot,
    IshtarBladeAttackSpeedStacks,
    JouleLeapResetNoStun,
    JouleThunderSpeed,
    KarasTwisterHeal,
    KarasDoubleGust,
    KarasDevilsDustAoE,
    KenseiMaxStacksOnHit,
    KenseiEmpoweredInvuln,
    KenseiResetOnUltHit,
    KestrelCamoRefillsGlimmershot,
    KestrelDoubleArrow,
    KestrelUltPierces,
    KineticTripleBurstA,
    KineticHitAReducesB,
    KoshkaAttacksReduceCdr,
    KoshkaUltAoEStun,
    KrulUltMaxWeakness,
    KrulSmiteHealsAllies,
    LanceWallGrantsAllyBarrier,
    LanceRollDamagesPath,
    LanceBasicReducesAC,
    LeoTurboMoveSpeed,
    LeoCycloneTaunt,
    LeoUltEmpowersBoth,
    LorelaiStunExtended,
    LorelaiUltInvulnerability,
    LyraTwinMissilesNoSlow,
    LyraGythianWardCleanseBarrier,
    MagnusMarkReveals,
    MagnusMarkNotConsumed,
    MagnusHitReducesAllCdr,
    MaleneShadowBExplosion,
    MaleneShadowAFear,
    MaleneLightBAllyBarrierBoom,
    MihoTrueBurstNoUnguarded,
    MihoUltHealsAllies,
    OzoUltBonusCharge,
    OzoCircusHealsAllies,
    OzoThirdBounceReset,
    PetalFastTrampoline,
    PetalUltKnockback,
    PetalSeedHeavyNoKnockback,
    PhinnBResetsAAndHalfCost,
    PhinnBGrantsSpeed,
    PhinnRapidRegen,
    ReimSpirePathDamage,
    ReimLongFortified,
    ReimChillBarrierSlowImmune,
    RezaDashBarrier,
    RezaDashAppliesFirestarter,
    RezaUltAttackSpeedDuration,
    RingoAnotherRoundKillReset,
    RingoSplitShotB,
    RingoSpikedBrewBounce,
    RonaWhirlwindMoveSpeed,
    RonaAttacksReduceA,
    RonaMaxBloodrage,
    SawPiercingBasics,
    SawRoadieGrantsSpinUpAndCdr,
    SawSpinUpSpeedNotSlow,
    SamuelFrequentPerk,
    SamuelSleepNeverWakes,
    SamuelSlowingDriftingDark,
    SanfengReverseFling,
    SanfengDivineFistSlow,
    SanfengCounterRefreshesA,
    SilvernailExtraBoltAndCRange,
    SilvernailThreeFlasks,
    SilvernailTripwireTriangle,
    SkaarfSpitfireDropsGoop,
    SkaarfSplitfireThreeCones,
    SkaarfFastFlashFire,
    SkyeSuriGrantsSpeed,
    SkyePiercingBarrage,
    TakaKakuHealInvuln,
    TakaUltChainsTargets,
    TonyJawbreakerAoE,
    TonyImmuneToTaunted,
    TonyPerkAspdBarrier,
    VaryaSpearStuns,
    VaryaFastArcBarrier,
    VaryaExtraGlobalStrikes,
    ViolaDoubleHarmonyStacks,
    ViolaBalladAmpAndDr,
    ViolaChordChargesAndUltReset,
    VoxExtraBounce,
    VoxBasicAttackBarrier,
    WarhawkGrenadeHealsAllies,
    WarhawkRocketMoreKnockback,
    WarhawkFiveBarrageMissiles,
    YatesFlailHeavyThirdHit,
    YatesFastUlt,
    YlvaThreeBindingMines,
    YlvaDropMineOnB,
}

/// Complete compile-time specification for one talent.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct TalentSpec {
    pub id: &'static str,
    /// `hero_id` from `talents_full.json` (`None` only for generic `Talent_Bonus*`; `Talent_Ringo_AnotherRound` restricts to Ringo `"Ringo"`).
    pub hero_id: Option<&'static str>,
    pub rarity: TalentRarity,
    pub scope: TalentSlotScope,
    pub mods: &'static [TalentModSpec],
    pub special: TalentSpecial,
}

/// Loaded formula for one modifier from `GameData` (`talents_full.json`), serialized deterministically.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct LoadedTalentMod {
    pub label: String,
    pub base: f32,
    pub per_level: f32,
    pub overdrive: f32,
    pub cp_ratio: f32,
    pub wp_ratio: f32,
    pub clamp: f32,
}

impl LoadedTalentMod {
    pub(crate) fn value_at(&self, level: NonZeroU8, cp: f32, wp: f32) -> f32 {
        let lvl = f32::from(level.get().saturating_sub(1));
        let raw = self.base + lvl * self.per_level + self.cp_ratio * cp + self.wp_ratio * wp;
        if self.clamp > 0.0 {
            if self.per_level < 0.0 || self.clamp < self.base {
                raw.max(self.clamp)
            } else {
                raw.min(self.clamp)
            }
        } else {
            raw
        }
    }
}

/// Loaded talent record from `GameData`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct LoadedTalent {
    pub id: String,
    pub hero: Option<String>,
    pub mods: BTreeMap<String, LoadedTalentMod>,
}

/// Serialized equipped-talent state stored inside `HeroKitState`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct ActiveTalentState {
    pub id: String,
    pub level: NonZeroU8,
}

/// Net stat adjustments contributed by an equipped talent at a given level.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TalentStatBonus {
    pub weapon_pct: f32,
    pub weapon_flat: f32,
    pub crystal_pct: f32,
    pub crystal_flat: f32,
    pub attack_speed: f32,
    pub armor_pct: f32,
    pub armor_flat: f32,
    pub shield_pct: f32,
    pub shield_flat: f32,
    pub move_speed_flat: f32,
    pub move_speed_pct: f32,
    pub attack_range: f32,
    pub crit_chance: f32,
    pub crit_damage: f32,
    pub cooldown_accel: f32,
    pub armor_pierce: f32,
    pub shield_pierce: f32,
    pub lifesteal: f32,
    pub hp_regen_per_sec: f32,
    pub max_resource_bonus: f32,
}

/// Load all talent definitions from `GameData` into deterministic `BTreeMap`.
pub(crate) fn load_talents_from_data(data: &GameData) -> BTreeMap<String, LoadedTalent> {
    let mut out = BTreeMap::new();
    for t in data.extra.combat_content().talents() {
        let mut mods = BTreeMap::new();
        for m in &t.modifiers {
            mods.insert(
                m.label.clone(),
                LoadedTalentMod {
                    label: m.label.clone(),
                    base: m.base,
                    per_level: m.per_level,
                    overdrive: m.overdrive,
                    cp_ratio: m.cp_ratio,
                    wp_ratio: m.wp_ratio,
                    clamp: m.clamp,
                },
            );
        }
        out.insert(
            t.id.clone(),
            LoadedTalent {
                id: t.id.clone(),
                hero: t.hero.clone(),
                mods,
            },
        );
    }
    out
}

/// Resolve compiled `TalentSpec` by exact talent id.
pub(crate) fn find_talent(id: &str) -> Option<&'static TalentSpec> {
    ALL_TALENTS.iter().find(|t| t.id == id)
}

/// Evaluate a modifier label on `talent_id` at `(level, cp, wp)`, preferring `GameData` if loaded.
pub(crate) fn eval_talent_mod(
    talent_id: &str,
    label: &str,
    level: NonZeroU8,
    cp: f32,
    wp: f32,
    loaded: Option<&BTreeMap<String, LoadedTalent>>,
) -> Option<f32> {
    if let Some(t) = loaded.and_then(|map| map.get(talent_id))
        && let Some(m) = t.mods.get(label)
    {
        return Some(m.value_at(level, cp, wp));
    }
    find_talent(talent_id).and_then(|spec| {
        spec.mods
            .iter()
            .find(|m| m.label == label)
            .map(|m| m.value_at(level, cp, wp))
    })
}

/// Check whether `talent_id` is eligible for hero (`hero_id`, e.g. `"Amael"`, `"Hero009"`, `"Sayoc"`).
#[allow(dead_code)]
pub(crate) fn talent_matches_hero(
    talent_id: &str,
    hero_id: &str,
    loaded: Option<&BTreeMap<String, LoadedTalent>>,
) -> Result<&'static TalentSpec, &'static str> {
    let spec = find_talent(talent_id).ok_or("unknown talent")?;
    if let Some(expected_hero) = spec.hero_id
        && !expected_hero.eq_ignore_ascii_case(hero_id)
    {
        return Err("talent belongs to another hero");
    }
    if spec.hero_id.is_none()
        && let Some(t) = loaded.and_then(|map| map.get(talent_id))
        && let Some(data_hero) = &t.hero
        && !data_hero.eq_ignore_ascii_case(hero_id)
    {
        return Err("talent belongs to another hero");
    }
    Ok(spec)
}

/// Helper checking cooldown modifier multiplier vs flat reduction vs attack CDR.
pub(crate) fn apply_cooldown_modifier(
    talent: &ActiveTalentState,
    slot: AbilitySlot,
    base_cd_sec: f32,
    cp: f32,
    wp: f32,
    loaded: Option<&BTreeMap<String, LoadedTalent>>,
) -> f32 {
    let Some(spec) = find_talent(&talent.id) else {
        return base_cd_sec;
    };
    if !spec.scope.matches(slot) {
        return base_cd_sec;
    }
    // Check explicit cooldown overrides first (e.g. Petal Bouncer = 1.0s override).
    if spec.special == TalentSpecial::PetalFastTrampoline
        && let Some(v) = eval_talent_mod(&talent.id, "COOLDOWN", talent.level, cp, wp, loaded)
    {
        return v.max(0.1);
    }
    if spec.special == TalentSpecial::ChurnwalkerTormentCloud
        && let Some(add_sec) = eval_talent_mod(
            &talent.id,
            "B_INCREASED_COOLDOWN",
            talent.level,
            cp,
            wp,
            loaded,
        )
    {
        return (base_cd_sec + add_sec).max(0.5);
    }
    // Skip Saw RoadRage where COOLDOWN_MODIFIER is flat reduction on basic attack, and Magnus C where it's per-hit CDR.
    if matches!(
        spec.special,
        TalentSpecial::SawRoadieGrantsSpinUpAndCdr | TalentSpecial::MagnusHitReducesAllCdr
    ) {
        return base_cd_sec;
    }
    if let Some(m) = eval_talent_mod(
        &talent.id,
        "COOLDOWN_MODIFIER",
        talent.level,
        cp,
        wp,
        loaded,
    ) {
        if m < 0.0 {
            return (base_cd_sec + m).max(0.5);
        }
        return (base_cd_sec * m).max(0.2);
    }
    base_cd_sec
}

/// Helper computing damage multiplier from an equipped talent for `slot` (or Perk/all).
pub(crate) fn talent_damage_multiplier(
    talent: &ActiveTalentState,
    slot: AbilitySlot,
    cp: f32,
    wp: f32,
    loaded: Option<&BTreeMap<String, LoadedTalent>>,
) -> f32 {
    let Some(spec) = find_talent(&talent.id) else {
        return 1.0;
    };
    // Bespoke legacy talents apply their own multipliers in combat.rs.
    if matches!(
        spec.special,
        TalentSpecial::ModeledLongShot
            | TalentSpecial::ModeledMobileBulwark
            | TalentSpecial::ModeledSpeedLoader
    ) {
        return 1.0;
    }
    if !spec.scope.matches(slot) {
        return 1.0;
    }
    if let Some(v) = eval_talent_mod(&talent.id, "DAMAGE_MODIFIER", talent.level, cp, wp, loaded)
        .or_else(|| eval_talent_mod(&talent.id, "DAMAGE_MODIFER", talent.level, cp, wp, loaded))
        .or_else(|| {
            eval_talent_mod(
                &talent.id,
                "ABILITY_DAMAGE_MODIFIER",
                talent.level,
                cp,
                wp,
                loaded,
            )
        })
    {
        return v.max(0.05);
    }
    1.0
}

/// Helper computing barrier multiplier from an equipped talent.
pub(crate) fn talent_barrier_multiplier(
    talent: &ActiveTalentState,
    slot: AbilitySlot,
    cp: f32,
    wp: f32,
    loaded: Option<&BTreeMap<String, LoadedTalent>>,
) -> f32 {
    let Some(spec) = find_talent(&talent.id) else {
        return 1.0;
    };
    if !spec.scope.matches(slot) && spec.scope != TalentSlotScope::Perk {
        return 1.0;
    }
    eval_talent_mod(&talent.id, "BARRIER_MODIFIER", talent.level, cp, wp, loaded)
        .unwrap_or(1.0)
        .max(0.1)
}

/// Helper computing heal multiplier from an equipped talent.
pub(crate) fn talent_heal_multiplier(
    talent: &ActiveTalentState,
    slot: AbilitySlot,
    cp: f32,
    wp: f32,
    loaded: Option<&BTreeMap<String, LoadedTalent>>,
) -> f32 {
    let Some(spec) = find_talent(&talent.id) else {
        return 1.0;
    };
    if !spec.scope.matches(slot) && spec.scope != TalentSlotScope::Perk {
        return 1.0;
    }
    eval_talent_mod(&talent.id, "HEAL_MODIFIER", talent.level, cp, wp, loaded)
        .or_else(|| eval_talent_mod(&talent.id, "HEAL_MULTIPLIER", talent.level, cp, wp, loaded))
        .or_else(|| eval_talent_mod(&talent.id, "HEALING_MODIFIER", talent.level, cp, wp, loaded))
        .unwrap_or(1.0)
        .max(0.1)
}

/// Helper computing bonus range added to `slot`.
pub(crate) fn talent_bonus_range(
    talent: &ActiveTalentState,
    slot: AbilitySlot,
    cp: f32,
    wp: f32,
    loaded: Option<&BTreeMap<String, LoadedTalent>>,
) -> f32 {
    let Some(spec) = find_talent(&talent.id) else {
        return 0.0;
    };
    if !spec.scope.matches(slot) {
        return 0.0;
    }
    if let Some(less) = eval_talent_mod(&talent.id, "LESS_RANGE", talent.level, cp, wp, loaded) {
        return -less;
    }
    eval_talent_mod(&talent.id, "BONUS_RANGE", talent.level, cp, wp, loaded)
        .or_else(|| eval_talent_mod(&talent.id, "BONUS_SHOT_RANGE", talent.level, cp, wp, loaded))
        .or_else(|| eval_talent_mod(&talent.id, "RANGE", talent.level, cp, wp, loaded))
        .unwrap_or(0.0)
}

/// Helper computing additional charges added to `slot`.
pub(crate) fn talent_bonus_charges(
    talent: &ActiveTalentState,
    slot: AbilitySlot,
    cp: f32,
    wp: f32,
    loaded: Option<&BTreeMap<String, LoadedTalent>>,
) -> u8 {
    let Some(spec) = find_talent(&talent.id) else {
        return 0;
    };
    if !spec.scope.matches(slot) {
        return 0;
    }
    let val = eval_talent_mod(&talent.id, "CHARGES", talent.level, cp, wp, loaded)
        .or_else(|| {
            eval_talent_mod(
                &talent.id,
                "ADDITIONAL_CHARGES",
                talent.level,
                cp,
                wp,
                loaded,
            )
        })
        .unwrap_or(0.0);
    val.max(0.0).round() as u8
}

/// Compute aggregate passive stat adjustments from an equipped talent.
pub(crate) fn talent_passive_stats(
    talent: &ActiveTalentState,
    cp: f32,
    wp: f32,
    loaded: Option<&BTreeMap<String, LoadedTalent>>,
) -> TalentStatBonus {
    let mut b = TalentStatBonus::default();
    let Some(spec) = find_talent(&talent.id) else {
        return b;
    };
    let lvl = f32::from(talent.level.get().saturating_sub(1));
    match spec.special {
        TalentSpecial::BonusAttackRange => b.attack_range += 0.5 + 0.05 * lvl,
        TalentSpecial::BonusAttackSpeed => b.attack_speed += 0.1 + 0.02 * lvl,
        TalentSpecial::BonusAttackSpeedCooldown => {
            b.attack_speed += 0.075 + 0.015 * lvl;
            b.cooldown_accel += 0.075 + 0.015 * lvl;
        }
        TalentSpecial::BonusCooldown => b.cooldown_accel += 0.1 + 0.02 * lvl,
        TalentSpecial::BonusCritChance => b.crit_chance += 0.1 + 0.02 * lvl,
        TalentSpecial::BonusCritChanceDamage => {
            b.crit_chance += 0.075 + 0.015 * lvl;
            b.crit_damage += 0.1 + 0.02 * lvl;
        }
        TalentSpecial::BonusCritDamage => b.crit_damage += 0.15 + 0.03 * lvl,
        TalentSpecial::BonusCrystal => b.crystal_flat += 15.0 + 3.0 * lvl,
        TalentSpecial::BonusCrystalPierce => b.shield_pierce += 0.08 + 0.02 * lvl,
        TalentSpecial::BonusCrystalVamp => b.lifesteal += 0.08 + 0.015 * lvl,
        TalentSpecial::BonusDamageAttackSpeed => {
            b.weapon_pct += 0.05 + 0.01 * lvl;
            b.crystal_pct += 0.05 + 0.01 * lvl;
            b.attack_speed += 0.05 + 0.01 * lvl;
        }
        TalentSpecial::BonusDefense => {
            b.armor_flat += 10.0 + 3.0 * lvl;
            b.shield_flat += 10.0 + 3.0 * lvl;
        }
        TalentSpecial::BonusMovementSpeed => b.move_speed_flat += 0.25 + 0.05 * lvl,
        TalentSpecial::BonusRegen => b.hp_regen_per_sec += 5.0 + 1.5 * lvl,
        TalentSpecial::BonusWeapon => b.weapon_flat += 10.0 + 2.5 * lvl,
        TalentSpecial::BonusWeaponPierce => b.armor_pierce += 0.08 + 0.02 * lvl,
        TalentSpecial::BonusWeaponVamp => b.lifesteal += 0.08 + 0.015 * lvl,
        _ => {}
    }

    for m in spec.mods {
        let v = eval_talent_mod(&talent.id, m.label, talent.level, cp, wp, loaded)
            .unwrap_or_else(|| m.value_at(talent.level, cp, wp));
        match m.label {
            "PASSIVE_WEAPON_POWER" | "PASSIVE_WEAPON" => b.weapon_pct += v,
            "PASSIVE_CRYSTAL_POWER" | "PASSIVE_CRYSTAL" | "BONUS_CRYSTAL_POWER" => {
                b.crystal_pct += v;
            }
            "PASSIVE_ATTACK_SPEED" | "ATTACK_SPEED_BONUS" => b.attack_speed += v,
            "PASSIVE_ARMOR" => b.armor_pct += v,
            "PASSIVE_SHIELD" => b.shield_pct += v,
            "PASSIVE_MOVEMENT" => b.move_speed_pct += v,
            "PASSIVE_BONUS_ATTACK_RANGE" => b.attack_range += v,
            "PASSIVE_CRITICAL_DAMAGE" => b.crit_damage += v,
            "BONUS_ALT_ENERGY" => b.max_resource_bonus += v,
            _ => {}
        }
    }
    b
}

const fn mod_s(label: &'static str, base: f32, per_level: f32) -> TalentModSpec {
    TalentModSpec::new(label, base, per_level, 0.0, 0.0, 0.0)
}

const fn mod_c(
    label: &'static str,
    base: f32,
    per_level: f32,
    cp_ratio: f32,
    clamp: f32,
) -> TalentModSpec {
    TalentModSpec::new(label, base, per_level, cp_ratio, 0.0, clamp)
}

/// Complete static table of all **189** talents matching `talents_full.json`.
pub(crate) static ALL_TALENTS: &[TalentSpec] = &[
    // 0..3 Adagio
    TalentSpec {
        id: "Talent_Adagio_AvengingWrath",
        hero_id: Some("Adagio"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("CHARGES", 1.0, 0.0),
            mod_s("COOLDOWN_OVERRIDE", 1.0, 0.0),
            mod_s("BONUS_BURN_DAMAGE", 1.65, 0.15),
            mod_s("CHARGE_TIME", 0.95, -0.05),
        ],
        special: TalentSpecial::None,
    },
    TalentSpec {
        id: "Talent_Adagio_GiftOfLife",
        hero_id: Some("Adagio"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("HEAL_TO_FORTIFIED_RATIO", 0.8, 0.1),
            mod_s("COOLDOWN_MODIFIER", 0.9, -0.01),
        ],
        special: TalentSpecial::AdagioGiftFortified,
    },
    TalentSpec {
        id: "Talent_Adagio_HealingVerse",
        hero_id: Some("Adagio"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 0.6, 0.1),
            mod_s("COOLDOWN_MODIFIER", 0.8, -0.1),
            mod_s("HEAL_MAX_HEALTH_RATIO", 0.65, 0.15),
        ],
        special: TalentSpecial::AdagioHealingVerseAllies,
    },
    // 3..6 Alpha
    TalentSpec {
        id: "Talent_Alpha_Acceleration",
        hero_id: Some("Alpha"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("COOLDOWN_MODIFIER", 0.8, -0.075),
            mod_s("BONUS_MOVESPEED_STRENGTH", 2.0, 1.0),
            mod_s("BARRIER_MODIFIER", 1.25, 0.075),
        ],
        special: TalentSpecial::AlphaAccelerationMoveSpeed,
    },
    TalentSpec {
        id: "Talent_Alpha_CoreOverclock",
        hero_id: Some("Alpha"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("ATTACK_SPEED_PER_STACK", 0.15, 0.0),
            mod_s("PASSIVE_WEAPON_POWER", 0.05, 0.01),
        ],
        special: TalentSpecial::None,
    },
    TalentSpec {
        id: "Talent_Alpha_PrimeTime",
        hero_id: Some("Alpha"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("HEALTH_FORT_PER_STACK", 0.15, 0.05),
            mod_s("FORTIFIED_HEALTH_DURATION", 3.0, 0.0),
            mod_s("COOLDOWN_MODIFIER", 1.0, -0.03),
        ],
        special: TalentSpecial::AlphaPrimeTimeFortified,
    },
    // 6..9 Amael
    TalentSpec {
        id: "Talent_Amael_TalentA",
        hero_id: Some("Amael"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("CHARGE_TIME", 0.85, -0.02),
            mod_s("DAMAGE_MODIFIER", 1.1, 0.03),
        ],
        special: TalentSpecial::None,
    },
    TalentSpec {
        id: "Talent_Amael_TalentB",
        hero_id: Some("Amael"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("PASSIVE_ARMOR", 0.12, 0.03),
            mod_s("DISTANCE", 2.0, 0.5),
        ],
        special: TalentSpecial::None,
    },
    TalentSpec {
        id: "Talent_Amael_TalentC",
        hero_id: Some("Amael"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("PASSIVE_WEAPON", 0.1, 0.05),
            mod_s("COOLDOWN_MODIFIER", -10.0, -2.0),
        ],
        special: TalentSpecial::None,
    },
    // 9..12 Anka
    TalentSpec {
        id: "Talent_Anka_TalentA",
        hero_id: Some("Anka"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 1.0, 0.05),
            mod_s("BONUS_MOVESPEED_STRENGTH", 1.5, 0.1),
            mod_s("BONUS_MOVESPEED_DURATION", 3.0, 0.0),
        ],
        special: TalentSpecial::None,
    },
    TalentSpec {
        id: "Talent_Anka_TalentB",
        hero_id: Some("Anka"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::Perk,
        mods: &[mod_s("PASSIVE_CRYSTAL_POWER", 0.02, 0.02)],
        special: TalentSpecial::AnkaRefreshAllOnKill,
    },
    TalentSpec {
        id: "Talent_Anka_TalentC",
        hero_id: Some("Anka"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("BONUS_RANGE", 50.0, 0.0),
            mod_s("COOLDOWN_MODIFIER", 0.6, -0.05),
            mod_s("DAMAGE_MODIFIER", 1.0, 0.1),
        ],
        special: TalentSpecial::None,
    },
    // 12..15 Ardan
    TalentSpec {
        id: "Talent_Ardan_HeavyHitter",
        hero_id: Some("Ardan"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 1.3, 0.15),
            mod_s("COOLDOWN_MODIFIER", 1.5, -0.03),
            mod_s("LESS_RANGE", 2.0, 0.0),
        ],
        special: TalentSpecial::None,
    },
    TalentSpec {
        id: "Talent_Ardan_KnockoutPunch",
        hero_id: Some("Ardan"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("DISTANCE", 10.0, 0.0),
            mod_s("DAMAGE_MODIFIER", 1.1, 0.2),
            mod_s("COOLDOWN_MODIFIER", 1.0, -0.1),
        ],
        special: TalentSpecial::ArdanKnockoutPush,
    },
    TalentSpec {
        id: "Talent_Ardan_Vanguardian",
        hero_id: Some("Ardan"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("BARRIER_MODIFIER", 1.2, 0.05),
            mod_s("DAMAGE_MODIFIER", 0.2, 0.01),
            mod_s("COOLDOWN_MODIFIER", 0.95, -0.01),
        ],
        special: TalentSpecial::ArdanFullSelfVanguard,
    },
    // 15..18 Baptiste
    TalentSpec {
        id: "Talent_Baptiste_ArmyOfSouls",
        hero_id: Some("Baptiste"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("COOLDOWN_MODIFIER", 0.7, -0.075),
            mod_s("RANGE", 15.0, 0.0),
            mod_s("DAMAGE_MODIFER", 1.2, 0.15),
        ],
        special: TalentSpecial::BaptisteArmyDualWave,
    },
    TalentSpec {
        id: "Talent_Baptiste_BadBadMojo",
        hero_id: Some("Baptiste"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("BONUS_RANGE", 2.0, 0.0),
            mod_s("COOLDOWN_MODIFIER", 0.8, -0.015),
            mod_s("DAMAGE_MODIFIER", 0.9, 0.025),
        ],
        special: TalentSpecial::None,
    },
    TalentSpec {
        id: "Talent_Baptiste_TheReaper",
        hero_id: Some("Baptiste"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::Perk,
        mods: &[
            mod_s("PASSIVE_ATTACK_SPEED", 0.3, 0.03),
            mod_s("HEAL_MODIFIER", 0.45, 0.04),
        ],
        special: TalentSpecial::BaptisteReaperOverflowSouls,
    },
    // 18..21 Baron
    TalentSpec {
        id: "Talent_Baron_EndlessMortars",
        hero_id: Some("Baron"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 0.5, 0.075),
            mod_s("CHARGES", 6.0, 0.0),
            mod_s("ENERGY_MODIFIER", 0.2, 0.0),
            mod_s("SPEED", 0.25, 0.0),
        ],
        special: TalentSpecial::None,
    },
    TalentSpec {
        id: "Talent_Baron_TrionCannon",
        hero_id: Some("Baron"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("CHARGES", 2.0, 0.0),
            mod_s("COOLDOWN_OVERRIDE", 1.5, 0.0),
            mod_s("CHARGE_TIME", 0.6, -0.075),
            mod_s("DAMAGE_MODIFIER", 0.8, 0.1),
            mod_s("PASSIVE_BONUS_ATTACK_RANGE", 0.5, 0.0),
        ],
        special: TalentSpecial::None,
    },
    TalentSpec {
        id: "Talent_Baron_TurboJets",
        hero_id: Some("Baron"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("COOLDOWN_MODIFIER", 1.1, -0.02),
            mod_s("TRIPLE_SHOT_DAMAGE", 0.7, 0.02),
        ],
        special: TalentSpecial::BaronTurboJetsTripleShot,
    },
    // 21..24 Blackfeather
    TalentSpec {
        id: "Talent_Blackfeather_Elusiveness",
        hero_id: Some("Blackfeather"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 1.0, 0.25),
            mod_s("CHARGES", 2.0, 0.0),
            mod_s("FOCUS_MODIFIER", 0.05, 0.0),
            mod_s("CHARGE_TIME", 0.5, -0.075),
        ],
        special: TalentSpecial::None,
    },
    TalentSpec {
        id: "Talent_Blackfeather_HummingbirdHeart",
        hero_id: Some("Blackfeather"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::Perk,
        mods: &[
            mod_s("PASSIVE_ATTACK_SPEED", 0.2, 0.02),
            mod_s("PASSIVE_WEAPON_POWER", 0.05, 0.01),
            mod_s("BONUS_DURATION", 4.0, 0.0),
        ],
        special: TalentSpecial::None,
    },
    TalentSpec {
        id: "Talent_Blackfeather_PointGuard",
        hero_id: Some("Blackfeather"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("BARRIER_MODIFIER", 1.5, 0.1),
            mod_s("BARRIER_DURATION", 2.0, 0.0),
            mod_s("RADIUS", 9.0, 0.0),
        ],
        special: TalentSpecial::BlackfeatherPointGuardAllies,
    },
    // 24..41 Generic 17 Bonus talents
    TalentSpec {
        id: "Talent_BonusAttackRange",
        hero_id: None,
        rarity: TalentRarity::Generic,
        scope: TalentSlotScope::None,
        mods: &[],
        special: TalentSpecial::BonusAttackRange,
    },
    TalentSpec {
        id: "Talent_BonusAttackSpeed",
        hero_id: None,
        rarity: TalentRarity::Generic,
        scope: TalentSlotScope::None,
        mods: &[],
        special: TalentSpecial::BonusAttackSpeed,
    },
    TalentSpec {
        id: "Talent_BonusAttackSpeedCooldown",
        hero_id: None,
        rarity: TalentRarity::Generic,
        scope: TalentSlotScope::None,
        mods: &[],
        special: TalentSpecial::BonusAttackSpeedCooldown,
    },
    TalentSpec {
        id: "Talent_BonusCooldown",
        hero_id: None,
        rarity: TalentRarity::Generic,
        scope: TalentSlotScope::None,
        mods: &[],
        special: TalentSpecial::BonusCooldown,
    },
    TalentSpec {
        id: "Talent_BonusCritChance",
        hero_id: None,
        rarity: TalentRarity::Generic,
        scope: TalentSlotScope::None,
        mods: &[],
        special: TalentSpecial::BonusCritChance,
    },
    TalentSpec {
        id: "Talent_BonusCritChanceDamage",
        hero_id: None,
        rarity: TalentRarity::Generic,
        scope: TalentSlotScope::None,
        mods: &[],
        special: TalentSpecial::BonusCritChanceDamage,
    },
    TalentSpec {
        id: "Talent_BonusCritDamage",
        hero_id: None,
        rarity: TalentRarity::Generic,
        scope: TalentSlotScope::None,
        mods: &[],
        special: TalentSpecial::BonusCritDamage,
    },
    TalentSpec {
        id: "Talent_BonusCrystal",
        hero_id: None,
        rarity: TalentRarity::Generic,
        scope: TalentSlotScope::None,
        mods: &[],
        special: TalentSpecial::BonusCrystal,
    },
    TalentSpec {
        id: "Talent_BonusCrystalPierce",
        hero_id: None,
        rarity: TalentRarity::Generic,
        scope: TalentSlotScope::None,
        mods: &[],
        special: TalentSpecial::BonusCrystalPierce,
    },
    TalentSpec {
        id: "Talent_BonusCrystalVamp",
        hero_id: None,
        rarity: TalentRarity::Generic,
        scope: TalentSlotScope::None,
        mods: &[],
        special: TalentSpecial::BonusCrystalVamp,
    },
    TalentSpec {
        id: "Talent_BonusDamageAttackSpeed",
        hero_id: None,
        rarity: TalentRarity::Generic,
        scope: TalentSlotScope::None,
        mods: &[],
        special: TalentSpecial::BonusDamageAttackSpeed,
    },
    TalentSpec {
        id: "Talent_BonusDefense",
        hero_id: None,
        rarity: TalentRarity::Generic,
        scope: TalentSlotScope::None,
        mods: &[],
        special: TalentSpecial::BonusDefense,
    },
    TalentSpec {
        id: "Talent_BonusMovementSpeed",
        hero_id: None,
        rarity: TalentRarity::Generic,
        scope: TalentSlotScope::None,
        mods: &[],
        special: TalentSpecial::BonusMovementSpeed,
    },
    TalentSpec {
        id: "Talent_BonusRegen",
        hero_id: None,
        rarity: TalentRarity::Generic,
        scope: TalentSlotScope::None,
        mods: &[],
        special: TalentSpecial::BonusRegen,
    },
    TalentSpec {
        id: "Talent_BonusWeapon",
        hero_id: None,
        rarity: TalentRarity::Generic,
        scope: TalentSlotScope::None,
        mods: &[],
        special: TalentSpecial::BonusWeapon,
    },
    TalentSpec {
        id: "Talent_BonusWeaponPierce",
        hero_id: None,
        rarity: TalentRarity::Generic,
        scope: TalentSlotScope::None,
        mods: &[],
        special: TalentSpecial::BonusWeaponPierce,
    },
    TalentSpec {
        id: "Talent_BonusWeaponVamp",
        hero_id: None,
        rarity: TalentRarity::Generic,
        scope: TalentSlotScope::None,
        mods: &[],
        special: TalentSpecial::BonusWeaponVamp,
    },
    // 41..44 Caine
    TalentSpec {
        id: "Talent_Caine_TalentA",
        hero_id: Some("Caine"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotB,
        mods: &[mod_s("COOLDOWN_MODIFIER", 0.8, -0.02)],
        special: TalentSpecial::CaineOnceMoreInvuln,
    },
    TalentSpec {
        id: "Talent_Caine_TalentB",
        hero_id: Some("Caine"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotC,
        mods: &[mod_s("EXECUTE_MODIFIER", 0.02, 0.02)],
        special: TalentSpecial::CaineHigherExecuteNoRefresh,
    },
    TalentSpec {
        id: "Talent_Caine_TalentC",
        hero_id: Some("Caine"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("COOLDOWN_MODIFIER", 0.8, -0.15),
            mod_s("BONUS_RANGE", 1.0, 0.5),
        ],
        special: TalentSpecial::CaineDoubleBullets,
    },
    // 44..47 Catherine
    TalentSpec {
        id: "Talent_Catherine_AdvancingGuard",
        hero_id: Some("Catherine"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::Perk,
        mods: &[
            mod_s("DURATION", 30.0, 0.0),
            mod_s("WEAPON_POWER_PER_STACK", 0.01, 0.01),
            mod_s("CRYSTAL_POWER_PER_STACK", 0.01, 0.01),
        ],
        special: TalentSpecial::CatherineAdvancingGuardPower,
    },
    TalentSpec {
        id: "Talent_Catherine_QuickPursuit",
        hero_id: Some("Catherine"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("COOLDOWN_REDUCTION_A", 0.1, 0.015),
            mod_s("COOLDOWN_REDUCTION_C", 0.05, 0.01),
        ],
        special: TalentSpecial::CatherineQuickPursuitCdr,
    },
    TalentSpec {
        id: "Talent_Catherine_Shockwave",
        hero_id: Some("Catherine"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("BOUNCE_DISTANCE", 6.0, 0.0),
            mod_s("BOUNCE_TIME", 0.5, 0.0),
            mod_s("COOLDOWN_MODIFIER", 0.7, -0.075),
            mod_s("DAMAGE_MODIFIER", 1.15, 0.15),
        ],
        special: TalentSpecial::CatherineShockwaveKnockbackStun,
    },
    // 47..50 Celeste
    TalentSpec {
        id: "Talent_Celeste_CoreLeafClover",
        hero_id: Some("Celeste"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 0.25, 0.025),
            mod_s("COOLDOWN_MODIFIER", 1.25, -0.04),
        ],
        special: TalentSpecial::CelesteFourCollapses,
    },
    TalentSpec {
        id: "Talent_Celeste_GalaxyCollapse",
        hero_id: Some("Celeste"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("IMPACT_DAMAGE_MODIFIER", 0.7, 0.05),
            mod_s("NOVA_DAMAGE_MODIFIER", 1.0, 0.1),
            mod_s("BONUS_DURATION", 6.0, 0.0),
        ],
        special: TalentSpecial::CelesteChainSupernova,
    },
    TalentSpec {
        id: "Talent_Celeste_YoungStar",
        hero_id: Some("Celeste"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("IMPACT_DAMAGE", 0.6, 0.01),
            mod_s("DAMAGE_PER_SECOND", 0.7, 0.05),
            mod_s("TICKS_PER_SECOND", 2.0, 0.0),
            mod_s("DAMAGE_RADIUS", 1.5, 0.0),
        ],
        special: TalentSpecial::CelesteYoungStarDoT,
    },
    // 50..53 Churnwalker
    TalentSpec {
        id: "Talent_Churnwalker_TalentA",
        hero_id: Some("Churnwalker"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::Perk,
        mods: &[
            mod_s("HEAL_MULTIPLIER", 2.1, 0.1),
            mod_s("DECREASE_DAMAGE_SHARE", 0.5, 0.01),
        ],
        special: TalentSpecial::ChurnwalkerHealShareTrade,
    },
    TalentSpec {
        id: "Talent_Churnwalker_TalentB",
        hero_id: Some("Churnwalker"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("CLOUD_DURATION", 3.0, 0.25),
            mod_s("CLOUD_DAMAGE_TICKS_PER_SECOND", 2.0, 0.0),
            mod_c("CLOUD_DAMAGE_PER_SECOND", 50.0, 40.0, 1.0, 0.0),
            mod_s("CLOUD_RADIUS", 4.0, 0.0),
            mod_s("B_INCREASED_COOLDOWN", 3.0, -0.2),
        ],
        special: TalentSpecial::ChurnwalkerTormentCloud,
    },
    TalentSpec {
        id: "Talent_Churnwalker_TalentC",
        hero_id: Some("Churnwalker"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("BONUS_SHOT_RANGE", 20.0, 5.0),
            mod_s("BONUS_TETHER_RANGE", 2.0, 0.2),
            mod_s("PULL_TO_RANGE", 2.0, 0.0),
            mod_s("PULL_TO_SPEED", 45.0, 0.0),
        ],
        special: TalentSpecial::ChurnwalkerGrapplePull,
    },
    // 53..56 Flicker
    TalentSpec {
        id: "Talent_Flicker_BindingDust",
        hero_id: Some("Flicker"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 0.8, 0.05),
            mod_s("BONUS_MOVESPEED_STRENGTH", 1.5, 0.15),
            mod_s("ROOT_DURATION", 1.2, 0.0),
        ],
        special: TalentSpecial::FlickerDustRootAndSpeed,
    },
    TalentSpec {
        id: "Talent_Flicker_FairyHealing",
        hero_id: Some("Flicker"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::Perk,
        mods: &[
            mod_s("MAX_HEALTH_TO_HEALING", 0.075, 0.025),
            mod_s("PASSIVE_CRYSTAL_POWER", 0.15, 0.05),
        ],
        special: TalentSpecial::FlickerStealthRegen,
    },
    TalentSpec {
        id: "Talent_Flicker_SwarmingLight",
        hero_id: Some("Flicker"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 0.9, 0.025),
            mod_s("CHARGES", 1.0, 0.0),
            mod_c("CHARGE_TIME", 0.9, -0.02, 0.0, 0.01),
            mod_s("COOLDOWN_OVERRIDE", 1.0, 0.0),
        ],
        special: TalentSpecial::None,
    },
    // 56..59 Fortress
    TalentSpec {
        id: "Talent_Fortress_CriticalWounds",
        hero_id: Some("Fortress"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("CRITICAL_CHANCE_PER_WOLF", 0.15, 0.0),
            mod_s("PASSIVE_CRITICAL_DAMAGE", 0.25, 0.025),
            mod_s("COOLDOWN_MODIFIER", 1.25, -0.05),
        ],
        special: TalentSpecial::FortressWolvesCritChance,
    },
    TalentSpec {
        id: "Talent_Fortress_FlankAttack",
        hero_id: Some("Fortress"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotAB,
        mods: &[
            mod_s("BLEED_DAMAGE", 0.6, 0.1),
            mod_s("COOLDOWN_MODIFIER", 1.2, -0.1),
        ],
        special: TalentSpecial::FortressSpawnWolvesOnAb,
    },
    TalentSpec {
        id: "Talent_Fortress_Longtooth",
        hero_id: Some("Fortress"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("BONUS_RANGE", 1.6, 0.1),
            mod_s("PASSIVE_WEAPON_POWER", 0.005, 0.005),
            mod_s("PASSIVE_ATTACK_SPEED", 0.005, 0.005),
        ],
        special: TalentSpecial::None,
    },
    // 59..62 Glaive
    TalentSpec {
        id: "Talent_Glaive_StunningBloodsong",
        hero_id: Some("Glaive"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("COOLDOWN_MODIFIER", 0.8, -0.05),
            mod_s("STUN_DURATION", 0.75, 0.0),
            mod_s("DAMAGE_MODIFIER", 0.4, 0.05),
        ],
        special: TalentSpecial::GlaiveBloodsongStun,
    },
    TalentSpec {
        id: "Talent_Glaive_TwistedPursuit",
        hero_id: Some("Glaive"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("COOLDOWN_MODIFIER", 0.7, -0.075),
            mod_s("BONUS_RANGE", 5.0, 0.0),
            mod_s("DASH_SPEED", 14.0, 0.0),
            mod_s("DAMAGE_MODIFIER", 0.9, 0.075),
        ],
        special: TalentSpecial::GlaiveTwistedDash,
    },
    TalentSpec {
        id: "Talent_Glaive_ViolentAfterburn",
        hero_id: Some("Glaive"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("STUN_DURATION", 1.0, 0.0),
            mod_s("DAMAGE_MODIFIER", 0.7, 0.02),
            mod_s("COOLDOWN_MODIFIER", 0.95, -0.02),
        ],
        special: TalentSpecial::GlaiveAfterburnPuntStun,
    },
    // 62..65 Grace
    TalentSpec {
        id: "Talent_Grace_DoubleBenediction",
        hero_id: Some("Grace"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("DAMAGE_REDUCTION_MODIFIER", 0.5, 0.03),
            mod_s("CHARGES", 1.0, 0.0),
            mod_s("CHARGE_TIME", 0.8, -0.035),
            mod_s("COOLDOWN_OVERRIDE", 1.0, 0.0),
            mod_s("CHARGES_FROM_OVERDRIVE", 1.0, 0.0),
        ],
        special: TalentSpecial::None,
    },
    TalentSpec {
        id: "Talent_Grace_SealOfRetribution",
        hero_id: Some("Grace"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::Perk,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 1.0, 0.04),
            mod_s("PASSIVE_CRYSTAL_POWER", 0.15, 0.025),
        ],
        special: TalentSpecial::None,
    },
    TalentSpec {
        id: "Talent_Grace_UnitedFront",
        hero_id: Some("Grace"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("HEAL_MODIFIER", 0.75, 0.15),
            mod_s("COOLDOWN_MODIFIER", 0.8, -0.075),
        ],
        special: TalentSpecial::GraceUnitedFrontSelfHeal,
    },
    // 65..68 Grumpjaw
    TalentSpec {
        id: "Talent_Grumpjaw_BigAppetite",
        hero_id: Some("Grumpjaw"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("COOLDOWN_MODIFIER", 0.4, -0.05),
            mod_s("BONUS_DURATION", 1.0, 0.4),
        ],
        special: TalentSpecial::GrumpjawEatMultiple,
    },
    TalentSpec {
        id: "Talent_Grumpjaw_Rampage",
        hero_id: Some("Grumpjaw"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("SWIPE_TO_CHARGE_DAMAGE", 0.9, 0.025),
            mod_s("RADIUS", 1.2, 0.0),
        ],
        special: TalentSpecial::GrumpjawGrumpyPathDamage,
    },
    TalentSpec {
        id: "Talent_Grumpjaw_Reconstitution",
        hero_id: Some("Grumpjaw"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("COOLDOWN_MODIFIER", 0.95, -0.025),
            mod_s("BONUS_LIFESTEAL", 0.25, 0.04),
        ],
        special: TalentSpecial::GrumpjawHangryLifesteal,
    },
    // 68..71 Gwen
    TalentSpec {
        id: "Talent_Gwen_Skedaddling",
        hero_id: Some("Gwen"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("BONUS_DURATION", 1.0, 0.2),
            mod_s("COOLDOWN_MODIFIER", 0.6, -0.025),
            mod_s("PASSIVE_WEAPON_POWER", 0.1, 0.01),
        ],
        special: TalentSpecial::None,
    },
    TalentSpec {
        id: "Talent_Gwen_SpeedLoader",
        hero_id: Some("Gwen"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 0.9, 0.025),
            mod_s("ENERGY_MODIFIER", 0.5, 0.0),
        ],
        special: TalentSpecial::ModeledSpeedLoader,
    },
    TalentSpec {
        id: "Talent_Gwen_ThreeOfAKind",
        hero_id: Some("Gwen"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 0.6, 0.1),
            mod_s("COOLDOWN_MODIFIER", 0.6, -0.05),
        ],
        special: TalentSpecial::GwenThreeCards,
    },
    // 71..74 Idris
    TalentSpec {
        id: "Talent_Idris_ChaChaChakram",
        hero_id: Some("Idris"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 0.6, 0.05),
            mod_s("COOLDOWN_MODIFIER", 1.1, -0.025),
            mod_s("ANGLE_MODIFIER", 25.0, 0.0),
        ],
        special: TalentSpecial::IdrisTripleChakram,
    },
    TalentSpec {
        id: "Talent_Idris_PathOfTheSeeker",
        hero_id: Some("Idris"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::Perk,
        mods: &[
            mod_s("PASSIVE_WEAPON_POWER", -0.1, 0.01),
            mod_s("PASSIVE_CRYSTAL_POWER", -0.1, 0.01),
        ],
        special: TalentSpecial::IdrisBothPaths,
    },
    TalentSpec {
        id: "Talent_Idris_ShimmerHeal",
        hero_id: Some("Idris"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("HEAL_PERCENT_MAX_HEALTH", 0.4, 0.075),
            mod_s("COOLDOWN_MODIFIER", 0.6, -0.075),
        ],
        special: TalentSpecial::IdrisShimmerHeal,
    },
    // 74..77 Inara
    TalentSpec {
        id: "Talent_Inara_TalentA",
        hero_id: Some("Inara"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("HEAL_AMOUNT", 125.0, 25.0),
            mod_s("DAMAGE_MODIFIER", 0.7, 0.02),
        ],
        special: TalentSpecial::InaraDanceHealAllies,
    },
    TalentSpec {
        id: "Talent_Inara_TalentB",
        hero_id: Some("Inara"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("BONUS_DISTANCE", 3.0, 1.0),
            mod_s("COOLDOWN_MODIFIER", 0.9, -0.03),
        ],
        special: TalentSpecial::None,
    },
    TalentSpec {
        id: "Talent_Inara_TalentC",
        hero_id: Some("Inara"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::AllAbilities,
        mods: &[mod_s("INVULN_DURATION", 1.0, 0.5)],
        special: TalentSpecial::InaraInvulnOnDash,
    },
    // 77..80 Ishtar
    TalentSpec {
        id: "Talent_Ishtar_TalentA",
        hero_id: Some("Ishtar"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::Perk,
        mods: &[
            mod_s("ARMOR_PER_STACK", 10.0, 1.0),
            mod_s("SHIELD_PER_STACK", 10.0, 1.0),
            mod_s("LIFESTEAL_PER_STACK", 0.05, 0.0),
            mod_s("MAX_STACKS", 10.0, 0.0),
        ],
        special: TalentSpecial::IshtarKillStackDefenses,
    },
    TalentSpec {
        id: "Talent_Ishtar_TalentB",
        hero_id: Some("Ishtar"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 1.1, 0.1),
            mod_s("ROOT_DURATION", 1.0, 0.05),
        ],
        special: TalentSpecial::IshtarWhirlRoot,
    },
    TalentSpec {
        id: "Talent_Ishtar_TalentC",
        hero_id: Some("Ishtar"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::Perk,
        mods: &[
            mod_s("ATK_SPEED_PER_STACK", 0.06, 0.01),
            mod_s("MAX_STACKS", 20.0, 0.0),
        ],
        special: TalentSpecial::IshtarBladeAttackSpeedStacks,
    },
    // 80..83 Joule
    TalentSpec {
        id: "Talent_Joule_BackupButton",
        hero_id: Some("Joule"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("CHARGES", 1.0, 0.0),
            mod_s("COOLDOWN_OVERRIDE", 1.0, 0.0),
            mod_s("CHARGE_TIME", 0.8, -0.025),
            mod_s("DAMAGE_MODIFIER", 0.75, 0.05),
        ],
        special: TalentSpecial::None,
    },
    TalentSpec {
        id: "Talent_Joule_RocketLeapfrog",
        hero_id: Some("Joule"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("PASSIVE_ARMOR", 0.2, 0.025),
            mod_s("PASSIVE_SHIELD", 0.2, 0.025),
            mod_s("COOLDOWN_MODIFIER", 0.9, -0.02),
            mod_s("ENERGY_MODIFIER", 0.5, 0.0),
        ],
        special: TalentSpecial::JouleLeapResetNoStun,
    },
    TalentSpec {
        id: "Talent_Joule_ThunderingSteps",
        hero_id: Some("Joule"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("BONUS_MOVESPEED_STRENGTH", 1.5, 0.5),
            mod_s("BONUS_MOVESPEED_DURATION", 2.0, 0.0),
            mod_s("PASSIVE_WEAPON_POWER", 0.075, 0.025),
        ],
        special: TalentSpecial::JouleThunderSpeed,
    },
    // 83..86 Karas
    TalentSpec {
        id: "Talent_Karas_TalentA",
        hero_id: Some("Karas"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("HEAL_ON_ATTACK", 125.0, 50.0),
            mod_s("ATTACK_SPEED_BONUS", 0.1, 0.02),
        ],
        special: TalentSpecial::KarasTwisterHeal,
    },
    TalentSpec {
        id: "Talent_Karas_TalentB",
        hero_id: Some("Karas"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 0.7, 0.05),
            mod_s("PASSIVE_CRYSTAL", 0.02, 0.02),
        ],
        special: TalentSpecial::KarasDoubleGust,
    },
    TalentSpec {
        id: "Talent_Karas_TalentC",
        hero_id: Some("Karas"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotC,
        mods: &[mod_s("DAMAGE_MODIFIER", 0.8, 0.1)],
        special: TalentSpecial::KarasDevilsDustAoE,
    },
    // 86..89 Kensei
    TalentSpec {
        id: "Talent_Kensei_TalentA",
        hero_id: Some("Kensei"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::Perk,
        mods: &[
            mod_s("BONUS_ATTACKSPEED", 0.2, 0.05),
            mod_s("BARRIER_MODIFIER", 0.4, 0.02),
        ],
        special: TalentSpecial::KenseiMaxStacksOnHit,
    },
    TalentSpec {
        id: "Talent_Kensei_TalentB",
        hero_id: Some("Kensei"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("PASSIVE_WEAPON_POWER", 0.1, 0.01),
            mod_s("COOLDOWN_MODIFIER", 1.2, -0.03),
            mod_s("DURATION", 1.1, 0.1),
        ],
        special: TalentSpecial::KenseiEmpoweredInvuln,
    },
    TalentSpec {
        id: "Talent_Kensei_TalentC",
        hero_id: Some("Kensei"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 1.2, 0.25),
            mod_s("COOLDOWN_MODIFIER", 0.5, -0.05),
        ],
        special: TalentSpecial::KenseiResetOnUltHit,
    },
    // 89..92 Kestrel
    TalentSpec {
        id: "Talent_Kestrel_ActiveReload",
        hero_id: Some("Kestrel"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("COOLDOWN_MODIFIER", 1.0, -0.02),
            mod_s("ADDITIONAL_CHARGES", 2.0, 0.0),
            mod_s("DAMAGE_MODIFIER", 0.8, 0.07),
            mod_s("ENERGY_MODIFIER", 0.5, 0.0),
        ],
        special: TalentSpecial::KestrelCamoRefillsGlimmershot,
    },
    TalentSpec {
        id: "Talent_Kestrel_GlimmerVolley",
        hero_id: Some("Kestrel"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 0.775, 0.025),
            mod_s("RANGE", 8.0, 0.0),
        ],
        special: TalentSpecial::KestrelDoubleArrow,
    },
    TalentSpec {
        id: "Talent_Kestrel_PiercingShot",
        hero_id: Some("Kestrel"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 1.2, 0.2),
            mod_s("COOLDOWN_MODIFIER", 1.0, -0.1),
            mod_s("TURRET_DAMAGE", 0.2, 0.0),
        ],
        special: TalentSpecial::KestrelUltPierces,
    },
    // 92..95 Kinetic
    TalentSpec {
        id: "Talent_Kinetic_TalentA",
        hero_id: Some("Kinetic"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::Perk,
        mods: &[
            mod_s("BONUS_ATTACKSPEED", 0.3, 0.02),
            mod_s("DAMAGE_MODIFIER", 1.04, 0.04),
        ],
        special: TalentSpecial::None,
    },
    TalentSpec {
        id: "Talent_Kinetic_TalentB",
        hero_id: Some("Kinetic"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 0.6, 0.05),
            mod_s("RANGE", -2.0, 0.0),
        ],
        special: TalentSpecial::KineticTripleBurstA,
    },
    TalentSpec {
        id: "Talent_Kinetic_TalentC",
        hero_id: Some("Kinetic"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 0.3, 0.2),
            mod_s("COOLDOWN_MODIFIER", 1.5, 0.0),
        ],
        special: TalentSpecial::KineticHitAReducesB,
    },
    // 95..98 Koshka
    TalentSpec {
        id: "Talent_Koshka_Frenzied",
        hero_id: Some("Koshka"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::AllAbilities,
        mods: &[
            mod_s("ABILITY_DAMAGE_MODIFIER", 0.75, 0.025),
            mod_s("COOLDOWN_REDUCTION_A", 0.1, 0.01),
            mod_s("COOLDOWN_REDUCTION_B", 0.15, 0.01),
            mod_s("COOLDOWN_REDUCTION_C", 0.05, 0.005),
        ],
        special: TalentSpecial::KoshkaAttacksReduceCdr,
    },
    TalentSpec {
        id: "Talent_Koshka_SlashyPlayTime",
        hero_id: Some("Koshka"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("RADIUS", 3.0, 0.0),
            mod_s("DAMAGE_MODIFIER", 0.35, 0.1),
            mod_s("COOLDOWN_MODIFIER", 0.55, -0.075),
        ],
        special: TalentSpecial::KoshkaUltAoEStun,
    },
    TalentSpec {
        id: "Talent_Koshka_TwirlyLife",
        hero_id: Some("Koshka"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("MAX_HEALTH_TO_BARRIER", 0.13, 0.03),
            mod_s("DAMAGE_MODIFIER", 0.65, 0.03),
            mod_s("BARRIER_DURATION", 2.0, 0.0),
        ],
        special: TalentSpecial::None,
    },
    // 98..101 Krul (Hero009)
    TalentSpec {
        id: "Talent_Krul_CursedBlade",
        hero_id: Some("Hero009"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 0.3, 0.1),
            mod_s("COOLDOWN_MODIFIER", 0.7, -0.05),
            mod_s("STACKS", 8.0, 0.0),
        ],
        special: TalentSpecial::KrulUltMaxWeakness,
    },
    TalentSpec {
        id: "Talent_Krul_DeadMansBarrier",
        hero_id: Some("Hero009"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("BARRIER_MODIFIER", 1.15, 0.05),
            mod_s("BONUS_RANGE", 0.5, 0.0),
        ],
        special: TalentSpecial::None,
    },
    TalentSpec {
        id: "Talent_Krul_HealingSmite",
        hero_id: Some("Hero009"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("HEALING_MODIFIER", 1.0, 0.05),
            mod_s("BONUS_CRYSTAL_POWER", 0.1, 0.025),
            mod_s("RADIUS", 9.0, 0.0),
        ],
        special: TalentSpecial::KrulSmiteHealsAllies,
    },
    // 101..104 Lance
    TalentSpec {
        id: "Talent_Lance_ReinforcedWalls",
        hero_id: Some("Lance"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("MAX_HEALTH_TO_BARRIER", 0.15, 0.025),
            mod_s("BARRIER_DURATION", 2.0, 0.0),
            mod_s("COOLDOWN_MODIFIER", 1.25, 0.0),
        ],
        special: TalentSpecial::LanceWallGrantsAllyBarrier,
    },
    TalentSpec {
        id: "Talent_Lance_Rollout",
        hero_id: Some("Lance"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("BASIC_ATTACK_TO_ROLL_DAMAGE", 1.2, 0.04),
            mod_s("RADIUS", 1.2, 0.0),
            mod_s("BONUS_RANGE", 2.0, 0.0),
            mod_s("COOLDOWN_MODIFIER", 0.8, -0.025),
        ],
        special: TalentSpecial::LanceRollDamagesPath,
    },
    TalentSpec {
        id: "Talent_Lance_Striker",
        hero_id: Some("Lance"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("IMPALE_DAMAGE_MODIFIER", 1.1, 0.1),
            mod_s("COOLDOWN_REDUCTION_A", 0.3, 0.1),
            mod_s("COOLDOWN_REDUCTION_C", 0.2, 0.1),
        ],
        special: TalentSpecial::LanceBasicReducesAC,
    },
    // 104..107 Leo
    TalentSpec {
        id: "Talent_Leo_TalentA",
        hero_id: Some("Leo"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::Perk,
        mods: &[
            mod_s("PASSIVE_WEAPON", 0.05, 0.01),
            mod_s("PASSIVE_CRYSTAL", 0.05, 0.01),
            mod_s("PASSIVE_MOVEMENT", 0.05, 0.05),
        ],
        special: TalentSpecial::LeoTurboMoveSpeed,
    },
    TalentSpec {
        id: "Talent_Leo_TalentB",
        hero_id: Some("Leo"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("PASSIVE_ARMOR", 0.1, 0.05),
            mod_s("PASSIVE_SHIELD", 0.1, 0.05),
        ],
        special: TalentSpecial::LeoCycloneTaunt,
    },
    TalentSpec {
        id: "Talent_Leo_TalentC",
        hero_id: Some("Leo"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("BUFF_DURATION", 6.0, 4.0),
            mod_s("COOLDOWN_MODIFIER", 0.9, -0.1),
        ],
        special: TalentSpecial::LeoUltEmpowersBoth,
    },
    // 107..110 Lorelai
    TalentSpec {
        id: "Talent_Lorelai_TalentA",
        hero_id: Some("Lorelai"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("ADDITIONAL_CHARGES", 1.0, 0.0),
            mod_s("COOLDOWN_OVERRIDE", 1.0, 0.0),
            mod_c("CHARGE_TIME", 0.88, -0.02, 0.0, 0.01),
            mod_s("ENERGY_MODIFIER", 0.25, 0.0),
        ],
        special: TalentSpecial::None,
    },
    TalentSpec {
        id: "Talent_Lorelai_TalentB",
        hero_id: Some("Lorelai"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotA,
        mods: &[mod_s("STUN_DURATION", 0.4, 0.15)],
        special: TalentSpecial::LorelaiStunExtended,
    },
    TalentSpec {
        id: "Talent_Lorelai_TalentC",
        hero_id: Some("Lorelai"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("INVULN_DURATION", 3.0, 1.0),
            mod_s("COOLDOWN_MODIFIER", 1.0, -0.05),
        ],
        special: TalentSpecial::LorelaiUltInvulnerability,
    },
    // 110..113 Lyra
    TalentSpec {
        id: "Talent_Lyra_GythianWard",
        hero_id: Some("Lyra"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("BARRIER_DURATION", 2.0, 0.0),
            mod_s("MAX_HEALTH_TO_BARRIER", 0.1, 0.05),
        ],
        special: TalentSpecial::LyraGythianWardCleanseBarrier,
    },
    TalentSpec {
        id: "Talent_Lyra_MobileBulwark",
        hero_id: Some("Lyra"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("BONUS_DURATION", 1.4, 0.4),
            mod_s("PASSIVE_CRYSTAL_POWER", 0.075, 0.025),
        ],
        special: TalentSpecial::ModeledMobileBulwark,
    },
    TalentSpec {
        id: "Talent_Lyra_TwinMissiles",
        hero_id: Some("Lyra"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::Perk,
        mods: &[mod_s("DAMAGE_MODIFIER", 0.65, 0.03)],
        special: TalentSpecial::LyraTwinMissilesNoSlow,
    },
    // 113..116 Magnus
    TalentSpec {
        id: "Talent_Magnus_TalentA",
        hero_id: Some("Magnus"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::Perk,
        mods: &[mod_s("MARK_BONUS_DURATION", 0.4, 0.4)],
        special: TalentSpecial::MagnusMarkReveals,
    },
    TalentSpec {
        id: "Talent_Magnus_TalentB",
        hero_id: Some("Magnus"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::Perk,
        mods: &[mod_s("PASSIVE_ATTACK_SPEED", 0.12, 0.07)],
        special: TalentSpecial::MagnusMarkNotConsumed,
    },
    TalentSpec {
        id: "Talent_Magnus_TalentC",
        hero_id: Some("Magnus"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::AllAbilities,
        mods: &[mod_s("COOLDOWN_ON_HIT", 0.1, 0.05)],
        special: TalentSpecial::MagnusHitReducesAllCdr,
    },
    // 116..119 Malene
    TalentSpec {
        id: "Talent_Malene_TalentA",
        hero_id: Some("Malene"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 1.03, 0.03),
            mod_s("COOLDOWN_MODIFIER", 0.9, -0.01),
        ],
        special: TalentSpecial::MaleneShadowBExplosion,
    },
    TalentSpec {
        id: "Talent_Malene_TalentB",
        hero_id: Some("Malene"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 1.01, 0.01),
            mod_s("FEAR_DURATION", 0.6, 0.1),
        ],
        special: TalentSpecial::MaleneShadowAFear,
    },
    TalentSpec {
        id: "Talent_Malene_TalentC",
        hero_id: Some("Malene"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("BARRIER_MODIFIER", 1.2, 0.2),
            mod_s("BLESSING_RADIUS", 10.0, 0.0),
            mod_c("EXPLOSION_DAMAGE", 200.0, 150.0, 1.0, 0.0),
        ],
        special: TalentSpecial::MaleneLightBAllyBarrierBoom,
    },
    // 119..122 Miho
    TalentSpec {
        id: "Talent_Miho_TalentA",
        hero_id: Some("Miho"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::Perk,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 1.2, 0.05),
            mod_s("PASSIVE_WEAPON", 0.01, 0.01),
            mod_s("PASSIVE_CRYSTAL", 0.01, 0.01),
        ],
        special: TalentSpecial::MihoTrueBurstNoUnguarded,
    },
    TalentSpec {
        id: "Talent_Miho_TalentB",
        hero_id: Some("Miho"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 1.2, 0.04),
            mod_s("HEAL_PERCENT_MAX_HEALTH", 0.2, 0.08),
            mod_s("COOLDOWN_MODIFIER", 0.8, -0.04),
        ],
        special: TalentSpecial::MihoUltHealsAllies,
    },
    TalentSpec {
        id: "Talent_Miho_TalentC",
        hero_id: Some("Miho"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 0.5, 0.1),
            mod_s("CHARGES", 2.0, 0.0),
        ],
        special: TalentSpecial::None,
    },
    // 122..125 Ozo
    TalentSpec {
        id: "Talent_Ozo_Bangarangarang",
        hero_id: Some("Ozo"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 0.9, 0.15),
            mod_s("CHARGE_TIME", 0.7, -0.075),
            mod_s("CHARGES", 1.0, 0.0),
            mod_s("COOLDOWN_OVERRIDE", 1.0, 0.0),
        ],
        special: TalentSpecial::OzoUltBonusCharge,
    },
    TalentSpec {
        id: "Talent_Ozo_RingLeader",
        hero_id: Some("Ozo"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("RADIUS", 9.0, 0.0),
            mod_s("HEAL_MODIFIER", 1.1, 0.075),
        ],
        special: TalentSpecial::OzoCircusHealsAllies,
    },
    TalentSpec {
        id: "Talent_Ozo_SecondWind",
        hero_id: Some("Ozo"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 0.65, 0.02),
            mod_s("FORTIFIED_HEALTH_MODIFIER", 1.55, 0.075),
        ],
        special: TalentSpecial::OzoThirdBounceReset,
    },
    // 125..128 Petal
    TalentSpec {
        id: "Talent_Petal_Bouncer",
        hero_id: Some("Petal"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("COOLDOWN", 1.0, 0.0),
            mod_s("ENERGY_MODIFIER", 1.85, -0.05),
        ],
        special: TalentSpecial::PetalFastTrampoline,
    },
    TalentSpec {
        id: "Talent_Petal_ExplosionForce",
        hero_id: Some("Petal"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("KNOCKBACK_DISTANCE", 4.0, 0.0),
            mod_s("KNOCKBACK_HEIGHT", 2.5, 0.0),
            mod_s("KNOCKBACK_TIME", 0.5, 0.0),
            mod_s("DAMAGE_MODIFIER", 1.0, 0.15),
        ],
        special: TalentSpecial::PetalUltKnockback,
    },
    TalentSpec {
        id: "Talent_Petal_KaboomSeeds",
        hero_id: Some("Petal"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotA,
        mods: &[mod_s("DAMAGE_MODIFIER", 1.8, 0.1)],
        special: TalentSpecial::PetalSeedHeavyNoKnockback,
    },
    // 128..131 Phinn
    TalentSpec {
        id: "Talent_Phinn_FriendlyCavil",
        hero_id: Some("Phinn"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotAB,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 0.7, 0.03),
            mod_s("ENERGY_MODIFIER", 0.5, 0.0),
        ],
        special: TalentSpecial::PhinnBResetsAAndHalfCost,
    },
    TalentSpec {
        id: "Talent_Phinn_Phaster",
        hero_id: Some("Phinn"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("FORTIFIED_HEALTH_MODIFIER", 1.0, 0.01),
            mod_s("BONUS_MOVESPEED_STRENGTH", 1.3, 0.2),
        ],
        special: TalentSpecial::PhinnBGrantsSpeed,
    },
    TalentSpec {
        id: "Talent_Phinn_RaidBoss",
        hero_id: Some("Phinn"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::Perk,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 0.45, 0.075),
            mod_s("CURRENT_HEALTH_REGEN", 0.07, 0.02),
        ],
        special: TalentSpecial::PhinnRapidRegen,
    },
    // 131..134 Reim
    TalentSpec {
        id: "Talent_Reim_CreepingCold",
        hero_id: Some("Reim"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotA,
        mods: &[mod_s("IMPACT_TO_TRAVEL_DAMAGE", 0.35, 0.025)],
        special: TalentSpecial::ReimSpirePathDamage,
    },
    TalentSpec {
        id: "Talent_Reim_LongWinter",
        hero_id: Some("Reim"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::Perk,
        mods: &[
            mod_s("PASSIVE_CRYSTAL_POWER", 0.15, 0.025),
            mod_s("BONUS_DURATION", 30.0, 0.0),
        ],
        special: TalentSpecial::ReimLongFortified,
    },
    TalentSpec {
        id: "Talent_Reim_Revenant",
        hero_id: Some("Reim"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("MAX_HEALTH_TO_BARRIER", 0.2, 0.075),
            mod_s("BARRIER_DURATION", 2.0, 0.0),
        ],
        special: TalentSpecial::ReimChillBarrierSlowImmune,
    },
    // 134..137 Reza
    TalentSpec {
        id: "Talent_Reza_BurningBarrier",
        hero_id: Some("Reza"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotB,
        mods: &[mod_s("BARRIER_MODIFIER", 0.075, 0.01)],
        special: TalentSpecial::RezaDashBarrier,
    },
    TalentSpec {
        id: "Talent_Reza_Firemaker",
        hero_id: Some("Reza"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotB,
        mods: &[mod_s("DAMAGE_MODIFIER", 1.0, 0.05)],
        special: TalentSpecial::RezaDashAppliesFirestarter,
    },
    TalentSpec {
        id: "Talent_Reza_NetherRage",
        hero_id: Some("Reza"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("PASSIVE_ATTACK_SPEED", 0.15, 0.05),
            mod_s("BONUS_DURATION", 2.0, 1.0),
        ],
        special: TalentSpecial::RezaUltAttackSpeedDuration,
    },
    // 137..141 Ringo
    TalentSpec {
        id: "Talent_Ringo_AnotherRound",
        hero_id: Some("Ringo"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotC,
        mods: &[mod_s("PASSIVE_CRYSTAL_POWER", 0.2, 0.02)],
        special: TalentSpecial::RingoAnotherRoundKillReset,
    },
    TalentSpec {
        id: "Talent_Ringo_BulletSplit",
        hero_id: Some("Ringo"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 0.875, 0.025),
            mod_s("COOLDOWN_MODIFIER", 0.9, -0.03),
            mod_s("SEARCH_RANGE", 4.0, 0.0),
        ],
        special: TalentSpecial::RingoSplitShotB,
    },
    TalentSpec {
        id: "Talent_Ringo_LongShot",
        hero_id: Some("Ringo"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 1.0, 0.03),
            mod_s("BONUS_RANGE", 4.0, 0.0),
            mod_s("COOLDOWN_MODIFIER", 1.35, -0.02),
        ],
        special: TalentSpecial::ModeledLongShot,
    },
    TalentSpec {
        id: "Talent_Ringo_SpikedBrew",
        hero_id: Some("Ringo"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 0.7, 0.15),
            mod_s("COOLDOWN_MODIFIER", 0.7, -0.075),
            mod_s("BOUNCE_RANGE", 8.0, 0.0),
        ],
        special: TalentSpecial::RingoSpikedBrewBounce,
    },
    // 141..144 Rona (`Hero016`)
    TalentSpec {
        id: "Talent_Rona_FastWhirlwind",
        hero_id: Some("Hero016"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 1.0, 0.1),
            mod_s("COOLDOWN_MODIFIER", 1.25, -0.075),
            mod_s("BONUS_MOVESPEED_STRENGTH", 1.2, 0.0),
        ],
        special: TalentSpecial::RonaWhirlwindMoveSpeed,
    },
    TalentSpec {
        id: "Talent_Rona_HeroicBrawler",
        hero_id: Some("Hero016"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("PASSIVE_ATTACK_SPEED", 0.25, 0.05),
            mod_s("DAMAGE_MODIFIER", 0.5, 0.075),
            mod_s("COOLDOWN_REDUCTION_A", 0.1, 0.0),
        ],
        special: TalentSpecial::RonaAttacksReduceA,
    },
    TalentSpec {
        id: "Talent_Rona_MOARRage",
        hero_id: Some("Hero016"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::Perk,
        mods: &[
            mod_s("BONUS_ALT_ENERGY", 60.0, 10.0),
            mod_s("PASSIVE_WEAPON_POWER", 0.05, 0.015),
        ],
        special: TalentSpecial::RonaMaxBloodrage,
    },
    // 144..147 SAW
    TalentSpec {
        id: "Talent_SAW_PiercingBullets",
        hero_id: Some("SAW"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::Perk,
        mods: &[
            mod_s("BONUS_RANGE", 0.5, 0.0),
            mod_s("DAMAGE_MODIFIER", 0.9, 0.025),
        ],
        special: TalentSpecial::SawPiercingBasics,
    },
    TalentSpec {
        id: "Talent_SAW_RoadRage",
        hero_id: Some("SAW"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 1.15, 0.15),
            mod_s("COOLDOWN_MODIFIER", 0.5, 0.0),
            mod_s("STACKS", 6.0, 1.0),
        ],
        special: TalentSpecial::SawRoadieGrantsSpinUpAndCdr,
    },
    TalentSpec {
        id: "Talent_SAW_SpeedySpinUp",
        hero_id: Some("SAW"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::Perk,
        mods: &[mod_s("SPEED_BOOST_PER_STACK", 0.012, 0.017)],
        special: TalentSpecial::SawSpinUpSpeedNotSlow,
    },
    // 147..150 Samuel
    TalentSpec {
        id: "Talent_Samuel_PowerSiphon",
        hero_id: Some("Samuel"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::Perk,
        mods: &[
            mod_s("HEALING_MODIFIER", 1.5, 0.1),
            mod_s("CHARGE_TIME", 2.0, 0.0),
        ],
        special: TalentSpecial::SamuelFrequentPerk,
    },
    TalentSpec {
        id: "Talent_Samuel_UnwakingNightmare",
        hero_id: Some("Samuel"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("COOLDOWN_MODIFIER", 0.7, -0.025),
            mod_s("SLEEP_DURATION", 0.65, 0.05),
        ],
        special: TalentSpecial::SamuelSleepNeverWakes,
    },
    TalentSpec {
        id: "Talent_Samuel_ViscousDarkness",
        hero_id: Some("Samuel"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("HEALING_MODIFIER", 1.5, 0.25),
            mod_s("DAMAGE_MODIFIER", 1.5, 0.25),
            mod_s("DURATION", 6.0, 1.0),
            mod_s("SPEED", 1.0, 0.0),
            mod_s("SLOW_STRENGTH", 0.5, 0.0),
        ],
        special: TalentSpecial::SamuelSlowingDriftingDark,
    },
    // 150..153 Sanfeng
    TalentSpec {
        id: "Talent_Sanfeng_TalentA",
        hero_id: Some("Sanfeng"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 0.8, 0.02),
            mod_s("COOLDOWN_MODIFIER", 0.8, -0.02),
            mod_s("DISTANCE_THROWN", 10.0, 0.0),
        ],
        special: TalentSpecial::SanfengReverseFling,
    },
    TalentSpec {
        id: "Talent_Sanfeng_TalentB",
        hero_id: Some("Sanfeng"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 0.9, 0.03),
            mod_s("COOLDOWN_MODIFIER", 0.8, -0.05),
            mod_s("SLOW_DURATION", 1.4, 0.0),
        ],
        special: TalentSpecial::SanfengDivineFistSlow,
    },
    TalentSpec {
        id: "Talent_Sanfeng_TalentC",
        hero_id: Some("Sanfeng"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 0.8, 0.1),
            mod_s("COOLDOWN_MODIFIER", 1.0, -0.05),
        ],
        special: TalentSpecial::SanfengCounterRefreshesA,
    },
    // 153..156 Shin
    TalentSpec {
        id: "Talent_Shin_TalentA",
        hero_id: Some("Shin"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 1.1, 0.02),
            mod_s("COOLDOWN_MODIFIER", 0.95, -0.02),
        ],
        special: TalentSpecial::None,
    },
    TalentSpec {
        id: "Talent_Shin_TalentB",
        hero_id: Some("Shin"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 0.8, 0.05),
            mod_s("PASSIVE_CRYSTAL", 0.02, 0.02),
            mod_s("BONUS_RANGE", 1.5, 0.5),
        ],
        special: TalentSpecial::None,
    },
    TalentSpec {
        id: "Talent_Shin_TalentC",
        hero_id: Some("Shin"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 1.2, 0.05),
            mod_s("COOLDOWN_MODIFIER", -2.0, -2.0),
        ],
        special: TalentSpecial::None,
    },
    // 156..159 Silvernail
    TalentSpec {
        id: "Talent_Silvernail_TalentA",
        hero_id: Some("Silvernail"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("COOLDOWN_MODIFIER", 0.8, -0.02),
            mod_s("DAMAGE_MODIFIER", 0.5, 0.075),
            mod_s("BONUS_RANGE", 1.0, 0.2),
        ],
        special: TalentSpecial::SilvernailExtraBoltAndCRange,
    },
    TalentSpec {
        id: "Talent_Silvernail_TalentB",
        hero_id: Some("Silvernail"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("COOLDOWN_MODIFIER", 0.9, -0.03),
            mod_s("DAMAGE_MODIFIER", 0.6, 0.05),
        ],
        special: TalentSpecial::SilvernailThreeFlasks,
    },
    TalentSpec {
        id: "Talent_Silvernail_TalentC",
        hero_id: Some("Silvernail"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 0.7, 0.1),
            mod_s("DURATION_MODIFIER", 0.3, 0.05),
            mod_s("BONUS_RANGE", 6.0, 0.0),
        ],
        special: TalentSpecial::SilvernailTripwireTriangle,
    },
    // 159..162 Skaarf (`Hero010`)
    TalentSpec {
        id: "Talent_Skaarf_FlamingSludge",
        hero_id: Some("Hero010"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("SPITFIRE_DAMAGE_MODIFIER", 0.7, 0.05),
            mod_s("GOOP_DAMAGE_MODIFIER", 0.6, 0.1),
            mod_s("SLOW_MODIFIER", 0.25, 0.0),
            mod_s("RANGE", 9.0, 0.0),
        ],
        special: TalentSpecial::SkaarfSpitfireDropsGoop,
    },
    TalentSpec {
        id: "Talent_Skaarf_FlashFire",
        hero_id: Some("Hero010"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("COOLDOWN_MODIFIER", 0.6, -0.03),
            mod_s("DAMAGE_MODIFIER", 1.1, 0.1),
        ],
        special: TalentSpecial::SkaarfFastFlashFire,
    },
    TalentSpec {
        id: "Talent_Skaarf_Splitfire",
        hero_id: Some("Hero010"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 0.6, 0.02),
            mod_s("DAMAGE_MODIFIER_PER_HIT", 0.5, 0.0),
            mod_s("ANGLE_BETWEEN_FIREBALLS", 15.0, 0.0),
        ],
        special: TalentSpecial::SkaarfSplitfireThreeCones,
    },
    // 162..165 Skye
    TalentSpec {
        id: "Talent_Skye_EnhancedThrusters",
        hero_id: Some("Skye"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("BONUS_MOVESPEED_STRENGTH", 1.5, 0.1),
            mod_s("BONUS_MOVESPEED_DURATION", 3.0, 0.0),
            mod_s("PASSIVE_CRYSTAL_POWER", 0.05, 0.015),
            mod_s("PASSIVE_WEAPON_POWER", 0.05, 0.015),
        ],
        special: TalentSpecial::SkyeSuriGrantsSpeed,
    },
    TalentSpec {
        id: "Talent_Skye_PiercingBarrage",
        hero_id: Some("Skye"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 1.0, 0.1),
            mod_s("BONUS_RANGE", 2.0, 0.0),
        ],
        special: TalentSpecial::SkyePiercingBarrage,
    },
    TalentSpec {
        id: "Talent_Skye_RainingDeath",
        hero_id: Some("Skye"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 1.15, 0.15),
            mod_s("CHARGES", 1.0, 0.0),
            mod_s("CHARGE_TIME", 0.7, -0.025),
            mod_s("COOLDOWN_OVERRIDE", 1.0, 0.0),
        ],
        special: TalentSpecial::None,
    },
    // 165..168 Taka (`Sayoc`)
    TalentSpec {
        id: "Talent_Taka_ProtectiveDecoy",
        hero_id: Some("Sayoc"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("DURATION", 0.8, 0.0),
            mod_s("COOLDOWN_MODIFIER", 0.7, -0.025),
            mod_s("HEALING", 100.0, 0.0),
            mod_s("HEALING_MODIFIER", 1.0, 0.05),
        ],
        special: TalentSpecial::TakaKakuHealInvuln,
    },
    TalentSpec {
        id: "Talent_Taka_RangedKaiten",
        hero_id: Some("Sayoc"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 1.25, 0.04),
            mod_s("BONUS_RANGE", 5.0, 0.0),
        ],
        special: TalentSpecial::None,
    },
    TalentSpec {
        id: "Talent_Taka_XXXRetsu",
        hero_id: Some("Sayoc"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 1.0, 0.15),
            mod_s("DASH_TARGETS", 3.0, 0.0),
        ],
        special: TalentSpecial::TakaUltChainsTargets,
    },
    // 168..171 Tony
    TalentSpec {
        id: "Talent_Tony_TalentA",
        hero_id: Some("Tony"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::Perk,
        mods: &[
            mod_s("BONUS_ATTACKSPEED", 0.1, 0.025),
            mod_s("BARRIER_MODIFIER", 0.2, 0.05),
        ],
        special: TalentSpecial::TonyPerkAspdBarrier,
    },
    TalentSpec {
        id: "Talent_Tony_TalentB",
        hero_id: Some("Tony"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotA,
        mods: &[mod_s("DAMAGE_MODIFIER", 0.2, 0.025)],
        special: TalentSpecial::TonyJawbreakerAoE,
    },
    TalentSpec {
        id: "Talent_Tony_TalentC",
        hero_id: Some("Tony"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::Perk,
        mods: &[mod_s("BONUS_DURATION", 1.0, 0.2)],
        special: TalentSpecial::TonyImmuneToTaunted,
    },
    // 171..174 Varya
    TalentSpec {
        id: "Talent_Varya_TalentA",
        hero_id: Some("Varya"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("BONUS_RANGE", 0.8, 0.3),
            mod_s("DAMAGE_MODIFIER", 1.01, 0.01),
            mod_s("STUN_DURATION", 0.12, 0.02),
        ],
        special: TalentSpecial::VaryaSpearStuns,
    },
    TalentSpec {
        id: "Talent_Varya_TalentB",
        hero_id: Some("Varya"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("CHARGE_TIME", 0.3, 0.0),
            mod_s("BARRIER_MODIFIER", 1.3, 0.15),
        ],
        special: TalentSpecial::VaryaFastArcBarrier,
    },
    TalentSpec {
        id: "Talent_Varya_TalentC",
        hero_id: Some("Varya"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotC,
        mods: &[mod_s("DAMAGE_MODIFIER", 1.3, 0.35)],
        special: TalentSpecial::VaryaExtraGlobalStrikes,
    },
    // 174..177 Viola
    TalentSpec {
        id: "Talent_Viola_TalentA",
        hero_id: Some("Viola"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("BARRIER_MODIFIER", 1.1, 0.05),
            mod_s("BARRIER_STACKS", 2.0, 0.0),
        ],
        special: TalentSpecial::ViolaDoubleHarmonyStacks,
    },
    TalentSpec {
        id: "Talent_Viola_TalentB",
        hero_id: Some("Viola"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("DAMAGE_AMP", 0.1, 0.1),
            mod_s("DAMAGE_REDUCTION", 0.03, 0.03),
        ],
        special: TalentSpecial::ViolaBalladAmpAndDr,
    },
    TalentSpec {
        id: "Talent_Viola_TalentC",
        hero_id: Some("Viola"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotAB,
        mods: &[
            mod_s("BARRIER_MODIFIER", 0.6, 0.1),
            mod_s("DAMAGE_MODIFIER", 0.6, 0.1),
        ],
        special: TalentSpecial::ViolaChordChargesAndUltReset,
    },
    // 177..180 Vox
    TalentSpec {
        id: "Talent_Vox_DeepDash",
        hero_id: Some("Vox"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("BONUS_DISTANCE", 3.0, 0.0),
            mod_s("PASSIVE_ATTACK_SPEED", 0.25, 0.01),
            mod_s("COOLDOWN_MODIFIER", 1.0, -0.025),
        ],
        special: TalentSpecial::None,
    },
    TalentSpec {
        id: "Talent_Vox_EchoChamber",
        hero_id: Some("Vox"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::Perk,
        mods: &[mod_s("DAMAGE_MODIFIER", 0.8, 0.15)],
        special: TalentSpecial::VoxExtraBounce,
    },
    TalentSpec {
        id: "Talent_Vox_SoundBarrier",
        hero_id: Some("Vox"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::Perk,
        mods: &[
            mod_s("MAX_HEALTH_TO_BARRIER", 0.05, 0.025),
            mod_s("BARRIER_DURATION", 4.0, 0.0),
        ],
        special: TalentSpecial::VoxBasicAttackBarrier,
    },
    // 180..183 Warhawk
    TalentSpec {
        id: "Talent_Warhawk_TalentA",
        hero_id: Some("Warhawk"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("COOLDOWN_MODIFIER", 0.8, -0.02),
            mod_s("HEAL_AMOUNT", 250.0, 50.0),
        ],
        special: TalentSpecial::WarhawkGrenadeHealsAllies,
    },
    TalentSpec {
        id: "Talent_Warhawk_TalentB",
        hero_id: Some("Warhawk"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("COOLDOWN_MODIFIER", 0.9, -0.04),
            mod_s("DAMAGE_MODIFIER", 1.15, 0.04),
            mod_s("KNOCKBACK", -6.0, -0.2),
            mod_s("DASH_DURATION", 0.3, 0.0),
            mod_s("KNOCKBACK_STRING", 6.0, 0.2),
        ],
        special: TalentSpecial::WarhawkRocketMoreKnockback,
    },
    TalentSpec {
        id: "Talent_Warhawk_TalentC",
        hero_id: Some("Warhawk"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("IMPACT_DAMAGE_MODIFIER", 0.4, 0.07),
            mod_s("BURN_DAMAGE_MODIFIER", 1.5, 0.1),
        ],
        special: TalentSpecial::WarhawkFiveBarrageMissiles,
    },
    // 183..186 Yates
    TalentSpec {
        id: "Talent_Yates_TalentA",
        hero_id: Some("Yates"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("COOLDOWN_MODIFIER", 1.1, -0.02),
            mod_s("DAMAGE_MODIFIER", 0.75, 0.05),
            mod_s("BONUS_RANGE", 2.5, 0.4),
        ],
        special: TalentSpecial::None,
    },
    TalentSpec {
        id: "Talent_Yates_TalentB",
        hero_id: Some("Yates"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::Perk,
        mods: &[
            mod_s("DAMAGE_MODIFIER", 1.2, 0.12),
            mod_s("PASSIVE_ATTACK_SPEED", 0.0, 0.05),
        ],
        special: TalentSpecial::YatesFlailHeavyThirdHit,
    },
    TalentSpec {
        id: "Talent_Yates_TalentC",
        hero_id: Some("Yates"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("COOLDOWN_MODIFIER", 0.8, -0.05),
            mod_s("DAMAGE_MODIFIER", 1.2, 0.3),
            mod_s("BARRIER_MODIFIER", 1.4, 0.2),
            mod_s("CAST_SPEED_MOD", 0.5, 0.0),
        ],
        special: TalentSpecial::YatesFastUlt,
    },
    // 186..189 Ylva
    TalentSpec {
        id: "Talent_Ylva_TalentA",
        hero_id: Some("Ylva"),
        rarity: TalentRarity::Rare,
        scope: TalentSlotScope::SlotA,
        mods: &[
            mod_s("CHARGE_TIME", 0.8, -0.03),
            mod_s("COOLDOWN_MODIFIER", 1.0, -0.02),
            mod_s("DAMAGE_MODIFIER", 1.0, 0.02),
        ],
        special: TalentSpecial::None,
    },
    TalentSpec {
        id: "Talent_Ylva_TalentB",
        hero_id: Some("Ylva"),
        rarity: TalentRarity::Epic,
        scope: TalentSlotScope::SlotC,
        mods: &[
            mod_s("CHARGES", 2.0, 0.0),
            mod_s("TRAP_LIMIT", 3.0, 0.0),
            mod_s("COOLDOWN_MODIFIER", 0.8, -0.03),
            mod_s("DAMAGE_MODIFIER", 0.75, 0.05),
            mod_s("BONUS_RANGE", 6.0, 0.0),
        ],
        special: TalentSpecial::YlvaThreeBindingMines,
    },
    TalentSpec {
        id: "Talent_Ylva_TalentC",
        hero_id: Some("Ylva"),
        rarity: TalentRarity::Legendary,
        scope: TalentSlotScope::SlotB,
        mods: &[
            mod_s("COOLDOWN_MODIFIER", 0.9, -0.04),
            mod_s("DAMAGE_MODIFIER", 1.0, 0.03),
        ],
        special: TalentSpecial::YlvaDropMineOnB,
    },
];
