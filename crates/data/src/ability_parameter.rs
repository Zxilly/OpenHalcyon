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
        BurnDuration => "Burn_Duration",
        BurnRadius => "BurnRadius",
        BurstHealStrength => "BurstHealStrength",
        ChannelDuration => "Channel_Duration",
        Cooldown => "Cooldown",
        Duration => "DURATION",
        EffectDuration => "EFFECT_DURATION",
        EmpoweredBarrierAmount => "E_BARRIER_AMOUNT",
        EmpoweredDuration => "E_DURATION",
        EmpoweredShotSpeed => "SHOT_SPEED_EMPOWERED",
        EmpoweredStunMultiplier => "EMP_STUN_MULTIPLIER",
        EnergyCost => "Energy Cost",
        FirstHitDelay => "First Hit Delay",
        ImpactSplashRadius => "ImpactSplashRadius",
        MinionDamageReduction => "MINION_DAMAGE_REDUCTION",
        PostStunAttackLock => "PostStunAttackLock",
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
        BonusRange => "BONUS_RANGE",
        CooldownModifier => "COOLDOWN_MODIFIER",
        DamageModifier => "DAMAGE_MODIFIER",
    }
}
