//! Comprehensive deterministic tests covering all T2/T3 active & passive items and consumables.
use super::*;
use crate::item_actives::{HEALING_FLASK_COOLDOWN_KEY, STORMGUARD_COOLDOWN_KEY};
use crate::vision_devices::VisionDeviceKind;
use crate::{
    AbilitySlot, BasicItem, CastTarget, Command, Event, ItemTarget, Kind, Position, TICK_HZ, World,
};
use halcyon_data::GameData;
use halcyon_data::items::StoreSettings;

fn sim_room(seed: u64) -> World {
    let mut w = World::new(seed, 6);
    w.use_six_human_roster().ok();
    for idx in 0..w.entities.len() {
        let e = &mut w.entities[idx];
        if e.kind == Kind::Hero {
            e.gold = 50_000;
            e.max_hp = 2000.;
            e.hp = 2000.;
            e.max_energy = 1000.;
            e.energy = 1000.;
            e.stats.armor = 0.;
            e.stats.shield = 0.;
        }
    }
    w
}

fn enable_all_store(w: &mut World) {
    w.shop.enable_all_store_for_test();
}

fn aral_room(seed: u64) -> World {
    let mut w = World::new(seed, 6);
    w.use_six_human_roster().unwrap();
    w.use_aral_lane().unwrap();
    enable_all_store(&mut w);
    for idx in 0..w.entities.len() {
        let e = &mut w.entities[idx];
        if e.kind == Kind::Hero {
            e.gold = 50_000;
        }
    }
    w
}

fn park_away_other_heroes(w: &mut World, active_ids: &[u32]) {
    for idx in 0..w.entities.len() {
        let e = &mut w.entities[idx];
        if e.kind == Kind::Hero && !active_ids.contains(&e.id) {
            e.position = Position { x: 950., y: 950. };
        }
    }
}

fn step_ticks(w: &mut World, ticks: u64) -> Vec<Event> {
    let mut out = Vec::new();
    for _ in 0..ticks {
        out.extend(w.step());
    }
    out
}

#[test]
fn fountain_of_renewal_team_missing_hp_hot_capacitor_amp_and_range() {
    let mut w = sim_room(1);
    enable_all_store(&mut w);
    park_away_other_heroes(&mut w, &[1, 3, 5]);
    // Put heroes 1, 3, 5 (all Team 0 -> even indices 0, 2, 4 in Validation)
    let caster_pos = w.entities[0].position;
    w.entities[2].position = Position {
        x: caster_pos.x + 6.,
        y: caster_pos.y,
    };
    w.entities[4].position = Position {
        x: caster_pos.x + 25.,
        y: caster_pos.y,
    };

    let fountain = w.purchase(1, BasicItem::FountainOfRenewal).unwrap().granted;
    w.purchase(1, BasicItem::CapacitorPlate).unwrap();

    // Missing 50% HP on caster (id=1, max 2400+450? update based on current max_hp).
    let max0 = w.entities[0].max_hp;
    let max2 = w.entities[2].max_hp;
    let max4 = w.entities[4].max_hp;
    w.entities[0].hp = max0 * 0.5;
    w.entities[2].hp = max2 * 0.5;
    w.entities[4].hp = max4 * 0.5;
    // Mark in-combat so Lifespring does not mask Fountain HoT.
    w.entities[0].rt_mut().last_hero_combat_tick = Some(w.tick);
    w.entities[2].rt_mut().last_hero_combat_tick = Some(w.tick);
    w.entities[4].rt_mut().last_hero_combat_tick = Some(w.tick);

    w.command(
        1,
        Command::ActivateItem {
            instance: fountain.instance,
            target: ItemTarget::SelfCast,
        },
    )
    .unwrap();

    // Capacitor Plate should grant ally #3 (+1.5 MS for 3.0s) immediately on heal/shield application.
    assert!(
        w.entities[2]
            .rt()
            .is_some_and(|rt| rt.capacitor_ms_bonus == 1.5)
    );

    // Cooldown prevents second activation right away.
    assert!(
        w.command(
            1,
            Command::ActivateItem {
                instance: fountain.instance,
                target: ItemTarget::SelfCast,
            },
        )
        .is_err()
    );

    let events = step_ticks(&mut w, 75);
    assert!(w.entities[0].hp > max0 * 0.65);
    assert!(w.entities[2].hp > max2 * 0.65);
    // Hero 5 was out of 12m range and stayed at 50%.
    assert!((w.entities[4].hp - max4 * 0.5).abs() < 1e-3);
    assert!(events.iter().any(|e| matches!(
        e,
        Event::Heal {
            source: 1,
            target: 3,
            ..
        }
    )));
}

