//! Typed names used to look up shipped combat parameters.
//! Damage operands preserve the case used by the client's hash.
macro_rules! native_labels {
    ($name:ident { $($variant:ident => $wire:literal,)* }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name { $($variant,)* }
        impl $name {
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $wire,)* }
            }
        }
    };
}

native_labels! {
    AbilityParameter {
        AreaStunDuration => "STUN_DURATION",
        BarrierAmount => "BARRIER_AMOUNT",
        BarrierRatio => "BARRIER_RATIO",
        BaseBonusDamage => "BaseBonusDamage",
        BonusAttackSpeedStrength => "BonusASPDStrength",
        BonusBasicAttackRange => "BonusBasicAttackRange",
        BonusMoveSpeedStrength => "BonusMSPDStrength",
        HealPerSecond => "HealPerSecond",
        HealDuration => "HealDuration",
        ArcaneFireDuration => "BurnDuration",
        ArcaneSlowStrength => "Slow_Strength",
        ArcaneSlowDuration => "Slow_Duration",
        BurnBonusDamage => "BurnBonusDamage",
        BurstHealHealthRatio => "Burst_Heal_Health_Ratio",
        ForwardDashDistance => "FORWARD_DASH_DISTANCE",
        ForwardDashTime => "FORWARD_DASH_TIME",
        PassiveDecay => "PassiveDecay",
        ActiveDecay => "ActiveDecay",
        DetonateDamageHeroes => "DetonateDamageHeroes",
        DetonationEffectRatio => "DETONATION_EFFECT_RATIO",
        DecayingSpeedBoost => "SpeedBoost (Decaying)",
        HealPerSecondHealthRatio => "HealPerSecondHealthRatio",
        BurnDuration => "Burn_Duration",
        BurnRadius => "BurnRadius",
        BurstHealStrength => "BurstHealStrength",
        EffectRadius => "EffectRadius",
        FortifiedHealthStrength => "FortifiedHealthStrength",
        VerseStunDuration => "Stun_Duration",
        ChannelDuration => "Channel_Duration",
        Cooldown => "Cooldown",
        Duration => "DURATION",
        EffectDuration => "EFFECT_DURATION",
        EmpoweredBarrierAmount => "E_BARRIER_AMOUNT",
        EmpoweredDuration => "E_DURATION",
        EmpoweredShotSpeed => "SHOT_SPEED_EMPOWERED",
        EmpoweredStunMultiplier => "EMP_STUN_MULTIPLIER",
        UppercaseEnergyCost => "ENERGY COST",
        EnergyCost => "Energy Cost",
        FirstHitDelay => "First Hit Delay",
        ImpactSplashRadius => "ImpactSplashRadius",
        MinionDamageReduction => "MINION_DAMAGE_REDUCTION",
        PostStunAttackLock => "PostStunAttackLock",
        Startup => "Startup",
        NonDecayingSlowAmount => "NonDecaying Slow Amount",
        BulwarkSlowDuration => "Slow Duration",
        ImmunityDuration => "Immunity Duration",
        SkedaddleImmunityDuration => "CC Immunity Duration",
        SkedaddleSpeed => "Speed Boost Amount",
        SkedaddleDuration => "Speed Boost Duration",
        BubbleDuration => "BubbleDuration",
        BurnRange => "BurnRange",
        BaseHealthThreshold => "Base_Health_Threshold",
        ReflectRange => "DamageReflectRange",
        ReflectMultiplier => "ReflectDamageMultiplier",
        DurationLossPerProc => "Duration_Loss_Per_Proc",
        SilenceDuration => "SilenceDuration",
        SelfRevealDuration => "SelfRevealDuration",
        PreCastLock => "PreCastLock",
        PostCastLock => "PostCastLock",
        PullSpeed => "PULL_SPEED",
        PullDistance => "PULL_TO_DISTANCE",
        ChannelTime => "CHANNEL_TIME",
        LandingRadius => "IMPACT_RADIUS",
        LandingSlow => "SLOW_STRENGTH",
        LandingSlowDuration => "SLOW_DURATION",
        LandingBarrier => "BARRIER_STRENGTH",
        LandingBarrierDuration => "BARRIER_DURATION",
        Radius => "Radius",
        Range => "Range",
        RegularDelayTime => "REG_DELAY_TIME",
        RootDuration => "B_RootDuration",
        SecondHitDelay => "Second Hit Delay",
        SelfBonusDamage => "SelfBonusDamage",
        ShotSpeed => "SHOT_SPEED",
        SigilDamagePerSecond => "DamagePerSecond",
        Slow => "SLOW",
        SlowCap => "SLOW_CAP",
        SlowDuration => "SlowDuration",
        SlowRatio => "SLOW_RATIO",
        SlowStrength => "SlowStrength",
        SpeedBoost => "SPEED_BOOST",
        SpeedBoostDuration => "SpeedBoostDuration",
        SpeedBoostStrength => "SpeedBoostStrength",
        SpeedBuffDuration => "SpeedBuffDuration",
        StackDuration => "Stack_Duration",
        StaminaFromHeroImpacts => "Stamina From Hero Impacts",
        StunDuration => "StunDuration",
        StunDurationCap => "STUN_DURATION_CAP",
        StunDurationRatio => "STUN_DURATION_RATIO",
        StunWindow => "StunWindow",
        SuppressingFireDamagePerSecond => "DAMAGE_PER_SECOND",
        TicksDuringChannel => "TicksDuringChannel",
        TicksPerSecond => "TicksPerSecond",
    }
}

