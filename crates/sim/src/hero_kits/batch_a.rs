//! Complete A / B / C / Sub-ability / Heroic Perk specifications for `abilities.json`
//! indices `0..30` (`Amael` ..= `Leo`, including `Idris` #18 and `Karas` #22).

use crate::combat::{AbilitySlot, CastTarget};
use crate::hero_kit::{
    AbilityKitSpec, AbilityVariantSpec, BuffStatKind, CastGate, CastTargeting, DamageKind,
    HeroKitContext, HeroKitSpec, HeroSlot, HeroicPerkSpec, KitEffectKind, MarkKind,
    PrimitiveEffect, ResourceKind, ValueSpec, marks, seconds_to_ticks,
};
use crate::score::DamageSchool;
use crate::talents_runtime::TalentSpecial;
use crate::{Kind, TICK_HZ};

// ============================================================================
// 0. Amael (tid=925)
// ============================================================================

fn amael_bump_pump(ctx: &mut HeroKitContext<'_, '_>, hit_hero: bool) {
    if !hit_hero {
        return;
    }
    let st = ctx.room.entities[ctx.caster]
        .hero_kit
        .get_or_insert_with(Box::default);
    st.counters[0] += 1;
    if st.counters[0] >= 2 {
        st.counters[0] = 0;
        ctx.add_mark(ctx.caster, marks::PUMP_IT_UP, 1, 1, 6.0);
        ctx.add_buff(ctx.caster, BuffStatKind::WeaponPowerFlat, 25.0, 6.0);
    }
}

fn amael_consume_pump_stun(ctx: &mut HeroKitContext<'_, '_>, targets: &[usize]) {
    if ctx.consume_mark(ctx.caster, marks::PUMP_IT_UP) > 0 {
        for &j in targets {
            if ctx.room.entities[j].kind == Kind::Hero {
                ctx.apply_stun(j, 1.0);
            }
        }
    }
}

fn amael_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    rank: u8,
    _target: CastTarget,
) -> Result<bool, &'static str> {
    let chg = ctx.eval_var(
        "Ability__Amael__A",
        rank,
        ValueSpec::var("Chargetime", [1.4; 5]),
    ) * ctx.talent_mod("CHARGE_TIME").unwrap_or(1.0);
    let dur = ctx.eval_var(
        "Ability__Amael__A",
        rank,
        ValueSpec::var("Duration", [5.0; 5]),
    );
    {
        let st = ctx.room.entities[ctx.caster]
            .hero_kit
            .get_or_insert_with(Box::default);
        st.timers[0] = ctx.tick + seconds_to_ticks(chg);
    }
    ctx.open_recast(AbilitySlot::A, 1, 0, dur.max(1.5));
    Ok(true)
}

fn amael_a_release(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let charge_ready = ctx.room.entities[ctx.caster]
        .hero_kit
        .as_deref()
        .is_some_and(|k| ctx.tick >= k.timers[0]);
    ctx.clear_recast(AbilitySlot::A);
    let max_range = ctx.eval_var(
        "Ability__Amael__A",
        rank,
        ValueSpec::var("Range", [10.0, 10.0, 10.0, 10.0, 11.0]),
    );
    let dash_dist = if charge_ready {
        max_range
    } else {
        max_range * 0.55
    };
    let goal = ctx.resolve_target_pos(target, dash_dist);
    let origin = ctx.caster_pos();
    if ctx.room.spell_motion_clear(origin, goal) {
        ctx.room.place_spell_actor(ctx.caster, goal, ctx.events);
    }
    let dmg = ctx.eval_var(
        "Ability__Amael__A",
        rank,
        ValueSpec::dmg(0, [100.0, 170.0, 240.0, 310.0, 450.0], 1.0, 1.5, 0.0),
    ) * ctx.talent_dmg_mult(AbilitySlot::A);
    let hits = ctx.enemies_in_radius(ctx.caster_pos(), 3.5, false);
    let mut hit_hero = false;
    for &j in &hits {
        if ctx.room.entities[j].kind == Kind::Hero {
            hit_hero = true;
        }
        ctx.deal_damage(j, dmg, DamageSchool::Weapon, 0.0, false);
        ctx.knockback(j, ctx.caster_pos(), 8.0, 0.0);
    }
    amael_consume_pump_stun(ctx, &hits);
    amael_bump_pump(ctx, hit_hero);
    Ok(true)
}

fn amael_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let mut dist = ctx.eval_var(
        "Ability__Amael__B",
        rank,
        ValueSpec::var("FORWARD_DASH_DISTANCE", [9.0; 5]),
    );
    if ctx.talent_special() == TalentSpecial::None
        && let Some(extra) = ctx.talent_mod("DISTANCE")
    {
        dist += extra;
    }
    let goal = ctx.resolve_target_pos(target, dist);
    let dmg = ctx.eval_var(
        "Ability__Amael__B",
        rank,
        ValueSpec::dmg(0, [125.0, 160.0, 195.0, 230.0, 300.0], 0.6, 0.8, 0.0),
    ) * ctx.talent_dmg_mult(AbilitySlot::B);
    let origin = ctx.caster_pos();
    if ctx.room.spell_motion_clear(origin, goal) {
        ctx.room.place_spell_actor(ctx.caster, goal, ctx.events);
    }
    let hits = ctx.enemies_in_radius(ctx.caster_pos(), 3.2, false);
    let mut hit_hero = false;
    for &j in &hits {
        if ctx.room.entities[j].kind == Kind::Hero {
            hit_hero = true;
        }
        ctx.deal_damage(j, dmg, DamageSchool::Weapon, 0.0, false);
    }
    amael_consume_pump_stun(ctx, &hits);
    amael_bump_pump(ctx, hit_hero);
    Ok(true)
}

fn amael_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let goal = ctx.resolve_target_pos(target, 7.0);
    let origin = ctx.caster_pos();
    if ctx.room.spell_motion_clear(origin, goal) {
        ctx.room.place_spell_actor(ctx.caster, goal, ctx.events);
    }
    let tj = ctx.resolve_target_idx(target).or_else(|| {
        ctx.enemies_in_radius(ctx.caster_pos(), 3.0, true)
            .into_iter()
            .next()
    });
    let Some(j) = tj else {
        return Ok(true);
    };
    let was_cc = ctx.room.entities[j].status.stun_until > ctx.tick
        || ctx.room.entities[j].status.root_until > ctx.tick;
    if was_cc {
        let aoe = ctx.eval_var(
            "Ability__Amael__C",
            rank,
            ValueSpec::dmg3(1, [400.0, 525.0, 650.0], 1.8, 1.5, 0.0),
        ) * ctx.talent_dmg_mult(AbilitySlot::C);
        let hits = ctx.enemies_in_radius(ctx.caster_pos(), 4.5, false);
        for &k in &hits {
            ctx.deal_damage(k, aoe, DamageSchool::Weapon, 0.0, false);
            ctx.apply_stun(k, 0.8);
        }
        amael_consume_pump_stun(ctx, &hits);
        amael_bump_pump(ctx, !hits.is_empty());
    } else {
        let dmg = ctx.eval_var(
            "Ability__Amael__C",
            rank,
            ValueSpec::dmg3(0, [300.0, 400.0, 500.0], 1.8, 1.5, 0.0),
        ) * ctx.talent_dmg_mult(AbilitySlot::C);
        ctx.deal_damage(j, dmg, DamageSchool::Weapon, 0.0, false);
        ctx.apply_stun(j, 0.8);
        amael_consume_pump_stun(ctx, &[j]);
        amael_bump_pump(ctx, ctx.room.entities[j].kind == Kind::Hero);
    }
    Ok(true)
}