#[test]
fn crucible_and_aegis_barriers_cc_immunity_and_distinct_cooldowns() {
    let mut w = sim_room(2);
    enable_all_store(&mut w);
    park_away_other_heroes(&mut w, &[1, 3, 5]);
    let p0 = w.entities[0].position;
    w.entities[2].position = Position {
        x: p0.x + 8.,
        y: p0.y,
    };
    w.entities[4].position = Position {
        x: p0.x + 20.,
        y: p0.y,
    };

    let crucible = w.purchase(1, BasicItem::Crucible).unwrap().granted;
    w.command(
        1,
        Command::ActivateItem {
            instance: crucible.instance,
            target: ItemTarget::SelfCast,
        },
    )
    .unwrap();
    assert!(w.entities[0].status.blocks_debuffs(w.tick));
    assert!(w.entities[2].status.blocks_debuffs(w.tick));
    assert!(!w.entities[4].status.blocks_debuffs(w.tick));
    assert!(
        w.command(
            1,
            Command::ActivateItem {
                instance: crucible.instance,
                target: ItemTarget::SelfCast,
            },
        )
        .is_err()
    );

    // Clear reflex and test Aegis 45s cooldown vs ReflexBlock 90s cooldown.
    w.entities[0].status.reflex = None;
    let aegis = w.purchase(1, BasicItem::Aegis).unwrap().granted;
    w.command(
        1,
        Command::ActivateItem {
            instance: aegis.instance,
            target: ItemTarget::SelfCast,
        },
    )
    .unwrap();
    assert!(w.entities[0].status.blocks_debuffs(w.tick));
    assert!(
        w.entities[0]
            .status
            .reflex
            .is_some_and(|r| r.barrier >= 100.)
    );
    assert_eq!(
        w.entities[0].item_stats.reflex_ready,
        Some(w.tick + 45 * TICK_HZ)
    );

    w.tick = 45 * TICK_HZ;
    w.command(
        1,
        Command::ActivateItem {
            instance: aegis.instance,
            target: ItemTarget::SelfCast,
        },
    )
    .unwrap();
}

#[test]
fn atlas_pauldron_delays_then_slows_attack_speed_and_reduces_wp_unless_blocked() {
    let mut w = sim_room(3);
    enable_all_store(&mut w);
    park_away_other_heroes(&mut w, &[1, 2, 4]);
    let p0 = w.entities[0].position;
    w.entities[1].position = Position {
        x: p0.x + 3.0,
        y: p0.y,
    };
    w.entities[3].position = Position {
        x: p0.x + 3.0,
        y: p0.y,
    };
    // Give hero 4 active Reflex Block during detonation.
    w.entities[3].status.reflex = Some(crate::reflex::Active {
        until: 40,
        barrier: 200.,
        effect: 0,
    });
    w.entities[1].stats.weapon = 100.;
    let base_interval = w.basic_attack_interval(&w.entities[1]);

    let atlas = w.purchase(1, BasicItem::AtlasPauldron).unwrap().granted;
    w.command(
        1,
        Command::ActivateItem {
            instance: atlas.instance,
            target: ItemTarget::SelfCast,
        },
    )
    .unwrap();
    // Before 0.8s (16 ticks), Atlas has not detonated yet.
    step_ticks(&mut w, 15);
    assert_eq!(w.basic_attack_interval(&w.entities[1]), base_interval);

    // After 0.8s detonation, hero 2 is debuffed, hero 4 blocked it.
    step_ticks(&mut w, 2);
    assert!(w.basic_attack_interval(&w.entities[1]) >= base_interval * 2);
    assert_eq!(w.basic_attack_interval(&w.entities[3]), base_interval);

    // WP damage from hero 2 is reduced by 30%.
    let mut events = Vec::new();
    let hp_before = w.entities[0].hp;
    w.hurt_basic(2, 1, 0, 100., score::DamageSchool::Weapon, &mut events);
    assert!((hp_before - w.entities[0].hp - 70.).abs() < 1e-3);
}

#[test]
fn boots_actives_travel_passive_journey_combat_cd_war_treads_and_teleport_boots() {
    let mut w = sim_room(4);
    enable_all_store(&mut w);
    park_away_other_heroes(&mut w, &[1, 2, 3, 5]);
    let p0 = w.entities[0].position;
    w.entities[2].position = Position {
        x: p0.x + 5.,
        y: p0.y,
    };
    w.entities[4].position = Position {
        x: p0.x + 20.,
        y: p0.y,
    };

    // Travel Boots: +0.3 base + +0.5 out-of-combat Travel passive.
    let travel = w.purchase(1, BasicItem::TravelBoots).unwrap().granted;
    assert_eq!(w.entities[0].item_stats.boot_speed, 0.3);
    assert_eq!(
        item_actives::extra_move_speed(&w.entities[0], &w.shop, w.tick),
        0.5
    );
    // Enemy hero damage cancels Travel passive for 5s.
    w.hurt(2, 1, 0, 10., score::DamageSchool::Weapon, &mut Vec::new());
    assert_eq!(
        item_actives::extra_move_speed(&w.entities[0], &w.shop, w.tick),
        0.0
    );
    w.tick += 5 * TICK_HZ;
    assert_eq!(
        item_actives::extra_move_speed(&w.entities[0], &w.shop, w.tick),
        0.5
    );

    // Halcyon Chargers active sprint.
    w.sell_item(1, travel.instance).unwrap();
    let halcyon = w.purchase(1, BasicItem::HalcyonChargers).unwrap().granted;
    w.command(
        1,
        Command::ActivateItem {
            instance: halcyon.instance,
            target: ItemTarget::SelfCast,
        },
    )
    .unwrap();
    assert_eq!(w.entities[0].item_stats.sprint_speed, 3.0);
    assert_eq!(w.entities[0].item_stats.sprint_until, w.tick + 3 * TICK_HZ);

    // Journey Boots combat cooldown cap to 30s on enemy hero damage.
    w.tick += 50 * TICK_HZ;
    w.sell_item(1, halcyon.instance).unwrap();
    let journey = w.purchase(1, BasicItem::JourneyBoots).unwrap().granted;
    w.command(
        1,
        Command::ActivateItem {
            instance: journey.instance,
            target: ItemTarget::SelfCast,
        },
    )
    .unwrap();
    assert_eq!(w.entities[0].item_stats.sprint_ready, w.tick + 60 * TICK_HZ);
    w.entities[1].position = p0;
    w.hurt(1, 0, 1, 10., score::DamageSchool::Weapon, &mut Vec::new());
    assert_eq!(w.entities[0].item_stats.sprint_ready, w.tick + 30 * TICK_HZ);

    // War Treads grants +3.0 MS to nearby allies in 12m.
    w.sell_item(1, journey.instance).unwrap();
    let treads = w.purchase(1, BasicItem::WarTreads).unwrap().granted;
    w.command(
        1,
        Command::ActivateItem {
            instance: treads.instance,
            target: ItemTarget::SelfCast,
        },
    )
    .unwrap();
    assert!(w.entities[2].item_stats.sprint_until > w.tick);
    assert_eq!(w.entities[2].item_stats.sprint_speed, 3.0);
    assert!(w.entities[4].item_stats.sprint_until <= w.tick);

    // Teleport Boots channels 2.5s, shields allied structure 600, teleports + grants speed, cancels on MoveTo.
    w.sell_item(1, treads.instance).unwrap();
    let tp = w.purchase(1, BasicItem::TeleportBoots).unwrap().granted;
    let tower_idx = w
        .entities
        .iter()
        .position(|e| e.team == 0 && e.kind == Kind::Tower)
        .unwrap();
    let tower_id = w.entities[tower_idx].id;
    let tower_pos = w.entities[tower_idx].position;
    w.entities[0].position = Position { x: p0.x, y: p0.y };
    w.command(
        1,
        Command::ActivateItem {
            instance: tp.instance,
            target: ItemTarget::Entity(tower_id),
        },
    )
    .unwrap();
    assert_eq!(w.entities[tower_idx].status.barrier, 600.);
    // Move order cancels teleport channel cleanly.
    w.command(
        1,
        Command::MoveTo(Position {
            x: p0.x + 1.,
            y: p0.y,
        }),
    )
    .unwrap();
    assert!(
        w.entities[0]
            .rt()
            .is_some_and(|rt| rt.teleport_channel.is_none())
    );

    // Wait cooldown and finish full channel.
    w.tick += 150 * TICK_HZ;
    w.command(
        1,
        Command::ActivateItem {
            instance: tp.instance,
            target: ItemTarget::Entity(tower_id),
        },
    )
    .unwrap();
    step_ticks(&mut w, 50);
    assert_eq!(w.entities[0].position, tower_pos);
    assert!(w.entities[0].item_stats.sprint_until > w.tick);
}

