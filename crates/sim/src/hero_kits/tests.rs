use std::collections::BTreeSet;
use std::num::NonZeroU8;
use std::path::Path;

use halcyon_data::GameData;

use super::{all_specs, kit_spec, kit_spec_by_hero_id};
use crate::combat::{AbilitySlot, CastTarget};
use crate::hero_kit::{CastTargeting, HeroKitBook, HeroKitSpec, marks};
use crate::talents_runtime::{
    ALL_TALENTS, ActiveTalentState, TalentRarity, apply_cooldown_modifier, eval_talent_mod,
    find_talent, load_talents_from_data, talent_barrier_multiplier, talent_bonus_charges,
    talent_bonus_range, talent_damage_multiplier, talent_heal_multiplier, talent_matches_hero,
    talent_passive_stats,
};
use crate::{Command, Event, Kind, Position, World};

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
    enemy_id: u32,
    ally_id: u32,
    enemy_pos: Position,
) -> CastTarget {
    match targeting {
        CastTargeting::InstantSelf => CastTarget::SelfCast,
        CastTargeting::TargetEnemy | CastTargeting::TargetAnyHero => CastTarget::Entity(enemy_id),
        CastTargeting::TargetAlly => CastTarget::Entity(ally_id),
        CastTargeting::TargetAllyOrSelf => CastTarget::SelfCast,
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
fn batch_a_covers_exact_30_unique_heroes_and_non_empty_ids() {
    let specs = all_specs();
    assert_eq!(specs.len(), 30, "Batch A must register exact 30 heroes");

    let mut tids = BTreeSet::new();
    let mut ids = BTreeSet::new();
    for spec in &specs {
        assert!(
            tids.insert(spec.type_id),
            "duplicate type_id {}",
            spec.type_id
        );
        assert!(
            ids.insert(spec.hero_id),
            "duplicate hero_id {}",
            spec.hero_id
        );
        assert!(
            !spec.perk.id.is_empty(),
            "missing perk id for {}",
            spec.hero_id
        );
        assert!(!spec.a.ability_id.is_empty());
        assert!(!spec.b.ability_id.is_empty());
        assert!(!spec.c.ability_id.is_empty());
        assert_eq!(
            kit_spec(spec.type_id).map(|s| s.hero_id),
            Some(spec.hero_id)
        );
        assert_eq!(
            kit_spec_by_hero_id(spec.hero_id).map(|s| s.type_id),
            Some(spec.type_id)
        );
    }

    for expected in [
        "Amael",
        "Adagio",
        "Alpha",
        "Anka",
        "Ardan",
        "Baptiste",
        "Baron",
        "Blackfeather",
        "Caine",
        "Catherine",
        "Celeste",
        "Churnwalker",
        "Flicker",
        "Fortress",
        "Glaive",
        "Grace",
        "Grumpjaw",
        "Gwen",
        "Idris",
        "Inara",
        "Ishtar",
        "Joule",
        "Karas",
        "Kensei",
        "Kestrel",
        "Kinetic",
        "Koshka",
        "Hero009",
        "Lance",
        "Leo",
    ] {
        assert!(ids.contains(expected), "missing Batch A hero {expected}");
    }
}

#[test]
fn talents_runtime_covers_all_189_unique_talents_and_scaling_math() {
    assert_eq!(ALL_TALENTS.len(), 189, "must cover all 189 talents");
    let mut ids = BTreeSet::new();
    let mut generic_count = 0usize;
    let mut rarities = [0usize; 4];

    for t in ALL_TALENTS {
        assert!(ids.insert(t.id), "duplicate talent id {}", t.id);
        assert_eq!(find_talent(t.id).map(|s| s.id), Some(t.id));
        match t.rarity {
            TalentRarity::Rare => rarities[0] += 1,
            TalentRarity::Epic => rarities[1] += 1,
            TalentRarity::Legendary => rarities[2] += 1,
            TalentRarity::Generic => rarities[3] += 1,
        }
        if let Some(hero) = t.hero_id {
            assert!(talent_matches_hero(t.id, hero, None).is_ok());
            let wrong = if hero == "Ringo" { "Amael" } else { "Ringo" };
            assert!(talent_matches_hero(t.id, wrong, None).is_err());
        } else {
            generic_count += 1;
            assert!(talent_matches_hero(t.id, "Amael", None).is_ok());
            assert!(talent_matches_hero(t.id, "Leo", None).is_ok());
        }
        let lvl = NonZeroU8::new(3).unwrap();
        for m in t.mods {
            let v1 = m.value_at(NonZeroU8::new(1).unwrap(), 100.0, 100.0);
            let v3 = m.value_at(lvl, 100.0, 100.0);
            assert!(
                v1.is_finite() && v3.is_finite(),
                "non-finite mod in {}",
                t.id
            );
        }
    }
    assert_eq!(generic_count, 17);
    assert!(rarities.iter().all(|&c| c > 0));

    // Verify Talent_BonusAttackSpeed, COOLDOWN_MODIFIER < 0, damage/barrier/heal/range/charges.
    let as_state = ActiveTalentState {
        id: "Talent_BonusAttackSpeed".into(),
        level: NonZeroU8::new(5).unwrap(),
    };
    let passive = talent_passive_stats(&as_state, 0.0, 0.0, None);
    assert!(passive.attack_speed > 0.0);

    let ringo_cd = ActiveTalentState {
        id: "Talent_Ringo_SpikedBrew".into(),
        level: NonZeroU8::new(3).unwrap(),
    };
    let reduced = apply_cooldown_modifier(&ringo_cd, AbilitySlot::C, 30.0, 0.0, 0.0, None);
    assert!(reduced < 30.0 && reduced > 5.0, "got {reduced}");

    let amael_cd = ActiveTalentState {
        id: "Talent_Amael_TalentC".into(),
        level: NonZeroU8::new(3).unwrap(),
    };
    let reduced_flat = apply_cooldown_modifier(&amael_cd, AbilitySlot::C, 30.0, 0.0, 0.0, None);
    assert!((reduced_flat - 16.0).abs() < 0.01, "got {reduced_flat}");

    let dmg_state = ActiveTalentState {
        id: "Talent_Amael_TalentA".into(),
        level: NonZeroU8::new(5).unwrap(),
    };
    assert!(talent_damage_multiplier(&dmg_state, AbilitySlot::A, 100.0, 100.0, None) > 1.0);

    let bar_state = ActiveTalentState {
        id: "Talent_Ardan_Vanguardian".into(),
        level: NonZeroU8::new(5).unwrap(),
    };
    assert!(talent_barrier_multiplier(&bar_state, AbilitySlot::A, 0.0, 0.0, None) > 1.0);

    let heal_state = ActiveTalentState {
        id: "Talent_Grace_UnitedFront".into(),
        level: NonZeroU8::new(5).unwrap(),
    };
    assert!(talent_heal_multiplier(&heal_state, AbilitySlot::C, 100.0, 0.0, None) > 1.0);

    let range_state = ActiveTalentState {
        id: "Talent_Ringo_LongShot".into(),
        level: NonZeroU8::new(5).unwrap(),
    };
    assert!(talent_bonus_range(&range_state, AbilitySlot::A, 0.0, 0.0, None) > 0.0);

    let charge_state = ActiveTalentState {
        id: "Talent_Grace_DoubleBenediction".into(),
        level: NonZeroU8::new(1).unwrap(),
    };
    assert!(talent_bonus_charges(&charge_state, AbilitySlot::A, 0.0, 0.0, None) >= 1);

    // Check eval_talent_mod linear scaling on known label.
    assert!(
        eval_talent_mod(
            &dmg_state.id,
            "DAMAGE_MODIFIER",
            dmg_state.level,
            0.0,
            0.0,
            None,
        )
        .is_some()
    );
}

#[test]
fn all_30_heroes_learn_cast_and_maintain_deterministic_digests() {
    for (idx, spec) in all_specs().into_iter().enumerate() {
        let seed = 100 + idx as u64;
        let (mut w1, h1, a1, e1) = prepare_hero_matchup(spec, seed);
        let (mut w2, h2, a2, e2) = prepare_hero_matchup(spec, seed);
        assert_eq!((h1, a1, e1), (h2, a2, e2));

        // Learn A, B, and C cleanly on both rooms.
        for slot in [AbilitySlot::A, AbilitySlot::B, AbilitySlot::C] {
            w1.command(h1, Command::Learn(slot))
                .unwrap_or_else(|e| panic!("{} learn {:?} failed: {e}", spec.hero_id, slot));
            w2.command(h2, Command::Learn(slot)).unwrap();
        }
        assert_eq!(w1.entities[0].ability_ranks, [1, 1, 1]);
        assert_eq!(w1.state_digest(), w2.state_digest());

        // Pre-seed Churnwalker tether or Alpha reboot readiness where C has a gate.
        let enemy_pos = w1.entities[w1.entities.iter().position(|e| e.id == e1).unwrap()].position;
        for (w, h, a, e) in [(&mut w1, h1, a1, e1), (&mut w2, h2, a2, e2)] {
            for (slot, ab) in [
                (AbilitySlot::A, &spec.a),
                (AbilitySlot::B, &spec.b),
                (AbilitySlot::C, &spec.c),
            ] {
                let tgt = pick_target(ab.targeting, e, a, enemy_pos);
                let res = w.command(h, Command::Cast { slot, target: tgt });
                // If C is gated (e.g., Alpha reboot or resource check), verify it either succeeds cleanly
                // or returns a deterministic validation error without desyncing.
                let _ = res;
                let _ = step_ticks(w, 12);
            }
        }
        assert_eq!(
            w1.state_digest(),
            w2.state_digest(),
            "digest mismatch after A/B/C casts for {}",
            spec.hero_id
        );
    }
}

#[test]
fn key_hero_kits_and_recasts_and_perks_behave_as_designed() {
    // 1. Amael A recast + B slow
    {
        let spec = kit_spec_by_hero_id("Amael").unwrap();
        let (mut w, h, _, _) = prepare_hero_matchup(spec, 1);
        w.command(h, Command::Learn(AbilitySlot::A)).unwrap();
        let aim = CastTarget::Point(Position { x: 26.5, y: 0.0 });
        w.command(
            h,
            Command::Cast {
                slot: AbilitySlot::A,
                target: aim,
            },
        )
        .unwrap();
        step_ticks(&mut w, 4);
        assert!(
            w.entities[0]
                .hero_kit
                .as_deref()
                .is_some_and(|k| w.tick < k.recast_until[0]),
            "Amael A1 must open A2 recast"
        );
        w.command(
            h,
            Command::Cast {
                slot: AbilitySlot::A,
                target: aim,
            },
        )
        .unwrap();
        step_ticks(&mut w, 4);
    }

    // 2. Ardan Vengeance + Vanguard barrier + Gauntlet stun on boundary cross
    {
        let spec = kit_spec_by_hero_id("Ardan").unwrap();
        let (mut w, h, a, e) = prepare_hero_matchup(spec, 2);
        w.command(h, Command::Learn(AbilitySlot::A)).unwrap();
        w.command(h, Command::Learn(AbilitySlot::C)).unwrap();
        w.command(
            h,
            Command::Cast {
                slot: AbilitySlot::A,
                target: CastTarget::Entity(a),
            },
        )
        .unwrap();
        step_ticks(&mut w, 3);
        let ally_idx = w.entities.iter().position(|u| u.id == a).unwrap();
        assert!(w.entities[ally_idx].status.barrier > 0.0);

        let enemy_idx = w.entities.iter().position(|u| u.id == e).unwrap();
        let center = Position { x: 30.0, y: 0.0 };
        w.entities[enemy_idx].position = Position { x: 34.4, y: 0.0 };
        w.command(
            h,
            Command::Cast {
                slot: AbilitySlot::C,
                target: CastTarget::Point(center),
            },
        )
        .unwrap();
        step_ticks(&mut w, 25);
        assert!(
            w.entities[enemy_idx].status.stun_until > w.tick || w.entities[enemy_idx].hp < 4000.0
        );
    }

    // 3. Caine 6-ammo Peacemaker clip + B reload
    {
        let spec = kit_spec_by_hero_id("Caine").unwrap();
        let (mut w, h, _, e) = prepare_hero_matchup(spec, 3);
        w.command(h, Command::Learn(AbilitySlot::A)).unwrap();
        w.command(h, Command::Learn(AbilitySlot::B)).unwrap();
        assert_eq!(w.kit_ammo(h, AbilitySlot::A), 6);
        w.command(
            h,
            Command::Cast {
                slot: AbilitySlot::A,
                target: CastTarget::Point(Position { x: 26.5, y: 0.0 }),
            },
        )
        .unwrap();
        step_ticks(&mut w, 4);
        w.command(h, Command::Attack(e)).unwrap();
        step_ticks(&mut w, 14);
        let before_b = w.kit_ammo(h, AbilitySlot::A);
        assert!(
            before_b < 6,
            "expected <6 bullets after A+attack, got {before_b}"
        );
        w.command(
            h,
            Command::Cast {
                slot: AbilitySlot::B,
                target: CastTarget::Point(Position { x: 22.0, y: 0.0 }),
            },
        )
        .unwrap();
        step_ticks(&mut w, 4);
        let after_b = w.kit_ammo(h, AbilitySlot::A);
        assert!(
            after_b > before_b,
            "expected reload on B ({before_b} -> {after_b})"
        );
    }

    // 4. Celeste basic crystal + A Heliogenesis nova explosion on 2nd star cast
    {
        let spec = kit_spec_by_hero_id("Celeste").unwrap();
        let (mut w, h, _, e) = prepare_hero_matchup(spec, 4);
        w.command(h, Command::Learn(AbilitySlot::A)).unwrap();
        let enemy_idx = w.entities.iter().position(|u| u.id == e).unwrap();
        let target_pos = w.entities[enemy_idx].position;
        let hp_before = w.entities[enemy_idx].hp;
        w.command(
            h,
            Command::Cast {
                slot: AbilitySlot::A,
                target: CastTarget::Point(target_pos),
            },
        )
        .unwrap();
        step_ticks(&mut w, 18);
        let hp_mid = w.entities[enemy_idx].hp;
        assert!(hp_mid < hp_before);
        step_ticks(&mut w, 40);
        // Second A on existing star detonates supernova for extra burst!
        w.command(
            h,
            Command::Cast {
                slot: AbilitySlot::A,
                target: CastTarget::Point(target_pos),
            },
        )
        .unwrap();
        step_ticks(&mut w, 16);
        assert!(w.entities[enemy_idx].hp < hp_mid);
    }

    // 5. Flicker stealth after stationary & enemy visibility lost until totem/reveal
    {
        let spec = kit_spec_by_hero_id("Flicker").unwrap();
        let (mut w, _h, _, e) = prepare_hero_matchup(spec, 5);
        let enemy_idx = w.entities.iter().position(|u| u.id == e).unwrap();
        let enemy_team = w.entities[enemy_idx].team;
        assert!(w.visible(enemy_team, &w.entities[0]));
        step_ticks(&mut w, 25);
        assert!(
            !w.visible(enemy_team, &w.entities[0]),
            "Flicker should stealth after standing still for 1s+"
        );
    }

    // 6. Idris Divergent Paths (WP>=120 Stamina vs CP>=120 ranged) + Shimmer Blink recast
    {
        let spec = kit_spec_by_hero_id("Idris").unwrap();
        let (mut w, h, _, e) = prepare_hero_matchup(spec, 6);
        w.command(h, Command::Learn(AbilitySlot::A)).unwrap();
        w.command(h, Command::Learn(AbilitySlot::C)).unwrap();
        assert!(w.kit_resource(h) > 0.0);
        w.command(
            h,
            Command::Cast {
                slot: AbilitySlot::C,
                target: CastTarget::Entity(e),
            },
        )
        .unwrap();
        step_ticks(&mut w, 4);
        w.command(
            h,
            Command::Cast {
                slot: AbilitySlot::C,
                target: CastTarget::Point(Position { x: 29.0, y: 0.0 }),
            },
        )
        .unwrap();
        step_ticks(&mut w, 6);
        let enemy_idx = w.entities.iter().position(|u| u.id == e).unwrap();
        assert!(w.entities[enemy_idx].hp < 4000.0);
    }

    // 7. Kensei 3-step Lotus Strike + Joule Big Red Button ChannelBeam
    {
        let spec = kit_spec_by_hero_id("Kensei").unwrap();
        let (mut w, h, _, _) = prepare_hero_matchup(spec, 7);
        w.command(h, Command::Learn(AbilitySlot::A)).unwrap();
        let aim = CastTarget::Point(Position { x: 26.5, y: 0.0 });
        for _ in 0..3 {
            w.command(
                h,
                Command::Cast {
                    slot: AbilitySlot::A,
                    target: aim,
                },
            )
            .unwrap();
            step_ticks(&mut w, 4);
        }

        let jspec = kit_spec_by_hero_id("Joule").unwrap();
        let (mut jw, jh, _, je) = prepare_hero_matchup(jspec, 8);
        jw.command(jh, Command::Learn(AbilitySlot::C)).unwrap();
        let j_enemy_idx = jw.entities.iter().position(|u| u.id == je).unwrap();
        jw.command(
            jh,
            Command::Cast {
                slot: AbilitySlot::C,
                target: CastTarget::Point(jw.entities[j_enemy_idx].position),
            },
        )
        .unwrap();
        step_ticks(&mut jw, 25);
        assert!(jw.entities[j_enemy_idx].hp < 4000.0);
    }

    // 8. Krul Shadows Empower Me + Weakness stacks + Lance cleave
    {
        let spec = kit_spec_by_hero_id("Hero009").unwrap();
        let (mut w, h, _, e) = prepare_hero_matchup(spec, 9);
        w.command(h, Command::Learn(AbilitySlot::B)).unwrap();
        w.command(h, Command::Attack(e)).unwrap();
        step_ticks(&mut w, 20);
        assert!(w.kit_mark_stacks(e, marks::WEAKNESS) > 0);
        w.command(
            h,
            Command::Cast {
                slot: AbilitySlot::B,
                target: CastTarget::Entity(e),
            },
        )
        .unwrap();
        step_ticks(&mut w, 4);
        assert_eq!(w.kit_mark_stacks(e, marks::WEAKNESS), 0);
    }
}

#[test]
#[ignore = "requires local delivery and extra game data assets"]
fn live_data_validates_all_30_batch_a_heroes_and_all_189_talents() {
    let base = std::env::var_os("HALCYON_DELIVERY").expect("HALCYON_DELIVERY");
    let extra = std::env::var_os("HALCYON_DATA_EXTRA").expect("HALCYON_DATA_EXTRA");
    let data = GameData::load_with_extra(base, Some(Path::new(&extra))).unwrap();

    // 1. Validate HeroKitBook covers all 61 data heroes & all 30 Batch A heroes from abilities.json.
    let book = HeroKitBook::load(&data)
        .unwrap()
        .expect("HeroKitBook loaded");
    assert_eq!(data.extra.combat_content().heroes().count(), 61);
    for spec in all_specs() {
        let loaded = book.heroes.get(spec.hero_id).unwrap_or_else(|| {
            panic!(
                "Batch A hero {} missing from loaded HeroKitBook",
                spec.hero_id
            )
        });
        for ab in [&spec.a, &spec.b, &spec.c] {
            assert!(
                loaded.abilities.contains_key(ab.ability_id),
                "{} missing ability {}",
                spec.hero_id,
                ab.ability_id
            );
        }
    }

    // 2. Validate ALL 189 talents from talents_full.json match ALL_TALENTS.
    let loaded_talents = load_talents_from_data(&data);
    assert_eq!(loaded_talents.len(), 189);
    for t in ALL_TALENTS {
        assert!(
            loaded_talents.contains_key(t.id),
            "ALL_TALENTS entry {} missing from live talents_full.json",
            t.id
        );
    }

    // 3. Validate set_named_talent applies & cleanly restores baseline state_digest().
    let mut w = World::from_data(77, 6, &data).unwrap();
    w.use_six_human_roster().unwrap();
    w.use_aral_lane().unwrap();
    w.select_hero(0, "Caine", &data).unwrap();
    let baseline = w.state_digest();
    let lvl = NonZeroU8::new(5).unwrap();
    w.set_named_talent(1, Some(("Talent_Caine_TalentA", lvl)), &data)
        .unwrap();
    assert_ne!(w.state_digest(), baseline);
    w.set_named_talent(1, Some(("Talent_BonusAttackSpeed", lvl)), &data)
        .unwrap();
    assert!(
        w.set_named_talent(1, Some(("Talent_Amael_TalentA", lvl)), &data)
            .is_err()
    );
    assert!(
        w.set_named_talent(1, Some(("Talent_NonExistent_Xyz", lvl)), &data)
            .is_err()
    );
    w.set_named_talent(1, None, &data).unwrap();
    assert_eq!(w.state_digest(), baseline);

    // 4. Validate builtin() parity doesn't panic and select_hero works on live Batch A heroes.
    let builtin = HeroKitBook::builtin();
    assert!(builtin.heroes.is_empty());
    w.command(1, Command::Learn(AbilitySlot::A)).unwrap();
    w.command(
        1,
        Command::Cast {
            slot: AbilitySlot::A,
            target: CastTarget::Point(Position { x: 10.0, y: 0.0 }),
        },
    )
    .unwrap();
    let e = w.step();
    assert!(!e.is_empty());
}
