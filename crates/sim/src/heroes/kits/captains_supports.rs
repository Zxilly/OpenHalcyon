//! Captain, support & event hero kit specifications (17 heroes).

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
// 1. Adagio (tid=244) - bespoke A/B/C; adds Arcane Renewal perk
// ============================================================================

fn adagio_on_ability_hit(ctx: &mut HeroKitContext<'_, '_>, target_idx: usize, dmg: f32) {
    if ctx.mark_stacks(target_idx, marks::ARCANE_FIRE) > 0 {
        let e = &mut ctx.room.entities[ctx.caster];
        e.energy = (e.energy + dmg * 0.35).min(e.max_energy);
    }
}

pub(super) static ADAGIO: HeroKitSpec = HeroKitSpec {
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

pub(super) static ARDAN: HeroKitSpec = HeroKitSpec {
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

pub(super) static CATHERINE: HeroKitSpec = HeroKitSpec {
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

pub(super) static CHURNWALKER: HeroKitSpec = HeroKitSpec {
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

pub(super) static FLICKER: HeroKitSpec = HeroKitSpec {
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

pub(super) static FORTRESS: HeroKitSpec = HeroKitSpec {
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

pub(super) static GRACE: HeroKitSpec = HeroKitSpec {
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

pub(super) static LANCE: HeroKitSpec = HeroKitSpec {
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
// 30. Lorelai (tid=409)
// ============================================================================

const LORELAI_POOL_ZONE: u8 = 30;

fn lorelai_consume_pool_near(ctx: &mut HeroKitContext<'_, '_>, pos: Position) -> bool {
    let owner_id = ctx.caster_id();
    let pool_entity = ctx
        .room
        .entities
        .ecs
        .query::<(bevy_ecs::entity::Entity, &KitEffect)>()
        .iter(&ctx.room.entities.ecs)
        .find_map(|(eh, eff)| {
            (eff.owner_id == owner_id
                && matches!(
                    eff.kind,
                    KitEffectKind::GroundZone {
                        zone_kind: LORELAI_POOL_ZONE,
                        radius,
                        ..
                    } if eff.position.dist(pos) <= radius + 1.5
                ))
            .then_some(eh)
        });
    if let Some(eh) = pool_entity {
        ctx.room.entities.remove_spell_handle::<KitEffect>(eh);
        true
    } else {
        false
    }
}

fn lorelai_perk_tick(ctx: &mut HeroKitContext<'_, '_>) {
    let standing = has_nearby_active_zone(ctx, ctx.caster_pos(), LORELAI_POOL_ZONE, 0.5);
    if standing {
        ctx.add_mark(ctx.caster, marks::WATER_POOL_BUFF, 1, 1, 0.6);
        let bonus_cp = 10.0 + f32::from(ctx.room.entities[ctx.caster].level) * 2.5;
        ctx.add_buff(ctx.caster, BuffStatKind::CrystalPowerFlat, bonus_cp, 0.5);
    }
}

fn lorelai_perk_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    target_idx: usize,
    _base_damage: f32,
    _was_crit: bool,
) -> (f32, DamageSchool) {
    let pos = ctx.caster_pos();
    let tpos = ctx.room.entities[target_idx].position;
    let in_pool = has_nearby_active_zone(ctx, pos, LORELAI_POOL_ZONE, 0.5)
        || has_nearby_active_zone(ctx, tpos, LORELAI_POOL_ZONE, 0.5);
    if in_pool {
        let cp = ctx.eval_inputs().cp;
        let bonus = 15.0 + f32::from(ctx.room.entities[ctx.caster].level) * 3.0 + cp * 0.4;
        (bonus, DamageSchool::Crystal)
    } else {
        (0.0, DamageSchool::Crystal)
    }
}

fn lorelai_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let range = ctx.eval_var(
        "Ability__Lorelai__A",
        rank,
        ValueSpec::var("RANGE", [8.0, 8.0, 8.0, 8.0, 9.5]),
    );
    let center = ctx.resolve_target_pos(target, range);
    let empowered = lorelai_consume_pool_near(ctx, center);
    let dmg = if empowered {
        ctx.eval_var(
            "Ability__Lorelai__A",
            rank,
            ValueSpec::dmg(1, [55.0, 95.0, 135.0, 175.0, 255.0], 1.1, 0.0, 0.0),
        )
    } else {
        ctx.eval_var(
            "Ability__Lorelai__A",
            rank,
            ValueSpec::dmg(0, [40.0, 75.0, 110.0, 145.0, 215.0], 0.8, 0.0, 0.0),
        )
    } * ctx.talent_dmg_mult(slot);
    let cp = ctx.eval_inputs().cp;
    let mut stun = (ctx.eval_var(
        "Ability__Lorelai__A",
        rank,
        ValueSpec::var("STUN_DURATION", [0.5; 5]),
    ) + cp * 0.0002)
        .min(1.0);
    if empowered {
        stun *= 2.0;
    }
    if ctx.talent_special() == TalentSpecial::LorelaiStunExtended
        && let Some(extra) = ctx.talent_mod("STUN_DURATION")
    {
        stun += extra;
    }
    let delay = ctx.eval_var(
        "Ability__Lorelai__A",
        rank,
        ValueSpec::var("REG_DELAY_TIME", [0.8; 5]),
    );
    ctx.spawn_effect(
        KitEffectKind::DelayedBlast {
            ability_slot: slot.idx() as u8,
            release_tick: ctx.tick + seconds_to_ticks(delay),
            radius: 2.5,
            inner_radius: 0.0,
            damage: dmg,
            outer_mult: 1.0,
            school: DamageSchool::Crystal,
            stun_ticks: seconds_to_ticks(stun),
            root_ticks: 0,
            sleep_ticks: 0,
            sleep_unwaking: false,
            slow_pct: 0.0,
            slow_ticks: 0,
            reveal_ticks: 0,
            bonus_mark: None,
            minion_mult: 0.65,
            applies_mark: None,
        },
        center,
        center,
    );
    Ok(true)
}

fn lorelai_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let center = ctx.resolve_target_pos(target, 9.0);
    let empowered = lorelai_consume_pool_near(ctx, center);
    let speed = ctx.eval_var(
        "Ability__Lorelai__B",
        rank,
        ValueSpec::var("SPEED_BOOST", [1.0, 1.2, 1.4, 1.6, 2.0]),
    );
    let cp = ctx.eval_inputs().cp;
    let slow = (ctx.eval_var(
        "Ability__Lorelai__B",
        rank,
        ValueSpec::var("SLOW", [0.2; 5]),
    ) + cp * 0.0002)
        .min(0.5);
    let radius = if empowered { 4.2 } else { 3.2 };
    let dur = if empowered { 6.0 } else { 4.0 };
    ctx.spawn_effect(
        KitEffectKind::GroundZone {
            ability_slot: slot.idx() as u8,
            zone_kind: LORELAI_POOL_ZONE,
            radius,
            ticks_interval: TICK_HZ / 2,
            next_tick: ctx.tick,
            end_tick: ctx.tick + seconds_to_ticks(dur),
            tick_damage: 0.0,
            tick_heal: 0.0,
            school: DamageSchool::Crystal,
            slow_pct: slow,
            ally_speed_bonus: speed,
            end_damage: 0.0,
            end_root_ticks: 0,
            end_stun_ticks: 0,
            drift_dx: 0.0,
            drift_dy: 0.0,
            follow_caster: false,
            ignited: false,
            minion_mult: 1.0,
            applies_mark: None,
        },
        center,
        center,
    );
    Ok(true)
}

fn lorelai_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let tj = ctx.resolve_target_idx(target).unwrap_or(ctx.caster);
    let empowered = lorelai_consume_pool_near(ctx, ctx.room.entities[tj].position);
    let hp_ratio = ctx.eval_var(
        "Ability__Lorelai__C",
        rank,
        ValueSpec::var3("BARRIER_RATIO", [0.15; 3]),
    );
    let base_bar = if empowered {
        ctx.eval_var(
            "Ability__Lorelai__C",
            rank,
            ValueSpec::var_scaled(
                "E_BARRIER_AMOUNT",
                [750.0, 1125.0, 1500.0, 1500.0, 1500.0],
                1.5,
                0.0,
                0.0,
                0.0,
            ),
        )
    } else {
        ctx.eval_var(
            "Ability__Lorelai__C",
            rank,
            ValueSpec::var_scaled(
                "BARRIER_AMOUNT",
                [500.0, 750.0, 1000.0, 1000.0, 1000.0],
                1.0,
                0.0,
                0.0,
                0.0,
            ),
        )
    };
    let bar = (base_bar + ctx.room.entities[ctx.caster].bonus_hp * hp_ratio)
        * ctx.talent_barrier_mult(slot);
    let dur = if empowered {
        ctx.eval_var(
            "Ability__Lorelai__C",
            rank,
            ValueSpec::var3("E_DURATION", [6.0; 3]),
        )
    } else {
        ctx.eval_var(
            "Ability__Lorelai__C",
            rank,
            ValueSpec::var3("DURATION", [4.0; 3]),
        )
    };
    ctx.cleanse_cc(tj);
    ctx.grant_barrier(tj, bar, dur, None);
    if ctx.talent_special() == TalentSpecial::LorelaiUltInvulnerability {
        let inv_dur = ctx.talent_mod("INVULN_DURATION").unwrap_or(3.0);
        ctx.set_untargetable(tj, inv_dur);
    }
    Ok(true)
}

pub(super) static LORELAI: HeroKitSpec = HeroKitSpec {
    hero_id: "Lorelai",
    type_id: 409,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Perk__Lorelai",
        on_tick: Some(lorelai_perk_tick),
        on_basic_attack: Some(lorelai_perk_attack),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Lorelai__A",
        targeting: CastTargeting::GroundPoint,
        max_rank: 5,
        startup: ValueSpec::var("STARTUP_DELAY", [0.4; 5]),
        range: ValueSpec::var("RANGE", [8.0, 8.0, 8.0, 8.0, 9.5]),
        cooldown: ValueSpec::var("COOLDOWN", [12.0, 11.5, 11.0, 10.5, 10.0]),
        cost: ValueSpec::var("ENERGY COST", [60.0, 70.0, 80.0, 90.0, 100.0]),
        custom_exec: Some(lorelai_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Lorelai__B",
        targeting: CastTargeting::GroundPoint,
        max_rank: 5,
        startup: ValueSpec::var("STARTUP_DELAY", [0.3; 5]),
        range: ValueSpec::constant(9.0),
        cooldown: ValueSpec::var("Cooldown", [10.0, 9.0, 8.0, 7.0, 5.0]),
        cost: ValueSpec::var("ENERGY COST", [50.0, 60.0, 70.0, 80.0, 40.0]),
        charges: ValueSpec::constant(1.0),
        between_charges_cd: ValueSpec::constant(0.8),
        custom_exec: Some(lorelai_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Lorelai__C",
        targeting: CastTargeting::TargetAllyOrSelf,
        max_rank: 3,
        startup: ValueSpec::var3("ANIMATION_DELAY", [0.3; 3]),
        range: ValueSpec::constant(12.0),
        cooldown: ValueSpec::var3("COOLDOWN", [90.0, 75.0, 60.0]),
        cost: ValueSpec::var3("ENERGY COST", [100.0, 125.0, 150.0]),
        custom_exec: Some(lorelai_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 31. Lyra (tid=269)
// ============================================================================

const LYRA_SIGIL_ZONE: u8 = 31;

fn lyra_perk_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    target_idx: usize,
    _base_damage: f32,
    _was_crit: bool,
) -> (f32, DamageSchool) {
    let inputs = ctx.eval_inputs();
    let cp = inputs.cp;
    let lvl = f32::from(ctx.room.entities[ctx.caster].level);
    let twin = ctx.talent_special() == TalentSpecial::LyraTwinMissilesNoSlow;
    let heavy = {
        let st = ctx.room.entities[ctx.caster]
            .hero_kit
            .get_or_insert_with(Box::default);
        st.counters[0] = (st.counters[0] + 1) % 2;
        st.counters[0] == 0
    };
    let mut bonus = if heavy {
        22.0 + lvl * 5.0 + cp * 0.55
    } else {
        12.0 + lvl * 3.0 + cp * 0.3
    };
    if twin {
        let mod_v = ctx.talent_mod("DAMAGE_MODIFIER").unwrap_or(0.65);
        bonus = (bonus + inputs.wp * 0.35) * (1.0 + mod_v);
    } else if heavy {
        ctx.apply_slow(target_idx, 0.3, 1.2);
    }
    (bonus, DamageSchool::Crystal)
}

fn lyra_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let range = ctx.eval_var(
        "Ability__Lyra__A",
        rank,
        ValueSpec::var("Range", [10.0, 10.0, 10.0, 10.0, 12.0]),
    );
    let center = ctx.resolve_target_pos(target, range);
    let rad = ctx.eval_var("Ability__Lyra__A", rank, ValueSpec::var("Radius", [3.5; 5]));
    let dur = ctx.eval_var(
        "Ability__Lyra__A",
        rank,
        ValueSpec::var("Duration", [12.0; 5]),
    );
    let dps = ctx.eval_var(
        "Ability__Lyra__A",
        rank,
        ValueSpec::var_scaled(
            "DamagePerSecond",
            [65.0, 110.0, 155.0, 200.0, 290.0],
            0.5,
            0.0,
            0.0,
            0.0,
        ),
    );
    let hps = ctx.eval_var(
        "Ability__Lyra__A",
        rank,
        ValueSpec::var_scaled(
            "HealPerSecond",
            [25.0, 35.0, 45.0, 55.0, 75.0],
            0.0,
            0.0,
            0.0,
            0.15,
        ),
    );
    ctx.spawn_effect(
        KitEffectKind::GroundZone {
            ability_slot: slot.idx() as u8,
            zone_kind: LYRA_SIGIL_ZONE,
            radius: rad,
            ticks_interval: TICK_HZ / 2,
            next_tick: ctx.tick,
            end_tick: ctx.tick + seconds_to_ticks(dur),
            tick_damage: dps,
            tick_heal: hps,
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
            minion_mult: 0.75,
            applies_mark: None,
        },
        center,
        center,
    );
    ctx.open_recast(AbilitySlot::A, 1, 0, dur.min(8.0));
    Ok(true)
}

fn lyra_a_detonate(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    _target: CastTarget,
) -> Result<bool, &'static str> {
    ctx.clear_recast(AbilitySlot::A);
    let owner_id = ctx.caster_id();
    let mut sigil_center = None;
    let eh_found = ctx
        .room
        .entities
        .ecs
        .query::<(bevy_ecs::entity::Entity, &KitEffect)>()
        .iter(&ctx.room.entities.ecs)
        .find_map(|(eh, eff)| {
            if eff.owner_id == owner_id
                && matches!(
                    eff.kind,
                    KitEffectKind::GroundZone {
                        zone_kind: LYRA_SIGIL_ZONE,
                        ..
                    }
                )
            {
                sigil_center = Some(eff.position);
                Some(eh)
            } else {
                None
            }
        });
    if let Some(eh) = eh_found {
        ctx.room.entities.remove_spell_handle::<KitEffect>(eh);
    }
    let center = sigil_center.unwrap_or_else(|| ctx.caster_pos());
    let dmg = ctx.eval_var(
        "Ability__Lyra__A",
        rank,
        ValueSpec::var_scaled(
            "DetonateDamageHeroes",
            [80.0, 130.0, 180.0, 230.0, 330.0],
            0.6,
            0.0,
            0.0,
            0.0,
        ),
    ) * ctx.talent_dmg_mult(slot);
    let heal = ctx.eval_var(
        "Ability__Lyra__A",
        rank,
        ValueSpec::var_scaled(
            "DetonateHeal",
            [40.0, 60.0, 80.0, 100.0, 140.0],
            0.0,
            0.0,
            0.0,
            0.15,
        ),
    ) * ctx.talent_heal_mult(slot);
    let rad = 3.5;
    for j in ctx.enemies_in_radius(center, rad, false) {
        let m = if ctx.room.entities[j].kind == Kind::Hero {
            1.0
        } else {
            0.5
        };
        ctx.deal_damage(j, dmg * m, DamageSchool::Crystal, 0.0, false);
    }
    let ms = ctx.eval_var(
        "Ability__Lyra__A",
        rank,
        ValueSpec::var("SpeedBoost (Decaying)", [1.5; 5]),
    );
    let ms_dur = ctx.eval_var(
        "Ability__Lyra__A",
        rank,
        ValueSpec::var("SpeedBoostDuration", [2.0; 5]),
    );
    for a in ctx.allies_in_radius(center, rad, true, true) {
        ctx.heal_unit(a, heal);
        ctx.add_buff(a, BuffStatKind::MoveSpeedFlat, ms, ms_dur);
    }
    Ok(true)
}

fn lyra_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    _target: CastTarget,
) -> Result<bool, &'static str> {
    let origin = ctx.caster_pos();
    let rad = ctx.eval_var("Ability__Lyra__B", rank, ValueSpec::var("Radius", [5.5; 5]));
    let dmg = ctx.eval_var(
        "Ability__Lyra__B",
        rank,
        ValueSpec::dmg(0, [80.0, 160.0, 240.0, 320.0, 400.0], 0.3, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let slow = ctx.eval_var(
        "Ability__Lyra__B",
        rank,
        ValueSpec::var("NonDecaying Slow Amount", [0.3, 0.3, 0.3, 0.3, 0.5]),
    );
    let slow_dur = ctx.eval_var(
        "Ability__Lyra__B",
        rank,
        ValueSpec::var("Slow Duration", [1.5, 1.5, 1.5, 1.5, 2.5]),
    );
    let dur = ctx.eval_var(
        "Ability__Lyra__B",
        rank,
        ValueSpec::var("Duration", [3.0; 5]),
    );
    for j in ctx.enemies_in_radius(origin, rad, false) {
        ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
        ctx.apply_slow(j, slow, slow_dur);
    }
    ctx.spawn_effect(
        KitEffectKind::GroundZone {
            ability_slot: slot.idx() as u8,
            zone_kind: 32,
            radius: rad,
            ticks_interval: TICK_HZ / 2,
            next_tick: ctx.tick + TICK_HZ / 2,
            end_tick: ctx.tick + seconds_to_ticks(dur),
            tick_damage: 0.0,
            tick_heal: 0.0,
            school: DamageSchool::Crystal,
            slow_pct: slow,
            ally_speed_bonus: 0.0,
            end_damage: 0.0,
            end_root_ticks: 0,
            end_stun_ticks: 0,
            drift_dx: 0.0,
            drift_dy: 0.0,
            follow_caster: false,
            ignited: false,
            minion_mult: 1.0,
            applies_mark: None,
        },
        origin,
        origin,
    );
    if ctx.talent_special() == TalentSpecial::LyraGythianWardCleanseBarrier {
        let b_dur = ctx.talent_mod("BARRIER_DURATION").unwrap_or(2.0);
        let pct = ctx.talent_mod("MAX_HEALTH_TO_BARRIER").unwrap_or(0.1);
        for a in ctx.allies_in_radius(origin, rad, true, true) {
            ctx.cleanse_cc(a);
            let amt = ctx.room.entities[a].max_hp * pct * ctx.talent_barrier_mult(slot);
            ctx.grant_barrier(a, amt, b_dur, None);
        }
    }
    Ok(true)
}

fn lyra_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let origin = ctx.caster_pos();
    let range = ctx.eval_var(
        "Ability__Lyra__C",
        rank,
        ValueSpec::var3("Range", [10.0, 20.0, 30.0]),
    );
    let dest = ctx.resolve_target_pos(target, range);
    move_toward_or_dash(ctx, dest);
    let portal_dur = ctx.eval_var(
        "Ability__Lyra__C",
        rank,
        ValueSpec::var3("Portal Duration", [8.5; 3]),
    );
    let portal_rad = ctx.eval_var(
        "Ability__Lyra__C",
        rank,
        ValueSpec::var3("Portal Radius", [1.2; 3]),
    );
    let speed = ctx.eval_var(
        "Ability__Lyra__C",
        rank,
        ValueSpec::var3("Decaying Speed Boost", [2.4; 3]),
    );
    let spd_dur = ctx.eval_var(
        "Ability__Lyra__C",
        rank,
        ValueSpec::var3("Speed Boost Duration", [1.0; 3]),
    );
    for portal_pos in [origin, dest] {
        ctx.spawn_effect(
            KitEffectKind::GroundZone {
                ability_slot: slot.idx() as u8,
                zone_kind: 33,
                radius: portal_rad.max(1.2),
                ticks_interval: TICK_HZ / 2,
                next_tick: ctx.tick,
                end_tick: ctx.tick + seconds_to_ticks(portal_dur),
                tick_damage: 0.0,
                tick_heal: 0.0,
                school: DamageSchool::Crystal,
                slow_pct: 0.0,
                ally_speed_bonus: speed,
                end_damage: 0.0,
                end_root_ticks: 0,
                end_stun_ticks: 0,
                drift_dx: 0.0,
                drift_dy: 0.0,
                follow_caster: false,
                ignited: false,
                minion_mult: 1.0,
                applies_mark: None,
            },
            portal_pos,
            portal_pos,
        );
    }
    ctx.add_buff(ctx.caster, BuffStatKind::MoveSpeedFlat, speed, spd_dur);
    Ok(true)
}

static LYRA_VARIANTS: &[AbilityVariantSpec] = &[AbilityVariantSpec {
    slot: HeroSlot::A2,
    spec: AbilityKitSpec {
        ability_id: "Ability__Lyra__A_Detonate",
        targeting: CastTargeting::InstantSelf,
        max_rank: 5,
        cooldown: ValueSpec::zero(),
        cost: ValueSpec::zero(),
        custom_exec: Some(lyra_a_detonate),
        ..AbilityKitSpec::DEFAULT
    },
}];

pub(super) static LYRA: HeroKitSpec = HeroKitSpec {
    hero_id: "Lyra",
    type_id: 269,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Perk__Lyra",
        on_basic_attack: Some(lyra_perk_attack),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Lyra__A",
        targeting: CastTargeting::GroundPoint,
        max_rank: 5,
        startup: ValueSpec::var("Startup", [0.2; 5]),
        range: ValueSpec::var("Range", [10.0, 10.0, 10.0, 10.0, 12.0]),
        cooldown: ValueSpec::var("Cooldown", [14.0, 13.0, 12.0, 11.0, 10.0]),
        cost: ValueSpec::var("Energy Cost", [80.0, 100.0, 120.0, 140.0, 160.0]),
        custom_exec: Some(lyra_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Lyra__B",
        targeting: CastTargeting::InstantSelf,
        max_rank: 5,
        startup: ValueSpec::var("Startup", [0.4; 5]),
        range: ValueSpec::var("Radius", [5.5; 5]),
        cooldown: ValueSpec::var("Cooldown", [18.0, 17.0, 16.0, 15.0, 12.0]),
        cost: ValueSpec::var("Energy Cost", [60.0, 70.0, 80.0, 90.0, 100.0]),
        custom_exec: Some(lyra_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Lyra__C",
        targeting: CastTargeting::GroundPoint,
        max_rank: 3,
        startup: ValueSpec::var3("Startup Delay", [0.3; 3]),
        range: ValueSpec::var3("Range", [10.0, 20.0, 30.0]),
        cooldown: ValueSpec::var3("Cooldown", [90.0, 80.0, 70.0]),
        cost: ValueSpec::var3("Energy Cost", [150.0, 175.0, 200.0]),
        passive_bonus: Some((
            BuffStatKind::AttackRange,
            ValueSpec::var3("Passive Attack Range Bonus", [0.5, 1.0, 1.5]),
        )),
        custom_exec: Some(lyra_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    variants: LYRA_VARIANTS,
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 37. Phinn (tid=279) — Unstoppable CC downgrade + armored guardian
// ============================================================================

fn phinn_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let center = ctx.resolve_target_pos(target, 6.5);
    let dmg = ctx.eval_var(
        "Ability__Phinn__A",
        rank,
        ValueSpec::dmg(0, [200.0, 300.0, 400.0, 500.0, 600.0], 1.7, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let cc_dur = ctx.eval_var(
        "Ability__Phinn__A",
        rank,
        ValueSpec::var("CCDuration", [1.1; 5]),
    );
    let slow = ctx.eval_var(
        "Ability__Phinn__A",
        rank,
        ValueSpec::var("SlowAmount", [0.55, 0.6, 0.65, 0.7, 0.0]),
    );
    for j in ctx.enemies_in_radius(center, 2.8, false) {
        ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
        if rank >= 5 {
            ctx.apply_stun(j, cc_dur);
        } else {
            ctx.apply_slow(j, slow, cc_dur);
        }
    }
    Ok(true)
}

fn phinn_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    _target: CastTarget,
) -> Result<bool, &'static str> {
    let origin = ctx.caster_pos();
    let rad = ctx.eval_var("Ability__Phinn__B", rank, ValueSpec::var("Range", [8.0; 5]));
    let dmg = ctx.eval_var(
        "Ability__Phinn__B",
        rank,
        ValueSpec::dmg(0, [75.0, 125.0, 175.0, 225.0, 275.0], 1.2, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    for j in ctx.enemies_in_radius(origin, rad, false) {
        ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
        ctx.pull_toward(j, origin, 3.0);
    }
    let e = &ctx.room.entities[ctx.caster];
    let fort = (ctx.eval_var(
        "Ability__Phinn__B",
        rank,
        ValueSpec::var("Fortified Health Base", [100.0, 125.0, 150.0, 175.0, 225.0]),
    ) + e.max_hp * 0.05
        + (e.stats.armor + e.stats.shield) * 0.18)
        * ctx.talent_barrier_mult(slot);
    let dur = ctx.eval_var(
        "Ability__Phinn__B",
        rank,
        ValueSpec::var("Duration", [2.0, 2.0, 2.0, 2.0, 3.0]),
    );
    for a in ctx.allies_in_radius(origin, rad, true, true) {
        ctx.grant_fortified(a, fort, dur);
        if ctx.talent_special() == TalentSpecial::PhinnBGrantsSpeed {
            let ms = ctx.talent_mod("BONUS_MOVESPEED_STRENGTH").unwrap_or(1.3);
            ctx.add_buff(a, BuffStatKind::MoveSpeedFlat, ms, dur);
        }
    }
    if ctx.talent_special() == TalentSpecial::PhinnBResetsAAndHalfCost {
        ctx.reset_slot_cd(AbilitySlot::A);
    }
    Ok(true)
}

fn phinn_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let origin = ctx.caster_pos();
    let goal = ctx.resolve_target_pos(target, 14.0);
    let dmg = ctx.eval_var(
        "Ability__Phinn__C",
        rank,
        ValueSpec::dmg3(0, [100.0, 300.0, 500.0], 1.5, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let anchor = clamp_pos(origin.advance(goal, 14.0));
    let mut pulled_any = false;
    for j in ctx.enemies_in_radius(lerp_pos(origin, anchor, 0.5), 7.5, false) {
        pulled_any = true;
        ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
        let dist = ctx.room.entities[j].position.dist(origin);
        ctx.pull_toward(j, origin, (dist - 1.5).max(0.0));
    }
    if !pulled_any && ctx.room.entities[ctx.caster].ability_ranks[2] >= 3 {
        ctx.reduce_slot_cd(AbilitySlot::C, 10.0);
    }
    Ok(true)
}

pub(super) static PHINN: HeroKitSpec = HeroKitSpec {
    hero_id: "Phinn",
    type_id: 279,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Perk__Phinn",
        unstoppable_cc_downgrade: true,
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Phinn__A",
        targeting: CastTargeting::GroundPoint,
        max_rank: 5,
        startup: ValueSpec::var("StartupDelay", [0.8; 5]),
        range: ValueSpec::constant(6.5),
        cooldown: ValueSpec::var("Cooldown", [10.0, 9.0, 8.0, 7.0, 6.0]),
        cost: ValueSpec::var("Energy Cost", [40.0, 45.0, 50.0, 55.0, 60.0]),
        custom_exec: Some(phinn_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Phinn__B",
        targeting: CastTargeting::InstantSelf,
        max_rank: 5,
        startup: ValueSpec::var("StartupDelay", [0.8; 5]),
        range: ValueSpec::var("Range", [8.0; 5]),
        cooldown: ValueSpec::var("Cooldown", [10.0, 9.5, 9.0, 8.5, 8.0]),
        cost: ValueSpec::var("Energy Cost", [40.0, 45.0, 50.0, 55.0, 60.0]),
        custom_exec: Some(phinn_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Phinn__C",
        targeting: CastTargeting::Directional,
        max_rank: 3,
        startup: ValueSpec::var3("Delay", [0.9; 3]),
        range: ValueSpec::constant(14.0),
        cooldown: ValueSpec::var3("Cooldown", [90.0, 75.0, 60.0]),
        cost: ValueSpec::var3("Energy Cost", [60.0; 3]),
        custom_exec: Some(phinn_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 52. Viola (tid=924)
// ============================================================================

fn viola_push_note(ctx: &mut HeroKitContext<'_, '_>, note: u8) {
    let k = ctx.room.entities[ctx.caster]
        .hero_kit
        .get_or_insert_with(Box::default);
    if k.musical_note_count < 3 {
        let idx = usize::from(k.musical_note_count);
        k.musical_notes[idx] = note;
        k.musical_note_count += 1;
    } else {
        k.musical_notes[0] = k.musical_notes[1];
        k.musical_notes[1] = k.musical_notes[2];
        k.musical_notes[2] = note;
    }
    let stacks = ctx.add_mark(ctx.caster, marks::MELODIOUS_TUNE, 1, 7, 5.0);
    if stacks > 0 {
        ctx.add_buff(
            ctx.caster,
            BuffStatKind::CooldownReduction,
            0.03 * f32::from(stacks),
            4.0,
        );
    }
}

fn viola_perk_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    _target_idx: usize,
    _raw_dmg: f32,
    _is_crit: bool,
) -> (f32, DamageSchool) {
    if ctx.consume_mark(ctx.caster, marks::MELODIOUS_TUNE) > 0 {
        let cp = ctx.eval_inputs().cp;
        (12.0 + cp * 0.25, DamageSchool::Crystal)
    } else {
        (0.0, DamageSchool::Weapon)
    }
}

fn viola_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    viola_push_note(ctx, 1);
    let ally_idx = ctx
        .resolve_target_idx(target)
        .filter(|&j| ctx.room.entities[j].team == ctx.caster_team())
        .unwrap_or(ctx.caster);
    let base = ctx.eval_var(
        "Ability__Viola__A",
        rank,
        ValueSpec::var("BARRIER_AMOUNT", [50.0, 85.0, 120.0, 155.0, 225.0]),
    );
    let ratio = ctx.eval_var(
        "Ability__Viola__A",
        rank,
        ValueSpec::var("BARRIER_HEALTH_RATIO", [0.07; 5]),
    );
    let dur = ctx.eval_var(
        "Ability__Viola__A",
        rank,
        ValueSpec::var("BARRIER_DURATION", [10.0; 5]),
    );
    let mult = ctx.talent_barrier_mult(slot);
    let bar =
        (base + ctx.room.entities[ctx.caster].max_hp * ratio + ctx.eval_inputs().cp * 0.4) * mult;
    let max_stacks = if ctx.talent_special() == TalentSpecial::ViolaDoubleHarmonyStacks {
        4
    } else {
        2
    };
    ctx.grant_barrier(
        ally_idx,
        bar,
        dur,
        Some((bar * max_stacks as f32, max_stacks)),
    );
    Ok(true)
}

fn viola_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    viola_push_note(ctx, 2);
    let dmg = ctx.eval_var(
        "Viola_B_Damage",
        rank,
        ValueSpec::dmg(0, [60.0, 100.0, 140.0, 180.0, 260.0], 1.15, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let slow = ctx.eval_var(
        "Ability__Viola__B",
        rank,
        ValueSpec::var("SLOW_STRENGTH", [0.25, 0.3, 0.35, 0.4, 0.45]),
    );
    let slow_dur = ctx.eval_var(
        "Ability__Viola__B",
        rank,
        ValueSpec::var("SLOW_DURATION", [1.0, 1.0, 1.0, 1.0, 1.5]),
    );
    let origin = ctx.caster_pos();
    let aim = ctx.resolve_target_pos(target, 11.0);
    let (dx, dy) = unit_dir(origin, aim);
    let end = clamp_pos(Position {
        x: origin.x + dx * 11.0,
        y: origin.y + dy * 11.0,
    });
    ctx.spawn_effect(
        KitEffectKind::Projectile {
            ability_slot: slot.idx() as u8,
            rank,
            speed_per_tick: 22.0 / TICK_HZ as f32,
            radius: 1.2,
            damage: dmg,
            school: DamageSchool::Crystal,
            piercing: false,
            returning: false,
            return_to_caster: false,
            repeat_mult: 1.0,
            splash_damage: 0.0,
            splash_radius: 0.0,
            stun_ticks: 0,
            root_ticks: 0,
            silence_ticks: 0,
            slow_pct: slow,
            slow_ticks: seconds_to_ticks(slow_dur),
            minion_mult: 0.7,
            resets_cd_on_hit: false,
            applies_mark: None,
            destroys_projectiles: false,
            hit_actors: Vec::new(),
        },
        origin,
        end,
    );
    Ok(true)
}

fn viola_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    rank: u8,
    _target: CastTarget,
) -> Result<bool, &'static str> {
    let b_notes = ctx.room.entities[ctx.caster]
        .hero_kit
        .as_ref()
        .map(|k| k.musical_notes.iter().filter(|&&n| n == 2).count())
        .unwrap_or(0);
    if let Some(k) = ctx.room.entities[ctx.caster].hero_kit.as_deref_mut() {
        k.musical_notes = [0; 3];
        k.musical_note_count = 0;
    }
    let center = ctx.caster_pos();
    let dr_bonus = if ctx.talent_special() == TalentSpecial::ViolaBalladAmpAndDr {
        ctx.talent_mod("DAMAGE_REDUCTION").unwrap_or(0.05)
    } else {
        0.0
    };
    match b_notes {
        0 => {
            // AAA: Tempesta — Cleanse + Move Speed
            for a in ctx.allies_in_radius(center, 6.5, true, true) {
                ctx.cleanse_cc(a);
                ctx.set_unstoppable(a, 0.8);
                ctx.add_buff(a, BuffStatKind::MoveSpeedFlat, 1.8, 3.0);
                if dr_bonus > 0.0 {
                    ctx.add_buff(a, BuffStatKind::DamageReduction, dr_bonus, 4.0);
                }
            }
        }
        1 => {
            // AAB: Ballata — Attack Speed + Cooldown Reduction
            for a in ctx.allies_in_radius(center, 6.5, true, true) {
                ctx.add_buff(a, BuffStatKind::AttackSpeed, 0.4, 5.0);
                ctx.add_buff(a, BuffStatKind::CooldownReduction, 0.15, 5.0);
                if dr_bonus > 0.0 {
                    ctx.add_buff(a, BuffStatKind::DamageReduction, dr_bonus, 4.0);
                }
            }
        }
        2 => {
            // ABB: Dissonanza — Burst + DoT + Mortal Wound
            let burst = ctx.eval_var(
                "Ability__Viola__C3",
                rank,
                ValueSpec::dmg3(0, [150.0, 250.0, 350.0], 0.7, 0.0, 0.0),
            ) * ctx.talent_dmg_mult(AbilitySlot::C);
            let dot_tick = ctx.eval_var(
                "Ability__Viola__C3",
                rank,
                ValueSpec::dmg3(1, [30.0, 60.0, 90.0], 0.15, 0.0, 0.0),
            );
            for j in ctx.enemies_in_radius(center, 6.5, false) {
                ctx.deal_damage(j, burst, DamageSchool::Crystal, 0.0, false);
                ctx.apply_mortal_wound(j, 3.0);
                spawn_single_hit_dot(
                    ctx,
                    AbilitySlot::C,
                    j,
                    dot_tick,
                    DamageSchool::Crystal,
                    3.0,
                    1.0,
                );
            }
        }
        _ => {
            // BBB: Lullaby — Heavy Slow + Unwaking Sleep
            let sleep_ticks = seconds_to_ticks(3.0);
            for j in ctx.enemies_in_radius(center, 6.5, false) {
                ctx.apply_slow(j, 0.65, 2.0);
                ctx.apply_sleep_ticks(j, sleep_ticks, false);
            }
        }
    }
    if ctx.talent_special() == TalentSpecial::ViolaChordChargesAndUltReset {
        ctx.reduce_slot_cd(AbilitySlot::C, 8.0);
    }
    Ok(true)
}

static VIOLA_VARIANTS: &[AbilityVariantSpec] = &[
    AbilityVariantSpec {
        slot: HeroSlot::C2,
        spec: AbilityKitSpec {
            ability_id: "Ability__Viola__C2",
            targeting: CastTargeting::InstantSelf,
            max_rank: 3,
            startup: ValueSpec::zero(),
            range: ValueSpec::constant(6.5),
            cooldown: ValueSpec::var3("Cooldown", [18.0, 15.0, 12.0]),
            cost: ValueSpec::var3("ENERGY COST", [40.0, 50.0, 60.0]),
            gate: CastGate::RequiresMusicalChord3,
            custom_exec: Some(viola_c_cast),
            ..AbilityKitSpec::DEFAULT
        },
    },
    AbilityVariantSpec {
        slot: HeroSlot::C3,
        spec: AbilityKitSpec {
            ability_id: "Ability__Viola__C3",
            targeting: CastTargeting::InstantSelf,
            max_rank: 3,
            startup: ValueSpec::zero(),
            range: ValueSpec::constant(6.5),
            cooldown: ValueSpec::var3("Cooldown", [22.0, 18.0, 14.0]),
            cost: ValueSpec::var3("ENERGY COST", [40.0, 50.0, 60.0]),
            gate: CastGate::RequiresMusicalChord3,
            custom_exec: Some(viola_c_cast),
            ..AbilityKitSpec::DEFAULT
        },
    },
    AbilityVariantSpec {
        slot: HeroSlot::C4,
        spec: AbilityKitSpec {
            ability_id: "Ability__Viola__C4",
            targeting: CastTargeting::InstantSelf,
            max_rank: 3,
            startup: ValueSpec::zero(),
            range: ValueSpec::constant(6.5),
            cooldown: ValueSpec::var3("Cooldown", [22.0, 18.0, 14.0]),
            cost: ValueSpec::var3("ENERGY COST", [40.0, 50.0, 60.0]),
            gate: CastGate::RequiresMusicalChord3,
            custom_exec: Some(viola_c_cast),
            ..AbilityKitSpec::DEFAULT
        },
    },
];

pub(super) static VIOLA: HeroKitSpec = HeroKitSpec {
    hero_id: "Viola",
    type_id: 924,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Perk__Viola",
        on_basic_attack: Some(viola_perk_attack),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Viola__A",
        targeting: CastTargeting::TargetAllyOrSelf,
        max_rank: 5,
        startup: ValueSpec::zero(),
        range: ValueSpec::var("Range", [11.0; 5]),
        cooldown: ValueSpec::var("Cooldown", [4.0, 4.0, 4.0, 4.0, 3.0]),
        cost: ValueSpec::var("ENERGY COST", [20.0, 25.0, 30.0, 35.0, 40.0]),
        charges: ValueSpec::constant(2.0),
        between_charges_cd: ValueSpec::constant(0.3),
        custom_exec: Some(viola_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Viola__B",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::zero(),
        range: ValueSpec::var("Range", [11.0; 5]),
        cooldown: ValueSpec::var("Cooldown", [4.0, 4.0, 4.0, 4.0, 3.0]),
        cost: ValueSpec::var("ENERGY COST", [20.0, 25.0, 30.0, 35.0, 40.0]),
        charges: ValueSpec::constant(2.0),
        between_charges_cd: ValueSpec::constant(0.3),
        custom_exec: Some(viola_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Viola__C",
        targeting: CastTargeting::InstantSelf,
        max_rank: 3,
        startup: ValueSpec::zero(),
        range: ValueSpec::constant(6.5),
        cooldown: ValueSpec::var3("Cooldown", [22.0, 18.0, 14.0]),
        cost: ValueSpec::var3("ENERGY COST", [40.0, 50.0, 60.0]),
        gate: CastGate::RequiresMusicalChord3,
        custom_exec: Some(viola_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    variants: VIOLA_VARIANTS,
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 55. Yates (tid=439)
// ============================================================================

fn yates_perk_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    target_idx: usize,
    _raw_dmg: f32,
    _is_crit: bool,
) -> (f32, DamageSchool) {
    let next = ctx.room.entities[ctx.caster]
        .hero_kit
        .as_ref()
        .map(|k| (k.counters[0] + 1) % 3)
        .unwrap_or(1);
    let k = ctx.room.entities[ctx.caster]
        .hero_kit
        .get_or_insert_with(Box::default);
    k.counters[0] = next;
    if next != 0 {
        return (0.0, DamageSchool::Weapon);
    }
    let lvl = f32::from(ctx.room.entities[ctx.caster].level.max(1));
    let mult = if ctx.talent_special() == TalentSpecial::YatesFlailHeavyThirdHit {
        ctx.talent_mod("DAMAGE_MODIFIER").unwrap_or(1.2)
    } else {
        1.0
    };
    let cleave =
        (25.0 + 6.0 * lvl + ctx.eval_inputs().cp * 0.5 + ctx.eval_inputs().wp * 0.25) * mult;
    let tpos = ctx.room.entities[target_idx].position;
    for j in ctx.enemies_in_radius(tpos, 3.2, false) {
        if j != target_idx {
            ctx.deal_damage(j, cleave * 0.75, DamageSchool::Crystal, 0.0, false);
        }
    }
    ctx.add_buff(ctx.caster, BuffStatKind::MoveSpeedFlat, 1.0, 2.0);
    (cleave, DamageSchool::Crystal)
}

fn yates_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let range = ctx.eval_var("Ability__Yates__A", rank, ValueSpec::var("RANGE", [9.5; 5]))
        + ctx.talent_mod("BONUS_RANGE").unwrap_or(0.0);
    let dmg = ctx.eval_var(
        "Ability__Yates__A",
        rank,
        ValueSpec::dmg(0, [165.0, 255.0, 345.0, 435.0, 525.0], 1.25, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let effects = [PrimitiveEffect::LungeHook {
        range: ValueSpec::constant(range),
        damage: ValueSpec::constant(dmg),
        school: DamageKind::Crystal,
        pull_distance: ValueSpec::constant(range - 1.2),
        knockback_distance: ValueSpec::zero(),
        stun_on_wall_seconds: ValueSpec::constant(0.4),
        tether_duration: ValueSpec::zero(),
        tether_break_range: ValueSpec::zero(),
        tether_dps_pct_hp: ValueSpec::zero(),
        devour_seconds: ValueSpec::zero(),
        suppress_untargetable: false,
    }];
    ctx.exec_primitives("Ability__Yates__A", slot, rank, target, &effects);
    Ok(true)
}

const YATES_B_EFFECTS: &[PrimitiveEffect] = &[
    PrimitiveEffect::GroundAoE {
        radius: ValueSpec::var("RADIUS", [3.5; 5]),
        duration: ValueSpec::constant(0.1),
        ticks_per_sec: 1.0,
        dps: ValueSpec::zero(),
        hps: ValueSpec::zero(),
        school: DamageKind::Crystal,
        slow_pct: ValueSpec::zero(),
        end_damage: Some(ValueSpec::dmg(
            0,
            [80.0, 160.0, 240.0, 320.0, 400.0],
            0.75,
            0.0,
            0.0,
        )),
        end_root_seconds: ValueSpec::zero(),
        end_stun_seconds: ValueSpec::var("STUN_DURATION", [0.8, 0.8, 0.8, 0.8, 1.0]),
        drifts_toward_aim: false,
        minion_mult: 0.7,
        applies_mark: None,
        zone_kind: 55,
    },
    PrimitiveEffect::SelfBuff {
        stat: BuffStatKind::MoveSpeedFlat,
        amount: ValueSpec::var("MSPD_STRENGTH", [1.2, 1.2, 1.2, 1.2, 1.8]),
        duration: ValueSpec::var("EFFECT_DURATION", [1.5; 5]),
        empowered_attacks: 0,
        empowered_bonus_damage: ValueSpec::zero(),
        empowered_stun_seconds: 0.0,
        empowered_slow_pct: 0.0,
        empowered_slow_seconds: 0.0,
        empowered_knockback: 0.0,
        resets_attack_cd: false,
    },
];

fn yates_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let dest = ctx.resolve_target_pos(target, 180.0);
    let dmg = ctx.eval_var(
        "Ability__Yates__C",
        rank,
        ValueSpec::dmg3(0, [350.0, 525.0, 700.0], 1.5, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let rad = ctx.eval_var(
        "Ability__Yates__C",
        rank,
        ValueSpec::var3("IMPACT_RADIUS", [6.0; 3]),
    );
    let slow = ctx.eval_var(
        "Ability__Yates__C",
        rank,
        ValueSpec::var3("SLOW_STRENGTH", [0.3, 0.4, 0.5]),
    );
    let bbase = ctx.eval_var(
        "Ability__Yates__C",
        rank,
        ValueSpec::var_scaled(
            "BARRIER_STRENGTH",
            [250.0, 375.0, 500.0, 500.0, 500.0],
            1.25,
            0.0,
            0.0,
            0.0,
        ),
    );
    let b_ratio = ctx.eval_var(
        "Ability__Yates__C",
        rank,
        ValueSpec::var3("BARRIER_HEALTHRATIO", [0.08; 3]),
    );
    let bar =
        (bbase + ctx.room.entities[ctx.caster].max_hp * b_ratio) * ctx.talent_barrier_mult(slot);
    ctx.grant_barrier(ctx.caster, bar, 3.0, None);
    for a in ctx.allies_in_radius(dest, rad, true, false) {
        ctx.grant_barrier(a, bar, 3.0, None);
    }
    move_toward_or_dash(ctx, dest);
    for j in ctx.enemies_in_radius(dest, rad, false) {
        ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
        ctx.apply_slow(j, slow, 3.0);
    }
    Ok(true)
}

pub(super) static YATES: HeroKitSpec = HeroKitSpec {
    hero_id: "Yates",
    type_id: 439,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Perk__Yates",
        on_basic_attack: Some(yates_perk_attack),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Yates__A",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::constant(0.15),
        range: ValueSpec::var("RANGE", [9.5; 5]),
        cooldown: ValueSpec::var("COOLDOWN", [18.0, 17.0, 16.0, 15.0, 12.0]),
        cost: ValueSpec::var("ENERGY COST", [80.0; 5]),
        custom_exec: Some(yates_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Yates__B",
        targeting: CastTargeting::GroundPoint,
        max_rank: 5,
        startup: ValueSpec::constant(0.15),
        range: ValueSpec::var("RADIUS", [3.5; 5]),
        cooldown: ValueSpec::var("COOLDOWN", [16.0, 15.0, 14.0, 13.0, 11.0]),
        cost: ValueSpec::var("ENERGY COST", [60.0, 65.0, 70.0, 75.0, 80.0]),
        effects: YATES_B_EFFECTS,
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Yates__C",
        targeting: CastTargeting::GroundPoint,
        max_rank: 3,
        startup: ValueSpec::var3("CHANNEL_TIME", [1.0; 3]),
        range: ValueSpec::var3("RANGE", [180.0; 3]),
        cooldown: ValueSpec::var3("COOLDOWN", [100.0, 90.0, 80.0]),
        cost: ValueSpec::var3("ENERGY COST", [100.0; 3]),
        custom_exec: Some(yates_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 57. Hero057 (tid=435)
// ============================================================================

fn hero057_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let range = ctx.eval_var(
        "Ability__Hero057__A",
        rank,
        ValueSpec::var("RANGE", [9.5; 5]),
    );
    let dmg = ctx.eval_var(
        "Ability__Hero057__A",
        rank,
        ValueSpec::var_scaled(
            "DAMAGE",
            [30.0, 50.0, 70.0, 90.0, 130.0],
            0.4,
            0.1,
            0.0,
            0.0,
        ),
    );
    let heal = ctx.eval_var(
        "Ability__Hero057__A",
        rank,
        ValueSpec::var_scaled("Heal", [25.0, 50.0, 75.0, 100.0, 150.0], 0.4, 0.0, 0.0, 0.0),
    );
    let speed = ctx.eval_var(
        "Ability__Hero057__A",
        rank,
        ValueSpec::var("SHOT_SPEED", [24.0; 5]),
    );
    if let Some(j) = ctx.resolve_target_idx(target)
        && ctx.room.entities[j].team == ctx.caster_team()
    {
        ctx.heal_unit(j, heal);
        return Ok(true);
    }
    let origin = ctx.caster_pos();
    let aim = ctx.resolve_target_pos(target, range);
    let (dx, dy) = unit_dir(origin, aim);
    let end = clamp_pos(Position {
        x: origin.x + dx * range,
        y: origin.y + dy * range,
    });
    ctx.spawn_effect(
        KitEffectKind::Projectile {
            ability_slot: slot.idx() as u8,
            rank,
            speed_per_tick: speed / TICK_HZ as f32,
            radius: 1.1,
            damage: dmg,
            school: DamageSchool::Crystal,
            piercing: false,
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
            minion_mult: 0.8,
            resets_cd_on_hit: false,
            applies_mark: None,
            destroys_projectiles: false,
            hit_actors: Vec::new(),
        },
        origin,
        end,
    );
    Ok(true)
}

fn hero057_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let attached = ctx.room.entities[ctx.caster]
        .hero_kit
        .as_ref()
        .is_some_and(|k| k.step_stage[1] == 1 && ctx.tick < k.recast_until[1]);
    if attached {
        ctx.clear_recast(AbilitySlot::B);
        let owner_id = ctx.caster_id();
        let handles: Vec<bevy_ecs::entity::Entity> = ctx
            .room
            .entities
            .ecs
            .query::<(bevy_ecs::entity::Entity, &KitEffect)>()
            .iter(&ctx.room.entities.ecs)
            .filter_map(|(eh, eff)| {
                (eff.owner_id == owner_id && matches!(eff.kind, KitEffectKind::Attachment { .. }))
                    .then_some(eh)
            })
            .collect();
        for eh in handles {
            ctx.room.entities.remove_spell_handle::<KitEffect>(eh);
        }
        let range = ctx.eval_var(
            "Ability__Hero057__B_LeapOff",
            rank,
            ValueSpec::var("RANGE", [6.0; 5]),
        );
        let dest = ctx.resolve_target_pos(target, range);
        move_toward_or_dash(ctx, dest);
        let rad = ctx.eval_var(
            "Ability__Hero057__B_LeapOff",
            rank,
            ValueSpec::var("Radius", [4.0; 5]),
        );
        let dmg = ctx.eval_var(
            "Ability__Hero057__B_LeapOff",
            rank,
            ValueSpec::var_scaled(
                "Damage",
                [100.0, 100.6, 101.2, 101.8, 102.4],
                1.0,
                0.0,
                0.0,
                0.0,
            ),
        );
        let stun = ctx.eval_var(
            "Ability__Hero057__B_LeapOff",
            rank,
            ValueSpec::var("STUN_DURATION", [0.5, 0.5, 0.5, 0.5, 0.7]),
        );
        for j in ctx.enemies_in_radius(dest, rad, false) {
            ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
            ctx.apply_stun(j, stun);
        }
        return Ok(true);
    }

    let ally_idx = ctx
        .resolve_target_idx(target)
        .filter(|&j| j != ctx.caster && ctx.room.entities[j].team == ctx.caster_team());
    let Some(hj) = ally_idx else {
        return Err("requires allied hero");
    };
    let host_id = ctx.room.entities[hj].id;
    let host_pos = ctx.room.entities[hj].position;
    ctx.spawn_effect(
        KitEffectKind::Attachment {
            ability_slot: slot.idx() as u8,
            host_id,
            is_devour: false,
            dps: 0.0,
            end_damage: 0.0,
            end_radius: 4.0,
            school: DamageSchool::Crystal,
            next_tick: ctx.tick + TICK_HZ,
            end_tick: ctx.tick + seconds_to_ticks(8.0),
        },
        host_pos,
        host_pos,
    );
    ctx.open_recast(AbilitySlot::B, 1, host_id, 8.0);
    Ok(true)
}

fn hero057_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    _target: CastTarget,
) -> Result<bool, &'static str> {
    let dur = ctx.eval_var(
        "Ability__Hero057__C",
        rank,
        ValueSpec::var3("DURATION", [5.0; 3]),
    );
    let rad = ctx.eval_var(
        "Ability__Hero057__C",
        rank,
        ValueSpec::var3("Radius", [4.0; 3]),
    );
    let dmg = ctx.eval_var(
        "Ability__Hero057__C",
        rank,
        ValueSpec::var_scaled(
            "DAMAGE",
            [50.0, 100.0, 150.0, 150.0, 150.0],
            0.3,
            0.0,
            0.0,
            0.0,
        ),
    );
    let aspd = ctx.eval_var(
        "Ability__Hero057__C",
        rank,
        ValueSpec::var3("ATTACKSPEED", [0.3, 0.4, 0.5]),
    );
    let ms = ctx.eval_var(
        "Ability__Hero057__C",
        rank,
        ValueSpec::var3("MOVESPEED_BONUS", [2.0; 3]),
    );
    let center = ctx.caster_pos();
    for a in ctx.allies_in_radius(center, rad, true, true) {
        ctx.add_buff(a, BuffStatKind::AttackSpeed, aspd, dur);
        ctx.add_buff(a, BuffStatKind::MoveSpeedFlat, ms, dur);
    }
    ctx.spawn_effect(
        KitEffectKind::GroundZone {
            ability_slot: slot.idx() as u8,
            zone_kind: 57,
            radius: rad,
            ticks_interval: TICK_HZ / 2,
            next_tick: ctx.tick + TICK_HZ / 2,
            end_tick: ctx.tick + seconds_to_ticks(dur),
            tick_damage: dmg,
            tick_heal: 0.0,
            school: DamageSchool::Crystal,
            slow_pct: 0.0,
            ally_speed_bonus: ms,
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
        center,
        center,
    );
    Ok(true)
}

static HERO057_VARIANTS: &[AbilityVariantSpec] = &[AbilityVariantSpec {
    slot: HeroSlot::B2,
    spec: AbilityKitSpec {
        ability_id: "Ability__Hero057__B_LeapOff",
        targeting: CastTargeting::GroundPoint,
        max_rank: 5,
        startup: ValueSpec::zero(),
        range: ValueSpec::var("RANGE", [6.0; 5]),
        cooldown: ValueSpec::constant(1.0),
        cost: ValueSpec::zero(),
        custom_exec: Some(hero057_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
}];

pub(super) static HERO057: HeroKitSpec = HeroKitSpec {
    hero_id: "Hero057",
    type_id: 435,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Perk__Hero057",
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Hero057__A",
        targeting: CastTargeting::TargetLocationOrEntity,
        max_rank: 5,
        startup: ValueSpec::zero(),
        range: ValueSpec::var("RANGE", [9.5; 5]),
        cooldown: ValueSpec::var("COOLDOWN", [12.0, 11.0, 10.0, 9.0, 8.0]),
        cost: ValueSpec::var("ENERGY COST", [30.0; 5]),
        custom_exec: Some(hero057_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Hero057__B",
        targeting: CastTargeting::TargetAlly,
        max_rank: 5,
        startup: ValueSpec::zero(),
        range: ValueSpec::var("RANGE", [6.0; 5]),
        cooldown: ValueSpec::var("COOLDOWN", [20.0, 18.0, 16.0, 14.0, 12.0]),
        cost: ValueSpec::var("ENERGY COST", [30.0; 5]),
        custom_exec: Some(hero057_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Hero057__C",
        targeting: CastTargeting::InstantSelf,
        max_rank: 3,
        startup: ValueSpec::var3("START_UP_DELAY", [0.3; 3]),
        range: ValueSpec::var3("Radius", [4.0; 3]),
        cooldown: ValueSpec::var3("COOLDOWN", [60.0, 59.85, 59.7]),
        cost: ValueSpec::var3("ENERGY COST", [1.0; 3]),
        custom_exec: Some(hero057_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    variants: HERO057_VARIANTS,
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 58. Hero062 (tid=912) & 59. Hero999 (tid=914)
// ============================================================================

const PROTO_A_EFFECTS: &[PrimitiveEffect] = &[PrimitiveEffect::SkillshotProjectile {
    damage: ValueSpec::ranks([80.0, 110.0, 140.0, 170.0, 200.0]),
    school: DamageKind::Crystal,
    range: ValueSpec::constant(8.0),
    speed: ValueSpec::constant(20.0),
    radius: ValueSpec::constant(1.1),
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
    minion_mult: 0.8,
    resets_cast_cd_on_hit: false,
    applies_mark: None,
    destroys_enemy_projectiles: false,
}];

const PROTO_B_EFFECTS: &[PrimitiveEffect] = &[
    PrimitiveEffect::DashBlink {
        range: ValueSpec::constant(5.5),
        duration: ValueSpec::constant(0.15),
        instant_blink: true,
        invuln_during: false,
        unstoppable_during: false,
        path_radius: 1.0,
        path_damage: ValueSpec::zero(),
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
    PrimitiveEffect::SelfBuff {
        stat: BuffStatKind::MoveSpeedFlat,
        amount: ValueSpec::constant(1.2),
        duration: ValueSpec::constant(2.0),
        empowered_attacks: 0,
        empowered_bonus_damage: ValueSpec::zero(),
        empowered_stun_seconds: 0.0,
        empowered_slow_pct: 0.0,
        empowered_slow_seconds: 0.0,
        empowered_knockback: 0.0,
        resets_attack_cd: false,
    },
];

const PROTO_C_EFFECTS: &[PrimitiveEffect] = &[PrimitiveEffect::GroundAoE {
    radius: ValueSpec::constant(3.5),
    duration: ValueSpec::constant(0.1),
    ticks_per_sec: 1.0,
    dps: ValueSpec::zero(),
    hps: ValueSpec::zero(),
    school: DamageKind::Crystal,
    slow_pct: ValueSpec::zero(),
    end_damage: Some(ValueSpec::ranks3([180.0, 260.0, 340.0])),
    end_root_seconds: ValueSpec::zero(),
    end_stun_seconds: ValueSpec::constant(0.5),
    drifts_toward_aim: false,
    minion_mult: 0.8,
    applies_mark: None,
    zone_kind: 58,
}];

pub(super) static HERO062: HeroKitSpec = HeroKitSpec {
    hero_id: "Hero062",
    type_id: 912,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Perk__Hero062",
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Hero062__A",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::zero(),
        range: ValueSpec::constant(8.0),
        cooldown: ValueSpec::var("Cooldown", [1.0; 5]),
        cost: ValueSpec::zero(),
        effects: PROTO_A_EFFECTS,
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Hero062__B",
        targeting: CastTargeting::GroundPoint,
        max_rank: 5,
        startup: ValueSpec::zero(),
        range: ValueSpec::constant(5.5),
        cooldown: ValueSpec::var("Cooldown", [1.0; 5]),
        cost: ValueSpec::zero(),
        effects: PROTO_B_EFFECTS,
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Hero062__C",
        targeting: CastTargeting::GroundPoint,
        max_rank: 3,
        startup: ValueSpec::zero(),
        range: ValueSpec::constant(7.0),
        cooldown: ValueSpec::var3("Cooldown", [1.0; 3]),
        cost: ValueSpec::zero(),
        effects: PROTO_C_EFFECTS,
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

pub(super) static HERO999: HeroKitSpec = HeroKitSpec {
    hero_id: "Hero999",
    type_id: 914,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Perk__Hero999",
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Hero999__A",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::zero(),
        range: ValueSpec::constant(8.0),
        cooldown: ValueSpec::var("Cooldown", [1.0; 5]),
        cost: ValueSpec::zero(),
        effects: PROTO_A_EFFECTS,
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Hero999__B",
        targeting: CastTargeting::GroundPoint,
        max_rank: 5,
        startup: ValueSpec::zero(),
        range: ValueSpec::constant(5.5),
        cooldown: ValueSpec::var("Cooldown", [1.0; 5]),
        cost: ValueSpec::zero(),
        effects: PROTO_B_EFFECTS,
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Hero999__C",
        targeting: CastTargeting::GroundPoint,
        max_rank: 3,
        startup: ValueSpec::zero(),
        range: ValueSpec::constant(7.0),
        cooldown: ValueSpec::var3("Cooldown", [1.0; 3]),
        cost: ValueSpec::zero(),
        effects: PROTO_C_EFFECTS,
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 60. LanceBall_Lance (tid=443)
// ============================================================================

fn lanceball_a_cast(
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
            "Ability__LanceBall_Lance__ThrowBall",
            rank,
            ValueSpec::var("RANGE", [8.0; 5]),
        );
        let speed = ctx.eval_var(
            "Ability__LanceBall_Lance__ThrowBall",
            rank,
            ValueSpec::var("SHOT_SPEED", [24.0; 5]),
        );
        let origin = ctx.caster_pos();
        let aim = ctx.resolve_target_pos(target, range);
        let (dx, dy) = unit_dir(origin, aim);
        let end = clamp_pos(Position {
            x: origin.x + dx * range,
            y: origin.y + dy * range,
        });
        ctx.spawn_effect(
            KitEffectKind::Projectile {
                ability_slot: 0,
                rank,
                speed_per_tick: speed / TICK_HZ as f32,
                radius: 1.0,
                damage: 120.0,
                school: DamageSchool::Weapon,
                piercing: false,
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
                minion_mult: 1.0,
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

    let range = ctx.eval_var(
        "Ability__LanceBall_Lance__A",
        rank,
        ValueSpec::var("RANGE", [4.5; 5]),
    );
    let dest = ctx.resolve_target_pos(target, range);
    move_toward_or_dash(ctx, dest);
    for j in ctx.enemies_in_radius(dest, 2.0, false) {
        ctx.deal_damage(j, 250.0, DamageSchool::Weapon, 0.0, false);
        ctx.apply_stun(j, 0.5);
    }
    Ok(true)
}

fn lanceball_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let range = ctx.eval_var(
        "Ability__LanceBall_Lance__B",
        rank,
        ValueSpec::var("RANGE", [4.5; 5]),
    );
    let kb = ctx.eval_var(
        "Ability__LanceBall_Lance__B",
        rank,
        ValueSpec::var("KNOCKBACK_DISTANCE", [10.0; 5]),
    );
    let pos = ctx.resolve_target_pos(target, range);
    for j in ctx.enemies_in_radius(pos, 3.0, false) {
        ctx.knockback(j, ctx.caster_pos(), kb, 0.5);
    }
    Ok(true)
}

fn lanceball_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let range = ctx.eval_var(
        "Ability__LanceBall_Lance__C",
        rank,
        ValueSpec::var3("RANGE", [5.0; 3]),
    );
    let dest = ctx.resolve_target_pos(target, range);
    move_toward_or_dash(ctx, dest);
    Ok(true)
}

static LANCEBALL_VARIANTS: &[AbilityVariantSpec] = &[AbilityVariantSpec {
    slot: HeroSlot::A2,
    spec: AbilityKitSpec {
        ability_id: "Ability__LanceBall_Lance__ThrowBall",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::zero(),
        range: ValueSpec::var("RANGE", [8.0; 5]),
        cooldown: ValueSpec::constant(0.5),
        cost: ValueSpec::zero(),
        custom_exec: Some(lanceball_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
}];

pub(super) static LANCEBALL_LANCE: HeroKitSpec = HeroKitSpec {
    hero_id: "LanceBall_Lance",
    type_id: 443,
    resource: ResourceKind::None,
    perk: HeroicPerkSpec {
        id: "Perk__LanceBall_Lance",
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__LanceBall_Lance__A",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::zero(),
        range: ValueSpec::var("RANGE", [4.5; 5]),
        cooldown: ValueSpec::var("COOLDOWN", [6.0; 5]),
        cost: ValueSpec::zero(),
        custom_exec: Some(lanceball_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__LanceBall_Lance__B",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::zero(),
        range: ValueSpec::var("RANGE", [4.5; 5]),
        cooldown: ValueSpec::var("COOLDOWN", [10.0; 5]),
        cost: ValueSpec::zero(),
        custom_exec: Some(lanceball_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__LanceBall_Lance__C",
        targeting: CastTargeting::Directional,
        max_rank: 3,
        startup: ValueSpec::zero(),
        range: ValueSpec::var3("RANGE", [5.0; 3]),
        cooldown: ValueSpec::var3("COOLDOWN", [4.0; 3]),
        cost: ValueSpec::zero(),
        custom_exec: Some(lanceball_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    variants: LANCEBALL_VARIANTS,
    ..HeroKitSpec::DEFAULT
};