#[test]
fn capacitor_plate_protector_contract_and_rooks_decree() {
    let mut w = sim_room(5);
    enable_all_store(&mut w);
    park_away_other_heroes(&mut w, &[1, 2, 3]);
    let p0 = w.entities[0].position;
    w.entities[1].position = Position {
        x: p0.x + 1.5,
        y: p0.y,
    };
    w.entities[2].position = Position {
        x: p0.x + 4.0,
        y: p0.y,
    };

    w.purchase(1, BasicItem::ProtectorContract).unwrap();
    w.purchase(1, BasicItem::CapacitorPlate).unwrap();
    w.entities[0].energy = 1000.;
    w.command(1, Command::Learn(AbilitySlot::A)).unwrap();
    w.command(
        1,
        Command::Cast {
            slot: AbilitySlot::A,
            target: CastTarget::Entity(2),
        },
    )
    .unwrap();
    step_ticks(&mut w, 12);
    // Next basic hit on enemy hero grants 150 * 1.15 = 172.5 barrier to nearby allies.
    w.basic_hit(1, 0, 1, 20., score::DamageSchool::Weapon, &mut Vec::new());
    assert!((w.entities[2].status.barrier - 172.5).abs() < 1e-2);
    assert!(
        w.entities[2]
            .rt()
            .is_some_and(|rt| rt.capacitor_ms_bonus == 1.5)
    );

    // Upgrade to Rook's Decree: grants (150 + 0.15 * bonus_hp) * 1.15 barrier on 10s cd.
    w.purchase(1, BasicItem::RooksDecree).unwrap();
    w.tick += 15 * TICK_HZ;
    w.entities[0].cooldown = [0; 3];
    w.entities[0].energy = 1000.;
    w.entities[2].status.barrier = 0.;
    w.entities[2].status.barrier_until = 0;
    w.command(
        1,
        Command::Cast {
            slot: AbilitySlot::A,
            target: CastTarget::Entity(2),
        },
    )
    .unwrap();
    step_ticks(&mut w, 12);
    let expected = (150. + 0.15 * w.entities[0].bonus_hp) * 1.15;
    w.basic_hit(1, 0, 1, 20., score::DamageSchool::Weapon, &mut Vec::new());
    assert!((w.entities[2].status.barrier - expected).abs() < 1e-1);
}

#[test]
fn shiversteel_and_dragonblood_contract_slows() {
    let mut w = sim_room(6);
    enable_all_store(&mut w);
    park_away_other_heroes(&mut w, &[1, 2]);
    let p0 = w.entities[0].position;
    w.entities[1].position = Position {
        x: p0.x + 2.0,
        y: p0.y,
    };
    w.entities[0].stats.range = 2.0; // Melee

    let dbc = w
        .purchase(1, BasicItem::DragonbloodContract)
        .unwrap()
        .granted;
    w.basic_hit(1, 0, 1, 10., score::DamageSchool::Weapon, &mut Vec::new());
    assert!((w.entities[1].status.slow_factor - 0.65).abs() < 1e-5);

    // Dragonblood Contract has 6s cooldown.
    w.tick += 2 * TICK_HZ;
    w.entities[1].status.slow_until = 0;
    w.basic_hit(1, 0, 1, 10., score::DamageSchool::Weapon, &mut Vec::new());
    assert_eq!(w.entities[1].status.slow_until, 0);

    // Shiversteel slows on every basic hit with no cooldown + supports active slow.
    w.sell_item(1, dbc.instance).unwrap();
    let shiver = w.purchase(1, BasicItem::Shiversteel).unwrap().granted;
    w.basic_hit(1, 0, 1, 10., score::DamageSchool::Weapon, &mut Vec::new());
    assert!((w.entities[1].status.slow_factor - 0.65).abs() < 1e-5);
    w.tick += 2 * TICK_HZ;
    w.basic_hit(1, 0, 1, 10., score::DamageSchool::Weapon, &mut Vec::new());
    assert!(w.entities[1].status.slow_until > w.tick);

    w.entities[1].status.slow_until = 0;
    w.command(
        1,
        Command::ActivateItem {
            instance: shiver.instance,
            target: ItemTarget::Entity(2),
        },
    )
    .unwrap();
    assert!(w.entities[1].status.slow_until > w.tick);
}