static AMAEL: HeroKitSpec = HeroKitSpec {
    hero_id: "Amael",
    type_id: 925,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Pump It Up",
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Amael__A",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        range: ValueSpec::var("Range", [10.0, 10.0, 10.0, 10.0, 11.0]),
        cooldown: ValueSpec::var("Cooldown", [14.0, 13.0, 12.0, 11.0, 8.0]),
        cost: ValueSpec::var("ENERGY COST", [40.0, 45.0, 50.0, 55.0, 60.0]),
        custom_exec: Some(amael_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Amael__B",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        range: ValueSpec::var("Range", [7.0; 5]),
        cooldown: ValueSpec::var("Cooldown", [8.0, 7.5, 7.0, 6.5, 6.0]),
        cost: ValueSpec::var("ENERGY COST", [50.0, 60.0, 70.0, 80.0, 90.0]),
        custom_exec: Some(amael_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Amael__C",
        targeting: CastTargeting::Directional,
        max_rank: 3,
        range: ValueSpec::var3("Range", [7.0; 3]),
        cooldown: ValueSpec::var3("Cooldown", [80.0, 65.0, 50.0]),
        cost: ValueSpec::var3("ENERGY COST", [80.0, 95.0, 110.0]),
        custom_exec: Some(amael_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    variants: &[AbilityVariantSpec {
        slot: HeroSlot::A2,
        spec: AbilityKitSpec {
            ability_id: "Ability__Amael__A_Cancel",
            targeting: CastTargeting::Directional,
            max_rank: 5,
            range: ValueSpec::var("Range", [10.0, 10.0, 10.0, 10.0, 11.0]),
            cooldown: ValueSpec::var("Cooldown", [14.0, 13.0, 12.0, 11.0, 8.0]),
            cost: ValueSpec::zero(),
            custom_exec: Some(amael_a_release),
            ..AbilityKitSpec::DEFAULT
        },
    }],
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 1. Adagio (tid=244) - bespoke A/B/C; adds Arcane Renewal perk
// ============================================================================

fn adagio_on_ability_hit(ctx: &mut HeroKitContext<'_, '_>, target_idx: usize, dmg: f32) {
    if ctx.mark_stacks(target_idx, marks::ARCANE_FIRE) > 0 {
        let e = &mut ctx.room.entities[ctx.caster];
        e.energy = (e.energy + dmg * 0.35).min(e.max_energy);
    }
}

static ADAGIO: HeroKitSpec = HeroKitSpec {
    hero_id: "Adagio",
    type_id: 244,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Arcane Renewal",
        on_ability_damage: Some(adagio_on_ability_hit),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Adagio__A",
        targeting: CastTargeting::TargetAllyOrSelf,
        range: ValueSpec::var("Range", [7.5; 5]),
        cooldown: ValueSpec::var("Cooldown", [12.0, 11.0, 10.0, 9.0, 8.0]),
        cost: ValueSpec::var("Energy Cost", [120.0, 135.0, 150.0, 165.0, 180.0]),
        effects: &[PrimitiveEffect::AllyHealBarrier {
            radius: ValueSpec::constant(5.0),
            include_self: true,
            target_only: true,
            burst_heal: ValueSpec::var("BurstHealStrength", [40.0, 60.0, 80.0, 100.0, 140.0]),
            hot_per_sec: ValueSpec::var("HealPerSecond", [10.0, 20.0, 30.0, 40.0, 50.0]),
            hot_duration: ValueSpec::constant(3.0),
            barrier: ValueSpec::zero(),
            barrier_duration: ValueSpec::zero(),
            fortified: ValueSpec::zero(),
            fortified_duration: ValueSpec::zero(),
            self_cast_mult: 1.0,
            grants_attack_buff_stacks: 0,
            grants_attack_bonus_dmg: ValueSpec::zero(),
        }],
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Adagio__B",
        targeting: CastTargeting::TargetAllyOrSelf,
        range: ValueSpec::var("Range", [7.5; 5]),
        cooldown: ValueSpec::var("Cooldown", [9.0; 5]),
        cost: ValueSpec::var("Energy Cost", [105.0, 130.0, 155.0, 180.0, 205.0]),
        effects: &[PrimitiveEffect::SelfBuff {
            stat: BuffStatKind::WeaponPowerFlat,
            amount: ValueSpec::var("BaseBonusDamage", [30.0, 50.0, 70.0, 90.0, 130.0]),
            duration: ValueSpec::constant(6.0),
            empowered_attacks: 5,
            empowered_bonus_damage: ValueSpec::var(
                "BaseBonusDamage",
                [30.0, 50.0, 70.0, 90.0, 130.0],
            ),
            empowered_stun_seconds: 0.0,
            empowered_slow_pct: 0.0,
            empowered_slow_seconds: 0.0,
            empowered_knockback: 0.0,
            resets_attack_cd: true,
        }],
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Adagio__C",
        targeting: CastTargeting::InstantSelf,
        max_rank: 3,
        startup: ValueSpec::var3("Channel_Duration", [2.0; 3]),
        range: ValueSpec::var3("EffectRadius", [9.0; 3]),
        cooldown: ValueSpec::var3("Cooldown", [100.0, 80.0, 60.0]),
        cost: ValueSpec::var3("Energy Cost", [140.0, 190.0, 240.0]),
        effects: &[PrimitiveEffect::DelayedBlast {
            delay: ValueSpec::zero(),
            radius: ValueSpec::var3("EffectRadius", [9.0; 3]),
            inner_radius: 0.0,
            damage: ValueSpec::dmg3(0, [800.0, 1000.0, 1200.0], 1.0, 0.0, 0.0),
            outer_mult: 1.0,
            school: DamageKind::Crystal,
            stun_seconds: ValueSpec::zero(),
            root_seconds: ValueSpec::zero(),
            sleep_seconds: ValueSpec::zero(),
            slow_pct: ValueSpec::zero(),
            slow_seconds: ValueSpec::zero(),
            reveal_seconds: 0.0,
            bonus_if_marked: None,
            minion_mult: 1.0,
            applies_mark: None,
        }],
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 2. Alpha (tid=268)
// ============================================================================

fn alpha_on_basic_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    target_idx: usize,
    _base: f32,
    _crit: bool,
) -> (f32, DamageSchool) {
    let stacks = ctx.mark_stacks(ctx.caster, marks::CORE_OVERLOAD);
    if stacks > 0 {
        let r = ctx.room.entities[ctx.caster].ability_ranks[1].max(1);
        let heal_per = ctx.eval_var(
            "Ability__Alpha__B",
            r,
            ValueSpec::var_scaled(
                "Heal Bonus per stack",
                [7.0, 8.0, 9.0, 10.0, 12.0],
                0.0,
                0.03,
                0.0,
                0.0,
            ),
        );
        let mult = if ctx.room.entities[target_idx].kind == Kind::Hero {
            1.0
        } else {
            0.25
        };
        ctx.heal_unit(ctx.caster, heal_per * f32::from(stacks) * mult);
        let cdr = if r >= 5 { 1.2 } else { 1.0 };
        ctx.reduce_slot_cd(AbilitySlot::B, cdr);
    }
    (0.0, DamageSchool::Weapon)
}

fn alpha_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let range = ctx.eval_var(
        "Ability__Alpha__A",
        rank,
        ValueSpec::var("Range", [10.0, 10.0, 10.0, 10.0, 12.0]),
    );
    let goal = ctx.resolve_target_pos(target, range);
    let origin = ctx.caster_pos();
    if ctx.room.spell_motion_clear(origin, goal) {
        ctx.room.place_spell_actor(ctx.caster, goal, ctx.events);
    }
    let stacks = ctx.consume_mark(ctx.caster, marks::CORE_OVERLOAD);
    let base_dmg = ctx.eval_var(
        "Ability__Alpha__A",
        rank,
        ValueSpec::dmg(0, [80.0, 100.0, 120.0, 140.0, 160.0], 1.8, 0.6, 0.0),
    );
    let per_stack = ctx.eval_var(
        "Ability__Alpha__A",
        rank,
        ValueSpec::var_scaled(
            "Damage Per Stack",
            [40.0, 60.0, 80.0, 100.0, 120.0],
            0.3,
            0.4,
            0.0,
            0.0,
        ),
    );
    let total = (base_dmg + per_stack * f32::from(stacks)) * ctx.talent_dmg_mult(AbilitySlot::A);
    let slow = ctx.eval_var(
        "Ability__Alpha__A",
        rank,
        ValueSpec::var("Slow", [0.4, 0.5, 0.6, 0.7, 0.8]),
    );
    for j in ctx.enemies_in_radius(ctx.caster_pos(), 4.5, false) {
        let m = if ctx.room.entities[j].kind == Kind::Hero {
            1.0
        } else {
            0.5
        };
        ctx.deal_damage(j, total * m, DamageSchool::Crystal, 0.0, false);
        ctx.apply_slow(j, slow, 1.5);
    }
    if stacks > 0 && ctx.talent_special() == TalentSpecial::AlphaPrimeTimeFortified {
        let fort_pct = ctx.talent_mod("HEALTH_FORT_PER_STACK").unwrap_or(0.15);
        let fort = ctx.room.entities[ctx.caster].max_hp * fort_pct * f32::from(stacks);
        ctx.grant_fortified(ctx.caster, fort, 3.0);
    }
    Ok(true)
}

fn alpha_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let stacks = ctx.mark_stacks(ctx.caster, marks::CORE_OVERLOAD);
    let hp_cost_pct = 0.02 + 0.02 * f32::from(stacks);
    let cur_hp = ctx.room.entities[ctx.caster].hp;
    ctx.room.entities[ctx.caster].hp = (cur_hp - cur_hp * hp_cost_pct).max(1.0);
    if let Some(j) = ctx.resolve_target_idx(target) {
        let goal = ctx.room.entities[j].position;
        if ctx.room.spell_motion_clear(ctx.caster_pos(), goal) {
            ctx.room.place_spell_actor(ctx.caster, goal, ctx.events);
        }
        let base_dmg = ctx.eval_var(
            "Ability__Alpha__B",
            rank,
            ValueSpec::dmg(0, [20.0, 35.0, 50.0, 65.0, 80.0], 1.0, 1.1, 0.0),
        );
        let stack_mult = 1.0 + f32::from(stacks) * if rank >= 5 { 0.15 } else { 0.11 };
        ctx.deal_damage(
            j,
            base_dmg * stack_mult * ctx.talent_dmg_mult(AbilitySlot::B),
            DamageSchool::Crystal,
            0.0,
            true,
        );
    }
    let new_stacks = ctx.add_mark(ctx.caster, marks::CORE_OVERLOAD, 1, 4, 8.0);
    if ctx.talent_special() == TalentSpecial::None && ctx.has_talent("Talent_Alpha_CoreOverclock") {
        ctx.add_buff(
            ctx.caster,
            BuffStatKind::AttackSpeed,
            0.15 * f32::from(new_stacks),
            8.0,
        );
    }
    Ok(true)
}

fn alpha_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    rank: u8,
    _target: CastTarget,
) -> Result<bool, &'static str> {
    let lvl = f32::from(ctx.room.entities[ctx.caster].level.saturating_sub(1)) / 11.0;
    let barrier = (400.0 + 600.0 * lvl) * ctx.talent_barrier_mult(AbilitySlot::C);
    ctx.grant_barrier(ctx.caster, barrier, 3.0, None);
    let spd = if ctx.talent_special() == TalentSpecial::AlphaAccelerationMoveSpeed {
        ctx.talent_mod("BONUS_MOVESPEED_STRENGTH").unwrap_or(2.0)
    } else {
        2.0
    };
    ctx.add_buff(ctx.caster, BuffStatKind::MoveSpeedFlat, spd, 1.5);
    let dmg = ctx.eval_var(
        "Ability__Alpha__C",
        rank,
        ValueSpec::dmg3(0, [529.0, 933.0, 1337.0], 2.5, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(AbilitySlot::C);
    let pos = ctx.caster_pos();
    ctx.spawn_effect(
        KitEffectKind::DelayedBlast {
            ability_slot: 2,
            release_tick: ctx.tick + seconds_to_ticks(3.0),
            radius: 4.5,
            inner_radius: 0.0,
            damage: dmg,
            outer_mult: 1.0,
            school: DamageSchool::Crystal,
            stun_ticks: 0,
            root_ticks: 0,
            sleep_ticks: 0,
            sleep_unwaking: false,
            slow_pct: 0.0,
            slow_ticks: 0,
            reveal_ticks: 0,
            bonus_mark: None,
            minion_mult: 1.0,
            applies_mark: None,
        },
        pos,
        pos,
    );
    Ok(true)
}

static ALPHA: HeroKitSpec = HeroKitSpec {
    hero_id: "Alpha",
    type_id: 268,
    resource: ResourceKind::None,
    perk: HeroicPerkSpec {
        id: "Infinite Reboot",
        on_basic_attack: Some(alpha_on_basic_attack),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Alpha__A",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::constant(0.4),
        range: ValueSpec::var("Range", [10.0, 10.0, 10.0, 10.0, 12.0]),
        cooldown: ValueSpec::var("Cooldown", [8.0, 7.5, 7.0, 6.5, 6.0]),
        custom_exec: Some(alpha_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Alpha__B",
        targeting: CastTargeting::TargetEnemy,
        max_rank: 5,
        startup: ValueSpec::constant(0.3),
        range: ValueSpec::constant(5.0),
        cooldown: ValueSpec::var("Cooldown", [5.0, 4.5, 4.0, 3.5, 3.0]),
        custom_exec: Some(alpha_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Alpha__C",
        targeting: CastTargeting::InstantSelf,
        max_rank: 3,
        range: ValueSpec::constant(4.5),
        cooldown: ValueSpec::var3("Cooldown", [85.0, 75.0, 65.0]),
        gate: CastGate::RequiresRebootReady,
        custom_exec: Some(alpha_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 3. Anka (tid=429)
// ============================================================================

fn anka_on_basic_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    target_idx: usize,
    _base: f32,
    _crit: bool,
) -> (f32, DamageSchool) {
    if ctx.room.entities[target_idx].kind != Kind::Hero {
        return (0.0, DamageSchool::Weapon);
    }
    let ready_at = ctx.room.entities[ctx.caster]
        .hero_kit
        .as_deref()
        .map_or(0, |k| k.timers[0]);
    if ctx.tick >= ready_at {
        let st = ctx.room.entities[ctx.caster]
            .hero_kit
            .get_or_insert_with(Box::default);
        st.timers[0] = ctx.tick + seconds_to_ticks(6.0);
        let t = &ctx.room.entities[target_idx];
        let missing = (t.max_hp - t.hp).max(0.0);
        return (missing * 0.12, DamageSchool::Crystal);
    }
    (0.0, DamageSchool::Weapon)
}

fn anka_on_kill_assist(
    ctx: &mut HeroKitContext<'_, '_>,
    _victim: usize,
    _killer: bool,
    is_hero: bool,
) {
    if !is_hero {
        return;
    }
    if let Some(k) = ctx.room.entities[ctx.caster].hero_kit.as_deref_mut() {
        k.timers[0] = 0;
    }
    ctx.reset_slot_cd(AbilitySlot::A);
}

fn anka_a_recast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let tid = ctx.room.entities[ctx.caster]
        .hero_kit
        .as_deref()
        .map_or(0, |k| k.recast_target[0]);
    ctx.clear_recast(AbilitySlot::A);
    let tj = ctx
        .room
        .index(tid)
        .or_else(|| ctx.resolve_target_idx(target));
    if let Some(j) = tj {
        let dest = ctx.room.entities[j].position;
        if ctx.room.spell_motion_clear(ctx.caster_pos(), dest) {
            ctx.room.place_spell_actor(ctx.caster, dest, ctx.events);
        }
        let dmg = ctx.eval_var(
            "Ability__Anka__A2",
            rank,
            ValueSpec::var_scaled(
                "Damage",
                [50.0, 85.0, 120.0, 155.0, 225.0],
                0.6,
                0.0,
                0.0,
                0.0,
            ),
        ) * ctx.talent_dmg_mult(AbilitySlot::A);
        ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
        let spd = ctx.talent_mod("BONUS_MOVESPEED_STRENGTH").unwrap_or(3.0);
        let dur = ctx.talent_mod("BONUS_MOVESPEED_DURATION").unwrap_or(2.0);
        ctx.add_buff(ctx.caster, BuffStatKind::MoveSpeedFlat, spd, dur);
    }
    Ok(true)
}

fn anka_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let r = ctx.eval_var(
        "Ability__Anka__A",
        rank,
        ValueSpec::var("Range", [10.0, 10.0, 10.0, 10.0, 12.0]),
    ) + ctx.talent_mod("BONUS_RANGE").unwrap_or(0.0);
    let aim = ctx.resolve_target_pos(target, r);
    let hits = ctx.enemies_in_radius(aim, 2.5, false);
    if let Some(&j) = hits.first() {
        let eid = ctx.room.entities[j].id;
        let dmg = ctx.eval_var(
            "Ability__Anka__A",
            rank,
            ValueSpec::dmg(0, [25.0, 50.0, 75.0, 100.0, 150.0], 0.4, 0.0, 0.0),
        ) * ctx.talent_dmg_mult(slot);
        ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
        ctx.add_mark(j, marks::ANKA_MARKED, 1, 1, 2.8);
        ctx.open_recast(AbilitySlot::A, 1, eid, 2.5);
        return Ok(true);
    }
    Ok(false)
}

static ANKA_B_EFFS: &[PrimitiveEffect] = &[PrimitiveEffect::ConeWave {
    range: ValueSpec::var("Range", [8.0; 5]),
    half_angle_deg: 35.0,
    damage: ValueSpec::dmg(0, [50.0, 80.0, 110.0, 140.0, 200.0], 0.45, 0.0, 0.0),
    school: DamageKind::Crystal,
    multi_bolts: 5,
    extra_hit_mult: 0.4,
    slow_pct: ValueSpec::zero(),
    slow_seconds: ValueSpec::zero(),
    stun_seconds: ValueSpec::zero(),
    fear_seconds: ValueSpec::zero(),
    silence_seconds: ValueSpec::zero(),
    knockback_dist: 0.0,
    reveal_seconds: 0.0,
    minion_mult: 1.0,
    applies_mark: None,
}];

static ANKA_C_EFFS: &[PrimitiveEffect] = &[
    PrimitiveEffect::CleanseImmune {
        cleanse_cc: false,
        cc_immune_seconds: ValueSpec::zero(),
        untargetable_seconds: ValueSpec::var3("DashDuration", [0.6; 3]),
    },
    PrimitiveEffect::DashBlink {
        range: ValueSpec::var3("Range", [6.0; 3]),
        duration: ValueSpec::var3("DashDuration", [0.6; 3]),
        instant_blink: false,
        invuln_during: true,
        unstoppable_during: true,
        path_radius: 2.2,
        path_damage: ValueSpec::dmg3(0, [100.0, 140.0, 180.0], 0.4, 0.0, 0.0),
        arrival_radius: 3.0,
        arrival_damage: ValueSpec::dmg3(1, [200.0, 280.0, 360.0], 0.8, 0.0, 0.0),
        school: DamageKind::Crystal,
        slow_pct: ValueSpec::zero(),
        slow_seconds: ValueSpec::zero(),
        stun_seconds: ValueSpec::zero(),
        knockback_dist: 0.0,
        backward_hop: false,
        applies_mark: None,
    },
];

static ANKA: HeroKitSpec = HeroKitSpec {
    hero_id: "Anka",
    type_id: 429,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Gythian Promise",
        on_basic_attack: Some(anka_on_basic_attack),
        on_kill_assist: Some(anka_on_kill_assist),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Anka__A",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        range: ValueSpec::var("Range", [10.0, 10.0, 10.0, 10.0, 12.0]),
        cooldown: ValueSpec::var("Cooldown", [12.0, 12.0, 12.0, 12.0, 8.0]),
        cost: ValueSpec::var("Energy Cost", [40.0, 45.0, 50.0, 55.0, 60.0]),
        effects: &[PrimitiveEffect::SkillshotProjectile {
            damage: ValueSpec::dmg(0, [25.0, 50.0, 75.0, 100.0, 150.0], 0.4, 0.0, 0.0),
            school: DamageKind::Crystal,
            range: ValueSpec::var("Range", [10.0, 10.0, 10.0, 10.0, 12.0]),
            speed: ValueSpec::constant(25.0),
            radius: ValueSpec::constant(1.0),
            piercing: false,
            return_to_caster: false,
            repeat_hit_mult: 1.0,
            splash_damage: None,
            splash_radius: 0.0,
            stun_seconds: ValueSpec::zero(),
            root_seconds: ValueSpec::zero(),
            silence_seconds: ValueSpec::zero(),
            slow_pct: ValueSpec::zero(),
            slow_seconds: ValueSpec::zero(),
            minion_mult: 1.0,
            resets_cast_cd_on_hit: false,
            applies_mark: Some((MarkKind::Custom(marks::ANKA_MARKED), 1, 2.8)),
            destroys_enemy_projectiles: false,
        }],
        custom_exec: Some(anka_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Anka__B",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        range: ValueSpec::var("Range", [8.0; 5]),
        cooldown: ValueSpec::var("Cooldown", [8.0, 7.0, 6.0, 5.0, 4.0]),
        cost: ValueSpec::var("Energy Cost", [30.0, 40.0, 50.0, 60.0, 70.0]),
        effects: ANKA_B_EFFS,
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Anka__C",
        targeting: CastTargeting::Directional,
        max_rank: 3,
        range: ValueSpec::var3("Range", [6.0; 3]),
        cooldown: ValueSpec::var3("Cooldown", [100.0, 80.0, 60.0]),
        cost: ValueSpec::var3("Energy Cost", [70.0, 80.0, 90.0]),
        effects: ANKA_C_EFFS,
        ..AbilityKitSpec::DEFAULT
    },
    variants: &[AbilityVariantSpec {
        slot: HeroSlot::A2,
        spec: AbilityKitSpec {
            ability_id: "Ability__Anka__A2",
            targeting: CastTargeting::Directional,
            max_rank: 5,
            range: ValueSpec::constant(60.0),
            cooldown: ValueSpec::var("Cooldown", [12.0, 12.0, 12.0, 12.0, 8.0]),
            cost: ValueSpec::zero(),
            custom_exec: Some(anka_a_recast),
            ..AbilityKitSpec::DEFAULT
        },
    }],
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 4. Ardan (tid=257)
// ============================================================================

fn ardan_on_take_damage(
    ctx: &mut HeroKitContext<'_, '_>,
    _src: Option<usize>,
    dmg: f32,
    _school: DamageSchool,
) -> f32 {
    let e = &ctx.room.entities[ctx.caster];
    let missing = (e.max_hp - e.hp).max(0.0);
    let heal = (missing * 0.15).min(dmg * 0.5);
    if heal > 0.0 {
        ctx.heal_unit(ctx.caster, heal);
    }
    ctx.add_resource(4.0);
    1.0
}

fn ardan_on_basic_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    _target: usize,
    _base: f32,
    crit: bool,
) -> (f32, DamageSchool) {
    ctx.add_resource(if crit { 15.0 } else { 10.0 });
    ctx.reduce_slot_cd(AbilitySlot::B, if crit { 1.4 } else { 0.9 });
    (0.0, DamageSchool::Weapon)
}

fn ardan_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let ally_idx = ctx.resolve_target_idx(target).unwrap_or(ctx.caster);
    let is_self = ally_idx == ctx.caster;
    let self_mult = if is_self && ctx.talent_special() != TalentSpecial::ArdanFullSelfVanguard {
        0.5
    } else {
        1.0
    };
    let ally_pos = ctx.room.entities[ally_idx].position;
    if !is_self && ctx.room.spell_motion_clear(ctx.caster_pos(), ally_pos) {
        ctx.room.place_spell_actor(ctx.caster, ally_pos, ctx.events);
    }
    let barrier = (ctx.eval_var(
        "Ability__Ardan__A",
        rank,
        ValueSpec::var("BarrierStrength", [80.0, 120.0, 160.0, 200.0, 240.0]),
    ) + ctx.room.entities[ctx.caster].bonus_hp * 0.35)
        * self_mult
        * ctx.talent_barrier_mult(AbilitySlot::A);
    ctx.grant_barrier(ally_idx, barrier, 2.0, None);
    ctx.add_buff(ally_idx, BuffStatKind::MoveSpeedFlat, 2.0 * self_mult, 2.0);
    let dmg = ctx.eval_var(
        "Ability__Ardan__A",
        rank,
        ValueSpec::dmg(0, [60.0, 100.0, 140.0, 180.0, 220.0], 1.2, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(AbilitySlot::A);
    for j in ctx.enemies_in_radius(ally_pos, 5.0, false) {
        ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
        ctx.apply_slow(j, 0.25, 2.0);
    }
    if rank >= 5 && !is_self {
        ctx.add_resource(50.0);
    }
    Ok(true)
}

fn ardan_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    ctx.spend_resource(100.0);
    if let Some(j) = ctx.resolve_target_idx(target) {
        let dest = ctx.room.entities[j].position;
        if ctx.room.spell_motion_clear(ctx.caster_pos(), dest) {
            ctx.room.place_spell_actor(ctx.caster, dest, ctx.events);
        }
        let mut dmg = ctx.eval_var(
            "Ability__Ardan__B",
            rank,
            ValueSpec::dmg(0, [60.0, 120.0, 180.0, 240.0, 360.0], 0.4, 1.0, 0.0),
        );
        if rank >= 5 {
            dmg *= 1.15;
        }
        dmg *= ctx.talent_dmg_mult(AbilitySlot::B);
        ctx.deal_damage(j, dmg, DamageSchool::Weapon, 0.0, true);
        if ctx.talent_special() == TalentSpecial::ArdanKnockoutPush {
            let dist = ctx.talent_mod("DISTANCE").unwrap_or(10.0);
            ctx.knockback(j, ctx.caster_pos(), dist, 0.0);
        }
    }
    Ok(true)
}

fn ardan_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    ctx.add_resource(100.0);
    let goal = ctx.resolve_target_pos(target, 8.0);
    if ctx.room.spell_motion_clear(ctx.caster_pos(), goal) {
        ctx.room.place_spell_actor(ctx.caster, goal, ctx.events);
    }
    let dmg = ctx.eval_var(
        "Ability__Ardan__C",
        rank,
        ValueSpec::dmg3(0, [400.0, 500.0, 600.0], 2.5, 0.0, 0.0),
    );
    let stun = ctx.eval_var(
        "Ability__Ardan__C",
        rank,
        ValueSpec::var3("StunDuration", [0.6, 0.9, 1.2]),
    );
    ctx.spawn_effect(
        KitEffectKind::SegmentBarrier {
            ability_slot: 2,
            armed_at: ctx.tick + seconds_to_ticks(1.0),
            end_tick: ctx.tick + seconds_to_ticks(7.0),
            end_pos: goal,
            radius: 7.0,
            is_ring_arena: true,
            dps_or_hit_damage: dmg,
            school: DamageSchool::Crystal,
            stun_ticks: seconds_to_ticks(stun),
            silence_ticks: 0,
            slow_pct: 0.0,
            next_tick: ctx.tick,
            struck_recently: Vec::new(),
        },
        goal,
        goal,
    );
    Ok(true)
}

static ARDAN: HeroKitSpec = HeroKitSpec {
    hero_id: "Ardan",
    type_id: 257,
    resource: ResourceKind::Vengeance,
    resource_max: 100.0,
    resource_initial: 50.0,
    resource_regen_per_sec: 4.0,
    perk: HeroicPerkSpec {
        id: "Julia's Gift",
        on_basic_attack: Some(ardan_on_basic_attack),
        on_take_damage: Some(ardan_on_take_damage),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Ardan__A",
        targeting: CastTargeting::TargetAllyOrSelf,
        max_rank: 5,
        range: ValueSpec::var("Range", [7.0, 7.0, 7.0, 7.0, 9.0]),
        cooldown: ValueSpec::var("Cooldown", [17.0, 16.0, 15.0, 14.0, 12.0]),
        custom_exec: Some(ardan_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Ardan__B",
        targeting: CastTargeting::TargetEnemy,
        max_rank: 5,
        range: ValueSpec::var("Range", [7.0; 5]),
        cooldown: ValueSpec::var("Cooldown", [20.0, 19.0, 18.0, 17.0, 16.0]),
        gate: CastGate::RequiresFullResource,
        custom_exec: Some(ardan_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Ardan__C",
        targeting: CastTargeting::TargetLocationOrEntity,
        max_rank: 3,
        range: ValueSpec::var3("Range", [8.0; 3]),
        cooldown: ValueSpec::var3("Cooldown", [120.0, 105.0, 90.0]),
        custom_exec: Some(ardan_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 5. Baptiste (tid=399)
// ============================================================================

fn baptiste_collect_soul(ctx: &mut HeroKitContext<'_, '_>) {
    if ctx.mark_stacks(ctx.caster, marks::BAPTISTE_EMPOWERED) > 0
        && ctx.talent_special() != TalentSpecial::BaptisteReaperOverflowSouls
    {
        return;
    }
    let lvl = f32::from(ctx.room.entities[ctx.caster].level.saturating_sub(1));
    let cp = ctx.eval_inputs().cp;
    let heal =
        (15.0 + 3.0 * lvl + 0.25 * cp) * ctx.talent_mod("HEAL_MODIFIER").map_or(1.0, |m| 1.0 + m);
    ctx.heal_unit(ctx.caster, heal);
    let stacks = ctx.add_mark(ctx.caster, marks::SOUL_FRAGMENT, 1, 4, 15.0);
    if stacks >= 4 {
        ctx.consume_mark(ctx.caster, marks::SOUL_FRAGMENT);
        ctx.add_mark(ctx.caster, marks::BAPTISTE_EMPOWERED, 1, 1, 4.0);
        ctx.add_buff(ctx.caster, BuffStatKind::DamageAmp, 0.2, 4.0);
    }
}

fn baptiste_on_basic_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    _target: usize,
    base: f32,
    _crit: bool,
) -> (f32, DamageSchool) {
    baptiste_collect_soul(ctx);
    let emp = ctx.mark_stacks(ctx.caster, marks::BAPTISTE_EMPOWERED) > 0;
    if emp {
        (base * 0.25, DamageSchool::Weapon)
    } else {
        (0.0, DamageSchool::Weapon)
    }
}

fn baptiste_on_ability_hit(ctx: &mut HeroKitContext<'_, '_>, target_idx: usize, _dmg: f32) {
    if ctx.room.entities[target_idx].kind == Kind::Hero {
        baptiste_collect_soul(ctx);
    }
}

fn baptiste_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let range = ctx.eval_var(
        "Ability__Baptiste__C",
        rank,
        ValueSpec::var3("Range", [22.0; 3]),
    ) + ctx.talent_mod("RANGE").unwrap_or(0.0);
    let dmg = ctx.eval_var(
        "Ability__Baptiste__C",
        rank,
        ValueSpec::var3("DAMAGE_PER_SECOND", [120.0, 160.0, 200.0]),
    ) * ctx.talent_dmg_mult(slot);
    let fear = ctx.eval_var(
        "Ability__Baptiste__C",
        rank,
        ValueSpec::var3("DEBUFF_DURATION", [1.0, 1.3, 1.6]),
    );
    let count = if ctx.talent_special() == TalentSpecial::BaptisteArmyDualWave {
        2
    } else {
        1
    };
    let aim = ctx.resolve_target_pos(target, range);
    let origin = ctx.caster_pos();
    let total_r = origin.dist(aim).clamp(4.0, range);
    for _ in 0..count {
        for j in ctx.enemies_in_radius(aim, total_r.min(8.0), false) {
            ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
            if ctx.room.entities[j].kind == Kind::Hero {
                ctx.apply_fear(j, fear);
            }
        }
    }
    Ok(true)
}

static BAPTISTE: HeroKitSpec = HeroKitSpec {
    hero_id: "Baptiste",
    type_id: 399,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Reap",
        on_basic_attack: Some(baptiste_on_basic_attack),
        on_ability_damage: Some(baptiste_on_ability_hit),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Baptiste__A",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        range: ValueSpec::var("Range", [7.0, 7.0, 7.0, 7.0, 8.0]),
        cooldown: ValueSpec::var("Cooldown", [4.0, 3.8, 3.6, 3.4, 3.0]),
        cost: ValueSpec::var("Energy Cost", [40.0, 45.0, 50.0, 55.0, 60.0]),
        effects: &[PrimitiveEffect::SkillshotProjectile {
            damage: ValueSpec::var_scaled(
                "Damage",
                [80.0, 120.0, 160.0, 200.0, 280.0],
                1.2,
                0.0,
                0.0,
                0.0,
            ),
            school: DamageKind::Crystal,
            range: ValueSpec::var("Range", [7.0, 7.0, 7.0, 7.0, 8.0]),
            speed: ValueSpec::var("PROJECTILE_SPEED", [17.0; 5]),
            radius: ValueSpec::constant(1.0),
            piercing: false,
            return_to_caster: false,
            repeat_hit_mult: 1.0,
            splash_damage: Some(ValueSpec::var_scaled(
                "AreaDamage",
                [40.0, 60.0, 80.0, 100.0, 140.0],
                0.62,
                0.0,
                0.0,
                0.0,
            )),
            splash_radius: 2.5,
            stun_seconds: ValueSpec::zero(),
            root_seconds: ValueSpec::zero(),
            silence_seconds: ValueSpec::zero(),
            slow_pct: ValueSpec::var("SLOW_STRENGTH", [0.6; 5]),
            slow_seconds: ValueSpec::var("SLOW_DURATION", [0.8, 0.8, 0.8, 0.8, 1.2]),
            minion_mult: 0.5,
            resets_cast_cd_on_hit: false,
            applies_mark: None,
            destroys_enemy_projectiles: false,
        }],
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Baptiste__B",
        targeting: CastTargeting::TargetEnemy,
        max_rank: 5,
        range: ValueSpec::var("Range", [6.0; 5]),
        cooldown: ValueSpec::var("Cooldown", [17.0, 16.0, 15.0, 14.0, 13.0]),
        cost: ValueSpec::var("Energy Cost", [60.0, 70.0, 80.0, 90.0, 100.0]),
        effects: &[PrimitiveEffect::LungeHook {
            range: ValueSpec::var("Range", [6.0; 5]),
            damage: ValueSpec::var_scaled(
                "Damage",
                [45.0, 75.0, 105.0, 135.0, 195.0],
                0.45,
                0.0,
                0.0,
                0.0,
            ),
            school: DamageKind::Crystal,
            pull_distance: ValueSpec::zero(),
            knockback_distance: ValueSpec::zero(),
            stun_on_wall_seconds: ValueSpec::var("StunDuration", [1.2, 1.2, 1.2, 1.2, 1.5]),
            tether_duration: ValueSpec::var("TETHER_DURATION", [2.5; 5]),
            tether_break_range: ValueSpec::var("TETHER_RADIUS", [4.0; 5]),
            tether_dps_pct_hp: ValueSpec::zero(),
            devour_seconds: ValueSpec::zero(),
            suppress_untargetable: false,
        }],
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Baptiste__C",
        targeting: CastTargeting::Directional,
        max_rank: 3,
        startup: ValueSpec::var3("CAST_DELAY", [0.7; 3]),
        range: ValueSpec::var3("Range", [22.0; 3]),
        cooldown: ValueSpec::var3("Cooldown", [80.0, 65.0, 50.0]),
        cost: ValueSpec::var3("Energy Cost", [60.0, 80.0, 100.0]),
        custom_exec: Some(baptiste_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 6. Baron (tid=261)
// ============================================================================

fn baron_on_basic_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    target_idx: usize,
    base: f32,
    _crit: bool,
) -> (f32, DamageSchool) {
    ctx.reduce_slot_cd(AbilitySlot::B, 1.5);
    let tpos = ctx.room.entities[target_idx].position;
    let splash = base * 0.3;
    for j in ctx.enemies_in_radius(tpos, 2.2, false) {
        if j != target_idx {
            let m = if ctx.room.entities[j].kind == Kind::Hero {
                1.0
            } else {
                0.5
            };
            ctx.deal_damage(j, splash * m, DamageSchool::Weapon, 0.0, false);
        }
    }
    (base * 0.3, DamageSchool::Weapon)
}

fn baron_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let goal = ctx.resolve_target_pos(target, 10.0);
    if rank >= 5 {
        ctx.set_unstoppable(ctx.caster, 1.0);
    }
    if ctx.room.spell_motion_clear(ctx.caster_pos(), goal) {
        ctx.room.place_spell_actor(ctx.caster, goal, ctx.events);
    }
    let spd = ctx.eval_var(
        "Ability__Baron__B",
        rank,
        ValueSpec::var_scaled(
            "Quick Attack Speed",
            [0.5, 0.6, 0.7, 0.8, 1.0],
            0.0,
            0.003,
            0.0,
            0.0,
        ),
    );
    let dur = ctx.eval_var(
        "Ability__Baron__B",
        rank,
        ValueSpec::var("Quick Attack Duration", [2.1; 5]),
    );
    ctx.add_buff(ctx.caster, BuffStatKind::MoveSpeedFlat, spd, dur);
    ctx.empower_next_attacks(
        if ctx.talent_special() == TalentSpecial::BaronTurboJetsTripleShot {
            2
        } else {
            1
        },
        ctx.room.entities[ctx.caster].weapon_power() * 0.65,
        false,
        0.0,
        0.0,
        0.0,
        0.0,
        dur,
    );
    Ok(true)
}

static BARON_A_EFFS: &[PrimitiveEffect] = &[PrimitiveEffect::DelayedBlast {
    delay: ValueSpec::var("Projectile Travel Time", [1.2; 5]),
    radius: ValueSpec::var("AOE Radius", [3.0; 5]),
    inner_radius: 0.0,
    damage: ValueSpec::dmg(0, [40.0, 120.0, 200.0, 280.0, 360.0], 1.3, 0.0, 0.7),
    outer_mult: 1.0,
    school: DamageKind::Crystal,
    stun_seconds: ValueSpec::zero(),
    root_seconds: ValueSpec::zero(),
    sleep_seconds: ValueSpec::zero(),
    slow_pct: ValueSpec::var_scaled("Decaying Slow Amount", [0.1; 5], 0.0, 0.001, 0.0, 0.0),
    slow_seconds: ValueSpec::var("Decaying Slow Duration", [2.2; 5]),
    reveal_seconds: 0.0,
    bonus_if_marked: None,
    minion_mult: 0.5,
    applies_mark: None,
}];

static BARON_C_EFFS: &[PrimitiveEffect] = &[PrimitiveEffect::DelayedBlast {
    delay: ValueSpec::var3("Delay Before Impact", [3.0; 3]),
    radius: ValueSpec::var3("Damage Radius", [5.0; 3]),
    inner_radius: 2.8,
    damage: ValueSpec::dmg3(0, [500.0, 750.0, 1000.0], 1.5, 0.0, 1.2),
    outer_mult: 0.65,
    school: DamageKind::Crystal,
    stun_seconds: ValueSpec::zero(),
    root_seconds: ValueSpec::zero(),
    sleep_seconds: ValueSpec::zero(),
    slow_pct: ValueSpec::zero(),
    slow_seconds: ValueSpec::zero(),
    reveal_seconds: 3.0,
    bonus_if_marked: None,
    minion_mult: 0.5,
    applies_mark: None,
}];

static BARON: HeroKitSpec = HeroKitSpec {
    hero_id: "Baron",
    type_id: 261,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Rocket Launcher",
        on_basic_attack: Some(baron_on_basic_attack),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Baron__A",
        targeting: CastTargeting::GroundPoint,
        max_rank: 5,
        startup: ValueSpec::constant(0.2),
        range: ValueSpec::var_scaled("Range", [8.0; 5], 0.015, 0.0, 0.0, 0.0),
        cooldown: ValueSpec::var("Cooldown", [5.0, 4.5, 4.0, 3.5, 2.5]),
        cost: ValueSpec::var("Energy Cost", [20.0, 30.0, 40.0, 50.0, 60.0]),
        charges: ValueSpec::var("Charges", [2.0; 5]),
        castable_during_movement: true,
        effects: BARON_A_EFFS,
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Baron__B",
        targeting: CastTargeting::GroundPoint,
        max_rank: 5,
        range: ValueSpec::var("Range", [10.0; 5]),
        cooldown: ValueSpec::var("Cooldown", [20.0, 19.0, 18.0, 17.0, 14.0]),
        cost: ValueSpec::var("Energy Cost", [130.0, 140.0, 150.0, 160.0, 170.0]),
        custom_exec: Some(baron_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Baron__C",
        targeting: CastTargeting::GroundPoint,
        max_rank: 3,
        range: ValueSpec::var3("Range", [200.0; 3]),
        cooldown: ValueSpec::var3("Cooldown", [80.0, 70.0, 60.0]),
        cost: ValueSpec::var3("Energy Cost", [100.0, 125.0, 150.0]),
        passive_bonus: Some((
            BuffStatKind::AttackRange,
            ValueSpec::var3("Passive Attack Range Bonus", [0.6, 1.2, 1.8]),
        )),
        effects: BARON_C_EFFS,
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 7. Blackfeather (tid=280)
// ============================================================================

fn bf_on_basic_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    target_idx: usize,
    _base: f32,
    _crit: bool,
) -> (f32, DamageSchool) {
    let dur = 4.0 + ctx.talent_mod("BONUS_DURATION").unwrap_or(0.0);
    let stacks = ctx.add_mark(target_idx, marks::HEARTTHROB, 1, 5, dur);
    ctx.add_resource(10.0);
    (f32::from(stacks) * 12.0, DamageSchool::Crystal)
}

fn bf_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    if let Some(j) = ctx.resolve_target_idx(target) {
        let dest = ctx.room.entities[j].position;
        if ctx.room.spell_motion_clear(ctx.caster_pos(), dest) {
            ctx.room.place_spell_actor(ctx.caster, dest, ctx.events);
        }
        let apply = (rank).clamp(1, 5);
        let dur = 4.0 + ctx.talent_mod("BONUS_DURATION").unwrap_or(0.0);
        let stacks = ctx.add_mark(j, marks::HEARTTHROB, apply, 5, dur);
        let dmg = ctx.eval_var(
            "Ability__Blackfeather__A",
            rank,
            ValueSpec::dmg(0, [60.0, 105.0, 150.0, 195.0, 240.0], 0.5, 1.0, 0.0),
        ) * ctx.talent_dmg_mult(AbilitySlot::A);
        ctx.deal_damage(j, dmg, DamageSchool::Weapon, 0.0, true);
        if stacks >= 5 {
            let pct = if rank >= 5 { 0.25 } else { 0.15 };
            let t = &ctx.room.entities[j];
            let exec = (t.max_hp - t.hp).max(0.0) * pct * ctx.talent_dmg_mult(AbilitySlot::A);
            ctx.deal_damage(j, exec, DamageSchool::Weapon, 0.0, false);
        }
        ctx.add_buff(ctx.caster, BuffStatKind::MoveSpeedFlat, 2.2, 3.0);
        if rank >= 5 && ctx.room.entities[j].hp <= 0.0 {
            ctx.reset_slot_cd(AbilitySlot::A);
        }
    }
    Ok(true)
}

fn bf_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let range = ctx.eval_var(
        "Ability__Blackfeather__B",
        rank,
        ValueSpec::var("Range", [7.0, 7.0, 7.0, 7.0, 9.0]),
    );
    let aim = ctx.resolve_target_pos(target, range);
    let dmg = ctx.eval_var(
        "Ability__Blackfeather__B",
        rank,
        ValueSpec::dmg(0, [80.0, 120.0, 160.0, 200.0, 240.0], 1.8, 0.0, 1.2),
    ) * ctx.talent_dmg_mult(AbilitySlot::B);
    let hits = ctx.enemies_in_radius(aim, 2.8, false);
    let mut hit_hero = false;
    for j in hits {
        if ctx.room.entities[j].kind == Kind::Hero {
            hit_hero = true;
            let dur = 4.0 + ctx.talent_mod("BONUS_DURATION").unwrap_or(0.0);
            ctx.add_mark(j, marks::HEARTTHROB, 1, 5, dur);
        }
        ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
    }
    if hit_hero {
        ctx.add_resource(40.0);
        let bar = (ctx.eval_var(
            "Ability__Blackfeather__B",
            rank,
            ValueSpec::var("BarrierGainOnHitHero", [125.0, 200.0, 275.0, 350.0, 425.0]),
        ) + ctx.room.entities[ctx.caster].bonus_hp * 0.05)
            * ctx.talent_barrier_mult(AbilitySlot::B);
        ctx.grant_barrier(ctx.caster, bar, 2.0, None);
        if ctx.talent_special() == TalentSpecial::BlackfeatherPointGuardAllies {
            let rad = ctx.talent_mod("RADIUS").unwrap_or(9.0);
            for aj in ctx.allies_in_radius(ctx.caster_pos(), rad, false, true) {
                ctx.grant_barrier(aj, bar, 2.0, None);
            }
        }
    }
    Ok(true)
}

static BF_C_EFFS: &[PrimitiveEffect] = &[
    PrimitiveEffect::CleanseImmune {
        cleanse_cc: true,
        cc_immune_seconds: ValueSpec::constant(0.7),
        untargetable_seconds: ValueSpec::zero(),
    },
    PrimitiveEffect::DashBlink {
        range: ValueSpec::constant(6.5),
        duration: ValueSpec::constant(0.5),
        instant_blink: false,
        invuln_during: false,
        unstoppable_during: true,
        path_radius: 1.8,
        path_damage: ValueSpec::dmg3(0, [120.0, 170.0, 220.0], 0.6, 1.25, 0.0),
        arrival_radius: 2.0,
        arrival_damage: ValueSpec::zero(),
        school: DamageKind::Weapon,
        slow_pct: ValueSpec::zero(),
        slow_seconds: ValueSpec::zero(),
        stun_seconds: ValueSpec::zero(),
        knockback_dist: 0.0,
        backward_hop: false,
        applies_mark: None,
    },
];

static BLACKFEATHER: HeroKitSpec = HeroKitSpec {
    hero_id: "Blackfeather",
    type_id: 280,
    resource: ResourceKind::Focus,
    resource_max: 100.0,
    resource_initial: 100.0,
    resource_regen_per_sec: 10.0,
    perk: HeroicPerkSpec {
        id: "Heartthrob",
        on_basic_attack: Some(bf_on_basic_attack),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Blackfeather__A",
        targeting: CastTargeting::TargetEnemy,
        max_rank: 5,
        startup: ValueSpec::constant(0.2),
        range: ValueSpec::constant(6.0),
        cooldown: ValueSpec::var("COOLDOWN", [9.0, 8.5, 8.0, 7.5, 7.0]),
        cost: ValueSpec::var("ALT_ENERGY_COST", [60.0; 5]),
        custom_exec: Some(bf_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Blackfeather__B",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::constant(0.4),
        range: ValueSpec::var("Range", [7.0, 7.0, 7.0, 7.0, 9.0]),
        cooldown: ValueSpec::var("Cooldown", [6.0, 5.5, 5.0, 4.5, 3.5]),
        cost: ValueSpec::var("Alt Energy Cost", [60.0, 60.0, 60.0, 60.0, 40.0]),
        custom_exec: Some(bf_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Blackfeather__C",
        targeting: CastTargeting::Directional,
        max_rank: 3,
        range: ValueSpec::constant(6.5),
        cooldown: ValueSpec::var3("Cooldown", [40.0, 30.0, 20.0]),
        cost: ValueSpec::var3("Alt Energy Cost", [45.0, 30.0, 15.0]),
        charges: ValueSpec::constant(2.0),
        between_charges_cd: ValueSpec::var3("Cooldown2", [0.2; 3]),
        effects: BF_C_EFFS,
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 8. Caine (tid=915)
// ============================================================================

fn caine_ammo_cap(ctx: &HeroKitContext<'_, '_>) -> u32 {
    if ctx.talent_special() == TalentSpecial::CaineDoubleBullets {
        12
    } else {
        6
    }
}

fn caine_on_tick(ctx: &mut HeroKitContext<'_, '_>) {
    let cap = caine_ammo_cap(ctx);
    let st = ctx.room.entities[ctx.caster]
        .hero_kit
        .get_or_insert_with(Box::default);
    if st.counters[0] == 0 && st.timers[0] == 0 {
        st.counters[0] = cap;
        st.timers[0] = 1;
    }
    if st.counters[0] == 0 && ctx.tick >= st.timers[1] {
        st.counters[0] = cap;
    } else if st.counters[0] == 0 {
        ctx.add_buff_ticks(ctx.caster, BuffStatKind::MoveSpeedFlat, 0.5, 3);
    }
}

fn caine_on_basic_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    _target: usize,
    _base: f32,
    _crit: bool,
) -> (f32, DamageSchool) {
    ctx.reduce_slot_cd(AbilitySlot::B, 1.0);
    let cap = caine_ammo_cap(ctx);
    let st = ctx.room.entities[ctx.caster]
        .hero_kit
        .get_or_insert_with(Box::default);
    if st.timers[0] == 0 {
        st.counters[0] = cap;
        st.timers[0] = 1;
    }
    st.counters[0] = st.counters[0].saturating_sub(1);
    if st.counters[0] == 0 {
        st.timers[1] = ctx.tick + seconds_to_ticks(2.0);
    }
    (0.0, DamageSchool::Weapon)
}

fn caine_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let cap = caine_ammo_cap(ctx);
    let bullets = {
        let st = ctx.room.entities[ctx.caster]
            .hero_kit
            .get_or_insert_with(Box::default);
        if st.timers[0] == 0 {
            st.counters[0] = cap;
            st.timers[0] = 1;
        }
        let shots = st.counters[0].min(3);
        st.counters[0] = st.counters[0].saturating_sub(shots);
        if st.counters[0] == 0 {
            st.timers[1] = ctx.tick + seconds_to_ticks(2.0);
        }
        shots.max(1)
    };
    let range = ctx.eval_var(
        "Ability__Caine__A",
        rank,
        ValueSpec::var("RANGE", [8.0, 8.0, 8.0, 8.0, 9.5]),
    ) + ctx.talent_mod("BONUS_RANGE").unwrap_or(0.0);
    let dmg = ctx.eval_var(
        "Ability__Caine__A",
        rank,
        ValueSpec::dmg(0, [35.0, 70.0, 105.0, 140.0, 175.0], 0.6, 1.15, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let slow = ctx.eval_var(
        "Ability__Caine__A",
        rank,
        ValueSpec::var("SLOW_STRENGTH", [0.3, 0.35, 0.4, 0.45, 0.6]),
    );
    let aim = ctx.resolve_target_pos(target, range);
    if let Some(&j) = ctx.enemies_in_radius(aim, 3.0, false).first() {
        ctx.deal_damage(j, dmg * bullets as f32, DamageSchool::Weapon, 0.0, false);
        ctx.apply_slow(j, slow, 2.0);
    }
    Ok(true)
}

fn caine_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    if ctx.talent_special() == TalentSpecial::CaineOnceMoreInvuln {
        ctx.set_untargetable(ctx.caster, 0.4);
    }
    let goal = ctx.resolve_target_pos(target, 3.9);
    if ctx.room.spell_motion_clear(ctx.caster_pos(), goal) {
        ctx.room.place_spell_actor(ctx.caster, goal, ctx.events);
    }
    let cap = caine_ammo_cap(ctx);
    let loaded = ctx.eval_var(
        "Ability__Caine__B",
        rank,
        ValueSpec::var(
            if ctx.talent_special() == TalentSpecial::CaineDoubleBullets {
                "AMMOLOADEDTALENT"
            } else {
                "AMMOLOADED"
            },
            if ctx.talent_special() == TalentSpecial::CaineDoubleBullets {
                [4.0, 6.0, 8.0, 10.0, 12.0]
            } else {
                [2.0, 3.0, 4.0, 5.0, 6.0]
            },
        ),
    ) as u32;
    let st = ctx.room.entities[ctx.caster]
        .hero_kit
        .get_or_insert_with(Box::default);
    st.timers[0] = 1;
    st.counters[0] = (st.counters[0] + loaded).min(cap);
    ctx.reset_slot_cd(AbilitySlot::A);
    if ctx.talent_special() != TalentSpecial::CaineHigherExecuteNoRefresh {
        ctx.reset_slot_cd(AbilitySlot::C);
    }
    Ok(true)
}

fn caine_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let dmg = ctx.eval_var(
        "Ability__Caine__C",
        rank,
        ValueSpec::dmg3(0, [50.0, 75.0, 100.0], 1.0, 1.35, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let blast = ctx.eval_var(
        "Ability__Caine__C",
        rank,
        ValueSpec::dmg3(1, [125.0, 200.0, 275.0], 1.2, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let exec_thr = ctx.eval_var(
        "Ability__Caine__C",
        rank,
        ValueSpec::var3("EXECUTE_THRESHOLD", [0.15, 0.175, 0.20]),
    ) + ctx.talent_mod("EXECUTE_MODIFIER").unwrap_or(0.0);
    let aim = ctx.resolve_target_pos(target, 10.0);
    for j in ctx.enemies_in_radius(aim, 3.0, false) {
        ctx.deal_damage(j, dmg + blast, DamageSchool::Weapon, 0.0, false);
        let t = &ctx.room.entities[j];
        if t.hp > 0.0 && t.max_hp > 0.0 && (t.hp / t.max_hp) <= exec_thr {
            let kill = t.hp + 9999.0;
            ctx.deal_damage(j, kill, DamageSchool::True, 1.0, false);
        }
    }
    Ok(true)
}

static CAINE: HeroKitSpec = HeroKitSpec {
    hero_id: "Caine",
    type_id: 915,
    resource: ResourceKind::None,
    perk: HeroicPerkSpec {
        id: "Peacemaker",
        on_tick: Some(caine_on_tick),
        on_basic_attack: Some(caine_on_basic_attack),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Caine__A",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        range: ValueSpec::var("RANGE", [8.0, 8.0, 8.0, 8.0, 9.5]),
        cooldown: ValueSpec::var("COOLDOWN", [14.0, 13.0, 12.0, 11.0, 10.0]),
        custom_exec: Some(caine_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Caine__B",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        range: ValueSpec::var("RANGE", [3.9; 5]),
        cooldown: ValueSpec::var("COOLDOWN", [18.0, 17.0, 16.0, 15.0, 12.0]),
        custom_exec: Some(caine_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Caine__C",
        targeting: CastTargeting::Directional,
        max_rank: 3,
        range: ValueSpec::var3("RANGE", [10.0; 3]),
        cooldown: ValueSpec::var3("COOLDOWN", [20.0, 18.0, 16.0]),
        custom_exec: Some(caine_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 9. Catherine (tid=242) - bespoke A/B/C + Captain of the Guard perk
// ============================================================================

fn catherine_on_ability_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    _rank: u8,
    _target: CastTarget,
) {
    if matches!(slot, AbilitySlot::A | AbilitySlot::C) {
        let dur = ctx.talent_mod("DURATION").unwrap_or(0.0);
        let wp = ctx.talent_mod("WEAPON_POWER_PER_STACK").unwrap_or(0.0);
        let cp = ctx.talent_mod("CRYSTAL_POWER_PER_STACK").unwrap_or(0.0);
        let st = ctx.room.entities[ctx.caster]
            .hero_kit
            .get_or_insert_with(Box::default);
        st.counters[0] += 1;
        let stacks = st.counters[0] as f32;
        ctx.add_buff(ctx.caster, BuffStatKind::Armor, stacks * 2.0, 3600.0);
        ctx.add_buff(ctx.caster, BuffStatKind::Shield, stacks * 2.0, 3600.0);
        if dur > 0.0 && (wp > 0.0 || cp > 0.0) {
            ctx.add_buff(ctx.caster, BuffStatKind::WeaponPowerPct, wp * stacks, dur);
            ctx.add_buff(ctx.caster, BuffStatKind::CrystalPowerPct, cp * stacks, dur);
        }
    }
}

static CATHERINE: HeroKitSpec = HeroKitSpec {
    hero_id: "Catherine",
    type_id: 242,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Captain of the Guard",
        on_ability_cast: Some(catherine_on_ability_cast),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Catherine__A",
        targeting: CastTargeting::InstantSelf,
        cooldown: ValueSpec::var("Cooldown", [16.0, 15.0, 14.0, 13.0, 12.0]),
        cost: ValueSpec::var("Energy Cost", [30.0, 40.0, 50.0, 60.0, 70.0]),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Catherine__B",
        targeting: CastTargeting::InstantSelf,
        cooldown: ValueSpec::var("Cooldown", [13.0, 12.5, 12.0, 11.5, 11.0]),
        cost: ValueSpec::var("Energy Cost", [40.0, 50.0, 60.0, 70.0, 80.0]),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Catherine__C",
        targeting: CastTargeting::Directional,
        max_rank: 3,
        range: ValueSpec::var3("Range", [11.0; 3]),
        cooldown: ValueSpec::var3("Cooldown", [90.0, 80.0, 70.0]),
        cost: ValueSpec::var3("Energy Cost", [120.0, 140.0, 160.0]),
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 10. Celeste (tid=285)
// ============================================================================

fn celeste_on_basic_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    target_idx: usize,
    _base: f32,
    _crit: bool,
) -> (f32, DamageSchool) {
    ctx.apply_reveal(target_idx, 2.0);
    (0.0, DamageSchool::Crystal)
}

fn celeste_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let range = ctx.eval_var(
        "Ability__Celeste__A",
        rank,
        ValueSpec::var("Range", [7.0, 7.0, 7.0, 7.0, 9.0]),
    );
    let pos = ctx.resolve_target_pos(target, range);
    let owner_id = ctx.caster_id();
    let star_handle = ctx
        .room
        .entities
        .ecs
        .query::<(bevy_ecs::entity::Entity, &crate::hero_kit::KitEffect)>()
        .iter(&ctx.room.entities.ecs)
        .find_map(|(eh, eff)| {
            (eff.owner_id == owner_id
                && matches!(eff.kind, KitEffectKind::GroundZone { zone_kind: 1, .. })
                && eff.position.dist(pos) <= 2.3)
                .then_some((eh, eff.position))
        });

    if let Some((eh, star_pos)) = star_handle {
        ctx.room
            .entities
            .remove_spell_handle::<crate::hero_kit::KitEffect>(eh);
        let nova = ctx.eval_var(
            "Ability__Celeste__A",
            rank,
            ValueSpec::var_scaled(
                "NovaDamage",
                [80.0, 145.0, 210.0, 275.0, 340.0],
                1.75,
                0.0,
                0.0,
                0.0,
            ),
        ) * ctx.talent_mod("NOVA_DAMAGE_MODIFIER").unwrap_or(1.0);
        ctx.spawn_effect(
            KitEffectKind::DelayedBlast {
                ability_slot: 0,
                release_tick: ctx.tick + seconds_to_ticks(0.4),
                radius: 3.5,
                inner_radius: 0.0,
                damage: nova,
                outer_mult: 1.0,
                school: DamageSchool::Crystal,
                stun_ticks: 0,
                root_ticks: 0,
                sleep_ticks: 0,
                sleep_unwaking: false,
                slow_pct: 0.0,
                slow_ticks: 0,
                reveal_ticks: seconds_to_ticks(2.0),
                bonus_mark: None,
                minion_mult: 0.5,
                applies_mark: None,
            },
            star_pos,
            star_pos,
        );
    } else {
        let dmg = ctx.eval_var(
            "Ability__Celeste__A",
            rank,
            ValueSpec::var_scaled(
                "Damage",
                [60.0, 110.0, 160.0, 210.0, 260.0],
                0.8,
                0.0,
                0.0,
                0.0,
            ),
        ) * ctx.talent_dmg_mult(slot);
        let star_dur = 10.0 + ctx.talent_mod("BONUS_DURATION").unwrap_or(0.0);
        let dot_dps = if ctx.talent_special() == TalentSpecial::CelesteYoungStarDoT {
            dmg * ctx.talent_mod("DAMAGE_PER_SECOND").unwrap_or(0.7)
        } else {
            0.0
        };
        ctx.spawn_effect(
            KitEffectKind::DelayedBlast {
                ability_slot: 0,
                release_tick: ctx.tick + seconds_to_ticks(0.6),
                radius: 2.1,
                inner_radius: 0.0,
                damage: dmg,
                outer_mult: 1.0,
                school: DamageSchool::Crystal,
                stun_ticks: 0,
                root_ticks: 0,
                sleep_ticks: 0,
                sleep_unwaking: false,
                slow_pct: 0.0,
                slow_ticks: 0,
                reveal_ticks: seconds_to_ticks(2.0),
                bonus_mark: None,
                minion_mult: 0.5,
                applies_mark: None,
            },
            pos,
            pos,
        );
        ctx.spawn_effect(
            KitEffectKind::GroundZone {
                ability_slot: 0,
                zone_kind: 1,
                radius: 2.1,
                ticks_interval: TICK_HZ / 2,
                next_tick: ctx.tick + TICK_HZ,
                end_tick: ctx.tick + seconds_to_ticks(star_dur),
                tick_damage: dot_dps,
                tick_heal: 0.0,
                school: DamageSchool::Crystal,
                slow_pct: 0.0,
                ally_speed_bonus: 0.0,
                end_damage: 0.0,
                end_root_ticks: 0,
                end_stun_ticks: 0,
                drift_dx: 0.0,
                drift_dy: 0.0,
                follow_caster: false,
                ignited: false,
                minion_mult: 0.5,
                applies_mark: None,
            },
            pos,
            pos,
        );
    }
    Ok(true)
}

static CELESTE_B_EFFS: &[PrimitiveEffect] = &[PrimitiveEffect::DelayedBlast {
    delay: ValueSpec::var("Damage Delay", [0.8; 5]),
    radius: ValueSpec::var("Damage Range", [2.1; 5]),
    inner_radius: 0.0,
    damage: ValueSpec::var_scaled(
        "Damage",
        [100.0, 175.0, 250.0, 325.0, 475.0],
        0.4,
        0.0,
        0.0,
        0.0,
    ),
    outer_mult: 1.0,
    school: DamageKind::Crystal,
    stun_seconds: ValueSpec::var("StunDuration", [1.0, 1.0, 1.0, 1.0, 1.5]),
    root_seconds: ValueSpec::zero(),
    sleep_seconds: ValueSpec::zero(),
    slow_pct: ValueSpec::zero(),
    slow_seconds: ValueSpec::zero(),
    reveal_seconds: 1.5,
    bonus_if_marked: None,
    minion_mult: 1.0,
    applies_mark: None,
}];

static CELESTE_C_EFFS: &[PrimitiveEffect] = &[PrimitiveEffect::SkillshotProjectile {
    damage: ValueSpec::dmg3(0, [300.0, 325.0, 350.0], 0.9, 0.0, 0.0),
    school: DamageKind::Crystal,
    range: ValueSpec::constant(150.0),
    speed: ValueSpec::constant(24.0),
    radius: ValueSpec::constant(2.2),
    piercing: false,
    return_to_caster: false,
    repeat_hit_mult: 1.0,
    splash_damage: Some(ValueSpec::dmg3(1, [150.0, 160.0, 170.0], 0.2, 0.0, 0.0)),
    splash_radius: 2.2,
    stun_seconds: ValueSpec::zero(),
    root_seconds: ValueSpec::zero(),
    silence_seconds: ValueSpec::zero(),
    slow_pct: ValueSpec::zero(),
    slow_seconds: ValueSpec::zero(),
    minion_mult: 0.65,
    resets_cast_cd_on_hit: false,
    applies_mark: None,
    destroys_enemy_projectiles: false,
}];

static CELESTE: HeroKitSpec = HeroKitSpec {
    hero_id: "Celeste",
    type_id: 285,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Julia's Light",
        basic_attack_is_crystal: true,
        on_basic_attack: Some(celeste_on_basic_attack),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Celeste__A",
        targeting: CastTargeting::GroundPoint,
        max_rank: 5,
        range: ValueSpec::var("Range", [7.0, 7.0, 7.0, 7.0, 9.0]),
        cooldown: ValueSpec::var("Cooldown", [2.8, 2.6, 2.4, 2.2, 1.8]),
        cost: ValueSpec::var("Energy Cost", [30.0, 35.0, 40.0, 45.0, 50.0]),
        custom_exec: Some(celeste_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Celeste__B",
        targeting: CastTargeting::GroundPoint,
        max_rank: 5,
        range: ValueSpec::constant(7.5),
        cooldown: ValueSpec::var("Cooldown", [14.0, 13.0, 12.0, 11.0, 9.0]),
        cost: ValueSpec::var("Energy Cost", [100.0, 100.0, 100.0, 100.0, 0.0]),
        effects: CELESTE_B_EFFS,
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Celeste__C",
        targeting: CastTargeting::Directional,
        max_rank: 3,
        range: ValueSpec::constant(150.0),
        cooldown: ValueSpec::var3("Cooldown", [70.0, 65.0, 60.0]),
        cost: ValueSpec::var3("Energy Cost", [150.0, 175.0, 200.0]),
        effects: CELESTE_C_EFFS,
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 11. Churnwalker (tid=408)
// ============================================================================

fn churnwalker_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let range = ctx.eval_var(
        "Ability__Churnwalker__A",
        rank,
        ValueSpec::var("Range", [8.5, 8.5, 8.5, 8.5, 9.5]),
    ) + ctx.talent_mod("BONUS_SHOT_RANGE").unwrap_or(0.0);
    let dmg = ctx.eval_var(
        "Ability__Churnwalker__A",
        rank,
        ValueSpec::dmg(0, [40.0, 80.0, 120.0, 160.0, 200.0], 0.5, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let dps_pct = ctx.eval_var(
        "Ability__Churnwalker__A",
        rank,
        ValueSpec::dmg(1, [0.01, 0.01, 0.01, 0.01, 0.03], 0.0, 0.0, 0.0),
    );
    let aim = ctx.resolve_target_pos(target, range);
    if let Some(&j) = ctx.enemies_in_radius(aim, 2.5, true).first() {
        let t_id = ctx.room.entities[j].id;
        ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
        ctx.add_mark(j, marks::CHURNWALKER_TETHER, 1, 1, 8.0);
        let tether_r = 9.5 + ctx.talent_mod("BONUS_TETHER_RANGE").unwrap_or(0.0);
        let anchor = ctx.caster_pos();
        ctx.spawn_effect(
            KitEffectKind::Tether {
                ability_slot: 0,
                target_id: t_id,
                anchor_pos: anchor,
                break_range: tether_r,
                dps_pct_hp: dps_pct,
                break_damage: 0.0,
                break_stun_ticks: 0,
                anchor_follows_caster: true,
                next_tick: ctx.tick + TICK_HZ / 2,
                end_tick: ctx.tick + seconds_to_ticks(8.0),
            },
            anchor,
            anchor,
        );
        ctx.reset_slot_cd(AbilitySlot::A);
        if ctx.talent_special() == TalentSpecial::ChurnwalkerGrapplePull {
            let pull_pos = ctx.room.entities[j].position;
            if ctx.room.spell_motion_clear(ctx.caster_pos(), pull_pos) {
                ctx.room.place_spell_actor(ctx.caster, pull_pos, ctx.events);
            }
        }
    }
    Ok(true)
}

fn churnwalker_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    rank: u8,
    _target: CastTarget,
) -> Result<bool, &'static str> {
    let owner_id = ctx.caster_id();
    let dmg = ctx.eval_var(
        "Ability__Churnwalker__B",
        rank,
        ValueSpec::dmg(0, [80.0, 140.0, 200.0, 260.0, 380.0], 0.8, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(AbilitySlot::B);
    let pull = ctx.eval_var(
        "Ability__Churnwalker__B",
        rank,
        ValueSpec::var("PullInDistance", [4.0, 4.0, 4.0, 4.0, 5.0]),
    );
    let origin = ctx.caster_pos();
    let targets: Vec<usize> = ctx
        .room
        .entities
        .iter()
        .enumerate()
        .filter_map(|(j, e)| {
            (e.hp > 0.0
                && e.hero_kit
                    .as_deref()
                    .and_then(|k| k.marks.get(&marks::CHURNWALKER_TETHER))
                    .is_some_and(|m| ctx.tick < m.until && m.source_id == owner_id))
            .then_some(j)
        })
        .collect();
    for j in targets {
        ctx.pull_toward(j, origin, pull);
        ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
    }
    if ctx.talent_special() == TalentSpecial::ChurnwalkerTormentCloud {
        let dps = ctx.talent_mod("CLOUD_DAMAGE_PER_SECOND").unwrap_or(50.0);
        let dur = ctx.talent_mod("CLOUD_DURATION").unwrap_or(3.0);
        let rad = ctx.talent_mod("CLOUD_RADIUS").unwrap_or(4.0);
        ctx.spawn_effect(
            KitEffectKind::GroundZone {
                ability_slot: 1,
                zone_kind: 6,
                radius: rad,
                ticks_interval: TICK_HZ / 2,
                next_tick: ctx.tick,
                end_tick: ctx.tick + seconds_to_ticks(dur),
                tick_damage: dps,
                tick_heal: 0.0,
                school: DamageSchool::Crystal,
                slow_pct: 0.0,
                ally_speed_bonus: 0.0,
                end_damage: 0.0,
                end_root_ticks: 0,
                end_stun_ticks: 0,
                drift_dx: 0.0,
                drift_dy: 0.0,
                follow_caster: true,
                ignited: false,
                minion_mult: 0.5,
                applies_mark: None,
            },
            origin,
            origin,
        );
    }
    Ok(true)
}

fn churnwalker_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let owner_id = ctx.caster_id();
    let goal = ctx.resolve_target_pos(target, 12.0);
    if ctx.room.spell_motion_clear(ctx.caster_pos(), goal) {
        ctx.room.place_spell_actor(ctx.caster, goal, ctx.events);
    }
    let dmg = ctx.eval_var(
        "Ability__Churnwalker__C",
        rank,
        ValueSpec::dmg3(0, [300.0, 400.0, 500.0], 1.0, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(AbilitySlot::C);
    let stun = ctx.eval_var(
        "Ability__Churnwalker__C",
        rank,
        ValueSpec::var3("Stun Duration", [1.2, 1.4, 1.6]),
    );
    let targets: Vec<usize> = ctx
        .room
        .entities
        .iter()
        .enumerate()
        .filter_map(|(j, e)| {
            (e.hp > 0.0
                && e.hero_kit
                    .as_deref()
                    .and_then(|k| k.marks.get(&marks::CHURNWALKER_TETHER))
                    .is_some_and(|m| ctx.tick < m.until && m.source_id == owner_id))
            .then_some(j)
        })
        .collect();
    for j in targets {
        ctx.consume_mark(j, marks::CHURNWALKER_TETHER);
        ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
        ctx.apply_stun(j, stun);
    }
    Ok(true)
}

static CHURNWALKER: HeroKitSpec = HeroKitSpec {
    hero_id: "Churnwalker",
    type_id: 408,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Futility of Life",
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Churnwalker__A",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::constant(0.4),
        range: ValueSpec::var("Range", [8.5, 8.5, 8.5, 8.5, 9.5]),
        cooldown: ValueSpec::var("Cooldown", [5.0, 4.5, 4.0, 3.5, 3.0]),
        cost: ValueSpec::var("Energy Cost", [20.0; 5]),
        custom_exec: Some(churnwalker_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Churnwalker__B",
        targeting: CastTargeting::InstantSelf,
        max_rank: 5,
        startup: ValueSpec::constant(0.4),
        cooldown: ValueSpec::var("Cooldown", [8.0; 5]),
        cost: ValueSpec::var("Energy Cost", [75.0; 5]),
        gate: CastGate::RequiresTetheredTarget,
        custom_exec: Some(churnwalker_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Churnwalker__C",
        targeting: CastTargeting::GroundPoint,
        max_rank: 3,
        startup: ValueSpec::constant(0.7),
        range: ValueSpec::constant(12.0),
        cooldown: ValueSpec::var3("Cooldown", [80.0, 70.0, 60.0]),
        cost: ValueSpec::var3("Energy Cost", [75.0; 3]),
        gate: CastGate::RequiresTetheredTarget,
        custom_exec: Some(churnwalker_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 12. Flicker (tid=393)
// ============================================================================

fn flicker_on_tick(ctx: &mut HeroKitContext<'_, '_>) {
    let stationary = ctx.room.entities[ctx.caster]
        .hero_kit
        .as_deref()
        .map_or(0, |k| k.stationary_ticks);
    if u64::from(stationary) >= TICK_HZ {
        ctx.set_stealth(ctx.caster, 1.5, 1.2, 0.2);
    }
}

static FLICKER_A_EFFS: &[PrimitiveEffect] = &[PrimitiveEffect::GroundAoE {
    radius: ValueSpec::var("FieldRadius", [2.0; 5]),
    duration: ValueSpec::var("FIELD_DURATION", [2.0; 5]),
    ticks_per_sec: 2.0,
    dps: ValueSpec::dmg(0, [30.0, 60.0, 90.0, 120.0, 150.0], 0.25, 0.0, 0.0),
    hps: ValueSpec::zero(),
    school: DamageKind::Crystal,
    slow_pct: ValueSpec::zero(),
    end_damage: None,
    end_root_seconds: ValueSpec::var("RootDuration", [1.0, 1.0, 1.0, 1.0, 1.5]),
    end_stun_seconds: ValueSpec::zero(),
    drifts_toward_aim: false,
    minion_mult: 0.5,
    applies_mark: None,
    zone_kind: 4,
}];

static FLICKER_B_EFFS: &[PrimitiveEffect] = &[PrimitiveEffect::GroundAoE {
    radius: ValueSpec::var("Range", [4.0; 5]),
    duration: ValueSpec::var("DUST_DURATION", [3.0; 5]),
    ticks_per_sec: 2.0,
    dps: ValueSpec::dmg(0, [20.0, 40.0, 60.0, 80.0, 120.0], 0.5, 0.0, 0.0),
    hps: ValueSpec::zero(),
    school: DamageKind::Crystal,
    slow_pct: ValueSpec::var("SlowStrength", [0.4, 0.45, 0.5, 0.55, 0.65]),
    end_damage: Some(ValueSpec::dmg(
        1,
        [80.0, 140.0, 200.0, 260.0, 380.0],
        1.0,
        0.0,
        0.0,
    )),
    end_root_seconds: ValueSpec::zero(),
    end_stun_seconds: ValueSpec::zero(),
    drifts_toward_aim: false,
    minion_mult: 0.5,
    applies_mark: None,
    zone_kind: 5,
}];

static FLICKER_C_EFFS: &[PrimitiveEffect] = &[PrimitiveEffect::StealthMode {
    pre_delay: ValueSpec::var3("PRE_CAST_DELAY", [1.0; 3]),
    duration: ValueSpec::var3("StealthDuration", [8.0, 10.0, 12.0]),
    speed_bonus: ValueSpec::var3("SpeedBoostStrength", [1.5; 3]),
    affect_allies: true,
    break_on_damage: true,
    break_on_attack: true,
    re_stealth_delay: 1.0,
}];

static FLICKER: HeroKitSpec = HeroKitSpec {
    hero_id: "Flicker",
    type_id: 393,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Willow Whisper",
        on_tick: Some(flicker_on_tick),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Flicker__A",
        targeting: CastTargeting::GroundPoint,
        max_rank: 5,
        startup: ValueSpec::constant(0.3),
        range: ValueSpec::var("Range", [8.0; 5]),
        cooldown: ValueSpec::var("Cooldown", [6.0, 5.5, 5.0, 4.5, 4.0]),
        cost: ValueSpec::var("Energy Cost", [30.0, 40.0, 50.0, 60.0, 70.0]),
        effects: FLICKER_A_EFFS,
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Flicker__B",
        targeting: CastTargeting::InstantSelf,
        max_rank: 5,
        startup: ValueSpec::constant(0.5),
        range: ValueSpec::var("Range", [4.0; 5]),
        cooldown: ValueSpec::var("Cooldown", [12.0, 11.0, 10.0, 9.0, 7.0]),
        cost: ValueSpec::var("Energy Cost", [60.0, 70.0, 80.0, 90.0, 100.0]),
        effects: FLICKER_B_EFFS,
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Flicker__C",
        targeting: CastTargeting::InstantSelf,
        max_rank: 3,
        startup: ValueSpec::var3("PRE_CAST_DELAY", [1.0; 3]),
        cooldown: ValueSpec::var3("Cooldown", [100.0, 80.0, 60.0]),
        cost: ValueSpec::var3("Energy Cost", [100.0; 3]),
        effects: FLICKER_C_EFFS,
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 13. Fortress (tid=259)
// ============================================================================

fn fortress_on_tick(ctx: &mut HeroKitContext<'_, '_>) {
    let allies = ctx.allies_in_radius(ctx.caster_pos(), 8.0, false, true);
    if !allies.is_empty() {
        ctx.add_buff_ticks(ctx.caster, BuffStatKind::MoveSpeedFlat, 1.2, 4);
    }
}

fn fortress_on_basic_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    target_idx: usize,
    _base: f32,
    _crit: bool,
) -> (f32, DamageSchool) {
    if ctx.mark_stacks(target_idx, marks::LAW_OF_THE_CLAW) > 0 {
        let stacks = ctx.add_mark(target_idx, marks::FORTRESS_MARK, 1, 6, 5.0);
        if stacks >= 6 {
            ctx.consume_mark(target_idx, marks::FORTRESS_MARK);
            ctx.consume_mark(target_idx, marks::LAW_OF_THE_CLAW);
            let max_hp = ctx.room.entities[target_idx].max_hp;
            let r = ctx.room.entities[ctx.caster].ability_ranks[1].max(1);
            let burst = max_hp * if r >= 5 { 0.20 } else { 0.10 };
            ctx.apply_mortal_wound(target_idx, 5.0);
            if r >= 5 {
                ctx.apply_slow(target_idx, 0.4, 2.5);
            }
            return (burst, DamageSchool::Crystal);
        }
    }
    (0.0, DamageSchool::Weapon)
}

static FORTRESS_A_EFFS: &[PrimitiveEffect] = &[PrimitiveEffect::TargetedStrike {
    damage: ValueSpec::dmg(0, [30.0, 60.0, 90.0, 120.0, 180.0], 1.1, 1.0, 0.0),
    school: DamageKind::Crystal,
    bonus_if_marked: None,
    triggers_basic_effects: true,
    resets_attack_cd: false,
    stun_seconds: ValueSpec::zero(),
    slow_pct: ValueSpec::zero(),
    slow_seconds: ValueSpec::zero(),
}];

static FORTRESS_B_EFFS: &[PrimitiveEffect] = &[
    PrimitiveEffect::TargetedStrike {
        damage: ValueSpec::dmg(0, [70.0, 100.0, 130.0, 160.0, 190.0], 0.7, 0.0, 0.0),
        school: DamageKind::Crystal,
        bonus_if_marked: None,
        triggers_basic_effects: false,
        resets_attack_cd: false,
        stun_seconds: ValueSpec::zero(),
        slow_pct: ValueSpec::var("SLOW_MAGNITUDE", [0.3; 5]),
        slow_seconds: ValueSpec::var("SLOW_DURATION", [2.5; 5]),
    },
    PrimitiveEffect::MarkStacks {
        mark: MarkKind::Custom(marks::LAW_OF_THE_CLAW),
        on_self: false,
        add_stacks: 1,
        max_stacks: 1,
        duration: 5.0,
        consume_all: false,
        damage_per_stack: ValueSpec::zero(),
        heal_per_stack: ValueSpec::zero(),
        school: DamageKind::Crystal,
        mortal_wound_seconds: 0.0,
    },
];

static FORTRESS_C_EFFS: &[PrimitiveEffect] = &[
    PrimitiveEffect::AllyHealBarrier {
        radius: ValueSpec::constant(1.0),
        include_self: true,
        target_only: false,
        burst_heal: ValueSpec::zero(),
        hot_per_sec: ValueSpec::zero(),
        hot_duration: ValueSpec::zero(),
        barrier: ValueSpec::zero(),
        barrier_duration: ValueSpec::zero(),
        fortified: ValueSpec::var3("FortifiedHealthBonus", [300.0, 425.0, 550.0]),
        fortified_duration: ValueSpec::var3("Duration", [10.0, 12.0, 14.0]),
        self_cast_mult: 1.0,
        grants_attack_buff_stacks: 0,
        grants_attack_bonus_dmg: ValueSpec::zero(),
    },
    PrimitiveEffect::SelfBuff {
        stat: BuffStatKind::AttackSpeed,
        amount: ValueSpec::var3("AttackSpeedBonus", [0.3, 0.45, 0.6]),
        duration: ValueSpec::var3("Duration", [10.0, 12.0, 14.0]),
        empowered_attacks: 0,
        empowered_bonus_damage: ValueSpec::zero(),
        empowered_stun_seconds: 0.0,
        empowered_slow_pct: 0.0,
        empowered_slow_seconds: 0.0,
        empowered_knockback: 0.0,
        resets_attack_cd: false,
    },
    PrimitiveEffect::SummonPet {
        count: 3,
        max_active: 3,
        duration: ValueSpec::var3("Duration", [10.0, 12.0, 14.0]),
        pet_hp: ValueSpec::var3("Wolf Base Health", [300.0, 450.0, 600.0]),
        pet_damage: ValueSpec::var3("WolfDamage", [20.0, 40.0, 60.0]),
        school: DamageKind::Weapon,
        hit_ticks: 3,
        applies_mark: Some((MarkKind::Custom(marks::BLEED), 1, 3.0)),
        replace_existing: true,
        pet_kind: 1,
    },
];

static FORTRESS: HeroKitSpec = HeroKitSpec {
    hero_id: "Fortress",
    type_id: 259,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Packmates",
        on_tick: Some(fortress_on_tick),
        on_basic_attack: Some(fortress_on_basic_attack),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Fortress__A",
        targeting: CastTargeting::TargetEnemy,
        max_rank: 5,
        range: ValueSpec::var("RANGE", [7.0, 7.0, 7.0, 7.0, 9.0]),
        cooldown: ValueSpec::var("Cooldown", [10.0, 9.5, 9.0, 8.5, 8.0]),
        cost: ValueSpec::var("Energy Cost", [40.0, 45.0, 50.0, 55.0, 60.0]),
        effects: FORTRESS_A_EFFS,
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Fortress__B",
        targeting: CastTargeting::TargetEnemy,
        max_rank: 5,
        range: ValueSpec::constant(3.2),
        cooldown: ValueSpec::var("Cooldown", [10.0, 9.5, 9.0, 8.5, 8.0]),
        cost: ValueSpec::var("Energy Cost", [50.0, 55.0, 60.0, 65.0, 70.0]),
        effects: FORTRESS_B_EFFS,
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Fortress__C",
        targeting: CastTargeting::InstantSelf,
        max_rank: 3,
        cooldown: ValueSpec::var3("Cooldown", [120.0, 110.0, 100.0]),
        cost: ValueSpec::var3("Energy Cost", [120.0, 140.0, 160.0]),
        effects: FORTRESS_C_EFFS,
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 14. Glaive (tid=249)
// ============================================================================

fn glaive_on_basic_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    target_idx: usize,
    base: f32,
    crit: bool,
) -> (f32, DamageSchool) {
    let stacks = ctx.add_mark(ctx.caster, marks::BLOODSONG, 1, 20, 12.0);
    ctx.add_buff(
        ctx.caster,
        BuffStatKind::Lifesteal,
        0.012 * f32::from(stacks),
        12.0,
    );
    if crit {
        let cleave_frac = 0.5;
        let center = ctx.room.entities[target_idx].position;
        for j in ctx.enemies_in_radius(center, 3.0, false) {
            if j != target_idx {
                let m = if ctx.room.entities[j].kind == Kind::Hero {
                    1.0
                } else {
                    0.5
                };
                ctx.deal_damage(j, base * cleave_frac * m, DamageSchool::Weapon, 0.0, false);
            }
        }
    }
    (0.0, DamageSchool::Weapon)
}

fn glaive_on_take_damage(
    ctx: &mut HeroKitContext<'_, '_>,
    _src: Option<usize>,
    _dmg: f32,
    _school: DamageSchool,
) -> f32 {
    ctx.add_mark(ctx.caster, marks::BLOODSONG, 1, 20, 12.0);
    1.0
}

static GLAIVE_A_EFFS: &[PrimitiveEffect] = &[
    PrimitiveEffect::DashBlink {
        range: ValueSpec::var("Range", [8.0; 5]),
        duration: ValueSpec::constant(0.25),
        instant_blink: false,
        invuln_during: false,
        unstoppable_during: false,
        path_radius: 1.8,
        path_damage: ValueSpec::dmg(0, [100.0, 150.0, 200.0, 250.0, 300.0], 1.3, 0.0, 0.0),
        arrival_radius: 2.0,
        arrival_damage: ValueSpec::zero(),
        school: DamageKind::Crystal,
        slow_pct: ValueSpec::zero(),
        slow_seconds: ValueSpec::zero(),
        stun_seconds: ValueSpec::zero(),
        knockback_dist: 0.0,
        backward_hop: false,
        applies_mark: None,
    },
    PrimitiveEffect::SelfBuff {
        stat: BuffStatKind::MoveSpeedPct,
        amount: ValueSpec::var("SpeedBoostStrength", [0.55; 5]),
        duration: ValueSpec::var("AfterburnBuffDuration", [5.0; 5]),
        empowered_attacks: 1,
        empowered_bonus_damage: ValueSpec::zero(),
        empowered_stun_seconds: 0.8,
        empowered_slow_pct: 0.0,
        empowered_slow_seconds: 0.0,
        empowered_knockback: 5.5,
        resets_attack_cd: false,
    },
];

fn glaive_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    if ctx.talent_special() == TalentSpecial::GlaiveTwistedDash {
        let dest = ctx.resolve_target_pos(target, 8.0);
        if ctx.room.spell_motion_clear(ctx.caster_pos(), dest) {
            ctx.room.place_spell_actor(ctx.caster, dest, ctx.events);
        }
    }
    let dmg = (ctx.eval_var(
        "Ability__Glaive__B",
        rank,
        ValueSpec::dmg(0, [10.0, 20.0, 30.0, 40.0, 50.0], 1.2, 1.0, 0.0),
    ) * 1.5)
        * ctx.talent_dmg_mult(slot);
    if let Some(j) = ctx.resolve_target_idx(target) {
        ctx.deal_damage(j, dmg, DamageSchool::Weapon, 0.0, true);
    }
    Ok(true)
}

fn glaive_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    _target: CastTarget,
) -> Result<bool, &'static str> {
    let stacks = ctx.consume_mark(ctx.caster, marks::BLOODSONG);
    let base = ctx.eval_var(
        "Ability__Glaive__C",
        rank,
        ValueSpec::var3("Damage", [100.0, 200.0, 300.0]),
    );
    let per_stack = ctx.eval_var(
        "Ability__Glaive__C",
        rank,
        ValueSpec::var3("DamagePerStack", [15.0, 20.0, 25.0]),
    );
    let total = (base + per_stack * f32::from(stacks)) * ctx.talent_dmg_mult(slot);
    let stun = if ctx.talent_special() == TalentSpecial::GlaiveBloodsongStun {
        ctx.talent_mod("STUN_DURATION").unwrap_or(0.75)
    } else {
        0.0
    };
    for j in ctx.enemies_in_radius(ctx.caster_pos(), 5.4, false) {
        ctx.deal_damage(j, total, DamageSchool::Crystal, 0.0, false);
        if stun > 0.0 {
            ctx.apply_stun(j, stun);
        }
    }
    Ok(true)
}

static GLAIVE: HeroKitSpec = HeroKitSpec {
    hero_id: "Glaive",
    type_id: 249,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Hunt The Weak",
        on_basic_attack: Some(glaive_on_basic_attack),
        on_take_damage: Some(glaive_on_take_damage),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Glaive__A",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        range: ValueSpec::var("Range", [8.0; 5]),
        cooldown: ValueSpec::var("Cooldown", [24.0, 22.0, 20.0, 18.0, 14.0]),
        cost: ValueSpec::var("Energy Cost", [110.0, 120.0, 130.0, 140.0, 150.0]),
        effects: GLAIVE_A_EFFS,
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Glaive__B",
        targeting: CastTargeting::TargetEnemy,
        max_rank: 5,
        range: ValueSpec::var("Range", [3.0; 5]),
        cooldown: ValueSpec::var("Cooldown", [10.0, 9.0, 8.0, 7.0, 6.0]),
        cost: ValueSpec::var("Energy Cost", [20.0, 25.0, 30.0, 35.0, 40.0]),
        passive_bonus: Some((
            BuffStatKind::CritChance,
            ValueSpec::var("BonusCritRate", [0.1, 0.15, 0.2, 0.25, 0.3]),
        )),
        custom_exec: Some(glaive_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Glaive__C",
        targeting: CastTargeting::InstantSelf,
        max_rank: 3,
        range: ValueSpec::constant(5.4),
        cooldown: ValueSpec::var3("Cooldown", [16.0; 3]),
        cost: ValueSpec::var3("Energy Cost", [20.0, 25.0, 30.0]),
        custom_exec: Some(glaive_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 15. Grace (tid=406)
// ============================================================================

fn grace_on_basic_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    target_idx: usize,
    _base: f32,
    _crit: bool,
) -> (f32, DamageSchool) {
    let slam = {
        let st = ctx.room.entities[ctx.caster]
            .hero_kit
            .get_or_insert_with(Box::default);
        st.counters[0] = (st.counters[0] + 1) % 2;
        st.counters[0] == 0
    };
    if slam {
        let bonus = ctx.room.entities[ctx.caster].weapon_power() * 0.3;
        let center = ctx.room.entities[target_idx].position;
        for j in ctx.enemies_in_radius(center, 2.8, false) {
            ctx.apply_slow(j, 0.35, 1.2);
        }
        ctx.reduce_slot_cd(AbilitySlot::A, 1.0);
        ctx.reduce_slot_cd(AbilitySlot::B, 1.0);
        ctx.reduce_slot_cd(AbilitySlot::C, 1.0);
        return (bonus, DamageSchool::Weapon);
    }
    (0.0, DamageSchool::Weapon)
}

fn grace_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    if let Some(j) = ctx.resolve_target_idx(target) {
        let dest = ctx.room.entities[j].position;
        if ctx.room.spell_motion_clear(ctx.caster_pos(), dest) {
            ctx.room.place_spell_actor(ctx.caster, dest, ctx.events);
        }
        let dmg = ctx.eval_var(
            "Ability__Grace__A",
            rank,
            ValueSpec::var_scaled(
                "BONUS_DAMAGE",
                [20.0, 40.0, 60.0, 80.0, 120.0],
                1.0,
                0.15,
                0.0,
                0.0,
            ),
        ) * ctx.talent_dmg_mult(slot);
        ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
    }
    let dr = 0.4;
    ctx.add_buff(ctx.caster, BuffStatKind::DamageReduction, dr, 3.0);
    for aj in ctx.allies_in_radius(ctx.caster_pos(), 12.0, false, true) {
        ctx.add_buff(aj, BuffStatKind::DamageReduction, dr * 0.65, 2.0);
    }
    Ok(true)
}

fn grace_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let ally = ctx.resolve_target_idx(target).unwrap_or(ctx.caster);
    ctx.add_buff(ctx.caster, BuffStatKind::DamageReduction, 0.4, 1.8);
    ctx.add_buff(ally, BuffStatKind::DamageReduction, 0.4, 1.8);
    let heal = ctx.eval_var(
        "Ability__Grace__C",
        rank,
        ValueSpec::var3_scaled("HEAL_STRENGTH", [400.0, 700.0, 1000.0], 2.0),
    ) * ctx.talent_heal_mult(slot);
    ctx.heal_unit(ally, heal);
    if ctx.talent_special() == TalentSpecial::GraceUnitedFrontSelfHeal && ally != ctx.caster {
        ctx.heal_unit(ctx.caster, heal * 0.75);
    }
    if ctx.room.entities[ctx.caster].ability_ranks[0] >= 5 {
        ctx.reset_slot_cd(AbilitySlot::A);
    }
    Ok(true)
}

static GRACE: HeroKitSpec = HeroKitSpec {
    hero_id: "Grace",
    type_id: 406,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Retribution",
        on_basic_attack: Some(grace_on_basic_attack),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Grace__A",
        targeting: CastTargeting::TargetEnemy,
        max_rank: 5,
        range: ValueSpec::var("Range", [7.0, 7.0, 7.0, 7.0, 9.0]),
        cooldown: ValueSpec::var("Cooldown", [12.0, 12.0, 12.0, 12.0, 10.0]),
        cost: ValueSpec::var("Energy Cost", [60.0; 5]),
        passive_bonus: Some((
            BuffStatKind::DamageReduction,
            ValueSpec::var("PASSIVE_DR", [0.08; 5]),
        )),
        charges: ValueSpec::constant(1.0),
        custom_exec: Some(grace_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Grace__B",
        targeting: CastTargeting::InstantSelf,
        max_rank: 5,
        startup: ValueSpec::constant(0.6),
        range: ValueSpec::constant(3.5),
        cooldown: ValueSpec::var("Cooldown", [10.0, 10.0, 10.0, 10.0, 8.0]),
        cost: ValueSpec::var("Energy Cost", [80.0, 90.0, 100.0, 110.0, 120.0]),
        effects: &[PrimitiveEffect::DelayedBlast {
            delay: ValueSpec::zero(),
            radius: ValueSpec::var("IMPACT_RADIUS", [3.5; 5]),
            inner_radius: 0.0,
            damage: ValueSpec::dmg(0, [210.0, 290.0, 370.0, 450.0, 530.0], 2.0, 0.0, 0.0),
            outer_mult: 1.0,
            school: DamageKind::Crystal,
            stun_seconds: ValueSpec::var("STUN_DURATION", [1.0; 5]),
            root_seconds: ValueSpec::zero(),
            sleep_seconds: ValueSpec::zero(),
            slow_pct: ValueSpec::zero(),
            slow_seconds: ValueSpec::zero(),
            reveal_seconds: 0.0,
            bonus_if_marked: None,
            minion_mult: 0.5,
            applies_mark: None,
        }],
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Grace__C",
        targeting: CastTargeting::TargetAllyOrSelf,
        max_rank: 3,
        startup: ValueSpec::var3("CHANNEL_DURATION", [1.8; 3]),
        range: ValueSpec::constant(9.0),
        cooldown: ValueSpec::var3("Cooldown", [80.0, 70.0, 60.0]),
        custom_exec: Some(grace_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 16. Grumpjaw (tid=396)
// ============================================================================

fn grumpjaw_on_tick(ctx: &mut HeroKitContext<'_, '_>) {
    let last_gain = ctx.room.entities[ctx.caster]
        .hero_kit
        .as_deref()
        .map_or(0, |k| k.timers[0]);
    if ctx.tick >= last_gain {
        if let Some(k) = ctx.room.entities[ctx.caster].hero_kit.as_deref_mut() {
            k.timers[0] = ctx.tick + TICK_HZ * 3;
        }
        ctx.add_mark(ctx.caster, marks::LIVING_ARMOR, 1, 5, 10.0);
    }
}

fn grumpjaw_on_basic_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    _target: usize,
    _base: f32,
    _crit: bool,
) -> (f32, DamageSchool) {
    ctx.add_mark(ctx.caster, marks::LIVING_ARMOR, 1, 5, 10.0);
    (0.0, DamageSchool::Weapon)
}

fn grumpjaw_on_take_damage(
    ctx: &mut HeroKitContext<'_, '_>,
    _src: Option<usize>,
    _dmg: f32,
    _school: DamageSchool,
) -> f32 {
    let stacks = ctx.mark_stacks(ctx.caster, marks::LIVING_ARMOR);
    (1.0 - 0.06 * f32::from(stacks)).max(0.6)
}

fn grumpjaw_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let stacks = ctx.mark_stacks(ctx.caster, marks::LIVING_ARMOR);
    let goal = ctx.resolve_target_pos(target, 8.0);
    if ctx.room.spell_motion_clear(ctx.caster_pos(), goal) {
        ctx.room.place_spell_actor(ctx.caster, goal, ctx.events);
    }
    let dmg = (ctx.eval_var(
        "Ability__Grumpjaw__A",
        rank,
        ValueSpec::var_scaled(
            "Damage",
            [75.0, 110.0, 145.0, 180.0, 250.0],
            0.75,
            0.0,
            0.0,
            0.0,
        ),
    ) + ctx.eval_var(
        "Ability__Grumpjaw__A",
        rank,
        ValueSpec::var_scaled(
            "Bonus Damage Per Perk Stack",
            [10.0, 15.0, 20.0, 25.0, 35.0],
            0.4,
            0.0,
            0.0,
            0.0,
        ),
    ) * f32::from(stacks))
        * ctx.talent_dmg_mult(slot);
    let slow_dur = ctx.eval_var(
        "Ability__Grumpjaw__A",
        rank,
        ValueSpec::var("Victim Slow Duration", [1.0, 1.3, 1.6, 1.9, 2.5]),
    );
    for j in ctx.enemies_in_radius(ctx.caster_pos(), 3.5, false) {
        ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
        ctx.apply_slow(j, 0.65, slow_dur);
    }
    Ok(true)
}

fn grumpjaw_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    if let Some(j) = ctx.resolve_target_idx(target) {
        let dest = ctx.room.entities[j].position;
        if ctx.room.spell_motion_clear(ctx.caster_pos(), dest) {
            ctx.room.place_spell_actor(ctx.caster, dest, ctx.events);
        }
    }
    let dur = ctx.eval_var(
        "Ability__Grumpjaw__B",
        rank,
        ValueSpec::var("Buff Duration", [5.0, 5.0, 5.0, 5.0, 6.0]),
    );
    let aspd = ctx.eval_var(
        "Ability__Grumpjaw__B",
        rank,
        ValueSpec::var("Attack Speed Bonus", [0.25, 0.3, 0.35, 0.4, 0.45]),
    );
    ctx.add_buff(ctx.caster, BuffStatKind::AttackSpeed, aspd, dur);
    if ctx.talent_special() == TalentSpecial::GrumpjawHangryLifesteal {
        let ls = ctx.talent_mod("BONUS_LIFESTEAL").unwrap_or(0.25);
        ctx.add_buff(ctx.caster, BuffStatKind::Lifesteal, ls, dur);
    }
    let bonus = ctx.eval_var(
        "Ability__Grumpjaw__B",
        rank,
        ValueSpec::dmg(0, [40.0, 60.0, 80.0, 100.0, 120.0], 0.9, 1.0, 0.0),
    );
    ctx.empower_next_attacks(5, bonus, true, 0.0, 0.0, 0.0, 0.0, dur);
    Ok(true)
}

fn grumpjaw_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let tj = ctx.resolve_target_idx(target).or_else(|| {
        ctx.enemies_in_radius(ctx.resolve_target_pos(target, 4.0), 2.5, true)
            .into_iter()
            .next()
    });
    let Some(j) = tj else {
        return Ok(true);
    };
    let host_id = ctx.room.entities[j].id;
    let dmg = ctx.eval_var(
        "Ability__Grumpjaw__C",
        rank,
        ValueSpec::dmg3(0, [400.0, 550.0, 700.0], 1.5, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(AbilitySlot::C);
    let dur = 3.0 + ctx.talent_mod("BONUS_DURATION").unwrap_or(0.0);
    ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
    ctx.spawn_effect(
        KitEffectKind::Attachment {
            ability_slot: 2,
            host_id,
            is_devour: true,
            dps: 0.0,
            end_damage: 0.0,
            end_radius: 1.5,
            school: DamageSchool::Crystal,
            next_tick: ctx.tick + TICK_HZ,
            end_tick: ctx.tick + seconds_to_ticks(dur),
        },
        ctx.caster_pos(),
        ctx.caster_pos(),
    );
    ctx.open_recast(AbilitySlot::C, 1, host_id, dur);
    Ok(true)
}

fn grumpjaw_c_spit(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    _rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let tid = ctx.room.entities[ctx.caster]
        .hero_kit
        .as_deref()
        .map_or(0, |k| k.recast_target[2]);
    ctx.clear_recast(AbilitySlot::C);
    let owner_id = ctx.caster_id();
    let to_rm: Vec<_> = ctx
        .room
        .entities
        .ecs
        .query::<(bevy_ecs::entity::Entity, &crate::hero_kit::KitEffect)>()
        .iter(&ctx.room.entities.ecs)
        .filter_map(|(eh, eff)| {
            (eff.owner_id == owner_id
                && matches!(
                    eff.kind,
                    KitEffectKind::Attachment {
                        is_devour: true,
                        ..
                    }
                ))
            .then_some(eh)
        })
        .collect();
    for eh in to_rm {
        ctx.room
            .entities
            .remove_spell_handle::<crate::hero_kit::KitEffect>(eh);
    }
    if let Some(j) = ctx.room.index(tid) {
        let dest = ctx.resolve_target_pos(target, 4.0);
        if ctx
            .room
            .spell_motion_clear(ctx.room.entities[j].position, dest)
        {
            ctx.room.place_spell_actor(j, dest, ctx.events);
        }
    }
    Ok(true)
}

static GRUMPJAW: HeroKitSpec = HeroKitSpec {
    hero_id: "Grumpjaw",
    type_id: 396,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Living Armor",
        on_tick: Some(grumpjaw_on_tick),
        on_basic_attack: Some(grumpjaw_on_basic_attack),
        on_take_damage: Some(grumpjaw_on_take_damage),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Grumpjaw__A",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::constant(0.5),
        range: ValueSpec::var("Range", [8.0; 5]),
        cooldown: ValueSpec::var("Cooldown", [12.0, 11.0, 10.0, 9.0, 8.0]),
        cost: ValueSpec::var("Energy Cost", [50.0, 60.0, 70.0, 80.0, 90.0]),
        custom_exec: Some(grumpjaw_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Grumpjaw__B",
        targeting: CastTargeting::TargetEnemy,
        max_rank: 5,
        range: ValueSpec::constant(5.0),
        cooldown: ValueSpec::var("Cooldown", [7.0; 5]),
        cost: ValueSpec::var("Energy Cost", [30.0, 40.0, 50.0, 60.0, 70.0]),
        custom_exec: Some(grumpjaw_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Grumpjaw__C",
        targeting: CastTargeting::TargetEnemy,
        max_rank: 3,
        startup: ValueSpec::constant(0.5),
        range: ValueSpec::var3("Range", [4.0; 3]),
        cooldown: ValueSpec::var3("Cooldown", [70.0, 60.0, 50.0]),
        cost: ValueSpec::var3("Energy Cost", [50.0, 60.0, 70.0]),
        custom_exec: Some(grumpjaw_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    variants: &[AbilityVariantSpec {
        slot: HeroSlot::C2,
        spec: AbilityKitSpec {
            ability_id: "Ability__Grumpjaw__C_SpitOut",
            targeting: CastTargeting::Directional,
            max_rank: 3,
            range: ValueSpec::constant(5.0),
            cooldown: ValueSpec::var3("Cooldown", [70.0, 60.0, 50.0]),
            cost: ValueSpec::zero(),
            custom_exec: Some(grumpjaw_c_spit),
            ..AbilityKitSpec::DEFAULT
        },
    }],
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 17. Gwen (tid=395) - bespoke B (Skedaddle); adds A, C, Boomstick perk & B OOC MS
// ============================================================================

fn gwen_on_tick(ctx: &mut HeroKitContext<'_, '_>) {
    // Skedaddle passive OOC move speed (+0.6 when B learned & out of hero combat for 5s).
    if ctx.room.entities[ctx.caster].ability_ranks[1] > 0 {
        let last_dmg = ctx.room.entities[ctx.caster]
            .hero_kit
            .as_deref()
            .map_or(0, |k| k.last_damaged_tick);
        if last_dmg == 0 || ctx.tick >= last_dmg + TICK_HZ * 5 {
            ctx.add_buff_ticks(ctx.caster, BuffStatKind::MoveSpeedFlat, 0.6, 3);
        }
    }
}

fn gwen_on_basic_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    _target: usize,
    _base: f32,
    _crit: bool,
) -> (f32, DamageSchool) {
    let last_boom = ctx.room.entities[ctx.caster]
        .hero_kit
        .as_deref()
        .map_or(0, |k| k.timers[0]);
    if ctx.tick >= last_boom {
        let aspd = 1.0
            + ctx.room.entities[ctx.caster]
                .item_stats
                .attack_speed
                .max(0.0);
        let cd_ticks = seconds_to_ticks(1.6 / aspd);
        let st = ctx.room.entities[ctx.caster]
            .hero_kit
            .get_or_insert_with(Box::default);
        st.timers[0] = ctx.tick + cd_ticks;
        let lvl = f32::from(ctx.room.entities[ctx.caster].level.saturating_sub(1));
        let bonus = 25.0 + 8.0 * lvl + ctx.room.entities[ctx.caster].item_stats.bonus_weapon * 0.35;
        return (bonus, DamageSchool::Weapon);
    }
    (0.0, DamageSchool::Weapon)
}

static GWEN_A_EFFS: &[PrimitiveEffect] = &[PrimitiveEffect::ConeWave {
    range: ValueSpec::var("Range", [10.0; 5]),
    half_angle_deg: 28.0,
    damage: ValueSpec::dmg(0, [60.0, 105.0, 150.0, 195.0, 240.0], 2.0, 0.65, 0.0),
    school: DamageKind::Weapon,
    multi_bolts: 1,
    extra_hit_mult: 1.0,
    slow_pct: ValueSpec::var_scaled("Slow Amount", [0.3; 5], 0.0, 0.002, 0.0, 0.0),
    slow_seconds: ValueSpec::var("Slow_Duration", [2.0; 5]),
    stun_seconds: ValueSpec::zero(),
    fear_seconds: ValueSpec::zero(),
    silence_seconds: ValueSpec::zero(),
    knockback_dist: 0.0,
    reveal_seconds: 2.0,
    minion_mult: 0.5,
    applies_mark: None,
}];

static GWEN_C_EFFS: &[PrimitiveEffect] = &[PrimitiveEffect::SkillshotProjectile {
    damage: ValueSpec::dmg3(0, [300.0, 400.0, 500.0], 2.5, 1.0, 0.0),
    school: DamageKind::Weapon,
    range: ValueSpec::var3("Range", [14.0; 3]),
    speed: ValueSpec::var3("Shot Speed", [18.0; 3]),
    radius: ValueSpec::constant(0.9),
    piercing: true,
    return_to_caster: false,
    repeat_hit_mult: 1.0,
    splash_damage: None,
    splash_radius: 0.0,
    stun_seconds: ValueSpec::var3("Stun Duration", [0.9, 1.2, 1.5]),
    root_seconds: ValueSpec::zero(),
    silence_seconds: ValueSpec::zero(),
    slow_pct: ValueSpec::zero(),
    slow_seconds: ValueSpec::zero(),
    minion_mult: 0.5,
    resets_cast_cd_on_hit: false,
    applies_mark: None,
    destroys_enemy_projectiles: false,
}];

static GWEN: HeroKitSpec = HeroKitSpec {
    hero_id: "Gwen",
    type_id: 395,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Boomstick",
        on_tick: Some(gwen_on_tick),
        on_basic_attack: Some(gwen_on_basic_attack),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Gwen__A",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::constant(0.3),
        range: ValueSpec::var("Range", [10.0; 5]),
        cooldown: ValueSpec::var("Cooldown", [7.0, 6.5, 6.0, 5.5, 4.0]),
        cost: ValueSpec::var("Energy Cost", [50.0, 55.0, 60.0, 65.0, 70.0]),
        effects: GWEN_A_EFFS,
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Gwen__B",
        targeting: CastTargeting::InstantSelf,
        cooldown: ValueSpec::var("Cooldown", [19.0, 17.0, 15.0, 13.0, 10.0]),
        cost: ValueSpec::var("Energy Cost", [60.0, 65.0, 70.0, 75.0, 80.0]),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Gwen__C",
        targeting: CastTargeting::Directional,
        max_rank: 3,
        startup: ValueSpec::constant(0.6),
        range: ValueSpec::var3("Range", [14.0; 3]),
        cooldown: ValueSpec::var3("Cooldown", [90.0, 75.0, 60.0]),
        cost: ValueSpec::var3("Energy Cost", [100.0, 85.0, 70.0]),
        effects: GWEN_C_EFFS,
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 18. Idris (tid=273) - bespoke B (Chakram); adds Stamina, Divergent Paths, A & C
// ============================================================================

fn idris_paths(ctx: &HeroKitContext<'_, '_>) -> (bool, bool) {
    let both = ctx.talent_special() == TalentSpecial::IdrisBothPaths;
    let wp = ctx.room.entities[ctx.caster].weapon_power();
    let cp = ctx.room.entities[ctx.caster].item_stats.crystal;
    (both || wp >= 100.0, both || cp >= 100.0)
}

fn idris_on_basic_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    _target: usize,
    _base: f32,
    _crit: bool,
) -> (f32, DamageSchool) {
    let (wp_path, cp_path) = idris_paths(ctx);
    if wp_path {
        ctx.add_resource(10.0);
        ctx.reduce_slot_cd(AbilitySlot::A, 1.0);
        ctx.reduce_slot_cd(AbilitySlot::C, 1.0);
    }
    if cp_path {
        let cp = ctx.room.entities[ctx.caster].item_stats.crystal;
        return (cp * 0.8, DamageSchool::Crystal);
    }
    (0.0, DamageSchool::Weapon)
}

fn idris_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let goal = ctx.resolve_target_pos(target, 5.5);
    if ctx.room.spell_motion_clear(ctx.caster_pos(), goal) {
        ctx.room.place_spell_actor(ctx.caster, goal, ctx.events);
    }
    let last_dmg = ctx.room.entities[ctx.caster]
        .hero_kit
        .as_deref()
        .map_or(0, |k| k.last_damaged_tick);
    let lockout = (7.0 - ctx.room.entities[ctx.caster].item_stats.bonus_weapon * 0.03).max(1.5);
    if last_dmg == 0 || ctx.tick >= last_dmg + seconds_to_ticks(lockout) {
        let bar = ctx.eval_var(
            "Ability__Idris__A",
            rank,
            ValueSpec::var_scaled(
                "Barrier Strength",
                [90.0, 130.0, 170.0, 210.0, 250.0],
                0.0,
                1.2,
                0.0,
                0.0,
            ),
        ) * ctx.talent_barrier_mult(AbilitySlot::A);
        ctx.grant_barrier(ctx.caster, bar, 2.0, None);
    }
    let emp_amp = if rank >= 5 { 0.5 } else { 0.3 };
    let bonus_wp = ctx.room.entities[ctx.caster].weapon_power() * emp_amp;
    ctx.empower_next_attacks(1, bonus_wp, false, 0.0, 0.0, 0.0, 0.0, 4.0);
    Ok(true)
}

fn idris_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let tj = ctx.resolve_target_idx(target).ok_or("target required")?;
    let host_id = ctx.room.entities[tj].id;
    let is_enemy = ctx.room.entities[tj].team != ctx.caster_team();
    let window = if is_enemy { 1.2 } else { 2.8 };
    let dot = if is_enemy {
        ctx.eval_var(
            "Ability__Idris__C",
            rank,
            ValueSpec::var3_scaled("Damage Over Time", [250.0, 425.0, 600.0], 1.5),
        ) / window
    } else {
        0.0
    };
    let sweep = ctx.eval_var(
        "Ability__Idris__C",
        rank,
        ValueSpec::dmg3(0, [150.0, 250.0, 350.0], 0.0, 1.25, 0.0),
    );
    ctx.cleanse_cc(ctx.caster);
    ctx.set_untargetable(ctx.caster, window);
    ctx.spawn_effect(
        KitEffectKind::Attachment {
            ability_slot: 2,
            host_id,
            is_devour: false,
            dps: dot,
            end_damage: sweep,
            end_radius: 4.0,
            school: DamageSchool::Weapon,
            next_tick: ctx.tick + TICK_HZ / 4,
            end_tick: ctx.tick + seconds_to_ticks(window),
        },
        ctx.room.entities[tj].position,
        ctx.room.entities[tj].position,
    );
    ctx.open_recast(AbilitySlot::C, 1, host_id, window);
    if ctx.talent_special() == TalentSpecial::IdrisShimmerHeal {
        let pct = ctx.talent_mod("HEAL_PERCENT_MAX_HEALTH").unwrap_or(0.4);
        ctx.heal_unit(ctx.caster, ctx.room.entities[ctx.caster].max_hp * pct);
        if !is_enemy {
            ctx.heal_unit(tj, ctx.room.entities[tj].max_hp * pct);
        }
    }
    Ok(true)
}

fn idris_c_leap(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    ctx.clear_recast(AbilitySlot::C);
    let owner_id = ctx.caster_id();
    let to_rm: Vec<_> = ctx
        .room
        .entities
        .ecs
        .query::<(bevy_ecs::entity::Entity, &crate::hero_kit::KitEffect)>()
        .iter(&ctx.room.entities.ecs)
        .filter_map(|(eh, eff)| {
            (eff.owner_id == owner_id
                && matches!(
                    eff.kind,
                    KitEffectKind::Attachment {
                        is_devour: false,
                        ..
                    }
                ))
            .then_some(eh)
        })
        .collect();
    for eh in to_rm {
        ctx.room
            .entities
            .remove_spell_handle::<crate::hero_kit::KitEffect>(eh);
    }
    let (_, cp_path) = idris_paths(ctx);
    let dist = if cp_path { 6.0 } else { 3.5 };
    let dest = ctx.resolve_target_pos(target, dist);
    if ctx.room.spell_motion_clear(ctx.caster_pos(), dest) {
        ctx.room.place_spell_actor(ctx.caster, dest, ctx.events);
    }
    let sweep = ctx.eval_var(
        "Ability__Idris__C",
        rank,
        ValueSpec::dmg3(0, [150.0, 250.0, 350.0], 0.0, 1.25, 0.0),
    ) * ctx.talent_dmg_mult(AbilitySlot::C);
    for j in ctx.enemies_in_radius(ctx.caster_pos(), 4.0, false) {
        ctx.deal_damage(j, sweep, DamageSchool::Weapon, 0.0, false);
    }
    Ok(true)
}

static IDRIS: HeroKitSpec = HeroKitSpec {
    hero_id: "Idris",
    type_id: 273,
    resource: ResourceKind::Stamina,
    resource_max: 100.0,
    resource_initial: 100.0,
    resource_regen_per_sec: 10.0,
    resource_energy_conversion: (0.5, 0.5),
    perk: HeroicPerkSpec {
        id: "Divergent Paths",
        on_basic_attack: Some(idris_on_basic_attack),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Idris__A",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        range: ValueSpec::constant(5.5),
        cooldown: ValueSpec::var("Cooldown", [13.0, 12.0, 11.0, 10.0, 8.0]),
        cost: ValueSpec::var("Alt Energy Cost", [70.0; 5]),
        custom_exec: Some(idris_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Idris__B",
        targeting: CastTargeting::Directional,
        range: ValueSpec::constant(11.0),
        cooldown: ValueSpec::var("Cooldown", [8.5, 8.0, 7.5, 7.0, 5.0]),
        cost: ValueSpec::var("Alt Energy Cost", [30.0; 5]),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Idris__C",
        targeting: CastTargeting::TargetAnyHero,
        max_rank: 3,
        range: ValueSpec::constant(6.0),
        cooldown: ValueSpec::var3("Cooldown", [110.0, 90.0, 70.0]),
        cost: ValueSpec::zero(),
        custom_exec: Some(idris_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    variants: &[AbilityVariantSpec {
        slot: HeroSlot::C2,
        spec: AbilityKitSpec {
            ability_id: "Ability__Idris__C_LeapOff",
            targeting: CastTargeting::Directional,
            max_rank: 3,
            range: ValueSpec::constant(6.0),
            cooldown: ValueSpec::var3("Cooldown", [110.0, 90.0, 70.0]),
            cost: ValueSpec::zero(),
            custom_exec: Some(idris_c_leap),
            ..AbilityKitSpec::DEFAULT
        },
    }],
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 19. Inara (tid=440)
// ============================================================================

fn inara_on_kill_assist(
    ctx: &mut HeroKitContext<'_, '_>,
    _victim: usize,
    _killer: bool,
    _is_hero: bool,
) {
    ctx.add_buff(ctx.caster, BuffStatKind::MoveSpeedFlat, 1.5, 4.0);
}

fn inara_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    _target: CastTarget,
) -> Result<bool, &'static str> {
    if ctx.talent_special() == TalentSpecial::InaraDanceHealAllies {
        let h = ctx.talent_mod("HEAL_AMOUNT").unwrap_or(125.0);
        for aj in ctx.allies_in_radius(ctx.caster_pos(), 5.0, true, true) {
            ctx.heal_unit(aj, h);
        }
    }
    let dmg = ctx.eval_var(
        "Ability__Inara__A",
        rank,
        ValueSpec::dmg(0, [80.0, 140.0, 200.0, 260.0, 440.0], 1.4, 1.2, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let slow = ctx.eval_var(
        "Ability__Inara__A",
        rank,
        ValueSpec::var("SLOW_STRENGTH", [0.3, 0.35, 0.4, 0.45, 0.5]),
    );
    for j in ctx.enemies_in_radius(ctx.caster_pos(), 5.0, false) {
        let m = if ctx.room.entities[j].kind == Kind::Hero {
            1.0
        } else {
            0.6
        };
        ctx.deal_damage(j, dmg * m, DamageSchool::Weapon, 0.0, false);
        ctx.apply_slow(j, slow, 1.5);
    }
    Ok(true)
}

fn inara_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    if ctx.talent_special() == TalentSpecial::InaraInvulnOnDash {
        let inv = ctx.talent_mod("INVULN_DURATION").unwrap_or(1.0);
        ctx.set_untargetable(ctx.caster, inv);
    }
    let kb = 6.0 + ctx.talent_mod("BONUS_DISTANCE").unwrap_or(0.0);
    let dmg = ctx.eval_var(
        "Ability__Inara__B",
        rank,
        ValueSpec::dmg(0, [60.0, 120.0, 180.0, 240.0, 360.0], 1.25, 1.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let aim = ctx.resolve_target_pos(target, 5.0);
    let origin = ctx.caster_pos();
    for j in ctx.enemies_in_radius(aim, 3.0, false) {
        ctx.deal_damage(j, dmg, DamageSchool::Weapon, 0.0, false);
        ctx.knockback(j, origin, kb, 0.5);
        ctx.add_mark(j, marks::BANISHING_MARK, 1, 1, 3.0);
    }
    Ok(true)
}

static INARA: HeroKitSpec = HeroKitSpec {
    hero_id: "Inara",
    type_id: 440,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Sacred Grove",
        on_kill_assist: Some(inara_on_kill_assist),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Inara__A",
        targeting: CastTargeting::InstantSelf,
        max_rank: 5,
        range: ValueSpec::var("RANGE", [5.0; 5]),
        cooldown: ValueSpec::var("COOLDOWN", [6.0, 5.5, 5.0, 4.5, 4.0]),
        cost: ValueSpec::var("ENERGY COST", [30.0, 35.0, 40.0, 45.0, 0.0]),
        custom_exec: Some(inara_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Inara__B",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        range: ValueSpec::var("RANGE", [5.0; 5]),
        cooldown: ValueSpec::var("COOLDOWN", [12.0, 11.5, 11.0, 10.5, 8.0]),
        cost: ValueSpec::var("ENERGY COST", [60.0; 5]),
        custom_exec: Some(inara_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Inara__C",
        targeting: CastTargeting::InstantSelf,
        max_rank: 3,
        range: ValueSpec::var3("RANGE", [14.0; 3]),
        cooldown: ValueSpec::var3("COOLDOWN", [90.0, 75.0, 60.0]),
        cost: ValueSpec::var3("ENERGY COST", [100.0; 3]),
        effects: &[
            PrimitiveEffect::SelfBuff {
                stat: BuffStatKind::MoveSpeedFlat,
                amount: ValueSpec::var3("DECAYING_MSPD_STRENGTH", [2.5; 3]),
                duration: ValueSpec::var3("MSPD_DURATION", [7.0, 8.0, 9.0]),
                empowered_attacks: 0,
                empowered_bonus_damage: ValueSpec::zero(),
                empowered_stun_seconds: 0.0,
                empowered_slow_pct: 0.0,
                empowered_slow_seconds: 0.0,
                empowered_knockback: 0.0,
                resets_attack_cd: false,
            },
            PrimitiveEffect::OpenRecast {
                slot: HeroSlot::C,
                window_seconds: ValueSpec::var3("DASH_AVAILABLE_DURATION", [10.0; 3]),
                stage: 1,
            },
        ],
        ..AbilityKitSpec::DEFAULT
    },
    variants: &[AbilityVariantSpec {
        slot: HeroSlot::C2,
        spec: AbilityKitSpec {
            ability_id: "Ability__Inara__C2",
            targeting: CastTargeting::Directional,
            max_rank: 5,
            range: ValueSpec::constant(8.0),
            cooldown: ValueSpec::var3("COOLDOWN", [90.0, 75.0, 60.0]),
            cost: ValueSpec::zero(),
            effects: &[PrimitiveEffect::DashBlink {
                range: ValueSpec::constant(8.0),
                duration: ValueSpec::constant(0.25),
                instant_blink: false,
                invuln_during: false,
                unstoppable_during: false,
                path_radius: 2.2,
                path_damage: ValueSpec::dmg3(0, [200.0, 300.0, 400.0], 1.5, 1.2, 0.0),
                arrival_radius: 2.5,
                arrival_damage: ValueSpec::zero(),
                school: DamageKind::Weapon,
                slow_pct: ValueSpec::zero(),
                slow_seconds: ValueSpec::zero(),
                stun_seconds: ValueSpec::zero(),
                knockback_dist: 0.0,
                backward_hop: false,
                applies_mark: None,
            }],
            ..AbilityKitSpec::DEFAULT
        },
    }],
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 20. Ishtar (tid=922)
// ============================================================================

fn ishtar_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    _target: CastTarget,
) -> Result<bool, &'static str> {
    let dmg = ctx.eval_var(
        "Ability__Ishtar__B",
        rank,
        ValueSpec::dmg(0, [40.0, 80.0, 120.0, 160.0, 240.0], 0.5, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let slow = ctx.eval_var(
        "Ability__Ishtar__B",
        rank,
        ValueSpec::var("SLOW_STRENGTH", [0.45, 0.45, 0.45, 0.45, 0.6]),
    );
    let root = if ctx.talent_special() == TalentSpecial::IshtarWhirlRoot {
        ctx.talent_mod("ROOT_DURATION").unwrap_or(1.0)
    } else {
        0.0
    };
    let hits = ctx.enemies_in_radius(ctx.caster_pos(), 3.5, false);
    let hit_any = !hits.is_empty();
    for j in hits {
        ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
        ctx.apply_slow(j, slow, 2.0);
        if root > 0.0 {
            ctx.apply_root(j, root);
        }
    }
    if hit_any {
        ctx.add_buff(ctx.caster, BuffStatKind::MoveSpeedFlat, 2.0, 2.0);
    }
    Ok(true)
}

static ISHTAR_A_EFFS: &[PrimitiveEffect] = &[
    PrimitiveEffect::SkillshotProjectile {
        damage: ValueSpec::var_scaled(
            "DASH_DAMAGE",
            [30.0, 60.0, 90.0, 120.0, 150.0],
            0.5,
            0.0,
            0.0,
            0.0,
        ),
        school: DamageKind::Crystal,
        range: ValueSpec::var("Range", [8.0; 5]),
        speed: ValueSpec::constant(24.0),
        radius: ValueSpec::constant(1.2),
        piercing: true,
        return_to_caster: false,
        repeat_hit_mult: 0.75,
        splash_damage: None,
        splash_radius: 0.0,
        stun_seconds: ValueSpec::zero(),
        root_seconds: ValueSpec::zero(),
        silence_seconds: ValueSpec::zero(),
        slow_pct: ValueSpec::zero(),
        slow_seconds: ValueSpec::zero(),
        minion_mult: 0.5,
        resets_cast_cd_on_hit: false,
        applies_mark: None,
        destroys_enemy_projectiles: false,
    },
    PrimitiveEffect::SummonPet {
        count: 1,
        max_active: 4,
        duration: ValueSpec::constant(15.0),
        pet_hp: ValueSpec::constant(300.0),
        pet_damage: ValueSpec::var_scaled("PET_DAMAGE", [10.0; 5], 0.3, 0.0, 0.0, 0.0),
        school: DamageKind::Crystal,
        hit_ticks: 3,
        applies_mark: None,
        replace_existing: false,
        pet_kind: 2,
    },
];

static ISHTAR_C_EFFS: &[PrimitiveEffect] = &[
    PrimitiveEffect::SummonPet {
        count: 4,
        max_active: 4,
        duration: ValueSpec::var3("C_DURATION", [8.0, 10.0, 12.0]),
        pet_hp: ValueSpec::constant(400.0),
        pet_damage: ValueSpec::constant(25.0),
        school: DamageKind::Crystal,
        hit_ticks: 3,
        applies_mark: None,
        replace_existing: true,
        pet_kind: 2,
    },
    PrimitiveEffect::SelfBuff {
        stat: BuffStatKind::Lifesteal,
        amount: ValueSpec::var3("BONUS_LIFESTEAL", [0.05, 0.1, 0.15]),
        duration: ValueSpec::var3("C_DURATION", [8.0, 10.0, 12.0]),
        empowered_attacks: 8,
        empowered_bonus_damage: ValueSpec::dmg3(0, [50.0; 3], 0.2, 0.55, 0.0),
        empowered_stun_seconds: 0.0,
        empowered_slow_pct: 0.0,
        empowered_slow_seconds: 0.0,
        empowered_knockback: 0.0,
        resets_attack_cd: false,
    },
    PrimitiveEffect::SelfBuff {
        stat: BuffStatKind::MoveSpeedFlat,
        amount: ValueSpec::var3("BONUS_MOVESPEED", [1.0; 3]),
        duration: ValueSpec::var3("C_DURATION", [8.0, 10.0, 12.0]),
        empowered_attacks: 0,
        empowered_bonus_damage: ValueSpec::zero(),
        empowered_stun_seconds: 0.0,
        empowered_slow_pct: 0.0,
        empowered_slow_seconds: 0.0,
        empowered_knockback: 0.0,
        resets_attack_cd: false,
    },
];

static ISHTAR: HeroKitSpec = HeroKitSpec {
    hero_id: "Ishtar",
    type_id: 922,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Demonic Decree",
        basic_attack_is_crystal: true,
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Ishtar__A",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        range: ValueSpec::var("Range", [8.0; 5]),
        cooldown: ValueSpec::var("Cooldown", [15.0, 14.0, 13.0, 12.0, 9.0]),
        cost: ValueSpec::var("ENERGY COST", [20.0, 25.0, 30.0, 35.0, 40.0]),
        effects: ISHTAR_A_EFFS,
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Ishtar__B",
        targeting: CastTargeting::InstantSelf,
        max_rank: 5,
        range: ValueSpec::constant(3.5),
        cooldown: ValueSpec::var("Cooldown", [12.0, 11.5, 11.0, 10.5, 9.0]),
        cost: ValueSpec::var("ENERGY COST", [60.0, 65.0, 70.0, 75.0, 80.0]),
        passive_bonus: Some((
            BuffStatKind::AttackSpeed,
            ValueSpec::var("PASSIVE_ASPD_BONUS", [0.03, 0.06, 0.09, 0.12, 0.2]),
        )),
        custom_exec: Some(ishtar_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Ishtar__C",
        targeting: CastTargeting::InstantSelf,
        max_rank: 3,
        cooldown: ValueSpec::var3("Cooldown", [85.0, 75.0, 65.0]),
        cost: ValueSpec::var3("ENERGY COST", [100.0, 125.0, 150.0]),
        effects: ISHTAR_C_EFFS,
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 21. Joule (tid=253)
// ============================================================================

fn joule_on_take_damage(
    _ctx: &mut HeroKitContext<'_, '_>,
    _src: Option<usize>,
    _dmg: f32,
    _school: DamageSchool,
) -> f32 {
    0.75
}

fn joule_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let goal = ctx.resolve_target_pos(target, 10.0);
    if ctx.room.spell_motion_clear(ctx.caster_pos(), goal) {
        ctx.room.place_spell_actor(ctx.caster, goal, ctx.events);
    }
    let dmg = ctx.eval_var(
        "Ability__Joule__A",
        rank,
        ValueSpec::dmg(0, [150.0, 200.0, 250.0, 300.0, 350.0], 1.0, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let stun = ctx.eval_var(
        "Ability__Joule__A",
        rank,
        ValueSpec::var("Stun Duration", [0.7, 0.8, 0.9, 1.0, 1.5]),
    );
    let slow = ctx.eval_var(
        "Ability__Joule__A",
        rank,
        ValueSpec::var("SLOW_STRENGTH", [0.3, 0.35, 0.4, 0.45, 0.6]),
    );
    let no_stun = ctx.talent_special() == TalentSpecial::JouleLeapResetNoStun;
    let mut hit_hero = false;
    for j in ctx.enemies_in_radius(goal, 3.9, false) {
        let d = ctx.room.entities[j].position.dist(goal);
        if ctx.room.entities[j].kind == Kind::Hero {
            hit_hero = true;
        }
        if d <= 2.3 {
            ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
            if !no_stun {
                ctx.apply_stun(j, stun);
            }
        } else {
            ctx.deal_damage(j, dmg * 0.5, DamageSchool::Crystal, 0.0, false);
            ctx.apply_slow(j, slow, 1.5);
        }
    }
    if no_stun && hit_hero {
        ctx.reset_slot_cd(AbilitySlot::A);
    }
    Ok(true)
}

static JOULE_B_EFFS: &[PrimitiveEffect] = &[PrimitiveEffect::SkillshotProjectile {
    damage: ValueSpec::dmg(0, [80.0, 110.0, 140.0, 170.0, 260.0], 1.4, 1.7, 0.0),
    school: DamageKind::Weapon,
    range: ValueSpec::var("RANGE", [7.0; 5]),
    speed: ValueSpec::constant(35.0),
    radius: ValueSpec::constant(1.2),
    piercing: true,
    return_to_caster: false,
    repeat_hit_mult: 1.0,
    splash_damage: None,
    splash_radius: 0.0,
    stun_seconds: ValueSpec::zero(),
    root_seconds: ValueSpec::zero(),
    silence_seconds: ValueSpec::zero(),
    slow_pct: ValueSpec::zero(),
    slow_seconds: ValueSpec::zero(),
    minion_mult: 0.6,
    resets_cast_cd_on_hit: false,
    applies_mark: None,
    destroys_enemy_projectiles: false,
}];

static JOULE_C_EFFS: &[PrimitiveEffect] = &[PrimitiveEffect::ChannelBeam {
    duration: ValueSpec::var3("CHANNEL_TIME", [1.5; 3]),
    ticks_per_sec: 6.0,
    range: ValueSpec::constant(14.0),
    width: 1.8,
    dps: ValueSpec::var3_scaled("DAMAGE_PER_SEC", [600.0, 900.0, 1200.0], 2.2),
    school: DamageKind::Crystal,
    lock_caster: true,
    slow_pct: ValueSpec::zero(),
    stun_target: false,
    global_all_heroes: false,
    shield_pierce: ValueSpec::zero(),
    structure_mult: 0.5,
    minion_mult: 1.0,
}];

static JOULE: HeroKitSpec = HeroKitSpec {
    hero_id: "Joule",
    type_id: 253,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Heavy Plating",
        on_take_damage: Some(joule_on_take_damage),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Joule__A",
        targeting: CastTargeting::GroundPoint,
        max_rank: 5,
        range: ValueSpec::constant(10.0),
        cooldown: ValueSpec::var("Cooldown", [15.0, 14.0, 13.0, 12.0, 8.0]),
        cost: ValueSpec::var("Energy Cost", [100.0, 115.0, 130.0, 145.0, 160.0]),
        custom_exec: Some(joule_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Joule__B",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        range: ValueSpec::var("RANGE", [7.0; 5]),
        cooldown: ValueSpec::var("COOLDOWN", [3.0, 2.8, 2.6, 2.4, 2.0]),
        cost: ValueSpec::var("ENERGY COST", [50.0, 55.0, 60.0, 65.0, 35.0]),
        effects: JOULE_B_EFFS,
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Joule__C",
        targeting: CastTargeting::Directional,
        max_rank: 3,
        startup: ValueSpec::constant(0.6),
        range: ValueSpec::constant(14.0),
        cooldown: ValueSpec::var3("Cooldown", [80.0, 70.0, 60.0]),
        cost: ValueSpec::var3("Energy Cost", [100.0, 125.0, 150.0]),
        charges: ValueSpec::constant(1.0),
        effects: JOULE_C_EFFS,
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 22. Karas (tid=926) - bespoke B (Twister); adds Mistral Blade, A & C
// ============================================================================

fn karas_on_ability_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    _rank: u8,
    _target: CastTarget,
) {
    ctx.add_buff(ctx.caster, BuffStatKind::MoveSpeedFlat, 0.4, 3.0);
}

fn karas_on_basic_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    target_idx: usize,
    _base: f32,
    _crit: bool,
) -> (f32, DamageSchool) {
    let shroud = ctx.room.entities[ctx.caster]
        .hero_kit
        .as_deref()
        .map_or(0, |k| k.counters[1]);
    if shroud > 0 {
        let r = ctx.room.entities[ctx.caster].ability_ranks[2].max(1);
        let hits = {
            let st = ctx.room.entities[ctx.caster]
                .hero_kit
                .as_deref_mut()
                .unwrap();
            st.counters[1] -= 1;
            st.counters[2] = (st.counters[2] + 1) % 4;
            st.counters[2] == 0
        };
        if hits {
            let aoe = ctx.eval_var(
                "Ability__Karas__C",
                r,
                ValueSpec::dmg3(1, [225.0, 275.0, 325.0], 0.5, 0.0, 0.0),
            ) * ctx.talent_dmg_mult(AbilitySlot::C);
            let center = ctx.room.entities[target_idx].position;
            for j in ctx.enemies_in_radius(center, 2.5, false) {
                ctx.deal_damage(j, aoe, DamageSchool::Crystal, 0.0, false);
            }
        }
        let d = ctx.eval_var(
            "Ability__Karas__C",
            r,
            ValueSpec::dmg3(0, [175.0, 200.0, 225.0], 0.5, 0.0, 0.0),
        ) * ctx.talent_dmg_mult(AbilitySlot::C);
        return (d, DamageSchool::Crystal);
    }
    (0.0, DamageSchool::Crystal)
}

fn karas_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    rank: u8,
    _target: CastTarget,
) -> Result<bool, &'static str> {
    let ammo = match rank {
        1 => 8,
        2 => 10,
        _ => 12,
    };
    let st = ctx.room.entities[ctx.caster]
        .hero_kit
        .get_or_insert_with(Box::default);
    st.counters[1] = ammo;
    st.counters[2] = 0;
    Ok(true)
}

static KARAS_A_EFFS: &[PrimitiveEffect] = &[PrimitiveEffect::SkillshotProjectile {
    damage: ValueSpec::dmg(0, [110.0, 140.0, 170.0, 200.0, 290.0], 1.2, 0.0, 0.0),
    school: DamageKind::Crystal,
    range: ValueSpec::var("Range", [7.0, 7.5, 8.0, 8.5, 10.0]),
    speed: ValueSpec::var("A_BladeSpeed", [22.0; 5]),
    radius: ValueSpec::constant(1.2),
    piercing: true,
    return_to_caster: false,
    repeat_hit_mult: 1.0,
    splash_damage: None,
    splash_radius: 0.0,
    stun_seconds: ValueSpec::zero(),
    root_seconds: ValueSpec::zero(),
    silence_seconds: ValueSpec::zero(),
    slow_pct: ValueSpec::var("A_Slow", [0.12; 5]),
    slow_seconds: ValueSpec::var("A_Slow_Duration", [1.5, 1.75, 2.0, 2.25, 2.5]),
    minion_mult: 0.7,
    resets_cast_cd_on_hit: false,
    applies_mark: None,
    destroys_enemy_projectiles: false,
}];

static KARAS: HeroKitSpec = HeroKitSpec {
    hero_id: "Karas",
    type_id: 926,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Mistral Blade",
        basic_attack_is_crystal: false,
        on_basic_attack: Some(karas_on_basic_attack),
        on_ability_cast: Some(karas_on_ability_cast),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Karas__A",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        range: ValueSpec::var("Range", [7.0, 7.5, 8.0, 8.5, 10.0]),
        cooldown: ValueSpec::var("Cooldown", [12.0, 11.0, 10.0, 9.0, 8.0]),
        cost: ValueSpec::var("Energy Cost", [60.0, 65.0, 70.0, 75.0, 80.0]),
        effects: KARAS_A_EFFS,
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Karas__B",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        range: ValueSpec::var("Range", [7.0; 5]),
        cooldown: ValueSpec::var("Cooldown", [13.0, 12.0, 11.0, 10.0, 9.0]),
        cost: ValueSpec::var("Energy Cost", [60.0, 70.0, 80.0, 90.0, 100.0]),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Karas__C",
        targeting: CastTargeting::InstantSelf,
        max_rank: 3,
        range: ValueSpec::var3("Range", [10.0; 3]),
        cooldown: ValueSpec::var3("Cooldown", [70.0, 60.0, 50.0]),
        cost: ValueSpec::var3("Energy Cost", [120.0, 140.0, 160.0]),
        custom_exec: Some(karas_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 23. Kensei (tid=412)
// ============================================================================

fn kensei_on_basic_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    target_idx: usize,
    base: f32,
    _crit: bool,
) -> (f32, DamageSchool) {
    let add = if ctx.talent_special() == TalentSpecial::KenseiMaxStacksOnHit {
        4
    } else {
        1
    };
    let stacks = ctx.add_mark(ctx.caster, marks::STANCE_COUNTER, add, 4, 8.0);
    if stacks >= 4 {
        ctx.consume_mark(ctx.caster, marks::STANCE_COUNTER);
        let lvl = f32::from(ctx.room.entities[ctx.caster].level.saturating_sub(1));
        let bar = (60.0 + 12.0 * lvl + ctx.room.entities[ctx.caster].weapon_power() * 0.5)
            * ctx.talent_barrier_mult(AbilitySlot::A);
        ctx.grant_barrier(ctx.caster, bar, 3.0, None);
    }
    let my_max = ctx.room.entities[ctx.caster].max_hp;
    let tgt_max = ctx.room.entities[target_idx].max_hp;
    if tgt_max > my_max {
        return (base * 0.15, DamageSchool::Weapon);
    }
    (0.0, DamageSchool::Weapon)
}

fn kensei_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let kensho = ctx.consume_mark(ctx.caster, marks::KENSHO_STANCE) > 0;
    let range = if kensho { 5.5 } else { 3.1 };
    let goal = ctx.resolve_target_pos(target, range);
    if ctx.room.spell_motion_clear(ctx.caster_pos(), goal) {
        ctx.room.place_spell_actor(ctx.caster, goal, ctx.events);
    }
    let dmg = ctx.eval_var(
        "Ability__Kensei__A",
        rank,
        ValueSpec::dmg(0, [20.0, 40.0, 60.0, 80.0, 100.0], 0.0, 0.65, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let slow = ctx.eval_var(
        "Ability__Kensei__A",
        rank,
        ValueSpec::var("SLOW_STRENGTH", [0.3, 0.35, 0.4, 0.45, 0.5]),
    );
    for j in ctx.enemies_in_radius(goal, 2.8, false) {
        ctx.deal_damage(j, dmg, DamageSchool::Weapon, 0.0, false);
        if kensho {
            ctx.apply_slow(j, slow, 1.2);
        }
    }
    let stage = ctx.room.entities[ctx.caster]
        .hero_kit
        .as_deref()
        .map_or(0, |k| k.step_stage[0]);
    if stage < 2 {
        ctx.open_recast(AbilitySlot::A, stage + 1, 0, 7.5);
    } else {
        ctx.clear_recast(AbilitySlot::A);
    }
    Ok(true)
}

fn kensei_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let aim = ctx.resolve_target_pos(target, 3.5);
    let dmg = ctx.eval_var(
        "Ability__Kensei__B",
        rank,
        ValueSpec::dmg(0, [20.0, 35.0, 50.0, 65.0, 110.0], 0.0, 1.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    for j in ctx.enemies_in_radius(aim, 2.5, false) {
        ctx.deal_damage(j, dmg, DamageSchool::Weapon, 0.0, false);
    }
    let fort = ctx.eval_var(
        "Ability__Kensei__B",
        rank,
        ValueSpec::var_scaled(
            "FORT_HEALTH_PER_SECOND",
            [30.0, 50.0, 70.0, 90.0, 110.0],
            0.0,
            0.5,
            0.0,
            0.0,
        ),
    ) * 4.0;
    ctx.grant_fortified(ctx.caster, fort, 4.0);
    ctx.add_buff(ctx.caster, BuffStatKind::MoveSpeedFlat, 1.5, 2.5);
    ctx.add_mark(ctx.caster, marks::KENSHO_STANCE, 1, 1, 2.5);
    if ctx.talent_special() == TalentSpecial::KenseiEmpoweredInvuln {
        let inv = ctx.talent_mod("DURATION").unwrap_or(1.1);
        ctx.set_untargetable(ctx.caster, inv);
    }
    Ok(true)
}

fn kensei_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let kensho = ctx.consume_mark(ctx.caster, marks::KENSHO_STANCE) > 0;
    let goal = ctx.resolve_target_pos(target, 10.0);
    if ctx.room.spell_motion_clear(ctx.caster_pos(), goal) {
        ctx.room.place_spell_actor(ctx.caster, goal, ctx.events);
    }
    let dmg = ctx.eval_var(
        "Ability__Kensei__C",
        rank,
        ValueSpec::dmg3(0, [200.0, 300.0, 400.0], 0.0, 1.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let stun = ctx.eval_var(
        "Ability__Kensei__C",
        rank,
        ValueSpec::var3("STUN_DURATION", [0.6, 0.8, 1.0]),
    );
    let hits = ctx.enemies_in_radius(goal, 3.0, false);
    let mut hit_hero = false;
    for j in hits {
        if ctx.room.entities[j].kind == Kind::Hero {
            hit_hero = true;
            if kensho {
                ctx.apply_stun(j, stun);
            }
        }
        ctx.deal_damage(j, dmg, DamageSchool::Weapon, 0.0, false);
    }
    if hit_hero && ctx.talent_special() == TalentSpecial::KenseiResetOnUltHit {
        ctx.reset_slot_cd(AbilitySlot::C);
    }
    Ok(true)
}

static KENSEI: HeroKitSpec = HeroKitSpec {
    hero_id: "Kensei",
    type_id: 412,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Immovable Mind",
        on_basic_attack: Some(kensei_on_basic_attack),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Kensei__A",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        range: ValueSpec::var("RANGE", [3.1; 5]),
        cooldown: ValueSpec::var("COOLDOWN", [12.0, 11.0, 10.0, 9.0, 7.0]),
        cost: ValueSpec::var("ENERGY COST", [25.0; 5]),
        custom_exec: Some(kensei_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Kensei__B",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        range: ValueSpec::var("RANGE", [3.5; 5]),
        cooldown: ValueSpec::var("COOLDOWN", [14.0, 12.0, 10.0, 8.0, 6.0]),
        cost: ValueSpec::var("ENERGY COST", [40.0, 45.0, 50.0, 55.0, 60.0]),
        custom_exec: Some(kensei_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Kensei__C",
        targeting: CastTargeting::Directional,
        max_rank: 3,
        startup: ValueSpec::constant(0.7),
        range: ValueSpec::var3("RANGE", [10.0; 3]),
        cooldown: ValueSpec::var3("COOLDOWN", [70.0, 55.0, 40.0]),
        cost: ValueSpec::var3("ENERGY COST", [100.0, 120.0, 140.0]),
        custom_exec: Some(kensei_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    variants: &[AbilityVariantSpec {
        slot: HeroSlot::A2,
        spec: AbilityKitSpec {
            ability_id: "Ability__Kensei__A",
            targeting: CastTargeting::Directional,
            max_rank: 5,
            range: ValueSpec::var("RANGE", [3.1; 5]),
            cooldown: ValueSpec::var("COOLDOWN", [12.0, 11.0, 10.0, 9.0, 7.0]),
            cost: ValueSpec::var("ENERGY COST", [25.0; 5]),
            custom_exec: Some(kensei_a_cast),
            ..AbilityKitSpec::DEFAULT
        },
    }],
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 24. Kestrel (tid=267)
// ============================================================================

fn kestrel_on_basic_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    _target: usize,
    _base: f32,
    _crit: bool,
) -> (f32, DamageSchool) {
    ctx.add_mark(ctx.caster, marks::ADRENALINE, 1, 5, 6.0);
    (0.0, DamageSchool::Weapon)
}

fn kestrel_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let impact_mult = ctx.eval_var(
        "Ability__Kestrel__A",
        rank,
        ValueSpec::var("ImpactDamage", [1.05, 1.1, 1.15, 1.2, 1.3]),
    );
    let impact_dmg =
        ctx.room.entities[ctx.caster].weapon_power() * impact_mult * ctx.talent_dmg_mult(slot);
    let splash_dmg = ctx.eval_var(
        "Ability__Kestrel__A",
        rank,
        ValueSpec::dmg(0, [40.0, 80.0, 120.0, 160.0, 200.0], 1.0, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let range = 10.0 + ctx.talent_mod("RANGE").unwrap_or(0.0);
    let aim = ctx.resolve_target_pos(target, range);
    if let Some(&j) = ctx.enemies_in_radius(aim, 2.5, false).first() {
        ctx.deal_damage(j, impact_dmg, DamageSchool::Weapon, 0.0, true);
        if ctx.room.entities[j].kind == Kind::Hero {
            ctx.reduce_slot_cd(AbilitySlot::B, 1.5);
        }
        let p = ctx.room.entities[j].position;
        for sj in ctx.enemies_in_radius(p, 3.0, false) {
            ctx.deal_damage(sj, splash_dmg, DamageSchool::Crystal, 0.0, false);
        }
    }
    if ctx.talent_special() == TalentSpecial::KestrelDoubleArrow
        && let Some(&j) = ctx.enemies_in_radius(aim, 2.5, false).first()
    {
        ctx.deal_damage(j, impact_dmg, DamageSchool::Weapon, 0.0, false);
    }
    Ok(true)
}

fn kestrel_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    _target: CastTarget,
) -> Result<bool, &'static str> {
    if ctx.talent_special() == TalentSpecial::KestrelCamoRefillsGlimmershot {
        let max_c = ctx.room.kit_max_charges(ctx.caster, AbilitySlot::A);
        let st = ctx.room.entities[ctx.caster]
            .hero_kit
            .get_or_insert_with(Box::default);
        st.charges[0] = max_c;
        st.charges_initialized[0] = true;
    }
    let dur = ctx.eval_var(
        "Ability__Kestrel__B",
        rank,
        ValueSpec::var_scaled(
            "StealthDuration",
            [1.2, 1.2, 1.2, 1.2, 1.7],
            0.03,
            0.0,
            0.0,
            0.0,
        ),
    );
    let spd = ctx.eval_var(
        "Ability__Kestrel__B",
        rank,
        ValueSpec::var_scaled("MoveSpeedBonus", [1.0; 5], 0.0, 0.005, 0.0, 0.0),
    );
    ctx.set_stealth(ctx.caster, dur, spd, 0.7);
    let pos = ctx.caster_pos();
    let trap_dmg = ctx.eval_var(
        "Ability__Kestrel__B",
        rank,
        ValueSpec::dmg(0, [140.0, 230.0, 320.0, 410.0, 500.0], 1.2, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    ctx.spawn_effect(
        KitEffectKind::TrapZone {
            ability_slot: 1,
            armed_at: ctx.tick + seconds_to_ticks(1.2),
            end_tick: ctx.tick + seconds_to_ticks(5.2 + f32::from(rank)),
            trigger_radius: 3.0,
            blast_radius: 3.0,
            damage: trap_dmg,
            school: DamageSchool::Crystal,
            root_ticks: 0,
            stun_ticks: seconds_to_ticks(0.8),
            slow_pct: 0.0,
            reveals: true,
            detonate_on_caster_damage: true,
        },
        pos,
        pos,
    );
    Ok(true)
}

static KESTREL_C_EFFS: &[PrimitiveEffect] = &[PrimitiveEffect::SkillshotProjectile {
    damage: ValueSpec::var3_scaled("Damage", [600.0, 800.0, 1000.0], 1.8),
    school: DamageKind::Weapon,
    range: ValueSpec::constant(200.0),
    speed: ValueSpec::constant(45.0),
    radius: ValueSpec::constant(1.2),
    piercing: false,
    return_to_caster: false,
    repeat_hit_mult: 1.0,
    splash_damage: None,
    splash_radius: 0.0,
    stun_seconds: ValueSpec::zero(),
    root_seconds: ValueSpec::zero(),
    silence_seconds: ValueSpec::zero(),
    slow_pct: ValueSpec::zero(),
    slow_seconds: ValueSpec::zero(),
    minion_mult: 0.5,
    resets_cast_cd_on_hit: false,
    applies_mark: None,
    destroys_enemy_projectiles: false,
}];

static KESTREL: HeroKitSpec = HeroKitSpec {
    hero_id: "Kestrel",
    type_id: 267,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Adrenaline",
        on_basic_attack: Some(kestrel_on_basic_attack),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Kestrel__A",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::constant(0.3),
        range: ValueSpec::constant(10.0),
        cooldown: ValueSpec::var("RELOAD_TIME", [2.4; 5]),
        cost: ValueSpec::var("Energy Cost", [30.0, 35.0, 40.0, 45.0, 50.0]),
        charges: ValueSpec::var("Charges", [4.0; 5]),
        custom_exec: Some(kestrel_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Kestrel__B",
        targeting: CastTargeting::InstantSelf,
        max_rank: 5,
        range: ValueSpec::constant(3.0),
        cooldown: ValueSpec::var("Cooldown", [10.0, 10.0, 10.0, 10.0, 8.0]),
        cost: ValueSpec::var("Energy Cost", [100.0, 110.0, 120.0, 130.0, 140.0]),
        gate: CastGate::RequiresNotRecentlyDamaged,
        custom_exec: Some(kestrel_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Kestrel__C",
        targeting: CastTargeting::Directional,
        max_rank: 3,
        startup: ValueSpec::var3("StartupDuration", [0.7; 3]),
        range: ValueSpec::constant(200.0),
        cooldown: ValueSpec::var3("Cooldown", [50.0, 40.0, 30.0]),
        cost: ValueSpec::var3("Energy Cost", [140.0, 155.0, 170.0]),
        effects: KESTREL_C_EFFS,
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 25. Kinetic (tid=420)
// ============================================================================

fn kinetic_on_basic_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    _target: usize,
    _base: f32,
    _crit: bool,
) -> (f32, DamageSchool) {
    let stacks = ctx.mark_stacks(ctx.caster, marks::TRACER_SHOTS);
    if stacks > 0 {
        return (f32::from(stacks) * 18.0, DamageSchool::Crystal);
    }
    (0.0, DamageSchool::Weapon)
}

fn kinetic_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let charged = ctx.consume_mark(ctx.caster, marks::CHARGED_KINETIC) > 0;
    let range = 11.0 + if charged { 3.0 } else { 0.0 } + ctx.talent_mod("RANGE").unwrap_or(0.0);
    let mut dmg = ctx.eval_var(
        "Ability__Kinetic__A",
        rank,
        ValueSpec::dmg(0, [60.0, 100.0, 140.0, 180.0, 260.0], 1.4, 0.8, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    if charged {
        dmg *= 1.3 + 0.05 * f32::from(rank.saturating_sub(1));
    }
    let bolts = if ctx.talent_special() == TalentSpecial::KineticTripleBurstA {
        3
    } else {
        1
    };
    let aim = ctx.resolve_target_pos(target, range);
    if let Some(&j) = ctx.enemies_in_radius(aim, 2.5, false).first() {
        for _ in 0..bolts {
            ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
        }
        ctx.add_mark(ctx.caster, marks::TRACER_SHOTS, 1, 4, 6.0);
        if charged {
            ctx.apply_slow(j, 0.3 + 0.05 * f32::from(rank.saturating_sub(1)), 1.0);
            if rank >= 5 {
                ctx.apply_stun(j, 0.2);
            }
        }
        if ctx.talent_special() == TalentSpecial::KineticHitAReducesB {
            ctx.reduce_slot_cd(AbilitySlot::B, 2.0);
        }
    }
    Ok(true)
}

fn kinetic_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    _rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let dest = ctx.resolve_target_pos(target, 4.5);
    if ctx.room.spell_motion_clear(ctx.caster_pos(), dest) {
        ctx.room.place_spell_actor(ctx.caster, dest, ctx.events);
    }
    ctx.add_mark(ctx.caster, marks::CHARGED_KINETIC, 1, 1, 3.0);
    ctx.reset_slot_cd(AbilitySlot::A);
    Ok(true)
}

fn kinetic_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let stacks = ctx.consume_mark(ctx.caster, marks::TRACER_SHOTS);
    let base = ctx.eval_var(
        "Ability__Kinetic__C",
        rank,
        ValueSpec::var3_scaled("DAMAGE", [250.0, 400.0, 550.0], 1.2).with_wp(1.2),
    );
    let total = base * (1.0 + 0.25 * f32::from(stacks)) * ctx.talent_dmg_mult(slot);
    let aim = ctx.resolve_target_pos(target, 20.0);
    if let Some(&j) = ctx.enemies_in_radius(aim, 3.0, false).first() {
        ctx.deal_damage(j, total, DamageSchool::Crystal, 0.0, false);
    }
    Ok(true)
}

static KINETIC: HeroKitSpec = HeroKitSpec {
    hero_id: "Kinetic",
    type_id: 420,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Tracer Shots",
        on_basic_attack: Some(kinetic_on_basic_attack),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Kinetic__A",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        range: ValueSpec::var("RANGE", [11.0; 5]),
        cooldown: ValueSpec::var("COOLDOWN", [3.0, 2.8, 2.6, 2.4, 2.0]),
        cost: ValueSpec::var("ENERGY COST", [15.0, 20.0, 25.0, 30.0, 35.0]),
        custom_exec: Some(kinetic_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Kinetic__B",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        range: ValueSpec::var("RANGE", [4.5; 5]),
        cooldown: ValueSpec::var("COOLDOWN", [20.0, 18.0, 16.0, 14.0, 12.0]),
        cost: ValueSpec::var("ENERGY COST", [50.0, 55.0, 60.0, 65.0, 70.0]),
        custom_exec: Some(kinetic_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Kinetic__C",
        targeting: CastTargeting::Directional,
        max_rank: 3,
        startup: ValueSpec::constant(1.0),
        range: ValueSpec::var3("RANGE", [20.0; 3]),
        cooldown: ValueSpec::var3("COOLDOWN", [80.0, 70.0, 60.0]),
        cost: ValueSpec::var3("ENERGY COST", [100.0; 3]),
        custom_exec: Some(kinetic_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 26. Koshka (tid=245)
// ============================================================================

fn koshka_on_basic_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    _target: usize,
    _base: f32,
    _crit: bool,
) -> (f32, DamageSchool) {
    if ctx.talent_special() == TalentSpecial::KoshkaAttacksReduceCdr {
        let ra = ctx.talent_mod("COOLDOWN_REDUCTION_A").unwrap_or(0.1);
        let rb = ctx.talent_mod("COOLDOWN_REDUCTION_B").unwrap_or(0.15);
        let rc = ctx.talent_mod("COOLDOWN_REDUCTION_C").unwrap_or(0.05);
        ctx.reduce_slot_cd(AbilitySlot::A, ra * 6.0);
        ctx.reduce_slot_cd(AbilitySlot::B, rb * 6.0);
        ctx.reduce_slot_cd(AbilitySlot::C, rc * 60.0);
    }
    (0.0, DamageSchool::Weapon)
}

fn koshka_on_ability_damage(ctx: &mut HeroKitContext<'_, '_>, _target: usize, _dmg: f32) {
    ctx.add_buff(ctx.caster, BuffStatKind::MoveSpeedFlat, 0.75, 3.0);
}

fn koshka_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    ctx.reset_attack_cd();
    if let Some(j) = ctx.resolve_target_idx(target) {
        let dest = ctx.room.entities[j].position;
        if ctx.room.spell_motion_clear(ctx.caster_pos(), dest) {
            ctx.room.place_spell_actor(ctx.caster, dest, ctx.events);
        }
        let dmg = ctx.eval_var(
            "Ability__Koshka__A",
            rank,
            ValueSpec::dmg(0, [10.0, 40.0, 70.0, 100.0, 160.0], 0.8, 1.0, 0.0),
        ) * ctx.talent_dmg_mult(slot);
        ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
    }
    Ok(true)
}

fn koshka_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    _target: CastTarget,
) -> Result<bool, &'static str> {
    ctx.reset_attack_cd();
    let dmg = ctx.eval_var(
        "Ability__Koshka__B",
        rank,
        ValueSpec::dmg(0, [60.0, 100.0, 140.0, 180.0, 260.0], 0.8, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let hits = ctx.enemies_in_radius(ctx.caster_pos(), 3.9, false);
    let mut hero_hits = 0u8;
    for j in hits {
        if ctx.room.entities[j].kind == Kind::Hero {
            hero_hits += 1;
        }
        ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
    }
    if hero_hits > 0 {
        let per_hit = ctx.eval_var(
            "Ability__Koshka__B",
            rank,
            ValueSpec::var_scaled(
                "BARRIER_PER_HIT",
                [25.0, 45.0, 65.0, 85.0, 125.0],
                0.3,
                0.0,
                0.0,
                0.0,
            ),
        );
        let max_hp_bar = ctx
            .talent_mod("MAX_HEALTH_TO_BARRIER")
            .map_or(0.0, |p| ctx.room.entities[ctx.caster].max_hp * p);
        ctx.grant_barrier(
            ctx.caster,
            per_hit * f32::from(hero_hits) + max_hp_bar,
            2.0,
            None,
        );
    }
    let emp_dmg = ctx.eval_var(
        "Ability__Koshka__B",
        rank,
        ValueSpec::dmg(1, [40.0, 70.0, 100.0, 130.0, 190.0], 1.5, 0.3, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    ctx.empower_next_attacks(2, emp_dmg, true, 0.0, 0.0, 0.0, 0.0, 4.0);
    Ok(true)
}

fn koshka_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let tj = ctx.resolve_target_idx(target).ok_or("target required")?;
    let dest = ctx.room.entities[tj].position;
    if ctx.room.spell_motion_clear(ctx.caster_pos(), dest) {
        ctx.room.place_spell_actor(ctx.caster, dest, ctx.events);
    }
    let tid = ctx.room.entities[tj].id;
    let total_dmg = ctx.eval_var(
        "Ability__Koshka__C",
        rank,
        ValueSpec::dmg3(0, [350.0, 575.0, 800.0], 1.2, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let dur = 2.2;
    if ctx.talent_special() == TalentSpecial::KoshkaUltAoEStun {
        let rad = ctx.talent_mod("RADIUS").unwrap_or(3.0);
        for ej in ctx.enemies_in_radius(dest, rad, true) {
            ctx.apply_stun(ej, dur);
        }
    }
    ctx.spawn_effect(
        KitEffectKind::ChannelBeam {
            ability_slot: 2,
            target_id: tid,
            ticks_interval: 4,
            next_tick: ctx.tick,
            end_tick: ctx.tick + seconds_to_ticks(dur),
            range: 6.0,
            width: 1.5,
            tick_damage: total_dmg / dur,
            tick_heal: 0.0,
            school: DamageSchool::Crystal,
            lock_caster: true,
            slow_pct: 0.0,
            stun_target: true,
            global_all_heroes: false,
            shield_pierce: 0.0,
            structure_mult: 1.0,
            minion_mult: 1.0,
        },
        dest,
        dest,
    );
    Ok(true)
}

static KOSHKA: HeroKitSpec = HeroKitSpec {
    hero_id: "Koshka",
    type_id: 245,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Bloodrush",
        on_basic_attack: Some(koshka_on_basic_attack),
        on_ability_damage: Some(koshka_on_ability_damage),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Koshka__A",
        targeting: CastTargeting::TargetEnemy,
        max_rank: 5,
        range: ValueSpec::var("Range", [8.0; 5]),
        cooldown: ValueSpec::var("Cooldown", [6.0, 6.0, 6.0, 6.0, 5.5]),
        cost: ValueSpec::var("Energy Cost", [30.0, 35.0, 40.0, 45.0, 50.0]),
        custom_exec: Some(koshka_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Koshka__B",
        targeting: CastTargeting::InstantSelf,
        max_rank: 5,
        range: ValueSpec::constant(3.9),
        cooldown: ValueSpec::var("Cooldown", [6.0, 6.0, 6.0, 6.0, 5.0]),
        cost: ValueSpec::var("Energy Cost", [30.0, 40.0, 50.0, 60.0, 70.0]),
        custom_exec: Some(koshka_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Koshka__C",
        targeting: CastTargeting::TargetEnemy,
        max_rank: 3,
        range: ValueSpec::var3("Range", [6.0; 3]),
        cooldown: ValueSpec::var3("Cooldown", [80.0, 65.0, 50.0]),
        cost: ValueSpec::var3("Energy Cost", [120.0, 150.0, 180.0]),
        custom_exec: Some(koshka_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 27. Hero009 / Krul (tid=254)
// ============================================================================

fn krul_on_tick(ctx: &mut HeroKitContext<'_, '_>) {
    let stat = ctx.room.entities[ctx.caster]
        .hero_kit
        .as_deref()
        .map_or(0, |k| k.stationary_ticks);
    if u64::from(stat) == TICK_HZ + 10 {
        ctx.add_buff(ctx.caster, BuffStatKind::MoveSpeedFlat, 1.8, 2.5);
        ctx.empower_next_attacks(1, 0.0, false, 0.0, 0.4, 1.5, 0.0, 3.0);
    }
}

fn krul_on_basic_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    target_idx: usize,
    _base: f32,
    _crit: bool,
) -> (f32, DamageSchool) {
    let stacks = ctx.add_mark(target_idx, marks::WEAKNESS, 1, 8, 4.0);
    let ls_per = if ctx.room.entities[ctx.caster].ability_ranks[1] >= 5 {
        0.075
    } else {
        0.05
    };
    ctx.add_buff(
        ctx.caster,
        BuffStatKind::Lifesteal,
        ls_per * f32::from(stacks),
        4.0,
    );
    (0.0, DamageSchool::Weapon)
}

fn krul_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    if let Some(j) = ctx.resolve_target_idx(target) {
        let dest = ctx.room.entities[j].position;
        if ctx.room.spell_motion_clear(ctx.caster_pos(), dest) {
            ctx.room.place_spell_actor(ctx.caster, dest, ctx.events);
        }
        let dmg = ctx.eval_var(
            "Ability__Krul__A",
            rank,
            ValueSpec::dmg(0, [0.0; 5], 0.7, 1.4, 0.0),
        ) * ctx.talent_dmg_mult(slot);
        ctx.deal_damage(j, dmg, DamageSchool::Weapon, 0.0, true);
        ctx.add_mark(j, marks::WEAKNESS, 1, 8, 4.0);
    }
    let bar = ctx.eval_var(
        "Ability__Krul__A",
        rank,
        ValueSpec::var_scaled(
            "Barrier",
            [80.0, 200.0, 320.0, 440.0, 560.0],
            1.25,
            0.0,
            0.0,
            0.0,
        ),
    ) * ctx.talent_barrier_mult(slot);
    ctx.grant_barrier(ctx.caster, bar, 2.0, None);
    Ok(true)
}

fn krul_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    if let Some(j) = ctx.resolve_target_idx(target) {
        let stacks = ctx.consume_mark(j, marks::WEAKNESS);
        let base_dmg = ctx.eval_var(
            "Ability__Krul__B",
            rank,
            ValueSpec::var_scaled(
                "Damage",
                [60.0, 120.0, 180.0, 240.0, 360.0],
                0.9,
                0.0,
                0.0,
                0.0,
            ),
        );
        let stack_dmg = ctx.eval_var(
            "Ability__Krul__B",
            rank,
            ValueSpec::var_scaled(
                "DAMAGE_PER_STACK",
                [30.0, 45.0, 60.0, 75.0, 100.0],
                0.3,
                0.0,
                0.0,
                0.0,
            ),
        );
        let base_heal = ctx.eval_var(
            "Ability__Krul__B",
            rank,
            ValueSpec::var_scaled("Heal", [40.0, 60.0, 80.0, 100.0, 140.0], 0.4, 0.0, 0.0, 0.0),
        );
        let stack_heal = ctx.eval_var(
            "Ability__Krul__B",
            rank,
            ValueSpec::var_scaled(
                "HEAL_PER_STACK",
                [30.0, 35.0, 40.0, 45.0, 60.0],
                0.2,
                0.0,
                0.0,
                0.0,
            ),
        );
        let dmg = (base_dmg + stack_dmg * f32::from(stacks)) * ctx.talent_dmg_mult(slot);
        let heal = (base_heal + stack_heal * f32::from(stacks)) * ctx.talent_heal_mult(slot);
        ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
        ctx.heal_unit(ctx.caster, heal);
        if ctx.talent_special() == TalentSpecial::KrulSmiteHealsAllies {
            let rad = ctx.talent_mod("RADIUS").unwrap_or(9.0);
            for aj in ctx.allies_in_radius(ctx.caster_pos(), rad, false, true) {
                ctx.heal_unit(aj, heal);
            }
        }
    }
    Ok(true)
}

fn krul_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let apply_stacks = if ctx.talent_special() == TalentSpecial::KrulUltMaxWeakness {
        8
    } else {
        match rank {
            1 => 2,
            2 => 3,
            _ => 4,
        }
    };
    let dmg = ctx.eval_var(
        "Ability__Krul__C",
        rank,
        ValueSpec::var3_scaled("Damage", [250.0, 450.0, 650.0], 1.2),
    ) * ctx.talent_dmg_mult(slot);
    let stun = ctx.eval_var(
        "Ability__Krul__C",
        rank,
        ValueSpec::var3("Stun Duration", [1.6, 1.8, 2.0]),
    );
    let slow_dur = ctx.eval_var(
        "Ability__Krul__C",
        rank,
        ValueSpec::var3("Slow Duration", [2.6, 3.3, 4.0]),
    );
    let aim = ctx.resolve_target_pos(target, 12.0);
    if let Some(&j) = ctx.enemies_in_radius(aim, 2.5, true).first() {
        let dist_factor =
            (ctx.caster_pos().dist(ctx.room.entities[j].position) / 8.0).clamp(0.5, 1.5);
        ctx.deal_damage(j, dmg * dist_factor, DamageSchool::Crystal, 0.0, false);
        ctx.apply_stun(j, stun * dist_factor);
        ctx.apply_slow(j, 0.45, slow_dur);
        ctx.add_mark(j, marks::WEAKNESS, apply_stacks, 8, 4.0);
    }
    Ok(true)
}

static KRUL: HeroKitSpec = HeroKitSpec {
    hero_id: "Hero009",
    type_id: 254,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Shadows Empower Me",
        on_tick: Some(krul_on_tick),
        on_basic_attack: Some(krul_on_basic_attack),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Krul__A",
        targeting: CastTargeting::TargetEnemy,
        max_rank: 5,
        range: ValueSpec::constant(6.0),
        cooldown: ValueSpec::var("Cooldown", [10.0, 9.5, 9.0, 8.5, 7.0]),
        cost: ValueSpec::var("Energy Cost", [45.0, 50.0, 55.0, 60.0, 65.0]),
        custom_exec: Some(krul_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Krul__B",
        targeting: CastTargeting::TargetEnemy,
        max_rank: 5,
        range: ValueSpec::constant(3.2),
        cooldown: ValueSpec::var("Cooldown", [10.0, 10.0, 10.0, 10.0, 8.0]),
        cost: ValueSpec::var("Energy Cost", [45.0, 50.0, 55.0, 60.0, 65.0]),
        custom_exec: Some(krul_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Krul__C",
        targeting: CastTargeting::Directional,
        max_rank: 3,
        range: ValueSpec::constant(12.0),
        cooldown: ValueSpec::var3("Cooldown", [80.0, 70.0, 60.0]),
        cost: ValueSpec::var3("Energy Cost", [55.0, 70.0, 85.0]),
        custom_exec: Some(krul_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 28. Lance (tid=275)
// ============================================================================

fn lance_on_basic_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    _target: usize,
    _base: f32,
    _crit: bool,
) -> (f32, DamageSchool) {
    if ctx.talent_special() == TalentSpecial::LanceBasicReducesAC {
        let ra = ctx.talent_mod("COOLDOWN_REDUCTION_A").unwrap_or(0.3);
        let rc = ctx.talent_mod("COOLDOWN_REDUCTION_C").unwrap_or(0.2);
        ctx.reduce_slot_cd(AbilitySlot::A, ra * 5.0);
        ctx.reduce_slot_cd(AbilitySlot::C, rc * 4.0);
    }
    (0.0, DamageSchool::Weapon)
}

fn lance_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let dest = ctx.resolve_target_pos(target, 7.5);
    if ctx.room.spell_motion_clear(ctx.caster_pos(), dest) {
        ctx.room.place_spell_actor(ctx.caster, dest, ctx.events);
    }
    let mut dmg = ctx.eval_var(
        "Ability__Lance__A",
        rank,
        ValueSpec::dmg(0, [220.0, 290.0, 360.0, 430.0, 500.0], 0.8, 0.8, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    if let Some(imp) = ctx.talent_mod("IMPALE_DAMAGE_MODIFIER") {
        dmg *= imp;
    }
    let root = (ctx.eval_var(
        "Ability__Lance__A",
        rank,
        ValueSpec::var_scaled(
            "Immobilize Duration",
            [1.0, 1.0, 1.0, 1.0, 1.2],
            0.003,
            0.0,
            0.0,
            0.0,
        ),
    ))
    .min(1.6);
    let hits = ctx.enemies_in_radius(dest, 2.5, false);
    let hit_any = !hits.is_empty();
    for j in hits {
        ctx.deal_damage(j, dmg, DamageSchool::Weapon, 0.0, false);
        ctx.apply_root(j, root);
    }
    if hit_any && rank >= 5 {
        ctx.reset_slot_cd(AbilitySlot::C);
    }
    Ok(true)
}

fn lance_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    if rank >= 5 {
        ctx.set_unstoppable(ctx.caster, 2.0);
    }
    let dr = (0.6 + ctx.eval_inputs().cp * 0.0015).min(0.9);
    ctx.add_buff(ctx.caster, BuffStatKind::DamageReduction, dr, 2.0);
    if ctx.talent_special() == TalentSpecial::LanceWallGrantsAllyBarrier {
        let pct = ctx.talent_mod("MAX_HEALTH_TO_BARRIER").unwrap_or(0.15);
        let bar = ctx.room.entities[ctx.caster].max_hp * pct;
        for aj in ctx.allies_in_radius(ctx.caster_pos(), 8.0, true, true) {
            ctx.grant_barrier(aj, bar, 2.0, None);
        }
    }
    let dmg = ctx.eval_var(
        "Ability__Lance__B",
        rank,
        ValueSpec::var_scaled(
            "Damage",
            [80.0, 115.0, 150.0, 185.0, 220.0],
            0.8,
            0.6,
            0.0,
            0.0,
        ),
    ) * ctx.talent_dmg_mult(slot);
    let wall_stun = (ctx.eval_var(
        "Ability__Lance__B",
        rank,
        ValueSpec::var_scaled(
            "WallStunDuration",
            [0.6, 0.7, 0.8, 0.9, 1.2],
            0.002,
            0.0,
            0.0,
            0.0,
        ),
    ))
    .min(1.6);
    let aim = ctx.resolve_target_pos(target, 4.5);
    let origin = ctx.caster_pos();
    for j in ctx.enemies_in_radius(aim, 3.0, false) {
        ctx.deal_damage(j, dmg, DamageSchool::Weapon, 0.0, false);
        ctx.knockback(j, origin, 4.2, wall_stun);
    }
    Ok(true)
}

fn lance_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let dist = 5.0 + ctx.talent_mod("BONUS_RANGE").unwrap_or(0.0);
    let goal = ctx.resolve_target_pos(target, dist);
    if ctx.room.spell_motion_clear(ctx.caster_pos(), goal) {
        ctx.room.place_spell_actor(ctx.caster, goal, ctx.events);
    }
    if ctx.talent_special() == TalentSpecial::LanceRollDamagesPath {
        let mult = ctx.talent_mod("BASIC_ATTACK_TO_ROLL_DAMAGE").unwrap_or(1.2);
        let dmg = ctx.room.entities[ctx.caster].weapon_power() * mult;
        for j in ctx.enemies_in_radius(goal, 2.5, false) {
            ctx.deal_damage(j, dmg, DamageSchool::Weapon, 0.0, false);
        }
    }
    let bonus = ctx.eval_var(
        "Ability__Lance__C",
        rank,
        ValueSpec::dmg3(0, [100.0, 175.0, 250.0], 0.8, 0.3, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    ctx.empower_next_attacks(1, bonus, true, 0.0, 0.0, 0.0, 0.0, 2.0);
    Ok(true)
}

static LANCE: HeroKitSpec = HeroKitSpec {
    hero_id: "Lance",
    type_id: 275,
    resource: ResourceKind::Stamina,
    resource_max: 100.0,
    resource_initial: 100.0,
    resource_regen_per_sec: 12.0,
    resource_energy_conversion: (0.4, 0.4),
    perk: HeroicPerkSpec {
        id: "Partisan's Technique",
        attack_speed_efficiency: 0.75,
        basic_cleave_radius: 2.2,
        basic_cleave_fraction: 1.0,
        on_basic_attack: Some(lance_on_basic_attack),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Lance__A",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        range: ValueSpec::constant(7.5),
        cooldown: ValueSpec::var("Cooldown", [11.0, 10.0, 9.0, 8.0, 6.0]),
        cost: ValueSpec::var("Alt Energy Cost", [30.0; 5]),
        custom_exec: Some(lance_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Lance__B",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::constant(0.2),
        range: ValueSpec::constant(4.5),
        cooldown: ValueSpec::var("Cooldown", [9.0, 8.5, 8.0, 7.5, 7.0]),
        cost: ValueSpec::var("Alt Energy Cost", [30.0; 5]),
        custom_exec: Some(lance_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Lance__C",
        targeting: CastTargeting::Directional,
        max_rank: 3,
        range: ValueSpec::var3("Roll Distance", [5.0; 3]),
        cooldown: ValueSpec::var3("Cooldown", [5.0, 4.0, 3.0]),
        cost: ValueSpec::var3("Alt Energy Cost", [25.0, 20.0, 15.0]),
        custom_exec: Some(lance_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 29. Leo (tid=913)
// ============================================================================

fn leo_on_ability_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    _rank: u8,
    _target: CastTarget,
) {
    ctx.add_buff(ctx.caster, BuffStatKind::MoveSpeedFlat, 0.5, 3.0);
}

fn leo_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let emp = if ctx.talent_special() == TalentSpecial::LeoUltEmpowersBoth {
        ctx.mark_stacks(ctx.caster, marks::LEO_EMPOWERED) > 0
    } else {
        ctx.consume_mark(ctx.caster, marks::LEO_EMPOWERED) > 0
    };
    let dmg = ctx.eval_var(
        "Ability__Leo__A",
        rank,
        ValueSpec::dmg(0, [80.0, 160.0, 240.0, 320.0, 480.0], 0.0, 2.8, 0.0),
    ) * ctx.talent_dmg_mult(slot)
        * if emp { 1.2 } else { 1.0 };
    let aim = ctx.resolve_target_pos(target, 3.8);
    for j in ctx.enemies_in_radius(aim, 2.8, false) {
        ctx.deal_damage(j, dmg, DamageSchool::Weapon, 0.0, false);
        if emp {
            let tid = ctx.room.entities[j].id;
            ctx.spawn_effect(
                KitEffectKind::DoT {
                    ability_slot: 0,
                    target_id: tid,
                    tick_damage: dmg * 0.15,
                    school: DamageSchool::Weapon,
                    ticks_interval: TICK_HZ,
                    next_tick: ctx.tick + TICK_HZ,
                    end_tick: ctx.tick + TICK_HZ * 3,
                    minion_mult: 0.7,
                },
                ctx.room.entities[j].position,
                ctx.room.entities[j].position,
            );
        }
    }
    Ok(true)
}

fn leo_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let emp = if ctx.talent_special() == TalentSpecial::LeoUltEmpowersBoth {
        ctx.mark_stacks(ctx.caster, marks::LEO_EMPOWERED) > 0
    } else {
        ctx.consume_mark(ctx.caster, marks::LEO_EMPOWERED) > 0
    };
    let goal = ctx.resolve_target_pos(target, 8.0);
    if ctx.room.spell_motion_clear(ctx.caster_pos(), goal) {
        ctx.room.place_spell_actor(ctx.caster, goal, ctx.events);
    }
    let dmg = ctx.eval_var(
        "Ability__Leo__B",
        rank,
        ValueSpec::dmg(0, [60.0, 100.0, 140.0, 180.0, 220.0], 0.0, 2.0, 0.0),
    ) * ctx.talent_dmg_mult(slot)
        * if emp { 1.2 } else { 1.0 };
    let slow = (ctx.eval_var(
        "Ability__Leo__B",
        rank,
        ValueSpec::var_scaled("SLOW_STRENGTH", [0.1; 5], 0.0, 0.003, 0.0, 0.0),
    ))
    .min(0.75);
    let slow_dur = ctx.eval_var(
        "Ability__Leo__B",
        rank,
        ValueSpec::var("SLOW_DURATION", [1.5, 1.5, 1.5, 1.5, 2.0]),
    );
    let taunt = ctx.talent_special() == TalentSpecial::LeoCycloneTaunt;
    for j in ctx.enemies_in_radius(goal, 3.2, false) {
        ctx.deal_damage(j, dmg, DamageSchool::Weapon, 0.0, false);
        if emp {
            ctx.apply_root(j, slow_dur);
        } else {
            ctx.apply_slow(j, slow, slow_dur);
        }
        if taunt && ctx.room.entities[j].kind == Kind::Hero {
            ctx.apply_taunt(j, 1.2);
        }
    }
    Ok(true)
}

fn leo_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let nearby_heroes = ctx.enemies_in_radius(ctx.caster_pos(), 3.5, true);
    if !nearby_heroes.is_empty() {
        let total = ctx.eval_var(
            "Ability__Leo__C",
            rank,
            ValueSpec::var3_scaled("TOTAL_DAMAGE", [250.0, 375.0, 500.0], 0.0).with_wp(1.5),
        ) * ctx.talent_dmg_mult(slot);
        let origin = ctx.caster_pos();
        for j in nearby_heroes {
            ctx.knockback(j, origin, 3.5, 0.0);
            ctx.deal_damage(j, total, DamageSchool::Weapon, 0.0, false);
        }
    } else {
        let goal = ctx.resolve_target_pos(target, 6.0);
        if ctx.room.spell_motion_clear(ctx.caster_pos(), goal) {
            ctx.room.place_spell_actor(ctx.caster, goal, ctx.events);
        }
        let dmg = ctx.eval_var(
            "Ability__Leo__C",
            rank,
            ValueSpec::dmg3(0, [150.0, 225.0, 300.0], 0.0, 1.5, 0.0),
        ) * ctx.talent_dmg_mult(slot);
        for j in ctx.enemies_in_radius(goal, 3.2, false) {
            ctx.deal_damage(j, dmg, DamageSchool::Weapon, 0.0, false);
        }
    }
    let dur = ctx.talent_mod("BUFF_DURATION").unwrap_or(6.0);
    ctx.add_mark(ctx.caster, marks::LEO_EMPOWERED, 1, 1, dur);
    Ok(true)
}

static LEO: HeroKitSpec = HeroKitSpec {
    hero_id: "Leo",
    type_id: 913,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Turbo Blade",
        basic_cleave_radius: 2.2,
        basic_cleave_fraction: 0.75,
        on_ability_cast: Some(leo_on_ability_cast),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Leo__A",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::constant(0.6),
        range: ValueSpec::constant(3.8),
        cooldown: ValueSpec::var("COOLDOWN", [8.0, 7.5, 7.0, 6.5, 5.0]),
        cost: ValueSpec::var("ENERGY COST", [40.0, 45.0, 50.0, 55.0, 60.0]),
        custom_exec: Some(leo_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Leo__B",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::constant(0.6),
        range: ValueSpec::var("RANGE", [8.0; 5]),
        cooldown: ValueSpec::var("COOLDOWN", [10.0, 9.5, 9.0, 8.5, 8.0]),
        cost: ValueSpec::var("ENERGY COST", [40.0, 50.0, 60.0, 70.0, 80.0]),
        custom_exec: Some(leo_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Leo__C",
        targeting: CastTargeting::Directional,
        max_rank: 3,
        startup: ValueSpec::constant(0.4),
        range: ValueSpec::var3("RANGE", [6.0; 3]),
        cooldown: ValueSpec::var3("COOLDOWN", [80.0, 65.0, 50.0]),
        cost: ValueSpec::var3("ENERGY COST", [100.0, 125.0, 150.0]),
        custom_exec: Some(leo_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

static BATCH_A_SPECS: &[&HeroKitSpec] = &[
    &AMAEL,        // 0
    &ADAGIO,       // 1
    &ALPHA,        // 2
    &ANKA,         // 3
    &ARDAN,        // 4
    &BAPTISTE,     // 5
    &BARON,        // 6
    &BLACKFEATHER, // 7
    &CAINE,        // 8
    &CATHERINE,    // 9
    &CELESTE,      // 10
    &CHURNWALKER,  // 11
    &FLICKER,      // 12
    &FORTRESS,     // 13
    &GLAIVE,       // 14
    &GRACE,        // 15
    &GRUMPJAW,     // 16
    &GWEN,         // 17
    &IDRIS,        // 18
    &INARA,        // 19
    &ISHTAR,       // 20
    &JOULE,        // 21
    &KARAS,        // 22
    &KENSEI,       // 23
    &KESTREL,      // 24
    &KINETIC,      // 25
    &KOSHKA,       // 26
    &KRUL,         // 27 (Hero009)
    &LANCE,        // 28
    &LEO,          // 29
];

/// Complete slice of all 30 Batch A hero specifications (`Amael` ..= `Leo`).
pub(crate) fn specs() -> &'static [&'static HeroKitSpec] {
    BATCH_A_SPECS
}