native_labels! {
    DamageOperand {
        AoEDamage => "AoEDamage",
        BonusCrystalDamage => "BonusCrystalDamage",
        BurnDamagePerSecond => "BurnDamagePerSecond",
        Damage => "Damage",
        DamageFirstHit => "Damage First Hit",
        DamageOnPassthrough => "Damage on Passthrough",
        DamageSecondHit => "Damage Second Hit",
        ImpactDamage => "ImpactDamage",
        LorelaiDamagePerSecond => "DAMAGE_PER_SEC",
        LorelaiImpactDamage => "IMPACT_DAMAGE",
        UppercaseDamage => "DAMAGE",
    }
}

native_labels! {
    TalentParameter {
        BonusDuration => "BONUS_DURATION",
        BonusRange => "BONUS_RANGE",
        CooldownModifier => "COOLDOWN_MODIFIER",
        DamageModifier => "DAMAGE_MODIFIER",
    }
}

/// Talents with a modeled authoritative effect. The content catalog also accepts
/// arbitrary IDs, including talents whose effects have not been implemented.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModeledTalent {
    LongShot,
    MobileBulwark,
    SpeedLoader,
}
impl ModeledTalent {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LongShot => "Talent_Ringo_LongShot",
            Self::MobileBulwark => "Talent_Lyra_MobileBulwark",
            Self::SpeedLoader => "Talent_Gwen_SpeedLoader",
        }
    }
    pub fn parse(id: &str) -> Option<Self> {
        [Self::LongShot, Self::MobileBulwark, Self::SpeedLoader]
            .into_iter()
            .find(|t| t.as_str() == id)
    }
    pub const fn hero(self) -> crate::EntityTemplate {
        match self {
            Self::LongShot => crate::EntityTemplate::Ringo,
            Self::MobileBulwark => crate::EntityTemplate::Lyra,
            Self::SpeedLoader => crate::EntityTemplate::Gwen,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat_content::{Ability, CombatContent, DamageVariable};
    use sha2::{Digest, Sha256};
    use std::path::Path;

    #[test]
    fn aoe_damage_operand_resolves_fnv1a_header_and_supplement_amael_c() {
        assert_eq!(DamageOperand::AoEDamage.as_str(), "AoEDamage");
        let synthetic = Ability {
            ability_id: "Ability__Amael__C".into(),
            damage_variables: vec![DamageVariable {
                header_u32: 2_853_935_461,
                ranks: vec![400., 525., 650.],
                cp_ratio: 1.8,
                wp_ratio: 1.5,
                ..Default::default()
            }],
            ..Default::default()
        };
        let aoe = synthetic
            .damage(DamageOperand::AoEDamage.as_str())
            .expect("synthetic AoEDamage lookup by FNV-1a header");
        assert_eq!(aoe.header_u32, 2_853_935_461);
        assert_eq!(aoe.ranks, [400., 525., 650.]);
        if let Some(extra) = std::env::var_os("HALCYON_DATA_EXTRA") {
            let content = CombatContent::load(Path::new(&extra), &mut Sha256::new())
                .expect("load HALCYON_DATA_EXTRA combat content");
            let amael_c = content
                .ability("Amael", "Ability__Amael__C")
                .expect("Ability__Amael__C");
            let loaded = amael_c
                .damage(DamageOperand::AoEDamage.as_str())
                .expect("Ability__Amael__C AoEDamage");
            assert_eq!(loaded.header_u32, 2_853_935_461);
            assert_eq!(loaded.ranks, [400., 525., 650.]);
            assert_eq!((loaded.cp_ratio, loaded.wp_ratio), (1.8, 1.5));
        }
    }
}
