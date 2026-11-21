//! Warrior & brawler hero kit specifications (16 heroes).

#[allow(unused_imports)]
use super::{
    clamp_pos, has_nearby_active_zone, lerp_pos, move_toward_or_dash, spawn_single_hit_dot,
    unit_dir,
};
#[allow(unused_imports)]
use crate::combat::{AbilitySlot, CastTarget};
#[allow(unused_imports)]
use crate::ecs::score::DamageSchool;
#[allow(unused_imports)]
use crate::heroes::engine::{
    AbilityKitSpec, AbilityVariantSpec, BuffStatKind, CastGate, CastTargeting, DamageKind,
    HeroKitContext, HeroKitSpec, HeroSlot, HeroicPerkSpec, KitEffect, KitEffectKind, MarkKind,
    PrimitiveEffect, ResourceKind, ValueSpec, marks, seconds_to_ticks,
};
#[allow(unused_imports)]
use crate::heroes::talents::TalentSpecial;
#[allow(unused_imports)]
use crate::{Kind, Position, TICK_HZ};

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

pub(super) static AMAEL: HeroKitSpec = HeroKitSpec {
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

pub(super) static ALPHA: HeroKitSpec = HeroKitSpec {
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

pub(super) static BAPTISTE: HeroKitSpec = HeroKitSpec {
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

pub(super) static BLACKFEATHER: HeroKitSpec = HeroKitSpec {
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

pub(super) static GLAIVE: HeroKitSpec = HeroKitSpec {
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
        .query::<(bevy_ecs::entity::Entity, &crate::heroes::engine::KitEffect)>()
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
            .remove_spell_handle::<crate::heroes::engine::KitEffect>(eh);
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

pub(super) static GRUMPJAW: HeroKitSpec = HeroKitSpec {
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

pub(super) static JOULE: HeroKitSpec = HeroKitSpec {
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

pub(super) static KENSEI: HeroKitSpec = HeroKitSpec {
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

pub(super) static KRUL: HeroKitSpec = HeroKitSpec {
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

pub(super) static LEO: HeroKitSpec = HeroKitSpec {
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

// ============================================================================
// 35. Ozo (tid=274) — 3-step A combo, 3-hop targeted B bounce, C Bangarang
// ============================================================================

fn ozo_a_step(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let stage = ctx.room.entities[ctx.caster]
        .hero_kit
        .as_deref()
        .map_or(0, |k| k.step_stage[slot.idx()]);
    let next_stage = if stage >= 2 { 0 } else { stage + 1 };
    let win = ctx.eval_var(
        "Ability__Ozo__A1",
        rank,
        ValueSpec::var("ComboWindowDuration", [5.0; 5]),
    );
    let pos = ctx.resolve_target_pos(target, 4.5);
    move_toward_or_dash(ctx, ctx.caster_pos().advance(pos, 1.5));
    let dmg = ctx.eval_var(
        "Ability__Ozo__A1",
        rank,
        ValueSpec::dmg(0, [25.0, 35.0, 45.0, 55.0, 65.0], 0.75, 0.2, 1.0),
    ) * ctx.talent_dmg_mult(slot);
    let heal_per_hit = ctx.eval_var(
        "Ability__Ozo__A1",
        rank,
        ValueSpec::var_scaled(
            "HealPerHeroHit",
            [20.0, 40.0, 60.0, 80.0, 100.0],
            0.15,
            0.0,
            0.0,
            0.0,
        ),
    ) * ctx.talent_heal_mult(slot);
    let rad = if ctx.talent_special() == TalentSpecial::OzoCircusHealsAllies {
        ctx.talent_mod("RADIUS").unwrap_or(4.0)
    } else {
        2.8
    };
    let hits = ctx.enemies_in_radius(ctx.caster_pos(), rad, false);
    let mut total_heal = 0.0;
    for j in hits {
        let is_hero = ctx.room.entities[j].kind == Kind::Hero;
        ctx.deal_damage(j, dmg, DamageSchool::Weapon, 0.0, true);
        total_heal += if is_hero {
            heal_per_hit
        } else {
            heal_per_hit * 0.3
        };
    }
    if total_heal > 0.0 {
        if ctx.talent_special() == TalentSpecial::OzoCircusHealsAllies {
            for a in ctx.allies_in_radius(ctx.caster_pos(), rad, true, true) {
                ctx.heal_unit(a, total_heal);
            }
        } else {
            ctx.heal_unit(ctx.caster, total_heal);
        }
    }
    if next_stage > 0 {
        ctx.open_recast(AbilitySlot::A, next_stage, 0, win);
    } else {
        ctx.clear_recast(AbilitySlot::A);
    }
    Ok(true)
}

fn ozo_b_bounce_hop(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let stage = ctx.room.entities[ctx.caster]
        .hero_kit
        .as_deref()
        .map_or(0, |k| k.step_stage[slot.idx()]);
    let tj = ctx.resolve_target_idx(target).or_else(|| {
        let pos = ctx.resolve_target_pos(target, 7.0);
        ctx.enemies_in_radius(pos, 5.0, false).into_iter().next()
    });
    let dest = tj
        .map(|j| ctx.room.entities[j].position)
        .unwrap_or_else(|| ctx.resolve_target_pos(target, 7.0));
    move_toward_or_dash(ctx, dest);
    let fort = ctx.eval_var(
        "Ability__Ozo__B",
        rank,
        ValueSpec::var_scaled(
            "FortifiedHealthStrength",
            [25.0, 50.0, 75.0, 100.0, 125.0],
            0.15,
            0.0,
            0.0,
            0.0,
        ),
    );
    let fort_dur = ctx.eval_var(
        "Ability__Ozo__B",
        rank,
        ValueSpec::var("FortifiedHealthDuration", [1.5; 5]),
    );
    ctx.grant_fortified(ctx.caster, fort, fort_dur);

    let mult = ctx.talent_dmg_mult(slot);
    if stage < 2 {
        let dmg = ctx.eval_var(
            "Ability__Ozo__B",
            rank,
            ValueSpec::var_scaled(
                "BounceDamage",
                [60.0, 120.0, 180.0, 240.0, 300.0],
                0.7,
                0.0,
                0.0,
                0.0,
            ),
        ) * mult;
        let rad = ctx.eval_var(
            "Ability__Ozo__B",
            rank,
            ValueSpec::var("BounceRadius", [2.0; 5]),
        );
        let slow = ctx.eval_var(
            "Ability__Ozo__B",
            rank,
            ValueSpec::var("IntermediateBounceSlow", [0.6; 5]),
        );
        for j in ctx.enemies_in_radius(dest, rad.max(2.0), false) {
            ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
            ctx.apply_slow(j, slow, 1.0);
        }
        let target_id = tj.map_or(0, |j| ctx.room.entities[j].id);
        ctx.open_recast(AbilitySlot::B, stage + 1, target_id, 2.5);
    } else {
        // 3rd hop: FinalBounce AoE + MS boost.
        ctx.clear_recast(AbilitySlot::B);
        let fdmg = ctx.eval_var(
            "Ability__Ozo__B",
            rank,
            ValueSpec::var_scaled(
                "FinalDamage",
                [80.0, 160.0, 240.0, 320.0, 400.0],
                0.7,
                0.0,
                0.0,
                0.0,
            ),
        ) * mult;
        let frad = ctx.eval_var(
            "Ability__Ozo__B",
            rank,
            ValueSpec::var("FinalRadius", [3.9; 5]),
        );
        for j in ctx.enemies_in_radius(dest, frad, false) {
            ctx.deal_damage(j, fdmg, DamageSchool::Crystal, 0.0, false);
        }
        let spd = ctx.eval_var(
            "Ability__Ozo__B",
            rank,
            ValueSpec::var("FinalBounceSpeedBoost", [3.0; 5]),
        );
        let sdur = ctx.eval_var(
            "Ability__Ozo__B",
            rank,
            ValueSpec::var("SpeedBoostDuration", [2.0; 5]),
        );
        ctx.add_buff(ctx.caster, BuffStatKind::MoveSpeedFlat, spd, sdur);
        if ctx.talent_special() == TalentSpecial::OzoThirdBounceReset {
            ctx.reset_slot_cd(AbilitySlot::B);
        }
    }
    Ok(true)
}

fn ozo_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let origin = ctx.caster_pos();
    let dest = ctx.resolve_target_pos(target, 10.0);
    let secondary = ctx.eval_var(
        "Ability__Ozo__C",
        rank,
        ValueSpec::dmg3(1, [125.0, 200.0, 275.0], 0.75, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let primary_dmg = ctx.eval_var(
        "Ability__Ozo__C",
        rank,
        ValueSpec::dmg3(0, [250.0, 400.0, 550.0], 1.5, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let stun = ctx.eval_var(
        "Ability__Ozo__C",
        rank,
        ValueSpec::var3("StunDuration", [0.8; 3]),
    );
    let tj = ctx.resolve_target_idx(target);
    move_toward_or_dash(ctx, dest);
    for j in ctx.enemies_in_radius(lerp_pos(origin, dest, 0.5), 4.0, false) {
        if Some(j) == tj {
            ctx.deal_damage(j, primary_dmg, DamageSchool::Crystal, 0.0, false);
            ctx.apply_stun(j, stun);
            ctx.knockback(j, origin, 2.5, 0.0);
        } else {
            ctx.deal_damage(j, secondary, DamageSchool::Crystal, 0.0, false);
            ctx.knockback(j, ctx.caster_pos(), 2.5, 0.0);
        }
    }
    Ok(true)
}

static OZO_VARIANTS: &[AbilityVariantSpec] = &[
    AbilityVariantSpec {
        slot: HeroSlot::A2,
        spec: AbilityKitSpec {
            ability_id: "Ability__Ozo__A2",
            targeting: CastTargeting::TargetLocationOrEntity,
            max_rank: 5,
            cooldown: ValueSpec::zero(),
            cost: ValueSpec::zero(),
            custom_exec: Some(ozo_a_step),
            ..AbilityKitSpec::DEFAULT
        },
    },
    AbilityVariantSpec {
        slot: HeroSlot::B2,
        spec: AbilityKitSpec {
            ability_id: "Ability__Ozo__B_QueueBounce",
            targeting: CastTargeting::TargetLocationOrEntity,
            max_rank: 5,
            cooldown: ValueSpec::zero(),
            cost: ValueSpec::zero(),
            custom_exec: Some(ozo_b_bounce_hop),
            ..AbilityKitSpec::DEFAULT
        },
    },
];

pub(super) static OZO: HeroKitSpec = HeroKitSpec {
    hero_id: "Ozo",
    type_id: 274,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Perk__Ozo",
        healing_received_mult_base: 1.10,
        healing_received_mult_per_level: 0.015,
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Ozo__A1",
        targeting: CastTargeting::TargetEnemy,
        max_rank: 5,
        startup: ValueSpec::constant(0.15),
        range: ValueSpec::constant(4.5),
        cooldown: ValueSpec::var("Cooldown", [8.0, 7.5, 7.0, 6.5, 5.0]),
        cost: ValueSpec::var("Energy Cost", [50.0, 55.0, 60.0, 65.0, 75.0]),
        custom_exec: Some(ozo_a_step),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Ozo__B",
        targeting: CastTargeting::TargetEnemy,
        max_rank: 5,
        startup: ValueSpec::constant(0.15),
        range: ValueSpec::constant(7.0),
        cooldown: ValueSpec::var("Cooldown", [10.0, 9.5, 9.0, 8.5, 7.0]),
        cost: ValueSpec::var("Energy Cost", [100.0; 5]),
        custom_exec: Some(ozo_b_bounce_hop),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Ozo__C",
        targeting: CastTargeting::TargetEnemy,
        max_rank: 3,
        startup: ValueSpec::var3("ChargeDuration", [0.8; 3]),
        range: ValueSpec::constant(10.0),
        cooldown: ValueSpec::var3("Cooldown", [70.0, 50.0, 30.0]),
        cost: ValueSpec::var3("Energy Cost", [120.0; 3]),
        custom_exec: Some(ozo_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    variants: OZO_VARIANTS,
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 38. Reim (tid=266) — Frostguard + Winter Spire + Chill Winds + Valkyrie
// ============================================================================

fn reim_grant_chill_fortified(ctx: &mut HeroKitContext<'_, '_>, dealt: f32) {
    if dealt <= 0.0 {
        return;
    }
    let mut dur = 2.5;
    if ctx.talent_special() == TalentSpecial::ReimLongFortified {
        dur += 2.0;
    }
    ctx.grant_fortified(ctx.caster, dealt * 0.3, dur);
    if ctx.talent_special() == TalentSpecial::ReimChillBarrierSlowImmune {
        ctx.set_unstoppable(ctx.caster, 1.0);
    }
}

fn reim_perk_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    target_idx: usize,
    _base_damage: f32,
    _was_crit: bool,
) -> (f32, DamageSchool) {
    let cp = ctx.eval_inputs().cp;
    let lvl = f32::from(ctx.room.entities[ctx.caster].level);
    ctx.apply_slow(target_idx, 0.35, 2.0);
    let tick_dmg = 12.0 + lvl * 3.0 + cp * 0.3;
    spawn_single_hit_dot(
        ctx,
        AbilitySlot::A,
        target_idx,
        tick_dmg,
        DamageSchool::Crystal,
        2.0,
        1.0,
    );
    reim_grant_chill_fortified(ctx, tick_dmg);
    (12.0 + lvl * 2.0 + cp * 0.25, DamageSchool::Crystal)
}

fn reim_perk_ability_hit(ctx: &mut HeroKitContext<'_, '_>, _target_idx: usize, dealt: f32) {
    reim_grant_chill_fortified(ctx, dealt);
}

fn reim_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let center = ctx.resolve_target_pos(target, 14.0);
    let dmg = ctx.eval_var(
        "Ability__Reim__C",
        rank,
        ValueSpec::dmg3(0, [300.0, 450.0, 600.0], 1.8, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let inner = ctx.eval_var(
        "Ability__Reim__C",
        rank,
        ValueSpec::var3("Stun Radius", [2.0; 3]),
    );
    let outer = ctx.eval_var(
        "Ability__Reim__C",
        rank,
        ValueSpec::var3("AOE Radius", [3.5; 3]),
    );
    let stun = ctx.eval_var(
        "Ability__Reim__C",
        rank,
        ValueSpec::var3("Stun Duration", [0.9, 1.2, 1.5]),
    );
    let slow = ctx.eval_var(
        "Ability__Reim__C",
        rank,
        ValueSpec::var3("Slow Magnitude", [0.8; 3]),
    );
    let slow_dur = ctx.eval_var(
        "Ability__Reim__C",
        rank,
        ValueSpec::var3("Max Slow Duration", [4.0, 5.0, 6.0]),
    );
    for j in ctx.enemies_in_radius(center, outer, false) {
        let dist = ctx.room.entities[j].position.dist(center);
        let mult = if dist <= inner { 1.0 } else { 0.5 };
        ctx.deal_damage(j, dmg * mult, DamageSchool::Crystal, 0.0, false);
        if dist <= inner {
            ctx.apply_stun(j, stun);
        }
        ctx.apply_slow(j, slow, slow_dur);
    }
    if ctx.talent_special() == TalentSpecial::None
        && let Some(pct) = ctx.talent_mod("MAX_HEALTH_TO_BARRIER")
    {
        let bar = ctx.room.entities[ctx.caster].max_hp * pct * ctx.talent_barrier_mult(slot);
        let bdur = ctx.talent_mod("BARRIER_DURATION").unwrap_or(2.0);
        ctx.grant_barrier(ctx.caster, bar, bdur, None);
    }
    Ok(true)
}

const REIM_A_EFFECTS: &[PrimitiveEffect] = &[
    PrimitiveEffect::DelayedBlast {
        delay: ValueSpec::var("First Hit Delay", [0.5; 5]),
        radius: ValueSpec::var("Radius", [2.0; 5]),
        inner_radius: 0.0,
        damage: ValueSpec::dmg(0, [60.0, 100.0, 140.0, 180.0, 220.0], 0.8, 0.0, 0.0),
        outer_mult: 1.0,
        school: DamageKind::Crystal,
        stun_seconds: ValueSpec::zero(),
        root_seconds: ValueSpec::zero(),
        sleep_seconds: ValueSpec::zero(),
        slow_pct: ValueSpec::constant(0.35),
        slow_seconds: ValueSpec::constant(1.5),
        reveal_seconds: 0.0,
        bonus_if_marked: None,
        minion_mult: 0.75,
        applies_mark: None,
    },
    PrimitiveEffect::DelayedBlast {
        delay: ValueSpec::var("Second Hit Delay", [1.0; 5]),
        radius: ValueSpec::var("Radius", [2.0; 5]),
        inner_radius: 0.0,
        damage: ValueSpec::dmg(1, [80.0, 140.0, 200.0, 260.0, 320.0], 1.6, 0.0, 0.0),
        outer_mult: 1.0,
        school: DamageKind::Crystal,
        stun_seconds: ValueSpec::zero(),
        root_seconds: ValueSpec::zero(),
        sleep_seconds: ValueSpec::zero(),
        slow_pct: ValueSpec::constant(0.45),
        slow_seconds: ValueSpec::constant(2.0),
        reveal_seconds: 0.0,
        bonus_if_marked: None,
        minion_mult: 0.75,
        applies_mark: None,
    },
];

const REIM_B_EFFECTS: &[PrimitiveEffect] = &[PrimitiveEffect::DelayedBlast {
    delay: ValueSpec::zero(),
    radius: ValueSpec::var("Range", [4.5; 5]),
    inner_radius: 0.0,
    damage: ValueSpec::dmg(0, [60.0, 100.0, 140.0, 180.0, 220.0], 0.6, 0.0, 0.0),
    outer_mult: 1.0,
    school: DamageKind::Crystal,
    stun_seconds: ValueSpec::zero(),
    root_seconds: ValueSpec::var("B_RootDuration", [1.0, 1.0, 1.0, 1.0, 1.5]),
    sleep_seconds: ValueSpec::zero(),
    slow_pct: ValueSpec::zero(),
    slow_seconds: ValueSpec::zero(),
    reveal_seconds: 0.0,
    bonus_if_marked: None,
    minion_mult: 0.7,
    applies_mark: None,
}];

pub(super) static REIM: HeroKitSpec = HeroKitSpec {
    hero_id: "Reim",
    type_id: 266,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Perk__Reim",
        on_basic_attack: Some(reim_perk_attack),
        on_ability_damage: Some(reim_perk_ability_hit),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Reim__A",
        targeting: CastTargeting::GroundPoint,
        max_rank: 5,
        startup: ValueSpec::var("Lock Time", [0.3; 5]),
        range: ValueSpec::var("Range", [6.0, 6.0, 6.0, 6.0, 8.0]),
        cooldown: ValueSpec::var("Cooldown", [4.0, 3.5, 3.0, 2.5, 2.0]),
        cost: ValueSpec::var("Energy Cost", [30.0, 35.0, 40.0, 45.0, 50.0]),
        effects: REIM_A_EFFECTS,
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Reim__B",
        targeting: CastTargeting::InstantSelf,
        max_rank: 5,
        startup: ValueSpec::zero(),
        range: ValueSpec::var("Range", [4.5; 5]),
        cooldown: ValueSpec::var("Cooldown", [12.0, 11.5, 11.0, 10.5, 10.0]),
        cost: ValueSpec::var("Energy Cost", [60.0, 65.0, 70.0, 75.0, 80.0]),
        effects: REIM_B_EFFECTS,
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Reim__C",
        targeting: CastTargeting::GroundPoint,
        max_rank: 3,
        startup: ValueSpec::var3("Delay Before Spawn", [0.6; 3]),
        range: ValueSpec::constant(14.0),
        cooldown: ValueSpec::var3("Cooldown", [90.0, 75.0, 60.0]),
        cost: ValueSpec::var3("Energy Cost", [100.0, 125.0, 150.0]),
        custom_exec: Some(reim_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 41. Hero016 / Rona (tid=260) — Bloodrage resource + Cleave basics + Red Mist
// ============================================================================

fn rona_perk_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    _target_idx: usize,
    _base_damage: f32,
    _was_crit: bool,
) -> (f32, DamageSchool) {
    ctx.add_resource(10.0);
    if ctx.talent_special() == TalentSpecial::RonaAttacksReduceA {
        ctx.reduce_slot_cd(AbilitySlot::A, 1.0);
    }
    ctx.reduce_slot_cd(AbilitySlot::B, 1.0);
    (0.0, DamageSchool::Weapon)
}

fn rona_perk_damaged(
    ctx: &mut HeroKitContext<'_, '_>,
    _source: Option<usize>,
    _amount: f32,
    _school: DamageSchool,
) -> f32 {
    ctx.add_resource(5.0);
    1.0
}

fn rona_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let range = ctx.eval_var("Ability__Rona__A", rank, ValueSpec::var("Range", [8.0; 5]));
    let dest = ctx.resolve_target_pos(target, range);
    move_toward_or_dash(ctx, dest);
    let impact_dmg = ctx.eval_var(
        "Ability__Rona__A",
        rank,
        ValueSpec::dmg(0, [60.0, 85.0, 110.0, 135.0, 185.0], 1.0, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let rage_hit = ctx.eval_var(
        "Ability__Rona__A",
        rank,
        ValueSpec::var("RagePerHeroHit", [25.0, 25.0, 25.0, 25.0, 35.0]),
    );
    let mut hero_hits = 0u32;
    for j in ctx.enemies_in_radius(dest, 2.2, false) {
        if ctx.room.entities[j].kind == Kind::Hero {
            hero_hits += 1;
        }
        ctx.deal_damage(j, impact_dmg, DamageSchool::Weapon, 0.0, false);
    }
    if hero_hits > 0 {
        ctx.add_resource(rage_hit * hero_hits as f32);
        let fort = ctx.eval_var(
            "Ability__Rona__A",
            rank,
            ValueSpec::var(
                "Fortified_Health_Strength",
                [50.0, 70.0, 90.0, 110.0, 150.0],
            ),
        );
        ctx.grant_fortified(ctx.caster, fort * hero_hits as f32, 4.0);
    }
    let rup_dmg = ctx.eval_var(
        "Ability__Rona__A",
        rank,
        ValueSpec::dmg(1, [100.0, 140.0, 180.0, 220.0, 300.0], 2.0, 0.0, 0.0),
    );
    ctx.spawn_effect(
        KitEffectKind::GroundZone {
            ability_slot: slot.idx() as u8,
            zone_kind: 41,
            radius: 2.5,
            ticks_interval: TICK_HZ / 2,
            next_tick: ctx.tick + seconds_to_ticks(0.8),
            end_tick: ctx.tick + seconds_to_ticks(3.8),
            tick_damage: rup_dmg / 3.0,
            tick_heal: 0.0,
            school: DamageSchool::Weapon,
            slow_pct: 0.4,
            ally_speed_bonus: 0.0,
            end_damage: 0.0,
            end_root_ticks: 0,
            end_stun_ticks: 0,
            drift_dx: 0.0,
            drift_dy: 0.0,
            follow_caster: false,
            ignited: false,
            minion_mult: 0.65,
            applies_mark: None,
        },
        dest,
        dest,
    );
    Ok(true)
}

fn rona_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let tj = ctx.resolve_target_idx(target);
    if let Some(j) = tj {
        move_toward_or_dash(
            ctx,
            ctx.caster_pos().advance(ctx.room.entities[j].position, 2.5),
        );
        let dmg = ctx.eval_var(
            "Ability__Rona__B",
            rank,
            ValueSpec::dmg(0, [10.0, 20.0, 30.0, 40.0, 70.0], 1.0, 0.0, 0.9),
        ) * ctx.talent_dmg_mult(slot);
        ctx.deal_damage(j, dmg, DamageSchool::Weapon, 0.0, false);
        let rage = ctx.eval_var(
            "Ability__Rona__B",
            rank,
            ValueSpec::var("RageGainOnHit", [5.0, 10.0, 15.0, 20.0, 25.0]),
        );
        ctx.add_resource(rage);
    }
    ctx.add_buff(ctx.caster, BuffStatKind::MoveSpeedFlat, 1.5, 3.0);
    let win = ctx.eval_var(
        "Ability__Rona__B",
        rank,
        ValueSpec::var("SecondHitWindow", [3.0; 5]),
    );
    let target_id = tj.map_or(0, |j| ctx.room.entities[j].id);
    ctx.open_recast(AbilitySlot::B, 1, target_id, win);
    Ok(true)
}

fn rona_b_followup(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    ctx.clear_recast(AbilitySlot::B);
    let rage_frac = (ctx.resource() / ctx.resource_max().max(1.0)).clamp(0.25, 1.0);
    let spent = ctx.resource();
    ctx.spend_resource(spent);
    let max_dmg = ctx.eval_var(
        "Ability__Rona__B_FollowUp",
        rank,
        ValueSpec::var_scaled(
            "SecondHitMaxDamage",
            [20.0, 40.0, 60.0, 80.0, 140.0],
            2.0,
            1.8,
            0.0,
            0.0,
        ),
    ) * ctx.talent_dmg_mult(slot);
    if let Some(tj) = ctx.resolve_target_idx(target).or_else(|| {
        ctx.enemies_in_radius(ctx.caster_pos(), 4.5, false)
            .into_iter()
            .next()
    }) {
        ctx.deal_damage(tj, max_dmg * rage_frac, DamageSchool::Weapon, 0.0, false);
        ctx.apply_mortal_wound(tj, 3.0);
    }
    let aspd = ctx.eval_var(
        "Ability__Rona__B_FollowUp",
        rank,
        ValueSpec::var("BonusASPDStrength", [0.25, 0.3, 0.35, 0.4, 0.5]),
    );
    let adur = ctx.eval_var(
        "Ability__Rona__B_FollowUp",
        rank,
        ValueSpec::var("Bonus_ASPD_Duration", [2.0, 2.5, 3.0, 3.5, 4.0]),
    );
    ctx.add_buff(ctx.caster, BuffStatKind::AttackSpeed, aspd, adur);
    Ok(true)
}

fn rona_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    _target: CastTarget,
) -> Result<bool, &'static str> {
    let dps = ctx.eval_var(
        "Ability__Rona__C",
        rank,
        ValueSpec::dmg3(0, [250.0, 350.0, 450.0], 1.2, 1.7, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let fort = ctx.eval_var(
        "Ability__Rona__C",
        rank,
        ValueSpec::var3("FortifiedHealthStrength", [50.0, 100.0, 150.0]),
    );
    let rad = ctx.eval_var(
        "Ability__Rona__C",
        rank,
        ValueSpec::var3("DamageRadius", [1.9; 3]),
    );
    ctx.grant_fortified(ctx.caster, fort, 3.5);
    ctx.set_unstoppable(ctx.caster, 3.0);
    if ctx.talent_special() == TalentSpecial::RonaWhirlwindMoveSpeed {
        let ms = ctx.talent_mod("BONUS_MOVESPEED_STRENGTH").unwrap_or(1.2);
        ctx.add_buff(ctx.caster, BuffStatKind::MoveSpeedFlat, ms, 3.5);
    }
    let spend = (ctx.resource()).min(75.0);
    ctx.spend_resource(spend);
    let pos = ctx.caster_pos();
    ctx.spawn_effect(
        KitEffectKind::GroundZone {
            ability_slot: slot.idx() as u8,
            zone_kind: 42,
            radius: rad.max(2.3),
            ticks_interval: TICK_HZ / 2,
            next_tick: ctx.tick,
            end_tick: ctx.tick + seconds_to_ticks(3.5),
            tick_damage: dps,
            tick_heal: 0.0,
            school: DamageSchool::Weapon,
            slow_pct: 0.0,
            ally_speed_bonus: 0.0,
            end_damage: 0.0,
            end_root_ticks: 0,
            end_stun_ticks: 0,
            drift_dx: 0.0,
            drift_dy: 0.0,
            follow_caster: true,
            ignited: false,
            minion_mult: 0.8,
            applies_mark: None,
        },
        pos,
        pos,
    );
    Ok(true)
}

static RONA_VARIANTS: &[AbilityVariantSpec] = &[AbilityVariantSpec {
    slot: HeroSlot::B2,
    spec: AbilityKitSpec {
        ability_id: "Ability__Rona__B_FollowUp",
        targeting: CastTargeting::TargetLocationOrEntity,
        max_rank: 5,
        cooldown: ValueSpec::zero(),
        cost: ValueSpec::zero(),
        custom_exec: Some(rona_b_followup),
        ..AbilityKitSpec::DEFAULT
    },
}];

pub(super) static RONA: HeroKitSpec = HeroKitSpec {
    hero_id: "Hero016",
    type_id: 260,
    resource: ResourceKind::Bloodrage,
    resource_max: 100.0,
    resource_initial: 0.0,
    resource_regen_per_sec: -2.0,
    perk: HeroicPerkSpec {
        id: "Perk__Rona",
        basic_cleave_radius: 2.0,
        basic_cleave_fraction: 0.5,
        on_basic_attack: Some(rona_perk_attack),
        on_take_damage: Some(rona_perk_damaged),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Rona__A",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::constant(0.15),
        range: ValueSpec::var("Range", [8.0; 5]),
        cooldown: ValueSpec::var("Cooldown", [12.0, 11.5, 11.0, 10.5, 10.0]),
        cost: ValueSpec::zero(),
        custom_exec: Some(rona_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Rona__B",
        targeting: CastTargeting::TargetEnemy,
        max_rank: 5,
        startup: ValueSpec::constant(0.1),
        range: ValueSpec::var("Range", [5.0; 5]),
        cooldown: ValueSpec::var("Cooldown", [14.0, 14.0, 14.0, 14.0, 12.0]),
        cost: ValueSpec::zero(),
        custom_exec: Some(rona_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Rona__C",
        targeting: CastTargeting::InstantSelf,
        max_rank: 3,
        startup: ValueSpec::constant(0.1),
        range: ValueSpec::var3("DamageRadius", [1.9; 3]),
        cooldown: ValueSpec::var3("Cooldown", [10.0; 3]),
        cost: ValueSpec::zero(),
        custom_exec: Some(rona_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    variants: RONA_VARIANTS,
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 43. Sanfeng (tid=446)
// ============================================================================

fn sanfeng_perk_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    _rank: u8,
    _target: CastTarget,
) {
    let lvl = f32::from(ctx.room.entities[ctx.caster].level);
    ctx.grant_fortified(ctx.caster, 50.0 + lvl * 25.0, 3.0);
}

fn sanfeng_perk_damaged(
    ctx: &mut HeroKitContext<'_, '_>,
    _src: Option<usize>,
    _amount: f32,
    _school: DamageSchool,
) -> f32 {
    let in_stance = ctx.room.entities[ctx.caster]
        .hero_kit
        .as_deref()
        .is_some_and(|k| ctx.tick < k.timers[0]);
    if in_stance {
        ctx.add_mark(ctx.caster, marks::STANCE_COUNTER, 1, 1, 2.0);
        0.5
    } else {
        1.0
    }
}

fn sanfeng_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    _target: CastTarget,
) -> Result<bool, &'static str> {
    let stance = ctx.eval_var(
        "Ability__Sanfeng__A",
        rank,
        ValueSpec::var("STANCE_DURATION", [0.9; 5]),
    );
    if rank >= 5 {
        ctx.set_unstoppable(ctx.caster, 2.0);
    }
    {
        let k = ctx.room.entities[ctx.caster]
            .hero_kit
            .get_or_insert_with(Box::default);
        k.timers[0] = ctx.tick + seconds_to_ticks(stance);
    }
    let dmg = ctx.eval_var(
        "Ability__Sanfeng__A",
        rank,
        ValueSpec::dmg(0, [35.0, 80.0, 125.0, 170.0, 260.0], 0.8, 0.0, 1.0),
    ) * ctx.talent_dmg_mult(slot);
    let countered = ctx.consume_mark(ctx.caster, marks::STANCE_COUNTER) > 0;
    let stun = ctx.eval_var(
        "Ability__Sanfeng__A",
        rank,
        ValueSpec::var("STUN_DURATION", [0.6, 0.7, 0.8, 0.9, 1.0]),
    );
    for j in ctx.enemies_in_radius(ctx.caster_pos(), 3.2, false) {
        let final_dmg = if countered { dmg * 1.5 } else { dmg };
        ctx.deal_damage(j, final_dmg, DamageSchool::Weapon, 0.0, false);
        ctx.apply_stun(j, stun);
    }
    if ctx.talent_special() == TalentSpecial::SanfengCounterRefreshesA && countered {
        ctx.reset_slot_cd(AbilitySlot::A);
    }
    Ok(true)
}

fn sanfeng_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let dmg = ctx.eval_var(
        "Ability__Sanfeng__B",
        rank,
        ValueSpec::dmg(0, [200.0, 350.0, 500.0, 650.0, 800.0], 0.6, 0.4, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let range = ctx.eval_var(
        "Ability__Sanfeng__B",
        rank,
        ValueSpec::var("RANGE", [9.0; 5]),
    );
    let aim = ctx.resolve_target_pos(target, range);
    let slow_dur = if ctx.talent_special() == TalentSpecial::SanfengDivineFistSlow {
        ctx.talent_mod("SLOW_DURATION").unwrap_or(1.4)
    } else {
        0.0
    };
    for j in ctx.enemies_in_radius(aim, 3.2, false) {
        ctx.deal_damage(j, dmg, DamageSchool::Weapon, 0.0, false);
        if slow_dur > 0.0 {
            ctx.apply_slow(j, 0.45, slow_dur);
        }
    }
    Ok(true)
}

const SANFENG_C_EFFECTS: &[PrimitiveEffect] = &[PrimitiveEffect::ConeWave {
    range: ValueSpec::var3("DAMAGE_RADIUS", [6.5; 3]),
    half_angle_deg: 180.0,
    damage: ValueSpec::dmg3(0, [400.0, 600.0, 800.0], 0.6, 0.4, 0.0),
    school: DamageKind::Weapon,
    multi_bolts: 1,
    extra_hit_mult: 1.0,
    slow_pct: ValueSpec::zero(),
    slow_seconds: ValueSpec::zero(),
    stun_seconds: ValueSpec::var3("STUN_DURATION", [1.6; 3]),
    fear_seconds: ValueSpec::zero(),
    silence_seconds: ValueSpec::zero(),
    knockback_dist: 3.0,
    reveal_seconds: 0.0,
    minion_mult: 0.75,
    applies_mark: None,
}];

pub(super) static SANFENG: HeroKitSpec = HeroKitSpec {
    hero_id: "Sanfeng",
    type_id: 446,
    resource: ResourceKind::None,
    perk: HeroicPerkSpec {
        id: "Perk__Sanfeng",
        on_ability_cast: Some(sanfeng_perk_cast),
        on_take_damage: Some(sanfeng_perk_damaged),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Sanfeng__A",
        targeting: CastTargeting::InstantSelf,
        max_rank: 5,
        startup: ValueSpec::constant(0.1),
        range: ValueSpec::constant(3.2),
        cooldown: ValueSpec::var("COOLDOWN", [10.0; 5]),
        cost: ValueSpec::zero(),
        custom_exec: Some(sanfeng_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Sanfeng__B",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::constant(0.5),
        range: ValueSpec::var("RANGE", [9.0; 5]),
        cooldown: ValueSpec::var("COOLDOWN", [10.0; 5]),
        cost: ValueSpec::zero(),
        custom_exec: Some(sanfeng_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Sanfeng__C",
        targeting: CastTargeting::InstantSelf,
        max_rank: 3,
        startup: ValueSpec::constant(0.6),
        range: ValueSpec::var3("DAMAGE_RADIUS", [6.5; 3]),
        cooldown: ValueSpec::var3("COOLDOWN", [100.0, 80.0, 60.0]),
        cost: ValueSpec::zero(),
        effects: SANFENG_C_EFFECTS,
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 45. Shin (tid=927)
// ============================================================================

fn shin_add_heat(ctx: &mut HeroKitContext<'_, '_>, amount: u8) {
    let stacks = ctx.add_mark(ctx.caster, marks::HEAT, amount, 4, 6.0);
    if stacks > 0 {
        ctx.add_buff(
            ctx.caster,
            BuffStatKind::MoveSpeedFlat,
            0.35 * f32::from(stacks),
            4.0,
        );
    }
}

fn shin_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    if slot == AbilitySlot::A
        && ctx.room.entities[ctx.caster]
            .hero_kit
            .as_ref()
            .is_some_and(|k| k.step_stage[0] == 1 && ctx.tick < k.recast_until[0])
    {
        ctx.clear_recast(AbilitySlot::A);
        let range = ctx.eval_var(
            "Ability__Shin__A1",
            rank,
            ValueSpec::var("Range", [8.0, 8.0, 8.0, 8.0, 9.0]),
        );
        let goal = ctx.resolve_target_pos(target, range);
        let dmg = ctx.eval_var(
            "Ability__Shin__A",
            rank,
            ValueSpec::var_scaled(
                "EmpDamage",
                [150.0, 192.0, 234.0, 276.0, 360.0],
                1.5,
                0.0,
                0.0,
                0.0,
            ),
        ) * 0.65
            * ctx.talent_dmg_mult(AbilitySlot::A);
        let origin = ctx.caster_pos();
        let (dx, dy) = unit_dir(origin, goal);
        let end = clamp_pos(Position {
            x: origin.x + dx * range,
            y: origin.y + dy * range,
        });
        ctx.spawn_effect(
            KitEffectKind::Projectile {
                ability_slot: 0,
                rank,
                speed_per_tick: 22.0 / TICK_HZ as f32,
                radius: 1.4,
                damage: dmg,
                school: DamageSchool::Crystal,
                piercing: true,
                returning: false,
                return_to_caster: false,
                repeat_mult: 1.0,
                splash_damage: 0.0,
                splash_radius: 0.0,
                stun_ticks: 0,
                root_ticks: 0,
                silence_ticks: 0,
                slow_pct: 0.0,
                slow_ticks: 0,
                minion_mult: 0.7,
                resets_cd_on_hit: false,
                applies_mark: None,
                destroys_projectiles: false,
                hit_actors: Vec::new(),
            },
            origin,
            end,
        );
        return Ok(true);
    }

    let empowered = ctx.mark_stacks(ctx.caster, marks::HEAT) >= 4;
    if empowered {
        ctx.consume_mark(ctx.caster, marks::HEAT);
    }
    let range = if empowered {
        ctx.eval_var(
            "Ability__Shin__A",
            rank,
            ValueSpec::var("BonusRange", [8.0, 8.0, 8.0, 8.0, 9.0]),
        )
    } else {
        ctx.eval_var(
            "Ability__Shin__A",
            rank,
            ValueSpec::var("Range", [7.0, 7.0, 7.0, 7.0, 8.0]),
        )
    } + ctx.talent_mod("BONUS_RANGE").unwrap_or(0.0);
    let origin = ctx.caster_pos();
    let goal = ctx.resolve_target_pos(target, range);
    let (dx, dy) = unit_dir(origin, goal);
    let end = clamp_pos(Position {
        x: origin.x + dx * range,
        y: origin.y + dy * range,
    });
    let dmg = if empowered {
        ctx.eval_var(
            "Ability__Shin__A",
            rank,
            ValueSpec::var_scaled(
                "EmpDamage",
                [150.0, 192.0, 234.0, 276.0, 360.0],
                1.5,
                0.0,
                0.0,
                0.0,
            ),
        )
    } else {
        ctx.eval_var(
            "Ability__Shin__A",
            rank,
            ValueSpec::dmg(0, [110.0, 150.0, 190.0, 230.0, 300.0], 1.23, 0.0, 0.0),
        )
    } * ctx.talent_dmg_mult(AbilitySlot::A);

    ctx.spawn_effect(
        KitEffectKind::Projectile {
            ability_slot: 0,
            rank,
            speed_per_tick: 20.0 / TICK_HZ as f32,
            radius: 1.5,
            damage: dmg,
            school: DamageSchool::Crystal,
            piercing: true,
            returning: false,
            return_to_caster: false,
            repeat_mult: 1.0,
            splash_damage: 0.0,
            splash_radius: 0.0,
            stun_ticks: 0,
            root_ticks: 0,
            silence_ticks: 0,
            slow_pct: 0.0,
            slow_ticks: 0,
            minion_mult: 0.7,
            resets_cd_on_hit: false,
            applies_mark: None,
            destroys_projectiles: false,
            hit_actors: Vec::new(),
        },
        origin,
        end,
    );
    if empowered {
        ctx.open_recast(AbilitySlot::A, 1, 0, 3.5);
    } else {
        shin_add_heat(ctx, 1);
    }
    Ok(true)
}

fn shin_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    if slot == AbilitySlot::B
        && ctx.room.entities[ctx.caster]
            .hero_kit
            .as_ref()
            .is_some_and(|k| k.step_stage[1] == 1 && ctx.tick < k.recast_until[1])
    {
        ctx.clear_recast(AbilitySlot::B);
        let range = 6.0 + ctx.talent_mod("BONUS_RANGE").unwrap_or(0.0);
        let pos = ctx.resolve_target_pos(target, range);
        let root_dur = ctx.eval_var(
            "Ability__Shin__B1",
            rank,
            ValueSpec::var("Root_Duration", [0.7, 0.7, 0.7, 0.7, 1.0]),
        );
        let dmg = ctx.eval_var(
            "Ability__Shin__B",
            rank,
            ValueSpec::var_scaled(
                "EmpDamage",
                [100.0, 133.0, 166.0, 199.0, 232.0],
                1.5,
                0.0,
                0.0,
                0.0,
            ),
        ) * 0.55
            * ctx.talent_dmg_mult(AbilitySlot::B);
        for j in ctx.enemies_in_radius(pos, 2.8, false) {
            ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
            ctx.apply_root(j, root_dur);
        }
        return Ok(true);
    }

    let empowered = ctx.mark_stacks(ctx.caster, marks::HEAT) >= 4;
    if empowered {
        ctx.consume_mark(ctx.caster, marks::HEAT);
    }
    let range = 6.0 + ctx.talent_mod("BONUS_RANGE").unwrap_or(0.0);
    let pos = ctx.resolve_target_pos(target, range);
    let dmg = if empowered {
        ctx.eval_var(
            "Ability__Shin__B",
            rank,
            ValueSpec::var_scaled(
                "EmpDamage",
                [100.0, 133.0, 166.0, 199.0, 232.0],
                1.5,
                0.0,
                0.0,
                0.0,
            ),
        )
    } else {
        ctx.eval_var(
            "Ability__Shin__B",
            rank,
            ValueSpec::dmg(0, [80.0, 110.0, 140.0, 170.0, 200.0], 1.0, 0.0, 0.0),
        )
    } * ctx.talent_dmg_mult(AbilitySlot::B);
    let slow = ctx.eval_var(
        "Ability__Shin__B",
        rank,
        ValueSpec::var("B_Slow", [0.3, 0.325, 0.35, 0.375, 0.4]),
    );
    let slow_dur = ctx.eval_var(
        "Ability__Shin__B",
        rank,
        ValueSpec::var("B_Slow_Durartion", [3.0, 3.0, 3.0, 3.0, 3.5]),
    );
    let root_dur = ctx.eval_var(
        "Ability__Shin__B",
        rank,
        ValueSpec::var("Root_Duration", [0.7, 0.7, 0.7, 0.7, 1.0]),
    );
    for j in ctx.enemies_in_radius(pos, 2.8, false) {
        ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
        ctx.apply_slow(j, slow, slow_dur);
        if empowered {
            ctx.apply_root(j, root_dur);
        }
    }
    if empowered {
        ctx.open_recast(AbilitySlot::B, 1, 0, 3.0);
    } else {
        shin_add_heat(ctx, 1);
    }
    Ok(true)
}

fn shin_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    if slot == AbilitySlot::C
        && ctx.room.entities[ctx.caster]
            .hero_kit
            .as_ref()
            .is_some_and(|k| k.step_stage[2] == 1 && ctx.tick < k.recast_until[2])
    {
        ctx.clear_recast(AbilitySlot::C);
        let pos = ctx.resolve_target_pos(target, 7.5);
        move_toward_or_dash(ctx, pos);
        let dmg = ctx.eval_var(
            "Ability__Shin__C",
            rank,
            ValueSpec::var_scaled(
                "DisplayDamage",
                [200.0, 250.0, 300.0, 300.0, 300.0],
                1.0,
                0.0,
                0.0,
                0.0,
            ),
        ) * ctx.talent_dmg_mult(AbilitySlot::C);
        for j in ctx.enemies_in_radius(pos, 3.2, false) {
            ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
        }
        return Ok(true);
    }

    let empowered = ctx.mark_stacks(ctx.caster, marks::HEAT) >= 4;
    if empowered {
        ctx.consume_mark(ctx.caster, marks::HEAT);
    }
    let pos = ctx.resolve_target_pos(target, 7.0);
    move_toward_or_dash(ctx, pos);
    let dmg = if empowered {
        ctx.eval_var(
            "Ability__Shin__C",
            rank,
            ValueSpec::var_scaled(
                "EmpDamage",
                [900.0, 1050.0, 1200.0, 1200.0, 1200.0],
                1.2,
                0.0,
                0.0,
                0.0,
            ),
        )
    } else {
        ctx.eval_var(
            "Ability__Shin__C",
            rank,
            ValueSpec::var_scaled(
                "DisplayDamage",
                [200.0, 250.0, 300.0, 300.0, 300.0],
                1.0,
                0.0,
                0.0,
                0.0,
            ),
        )
    } * ctx.talent_dmg_mult(AbilitySlot::C);
    for j in ctx.enemies_in_radius(pos, 3.5, false) {
        ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
        if empowered {
            ctx.apply_stun(j, 0.8);
        }
    }
    if empowered {
        ctx.open_recast(AbilitySlot::C, 1, 0, 3.5);
    } else {
        shin_add_heat(ctx, 2);
    }
    Ok(true)
}

fn shin_perk_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    _target_idx: usize,
    _raw_dmg: f32,
    _is_crit: bool,
) -> (f32, DamageSchool) {
    let pct = ctx.talent_mod("PASSIVE_CRYSTAL").unwrap_or(0.0);
    let bonus = if pct > 0.0 {
        ctx.eval_inputs().cp * pct
    } else {
        0.0
    };
    (bonus, DamageSchool::Crystal)
}

static SHIN_VARIANTS: &[AbilityVariantSpec] = &[
    AbilityVariantSpec {
        slot: HeroSlot::A2,
        spec: AbilityKitSpec {
            ability_id: "Ability__Shin__A1",
            targeting: CastTargeting::Directional,
            max_rank: 5,
            startup: ValueSpec::zero(),
            range: ValueSpec::var("Range", [8.0, 8.0, 8.0, 8.0, 9.0]),
            cooldown: ValueSpec::var("Cooldown", [6.0, 5.5, 5.0, 4.5, 3.0]),
            cost: ValueSpec::zero(),
            custom_exec: Some(shin_a_cast),
            ..AbilityKitSpec::DEFAULT
        },
    },
    AbilityVariantSpec {
        slot: HeroSlot::B2,
        spec: AbilityKitSpec {
            ability_id: "Ability__Shin__B1",
            targeting: CastTargeting::GroundPoint,
            max_rank: 5,
            startup: ValueSpec::zero(),
            range: ValueSpec::var("Range", [6.0; 5]),
            cooldown: ValueSpec::var("Cooldown", [8.0, 7.5, 7.0, 6.5, 6.0]),
            cost: ValueSpec::zero(),
            custom_exec: Some(shin_b_cast),
            ..AbilityKitSpec::DEFAULT
        },
    },
    AbilityVariantSpec {
        slot: HeroSlot::C2,
        spec: AbilityKitSpec {
            ability_id: "Ability__Shin__C2",
            targeting: CastTargeting::GroundPoint,
            max_rank: 3,
            startup: ValueSpec::zero(),
            range: ValueSpec::constant(7.5),
            cooldown: ValueSpec::var3("COOLDOWN", [60.0, 50.0, 40.0]),
            cost: ValueSpec::zero(),
            custom_exec: Some(shin_c_cast),
            ..AbilityKitSpec::DEFAULT
        },
    },
];

pub(super) static SHIN: HeroKitSpec = HeroKitSpec {
    hero_id: "Shin",
    type_id: 927,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Perk__Shin",
        on_basic_attack: Some(shin_perk_attack),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Shin__A",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::zero(),
        range: ValueSpec::var("Range", [7.0, 7.0, 7.0, 7.0, 8.0]),
        cooldown: ValueSpec::var("Cooldown", [6.0, 5.5, 5.0, 4.5, 3.0]),
        cost: ValueSpec::var("Energy Cost", [60.0, 65.0, 70.0, 75.0, 80.0]),
        custom_exec: Some(shin_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Shin__B",
        targeting: CastTargeting::GroundPoint,
        max_rank: 5,
        startup: ValueSpec::zero(),
        range: ValueSpec::var("Range", [6.0; 5]),
        cooldown: ValueSpec::var("Cooldown", [8.0, 7.5, 7.0, 6.5, 6.0]),
        cost: ValueSpec::var("Energy Cost", [60.0, 65.0, 70.0, 75.0, 80.0]),
        custom_exec: Some(shin_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Shin__C",
        targeting: CastTargeting::GroundPoint,
        max_rank: 3,
        startup: ValueSpec::zero(),
        range: ValueSpec::var3("RANGE", [7.0; 3]),
        cooldown: ValueSpec::var3("COOLDOWN", [60.0, 50.0, 40.0]),
        cost: ValueSpec::var3("ENERGY COST", [100.0, 120.0, 140.0]),
        custom_exec: Some(shin_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    variants: SHIN_VARIANTS,
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 50. Tony (tid=397)
// ============================================================================

fn tony_perk_tick(ctx: &mut HeroKitContext<'_, '_>) {
    if let Some(k) = ctx.room.entities[ctx.caster].hero_kit.as_deref_mut()
        && k.counters[0] > 0
    {
        k.counters[0] = k.counters[0].saturating_sub(1);
    }
    if ctx.talent_special() == TalentSpecial::TonyImmuneToTaunted {
        ctx.add_buff(
            ctx.caster,
            BuffStatKind::AttackSpeed,
            ctx.talent_mod("BONUS_ATTACKSPEED").unwrap_or(0.0),
            0.25,
        );
    }
}

fn tony_perk_take_damage(
    ctx: &mut HeroKitContext<'_, '_>,
    _source_idx: Option<usize>,
    _amount: f32,
    _school: DamageSchool,
) -> f32 {
    let cd_zero = ctx.room.entities[ctx.caster]
        .hero_kit
        .as_ref()
        .is_none_or(|k| k.counters[0] == 0);
    if cd_zero {
        let mult = if ctx.talent_special() == TalentSpecial::TonyPerkAspdBarrier {
            1.0 + ctx.talent_mod("BARRIER_MODIFIER").unwrap_or(0.0)
        } else {
            1.0
        };
        let barrier = ctx.room.entities[ctx.caster].max_hp * 0.12 * mult;
        ctx.grant_barrier(ctx.caster, barrier, 3.0, None);
        if let Some(k) = ctx.room.entities[ctx.caster].hero_kit.as_deref_mut() {
            k.counters[0] = seconds_to_ticks(10.0) as u32;
        }
    }
    1.0
}

fn tony_perk_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    target_idx: usize,
    _raw_dmg: f32,
    _is_crit: bool,
) -> (f32, DamageSchool) {
    let rem = ctx.room.entities[ctx.caster]
        .hero_kit
        .as_ref()
        .filter(|k| ctx.tick < k.timers[0])
        .map(|k| k.counters[1])
        .unwrap_or(0);
    if rem == 0 {
        return (0.0, DamageSchool::Weapon);
    }
    if let Some(k) = ctx.room.entities[ctx.caster].hero_kit.as_deref_mut() {
        k.counters[1] = rem - 1;
    }
    let a_rank = ctx.room.entities[ctx.caster].ability_ranks[0].max(1);
    let bonus = ctx.eval_var(
        "Ability__Tony__A",
        a_rank,
        ValueSpec::var_scaled(
            "BONUS_WEAPON_DAMAGE",
            [10.0, 20.0, 30.0, 40.0, 50.0],
            0.0,
            0.7,
            0.0,
            0.0,
        ),
    ) * ctx.talent_dmg_mult(AbilitySlot::A);
    if rem == 1 {
        let stun = ctx.eval_var(
            "Ability__Tony__A",
            a_rank,
            ValueSpec::var("STUN_DURATION", [0.8, 0.8, 0.8, 0.8, 1.2]),
        );
        ctx.apply_stun(target_idx, stun);
        if ctx.talent_special() == TalentSpecial::TonyJawbreakerAoE {
            let center = ctx.room.entities[target_idx].position;
            for j in ctx.enemies_in_radius(center, 3.0, false) {
                if j != target_idx {
                    ctx.deal_damage(j, bonus * 0.75, DamageSchool::Weapon, 0.0, false);
                    ctx.apply_stun(j, stun * 0.65);
                }
            }
        }
    }
    (bonus, DamageSchool::Weapon)
}

fn tony_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    rank: u8,
    _target: CastTarget,
) -> Result<bool, &'static str> {
    let dur = ctx.eval_var(
        "Ability__Tony__A",
        rank,
        ValueSpec::var("EMPOWER_DURATION", [4.0; 5]),
    );
    let aspd = ctx.eval_var(
        "Ability__Tony__A",
        rank,
        ValueSpec::var("BONUS_ATTACK_SPEED", [0.4, 0.5, 0.6, 0.7, 0.8]),
    );
    let ms = ctx.eval_var(
        "Ability__Tony__A",
        rank,
        ValueSpec::var("MOVEMENTSPEED_AMOUNT", [2.0, 2.25, 2.5, 2.75, 3.0]),
    );
    ctx.add_buff(ctx.caster, BuffStatKind::AttackSpeed, aspd, dur);
    ctx.add_buff(ctx.caster, BuffStatKind::MoveSpeedFlat, ms, dur);
    let k = ctx.room.entities[ctx.caster]
        .hero_kit
        .get_or_insert_with(Box::default);
    k.counters[1] = 3;
    k.timers[0] = ctx.tick + seconds_to_ticks(dur);
    ctx.reset_attack_cd();
    Ok(true)
}

fn tony_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let dest = ctx.resolve_target_pos(target, 5.0);
    move_toward_or_dash(ctx, dest);
    let dr = ctx.eval_var(
        "Ability__Tony__B",
        rank,
        ValueSpec::var("DAMAGE_REDUCTION", [0.25; 5]),
    );
    ctx.add_buff(ctx.caster, BuffStatKind::DamageReduction, dr, 0.8);
    if let Some(k) = ctx.room.entities[ctx.caster].hero_kit.as_deref_mut() {
        k.counters[0] = 0;
    }
    let dmg = ctx.eval_var(
        "Ability__Tony__B",
        rank,
        ValueSpec::dmg(0, [50.0, 90.0, 130.0, 170.0, 210.0], 1.25, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let base_taunt = ctx.eval_var(
        "Ability__Tony__B",
        rank,
        ValueSpec::var("TAUNT_DURATION", [1.2, 1.2, 1.2, 1.2, 1.6]),
    );
    let bonus_dur = (ctx.eval_inputs().bonus_wp.min(200.0) * 0.004 + base_taunt).min(2.0);
    for j in ctx.enemies_in_radius(dest, 3.8, false) {
        ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
        ctx.apply_taunt(j, bonus_dur);
    }
    Ok(true)
}

fn tony_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let dest = ctx.resolve_target_pos(target, 7.5);
    move_toward_or_dash(ctx, dest);
    let dmg = ctx.eval_var(
        "Ability__Tony__C",
        rank,
        ValueSpec::dmg3(0, [300.0, 450.0, 600.0], 1.0, 1.5, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let rad = ctx.eval_var(
        "Ability__Tony__C",
        rank,
        ValueSpec::var3("KNOCKAWAY_RADIUS", [3.5; 3]),
    );
    let dist = ctx
        .eval_var(
            "Ability__Tony__C",
            rank,
            ValueSpec::var3("KNOCKAWAY_DISTANCE", [8.0, 13.0, 18.0]),
        )
        .min(12.0);
    let stun = ctx.eval_var(
        "Ability__Tony__C",
        rank,
        ValueSpec::var3("KNOCKAWAY_STUN_DURATION", [0.3; 3]),
    );
    let ms = ctx.eval_var(
        "Ability__Tony__C",
        rank,
        ValueSpec::var3("MOVEMENTSPEED_AMOUNT", [2.0, 2.25, 2.5]),
    );
    let aspd = ctx.eval_var(
        "Ability__Tony__C",
        rank,
        ValueSpec::var3("ATTACKSPEED_AMOUNT", [0.5, 0.6, 0.7]),
    );
    let bdur = ctx.eval_var(
        "Ability__Tony__C",
        rank,
        ValueSpec::var3("SELFBUFF_DURATION", [4.0; 3]),
    );
    ctx.add_buff(ctx.caster, BuffStatKind::MoveSpeedFlat, ms, bdur);
    ctx.add_buff(ctx.caster, BuffStatKind::AttackSpeed, aspd, bdur);
    for j in ctx.enemies_in_radius(dest, rad, false) {
        ctx.deal_damage(j, dmg, DamageSchool::Weapon, 0.0, false);
        ctx.apply_stun(j, stun);
        ctx.knockback(j, dest, dist, stun);
    }
    Ok(true)
}

pub(super) static TONY: HeroKitSpec = HeroKitSpec {
    hero_id: "Tony",
    type_id: 397,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Perk__Tony",
        on_tick: Some(tony_perk_tick),
        on_basic_attack: Some(tony_perk_attack),
        on_take_damage: Some(tony_perk_take_damage),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Tony__A",
        targeting: CastTargeting::InstantSelf,
        max_rank: 5,
        startup: ValueSpec::zero(),
        range: ValueSpec::constant(3.5),
        cooldown: ValueSpec::var("COOLDOWN", [12.0; 5]),
        cost: ValueSpec::var("ENERGY COST", [40.0, 45.0, 50.0, 55.0, 60.0]),
        custom_exec: Some(tony_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Tony__B",
        targeting: CastTargeting::GroundPoint,
        max_rank: 5,
        startup: ValueSpec::constant(0.35),
        range: ValueSpec::var("RANGE", [8.0; 5]),
        cooldown: ValueSpec::var("COOLDOWN", [20.0, 18.0, 16.0, 14.0, 12.0]),
        cost: ValueSpec::var("ENERGY COST", [80.0, 90.0, 100.0, 110.0, 120.0]),
        custom_exec: Some(tony_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Tony__C",
        targeting: CastTargeting::Directional,
        max_rank: 3,
        startup: ValueSpec::constant(0.35),
        range: ValueSpec::constant(9.0),
        cooldown: ValueSpec::var3("COOLDOWN", [80.0, 70.0, 60.0]),
        cost: ValueSpec::var3("ENERGY COST", [100.0; 3]),
        custom_exec: Some(tony_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};