#[test]
fn vision_items_flare_flaregun_contraption_pak_tuff_superscout_and_trap_damage() {
    let mut w = aral_room(7);
    let p0 = w.entities[0].position;
    let flare_spot = Position {
        x: p0.x + 5.,
        y: p0.y,
    };
    w.entities[1].position = flare_spot;

    // ScoutPak (+1 max charge, -30s cooldown) + ScoutTuff (+1 hit, +30s lifespan) + SuperScout2000.
    let pak = w.purchase(1, BasicItem::ScoutPak).unwrap().granted;
    let cam = Command::DeployVisionTotem {
        instance: 2001,
        point: p0,
    };
    // 2 base + 1 ScoutPak = 3 ScoutCam charges.
    w.command(1, cam).unwrap();
    w.command(1, cam).unwrap();
    w.command(1, cam).unwrap();
    assert!(w.command(1, cam).is_err());

    // SuperScout2000 (+3 charges, -60s cd, +3 hits, +60s life) + ScoutTrap reveal + Flare.
    w.sell_item(1, pak.instance).unwrap();
    let super_scout = w.purchase(1, BasicItem::SuperScout2000).unwrap().granted;
    w.tick += 120 * TICK_HZ; // -60s from 180s recharge
    w.command(1, cam).unwrap();
    let cam_id = w.vision_devices().last().unwrap().id;
    // Extra 3 hits => takes 8 hits total to destroy before stealth (40 ticks)!
    let enemy_ids = [2u32, 4, 6];
    for &eid in &enemy_ids {
        let idx = w.index(eid).unwrap();
        w.entities[idx].position = p0;
    }
    for hit in 0..8 {
        if hit == 3 || hit == 6 {
            w.tick += 19;
        }
        let eid = enemy_ids[hit % 3];
        w.command(eid, Command::HitScoutCam { device: cam_id })
            .unwrap();
        if hit < 7 {
            assert!(w.vision_devices().any(|v| v.id == cam_id));
        }
    }
    assert!(!w.vision_devices().any(|v| v.id == cam_id));
    w.entities[1].position = w.layout.home(&w.entities[1]);

    // Enemy buys ScoutTrap at base, walks to flare_spot, places it, and Hero 1 fires SuperScout2000 Flare.
    let trap = w.purchase(2, BasicItem::ScoutTrap).unwrap().granted;
    w.entities[1].position = flare_spot;
    w.command(
        2,
        Command::DeployScoutTrap {
            instance: trap.instance,
        },
    )
    .unwrap();
    w.command(
        1,
        Command::ActivateItem {
            instance: super_scout.instance,
            target: ItemTarget::Point(flare_spot),
        },
    )
    .unwrap();
    assert!(
        w.vision_devices()
            .any(|v| v.kind == VisionDeviceKind::Flare)
    );
    assert!(
        w.vision_devices()
            .any(|v| v.kind == VisionDeviceKind::ScoutTrap && v.revealed)
    );
    assert!(w.entities[1].status.reveal_until > w.tick);

    // Contraption places ScoutTrap on SelfCast and Flare at range.
    let contraption = w.purchase(1, BasicItem::Contraption).unwrap().granted;
    let traps_before = w
        .vision_devices()
        .filter(|v| v.kind == VisionDeviceKind::ScoutTrap && v.team == 0)
        .count();
    w.command(
        1,
        Command::ActivateItem {
            instance: contraption.instance,
            target: ItemTarget::SelfCast,
        },
    )
    .unwrap();
    let traps_after = w
        .vision_devices()
        .filter(|v| v.kind == VisionDeviceKind::ScoutTrap && v.team == 0)
        .count();
    assert_eq!(traps_after, traps_before + 1);

    w.tick += 5 * TICK_HZ;
    let flares_before = w
        .vision_devices()
        .filter(|v| v.kind == VisionDeviceKind::Flare)
        .count();
    w.command(
        1,
        Command::ActivateItem {
            instance: contraption.instance,
            target: ItemTarget::Point(flare_spot),
        },
    )
    .unwrap();
    assert_eq!(
        w.vision_devices()
            .filter(|v| v.kind == VisionDeviceKind::Flare)
            .count(),
        flares_before + 1
    );

    // Consumable Flare, FlareLoader, Flaregun, and ScoutTuff.
    let mut non_aral = sim_room(8);
    enable_all_store(&mut non_aral);
    let np0 = non_aral.entities[0].position;
    let fl = non_aral.purchase(1, BasicItem::Flare).unwrap().granted;
    non_aral
        .command(
            1,
            Command::ActivateItem {
                instance: fl.instance,
                target: ItemTarget::Point(Position {
                    x: np0.x + 8.,
                    y: np0.y,
                }),
            },
        )
        .unwrap();
    assert!(non_aral.entities[0].inventory.is_empty());

    let loader = non_aral
        .purchase(1, BasicItem::FlareLoader)
        .unwrap()
        .granted;
    non_aral.tick += 10 * TICK_HZ;
    non_aral
        .command(
            1,
            Command::ActivateItem {
                instance: loader.instance,
                target: ItemTarget::Point(Position {
                    x: np0.x + 4.,
                    y: np0.y,
                }),
            },
        )
        .unwrap();
    let gun = non_aral.purchase(1, BasicItem::Flaregun).unwrap().granted;
    non_aral.tick += 10 * TICK_HZ;
    non_aral
        .command(
            1,
            Command::ActivateItem {
                instance: gun.instance,
                target: ItemTarget::Point(Position {
                    x: np0.x + 5.,
                    y: np0.y,
                }),
            },
        )
        .unwrap();
    let tuff = non_aral.purchase(1, BasicItem::ScoutTuff).unwrap().granted;
    non_aral.tick += 10 * TICK_HZ;
    non_aral
        .command(
            1,
            Command::ActivateItem {
                instance: tuff.instance,
                target: ItemTarget::Point(Position {
                    x: np0.x + 6.,
                    y: np0.y,
                }),
            },
        )
        .unwrap();
}

