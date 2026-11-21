//! Assassin & sniper hero kit specifications (17 heroes).

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

pub(super) static ANKA: HeroKitSpec = HeroKitSpec {
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
    if let Some(landing) = ctx.room.project_spell_landing(goal) {
        ctx.room.place_spell_actor(ctx.caster, landing, ctx.events);
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

pub(super) static BARON: HeroKitSpec = HeroKitSpec {
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

pub(super) static CAINE: HeroKitSpec = HeroKitSpec {
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

pub(super) static GWEN: HeroKitSpec = HeroKitSpec {
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
        .query::<(bevy_ecs::entity::Entity, &crate::heroes::engine::KitEffect)>()
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
            .remove_spell_handle::<crate::heroes::engine::KitEffect>(eh);
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

pub(super) static IDRIS: HeroKitSpec = HeroKitSpec {
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

pub(super) static INARA: HeroKitSpec = HeroKitSpec {
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

pub(super) static KESTREL: HeroKitSpec = HeroKitSpec {
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

pub(super) static KINETIC: HeroKitSpec = HeroKitSpec {
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

pub(super) static KOSHKA: HeroKitSpec = HeroKitSpec {
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
// 34. Miho (tid=919)
// ============================================================================

fn miho_apply_keen_eye(ctx: &mut HeroKitContext<'_, '_>, target_idx: usize, stacks: u8) {
    if target_idx >= ctx.room.entities.len() || ctx.room.entities[target_idx].hp <= 0.0 {
        return;
    }
    let total = ctx.add_mark(target_idx, marks::KEEN_EYE, stacks, 3, 5.0);
    if total >= 3 {
        ctx.consume_mark(target_idx, marks::KEEN_EYE);
        let lvl = f32::from(ctx.room.entities[ctx.caster].level);
        if ctx.talent_special() != TalentSpecial::MihoTrueBurstNoUnguarded {
            ctx.apply_stun(target_idx, 0.6);
            ctx.add_mark(target_idx, marks::UNGUARDED, 1, 1, 2.5);
        }
        ctx.deal_damage(
            target_idx,
            45.0 + lvl * 12.0,
            DamageSchool::True,
            0.0,
            false,
        );
        ctx.grant_barrier(ctx.caster, 70.0 + lvl * 15.0, 2.5, None);
        if ctx.room.entities[ctx.caster].ability_ranks[0] >= 5 {
            ctx.reset_slot_cd(AbilitySlot::A);
        }
    }
}

fn miho_perk_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    target_idx: usize,
    _base_damage: f32,
    _was_crit: bool,
) -> (f32, DamageSchool) {
    ctx.add_resource(5.0);
    miho_apply_keen_eye(ctx, target_idx, 1);
    (0.0, DamageSchool::Weapon)
}

fn miho_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let range = ctx.eval_var("Ability__Miho__A", rank, ValueSpec::var("Range", [9.0; 5]));
    let origin = ctx.caster_pos();
    let goal = ctx.resolve_target_pos(target, range);
    move_toward_or_dash(ctx, goal);
    let dmg = ctx.eval_var(
        "Ability__Miho__A",
        rank,
        ValueSpec::dmg(0, [30.0, 50.0, 70.0, 90.0, 130.0], 0.0, 0.0, 1.0),
    ) * ctx.talent_dmg_mult(slot);
    for j in ctx.enemies_in_radius(lerp_pos(origin, goal, 0.5), 5.0, false) {
        ctx.deal_damage(j, dmg, DamageSchool::Weapon, 0.0, false);
        miho_apply_keen_eye(ctx, j, 1);
    }
    Ok(true)
}

fn miho_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let dur = ctx.eval_var(
        "Ability__Miho__B",
        rank,
        ValueSpec::var("TIMEOUT_WINDOW", [1.5; 5]),
    );
    ctx.add_buff(ctx.caster, BuffStatKind::DamageReduction, 0.25, dur);
    ctx.open_recast(AbilitySlot::B, 1, 0, dur);
    let range = ctx.eval_var("Ability__Miho__B", rank, ValueSpec::var("RANGE", [9.0; 5]));
    let dmg = ctx.eval_var(
        "Ability__Miho__B",
        rank,
        ValueSpec::dmg(0, [35.0, 70.0, 105.0, 140.0, 210.0], 0.9, 0.45, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let goal = ctx.resolve_target_pos(target, range);
    for j in ctx.enemies_in_radius(goal, 2.5, false) {
        ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
        miho_apply_keen_eye(ctx, j, 1);
    }
    Ok(true)
}

fn miho_b_slash(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    ctx.clear_recast(AbilitySlot::B);
    let range = ctx.eval_var(
        "Ability__Miho__B_Slash",
        rank,
        ValueSpec::var("RANGE", [9.0; 5]),
    );
    let dmg = ctx.eval_var(
        "Ability__Miho__B_Slash",
        rank,
        ValueSpec::dmg(0, [100.0, 145.0, 190.0, 235.0, 325.0], 0.85, 0.5, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let goal = ctx.resolve_target_pos(target, range);
    move_toward_or_dash(ctx, goal);
    for j in ctx.enemies_in_radius(ctx.caster_pos(), 3.0, false) {
        ctx.deal_damage(j, dmg, DamageSchool::Weapon, 0.0, false);
        miho_apply_keen_eye(ctx, j, 1);
    }
    Ok(true)
}

fn miho_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let origin = ctx.caster_pos();
    let range = ctx.eval_var(
        "Ability__Miho__C",
        rank,
        ValueSpec::var3("RANGE", [25.0; 3]),
    );
    let spd = ctx.eval_var(
        "Ability__Miho__C",
        rank,
        ValueSpec::var3("PROJECTILE_SPEED", [18.0; 3]),
    );
    let dmg = ctx.eval_var(
        "Ability__Miho__C",
        rank,
        ValueSpec::dmg3(0, [300.0, 450.0, 600.0], 1.1, 0.0, 0.0),
    );
    let goal = ctx.resolve_target_pos(target, range);
    let (dx, dy) = unit_dir(origin, goal);
    let end = clamp_pos(Position {
        x: origin.x + dx * range,
        y: origin.y + dy * range,
    });
    ctx.spawn_effect(
        KitEffectKind::Projectile {
            ability_slot: slot.idx() as u8,
            rank,
            speed_per_tick: spd / TICK_HZ as f32,
            radius: 1.8,
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
            minion_mult: 0.65,
            resets_cd_on_hit: false,
            applies_mark: Some((marks::KEEN_EYE, 3, TICK_HZ * 5)),
            destroys_projectiles: true,
            hit_actors: Vec::new(),
        },
        origin,
        end,
    );
    if ctx.talent_special() == TalentSpecial::MihoUltHealsAllies {
        let pct = ctx.talent_mod("HEAL_PERCENT_MAX_HEALTH").unwrap_or(0.2);
        for a in ctx.allies_in_radius(origin, 8.0, true, true) {
            let amt = ctx.room.entities[a].max_hp * pct * ctx.talent_heal_mult(slot);
            ctx.heal_unit(a, amt);
        }
    }
    Ok(true)
}

static MIHO_VARIANTS: &[AbilityVariantSpec] = &[AbilityVariantSpec {
    slot: HeroSlot::B2,
    spec: AbilityKitSpec {
        ability_id: "Ability__Miho__B_Slash",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        cooldown: ValueSpec::zero(),
        cost: ValueSpec::zero(),
        custom_exec: Some(miho_b_slash),
        ..AbilityKitSpec::DEFAULT
    },
}];

pub(super) static MIHO: HeroKitSpec = HeroKitSpec {
    hero_id: "Miho",
    type_id: 919,
    resource: ResourceKind::Focus,
    resource_max: 100.0,
    resource_initial: 100.0,
    resource_regen_per_sec: 8.0,
    perk: HeroicPerkSpec {
        id: "Perk__Miho",
        on_basic_attack: Some(miho_perk_attack),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Miho__A",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::constant(0.1),
        range: ValueSpec::var("Range", [9.0; 5]),
        cooldown: ValueSpec::var("Cooldown", [18.0, 17.5, 17.0, 16.5, 16.0]),
        cost: ValueSpec::zero(),
        custom_exec: Some(miho_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Miho__B",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::constant(0.15),
        range: ValueSpec::var("RANGE", [9.0; 5]),
        cooldown: ValueSpec::var("Cooldown", [12.0, 11.0, 10.0, 9.0, 5.0]),
        cost: ValueSpec::var("FOCUS_COST", [25.0; 5]),
        custom_exec: Some(miho_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Miho__C",
        targeting: CastTargeting::Directional,
        max_rank: 3,
        startup: ValueSpec::constant(0.25),
        range: ValueSpec::var3("RANGE", [25.0; 3]),
        cooldown: ValueSpec::var3("Cooldown", [90.0, 80.0, 70.0]),
        cost: ValueSpec::var3("FOCUS_COST", [35.0; 3]),
        custom_exec: Some(miho_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    variants: MIHO_VARIANTS,
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 39. Reza (tid=403)
// ============================================================================

fn reza_perk_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    target_idx: usize,
    _base_damage: f32,
    _was_crit: bool,
) -> (f32, DamageSchool) {
    let mut bonus = 0.0;
    if ctx.consume_mark(target_idx, marks::FIRESTARTER) > 0 {
        let lvl = f32::from(ctx.room.entities[ctx.caster].level);
        let cp = ctx.eval_inputs().cp;
        bonus += 40.0 + lvl * 8.0 + cp * 0.7;
    }
    if ctx
        .room
        .entities
        .get(ctx.caster)
        .and_then(|e| e.hero_kit.as_deref())
        .is_some_and(|k| ctx.tick < k.timers[1])
    {
        ctx.add_mark(target_idx, marks::FIRESTARTER, 1, 1, 5.0);
        bonus += 30.0 + ctx.eval_inputs().cp * 0.35;
    }
    (bonus, DamageSchool::Crystal)
}

fn reza_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let origin = ctx.caster_pos();
    let dest = ctx.resolve_target_pos(target, 6.0);
    move_toward_or_dash(ctx, dest);
    let dmg = ctx.eval_var(
        "Ability__Reza__B",
        rank,
        ValueSpec::dmg(0, [80.0, 110.0, 140.0, 170.0, 200.0], 0.4, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let apply_fs = ctx.talent_special() == TalentSpecial::RezaDashAppliesFirestarter;
    for j in ctx.enemies_in_radius(lerp_pos(origin, dest, 0.5), 3.0, false) {
        ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
        if apply_fs {
            ctx.add_mark(j, marks::FIRESTARTER, 1, 1, 5.0);
        }
    }
    let emp = ctx.eval_var(
        "Ability__Reza__B",
        rank,
        ValueSpec::var_scaled(
            "EMPOWERED_DAMAGE",
            [20.0, 40.0, 60.0, 80.0, 100.0],
            0.2,
            0.0,
            0.0,
            0.0,
        ),
    );
    let emp_dur = ctx.eval_var(
        "Ability__Reza__B",
        rank,
        ValueSpec::var("EMPOWERED_DURATION", [3.0; 5]),
    );
    ctx.empower_next_attacks(1, emp, true, 0.0, 0.0, 0.0, 0.0, emp_dur);
    if ctx.talent_special() == TalentSpecial::RezaDashBarrier {
        let b = ctx.room.entities[ctx.caster].max_hp
            * ctx.talent_mod("BARRIER_MODIFIER").unwrap_or(0.08)
            * ctx.talent_barrier_mult(slot);
        ctx.grant_barrier(ctx.caster, b, 2.5, None);
    }
    Ok(true)
}

fn reza_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let dest = ctx.resolve_target_pos(target, 7.5);
    ctx.set_untargetable(ctx.caster, 0.6);
    move_toward_or_dash(ctx, dest);
    let rad = ctx.eval_var(
        "Ability__Reza__C",
        rank,
        ValueSpec::var3("RADIUS", [5.0; 3]),
    );
    let dmg = ctx.eval_var(
        "Ability__Reza__C",
        rank,
        ValueSpec::dmg3(0, [200.0, 300.0, 400.0], 0.8, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let fort = ctx.eval_var(
        "Ability__Reza__C",
        rank,
        ValueSpec::var_scaled(
            "FORTIFIED_HEALTH",
            [50.0, 100.0, 150.0, 150.0, 150.0],
            1.5,
            0.0,
            0.0,
            0.0,
        ),
    );
    let mut dur = ctx.eval_var(
        "Ability__Reza__C",
        rank,
        ValueSpec::var3("DURATION", [6.0; 3]),
    );
    if ctx.talent_special() == TalentSpecial::RezaUltAttackSpeedDuration {
        dur += ctx.talent_mod("BONUS_DURATION").unwrap_or(2.0);
        if let Some(aspd) = ctx.talent_mod("PASSIVE_ATTACK_SPEED") {
            ctx.add_buff(ctx.caster, BuffStatKind::AttackSpeed, aspd, dur);
        }
    }
    ctx.grant_fortified(ctx.caster, fort, dur);
    ctx.add_buff(ctx.caster, BuffStatKind::MoveSpeedFlat, 1.0, dur);
    {
        let k = ctx.room.entities[ctx.caster]
            .hero_kit
            .get_or_insert_with(Box::default);
        k.timers[1] = ctx.tick + seconds_to_ticks(dur);
    }
    for j in ctx.enemies_in_radius(ctx.caster_pos(), rad, false) {
        let bonus = if ctx.consume_mark(j, marks::FIRESTARTER) > 0 {
            60.0 + ctx.eval_inputs().cp * 0.4
        } else {
            0.0
        };
        ctx.deal_damage(j, dmg + bonus, DamageSchool::Crystal, 0.0, false);
        ctx.add_mark(j, marks::FIRESTARTER, 1, 1, 5.0);
    }
    Ok(true)
}

const REZA_A_EFFECTS: &[PrimitiveEffect] = &[PrimitiveEffect::SkillshotProjectile {
    damage: ValueSpec::dmg(0, [60.0, 95.0, 130.0, 165.0, 200.0], 0.5, 0.0, 0.0),
    school: DamageKind::Crystal,
    range: ValueSpec::var("RANGE", [7.5, 7.5, 7.5, 7.5, 9.5]),
    speed: ValueSpec::var("SHOT_SPEED", [15.0; 5]),
    radius: ValueSpec::var("SHOT_RADIUS", [1.0; 5]),
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
    minion_mult: 0.75,
    resets_cast_cd_on_hit: false,
    applies_mark: Some((MarkKind::Custom(marks::FIRESTARTER), 1, 5.0)),
    destroys_enemy_projectiles: false,
}];

pub(super) static REZA: HeroKitSpec = HeroKitSpec {
    hero_id: "Reza",
    type_id: 403,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Perk__Reza",
        on_basic_attack: Some(reza_perk_attack),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Reza__A",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::var("STARTUP_DELAY", [0.2; 5]),
        range: ValueSpec::var("RANGE", [7.5, 7.5, 7.5, 7.5, 9.5]),
        cooldown: ValueSpec::var("COOLDOWN", [5.0, 4.5, 4.0, 3.5, 3.0]),
        cost: ValueSpec::var("ENERGY COST", [40.0, 45.0, 50.0, 55.0, 60.0]),
        effects: REZA_A_EFFECTS,
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Reza__B",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::var("DASH_STARTUP_DELAY", [0.1; 5]),
        range: ValueSpec::constant(6.0),
        cooldown: ValueSpec::var("COOLDOWN", [15.0, 14.0, 13.0, 12.0, 10.0]),
        cost: ValueSpec::var("ENERGY COST", [60.0, 75.0, 90.0, 105.0, 120.0]),
        charges: ValueSpec::constant(2.0),
        between_charges_cd: ValueSpec::var("COOLDOWN2", [0.5, 0.5, 0.5, 0.5, 0.2]),
        custom_exec: Some(reza_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Reza__C",
        targeting: CastTargeting::GroundPoint,
        max_rank: 3,
        startup: ValueSpec::var3("TELEPORT_START_DELAY", [0.2; 3]),
        range: ValueSpec::constant(7.5),
        cooldown: ValueSpec::var3("COOLDOWN", [90.0, 70.0, 50.0]),
        cost: ValueSpec::var3("ENERGY COST", [150.0, 175.0, 200.0]),
        custom_exec: Some(reza_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 40. Ringo (tid=243) — Double Down + Achilles Shot + Twirling Silver + Hellfire Brew
// ============================================================================

fn ringo_perk_kill(
    ctx: &mut HeroKitContext<'_, '_>,
    _victim_idx: usize,
    is_killer: bool,
    _is_hero: bool,
) {
    if is_killer {
        let wp = ctx.eval_inputs().wp;
        ctx.empower_next_attacks(1, wp * 0.5, false, 0.0, 0.0, 0.0, 0.0, 6.0);
    }
}

fn ringo_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let Some(tj) = ctx.resolve_target_idx(target) else {
        return Err("target required");
    };
    let mult = ctx.talent_dmg_mult(slot);
    let impact = ctx.eval_var(
        "Ability__Ringo__C",
        rank,
        ValueSpec::dmg3(0, [250.0, 400.0, 550.0], 0.75, 0.0, 0.0),
    ) * mult;
    let burn_tick = ctx.eval_var(
        "Ability__Ringo__C",
        rank,
        ValueSpec::dmg3(1, [55.0, 90.0, 125.0], 0.4, 0.0, 0.0),
    ) * mult;
    let splash_r = ctx.eval_var(
        "Ability__Ringo__C",
        rank,
        ValueSpec::var3("ImpactSplashRadius", [3.5; 3]),
    );
    let burn_dur = ctx.eval_var(
        "Ability__Ringo__C",
        rank,
        ValueSpec::var3("Burn_Duration", [4.0; 3]),
    );
    let pos = ctx.room.entities[tj].position;
    ctx.deal_damage(tj, impact, DamageSchool::Crystal, 0.0, false);
    spawn_single_hit_dot(
        ctx,
        slot,
        tj,
        burn_tick,
        DamageSchool::Crystal,
        burn_dur,
        1.0,
    );
    for oj in ctx.enemies_in_radius(pos, splash_r, false) {
        if oj != tj {
            ctx.deal_damage(oj, impact * 0.5, DamageSchool::Crystal, 0.0, false);
            spawn_single_hit_dot(
                ctx,
                slot,
                oj,
                burn_tick,
                DamageSchool::Crystal,
                burn_dur,
                0.7,
            );
        }
    }
    if ctx.talent_special() == TalentSpecial::RingoSpikedBrewBounce {
        let bounce_r = ctx.talent_mod("BOUNCE_RANGE").unwrap_or(8.0);
        if let Some(next_j) = ctx
            .enemies_in_radius(pos, bounce_r, true)
            .into_iter()
            .find(|&x| x != tj)
        {
            ctx.deal_damage(next_j, impact * 0.7, DamageSchool::Crystal, 0.0, false);
        }
    }
    Ok(true)
}

const RINGO_A_EFFECTS: &[PrimitiveEffect] = &[PrimitiveEffect::TargetedStrike {
    damage: ValueSpec::dmg(0, [80.0, 125.0, 170.0, 215.0, 350.0], 1.25, 0.0, 0.0),
    school: DamageKind::Crystal,
    bonus_if_marked: None,
    triggers_basic_effects: false,
    resets_attack_cd: false,
    stun_seconds: ValueSpec::zero(),
    slow_pct: ValueSpec::var("SlowStrength", [0.3, 0.35, 0.4, 0.45, 0.5]),
    slow_seconds: ValueSpec::var("SlowDuration", [1.5, 1.5, 1.5, 1.5, 2.5]),
}];

const RINGO_B_EFFECTS: &[PrimitiveEffect] = &[
    PrimitiveEffect::SelfBuff {
        stat: BuffStatKind::AttackSpeed,
        amount: ValueSpec::var("BonusASPDStrength", [0.3, 0.4, 0.5, 0.6, 0.8]),
        duration: ValueSpec::var("SpeedBuffDuration", [4.0; 5]),
        empowered_attacks: 0,
        empowered_bonus_damage: ValueSpec::zero(),
        empowered_stun_seconds: 0.0,
        empowered_slow_pct: 0.0,
        empowered_slow_seconds: 0.0,
        empowered_knockback: 0.0,
        resets_attack_cd: true,
    },
    PrimitiveEffect::SelfBuff {
        stat: BuffStatKind::MoveSpeedFlat,
        amount: ValueSpec::var("BonusMSPDStrength", [0.5, 0.5, 0.5, 0.5, 0.8]),
        duration: ValueSpec::var("SpeedBuffDuration", [4.0; 5]),
        empowered_attacks: 0,
        empowered_bonus_damage: ValueSpec::zero(),
        empowered_stun_seconds: 0.0,
        empowered_slow_pct: 0.0,
        empowered_slow_seconds: 0.0,
        empowered_knockback: 0.0,
        resets_attack_cd: false,
    },
];

pub(super) static RINGO: HeroKitSpec = HeroKitSpec {
    hero_id: "Ringo",
    type_id: 243,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Perk__Ringo",
        on_kill_assist: Some(ringo_perk_kill),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Ringo__A",
        targeting: CastTargeting::TargetEnemy,
        max_rank: 5,
        startup: ValueSpec::constant(0.2),
        range: ValueSpec::var("Range", [8.0; 5]),
        cooldown: ValueSpec::var("Cooldown", [9.0, 8.5, 8.0, 7.5, 7.0]),
        cost: ValueSpec::var("Energy Cost", [40.0, 45.0, 50.0, 55.0, 60.0]),
        effects: RINGO_A_EFFECTS,
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Ringo__B",
        targeting: CastTargeting::InstantSelf,
        max_rank: 5,
        startup: ValueSpec::constant(0.05),
        range: ValueSpec::constant(1.5),
        cooldown: ValueSpec::var("Cooldown", [8.0; 5]),
        cost: ValueSpec::var("Energy Cost", [40.0, 45.0, 50.0, 55.0, 60.0]),
        effects: RINGO_B_EFFECTS,
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Ringo__C",
        targeting: CastTargeting::TargetEnemy,
        max_rank: 3,
        startup: ValueSpec::constant(1.1),
        range: ValueSpec::var3("Range", [13.0; 3]),
        cooldown: ValueSpec::var3("Cooldown", [90.0, 80.0, 70.0]),
        cost: ValueSpec::var3("Energy Cost", [100.0, 115.0, 130.0]),
        custom_exec: Some(ringo_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 44. SAW (tid=250) — Spin Up + Roadie Run + Suppressing Fire + Mad Cannon
// ============================================================================

fn saw_perk_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    target_idx: usize,
    _base_damage: f32,
    _was_crit: bool,
) -> (f32, DamageSchool) {
    let stacks = ctx.add_mark(ctx.caster, marks::SPIN_UP, 1, 12, 3.5);
    ctx.add_buff(
        ctx.caster,
        BuffStatKind::AttackSpeed,
        0.08 * f32::from(stacks),
        3.5,
    );
    if ctx.talent_special() == TalentSpecial::SawSpinUpSpeedNotSlow {
        let bonus = ctx.talent_mod("SPEED_BOOST_PER_STACK").unwrap_or(0.05) * f32::from(stacks);
        ctx.add_buff(ctx.caster, BuffStatKind::MoveSpeedFlat, bonus, 3.5);
    } else {
        ctx.add_buff(
            ctx.caster,
            BuffStatKind::MoveSpeedFlat,
            -0.05 * f32::from(stacks),
            3.5,
        );
    }
    // Mad Cannon ammo check on C charges when C is learned.
    let mut bonus = 0.0;
    let mut school = DamageSchool::Weapon;
    if ctx
        .room
        .entities
        .get(ctx.caster)
        .and_then(|e| e.hero_kit.as_deref())
        .is_some_and(|k| k.counters[2] > 0)
    {
        if let Some(k) = ctx.room.entities[ctx.caster].hero_kit.as_deref_mut() {
            k.counters[2] -= 1;
        }
        let rank_c = ctx.room.entities[ctx.caster].ability_ranks[2].max(1);
        bonus += ctx.eval_var(
            "Ability__SAW__C",
            rank_c,
            ValueSpec::dmg3(0, [250.0, 275.0, 300.0], 1.5, 1.0, 0.0),
        ) * ctx.talent_dmg_mult(AbilitySlot::C);
        school = DamageSchool::Crystal;
        let tpos = ctx.room.entities[target_idx].position;
        for oj in ctx.enemies_in_radius(tpos, 3.0, false) {
            if oj != target_idx {
                ctx.deal_damage(oj, bonus * 0.6, DamageSchool::Crystal, 0.0, false);
            }
        }
    }
    (bonus, school)
}

fn saw_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    _target: CastTarget,
) -> Result<bool, &'static str> {
    let dur = ctx.eval_var(
        "Ability__SAW__A",
        rank,
        ValueSpec::var("AttackOverrideDuration", [2.2; 5]),
    );
    let ms = ctx.eval_var(
        "Ability__SAW__A",
        rank,
        ValueSpec::var("SpeedBoostStrength", [2.0; 5]),
    );
    ctx.add_buff(ctx.caster, BuffStatKind::MoveSpeedFlat, ms, dur);
    let shank_dmg = ctx.eval_var(
        "Ability__SAW__A",
        rank,
        ValueSpec::dmg(0, [80.0, 135.0, 190.0, 245.0, 300.0], 2.8, 0.0, 1.0),
    ) * ctx.talent_dmg_mult(slot);
    ctx.empower_next_attacks(1, shank_dmg, false, 0.0, 0.0, 0.0, 0.0, dur);
    if ctx.talent_special() == TalentSpecial::SawRoadieGrantsSpinUpAndCdr {
        ctx.add_mark(ctx.caster, marks::SPIN_UP, 6, 12, 4.0);
    } else {
        ctx.consume_mark(ctx.caster, marks::SPIN_UP);
    }
    Ok(true)
}

fn saw_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    rank: u8,
    _target: CastTarget,
) -> Result<bool, &'static str> {
    let shells = ctx
        .eval_var(
            "Ability__SAW__C",
            rank,
            ValueSpec::var3("CHARGES", [3.0, 4.0, 5.0]),
        )
        .round()
        .max(1.0) as u32;
    let k = ctx.room.entities[ctx.caster]
        .hero_kit
        .get_or_insert_with(Box::default);
    k.counters[2] = shells;
    ctx.reset_attack_cd();
    Ok(true)
}

const SAW_B_EFFECTS: &[PrimitiveEffect] = &[
    PrimitiveEffect::ChannelBeam {
        duration: ValueSpec::var("Channel_Duration", [2.0; 5]),
        ticks_per_sec: 3.0,
        range: ValueSpec::constant(10.5),
        width: 1.8,
        dps: ValueSpec::var_scaled(
            "DAMAGE_PER_SECOND",
            [140.0, 175.0, 210.0, 245.0, 315.0],
            2.2,
            0.0,
            0.0,
            0.0,
        ),
        school: DamageKind::Crystal,
        lock_caster: true,
        slow_pct: ValueSpec::var("SlowStrength", [0.25, 0.3, 0.35, 0.4, 0.6]),
        stun_target: false,
        global_all_heroes: false,
        shield_pierce: ValueSpec::zero(),
        structure_mult: 0.25,
        minion_mult: 0.5,
    },
    PrimitiveEffect::MarkStacks {
        mark: MarkKind::Custom(marks::SPIN_UP),
        on_self: true,
        add_stacks: 12,
        max_stacks: 12,
        duration: 4.5,
        consume_all: false,
        damage_per_stack: ValueSpec::zero(),
        heal_per_stack: ValueSpec::zero(),
        school: DamageKind::Weapon,
        mortal_wound_seconds: 0.0,
    },
];

pub(super) static SAW: HeroKitSpec = HeroKitSpec {
    hero_id: "SAW",
    type_id: 250,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Perk__SAW",
        on_basic_attack: Some(saw_perk_attack),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__SAW__A",
        targeting: CastTargeting::InstantSelf,
        max_rank: 5,
        startup: ValueSpec::zero(),
        range: ValueSpec::constant(2.5),
        cooldown: ValueSpec::var("Cooldown", [13.0, 12.0, 11.0, 10.0, 9.0]),
        cost: ValueSpec::var("Energy Cost", [60.0, 65.0, 70.0, 75.0, 80.0]),
        custom_exec: Some(saw_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__SAW__B",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::constant(0.3),
        range: ValueSpec::constant(10.5),
        cooldown: ValueSpec::var("Cooldown", [15.0, 15.0, 15.0, 15.0, 13.0]),
        cost: ValueSpec::var("Energy Cost", [40.0, 50.0, 60.0, 70.0, 80.0]),
        effects: SAW_B_EFFECTS,
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__SAW__C",
        targeting: CastTargeting::InstantSelf,
        max_rank: 3,
        startup: ValueSpec::zero(),
        range: ValueSpec::var3("RANGE", [9.0; 3]),
        cooldown: ValueSpec::var3("Cooldown", [25.0, 20.0, 15.0]),
        cost: ValueSpec::var3("ENERGY COST", [40.0; 3]),
        custom_exec: Some(saw_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 46. Silvernail (tid=432)
// ============================================================================

fn point_to_seg_distance(p: Position, a: Position, b: Position) -> f32 {
    let abx = b.x - a.x;
    let aby = b.y - a.y;
    let len2 = abx * abx + aby * aby;
    if len2 <= 0.0001 {
        return p.dist(a);
    }
    let t = (((p.x - a.x) * abx + (p.y - a.y) * aby) / len2).clamp(0.0, 1.0);
    let proj = Position {
        x: a.x + abx * t,
        y: a.y + aby * t,
    };
    p.dist(proj)
}

fn silvernail_perk_tick(ctx: &mut HeroKitContext<'_, '_>) {
    let bonus_as = ctx.eval_inputs().bonus_as.max(0.0);
    let cd_sec = (3.5 / (1.0 + bonus_as)).max(1.1);
    let ready = ctx.mark_stacks(ctx.caster, marks::DOUBLE_SHOT_READY) > 0;
    let trigger = {
        let k = ctx.room.entities[ctx.caster]
            .hero_kit
            .get_or_insert_with(Box::default);
        if k.counters[0] == 0 {
            k.counters[0] = seconds_to_ticks(cd_sec) as u32;
            true
        } else {
            if !ready {
                k.counters[0] = k.counters[0].saturating_sub(1);
            }
            false
        }
    };
    if trigger {
        ctx.add_mark(ctx.caster, marks::DOUBLE_SHOT_READY, 1, 1, 8.0);
    }
}

fn silvernail_perk_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    _target_idx: usize,
    _raw_dmg: f32,
    _is_crit: bool,
) -> (f32, DamageSchool) {
    if ctx.consume_mark(ctx.caster, marks::DOUBLE_SHOT_READY) > 0 {
        let bonus_as = ctx.eval_inputs().bonus_as.max(0.0);
        let cd_sec = (3.5 / (1.0 + bonus_as)).max(1.1);
        let k = ctx.room.entities[ctx.caster]
            .hero_kit
            .get_or_insert_with(Box::default);
        k.counters[0] = seconds_to_ticks(cd_sec) as u32;
        let c_rank = ctx.room.entities[ctx.caster].ability_ranks[2];
        let c_mult = if c_rank > 0 {
            ctx.eval_var(
                "Ability__Silvernail__C",
                c_rank,
                ValueSpec::var3("PERK_ATTACK_DAMAGE_BONUS", [0.2, 0.3, 0.4]),
            )
        } else {
            0.0
        };
        let extra_bolt = if ctx.talent_special() == TalentSpecial::SilvernailExtraBoltAndCRange {
            0.4
        } else {
            0.0
        };
        let dmg = ctx.eval_inputs().wp * (0.65 + c_mult + extra_bolt);
        (dmg, DamageSchool::Weapon)
    } else {
        (0.0, DamageSchool::Weapon)
    }
}

fn silvernail_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let range = ctx.eval_var(
        "Ability__Silvernail__A",
        rank,
        ValueSpec::var("RANGE", [8.5; 5]),
    ) + ctx.talent_mod("BONUS_RANGE").unwrap_or(0.0);
    let span = ctx.eval_var(
        "Ability__Silvernail__A",
        rank,
        ValueSpec::var("TRIPWIRE_ACTIVE_RANGE", [8.0; 5]),
    );
    let dur = ctx.eval_var(
        "Ability__Silvernail__A",
        rank,
        ValueSpec::var("TRIPWIRE_DURATION", [4.0, 4.5, 5.0, 5.5, 7.0]),
    ) * (1.0 + ctx.talent_mod("DURATION_MODIFIER").unwrap_or(0.0));
    let arm = ctx.eval_var(
        "Ability__Silvernail__A",
        rank,
        ValueSpec::var("TRIPWIRE_ARM_TIME", [0.4; 5]),
    );
    let dmg = ctx.eval_var(
        "Ability__Silvernail__A",
        rank,
        ValueSpec::dmg(0, [80.0, 120.0, 160.0, 200.0, 240.0], 1.2, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let slow = ctx.eval_var(
        "Ability__Silvernail__A",
        rank,
        ValueSpec::var("TRIPWIRE_SLOW_STRENGTH", [0.4; 5]),
    );
    let max_barriers = if ctx.talent_special() == TalentSpecial::SilvernailTripwireTriangle {
        4
    } else {
        3
    };
    let owner_id = ctx.caster_id();
    let existing: Vec<(bevy_ecs::entity::Entity, Position, Position)> = ctx
        .room
        .entities
        .ecs
        .query::<(bevy_ecs::entity::Entity, &KitEffect)>()
        .iter(&ctx.room.entities.ecs)
        .filter_map(|(eh, eff)| {
            (eff.owner_id == owner_id
                && matches!(
                    eff.kind,
                    KitEffectKind::SegmentBarrier {
                        is_ring_arena: false,
                        ..
                    }
                ))
            .then_some((
                eh,
                eff.position,
                match eff.kind {
                    KitEffectKind::SegmentBarrier { end_pos, .. } => end_pos,
                    _ => eff.position,
                },
            ))
        })
        .collect();
    if existing.len() >= max_barriers {
        let excess = existing.len() + 1 - max_barriers;
        for (eh, _, _) in existing.iter().take(excess) {
            ctx.room.entities.remove_spell_handle::<KitEffect>(*eh);
        }
    }

    let center = ctx.resolve_target_pos(target, range);
    let first_stake = existing
        .first()
        .map(|(_, pos, _)| *pos)
        .filter(|p| p.dist(center) <= span * 2.0);
    let triangle = ctx.talent_special() == TalentSpecial::SilvernailTripwireTriangle;

    let effects = [PrimitiveEffect::SegmentBarrier {
        radius_or_span: ValueSpec::constant(span),
        duration: ValueSpec::constant(dur),
        arm_delay: ValueSpec::constant(arm),
        dps_or_hit_damage: ValueSpec::constant(dmg),
        school: DamageKind::Crystal,
        stun_seconds: ValueSpec::zero(),
        silence_seconds: ValueSpec::constant(1.5),
        slow_pct: ValueSpec::constant(slow),
        max_stakes: max_barriers as u8,
        is_ring_arena: false,
    }];
    ctx.exec_primitives("Ability__Silvernail__A", slot, rank, target, &effects);

    if triangle
        && existing.len() >= 2
        && let Some(closing_start) = first_stake
    {
        ctx.spawn_effect(
            KitEffectKind::SegmentBarrier {
                ability_slot: slot.idx() as u8,
                armed_at: ctx.tick + seconds_to_ticks(arm),
                end_tick: ctx.tick + seconds_to_ticks(dur),
                end_pos: closing_start,
                radius: span,
                is_ring_arena: false,
                dps_or_hit_damage: dmg,
                school: DamageSchool::Crystal,
                stun_ticks: 0,
                silence_ticks: seconds_to_ticks(1.5),
                slow_pct: slow,
                next_tick: ctx.tick,
                struck_recently: Vec::new(),
            },
            center,
            closing_start,
        );
    }
    Ok(true)
}

fn silvernail_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let range = ctx.eval_var(
        "Ability__Silvernail__B",
        rank,
        ValueSpec::var("RANGE", [9.0, 9.0, 9.0, 9.0, 11.0]),
    );
    let radius = ctx.eval_var(
        "Ability__Silvernail__B",
        rank,
        ValueSpec::var("IMPACT_RADIUS", [3.0; 5]),
    );
    let dur = ctx.eval_var(
        "Ability__Silvernail__B",
        rank,
        ValueSpec::var("DAMAGE_DURATION", [4.0; 5]),
    );
    let burst = ctx.eval_var(
        "Ability__Silvernail__B",
        rank,
        ValueSpec::dmg(0, [50.0, 90.0, 130.0, 170.0, 250.0], 1.0, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let dot_tick = ctx.eval_var(
        "Ability__Silvernail__B",
        rank,
        ValueSpec::dmg(1, [5.0, 10.0, 15.0, 20.0, 30.0], 0.2, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let ms = ctx.eval_var(
        "Ability__Silvernail__B",
        rank,
        ValueSpec::var("BONUS_MSPD_STRENGTH", [1.0; 5]),
    );
    let ms_dur = ctx.eval_var(
        "Ability__Silvernail__B",
        rank,
        ValueSpec::var("BONUS_MSPD_DURATION", [1.2; 5]),
    );
    ctx.add_buff(ctx.caster, BuffStatKind::MoveSpeedFlat, ms, ms_dur);

    let center = ctx.resolve_target_pos(target, range);
    let origin = ctx.caster_pos();
    let (dx, dy) = unit_dir(origin, center);
    let px = -dy;
    let py = dx;
    let centers = if ctx.talent_special() == TalentSpecial::SilvernailThreeFlasks {
        vec![
            center,
            clamp_pos(Position {
                x: center.x + px * 2.4,
                y: center.y + py * 2.4,
            }),
            clamp_pos(Position {
                x: center.x - px * 2.4,
                y: center.y - py * 2.4,
            }),
        ]
    } else {
        vec![center]
    };

    for c in centers {
        for j in ctx.enemies_in_radius(c, radius, false) {
            ctx.deal_damage(j, burst, DamageSchool::Crystal, 0.0, false);
        }
        ctx.spawn_effect(
            KitEffectKind::GroundZone {
                ability_slot: slot.idx() as u8,
                zone_kind: 46,
                radius,
                ticks_interval: TICK_HZ / 2,
                next_tick: ctx.tick + TICK_HZ / 2,
                end_tick: ctx.tick + seconds_to_ticks(dur),
                tick_damage: dot_tick * 0.5,
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
                minion_mult: 0.7,
                applies_mark: None,
            },
            c,
            c,
        );
    }
    Ok(true)
}

fn silvernail_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let range = ctx.eval_var(
        "Ability__Silvernail__C",
        rank,
        ValueSpec::var3("RANGE", [9.0; 3]),
    ) + ctx.talent_mod("BONUS_RANGE").unwrap_or(0.0);
    let origin = ctx.caster_pos();
    let aim = ctx.resolve_target_pos(target, range);
    let (dx, dy) = unit_dir(origin, aim);
    let dmg = ctx.eval_var(
        "Ability__Silvernail__C",
        rank,
        ValueSpec::dmg3(0, [120.0, 180.0, 240.0], 1.2, 0.75, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let wall_dmg = ctx.eval_var(
        "Ability__Silvernail__C",
        rank,
        ValueSpec::dmg3(1, [60.0, 90.0, 120.0], 1.2, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let stun = ctx.eval_var(
        "Ability__Silvernail__C",
        rank,
        ValueSpec::var3("STUN_DURATION", [0.6, 0.8, 1.0]),
    );
    let kb_dist = ctx
        .eval_var(
            "Ability__Silvernail__C",
            rank,
            ValueSpec::var3("MINIMUM_KNOCKBACK", [2.2; 3]),
        )
        .max(3.5);

    let mut best_target = None;
    let mut best_proj = range + 1.0;
    let caster_team = ctx.caster_team();
    for (j, e) in ctx.room.entities.iter().enumerate() {
        if e.team == caster_team || e.hp <= 0.0 {
            continue;
        }
        let vx = e.position.x - origin.x;
        let vy = e.position.y - origin.y;
        let proj = vx * dx + vy * dy;
        if proj < 0.0 || proj > range {
            continue;
        }
        let perp = (vx * dy - vy * dx).abs();
        if perp <= 1.2 && proj < best_proj {
            best_proj = proj;
            best_target = Some(j);
        }
    }

    if let Some(j) = best_target {
        ctx.deal_damage(j, dmg, DamageSchool::Weapon, 0.0, false);
        let hit_wall = ctx.knockback(j, origin, kb_dist, stun);
        let new_pos = ctx.room.entities[j].position;
        let owner_id = ctx.caster_id();
        let hit_tripwire = ctx.room.entities.spells::<KitEffect>().any(|eff| {
            eff.owner_id == owner_id
                && match eff.kind {
                    KitEffectKind::SegmentBarrier {
                        end_pos,
                        is_ring_arena: false,
                        ..
                    } => point_to_seg_distance(new_pos, eff.position, end_pos) <= 1.35,
                    _ => false,
                }
        });
        if hit_wall || hit_tripwire {
            ctx.deal_damage(j, wall_dmg, DamageSchool::Crystal, 0.0, false);
            ctx.apply_stun(j, stun);
        }
    }
    Ok(true)
}

pub(super) static SILVERNAIL: HeroKitSpec = HeroKitSpec {
    hero_id: "Silvernail",
    type_id: 432,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Perk__Silvernail",
        on_tick: Some(silvernail_perk_tick),
        on_basic_attack: Some(silvernail_perk_attack),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Silvernail__A",
        targeting: CastTargeting::GroundPoint,
        max_rank: 5,
        startup: ValueSpec::zero(),
        range: ValueSpec::var("RANGE", [8.5; 5]),
        cooldown: ValueSpec::var("COOLDOWN", [8.0, 8.0, 8.0, 8.0, 7.0]),
        cost: ValueSpec::var("ENERGY COST", [30.0, 35.0, 40.0, 45.0, 50.0]),
        charges: ValueSpec::var("CHARGES", [2.0; 5]),
        between_charges_cd: ValueSpec::constant(0.5),
        custom_exec: Some(silvernail_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Silvernail__B",
        targeting: CastTargeting::GroundPoint,
        max_rank: 5,
        startup: ValueSpec::constant(0.15),
        range: ValueSpec::var("RANGE", [9.0, 9.0, 9.0, 9.0, 11.0]),
        cooldown: ValueSpec::var("COOLDOWN", [8.0, 7.5, 7.0, 6.5, 5.0]),
        cost: ValueSpec::var("ENERGY COST", [40.0, 45.0, 50.0, 55.0, 60.0]),
        custom_exec: Some(silvernail_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Silvernail__C",
        targeting: CastTargeting::Directional,
        max_rank: 3,
        startup: ValueSpec::constant(0.15),
        range: ValueSpec::var3("RANGE", [9.0; 3]),
        cooldown: ValueSpec::var3("COOLDOWN", [60.0, 45.0, 30.0]),
        cost: ValueSpec::var3("ENERGY COST", [60.0, 70.0, 80.0]),
        custom_exec: Some(silvernail_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 49. Sayoc / Taka (tid=256)
// ============================================================================

fn taka_add_ki(ctx: &mut HeroKitContext<'_, '_>, amount: u8) {
    let stacks = ctx.add_mark(ctx.caster, marks::KI, amount, 5, 6.0);
    if stacks > 0 {
        ctx.add_buff(
            ctx.caster,
            BuffStatKind::MoveSpeedFlat,
            0.35 * f32::from(stacks),
            4.0,
        );
        ctx.add_buff(
            ctx.caster,
            BuffStatKind::CooldownReduction,
            0.08 * f32::from(stacks),
            4.0,
        );
    }
}

fn taka_perk_tick(ctx: &mut HeroKitContext<'_, '_>) {
    let ki = ctx.mark_stacks(ctx.caster, marks::KI);
    let cd = (5.0 - f32::from(ki) * 0.6).max(1.4);
    let k = ctx.room.entities[ctx.caster]
        .hero_kit
        .get_or_insert_with(Box::default);
    if k.counters[0] > 0 {
        k.counters[0] = k.counters[0].saturating_sub(1);
    } else if k.counters[1] == 0 {
        k.counters[1] = 1;
        k.counters[0] = seconds_to_ticks(cd) as u32;
    }
}

fn taka_perk_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    _target_idx: usize,
    _raw_dmg: f32,
    _is_crit: bool,
) -> (f32, DamageSchool) {
    taka_add_ki(ctx, 1);
    let ready = ctx.room.entities[ctx.caster]
        .hero_kit
        .as_ref()
        .is_some_and(|k| k.counters[1] > 0);
    if ready {
        if let Some(k) = ctx.room.entities[ctx.caster].hero_kit.as_deref_mut() {
            k.counters[1] = 0;
        }
        let bonus = ctx.eval_inputs().wp * 0.45 + 18.0;
        let heal = (ctx.eval_inputs().wp + bonus) * 0.35;
        ctx.heal_unit(ctx.caster, heal);
        ctx.add_buff(ctx.caster, BuffStatKind::MoveSpeedFlat, 1.5, 1.5);
        (bonus, DamageSchool::Weapon)
    } else {
        (0.0, DamageSchool::Weapon)
    }
}

fn taka_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let dodge = ctx.eval_var(
        "Ability__Taka__A",
        rank,
        ValueSpec::var("DodgeDuration", [0.6; 5]),
    );
    ctx.set_untargetable(ctx.caster, dodge);
    let range = ctx.eval_var("Ability__Taka__A", rank, ValueSpec::var("Range", [3.5; 5]))
        + ctx.talent_mod("BONUS_RANGE").unwrap_or(0.0);
    if let Some(j) = ctx.resolve_target_idx(target) {
        let tpos = ctx.room.entities[j].position;
        if tpos.dist(ctx.caster_pos()) <= range + 1.5 {
            let (dx, dy) = unit_dir(ctx.caster_pos(), tpos);
            let flip = clamp_pos(Position {
                x: tpos.x + dx * 1.5,
                y: tpos.y + dy * 1.5,
            });
            move_toward_or_dash(ctx, flip);
            let dmg = ctx.eval_var(
                "Ability__Taka__A",
                rank,
                ValueSpec::dmg(0, [80.0, 120.0, 160.0, 200.0, 240.0], 1.4, 0.0, 0.0),
            ) * ctx.talent_dmg_mult(slot);
            ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
        }
    }
    taka_add_ki(ctx, 1);
    Ok(true)
}

fn taka_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    rank: u8,
    _target: CastTarget,
) -> Result<bool, &'static str> {
    let dur = ctx.eval_var(
        "Ability__Taka__B",
        rank,
        ValueSpec::var("KakuDuration", [4.0, 4.5, 5.0, 5.5, 6.0]),
    ) + if ctx.talent_special() == TalentSpecial::TakaKakuHealInvuln {
        ctx.talent_mod("DURATION").unwrap_or(0.8)
    } else {
        0.0
    };
    let spd = ctx.eval_var(
        "Ability__Taka__B",
        rank,
        ValueSpec::var("SpeedBoostStrength", [1.7, 1.8, 1.9, 2.0, 2.6]),
    );
    let grace = ctx.eval_var(
        "Ability__Taka__B",
        rank,
        ValueSpec::var("Grace Period Duration", [0.7; 5]),
    );
    ctx.set_stealth(ctx.caster, dur, spd, grace);
    if ctx.talent_special() == TalentSpecial::TakaKakuHealInvuln {
        let heal =
            ctx.talent_mod("HEALING").unwrap_or(100.0) * ctx.talent_heal_mult(AbilitySlot::B);
        ctx.heal_unit(ctx.caster, heal);
        ctx.set_untargetable(ctx.caster, 0.8);
    }
    Ok(true)
}

fn taka_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let stacks = ctx
        .eval_var(
            "Ability__Taka__C",
            rank,
            ValueSpec::var3("PerkStacksGranted", [1.0, 2.0, 3.0]),
        )
        .round()
        .max(1.0) as u8;
    let wound = ctx.eval_var(
        "Ability__Taka__C",
        rank,
        ValueSpec::var3("WOUND_DURATION", [3.0; 3]),
    );
    let burst = ctx.eval_var(
        "Ability__Taka__C",
        rank,
        ValueSpec::dmg3(0, [250.0, 300.0, 350.0], 1.8, 1.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let dot_tick = ctx.eval_var(
        "Ability__Taka__C_DoT",
        rank,
        ValueSpec::dmg3(0, [30.0, 35.0, 40.0], 0.85, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);

    if let Some(j) = ctx.resolve_target_idx(target) {
        let tpos = ctx.room.entities[j].position;
        move_toward_or_dash(ctx, tpos);
        ctx.deal_damage(j, burst, DamageSchool::Crystal, 0.0, true);
        ctx.apply_mortal_wound(j, wound);
        spawn_single_hit_dot(ctx, slot, j, dot_tick, DamageSchool::Crystal, 3.0, 1.0);
        if ctx.talent_special() == TalentSpecial::TakaUltChainsTargets {
            let max_extra = ctx.talent_mod("DASH_TARGETS").unwrap_or(3.0).round() as usize;
            let extras: Vec<usize> = ctx
                .enemies_in_radius(tpos, 6.0, false)
                .into_iter()
                .filter(|&k| k != j)
                .take(max_extra)
                .collect();
            for k_idx in extras {
                ctx.deal_damage(k_idx, burst * 0.65, DamageSchool::Crystal, 0.0, false);
                ctx.apply_mortal_wound(k_idx, wound);
            }
        }
    }
    taka_add_ki(ctx, stacks);
    Ok(true)
}

pub(super) static TAKA: HeroKitSpec = HeroKitSpec {
    hero_id: "Sayoc",
    type_id: 256,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Perk__Sayoc",
        on_tick: Some(taka_perk_tick),
        on_basic_attack: Some(taka_perk_attack),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Taka__A",
        targeting: CastTargeting::TargetEnemy,
        max_rank: 5,
        startup: ValueSpec::zero(),
        range: ValueSpec::var("Range", [3.5; 5]),
        cooldown: ValueSpec::var("Cooldown", [9.0, 9.0, 9.0, 9.0, 7.5]),
        cost: ValueSpec::var("Energy Cost", [55.0, 55.0, 55.0, 55.0, 0.0]),
        custom_exec: Some(taka_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Taka__B",
        targeting: CastTargeting::InstantSelf,
        max_rank: 5,
        startup: ValueSpec::zero(),
        range: ValueSpec::constant(4.0),
        cooldown: ValueSpec::var("Cooldown", [12.0, 12.0, 12.0, 12.0, 10.0]),
        cost: ValueSpec::var("Energy Cost", [40.0, 50.0, 60.0, 70.0, 80.0]),
        custom_exec: Some(taka_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Taka__C",
        targeting: CastTargeting::TargetEnemy,
        max_rank: 3,
        startup: ValueSpec::constant(0.1),
        range: ValueSpec::var3("Range", [8.0; 3]),
        cooldown: ValueSpec::var3("Cooldown", [20.0, 17.5, 15.0]),
        cost: ValueSpec::var3("Energy Cost", [80.0, 100.0, 120.0]),
        custom_exec: Some(taka_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 53. Vox (tid=258)
// ============================================================================

fn vox_resonance_bounce(ctx: &mut HeroKitContext<'_, '_>, target_idx: usize) {
    if ctx.mark_stacks(target_idx, marks::RESONANCE) == 0 {
        return;
    }
    let b_rank = ctx.room.entities[ctx.caster].ability_ranks[1];
    let bonus = if b_rank > 0 {
        ctx.eval_var(
            "Ability__Vox__B",
            b_rank,
            ValueSpec::var("BounceBonusDamage", [15.0, 30.0, 45.0, 60.0, 90.0]),
        )
    } else {
        0.0
    };
    let bounce_dmg = (30.0 + ctx.eval_inputs().cp * 0.35 + bonus)
        * if ctx.talent_special() == TalentSpecial::VoxExtraBounce {
            ctx.talent_mod("DAMAGE_MODIFIER").unwrap_or(1.0)
        } else {
            1.0
        };
    let max_bounces = if ctx.talent_special() == TalentSpecial::VoxExtraBounce {
        3
    } else {
        2
    };
    let tpos = ctx.room.entities[target_idx].position;
    let others: Vec<usize> = ctx
        .enemies_in_radius(tpos, 5.5, false)
        .into_iter()
        .filter(|&j| j != target_idx)
        .take(max_bounces)
        .collect();
    for j in others {
        ctx.deal_damage(j, bounce_dmg, DamageSchool::Crystal, 0.0, false);
    }
}

fn vox_perk_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    target_idx: usize,
    _raw_dmg: f32,
    _is_crit: bool,
) -> (f32, DamageSchool) {
    vox_resonance_bounce(ctx, target_idx);
    if ctx.talent_special() == TalentSpecial::VoxBasicAttackBarrier {
        let ratio = ctx.talent_mod("MAX_HEALTH_TO_BARRIER").unwrap_or(0.05);
        let dur = ctx.talent_mod("BARRIER_DURATION").unwrap_or(4.0);
        let bar = ctx.room.entities[ctx.caster].max_hp * ratio;
        ctx.grant_barrier(ctx.caster, bar, dur, Some((bar * 2.0, 2)));
    }
    let cp_on_hit = 10.0 + ctx.eval_inputs().cp * 0.25;
    (cp_on_hit, DamageSchool::Crystal)
}

fn vox_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let dist = ctx.eval_var(
        "Ability__Vox__A",
        rank,
        ValueSpec::var("Dash Distance", [3.8; 5]),
    ) + ctx.talent_mod("BONUS_DISTANCE").unwrap_or(0.0);
    let dest = ctx.resolve_target_pos(target, dist);
    move_toward_or_dash(ctx, dest);

    let scale = ctx.eval_var(
        "Ability__Vox__A",
        rank,
        ValueSpec::var("Damage Scaling", [0.5, 0.6, 0.7, 0.8, 1.0]),
    );
    let shot_dmg = (ctx.eval_inputs().wp * scale + ctx.eval_inputs().cp * 0.35 + 15.0)
        * ctx.talent_dmg_mult(slot);
    let range = 7.0;
    let nearest = ctx
        .enemies_in_radius(dest, range, false)
        .into_iter()
        .min_by(|&a, &b| {
            let ra = ctx.mark_stacks(a, marks::RESONANCE) > 0;
            let rb = ctx.mark_stacks(b, marks::RESONANCE) > 0;
            rb.cmp(&ra).then_with(|| {
                ctx.room.entities[a]
                    .position
                    .dist(dest)
                    .total_cmp(&ctx.room.entities[b].position.dist(dest))
            })
        });
    if let Some(j) = nearest {
        for _ in 0..2 {
            ctx.deal_damage(j, shot_dmg, DamageSchool::Weapon, 0.0, false);
            vox_resonance_bounce(ctx, j);
        }
    }
    Ok(true)
}

fn vox_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    rank: u8,
    _target: CastTarget,
) -> Result<bool, &'static str> {
    let rad = ctx.eval_var(
        "Ability__Vox__B",
        rank,
        ValueSpec::var("Pulse Max Range Radius", [10.0; 5]),
    );
    let slow = ctx.eval_var(
        "Ability__Vox__B",
        rank,
        ValueSpec::var("SlowAtCenter", [0.15, 0.17, 0.19, 0.21, 0.25]),
    );
    let sdur = ctx.eval_var(
        "Ability__Vox__B",
        rank,
        ValueSpec::var("Slow Duration", [1.2; 5]),
    );
    for j in ctx.enemies_in_radius(ctx.caster_pos(), rad, false) {
        ctx.add_mark(j, marks::RESONANCE, 1, 1, 6.0);
        ctx.apply_slow(j, slow, sdur);
    }
    Ok(true)
}

fn vox_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let origin = ctx.caster_pos();
    let aim = ctx.resolve_target_pos(target, 12.0);
    let (dx, dy) = unit_dir(origin, aim);
    let end = clamp_pos(Position {
        x: origin.x + dx * 12.0,
        y: origin.y + dy * 12.0,
    });
    let initial = ctx.eval_var(
        "Ability__Vox__C",
        rank,
        ValueSpec::dmg3(0, [100.0, 175.0, 250.0], 0.6, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let shockwave = ctx.eval_var(
        "Ability__Vox__C",
        rank,
        ValueSpec::var_scaled(
            "ShockwaveDamage",
            [200.0, 350.0, 500.0, 500.0, 500.0],
            1.2,
            0.0,
            0.0,
            0.0,
        ),
    ) * ctx.talent_dmg_mult(slot);
    let silence = ctx.eval_var(
        "Ability__Vox__C",
        rank,
        ValueSpec::var3("Silence Duration", [0.6, 0.8, 1.0]),
    );
    let delay = ctx.eval_var("Ability__Vox__C", rank, ValueSpec::var3("Delay", [1.0; 3]));
    ctx.spawn_effect(
        KitEffectKind::Projectile {
            ability_slot: slot.idx() as u8,
            rank,
            speed_per_tick: 20.0 / TICK_HZ as f32,
            radius: 1.6,
            damage: initial,
            school: DamageSchool::Crystal,
            piercing: true,
            returning: false,
            return_to_caster: false,
            repeat_mult: 1.0,
            splash_damage: 0.0,
            splash_radius: 0.0,
            stun_ticks: 0,
            root_ticks: 0,
            silence_ticks: seconds_to_ticks(silence),
            slow_pct: 0.0,
            slow_ticks: 0,
            minion_mult: 0.7,
            resets_cd_on_hit: false,
            applies_mark: Some((marks::RESONANCE, 1, seconds_to_ticks(6.0))),
            destroys_projectiles: false,
            hit_actors: Vec::new(),
        },
        origin,
        end,
    );
    let mid = clamp_pos(Position {
        x: origin.x + dx * 6.0,
        y: origin.y + dy * 6.0,
    });
    ctx.spawn_effect(
        KitEffectKind::DelayedBlast {
            ability_slot: slot.idx() as u8,
            release_tick: ctx.tick + seconds_to_ticks(delay),
            radius: 7.0,
            inner_radius: 0.0,
            damage: shockwave,
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
            minion_mult: 0.7,
            applies_mark: None,
        },
        mid,
        mid,
    );
    Ok(true)
}

pub(super) static VOX: HeroKitSpec = HeroKitSpec {
    hero_id: "Vox",
    type_id: 258,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Perk__Vox",
        on_basic_attack: Some(vox_perk_attack),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Vox__A",
        targeting: CastTargeting::GroundPoint,
        max_rank: 5,
        startup: ValueSpec::constant(0.1),
        range: ValueSpec::var("Dash Distance", [3.8; 5]),
        cooldown: ValueSpec::var("Cooldown", [7.0, 6.5, 6.0, 5.5, 4.0]),
        cost: ValueSpec::var("Energy Cost", [40.0, 45.0, 50.0, 55.0, 60.0]),
        custom_exec: Some(vox_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Vox__B",
        targeting: CastTargeting::InstantSelf,
        max_rank: 5,
        startup: ValueSpec::zero(),
        range: ValueSpec::var("Pulse Max Range Radius", [10.0; 5]),
        cooldown: ValueSpec::var("Cooldown", [8.0, 8.0, 8.0, 8.0, 6.0]),
        cost: ValueSpec::var("Energy Cost", [60.0, 70.0, 80.0, 90.0, 100.0]),
        custom_exec: Some(vox_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Vox__C",
        targeting: CastTargeting::Directional,
        max_rank: 3,
        startup: ValueSpec::constant(0.15),
        range: ValueSpec::constant(12.0),
        cooldown: ValueSpec::var3("Cooldown", [70.0, 60.0, 50.0]),
        cost: ValueSpec::var3("Energy Cost", [100.0; 3]),
        custom_exec: Some(vox_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 56. Ylva (tid=436)
// ============================================================================

fn ylva_perk_tick(ctx: &mut HeroKitContext<'_, '_>) {
    if !ctx.tick.is_multiple_of(5) {
        return;
    }
    for j in ctx.enemies_in_radius(ctx.caster_pos(), 12.0, true) {
        ctx.apply_reveal(j, 0.6);
    }
}

fn ylva_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let bonus = ctx.eval_var(
        "Ability__Ylva__A",
        rank,
        ValueSpec::var_scaled(
            "BonusDamage",
            [80.0, 120.0, 160.0, 200.0, 240.0],
            1.0,
            0.0,
            0.0,
            0.0,
        ),
    ) * ctx.talent_dmg_mult(slot);
    let stun = ctx.eval_var(
        "Ability__Ylva__A",
        rank,
        ValueSpec::var("StunDuration", [1.0; 5]),
    );
    if let Some(j) = ctx.resolve_target_idx(target) {
        let dest = ctx.room.entities[j].position;
        move_toward_or_dash(ctx, dest);
        let dmg = bonus + ctx.eval_inputs().wp * 0.9;
        ctx.deal_damage(j, dmg, DamageSchool::Weapon, 0.0, true);
        ctx.apply_stun(j, stun);
    }
    Ok(true)
}

fn ylva_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let range = ctx.eval_var(
        "Ability__Ylva__B",
        rank,
        ValueSpec::var("Range", [5.5, 5.5, 5.5, 5.5, 7.5]),
    );
    let origin = ctx.caster_pos();
    let dest = ctx.resolve_target_pos(target, range);
    if ctx.talent_special() == TalentSpecial::YlvaDropMineOnB {
        let c_rank = ctx.room.entities[ctx.caster].ability_ranks[2].max(1);
        let c_dmg = ctx.eval_var(
            "Ability__Ylva__C",
            c_rank,
            ValueSpec::dmg3(0, [300.0, 450.0, 600.0], 1.5, 0.0, 0.0),
        );
        ctx.spawn_effect(
            KitEffectKind::TrapZone {
                ability_slot: 2,
                armed_at: ctx.tick + seconds_to_ticks(1.2),
                end_tick: ctx.tick + seconds_to_ticks(45.0),
                trigger_radius: 2.5,
                blast_radius: 6.0,
                damage: c_dmg,
                school: DamageSchool::Crystal,
                root_ticks: seconds_to_ticks(1.6),
                stun_ticks: 0,
                slow_pct: 0.0,
                reveals: true,
                detonate_on_caster_damage: false,
            },
            origin,
            origin,
        );
    }
    move_toward_or_dash(ctx, dest);
    let dmg = ctx.eval_var(
        "Ability__Ylva__B",
        rank,
        ValueSpec::dmg(0, [140.0, 190.0, 240.0, 290.0, 390.0], 1.75, 1.4, 0.0),
    ) * 0.75
        * ctx.talent_dmg_mult(slot);
    for j in ctx.enemies_in_radius(dest, 2.5, false) {
        ctx.deal_damage(j, dmg, DamageSchool::Weapon, 0.0, false);
    }
    let bdur = ctx.eval_var(
        "Ability__Ylva__B",
        rank,
        ValueSpec::var("SpeedBuffDuration", [3.0; 5]),
    );
    let aspd = ctx.eval_var(
        "Ability__Ylva__B",
        rank,
        ValueSpec::var("BonusAttackSpeed", [0.4, 0.45, 0.5, 0.55, 0.6]),
    );
    let ms = ctx.eval_var(
        "Ability__Ylva__B",
        rank,
        ValueSpec::var("BonusMoveSpeed", [1.5, 1.5, 1.5, 1.5, 2.0]),
    );
    ctx.add_buff(ctx.caster, BuffStatKind::AttackSpeed, aspd, bdur);
    ctx.add_buff(ctx.caster, BuffStatKind::MoveSpeedFlat, ms, bdur);
    Ok(true)
}

const YLVA_C_EFFECTS: &[PrimitiveEffect] = &[PrimitiveEffect::TrapZone {
    arm_delay: ValueSpec::var3("ArmDuration", [2.0; 3]),
    duration: ValueSpec::var3("TrapDuration", [60.0; 3]),
    trigger_radius: ValueSpec::var3("TrapTriggerRadius", [2.5; 3]),
    blast_radius: ValueSpec::var3("ExplosionRadius", [6.0; 3]),
    damage: ValueSpec::dmg3(0, [300.0, 450.0, 600.0], 1.5, 0.0, 0.0),
    school: DamageKind::Crystal,
    root_seconds: ValueSpec::var3("RootDuration", [1.6, 1.8, 2.0]),
    stun_seconds: ValueSpec::zero(),
    slow_pct: ValueSpec::zero(),
    max_traps: 1,
    reveals_enemies: true,
}];

pub(super) static YLVA: HeroKitSpec = HeroKitSpec {
    hero_id: "Ylva",
    type_id: 436,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Perk__Ylva",
        on_tick: Some(ylva_perk_tick),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Ylva__A",
        targeting: CastTargeting::TargetEnemy,
        max_rank: 5,
        startup: ValueSpec::constant(0.2),
        range: ValueSpec::constant(5.5),
        cooldown: ValueSpec::var("Cooldown", [8.0, 7.5, 7.0, 6.5, 6.0]),
        cost: ValueSpec::var("Energy Cost", [25.0, 30.0, 35.0, 40.0, 45.0]),
        custom_exec: Some(ylva_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Ylva__B",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::zero(),
        range: ValueSpec::var("Range", [5.5, 5.5, 5.5, 5.5, 7.5]),
        cooldown: ValueSpec::var("Cooldown", [10.0, 9.5, 9.0, 8.5, 8.0]),
        cost: ValueSpec::var("Energy Cost", [25.0, 30.0, 35.0, 40.0, 45.0]),
        custom_exec: Some(ylva_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Ylva__C",
        targeting: CastTargeting::GroundPoint,
        max_rank: 3,
        startup: ValueSpec::zero(),
        range: ValueSpec::constant(6.0),
        cooldown: ValueSpec::var3("Cooldown", [25.0, 20.0, 15.0]),
        cost: ValueSpec::var3("Energy Cost", [50.0, 60.0, 70.0]),
        charges: ValueSpec::constant(1.0),
        between_charges_cd: ValueSpec::constant(1.0),
        effects: YLVA_C_EFFECTS,
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};
