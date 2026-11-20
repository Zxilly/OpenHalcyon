//! Complete A / B / C / Sub-ability / Heroic Perk specifications for `abilities.json`
//! indices `30..61` (`Lorelai` ..= `LanceBall_Lance`).

use crate::combat::{AbilitySlot, CastTarget};
use crate::hero_kit::{
    AbilityKitSpec, AbilityVariantSpec, BuffStatKind, CastGate, CastTargeting, DamageKind,
    HeroKitContext, HeroKitSpec, HeroSlot, HeroicPerkSpec, KitEffect, KitEffectKind, MarkKind,
    PrimitiveEffect, ResourceKind, ValueSpec, marks, seconds_to_ticks,
};
use crate::score::DamageSchool;
use crate::talents_runtime::TalentSpecial;
use crate::{Kind, Position, TICK_HZ};

// ============================================================================
// Shared deterministic helpers for Batch B kits
// ============================================================================

fn clamp_pos(p: Position) -> Position {
    Position {
        x: p.x.clamp(-120.0, 120.0),
        y: p.y.clamp(-80.0, 80.0),
    }
}

fn unit_dir(from: Position, to: Position) -> (f32, f32) {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let d = dx.hypot(dy);
    if d <= 0.0001 {
        (1.0, 0.0)
    } else {
        (dx / d, dy / d)
    }
}

fn lerp_pos(a: Position, b: Position, t: f32) -> Position {
    Position {
        x: a.x + (b.x - a.x) * t,
        y: a.y + (b.y - a.y) * t,
    }
}

fn move_toward_or_dash(ctx: &mut HeroKitContext<'_, '_>, goal: Position) {
    let origin = ctx.caster_pos();
    if ctx.room.spell_motion_clear(origin, goal) {
        ctx.room.place_spell_actor(ctx.caster, goal, ctx.events);
    }
}

fn spawn_single_hit_dot(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    target_idx: usize,
    tick_dmg: f32,
    school: DamageSchool,
    dur_sec: f32,
    minion_mult: f32,
) {
    if target_idx >= ctx.room.entities.len() || tick_dmg <= 0.0 || dur_sec <= 0.0 {
        return;
    }
    let pos = ctx.room.entities[target_idx].position;
    let target_id = ctx.room.entities[target_idx].id;
    ctx.spawn_effect(
        KitEffectKind::DoT {
            ability_slot: slot.idx() as u8,
            target_id,
            tick_damage: tick_dmg,
            school,
            ticks_interval: TICK_HZ,
            next_tick: ctx.tick + TICK_HZ,
            end_tick: ctx.tick + seconds_to_ticks(dur_sec),
            minion_mult,
        },
        pos,
        pos,
    );
}

fn has_nearby_active_zone(
    ctx: &HeroKitContext<'_, '_>,
    pos: Position,
    zone_kind: u8,
    pad: f32,
) -> bool {
    let owner_id = ctx.caster_id();
    ctx.room.entities.spells::<KitEffect>().any(|eff| {
        eff.owner_id == owner_id
            && matches!(
                eff.kind,
                KitEffectKind::GroundZone {
                    zone_kind: zk,
                    radius,
                    ..
                } if zk == zone_kind && eff.position.dist(pos) <= radius + pad
            )
    })
}

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

pub(crate) static LORELAI: HeroKitSpec = HeroKitSpec {
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

pub(crate) static LYRA: HeroKitSpec = HeroKitSpec {
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
// 32. Magnus (tid=418)
// ============================================================================

fn magnus_mark_dur(ctx: &HeroKitContext<'_, '_>) -> f32 {
    let mut dur = 4.0;
    if ctx.talent_special() == TalentSpecial::None
        && let Some(extra) = ctx.talent_mod("MARK_BONUS_DURATION")
    {
        dur += extra;
    }
    dur
}

fn magnus_perk_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    target_idx: usize,
    _base_damage: f32,
    _was_crit: bool,
) -> (f32, DamageSchool) {
    let sp = ctx.talent_special();
    let stacks = if sp == TalentSpecial::MagnusMarkNotConsumed {
        ctx.mark_stacks(target_idx, marks::ARCANE_MARK)
    } else {
        ctx.consume_mark(target_idx, marks::ARCANE_MARK)
    };
    if stacks > 0 {
        let lvl = f32::from(ctx.room.entities[ctx.caster].level);
        let cp = ctx.eval_inputs().cp;
        if sp == TalentSpecial::MagnusHitReducesAllCdr {
            let cdr = ctx.talent_mod("COOLDOWN_ON_HIT").unwrap_or(0.15);
            for s in [AbilitySlot::A, AbilitySlot::B, AbilitySlot::C] {
                ctx.reduce_slot_cd(s, cdr * 5.0);
            }
        }
        (40.0 + lvl * 5.0 + cp * 0.65, DamageSchool::Crystal)
    } else {
        (0.0, DamageSchool::Crystal)
    }
}

fn magnus_perk_ability_hit(ctx: &mut HeroKitContext<'_, '_>, target_idx: usize, _dealt: f32) {
    let dur = magnus_mark_dur(ctx);
    ctx.add_mark(target_idx, marks::ARCANE_MARK, 1, 1, dur);
    if ctx.talent_special() == TalentSpecial::MagnusMarkReveals {
        ctx.apply_reveal(target_idx, dur);
    }
}

fn magnus_perk_kill(
    ctx: &mut HeroKitContext<'_, '_>,
    victim_idx: usize,
    _is_killer: bool,
    is_hero: bool,
) {
    if is_hero {
        let pos = ctx.room.entities[victim_idx].position;
        let cp = ctx.eval_inputs().cp;
        let chain_dmg = 120.0 + cp * 0.6;
        for j in ctx.enemies_in_radius(pos, 5.0, false) {
            if j != victim_idx {
                ctx.deal_damage(j, chain_dmg, DamageSchool::Crystal, 0.0, false);
            }
        }
    }
}

fn magnus_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let range = ctx.eval_var(
        "Ability__Magnus__B",
        rank,
        ValueSpec::var("RANGE", [10.0; 5]),
    );
    let origin = ctx.caster_pos();
    let goal = ctx.resolve_target_pos(target, range);
    let (dx, dy) = unit_dir(origin, goal);
    let end = clamp_pos(Position {
        x: origin.x + dx * range,
        y: origin.y + dy * range,
    });
    let dmg = ctx.eval_var(
        "Ability__Magnus__B",
        rank,
        ValueSpec::dmg(0, [60.0, 100.0, 140.0, 180.0, 220.0], 0.5, 0.0, 0.0),
    );
    let stun = ctx.eval_var(
        "Ability__Magnus__B",
        rank,
        ValueSpec::var("STUN_DURATION", [0.4, 0.5, 0.6, 0.7, 1.0]),
    );
    ctx.spawn_effect(
        KitEffectKind::Projectile {
            ability_slot: slot.idx() as u8,
            rank,
            speed_per_tick: 16.0 / TICK_HZ as f32,
            radius: 1.4,
            damage: dmg,
            school: DamageSchool::Crystal,
            piercing: true,
            returning: false,
            return_to_caster: true,
            repeat_mult: 0.75,
            splash_damage: 0.0,
            splash_radius: 0.0,
            stun_ticks: seconds_to_ticks(stun),
            root_ticks: 0,
            silence_ticks: 0,
            slow_pct: 0.0,
            slow_ticks: 0,
            minion_mult: 0.6,
            resets_cd_on_hit: false,
            applies_mark: Some((
                marks::ARCANE_MARK,
                1,
                seconds_to_ticks(magnus_mark_dur(ctx)),
            )),
            destroys_projectiles: false,
            hit_actors: Vec::new(),
        },
        origin,
        end,
    );
    Ok(true)
}

const MAGNUS_A_EFFECTS: &[PrimitiveEffect] = &[PrimitiveEffect::SkillshotProjectile {
    damage: ValueSpec::dmg(0, [80.0, 140.0, 200.0, 260.0, 320.0], 1.0, 0.0, 0.0),
    school: DamageKind::Crystal,
    range: ValueSpec::var("RANGE", [10.0, 10.0, 10.0, 10.0, 12.0]),
    speed: ValueSpec::var("MAIN_SHOTSPEED", [20.0; 5]),
    radius: ValueSpec::constant(0.9),
    piercing: false,
    return_to_caster: false,
    repeat_hit_mult: 1.0,
    splash_damage: Some(ValueSpec::dmg(
        1,
        [60.0, 110.0, 160.0, 210.0, 260.0],
        0.8,
        0.0,
        0.0,
    )),
    splash_radius: 3.5,
    stun_seconds: ValueSpec::zero(),
    root_seconds: ValueSpec::zero(),
    silence_seconds: ValueSpec::zero(),
    slow_pct: ValueSpec::zero(),
    slow_seconds: ValueSpec::zero(),
    minion_mult: 0.65,
    resets_cast_cd_on_hit: false,
    applies_mark: Some((MarkKind::Custom(marks::ARCANE_MARK), 1, 4.0)),
    destroys_enemy_projectiles: false,
}];

const MAGNUS_C_EFFECTS: &[PrimitiveEffect] = &[PrimitiveEffect::DelayedBlast {
    delay: ValueSpec::var3("CAST_DELAY", [1.0; 3]),
    radius: ValueSpec::constant(4.0),
    inner_radius: 0.0,
    damage: ValueSpec::var_scaled(
        "DAMAGE",
        [250.0, 375.0, 500.0, 500.0, 500.0],
        1.2,
        0.0,
        0.0,
        0.0,
    ),
    outer_mult: 1.0,
    school: DamageKind::Crystal,
    stun_seconds: ValueSpec::zero(),
    root_seconds: ValueSpec::zero(),
    sleep_seconds: ValueSpec::zero(),
    slow_pct: ValueSpec::zero(),
    slow_seconds: ValueSpec::zero(),
    reveal_seconds: 0.0,
    bonus_if_marked: Some((MarkKind::Custom(marks::ARCANE_MARK), 0.5)),
    minion_mult: 0.5,
    applies_mark: None,
}];