#[test]
fn breaking_point_and_dragons_eye_stacking_decay_and_sale_cleanup() {
    let mut w = sim_room(9);
    enable_all_store(&mut w);
    park_away_other_heroes(&mut w, &[1, 2]);
    w.entities[1].max_hp = 20_000.;
    w.entities[1].hp = 20_000.;
    w.entities[0].stats.range = 2.0; // Melee: 100 + 5*k per stack

    let bp = w.purchase(1, BasicItem::BreakingPoint).unwrap().granted;
    let base_wp = w.entities[0].stats.weapon;
    // 205 damage gains 2 stacks (100 + 105 = 205 => +10 WP).
    w.basic_hit(1, 0, 1, 205., score::DamageSchool::Weapon, &mut Vec::new());
    assert_eq!(w.entities[0].rt().unwrap().bp_stacks, 2);
    assert!((w.entities[0].stats.weapon - (base_wp + 10.)).abs() < 1e-3);

    // Wait 3.0s decay start + 0.4s (decays 2 stacks at 5/sec).
    step_ticks(&mut w, 68);
    assert_eq!(w.entities[0].rt().unwrap().bp_stacks, 0);
    assert!((w.entities[0].stats.weapon - base_wp).abs() < 1e-3);

    // Re-stack then sell removes WP bonus cleanly.
    w.basic_hit(1, 0, 1, 100., score::DamageSchool::Weapon, &mut Vec::new());
    assert_eq!(w.entities[0].rt().unwrap().bp_stacks, 1);
    w.sell_item(1, bp.instance).unwrap();
    assert_eq!(w.entities[0].rt().unwrap().bp_stacks, 0);
    assert!((w.entities[0].stats.weapon - (base_wp - 50.)).abs() < 1e-3);

    // Dragon's Eye: +10 CP/sec up to 15 stacks, holds 5s, decays 3/sec.
    let de = w.purchase(1, BasicItem::DragonsEye).unwrap().granted;
    let base_cp = w.entities[0].item_stats.crystal;
    for sec in 0..3 {
        w.tick = (100 + sec) * TICK_HZ;
        w.hurt(1, 0, 1, 50., score::DamageSchool::Crystal, &mut Vec::new());
    }
    assert_eq!(w.entities[0].rt().unwrap().de_stacks, 3);
    assert!((w.entities[0].item_stats.crystal - (base_cp + 30.)).abs() < 1e-3);

    // After 5s hold + 1s decay => loses 3 stacks.
    step_ticks(&mut w, 6 * TICK_HZ);
    assert_eq!(w.entities[0].rt().unwrap().de_stacks, 0);
    assert!((w.entities[0].item_stats.crystal - base_cp).abs() < 1e-3);

    w.hurt(1, 0, 1, 50., score::DamageSchool::Crystal, &mut Vec::new());
    assert_eq!(w.entities[0].rt().unwrap().de_stacks, 1);
    w.sell_item(1, de.instance).unwrap();
    assert_eq!(w.entities[0].item_stats.crystal, 0.);
}

