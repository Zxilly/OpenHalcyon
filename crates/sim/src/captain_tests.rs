use super::*;
use std::path::Path;

#[test]
fn ordinary_damage_keeps_captain_state_unallocated_and_checkpoint_stable() {
    let mut world = World::new(29, 6);
    world.hurt(2, 1, 0, 10., DamageSchool::Weapon, &mut Vec::new());
    assert!(
        world.entities[0]
            .status
            .captain_effects
            .active_mut()
            .is_none()
    );
    let checkpoint = world.state_digest();
    world.entities[0].status.captain_effects.arm();
    assert_eq!(checkpoint, world.state_digest());
    println!(
        "entity={} status={} active_captain={}",
        size_of::<Entity>(),
        size_of::<combat::Status>(),
        size_of::<combat::CaptainState>(),
    );
}

fn room(hero: &str) -> World {
    let data = GameData::load_with_extra(
        std::env::var_os("HALCYON_DELIVERY").unwrap(),
        Some(Path::new(&std::env::var_os("HALCYON_DATA_EXTRA").unwrap())),
    )
    .unwrap();
    let mut w = World::from_data(29, 6, &data).unwrap();
    w.use_six_human_roster().unwrap();
    w.select_hero(0, hero, &data).unwrap();
    w.use_aral_lane().unwrap();
    for i in 0..w.entities.len().min(6) {
        let e = &mut w.entities[i];
        e.position = Position {
            x: i as f32 * 2.,
            y: 0.,
        };
        e.hp = 2500.;
        e.max_hp = 2500.;
        e.stats.shield = 0.;
    }
    let e = &mut w.entities[0];
    e.level = 6;
    e.skill_points = 3;
    e.energy = 900.;
    w
}
fn cast(w: &mut World, slot: AbilitySlot, target: CastTarget, delay: u64, events: &mut Vec<Event>) {
    w.command(1, Command::Learn(slot)).unwrap();
    w.command(1, Command::Cast { slot, target }).unwrap();
    w.tick += delay;
    w.advance_cast(0, events);
}
#[test]
#[ignore = "requires original delivery and supplemental combat data"]
fn wolf_maw_hits_the_first_body_once_pulls_over_time_and_cannot_reach_an_ally() {
    let mut w = room("Yates");
    let mut out = vec![];
    w.entities[1].position.x = 5.;
    w.entities[3].position.x = 3.;
    w.entities[2].position.x = 1.;
    let hp = w.entities[3].hp;
    cast(
        &mut w,
        AbilitySlot::A,
        CastTarget::Point(Position { x: 9., y: 0. }),
        1,
        &mut out,
    );
    assert_eq!(w.entities[0].energy, 820.);
    for _ in 0..10 {
        w.tick += 1;
        w.advance_flail(0, &mut out);
    }
    assert_eq!(w.entities[3].hp, hp - 165.);
    assert!((w.entities[3].position.x - 1.2).abs() < 0.001);
    assert_eq!(w.entities[1].hp, hp);
    assert_eq!(w.entities[2].hp, hp);
    assert!(w.entities[0].status.captain_effects.read().flail.is_none());
}
#[test]
#[ignore = "requires original delivery and supplemental combat data"]
fn iron_mandate_channels_can_cancel_and_only_affects_the_landing_circle() {
    let mut w = room("Yates");
    let mut out = vec![];
    w.entities[0].item_stats.crystal = 40.;
    let p = Position { x: 12., y: 0. };
    w.entities[1].position.x = 18.;
    w.entities[3].position.x = 18.1;
    w.entities[2].position = p;
    w.command(1, Command::Learn(AbilitySlot::C)).unwrap();
    let c = Command::Cast {
        slot: AbilitySlot::C,
        target: CastTarget::Point(p),
    };
    w.command(1, c).unwrap();
    w.command(1, Command::Stop).unwrap();
    assert_eq!(w.entities[0].energy, 900.);
    w.command(1, c).unwrap();
    w.tick = 19;
    w.advance_cast(0, &mut out);
    assert_eq!(w.entities[0].position, Position::default());
    w.tick = 20;
    w.advance_cast(0, &mut out);
    assert_eq!(w.entities[0].position, p);
    assert_eq!(w.entities[0].energy, 800.);
    assert_eq!(w.entities[2].status.barrier, 300.);
    assert_eq!(w.entities[1].status.slow_until, 80);
    assert_eq!(w.entities[1].status.slow_factor, 0.7);
    assert!(w.entities[1].hp < 2500.);
    assert_eq!(w.entities[3].hp, 2500.);
}
#[test]
#[ignore = "requires original delivery and supplemental combat data"]
fn stormguard_caps_the_burst_shares_only_the_excess_and_pulses() {
    let mut w = room("Catherine");
    let mut out = vec![];
    cast(&mut w, AbilitySlot::B, CastTarget::SelfCast, 1, &mut out);
    let cap = 2500. * 0.075;
    w.hurt(2, 1, 0, cap + 300., DamageSchool::Weapon, &mut out);
    assert_eq!(w.entities[0].hp, 2500. - cap);
    for j in [1, 3, 5] {
        assert_eq!(w.entities[j].hp, 2400.)
    }
    w.tick = 21;
    w.advance_stormguard(0, &mut out);
    assert_eq!(w.entities[1].hp, 2355.);
    assert_eq!(w.entities[3].hp, 2400.);
    w.tick = 78;
    w.hurt(2, 1, 0, cap + 300., DamageSchool::Weapon, &mut out);
    assert_eq!(w.entities[0].hp, 2500. - cap - cap - 300.);
}
#[test]
#[ignore = "requires original delivery and supplemental combat data"]
fn blast_tremor_hits_only_the_forward_sector_and_silence_permits_basic_actions() {
    let mut w = room("Catherine");
    let mut out = vec![];
    w.entities[3].position = Position { x: 0., y: 4. };
    w.entities[5].position = Position { x: 11.1, y: 0. };
    cast(
        &mut w,
        AbilitySlot::C,
        CastTarget::Point(Position { x: 10., y: 0. }),
        20,
        &mut out,
    );
    assert_eq!(w.entities[1].hp, 2100.);
    assert_eq!(w.entities[1].status.silence_until, 50);
    assert_eq!(w.entities[3].hp, 2500.);
    assert_eq!(w.entities[5].hp, 2500.);
    assert_eq!(w.entities[2].hp, 2500.);
    assert_eq!(w.entities[0].status.reveal_until, 80);
    assert!(
        w.command(
            2,
            Command::Cast {
                slot: AbilitySlot::A,
                target: CastTarget::Entity(1)
            }
        )
        .is_err()
    );
    w.command(2, Command::Attack(1)).unwrap();
}
#[test]
#[ignore = "requires original delivery and supplemental combat data"]
fn iron_mandate_checks_landing_ground_and_releases_overdue_overwhelm_after_landing() {
    let mut w = room("Yates");
    let mut out = vec![];
    w.layout = Layout::Validation;
    w.entities[0].position = Position { x: 16., y: 3. };
    w.entities[1].position = Position { x: 25., y: 3. };
    w.command(1, Command::Learn(AbilitySlot::A)).unwrap();
    assert!(
        w.command(
            1,
            Command::Cast {
                slot: AbilitySlot::A,
                target: CastTarget::Point(w.entities[1].position)
            }
        )
        .is_err()
    );
    cast(&mut w, AbilitySlot::B, CastTarget::SelfCast, 1, &mut out);
    w.tick = 22;
    w.command(1, Command::Learn(AbilitySlot::C)).unwrap();
    assert!(
        w.command(
            1,
            Command::Cast {
                slot: AbilitySlot::C,
                target: CastTarget::Point(Position { x: 19., y: 3. })
            }
        )
        .is_err()
    );
    let landing = w.entities[1].position;
    w.command(
        1,
        Command::Cast {
            slot: AbilitySlot::C,
            target: CastTarget::Point(landing),
        },
    )
    .unwrap();
    for _ in 0..19 {
        w.step();
    }
    assert_eq!(w.entities[1].status.stun_until, 0);
    assert_eq!(w.entities[0].position.x, 16.);
    assert!(
        w.entities[0]
            .status
            .captain_effects
            .read()
            .overwhelm
            .is_some()
    );
    w.step();
    assert_eq!(w.entities[0].position, landing);
    assert_eq!(w.entities[1].status.stun_until, 58);
    assert!(
        w.entities[0]
            .status
            .captain_effects
            .read()
            .overwhelm
            .is_none()
    );
}
#[test]
#[ignore = "requires original delivery and supplemental combat data"]
fn captain_bots_choose_each_learned_slot_through_the_common_command_path() {
    for (hero, slot, self_cast) in [
        ("Catherine", AbilitySlot::A, true),
        ("Catherine", AbilitySlot::B, true),
        ("Catherine", AbilitySlot::C, false),
        ("Yates", AbilitySlot::A, false),
        ("Yates", AbilitySlot::B, true),
        ("Yates", AbilitySlot::C, false),
    ] {
        let mut w = room(hero);
        w.command(1, Command::Learn(slot)).unwrap();
        w.entities[0].bot = true;
        let action = w.decide(0);
        let expected = if self_cast {
            CastTarget::SelfCast
        } else {
            CastTarget::Point(w.entities[1].position)
        };
        assert_eq!(
            action,
            Command::Cast {
                slot,
                target: expected
            },
            "{hero} {slot:?}"
        );
        w.order(0, action).unwrap();
    }
}