pub(crate) static MAGNUS: HeroKitSpec = HeroKitSpec {
    hero_id: "Magnus",
    type_id: 418,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Perk__Magnus",
        on_basic_attack: Some(magnus_perk_attack),
        on_ability_damage: Some(magnus_perk_ability_hit),
        on_kill_assist: Some(magnus_perk_kill),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Magnus__A",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::var("CAST_DELAY", [0.3; 5]),
        range: ValueSpec::var("RANGE", [10.0, 10.0, 10.0, 10.0, 12.0]),
        cooldown: ValueSpec::var("COOLDOWN", [6.0, 6.0, 6.0, 6.0, 4.0]),
        cost: ValueSpec::var("ENERGY COST", [40.0, 45.0, 50.0, 55.0, 60.0]),
        effects: MAGNUS_A_EFFECTS,
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Magnus__B",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::var("CAST_DELAY", [0.3; 5]),
        range: ValueSpec::var("RANGE", [10.0; 5]),
        cooldown: ValueSpec::var("COOLDOWN", [14.0, 13.0, 12.0, 11.0, 10.0]),
        cost: ValueSpec::var("ENERGY COST", [60.0, 70.0, 80.0, 90.0, 100.0]),
        custom_exec: Some(magnus_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Magnus__C",
        targeting: CastTargeting::GroundPoint,
        max_rank: 3,
        startup: ValueSpec::constant(0.3),
        range: ValueSpec::var3("RANGE", [22.0; 3]),
        cooldown: ValueSpec::var3("COOLDOWN", [90.0, 75.0, 60.0]),
        cost: ValueSpec::var3("ENERGY COST", [100.0, 125.0, 150.0]),
        effects: MAGNUS_C_EFFECTS,
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 33. Malene (tid=281) — 6-slot Light / Shadow dual-form kit
// ============================================================================

fn malene_b1_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    _target: CastTarget,
) -> Result<bool, &'static str> {
    let dur = ctx.eval_var(
        "Ability__Malene__B1",
        rank,
        ValueSpec::var("Duration", [3.0; 5]),
    );
    let bar = ctx.eval_var(
        "Ability__Malene__B1",
        rank,
        ValueSpec::var_scaled(
            "Malene Barrier",
            [75.0, 120.0, 165.0, 210.0, 300.0],
            0.6,
            0.0,
            0.0,
            0.0,
        ),
    ) * ctx.talent_barrier_mult(slot);
    let ms = ctx.eval_var(
        "Ability__Malene__B1",
        rank,
        ValueSpec::var("Movespeed Boost", [0.8, 1.0, 1.2, 1.4, 1.8]),
    );
    if ctx.talent_special() == TalentSpecial::MaleneLightBAllyBarrierBoom {
        for a in ctx.allies_in_radius(ctx.caster_pos(), 6.0, true, true) {
            ctx.grant_barrier(a, bar, dur, None);
            ctx.add_buff(a, BuffStatKind::MoveSpeedFlat, ms, dur);
        }
        let dmg = bar * 0.75 * ctx.talent_dmg_mult(slot);
        for j in ctx.enemies_in_radius(ctx.caster_pos(), 3.5, false) {
            ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
        }
    } else {
        ctx.grant_barrier(ctx.caster, bar, dur, None);
        ctx.add_buff(ctx.caster, BuffStatKind::MoveSpeedFlat, ms, dur);
    }
    Ok(true)
}

fn malene_c_talent_explosion(ctx: &mut HeroKitContext<'_, '_>, slot: AbilitySlot) {
    if ctx.talent_special() == TalentSpecial::None
        && let Some(boom_dmg) = ctx.talent_mod("EXPLOSION_DAMAGE")
    {
        let rad = ctx.talent_mod("BLESSING_RADIUS").unwrap_or(5.0);
        for j in ctx.enemies_in_radius(ctx.caster_pos(), rad.min(8.0), false) {
            ctx.deal_damage(j, boom_dmg, DamageSchool::Crystal, 0.0, false);
        }
        let bar = 100.0 * ctx.talent_barrier_mult(slot);
        for a in ctx.allies_in_radius(ctx.caster_pos(), rad.min(8.0), true, true) {
            ctx.grant_barrier(a, bar, 3.0, None);
        }
    }
}

fn malene_c1_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    _target: CastTarget,
) -> Result<bool, &'static str> {
    ctx.swap_form(1);
    let emp_dmg = ctx.eval_var(
        "Ability__Malene__C1",
        rank,
        ValueSpec::var_scaled(
            "Emp Bonus Damage",
            [30.0, 60.0, 90.0, 120.0, 150.0],
            0.5,
            0.0,
            0.0,
            0.0,
        ),
    ) * ctx.talent_dmg_mult(slot);
    let dur = ctx.eval_var(
        "Ability__Malene__C1",
        rank,
        ValueSpec::var("Dark Emp Duration", [6.0; 5]),
    );
    ctx.empower_next_attacks(1, emp_dmg, true, 0.0, 0.0, 0.0, 0.0, dur);
    malene_c_talent_explosion(ctx, slot);
    Ok(true)
}

fn malene_c2_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    _target: CastTarget,
) -> Result<bool, &'static str> {
    ctx.swap_form(0);
    let emp_dmg = ctx.eval_var(
        "Ability__Malene__C2",
        rank,
        ValueSpec::dmg(0, [70.0; 5], 0.5, 0.0, 1.0),
    ) * ctx.talent_dmg_mult(slot);
    let slow = ctx.eval_var(
        "Ability__Malene__C2",
        rank,
        ValueSpec::var("Light Emp Slow", [0.25, 0.3, 0.35, 0.4, 0.45]),
    );
    let slow_dur = ctx.eval_var(
        "Ability__Malene__C2",
        rank,
        ValueSpec::var("Light Slow Duration", [1.0; 5]),
    );
    let dur = ctx.eval_var(
        "Ability__Malene__C2",
        rank,
        ValueSpec::var("Light Emp Duration", [6.0; 5]),
    );
    ctx.empower_next_attacks(1, emp_dmg, true, 0.0, slow, slow_dur, 0.0, dur);
    malene_c_talent_explosion(ctx, slot);
    Ok(true)
}

const MALENE_A1_EFFECTS: &[PrimitiveEffect] = &[PrimitiveEffect::SkillshotProjectile {
    damage: ValueSpec::dmg(0, [60.0, 105.0, 150.0, 195.0, 240.0], 1.1, 0.0, 0.0),
    school: DamageKind::Crystal,
    range: ValueSpec::var("RANGE", [9.0; 5]),
    speed: ValueSpec::var("Shot Speed", [12.0; 5]),
    radius: ValueSpec::constant(0.9),
    piercing: false,
    return_to_caster: false,
    repeat_hit_mult: 1.0,
    splash_damage: None,
    splash_radius: 0.0,
    stun_seconds: ValueSpec::zero(),
    root_seconds: ValueSpec::var("Root Duration", [1.0, 1.0, 1.0, 1.0, 1.5]),
    silence_seconds: ValueSpec::zero(),
    slow_pct: ValueSpec::zero(),
    slow_seconds: ValueSpec::zero(),
    minion_mult: 0.7,
    resets_cast_cd_on_hit: false,
    applies_mark: None,
    destroys_enemy_projectiles: false,
}];

const MALENE_A2_EFFECTS: &[PrimitiveEffect] = &[PrimitiveEffect::ConeWave {
    range: ValueSpec::var("RANGE", [8.5; 5]),
    half_angle_deg: 28.0,
    damage: ValueSpec::dmg(0, [30.0, 60.0, 90.0, 120.0, 150.0], 0.65, 0.0, 0.0),
    school: DamageKind::Crystal,
    multi_bolts: 3,
    extra_hit_mult: 0.5,
    slow_pct: ValueSpec::zero(),
    slow_seconds: ValueSpec::zero(),
    stun_seconds: ValueSpec::zero(),
    fear_seconds: ValueSpec::zero(),
    silence_seconds: ValueSpec::zero(),
    knockback_dist: 0.0,
    reveal_seconds: 0.0,
    minion_mult: 0.7,
    applies_mark: None,
}];

const MALENE_B2_EFFECTS: &[PrimitiveEffect] = &[
    PrimitiveEffect::CleanseImmune {
        cleanse_cc: false,
        cc_immune_seconds: ValueSpec::zero(),
        untargetable_seconds: ValueSpec::constant(1.5),
    },
    PrimitiveEffect::DashBlink {
        range: ValueSpec::var("Range", [10.0; 5]),
        duration: ValueSpec::var("Travel Time", [1.5; 5]),
        instant_blink: false,
        invuln_during: true,
        unstoppable_during: true,
        path_radius: 1.5,
        path_damage: ValueSpec::zero(),
        arrival_radius: 3.5,
        arrival_damage: ValueSpec::dmg(0, [80.0, 160.0, 240.0, 320.0, 480.0], 0.7, 0.0, 0.0),
        school: DamageKind::Crystal,
        slow_pct: ValueSpec::var("Shadow Slow Amount", [0.3, 0.35, 0.4, 0.45, 0.5]),
        slow_seconds: ValueSpec::var("Shadow Slow Duration", [2.0; 5]),
        stun_seconds: ValueSpec::zero(),
        knockback_dist: 0.0,
        backward_hop: false,
        applies_mark: None,
    },
];

static MALENE_VARIANTS: &[AbilityVariantSpec] = &[
    AbilityVariantSpec {
        slot: HeroSlot::A2,
        spec: AbilityKitSpec {
            ability_id: "Ability__Malene__A2",
            targeting: CastTargeting::Directional,
            max_rank: 5,
            startup: ValueSpec::var("Delay", [0.4; 5]),
            range: ValueSpec::var("RANGE", [8.5; 5]),
            cooldown: ValueSpec::var("Cooldown", [10.0, 9.5, 9.0, 8.5, 8.0]),
            cost: ValueSpec::var("Energy Cost", [40.0, 50.0, 60.0, 70.0, 35.0]),
            effects: MALENE_A2_EFFECTS,
            ..AbilityKitSpec::DEFAULT
        },
    },
    AbilityVariantSpec {
        slot: HeroSlot::B2,
        spec: AbilityKitSpec {
            ability_id: "Ability__Malene__B2",
            targeting: CastTargeting::GroundPoint,
            max_rank: 5,
            startup: ValueSpec::constant(0.1),
            range: ValueSpec::var("Range", [10.0; 5]),
            cooldown: ValueSpec::var("Cooldown", [20.0, 19.0, 18.0, 17.0, 14.0]),
            cost: ValueSpec::var("Energy Cost", [80.0, 90.0, 100.0, 110.0, 120.0]),
            effects: MALENE_B2_EFFECTS,
            ..AbilityKitSpec::DEFAULT
        },
    },
    AbilityVariantSpec {
        slot: HeroSlot::C2,
        spec: AbilityKitSpec {
            ability_id: "Ability__Malene__C2",
            targeting: CastTargeting::InstantSelf,
            max_rank: 5,
            startup: ValueSpec::zero(),
            range: ValueSpec::constant(1.5),
            cooldown: ValueSpec::var("Cooldown", [6.0, 5.0, 4.0, 3.0, 2.0]),
            cost: ValueSpec::zero(),
            custom_exec: Some(malene_c2_cast),
            ..AbilityKitSpec::DEFAULT
        },
    },
];