#[test]
fn aftershock_spellfire_frostburn_and_clockwork_ability_passives() {
    let mut w = sim_room(10);
    enable_all_store(&mut w);
    park_away_other_heroes(&mut w, &[1, 2]);
    let p0 = w.entities[0].position;
    w.entities[1].position = Position {
        x: p0.x + 2.,
        y: p0.y,
    };
    w.entities[1].max_hp = 2000.;
    w.entities[1].hp = 2000.;

    w.purchase(1, BasicItem::Aftershock).unwrap();
    w.purchase(1, BasicItem::Spellfire).unwrap();
    w.purchase(1, BasicItem::Frostburn).unwrap();
    w.purchase(1, BasicItem::Clockwork).unwrap();

    // Put slot B & C on 20s cooldown to verify Clockwork 10% cooldown reduction (2s).
    w.entities[0].cooldown[1] = w.tick + 20 * TICK_HZ;
    w.entities[0].cooldown[2] = w.tick + 20 * TICK_HZ;
    w.command(1, Command::Learn(AbilitySlot::A)).unwrap();
    w.entities[0].hp = 1000.;
    w.command(
        1,
        Command::Cast {
            slot: AbilitySlot::A,
            target: CastTarget::Entity(2),
        },
    )
    .unwrap();
    step_ticks(&mut w, 12);

    // Aftershock should be armed; Frostburn slow & Spellfire Mortal Wounds/burn active; Clockwork refunded 2s.
    assert!(w.entities[0].rt().unwrap().aftershock_armed_until > w.tick);
    assert!(w.entities[1].status.slow_until > w.tick);
    // Ringo A applies its own stronger slow; verify Frostburn's exact 25% slow (factor 0.75) on a non-slowing ability hit.
    w.entities[1].status.slow_until = 0;
    w.entities[1].status.slow_factor = 1.0;
    w.hurt(1, 0, 1, 50., score::DamageSchool::Crystal, &mut Vec::new());
    assert!(w.entities[1].status.slow_until > w.tick);
    assert!((w.entities[1].status.slow_factor - 0.75).abs() < 1e-4);
    assert!(w.entities[1].rt().unwrap().mortal_wounds_until > w.tick);
    assert!(w.entities[1].rt().unwrap().spellfire_burn.is_some());
    assert!(w.entities[0].cooldown[1] <= w.tick + 18 * TICK_HZ);

    // Trigger Aftershock next basic: 10% target max HP (200 crystal damage) + 25% lifesteal (50 HP).
    let hp_before = w.entities[1].hp;
    let atk_before = w.entities[0].hp;
    w.basic_hit(1, 0, 1, 0.01, score::DamageSchool::Weapon, &mut Vec::new());
    let delta_target = hp_before - w.entities[1].hp;
    assert!((delta_target - 200.).abs() < 1.0);
    assert!(w.entities[0].hp > atk_before + 45.);

    // Spellfire burn ticks over 3s.
    let pre_burn = w.entities[1].hp;
    step_ticks(&mut w, 3 * TICK_HZ + 5);
    assert!(w.entities[1].hp < pre_burn - 50.);

    // Verify Spellfire ignores lane minions.
    let minion = w.spawn_with_stats(1, Kind::Minion, w.stats[4], p0, false);
    let m_idx = w.index(minion).unwrap();
    w.hurt(
        1,
        0,
        m_idx,
        50.,
        score::DamageSchool::Crystal,
        &mut Vec::new(),
    );
    assert!(w.entities[m_idx].rt().is_none());
}

#[test]
fn poisoned_shiv_eve_of_harvest_serpent_mask_mortal_wounds_and_pools() {
    let mut w = sim_room(11);
    enable_all_store(&mut w);
    park_away_other_heroes(&mut w, &[1, 2]);
    w.entities[1].max_hp = 5000.;
    w.entities[1].hp = 5000.;
    w.entities[0].hp = 500.;

    // Poisoned Shiv: every 2nd hit (per items_full.json Hits_To_Proc=2.0) applies Mortal Wounds.
    let shiv = w.purchase(1, BasicItem::PoisonedShiv).unwrap().granted;
    w.basic_hit(1, 0, 1, 100., score::DamageSchool::Weapon, &mut Vec::new());
    assert!(w.entities[1].rt().is_none()); // hit 1
    w.basic_hit(1, 0, 1, 100., score::DamageSchool::Weapon, &mut Vec::new());
    assert!(w.entities[1].rt().unwrap().mortal_wounds_until > w.tick); // hit 2

    // Mortal Wounds cuts healing to 67%.
    w.entities[1].hp = 1000.;
    w.heal(2, 1, 100., &mut Vec::new());
    assert!((w.entities[1].hp - 1067.).abs() < 1e-2);

    // Serpent Mask: 15% base + 25% pool (= 40% on first 400 HP dmg vs hero at lvl 1).
    w.tick += 5 * TICK_HZ;
    w.sell_item(1, shiv.instance).unwrap();
    let mask = w.purchase(1, BasicItem::SerpentMask).unwrap().granted;
    w.entities[0].hp = 500.;
    w.basic_hit(1, 0, 1, 100., score::DamageSchool::Weapon, &mut Vec::new());
    assert!((w.entities[0].hp - 540.).abs() < 1e-2);
    assert!((w.entities[0].rt().unwrap().serpent_pool - 300.).abs() < 1e-2);

    // Eve of Harvest: 15% omni + 25% pool (= 40% on crystal ability dmg vs hero).
    w.sell_item(1, mask.instance).unwrap();
    w.purchase(1, BasicItem::EveOfHarvest).unwrap();
    w.entities[0].hp = 500.;
    w.hurt(1, 0, 1, 100., score::DamageSchool::Crystal, &mut Vec::new());
    assert!((w.entities[0].hp - 540.).abs() < 1e-2);
    assert!((w.entities[0].rt().unwrap().eve_pool - 300.).abs() < 1e-2);
}

