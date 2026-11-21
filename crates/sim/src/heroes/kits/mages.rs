//! Mage hero kit specifications (11 heroes).

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
        .query::<(bevy_ecs::entity::Entity, &crate::heroes::engine::KitEffect)>()
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
            .remove_spell_handle::<crate::heroes::engine::KitEffect>(eh);
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

pub(super) static CELESTE: HeroKitSpec = HeroKitSpec {
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

pub(super) static ISHTAR: HeroKitSpec = HeroKitSpec {
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

pub(super) static KARAS: HeroKitSpec = HeroKitSpec {
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

pub(super) static MAGNUS: HeroKitSpec = HeroKitSpec {
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

pub(super) static MALENE: HeroKitSpec = HeroKitSpec {
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

pub(super) static PETAL: HeroKitSpec = HeroKitSpec {
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

pub(super) static SAMUEL: HeroKitSpec = HeroKitSpec {
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

pub(super) static SKAARF: HeroKitSpec = HeroKitSpec {
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

pub(super) static SKYE: HeroKitSpec = HeroKitSpec {
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

pub(super) static VARYA: HeroKitSpec = HeroKitSpec {
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

pub(super) static WARHAWK: HeroKitSpec = HeroKitSpec {
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
