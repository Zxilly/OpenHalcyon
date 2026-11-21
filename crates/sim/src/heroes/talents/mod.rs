//! Runtime evaluation, validation, and combat effects for all 189 Vainglory talents.

use std::collections::BTreeMap;
use std::num::NonZeroU8;

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

pub(crate) mod catalog;
pub(crate) mod runtime;

pub(crate) use catalog::ALL_TALENTS;
pub(crate) use runtime::*;