#[test]
fn slumbering_husk_seraphic_shell_metal_jacket_and_warmail() {
    let mut w = sim_room(12);
    enable_all_store(&mut w);
    park_away_other_heroes(&mut w, &[1, 2]);

    // Slumbering Husk: 25% HP burst within 1s fortifies remaining HP for 3s (50% reduction).
    let husk = w.purchase(1, BasicItem::SlumberingHusk).unwrap().granted;
    w.entities[0].stats.armor = 0.;
    w.entities[0].stats.shield = 0.;
    let max_hp = w.entities[0].max_hp;
    w.hurt_basic(
        2,
        1,
        0,
        max_hp * 0.26,
        score::DamageSchool::Weapon,
        &mut Vec::new(),
    );
    assert!(w.entities[0].rt().unwrap().slumbering_fortify_until > w.tick);
    let hp_mid = w.entities[0].hp;
    w.hurt_basic(2, 1, 0, 100., score::DamageSchool::Weapon, &mut Vec::new());
    assert!((hp_mid - w.entities[0].hp - 50.).abs() < 1e-3);
    w.tick += 4 * TICK_HZ; // Wait for Slumbering Husk fortify window (3s) to expire.
    w.sell_item(1, husk.instance).unwrap();

    // Seraphic Shell (Celestial Shroud): blocks enemy hero ability damage & debuffs, not basics, 35s cd.
    let shroud = w.purchase(1, BasicItem::SeraphicShell).unwrap().granted;
    w.entities[0].stats.shield = 0.;
    w.entities[0].stats.armor = 0.;
    assert!(w.entities[0].status.blocks_debuffs(w.tick));
    let before = w.entities[0].hp;
    w.hurt(2, 1, 0, 300., score::DamageSchool::Crystal, &mut Vec::new());
    assert_eq!(w.entities[0].hp, before); // Blocked!
    // Basic attack is NOT blocked by Celestial Shroud.
    w.hurt_basic(2, 1, 0, 100., score::DamageSchool::Weapon, &mut Vec::new());
    assert_eq!(w.entities[0].hp, before - 100.);
    // After 0.5s window (10 ticks), shield enters 35s cooldown.
    step_ticks(&mut w, 12);
    assert!(!w.entities[0].status.blocks_debuffs(w.tick));
    let hp2 = w.entities[0].hp;
    w.hurt(2, 1, 0, 200., score::DamageSchool::Crystal, &mut Vec::new());
    assert_eq!(w.entities[0].hp, hp2 - 200.);
    w.sell_item(1, shroud.instance).unwrap();

    // Metal Jacket reduces BasicAttack by 15% and leaves abilities full damage.
    let jacket = w.purchase(1, BasicItem::MetalJacket).unwrap().granted;
    w.entities[0].stats.armor = 0.;
    w.entities[0].stats.shield = 0.;
    let hp3 = w.entities[0].hp;
    w.hurt_basic(2, 1, 0, 100., score::DamageSchool::Weapon, &mut Vec::new());
    assert!((hp3 - w.entities[0].hp - 85.).abs() < 1e-3);
    let hp4 = w.entities[0].hp;
    w.hurt(2, 1, 0, 100., score::DamageSchool::Crystal, &mut Vec::new());
    assert!((hp4 - w.entities[0].hp - 100.).abs() < 1e-3);
    w.sell_item(1, jacket.instance).unwrap();

    // Warmail grants Armor + Shield cleanly.
    w.entities[0].stats.armor = 0.;
    w.entities[0].stats.shield = 0.;
    w.purchase(1, BasicItem::Warmail).unwrap();
    assert_eq!(w.entities[0].stats.armor, 30.);
    assert_eq!(w.entities[0].stats.shield, 30.);
}

#[test]
fn spellsword_stormcrown_banner_bonesaw_pulseweave_tornado_lifespring_and_minions_foot() {
    let mut w = sim_room(13);
    enable_all_store(&mut w);
    park_away_other_heroes(&mut w, &[1, 2]);
    let p0 = w.entities[0].position;
    w.entities[1].position = Position {
        x: p0.x + 1.5,
        y: p0.y,
    };
    w.entities[1].max_hp = 5000.;
    w.entities[1].hp = 5000.;
    w.entities[1].stats.armor = 100.;

    // Spellsword: +12 energy vs hero, +4 vs minion.
    let ss = w.purchase(1, BasicItem::Spellsword).unwrap().granted;
    w.entities[0].energy = 100.;
    w.basic_hit(1, 0, 1, 10., score::DamageSchool::Weapon, &mut Vec::new());
    assert_eq!(w.entities[0].energy, 112.);
    let minion = w.spawn_with_stats(1, Kind::Minion, w.stats[4], p0, false);
    let m_idx = w.index(minion).unwrap();
    w.basic_hit(
        1,
        0,
        m_idx,
        10.,
        score::DamageSchool::Weapon,
        &mut Vec::new(),
    );
    assert_eq!(w.entities[0].energy, 116.);
    w.sell_item(1, ss.instance).unwrap();

    // StormguardBanner + Stormcrown (non-stacking, Stormcrown wins, 25% vs hero, 100% vs Neutral, cooldown key 0xdb161949).
    let banner = w.purchase(1, BasicItem::StormguardBanner).unwrap().granted;
    w.purchase(1, BasicItem::Stormcrown).unwrap(); // consumes banner or upgrades
    let _ = banner;
    let mut events = Vec::new();
    let hp_before = w.entities[1].hp;
    w.basic_hit(1, 0, 1, 0., score::DamageSchool::Weapon, &mut events);
    // Level 1 Stormcrown vs Hero: 500 * 0.25 = 125 true damage.
    assert!((hp_before - w.entities[1].hp - 125.).abs() < 1e-2);
    assert!(events.iter().any(|e| matches!(
        e,
        Event::ItemCooldownUpdated {
            actor: 1,
            key: STORMGUARD_COOLDOWN_KEY,
            ..
        }
    )));
    // Neutral jungle monster gets full 100% (500 true damage).
    w.tick += 6 * TICK_HZ;
    let neutral = w.spawn_with_stats(2, Kind::Neutral, w.stats[5], p0, false);
    let n_idx = w.index(neutral).unwrap();
    w.entities[n_idx].max_hp = 3000.;
    w.entities[n_idx].hp = 3000.;
    w.basic_hit(
        1,
        0,
        n_idx,
        0.,
        score::DamageSchool::Weapon,
        &mut Vec::new(),
    );
    assert!((3000. - w.entities[n_idx].hp - 500.).abs() < 1e-2);

    // Bonesaw shreds 10% armor/stack up to 5 stacks.
    w.purchase(1, BasicItem::Bonesaw).unwrap();
    w.tick += 6 * TICK_HZ;
    for _ in 0..6 {
        w.basic_hit(1, 0, 1, 10., score::DamageSchool::Weapon, &mut Vec::new());
    }
    assert_eq!(w.entities[1].rt().unwrap().armor_shred_stacks, 5);
    assert!((item_actives::armor_with_shred(&w.entities[1], w.tick) - 50.).abs() < 1e-3);

    // Tornado Trigger grants +0.1 MS for 1.2s.
    w.purchase(1, BasicItem::TornadoTrigger).unwrap();
    w.basic_hit(1, 0, 1, 10., score::DamageSchool::Weapon, &mut Vec::new());
    assert!((w.entities[0].rt().unwrap().tornado_ms_bonus - 0.1).abs() < 1e-5);

    // Lifespring heals 1.5% missing HP/sec out of combat (non-stacking with Pulseweave).
    w.purchase(1, BasicItem::Lifespring).unwrap();
    let pw = w.purchase(1, BasicItem::Pulseweave).unwrap().granted;
    w.tick += 10 * TICK_HZ;
    let max0 = w.entities[0].max_hp;
    w.entities[0].hp = max0 * 0.5;
    let pre_regen = w.entities[0].hp;
    step_ticks(&mut w, TICK_HZ);
    assert!(w.entities[0].hp > pre_regen);

    // Pulseweave burst on enemy hero hit: +2.0 MS for 3s then AoE burst + slow.
    w.hurt(2, 1, 0, 50., score::DamageSchool::Weapon, &mut Vec::new());
    assert!(w.entities[0].rt().unwrap().pulseweave_burst.is_some());
    assert!(item_actives::extra_move_speed(&w.entities[0], &w.shop, w.tick) >= 2.0);
    w.entities[1].status.slow_until = 0;
    let hp_pre_pw = w.entities[1].hp;
    step_ticks(&mut w, 3 * TICK_HZ + 1);
    assert!(w.entities[1].hp < hp_pre_pw - 20.);
    assert!(w.entities[1].status.slow_until > w.tick);
    w.sell_item(1, pw.instance).unwrap();

    // Minion's Foot guarantees crit on first basic vs enemy hero after purchase.
    w.purchase(1, BasicItem::MinionsFoot).unwrap();
    assert!(w.entities[0].rt().unwrap().minions_foot_ready);
    w.entities[0].stats.range = 6.0;
    w.entities[0].order = Command::Attack(2);
    w.entities[0].winding = Some((w.tick, 2));
    w.entities[0].next_attack = w.tick;
    let _ = w.step();
    assert!(!w.entities[0].rt().unwrap().minions_foot_ready);
}