pub(crate) static MALENE: HeroKitSpec = HeroKitSpec {
    hero_id: "Malene",
    type_id: 281,
    resource: ResourceKind::Energy,
    ult_from_level_one: true,
    ult_max_rank: 5,
    perk: HeroicPerkSpec {
        id: "Perk__Malene",
        basic_attack_is_crystal: true,
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Malene__A1",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::var("Delay", [0.3; 5]),
        range: ValueSpec::var("RANGE", [9.0; 5]),
        cooldown: ValueSpec::var("Cooldown", [16.0, 15.0, 14.0, 13.0, 12.0]),
        cost: ValueSpec::var("Energy Cost", [40.0, 50.0, 60.0, 70.0, 35.0]),
        effects: MALENE_A1_EFFECTS,
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Malene__B1",
        targeting: CastTargeting::InstantSelf,
        max_rank: 5,
        startup: ValueSpec::zero(),
        range: ValueSpec::constant(1.5),
        cooldown: ValueSpec::var("Cooldown", [12.0; 5]),
        cost: ValueSpec::var("Energy Cost", [60.0, 70.0, 80.0, 90.0, 100.0]),
        custom_exec: Some(malene_b1_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Malene__C1",
        targeting: CastTargeting::InstantSelf,
        max_rank: 5,
        startup: ValueSpec::zero(),
        range: ValueSpec::constant(1.5),
        cooldown: ValueSpec::var("Cooldown", [6.0, 5.0, 4.0, 3.0, 2.0]),
        cost: ValueSpec::zero(),
        custom_exec: Some(malene_c1_cast),
        ..AbilityKitSpec::DEFAULT
    },
    variants: MALENE_VARIANTS,
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

pub(crate) static MIHO: HeroKitSpec = HeroKitSpec {
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

pub(crate) static OZO: HeroKitSpec = HeroKitSpec {
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
// 36. Petal (tid=246)
// ============================================================================

const PETAL_MUNION_KIND: u8 = 1;

fn petal_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    _target: CastTarget,
) -> Result<bool, &'static str> {
    let owner_id = ctx.caster_id();
    let pet = ctx
        .room
        .entities
        .ecs
        .query::<(bevy_ecs::entity::Entity, &KitEffect)>()
        .iter(&ctx.room.entities.ecs)
        .find_map(|(eh, eff)| {
            (eff.owner_id == owner_id
                && matches!(
                    eff.kind,
                    KitEffectKind::SummonPet {
                        pet_kind: PETAL_MUNION_KIND,
                        ..
                    }
                ))
            .then_some((eh, eff.position))
        });
    let center = if let Some((eh, pos)) = pet {
        ctx.room.entities.remove_spell_handle::<KitEffect>(eh);
        pos
    } else {
        ctx.caster_pos()
    };
    let dmg = ctx.eval_var(
        "Ability__Petal__C",
        rank,
        ValueSpec::var_scaled(
            "Damage",
            [180.0, 240.0, 300.0, 300.0, 300.0],
            0.7,
            0.0,
            0.0,
            0.0,
        ),
    ) * ctx.talent_dmg_mult(slot);
    let heal = ctx.eval_var(
        "Ability__Petal__C",
        rank,
        ValueSpec::var_scaled(
            "Heal",
            [75.0, 125.0, 175.0, 175.0, 175.0],
            0.75,
            0.0,
            0.0,
            0.0,
        ),
    ) * ctx.talent_heal_mult(slot);
    let rad = ctx.eval_var(
        "Ability__Petal__C",
        rank,
        ValueSpec::var3("Explode Radius", [5.0; 3]),
    );
    let slow = ctx.eval_var("Ability__Petal__C", rank, ValueSpec::var3("Slow", [0.3; 3]));
    let kb = if ctx.talent_special() == TalentSpecial::PetalUltKnockback {
        ctx.talent_mod("KNOCKBACK_DISTANCE").unwrap_or(4.0)
    } else {
        0.0
    };
    for j in ctx.enemies_in_radius(center, rad, false) {
        ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
        ctx.apply_slow(j, slow, 2.0);
        if kb > 0.0 {
            ctx.knockback(j, center, kb, 0.0);
        }
    }
    for a in ctx.allies_in_radius(center, rad, true, true) {
        ctx.heal_unit(a, heal);
    }
    Ok(true)
}

const PETAL_A_EFFECTS: &[PrimitiveEffect] = &[
    PrimitiveEffect::TrapZone {
        arm_delay: ValueSpec::var("DelayBeforeExplode", [1.2; 5]),
        duration: ValueSpec::var("Seed Lifetime", [12.0; 5]),
        trigger_radius: ValueSpec::var("LandMine Radius", [1.9; 5]),
        blast_radius: ValueSpec::var("Explosion Radius", [1.9; 5]),
        damage: ValueSpec::var_scaled(
            "Explosion Damage",
            [75.0, 125.0, 175.0, 225.0, 275.0],
            1.0,
            0.0,
            0.0,
            0.0,
        ),
        school: DamageKind::Crystal,
        root_seconds: ValueSpec::zero(),
        stun_seconds: ValueSpec::zero(),
        slow_pct: ValueSpec::constant(0.25),
        max_traps: 6,
        reveals_enemies: true,
    },
    PrimitiveEffect::SummonPet {
        count: 1,
        max_active: 3,
        duration: ValueSpec::constant(45.0),
        pet_hp: ValueSpec::var_scaled(
            "Munion Health",
            [100.0, 180.0, 260.0, 340.0, 500.0],
            0.7,
            0.0,
            0.0,
            0.0,
        ),
        pet_damage: ValueSpec::var_scaled(
            "Munion Damage",
            [7.0, 14.0, 21.0, 28.0, 42.0],
            0.4,
            0.0,
            0.0,
            0.0,
        ),
        school: DamageKind::Crystal,
        hit_ticks: 3,
        applies_mark: None,
        replace_existing: false,
        pet_kind: PETAL_MUNION_KIND,
    },
];

const PETAL_B_EFFECTS: &[PrimitiveEffect] = &[
    PrimitiveEffect::DashBlink {
        range: ValueSpec::var("MOVE_DISTANCE", [6.5; 5]),
        duration: ValueSpec::constant(0.2),
        instant_blink: true,
        invuln_during: false,
        unstoppable_during: false,
        path_radius: 1.0,
        path_damage: ValueSpec::zero(),
        arrival_radius: 1.5,
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
        stat: BuffStatKind::AttackRange,
        amount: ValueSpec::var("BonusAttackRange", [1.2; 5]),
        duration: ValueSpec::var("DURATION", [5.0; 5]),
        empowered_attacks: 0,
        empowered_bonus_damage: ValueSpec::zero(),
        empowered_stun_seconds: 0.0,
        empowered_slow_pct: 0.0,
        empowered_slow_seconds: 0.0,
        empowered_knockback: 0.0,
        resets_attack_cd: false,
    },
    PrimitiveEffect::SelfBuff {
        stat: BuffStatKind::CrystalPowerPct,
        amount: ValueSpec::var("Crystal Amp", [0.15; 5]),
        duration: ValueSpec::var("DURATION", [5.0; 5]),
        empowered_attacks: 0,
        empowered_bonus_damage: ValueSpec::zero(),
        empowered_stun_seconds: 0.0,
        empowered_slow_pct: 0.0,
        empowered_slow_seconds: 0.0,
        empowered_knockback: 0.0,
        resets_attack_cd: false,
    },
    PrimitiveEffect::SelfBuff {
        stat: BuffStatKind::WeaponPowerPct,
        amount: ValueSpec::var("Weapon Amp", [0.15; 5]),
        duration: ValueSpec::var("DURATION", [5.0; 5]),
        empowered_attacks: 0,
        empowered_bonus_damage: ValueSpec::zero(),
        empowered_stun_seconds: 0.0,
        empowered_slow_pct: 0.0,
        empowered_slow_seconds: 0.0,
        empowered_knockback: 0.0,
        resets_attack_cd: false,
    },
];

pub(crate) static PETAL: HeroKitSpec = HeroKitSpec {
    hero_id: "Petal",
    type_id: 246,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Perk__Petal",
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Petal__A",
        targeting: CastTargeting::GroundPoint,
        max_rank: 5,
        startup: ValueSpec::constant(0.1),
        range: ValueSpec::constant(6.0),
        cooldown: ValueSpec::var("Cooldown", [2.0; 5]),
        cost: ValueSpec::var("Energy Cost", [40.0; 5]),
        effects: PETAL_A_EFFECTS,
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Petal__B",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::zero(),
        range: ValueSpec::var("MOVE_DISTANCE", [6.5; 5]),
        cooldown: ValueSpec::var("Cooldown", [15.0, 14.5, 14.0, 13.5, 12.0]),
        cost: ValueSpec::var("Energy Cost", [80.0, 85.0, 90.0, 95.0, 100.0]),
        effects: PETAL_B_EFFECTS,
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Petal__C",
        targeting: CastTargeting::InstantSelf,
        max_rank: 3,
        startup: ValueSpec::constant(0.2),
        range: ValueSpec::var3("Explode Radius", [5.0; 3]),
        cooldown: ValueSpec::var3("Cooldown", [30.0, 25.0, 20.0]),
        cost: ValueSpec::var3("Energy Cost", [40.0; 3]),
        charges: ValueSpec::constant(3.0),
        between_charges_cd: ValueSpec::var3("Cooldown2", [0.4; 3]),
        custom_exec: Some(petal_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
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

pub(crate) static PHINN: HeroKitSpec = HeroKitSpec {
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

pub(crate) static REIM: HeroKitSpec = HeroKitSpec {
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

pub(crate) static REZA: HeroKitSpec = HeroKitSpec {
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

pub(crate) static RINGO: HeroKitSpec = HeroKitSpec {
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

pub(crate) static RONA: HeroKitSpec = HeroKitSpec {
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
// 42. Samuel (tid=276)
// ============================================================================

const SAMUEL_DARK_ZONE: u8 = 42;

fn samuel_perk_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    _target_idx: usize,
    _base_damage: f32,
    _was_crit: bool,
) -> (f32, DamageSchool) {
    let cd_sec = if ctx.talent_special() == TalentSpecial::SamuelFrequentPerk {
        ctx.talent_mod("CHARGE_TIME").unwrap_or(2.0)
    } else {
        6.0
    };
    let ready = ctx.room.entities[ctx.caster]
        .hero_kit
        .as_deref()
        .is_none_or(|k| ctx.tick >= k.timers[0]);
    if ready {
        let lvl = f32::from(ctx.room.entities[ctx.caster].level);
        let cp = ctx.eval_inputs().cp;
        {
            let k = ctx.room.entities[ctx.caster]
                .hero_kit
                .get_or_insert_with(Box::default);
            k.timers[0] = ctx.tick + seconds_to_ticks(cd_sec);
        }
        let mut heal = 50.0 + lvl * 15.0 + cp * 0.25;
        if ctx.talent_special() == TalentSpecial::SamuelFrequentPerk {
            heal *= ctx.talent_mod("HEALING_MODIFIER").unwrap_or(1.5);
        }
        ctx.heal_unit(ctx.caster, heal);
        let e = &mut ctx.room.entities[ctx.caster];
        e.energy = (e.energy + 45.0).min(e.max_energy);
        (50.0 + lvl * 15.0 + cp * 0.6, DamageSchool::Crystal)
    } else {
        (0.0, DamageSchool::Crystal)
    }
}

fn samuel_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let empowered = has_nearby_active_zone(ctx, ctx.caster_pos(), SAMUEL_DARK_ZONE, 0.5);
    let range = if empowered { 12.0 } else { 10.0 };
    let dmg = if empowered {
        ctx.eval_var(
            "Ability__Samuel__A",
            rank,
            ValueSpec::dmg(1, [75.0, 105.0, 135.0, 165.0, 225.0], 1.0, 0.0, 0.0),
        )
    } else {
        ctx.eval_var(
            "Ability__Samuel__A",
            rank,
            ValueSpec::dmg(0, [60.0, 90.0, 120.0, 150.0, 210.0], 0.9, 0.0, 0.0),
        )
    };
    let origin = ctx.caster_pos();
    let goal = ctx.resolve_target_pos(target, range);
    let (dx, dy) = unit_dir(origin, goal);
    let end = clamp_pos(Position {
        x: origin.x + dx * range,
        y: origin.y + dy * range,
    });
    let spd = ctx.eval_var(
        "Ability__Samuel__A",
        rank,
        ValueSpec::var("Speed Boost While Firing", [1.2; 5]),
    );
    ctx.add_buff(ctx.caster, BuffStatKind::MoveSpeedFlat, spd, 1.0);
    ctx.spawn_effect(
        KitEffectKind::Projectile {
            ability_slot: slot.idx() as u8,
            rank,
            speed_per_tick: 18.0 / TICK_HZ as f32,
            radius: 0.85,
            damage: dmg,
            school: DamageSchool::Crystal,
            piercing: false,
            return_to_caster: false,
            returning: false,
            repeat_mult: 1.0,
            splash_damage: if empowered { dmg * 0.7 } else { 0.0 },
            splash_radius: 3.0,
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
    Ok(true)
}

fn samuel_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let range = ctx.eval_var(
        "Ability__Samuel__B",
        rank,
        ValueSpec::var("Range", [14.0, 18.0, 22.0, 26.0, 30.0]),
    );
    let origin = ctx.caster_pos();
    let aim = ctx.resolve_target_pos(target, range);
    let (dx, dy) = unit_dir(origin, aim);
    let rad = ctx.eval_var(
        "Ability__Samuel__B",
        rank,
        ValueSpec::var("Dark Field Radius", [6.0; 5]),
    );
    let dps = ctx.eval_var(
        "Ability__Samuel__B",
        rank,
        ValueSpec::var_scaled(
            "Dark Field DPS",
            [20.0, 30.0, 40.0, 50.0, 60.0],
            0.1,
            0.0,
            0.0,
            0.0,
        ),
    );
    let hps = ctx.eval_var(
        "Ability__Samuel__B",
        rank,
        ValueSpec::var_scaled(
            "Dark Field HPS Per Target",
            [2.0, 4.0, 6.0, 8.0, 10.0],
            0.1,
            0.0,
            0.0,
            0.0,
        ),
    );
    let mut dur = 6.0;
    let mut slow = 0.0;
    if ctx.talent_special() == TalentSpecial::SamuelSlowingDriftingDark {
        dur += ctx.talent_mod("DURATION").unwrap_or(2.0);
        slow = ctx.talent_mod("SLOW_STRENGTH").unwrap_or(0.5);
    }
    ctx.spawn_effect(
        KitEffectKind::GroundZone {
            ability_slot: slot.idx() as u8,
            zone_kind: SAMUEL_DARK_ZONE,
            radius: rad,
            ticks_interval: TICK_HZ / 2,
            next_tick: ctx.tick + seconds_to_ticks(0.4),
            end_tick: ctx.tick + seconds_to_ticks(dur),
            tick_damage: dps,
            tick_heal: hps,
            school: DamageSchool::Crystal,
            slow_pct: slow,
            ally_speed_bonus: 0.0,
            end_damage: 0.0,
            end_root_ticks: 0,
            end_stun_ticks: 0,
            drift_dx: dx * 0.14,
            drift_dy: dy * 0.14,
            follow_caster: false,
            ignited: false,
            minion_mult: 0.5,
            applies_mark: None,
        },
        origin,
        origin,
    );
    Ok(true)
}

fn samuel_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let center = ctx.resolve_target_pos(target, 12.0);
    let warn = ctx.eval_var(
        "Ability__Samuel__C",
        rank,
        ValueSpec::var3("Warning Time", [1.0; 3]),
    );
    let rad = ctx.eval_var(
        "Ability__Samuel__C",
        rank,
        ValueSpec::var3("Radius", [4.0; 3]),
    );
    let dmg = ctx.eval_var(
        "Ability__Samuel__C",
        rank,
        ValueSpec::dmg3(0, [200.0, 325.0, 450.0], 1.0, 0.0, 0.0),
    );
    let mut sleep_dur = ctx.eval_var(
        "Ability__Samuel__C",
        rank,
        ValueSpec::var3("CC Duration", [2.0, 2.5, 3.0]),
    );
    let unwaking = ctx.talent_special() == TalentSpecial::SamuelSleepNeverWakes;
    if unwaking && let Some(s) = ctx.talent_mod("SLEEP_DURATION") {
        sleep_dur += s;
    }
    ctx.spawn_effect(
        KitEffectKind::DelayedBlast {
            ability_slot: slot.idx() as u8,
            release_tick: ctx.tick + seconds_to_ticks(warn),
            radius: rad,
            inner_radius: 0.0,
            damage: dmg,
            outer_mult: 1.0,
            school: DamageSchool::Crystal,
            stun_ticks: 0,
            root_ticks: 0,
            sleep_ticks: seconds_to_ticks(sleep_dur),
            sleep_unwaking: unwaking,
            slow_pct: 0.0,
            slow_ticks: 0,
            reveal_ticks: 0,
            bonus_mark: None,
            minion_mult: 0.5,
            applies_mark: None,
        },
        center,
        center,
    );
    Ok(true)
}

pub(crate) static SAMUEL: HeroKitSpec = HeroKitSpec {
    hero_id: "Samuel",
    type_id: 276,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Perk__Samuel",
        on_basic_attack: Some(samuel_perk_attack),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Samuel__A",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::var("Delay Before First Shot", [0.1; 5]),
        range: ValueSpec::var("Range", [10.0; 5]),
        cooldown: ValueSpec::var("Cooldown", [4.0, 4.0, 4.0, 4.0, 3.0]),
        cost: ValueSpec::var("Energy Cost", [40.0, 45.0, 50.0, 55.0, 60.0]),
        custom_exec: Some(samuel_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Samuel__B",
        targeting: CastTargeting::GroundPoint,
        max_rank: 5,
        startup: ValueSpec::var("Cast Delay", [0.4; 5]),
        range: ValueSpec::var("Range", [14.0, 18.0, 22.0, 26.0, 30.0]),
        cooldown: ValueSpec::var("Cooldown", [26.0; 5]),
        cost: ValueSpec::var("Energy Cost", [120.0, 160.0, 200.0, 240.0, 280.0]),
        custom_exec: Some(samuel_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Samuel__C",
        targeting: CastTargeting::GroundPoint,
        max_rank: 3,
        startup: ValueSpec::var3("Cast Time", [0.5; 3]),
        range: ValueSpec::constant(12.0),
        cooldown: ValueSpec::var3("Cooldown", [90.0, 75.0, 60.0]),
        cost: ValueSpec::var3("Energy Cost", [100.0, 130.0, 160.0]),
        custom_exec: Some(samuel_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
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

pub(crate) static SANFENG: HeroKitSpec = HeroKitSpec {
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

pub(crate) static SAW: HeroKitSpec = HeroKitSpec {
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

pub(crate) static SHIN: HeroKitSpec = HeroKitSpec {
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

pub(crate) static SILVERNAIL: HeroKitSpec = HeroKitSpec {
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
// 47. Hero010 / Skaarf (tid=255)
// ============================================================================

fn skaarf_perk_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    target_idx: usize,
    _raw_dmg: f32,
    _is_crit: bool,
) -> (f32, DamageSchool) {
    if ctx.mark_stacks(target_idx, marks::GOOP) > 0 {
        let lvl = f32::from(ctx.room.entities[ctx.caster].level.max(1));
        let max_hp = ctx.room.entities[target_idx].max_hp;
        let bonus = max_hp * (0.02 + 0.0025 * lvl);
        (bonus, DamageSchool::Crystal)
    } else {
        (0.0, DamageSchool::Weapon)
    }
}

fn skaarf_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let range = ctx.talent_mod("RANGE").unwrap_or(11.5);
    let dmg = if ctx.talent_special() == TalentSpecial::SkaarfSpitfireDropsGoop {
        ctx.eval_var(
            "Ability__Skaarf__A",
            rank,
            ValueSpec::var_scaled(
                "Damage",
                [75.0, 125.0, 175.0, 225.0, 275.0],
                1.1,
                0.0,
                0.0,
                0.0,
            ),
        ) * ctx.talent_mod("SPITFIRE_DAMAGE_MODIFIER").unwrap_or(1.0)
    } else {
        ctx.eval_var(
            "Ability__Skaarf__A",
            rank,
            ValueSpec::var_scaled(
                "Damage",
                [75.0, 125.0, 175.0, 225.0, 275.0],
                1.1,
                0.0,
                0.0,
                0.0,
            ),
        ) * ctx.talent_dmg_mult(slot)
    };
    let speed = ctx.eval_var(
        "Ability__Skaarf__A",
        rank,
        ValueSpec::var("SHOT_SPEED", [14.5; 5]),
    );
    let minion_mult = ctx.eval_var(
        "Ability__Skaarf__A",
        rank,
        ValueSpec::var("MINION_DAMAGE_REDUCTION", [0.5; 5]),
    );
    let origin = ctx.caster_pos();
    let aim = ctx.resolve_target_pos(target, range);
    let (dx, dy) = unit_dir(origin, aim);
    let angles: &[f32] = if ctx.talent_special() == TalentSpecial::SkaarfSplitfireThreeCones {
        &[0.0, 15.0_f32.to_radians(), -15.0_f32.to_radians()]
    } else {
        &[0.0]
    };

    for &ang in angles {
        let c = ang.cos();
        let s = ang.sin();
        let rx = dx * c - dy * s;
        let ry = dx * s + dy * c;
        let end = clamp_pos(Position {
            x: origin.x + rx * range,
            y: origin.y + ry * range,
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
                repeat_mult: 0.85,
                splash_damage: 0.0,
                splash_radius: 0.0,
                stun_ticks: 0,
                root_ticks: 0,
                silence_ticks: 0,
                slow_pct: 0.0,
                slow_ticks: 0,
                minion_mult,
                resets_cd_on_hit: false,
                applies_mark: Some((marks::GOOP, 1, seconds_to_ticks(4.5))),
                destroys_projectiles: false,
                hit_actors: Vec::new(),
            },
            origin,
            end,
        );
    }

    if ctx.talent_special() == TalentSpecial::SkaarfSpitfireDropsGoop {
        let dot = dmg * 0.25 * ctx.talent_mod("GOOP_DAMAGE_MODIFIER").unwrap_or(1.0);
        let slow = ctx.talent_mod("SLOW_MODIFIER").unwrap_or(0.25);
        ctx.spawn_effect(
            KitEffectKind::GroundZone {
                ability_slot: slot.idx() as u8,
                zone_kind: 47,
                radius: 2.6,
                ticks_interval: TICK_HZ / 2,
                next_tick: ctx.tick + TICK_HZ / 2,
                end_tick: ctx.tick + seconds_to_ticks(4.5),
                tick_damage: dot * 0.5,
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
                ignited: true,
                minion_mult: 0.5,
                applies_mark: Some((marks::GOOP, 1, seconds_to_ticks(4.0))),
            },
            aim,
            aim,
        );
    }
    Ok(true)
}

fn skaarf_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let center = ctx.resolve_target_pos(target, 9.0);
    let burst = ctx.eval_var(
        "Ability__Skaarf__B",
        rank,
        ValueSpec::var_scaled(
            "Damage",
            [60.0, 105.0, 150.0, 195.0, 240.0],
            0.7,
            0.0,
            0.0,
            0.0,
        ),
    ) * ctx.talent_dmg_mult(slot);
    let dot = ctx.eval_var(
        "Ability__Skaarf__B",
        rank,
        ValueSpec::var_scaled(
            "DamageOverTime",
            [40.0, 70.0, 100.0, 130.0, 160.0],
            0.5,
            0.0,
            0.0,
            0.0,
        ),
    ) * ctx.talent_dmg_mult(slot);
    let slow = ctx.eval_var("Ability__Skaarf__B", rank, ValueSpec::var("Slow", [0.4; 5]));
    let dur = ctx.eval_var(
        "Ability__Skaarf__B",
        rank,
        ValueSpec::var("Duration", [8.0, 8.0, 8.0, 8.0, 10.0]),
    );

    for j in ctx.enemies_in_radius(center, 3.2, false) {
        ctx.deal_damage(j, burst * 0.35, DamageSchool::Crystal, 0.0, false);
        ctx.add_mark(j, marks::GOOP, 1, 1, 4.5);
    }
    ctx.spawn_effect(
        KitEffectKind::GroundZone {
            ability_slot: slot.idx() as u8,
            zone_kind: 47,
            radius: 3.2,
            ticks_interval: TICK_HZ / 2,
            next_tick: ctx.tick + TICK_HZ / 2,
            end_tick: ctx.tick + seconds_to_ticks(dur),
            tick_damage: dot * 0.5,
            tick_heal: 0.0,
            school: DamageSchool::Crystal,
            slow_pct: slow,
            ally_speed_bonus: 0.0,
            end_damage: burst * 0.65,
            end_root_ticks: 0,
            end_stun_ticks: 0,
            drift_dx: 0.0,
            drift_dy: 0.0,
            follow_caster: false,
            ignited: false,
            minion_mult: 0.5,
            applies_mark: Some((marks::GOOP, 1, seconds_to_ticks(4.5))),
        },
        center,
        center,
    );
    Ok(true)
}

const SKAARF_C_EFFECTS: &[PrimitiveEffect] = &[
    PrimitiveEffect::SelfBuff {
        stat: BuffStatKind::MoveSpeedFlat,
        amount: ValueSpec::var3("SPEED_BOOST", [1.0; 3]),
        duration: ValueSpec::constant(4.5),
        empowered_attacks: 0,
        empowered_bonus_damage: ValueSpec::zero(),
        empowered_stun_seconds: 0.0,
        empowered_slow_pct: 0.0,
        empowered_slow_seconds: 0.0,
        empowered_knockback: 0.0,
        resets_attack_cd: false,
    },
    PrimitiveEffect::SelfBuff {
        stat: BuffStatKind::FortifiedHealth,
        amount: ValueSpec::constant(220.0),
        duration: ValueSpec::constant(4.5),
        empowered_attacks: 0,
        empowered_bonus_damage: ValueSpec::zero(),
        empowered_stun_seconds: 0.0,
        empowered_slow_pct: 0.0,
        empowered_slow_seconds: 0.0,
        empowered_knockback: 0.0,
        resets_attack_cd: false,
    },
    PrimitiveEffect::ChannelBeam {
        duration: ValueSpec::var3("CHANNEL_DUR", [3.0; 3]),
        ticks_per_sec: 6.0,
        range: ValueSpec::constant(7.5),
        width: 2.6,
        dps: ValueSpec::var_scaled(
            "DAMAGE_PER_SECOND",
            [300.0, 475.0, 650.0, 650.0, 650.0],
            1.5,
            0.0,
            0.0,
            0.0,
        ),
        school: DamageKind::Crystal,
        lock_caster: false,
        slow_pct: ValueSpec::var3("SLOW", [0.35; 3]),
        stun_target: false,
        global_all_heroes: false,
        shield_pierce: ValueSpec::constant(0.25),
        structure_mult: 0.2,
        minion_mult: 0.5,
    },
];

pub(crate) static SKAARF: HeroKitSpec = HeroKitSpec {
    hero_id: "Hero010",
    type_id: 255,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Perk__Skaarf",
        on_basic_attack: Some(skaarf_perk_attack),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Skaarf__A",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::constant(0.15),
        range: ValueSpec::constant(11.5),
        cooldown: ValueSpec::var("Cooldown", [6.0, 5.5, 5.0, 4.5, 4.0]),
        cost: ValueSpec::var("Energy Cost", [30.0, 35.0, 40.0, 45.0, 50.0]),
        custom_exec: Some(skaarf_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Skaarf__B",
        targeting: CastTargeting::GroundPoint,
        max_rank: 5,
        startup: ValueSpec::constant(0.15),
        range: ValueSpec::constant(9.0),
        cooldown: ValueSpec::var("Cooldown", [9.0, 8.5, 8.0, 7.5, 6.0]),
        cost: ValueSpec::var("Energy Cost", [40.0, 50.0, 60.0, 70.0, 80.0]),
        custom_exec: Some(skaarf_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Skaarf__C",
        targeting: CastTargeting::InstantSelf,
        max_rank: 3,
        startup: ValueSpec::constant(0.8),
        range: ValueSpec::constant(7.5),
        cooldown: ValueSpec::var3("Cooldown", [90.0, 75.0, 60.0]),
        cost: ValueSpec::var3("Energy Cost", [100.0, 125.0, 150.0]),
        effects: SKAARF_C_EFFECTS,
        ..AbilityKitSpec::DEFAULT
    },
    ..HeroKitSpec::DEFAULT
};

// ============================================================================
// 48. Skye (tid=265)
// ============================================================================

fn skye_perk_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    target_idx: usize,
    _raw_dmg: f32,
    _is_crit: bool,
) -> (f32, DamageSchool) {
    let tid = ctx.room.entities[target_idx].id;
    let b_rank = ctx.room.entities[ctx.caster].ability_ranks[1];
    let bonus_dur = if b_rank > 0 {
        ctx.eval_var(
            "Ability__Skye__B",
            b_rank,
            ValueSpec::var("LockOnDurationBonus", [1.0, 1.5, 2.0, 2.5, 4.0]),
        )
    } else {
        0.0
    };
    let k = ctx.room.entities[ctx.caster]
        .hero_kit
        .get_or_insert_with(Box::default);
    k.lock_target_id = tid;
    k.lock_target_until = ctx.tick + seconds_to_ticks(3.5 + bonus_dur);
    ctx.add_buff(ctx.caster, BuffStatKind::MoveSpeedFlat, 1.4, 2.0);
    (0.0, DamageSchool::Weapon)
}

fn skye_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let range = 9.0 + ctx.talent_mod("BONUS_RANGE").unwrap_or(0.0);
    let dps = ctx.eval_var(
        "Ability__Skye__A",
        rank,
        ValueSpec::var_scaled(
            "DamagePerSecond",
            [140.0, 180.0, 220.0, 260.0, 340.0],
            1.8,
            1.2,
            0.0,
            0.0,
        ),
    ) * ctx.talent_dmg_mult(slot);
    let locked_id = ctx.room.entities[ctx.caster]
        .hero_kit
        .as_ref()
        .filter(|k| ctx.tick < k.lock_target_until)
        .map(|k| k.lock_target_id)
        .unwrap_or(0);
    let dur = ctx.eval_var(
        "Ability__Skye__A",
        rank,
        ValueSpec::var("Duration", [3.0; 5]),
    );
    let mult = if locked_id != 0 {
        ctx.eval_var(
            "Ability__Skye__A",
            rank,
            ValueSpec::var("LOCKED_TARGET_PERCENT", [1.25; 5]),
        )
    } else {
        1.0
    };
    let origin = ctx.caster_pos();
    let aim = ctx.resolve_target_pos(target, range);
    let (dx, dy) = unit_dir(origin, aim);
    let end = clamp_pos(Position {
        x: origin.x + dx * range,
        y: origin.y + dy * range,
    });
    let tick_dmg = (dps * mult) / 4.0;
    let piercing = ctx.talent_special() == TalentSpecial::SkyePiercingBarrage;
    for wave in 0..4_u64 {
        let speed_ticks = (16.0 / TICK_HZ as f32).max(0.5);
        ctx.spawn_effect(
            KitEffectKind::Projectile {
                ability_slot: slot.idx() as u8,
                rank,
                speed_per_tick: speed_ticks + 0.02 * wave as f32,
                radius: 1.3,
                damage: tick_dmg,
                school: DamageSchool::Crystal,
                piercing,
                returning: false,
                return_to_caster: false,
                repeat_mult: 0.75,
                splash_damage: 0.0,
                splash_radius: 0.0,
                stun_ticks: 0,
                root_ticks: 0,
                silence_ticks: 0,
                slow_pct: 0.2,
                slow_ticks: seconds_to_ticks(dur * 0.5),
                minion_mult: 0.7,
                resets_cd_on_hit: false,
                applies_mark: None,
                destroys_projectiles: false,
                hit_actors: Vec::new(),
            },
            origin,
            end,
        );
    }
    Ok(true)
}

fn skye_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let dest = ctx.resolve_target_pos(target, 10.0);
    move_toward_or_dash(ctx, dest);
    let lock_pos = ctx.room.entities[ctx.caster]
        .hero_kit
        .as_ref()
        .and_then(|k| (ctx.tick < k.lock_target_until).then_some(k.lock_target_id))
        .and_then(|id| ctx.room.index(id))
        .map(|j| ctx.room.entities[j].position)
        .unwrap_or(dest);
    let dmg = ctx.eval_var(
        "Ability__Skye__B",
        rank,
        ValueSpec::dmg(0, [90.0, 150.0, 210.0, 270.0, 330.0], 1.0, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let aoe_r = ctx.eval_var(
        "Ability__Skye__B",
        rank,
        ValueSpec::var("MissileAOERadius", [2.2; 5]),
    );
    for j in ctx.enemies_in_radius(lock_pos, aoe_r, false) {
        ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
    }
    let reset_pct = ctx.eval_var(
        "Ability__Skye__B",
        rank,
        ValueSpec::var("AbilityAreset", [0.4, 0.55, 0.7, 0.85, 1.0]),
    );
    ctx.reduce_slot_cd(AbilitySlot::A, 6.0 * reset_pct);
    if ctx.talent_special() == TalentSpecial::SkyeSuriGrantsSpeed {
        let ms = ctx.talent_mod("BONUS_MOVESPEED_STRENGTH").unwrap_or(1.5);
        let dur = ctx.talent_mod("BONUS_MOVESPEED_DURATION").unwrap_or(3.0);
        ctx.add_buff(ctx.caster, BuffStatKind::MoveSpeedFlat, ms, dur);
    }
    Ok(true)
}

fn skye_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let lock_bonus = ctx.eval_var(
        "Ability__Skye__C",
        rank,
        ValueSpec::var3("LockOnMaxDistanceBonus", [2.0, 3.0, 4.0]),
    );
    let center = ctx.resolve_target_pos(target, 10.0 + lock_bonus);
    let delay = ctx.eval_var("Ability__Skye__C", rank, ValueSpec::var3("DELAY", [1.3; 3]));
    let dur = ctx.eval_var(
        "Ability__Skye__C",
        rank,
        ValueSpec::var3("Duration", [2.0, 3.0, 4.0]),
    );
    let slow = ctx.eval_var(
        "Ability__Skye__C",
        rank,
        ValueSpec::var3("SlowMagnitude", [0.55, 0.6, 0.65]),
    );
    let stun = ctx.eval_var(
        "Ability__Skye__C",
        rank,
        ValueSpec::var3("STUN_DURATION", [0.5; 3]),
    );
    let dmg = ctx.eval_var(
        "Ability__Skye__C",
        rank,
        ValueSpec::dmg3(0, [250.0, 300.0, 350.0], 0.5, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);

    ctx.spawn_effect(
        KitEffectKind::DelayedBlast {
            ability_slot: slot.idx() as u8,
            release_tick: ctx.tick + seconds_to_ticks(delay),
            radius: 3.5,
            inner_radius: 0.0,
            damage: dmg,
            outer_mult: 1.0,
            school: DamageSchool::Crystal,
            stun_ticks: seconds_to_ticks(stun),
            root_ticks: 0,
            sleep_ticks: 0,
            sleep_unwaking: false,
            slow_pct: slow,
            slow_ticks: seconds_to_ticks(dur),
            reveal_ticks: 0,
            bonus_mark: None,
            minion_mult: 0.7,
            applies_mark: None,
        },
        center,
        center,
    );
    ctx.spawn_effect(
        KitEffectKind::GroundZone {
            ability_slot: slot.idx() as u8,
            zone_kind: 48,
            radius: 3.5,
            ticks_interval: TICK_HZ / 2,
            next_tick: ctx.tick + seconds_to_ticks(delay),
            end_tick: ctx.tick + seconds_to_ticks(delay + dur),
            tick_damage: dmg * 0.25,
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
            minion_mult: 0.7,
            applies_mark: None,
        },
        center,
        center,
    );
    Ok(true)
}

pub(crate) static SKYE: HeroKitSpec = HeroKitSpec {
    hero_id: "Skye",
    type_id: 265,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Perk__Skye",
        on_basic_attack: Some(skye_perk_attack),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Skye__A",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::constant(0.15),
        range: ValueSpec::constant(9.0),
        cooldown: ValueSpec::var("Cooldown", [6.0, 6.0, 6.0, 6.0, 5.0]),
        cost: ValueSpec::var("Energy Cost", [40.0, 50.0, 60.0, 70.0, 80.0]),
        custom_exec: Some(skye_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Skye__B",
        targeting: CastTargeting::GroundPoint,
        max_rank: 5,
        startup: ValueSpec::zero(),
        range: ValueSpec::constant(10.0),
        cooldown: ValueSpec::var("Cooldown", [16.0, 14.0, 12.0, 10.0, 6.0]),
        cost: ValueSpec::var("Energy Cost", [70.0; 5]),
        gate: CastGate::RequiresLockedTarget,
        custom_exec: Some(skye_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Skye__C",
        targeting: CastTargeting::GroundPoint,
        max_rank: 3,
        startup: ValueSpec::constant(0.2),
        range: ValueSpec::constant(12.0),
        cooldown: ValueSpec::var3("Cooldown", [30.0, 24.0, 18.0]),
        cost: ValueSpec::var3("Energy Cost", [70.0, 90.0, 110.0]),
        charges: ValueSpec::var3("CHARGES", [1.0; 3]),
        between_charges_cd: ValueSpec::constant(1.0),
        gate: CastGate::RequiresLockedTarget,
        custom_exec: Some(skye_c_cast),
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

pub(crate) static TAKA: HeroKitSpec = HeroKitSpec {
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

pub(crate) static TONY: HeroKitSpec = HeroKitSpec {
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

// ============================================================================
// 51. Varya (tid=413)
// ============================================================================

fn varya_perk_attack(
    ctx: &mut HeroKitContext<'_, '_>,
    target_idx: usize,
    _raw_dmg: f32,
    _is_crit: bool,
) -> (f32, DamageSchool) {
    let stationary = ctx.room.entities[ctx.caster]
        .hero_kit
        .as_ref()
        .is_some_and(|k| k.stationary_ticks >= 6);
    if !stationary {
        return (0.0, DamageSchool::Weapon);
    }
    let c_rank = ctx.room.entities[ctx.caster].ability_ranks[2];
    let chain_bonus = usize::from(c_rank);
    let cp = ctx.eval_inputs().cp;
    let tpos = ctx.room.entities[target_idx].position;
    let bounce_dmg = 20.0 + cp * 0.25;
    let targets: Vec<usize> = ctx
        .enemies_in_radius(tpos, 4.5, false)
        .into_iter()
        .filter(|&j| j != target_idx)
        .take(1 + chain_bonus)
        .collect();
    for j in targets {
        ctx.deal_damage(j, bounce_dmg, DamageSchool::Crystal, 0.0, false);
    }
    (20.0 + cp * 0.35, DamageSchool::Crystal)
}

fn varya_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let range = ctx.eval_var(
        "Ability__Varya__A",
        rank,
        ValueSpec::var("Range", [12.0, 12.0, 12.0, 12.0, 14.0]),
    ) + ctx.talent_mod("BONUS_RANGE").unwrap_or(0.0);
    let origin = ctx.caster_pos();
    let aim = ctx.resolve_target_pos(target, range);
    let (dx, dy) = unit_dir(origin, aim);
    let end = clamp_pos(Position {
        x: origin.x + dx * range,
        y: origin.y + dy * range,
    });
    let impact_dmg = ctx.eval_var(
        "Ability__Varya__AImpact",
        rank,
        ValueSpec::dmg(0, [70.0, 110.0, 150.0, 190.0, 230.0], 1.4, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let chain_dmg = ctx.eval_var(
        "Ability__Varya__AChain",
        rank,
        ValueSpec::dmg(0, [60.0, 95.0, 130.0, 165.0, 200.0], 1.3, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    let stun = if ctx.talent_special() == TalentSpecial::VaryaSpearStuns {
        ctx.talent_mod("STUN_DURATION").unwrap_or(0.12)
    } else {
        0.0
    };
    ctx.spawn_effect(
        KitEffectKind::Projectile {
            ability_slot: slot.idx() as u8,
            rank,
            speed_per_tick: 24.0 / TICK_HZ as f32,
            radius: 1.1,
            damage: impact_dmg,
            school: DamageSchool::Crystal,
            piercing: false,
            returning: false,
            return_to_caster: false,
            repeat_mult: 1.0,
            splash_damage: chain_dmg * 0.75,
            splash_radius: 4.5,
            stun_ticks: seconds_to_ticks(stun),
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
    Ok(true)
}

fn varya_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let is_recast = slot == AbilitySlot::B
        && ctx.room.entities[ctx.caster]
            .hero_kit
            .as_ref()
            .is_some_and(|k| k.step_stage[1] == 1 && ctx.tick < k.recast_until[1]);
    let range = ctx.eval_var(
        "Ability__Varya__B_Move",
        rank,
        ValueSpec::var("Range", [4.5, 4.5, 4.5, 4.5, 6.5]),
    );
    let dest = ctx.resolve_target_pos(target, range);
    move_toward_or_dash(ctx, dest);

    let search_r = ctx.eval_var(
        "Ability__Varya__B",
        rank,
        ValueSpec::var("TARGET_SEARCH_RADIUS", [7.3, 7.3, 7.3, 7.3, 8.3]),
    );
    let dmg = ctx.eval_var(
        "Ability__Varya__B",
        rank,
        ValueSpec::dmg(0, [40.0, 70.0, 100.0, 130.0, 190.0], 0.6, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(AbilitySlot::B);
    let hits: Vec<usize> = ctx
        .enemies_in_radius(dest, search_r, false)
        .into_iter()
        .take(3)
        .collect();
    for &j in &hits {
        ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
    }
    let bar_pct = ctx.eval_var(
        "Ability__Varya__B",
        rank,
        ValueSpec::var("BARRIER_PER_STRIKE", [0.03; 5]),
    ) * ctx.talent_barrier_mult(AbilitySlot::B);
    if !hits.is_empty() {
        let amt = ctx.room.entities[ctx.caster].max_hp * bar_pct * hits.len() as f32;
        ctx.grant_barrier(ctx.caster, amt, 2.0, None);
    }
    if is_recast {
        ctx.clear_recast(AbilitySlot::B);
    } else {
        let window = ctx.eval_var(
            "Ability__Varya__B",
            rank,
            ValueSpec::var("REDASH_WINDOW", [1.8; 5]),
        );
        ctx.open_recast(AbilitySlot::B, 1, 0, window);
    }
    Ok(true)
}

fn varya_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    _target: CastTarget,
) -> Result<bool, &'static str> {
    let strike_dmg = ctx.eval_var(
        "Ability__Varya__C",
        rank,
        ValueSpec::var_scaled(
            "PER_STRIKE_DAMAGE",
            [120.0, 180.0, 240.0, 240.0, 240.0],
            0.75,
            0.0,
            0.0,
            0.0,
        ),
    ) * ctx.talent_dmg_mult(slot);
    let rad = ctx.eval_var(
        "Ability__Varya__C",
        rank,
        ValueSpec::var3("PER_STRIKE_RADIUS", [1.6, 1.8, 2.0]),
    );
    let strikes = if ctx.talent_special() == TalentSpecial::VaryaExtraGlobalStrikes {
        3
    } else {
        2
    };
    let caster_team = ctx.caster_team();
    let targets: Vec<Position> = ctx
        .room
        .entities
        .iter()
        .filter(|e| e.kind == Kind::Hero && e.team != caster_team && e.hp > 0.0)
        .map(|e| e.position)
        .collect();
    for pos in targets {
        for s in 0..strikes {
            let delay = 0.8 + s as f32 * 0.7;
            let mult = if s == 0 { 1.0 } else { 0.5 };
            ctx.spawn_effect(
                KitEffectKind::DelayedBlast {
                    ability_slot: slot.idx() as u8,
                    release_tick: ctx.tick + seconds_to_ticks(delay),
                    radius: rad,
                    inner_radius: 0.0,
                    damage: strike_dmg * mult,
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
                    minion_mult: 0.5,
                    applies_mark: None,
                },
                pos,
                pos,
            );
        }
    }
    Ok(true)
}

static VARYA_VARIANTS: &[AbilityVariantSpec] = &[AbilityVariantSpec {
    slot: HeroSlot::B2,
    spec: AbilityKitSpec {
        ability_id: "Ability__Varya__B_Move",
        targeting: CastTargeting::GroundPoint,
        max_rank: 5,
        startup: ValueSpec::zero(),
        range: ValueSpec::var("Range", [4.5, 4.5, 4.5, 4.5, 6.5]),
        cooldown: ValueSpec::constant(1.0),
        cost: ValueSpec::zero(),
        custom_exec: Some(varya_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
}];

pub(crate) static VARYA: HeroKitSpec = HeroKitSpec {
    hero_id: "Varya",
    type_id: 413,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Perk__Varya",
        on_basic_attack: Some(varya_perk_attack),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Varya__A",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::constant(0.6),
        range: ValueSpec::var("Range", [12.0, 12.0, 12.0, 12.0, 14.0]),
        cooldown: ValueSpec::var("Cooldown", [7.0, 6.5, 6.0, 5.5, 4.5]),
        cost: ValueSpec::constant(60.0),
        custom_exec: Some(varya_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Varya__B",
        targeting: CastTargeting::GroundPoint,
        max_rank: 5,
        startup: ValueSpec::constant(0.25),
        range: ValueSpec::constant(6.5),
        cooldown: ValueSpec::var("Cooldown", [24.0, 23.0, 22.0, 21.0, 20.0]),
        cost: ValueSpec::constant(60.0),
        custom_exec: Some(varya_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Varya__C",
        targeting: CastTargeting::InstantSelf,
        max_rank: 3,
        startup: ValueSpec::constant(0.6),
        range: ValueSpec::constant(180.0),
        cooldown: ValueSpec::var3("Cooldown", [90.0, 75.0, 60.0]),
        cost: ValueSpec::constant(120.0),
        passive_bonus: Some((
            BuffStatKind::AttackSpeed,
            ValueSpec::var3("PASSIVE_ASPD_BONUS", [0.1, 0.2, 0.3]),
        )),
        custom_exec: Some(varya_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    variants: VARYA_VARIANTS,
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

pub(crate) static VIOLA: HeroKitSpec = HeroKitSpec {
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

pub(crate) static VOX: HeroKitSpec = HeroKitSpec {
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
// 54. Warhawk (tid=916)
// ============================================================================

fn warhawk_perk_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    _slot: AbilitySlot,
    _rank: u8,
    _target: CastTarget,
) {
    let inputs = ctx.eval_inputs();
    let bonus = 30.0 + inputs.wp * 0.35 + inputs.cp * 0.4;
    ctx.empower_next_attacks(1, bonus, false, 0.0, 0.0, 0.0, 0.0, 5.0);
    ctx.add_buff(ctx.caster, BuffStatKind::AttackRange, 3.0, 5.0);
}

fn warhawk_a_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let range = ctx.eval_var(
        "Ability__Warhawk__A",
        rank,
        ValueSpec::var("RANGE", [9.0; 5]),
    );
    let origin = ctx.caster_pos();
    let aim = ctx.resolve_target_pos(target, range);
    let (dx, dy) = unit_dir(origin, aim);
    let end = clamp_pos(Position {
        x: origin.x + dx * range,
        y: origin.y + dy * range,
    });
    let dmg = ctx.eval_var(
        "Warhawk_A_Damage",
        rank,
        ValueSpec::dmg(0, [95.0, 145.0, 195.0, 245.0, 345.0], 1.25, 0.8, 0.0),
    ) * ctx.talent_dmg_mult(slot);
    ctx.spawn_effect(
        KitEffectKind::Projectile {
            ability_slot: slot.idx() as u8,
            rank,
            speed_per_tick: 24.0 / TICK_HZ as f32,
            radius: 1.4,
            damage: dmg,
            school: DamageSchool::Weapon,
            piercing: false,
            returning: false,
            return_to_caster: false,
            repeat_mult: 1.0,
            splash_damage: dmg * 0.6,
            splash_radius: 2.5,
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
    let back = if ctx.talent_special() == TalentSpecial::WarhawkRocketMoreKnockback {
        6.0
    } else {
        4.0
    };
    let hop = clamp_pos(Position {
        x: origin.x - dx * back,
        y: origin.y - dy * back,
    });
    move_toward_or_dash(ctx, hop);
    Ok(true)
}

fn warhawk_b_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let is_detonate =
        slot == AbilitySlot::B && ctx.mark_stacks(ctx.caster, marks::WARHAWK_GRENADE_OUT) > 0;
    let dmg = ctx.eval_var(
        "Ability__Warhawk__B",
        rank,
        ValueSpec::dmg(0, [150.0, 225.0, 300.0, 375.0, 525.0], 0.4, 0.0, 0.0),
    ) * ctx.talent_dmg_mult(AbilitySlot::B);
    let stun = ctx.eval_var(
        "Ability__Warhawk__B",
        rank,
        ValueSpec::var("STUN_DURATION", [0.5, 0.5, 0.5, 0.5, 1.0]),
    );

    if is_detonate {
        ctx.consume_mark(ctx.caster, marks::WARHAWK_GRENADE_OUT);
        ctx.clear_recast(AbilitySlot::B);
        let owner_id = ctx.caster_id();
        let grenade = ctx
            .room
            .entities
            .ecs
            .query::<(bevy_ecs::entity::Entity, &KitEffect)>()
            .iter(&ctx.room.entities.ecs)
            .find_map(|(eh, eff)| {
                (eff.owner_id == owner_id
                    && matches!(eff.kind, KitEffectKind::GroundZone { zone_kind: 54, .. }))
                .then_some((eh, eff.position))
            });
        let center = if let Some((eh, pos)) = grenade {
            ctx.room.entities.remove_spell_handle::<KitEffect>(eh);
            pos
        } else {
            ctx.resolve_target_pos(target, 9.0)
        };
        for j in ctx.enemies_in_radius(center, 3.6, false) {
            ctx.deal_damage(j, dmg, DamageSchool::Crystal, 0.0, false);
            ctx.apply_stun(j, stun);
            ctx.knockback(j, center, 2.5, stun);
        }
        if ctx.talent_special() == TalentSpecial::WarhawkGrenadeHealsAllies {
            let heal = ctx.talent_mod("HEAL_AMOUNT").unwrap_or(250.0)
                * ctx.talent_heal_mult(AbilitySlot::B);
            for a in ctx.allies_in_radius(center, 3.6, true, true) {
                ctx.heal_unit(a, heal);
            }
        }
        return Ok(true);
    }

    let range = ctx.eval_var(
        "Ability__Warhawk__B",
        rank,
        ValueSpec::var("RANGE", [9.0; 5]),
    );
    let dur = ctx.eval_var(
        "Ability__Warhawk__B",
        rank,
        ValueSpec::var("DURATION", [5.0; 5]),
    );
    let center = ctx.resolve_target_pos(target, range);
    ctx.add_mark(ctx.caster, marks::WARHAWK_GRENADE_OUT, 1, 1, dur);
    ctx.open_recast(AbilitySlot::B, 1, 0, dur);
    ctx.spawn_effect(
        KitEffectKind::GroundZone {
            ability_slot: 1,
            zone_kind: 54,
            radius: 3.5,
            ticks_interval: TICK_HZ,
            next_tick: ctx.tick + seconds_to_ticks(dur),
            end_tick: ctx.tick + seconds_to_ticks(dur),
            tick_damage: 0.0,
            tick_heal: 0.0,
            school: DamageSchool::Crystal,
            slow_pct: 0.0,
            ally_speed_bonus: 0.0,
            end_damage: dmg,
            end_root_ticks: 0,
            end_stun_ticks: seconds_to_ticks(stun),
            drift_dx: 0.0,
            drift_dy: 0.0,
            follow_caster: false,
            ignited: false,
            minion_mult: 0.7,
            applies_mark: None,
        },
        center,
        center,
    );
    Ok(true)
}

fn warhawk_c_cast(
    ctx: &mut HeroKitContext<'_, '_>,
    slot: AbilitySlot,
    rank: u8,
    target: CastTarget,
) -> Result<bool, &'static str> {
    let range = ctx.eval_var(
        "Ability__Warhawk__C",
        rank,
        ValueSpec::var3("RANGE", [18.0; 3]),
    );
    let center = ctx.resolve_target_pos(target, range);
    let impact = ctx.eval_var(
        "Warhawk_C_Impact_Damage",
        rank,
        ValueSpec::dmg3(0, [150.0, 200.0, 250.0], 0.7, 2.0, 0.0),
    ) * ctx.talent_mod("IMPACT_DAMAGE_MODIFIER").unwrap_or(1.0);
    let burn = ctx.eval_var(
        "Warhawk_C_Burn_Damage",
        rank,
        ValueSpec::dmg3(1, [180.0, 240.0, 300.0], 1.0, 0.0, 0.0),
    ) * ctx.talent_mod("BURN_DAMAGE_MODIFIER").unwrap_or(1.0);
    let dur = ctx.eval_var(
        "Ability__Warhawk__C",
        rank,
        ValueSpec::var3("DURATION", [8.0; 3]),
    );
    let slow = ctx.eval_var(
        "Ability__Warhawk__C",
        rank,
        ValueSpec::var3("SLOW_STRENGTH", [0.25, 0.35, 0.45]),
    );
    let count = if ctx.talent_special() == TalentSpecial::WarhawkFiveBarrageMissiles {
        5
    } else {
        3
    };
    for idx in 0..count {
        let offset = (idx as f32 - (count - 1) as f32 * 0.5) * 2.2;
        let pos = clamp_pos(Position {
            x: center.x + offset,
            y: center.y,
        });
        for j in ctx.enemies_in_radius(pos, 2.8, false) {
            ctx.deal_damage(j, impact, DamageSchool::Weapon, 0.0, false);
        }
        ctx.spawn_effect(
            KitEffectKind::GroundZone {
                ability_slot: slot.idx() as u8,
                zone_kind: 55,
                radius: 2.8,
                ticks_interval: TICK_HZ / 2,
                next_tick: ctx.tick + TICK_HZ / 2,
                end_tick: ctx.tick + seconds_to_ticks(dur),
                tick_damage: (burn / dur) * 0.5,
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
                minion_mult: 0.67,
                applies_mark: None,
            },
            pos,
            pos,
        );
    }
    Ok(true)
}

static WARHAWK_VARIANTS: &[AbilityVariantSpec] = &[AbilityVariantSpec {
    slot: HeroSlot::B2,
    spec: AbilityKitSpec {
        ability_id: "Ability__Warhawk__B_Detonate",
        targeting: CastTargeting::InstantSelf,
        max_rank: 5,
        startup: ValueSpec::zero(),
        range: ValueSpec::constant(9.0),
        cooldown: ValueSpec::constant(3.0),
        cost: ValueSpec::zero(),
        custom_exec: Some(warhawk_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
}];

pub(crate) static WARHAWK: HeroKitSpec = HeroKitSpec {
    hero_id: "Warhawk",
    type_id: 916,
    resource: ResourceKind::Energy,
    perk: HeroicPerkSpec {
        id: "Perk__Warhawk",
        on_ability_cast: Some(warhawk_perk_cast),
        ..HeroicPerkSpec::DEFAULT
    },
    a: AbilityKitSpec {
        ability_id: "Ability__Warhawk__A",
        targeting: CastTargeting::Directional,
        max_rank: 5,
        startup: ValueSpec::zero(),
        range: ValueSpec::var("RANGE", [9.0; 5]),
        cooldown: ValueSpec::var("Cooldown", [12.0, 11.5, 11.0, 10.5, 9.0]),
        cost: ValueSpec::var("ENERGY COST", [30.0, 35.0, 40.0, 45.0, 50.0]),
        custom_exec: Some(warhawk_a_cast),
        ..AbilityKitSpec::DEFAULT
    },
    b: AbilityKitSpec {
        ability_id: "Ability__Warhawk__B",
        targeting: CastTargeting::GroundPoint,
        max_rank: 5,
        startup: ValueSpec::zero(),
        range: ValueSpec::var("RANGE", [9.0; 5]),
        cooldown: ValueSpec::var("Cooldown", [20.0, 19.0, 18.0, 17.0, 15.0]),
        cost: ValueSpec::var("ENERGY COST", [40.0, 45.0, 50.0, 55.0, 60.0]),
        custom_exec: Some(warhawk_b_cast),
        ..AbilityKitSpec::DEFAULT
    },
    c: AbilityKitSpec {
        ability_id: "Ability__Warhawk__C",
        targeting: CastTargeting::GroundPoint,
        max_rank: 3,
        startup: ValueSpec::constant(0.25),
        range: ValueSpec::var3("RANGE", [18.0; 3]),
        cooldown: ValueSpec::var3("Cooldown", [70.0, 65.0, 60.0]),
        cost: ValueSpec::var3("ENERGY COST", [100.0, 125.0, 150.0]),
        custom_exec: Some(warhawk_c_cast),
        ..AbilityKitSpec::DEFAULT
    },
    variants: WARHAWK_VARIANTS,
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

pub(crate) static YATES: HeroKitSpec = HeroKitSpec {
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

pub(crate) static YLVA: HeroKitSpec = HeroKitSpec {
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

pub(crate) static HERO057: HeroKitSpec = HeroKitSpec {
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

pub(crate) static HERO062: HeroKitSpec = HeroKitSpec {
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

pub(crate) static HERO999: HeroKitSpec = HeroKitSpec {
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

pub(crate) static LANCEBALL_LANCE: HeroKitSpec = HeroKitSpec {
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

// ============================================================================
// Registry export for all 31 Batch B heroes (`abilities.json` indices `30..61`)
// ============================================================================

static BATCH_B_SPECS: &[&HeroKitSpec] = &[
    &LORELAI,
    &LYRA,
    &MAGNUS,
    &MALENE,
    &MIHO,
    &OZO,
    &PETAL,
    &PHINN,
    &REIM,
    &REZA,
    &RINGO,
    &RONA,
    &SAMUEL,
    &SANFENG,
    &SAW,
    &SHIN,
    &SILVERNAIL,
    &SKAARF,
    &SKYE,
    &TAKA,
    &TONY,
    &VARYA,
    &VIOLA,
    &VOX,
    &WARHAWK,
    &YATES,
    &YLVA,
    &HERO057,
    &HERO062,
    &HERO999,
    &LANCEBALL_LANCE,
];

pub(crate) fn specs() -> &'static [&'static HeroKitSpec] {
    BATCH_B_SPECS
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::{Command, Event, Position, World};

    fn prepare_hero_matchup(spec: &HeroKitSpec, seed: u64) -> (World, u32, u32, u32) {
        let mut w = World::new(seed, 10);
        w.enable_builtin_hero_kits();

        let hero_idx = 0usize;
        let hero_team = w.entities[hero_idx].team;
        let ally_idx = w
            .entities
            .iter()
            .position(|e| {
                e.kind == Kind::Hero && e.team == hero_team && e.id != w.entities[hero_idx].id
            })
            .expect("ally hero");
        let enemy_idx = w
            .entities
            .iter()
            .position(|e| e.kind == Kind::Hero && e.team != hero_team)
            .expect("enemy hero");

        for (idx, pos) in [
            (hero_idx, Position { x: 24.0, y: 0.0 }),
            (ally_idx, Position { x: 25.5, y: 0.5 }),
            (enemy_idx, Position { x: 26.5, y: 0.0 }),
        ] {
            let e = &mut w.entities[idx];
            e.position = pos;
            e.hp = 4000.0;
            e.max_hp = 4000.0;
            e.energy = 2000.0;
            e.max_energy = 2000.0;
        }

        let hero = &mut w.entities[hero_idx];
        hero.type_id = spec.type_id;
        hero.level = 12;
        hero.skill_points = 12;
        hero.ability_ranks = [0; 3];
        hero.item_stats.crystal = 150.0;
        hero.item_stats.bonus_weapon = 120.0;
        hero.bonus_hp = 400.0;

        let hid = w.entities[hero_idx].id;
        let aid = w.entities[ally_idx].id;
        let eid = w.entities[enemy_idx].id;
        (w, hid, aid, eid)
    }

    fn pick_target(
        targeting: CastTargeting,
        hid: u32,
        enemy_id: u32,
        ally_id: u32,
        enemy_pos: Position,
    ) -> CastTarget {
        match targeting {
            CastTargeting::InstantSelf => CastTarget::SelfCast,
            CastTargeting::TargetEnemy | CastTargeting::TargetAnyHero => {
                CastTarget::Entity(enemy_id)
            }
            CastTargeting::TargetAlly => CastTarget::Entity(ally_id),
            CastTargeting::TargetAllyOrSelf => CastTarget::Entity(hid),
            CastTargeting::GroundPoint
            | CastTargeting::Directional
            | CastTargeting::TargetLocationOrEntity => CastTarget::Point(enemy_pos),
        }
    }

    fn step_ticks(w: &mut World, n: usize) -> Vec<Event> {
        let mut out = Vec::new();
        for _ in 0..n {
            out.extend(w.step());
        }
        out
    }

    #[test]
    fn batch_b_covers_exact_31_unique_heroes_and_non_empty_ids() {
        let list = specs();
        assert_eq!(list.len(), 31, "Batch B must register exact 31 heroes");
        let mut tids = BTreeSet::new();
        let mut names = BTreeSet::new();
        for spec in list {
            assert!(!spec.hero_id.is_empty());
            assert_ne!(spec.type_id, 0);
            assert!(!spec.a.ability_id.is_empty());
            assert!(!spec.b.ability_id.is_empty());
            assert!(!spec.c.ability_id.is_empty());
            assert!(
                tids.insert(spec.type_id),
                "duplicate type_id {}",
                spec.type_id
            );
            assert!(
                names.insert(spec.hero_id),
                "duplicate hero_id {}",
                spec.hero_id
            );
        }
    }

    #[test]
    fn batch_b_all_31_heroes_learn_cast_and_preserve_state_digest() {
        for (idx, spec) in specs().iter().enumerate() {
            let seed = 0xB47C_B000 + idx as u64;
            let run_once = || {
                let (mut w, hid, aid, eid) = prepare_hero_matchup(spec, seed);
                for slot in [AbilitySlot::A, AbilitySlot::B, AbilitySlot::C] {
                    w.command(hid, Command::Learn(slot))
                        .unwrap_or_else(|e| panic!("{} learn {:?}: {e}", spec.hero_id, slot));
                }

                // Satisfy hero-specific cast prerequisites (Skye Target Lock, Viola 3-note chord).
                if spec.hero_id == "Skye" {
                    let k = w.entities[0].hero_kit.get_or_insert_with(Box::default);
                    k.lock_target_id = eid;
                    k.lock_target_until = 500;
                } else if spec.hero_id == "Viola" {
                    let k = w.entities[0].hero_kit.get_or_insert_with(Box::default);
                    k.musical_notes = [1, 2, 2];
                    k.musical_note_count = 3;
                }

                let enemy_pos = w
                    .entities
                    .iter()
                    .find(|e| e.id == eid)
                    .expect("enemy entity")
                    .position;
                w.command(hid, Command::Attack(eid)).ok();
                step_ticks(&mut w, 6);

                for (slot, ab) in [
                    (AbilitySlot::A, spec.a),
                    (AbilitySlot::B, spec.b),
                    (AbilitySlot::C, spec.c),
                ] {
                    let target = pick_target(ab.targeting, hid, eid, aid, enemy_pos);
                    w.command(hid, Command::Cast { slot, target })
                        .unwrap_or_else(|e| panic!("{} cast {:?}: {e}", spec.hero_id, slot));
                    step_ticks(&mut w, 25);
                }
                w.state_digest()
            };

            let d1 = run_once();
            let d2 = run_once();
            assert_eq!(d1, d2, "digest diverged for {}", spec.hero_id);
        }
    }

    #[test]
    fn malene_six_slot_form_swap_switches_light_and_dark_kits() {
        let (mut w, hid, _aid, _eid) = prepare_hero_matchup(&MALENE, 0xA1E4_0001);
        assert!(MALENE.ult_from_level_one);
        for slot in [AbilitySlot::A, AbilitySlot::B, AbilitySlot::C] {
            w.command(hid, Command::Learn(slot)).unwrap();
        }
        assert_eq!(w.kit_form(hid), 0);
        w.command(
            hid,
            Command::Cast {
                slot: AbilitySlot::C,
                target: CastTarget::SelfCast,
            },
        )
        .unwrap();
        step_ticks(&mut w, 5);
        assert_eq!(w.kit_form(hid), 1);
        w.command(
            hid,
            Command::Cast {
                slot: AbilitySlot::A,
                target: CastTarget::Point(Position { x: 26.5, y: 0.0 }),
            },
        )
        .unwrap();
        step_ticks(&mut w, 10);
    }

    #[test]
    fn silvernail_tripwires_spawn_segment_barrier_and_c_stuns_behind_target() {
        let (mut w, hid, _aid, eid) = prepare_hero_matchup(&SILVERNAIL, 0x5117_0001);
        w.command(hid, Command::Learn(AbilitySlot::A)).unwrap();
        w.command(hid, Command::Learn(AbilitySlot::C)).unwrap();
        let enemy_idx = w
            .entities
            .iter()
            .position(|e| e.id == eid)
            .expect("enemy entity");
        let eid_pos = w.entities[enemy_idx].position;
        w.command(
            hid,
            Command::Cast {
                slot: AbilitySlot::A,
                target: CastTarget::Point(Position {
                    x: eid_pos.x + 2.0,
                    y: eid_pos.y - 2.0,
                }),
            },
        )
        .unwrap();
        step_ticks(&mut w, 15);
        w.command(
            hid,
            Command::Cast {
                slot: AbilitySlot::A,
                target: CastTarget::Point(Position {
                    x: eid_pos.x + 2.0,
                    y: eid_pos.y + 2.0,
                }),
            },
        )
        .unwrap();
        step_ticks(&mut w, 5);
        let has_barrier = w
            .entities
            .spells::<KitEffect>()
            .any(|eff| matches!(eff.kind, KitEffectKind::SegmentBarrier { .. }));
        assert!(has_barrier, "Silvernail A should spawn SegmentBarrier");

        let before_hp = w.entities[enemy_idx].hp;
        w.command(
            hid,
            Command::Cast {
                slot: AbilitySlot::C,
                target: CastTarget::Point(eid_pos),
            },
        )
        .unwrap();
        step_ticks(&mut w, 10);
        assert!(w.entities[enemy_idx].hp < before_hp);
    }

    #[test]
    fn viola_all_four_three_note_chords_execute_cleanly() {
        for notes in [[1, 1, 1], [1, 1, 2], [1, 2, 2], [2, 2, 2]] {
            let (mut w, hid, _aid, _eid) = prepare_hero_matchup(&VIOLA, 0xF101_0001);
            w.command(hid, Command::Learn(AbilitySlot::A)).unwrap();
            w.command(hid, Command::Learn(AbilitySlot::B)).unwrap();
            w.command(hid, Command::Learn(AbilitySlot::C)).unwrap();
            {
                let k = w.entities[0].hero_kit.get_or_insert_with(Box::default);
                k.musical_notes = notes;
                k.musical_note_count = 3;
            }
            w.command(
                hid,
                Command::Cast {
                    slot: AbilitySlot::C,
                    target: CastTarget::SelfCast,
                },
            )
            .unwrap();
            step_ticks(&mut w, 5);
            assert_eq!(
                w.entities[0]
                    .hero_kit
                    .as_ref()
                    .map_or(0, |k| k.musical_note_count),
                0
            );
        }
    }

    #[test]
    fn shin_four_heat_stacks_empower_ability_and_open_recast() {
        let (mut w, hid, _aid, eid) = prepare_hero_matchup(&SHIN, 0x5414_0001);
        w.command(hid, Command::Learn(AbilitySlot::A)).unwrap();
        {
            let mut ev = Vec::new();
            let mut ctx = HeroKitContext::new(&mut w, &mut ev, 0);
            ctx.add_mark(0, marks::HEAT, 4, 4, 10.0);
        }
        assert_eq!(w.kit_mark_stacks(hid, marks::HEAT), 4);
        let epos = w
            .entities
            .iter()
            .find(|e| e.id == eid)
            .expect("enemy entity")
            .position;
        w.command(
            hid,
            Command::Cast {
                slot: AbilitySlot::A,
                target: CastTarget::Point(epos),
            },
        )
        .unwrap();
        step_ticks(&mut w, 5);
        assert_eq!(w.kit_mark_stacks(hid, marks::HEAT), 0);
        assert_eq!(
            w.entities[0]
                .hero_kit
                .as_ref()
                .map_or(0, |k| k.step_stage[0]),
            1
        );
    }

    #[test]
    fn hero057_attaches_to_ally_and_leaps_off_with_stun() {
        let (mut w, hid, aid, eid) = prepare_hero_matchup(&HERO057, 0x4570_0001);
        w.command(hid, Command::Learn(AbilitySlot::B)).unwrap();
        w.command(
            hid,
            Command::Cast {
                slot: AbilitySlot::B,
                target: CastTarget::Entity(aid),
            },
        )
        .unwrap();
        step_ticks(&mut w, 4);
        assert!(
            w.entities
                .spells::<KitEffect>()
                .any(|eff| matches!(eff.kind, KitEffectKind::Attachment { .. }))
        );
        let epos = w
            .entities
            .iter()
            .find(|e| e.id == eid)
            .expect("enemy entity")
            .position;
        w.command(
            hid,
            Command::Cast {
                slot: AbilitySlot::B,
                target: CastTarget::Point(epos),
            },
        )
        .unwrap();
        step_ticks(&mut w, 6);
        assert!(
            !w.entities
                .spells::<KitEffect>()
                .any(|eff| matches!(eff.kind, KitEffectKind::Attachment { .. }))
        );
    }
}