#[test]
fn state_digest_determinism_and_healing_flask_cooldown_event() {
    fn run_match(seed: u64) -> String {
        let mut w = aral_room(seed);
        let p0 = w.entities[0].position;
        w.entities[1].position = Position {
            x: p0.x + 2.,
            y: p0.y,
        };
        let aegis = w.purchase(1, BasicItem::Aegis).unwrap().granted;
        w.purchase(1, BasicItem::Spellfire).unwrap();
        w.command(
            1,
            Command::ActivateItem {
                instance: aegis.instance,
                target: ItemTarget::SelfCast,
            },
        )
        .unwrap();
        w.command(1, Command::ActivateHealingFlask { instance: 2000 })
            .unwrap();
        let evs = step_ticks(&mut w, 80);
        assert!(evs.iter().any(|e| matches!(
            e,
            Event::ItemCooldownUpdated {
                actor: 1,
                key: HEALING_FLASK_COOLDOWN_KEY,
                ..
            }
        )));
        w.state_digest()
    }
    assert_eq!(run_match(777), run_match(777));
    // Non-hero entities retain `item_runtime: None` cleanly in default rooms.
    let mut plain = World::new(0, 0);
    step_ticks(&mut plain, 40);
    assert!(
        plain
            .entities
            .iter()
            .filter(|e| e.kind != Kind::Hero)
            .all(|e| e.rt().is_none())
    );
}

#[test]
#[ignore = "requires original HALCYON_DATA_EXTRA items_full.json"]
fn catalog_loaded_rules_match_all_76_store_items_and_items_full() {
    let data = GameData::load_with_extra(
        std::env::var_os("HALCYON_DELIVERY").unwrap(),
        Some(std::path::Path::new(
            &std::env::var_os("HALCYON_DATA_EXTRA").unwrap(),
        )),
    )
    .expect("game data from env");
    let mut w = World::from_data(1, 6, &data).unwrap();
    w.use_catalog_store(StoreSettings::Five);
    assert!(w.shop.effect_rules.active.aegis_cooldown == 45 * TICK_HZ);
    assert!(w.shop.effect_rules.active.fountain_cooldown == 75 * TICK_HZ);
    assert!(w.shop.effect_rules.active.atlas_delay == 16);
    assert!(w.shop.effect_rules.passive.bp_max_stacks == 35);
    assert!(w.shop.effect_rules.passive.de_max_stacks == 15);
    assert!(w.shop.effect_rules.passive.shiv_hits_to_proc == 2);
    assert!(w.shop.effect_rules.passive.seraphic_cooldown == 35 * TICK_HZ);
    assert!((w.shop.effect_rules.passive.stormcrown_per_level - 22.727).abs() < 1e-3);

    // Every one of the 76 ordinary store items in `items_full.json` can be purchased & sold cleanly.
    w.shop.enable_all_store_for_test();
    for kind in crate::shop::Shop::all_kinds() {
        w.entities[0].gold = 50_000;
        let receipt = w.purchase(1, kind).unwrap();
        let _ = w.sell_item(1, receipt.granted.instance).unwrap();
    }
    assert!(w.entities[0].inventory.is_empty());
}
