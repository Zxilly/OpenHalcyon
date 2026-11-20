use super::*;
use sha2::{Digest, Sha256};

pub(super) fn field_replay_world(data: &GameData) -> World {
    let mut world = World::from_data(23, 6, data).unwrap();
    // These rules were added after the recorded checkpoint. Later
    // skills are outside this fixture, but the digest includes the whole book.
    let book = std::sync::Arc::make_mut(&mut world.abilities);
    book.verse = None;
    book.skedaddle = None;
    book.super_kick = None;
    world.use_six_human_roster().unwrap();
    for (seat, name) in [(0, "Lyra"), (1, "Adagio"), (2, "Lyra"), (3, "Adagio")] {
        world.select_hero(seat, name, data).unwrap();
    }
    world.use_aral_lane().unwrap();
    // This pre-migration fixture predates native tower attachment allocation.
    world.next_id = 517;
    world
}

/// Replays simultaneous casts and their overlapping field, heal and burn ticks.
#[test]
#[ignore = "requires original delivery and supplemental combat data"]
fn overlapping_bulwark_and_fire_match_baseline() {
    let data = GameData::load_with_extra(
        std::env::var_os("HALCYON_DELIVERY").unwrap(),
        Some(std::path::Path::new(
            &std::env::var_os("HALCYON_DATA_EXTRA").unwrap(),
        )),
    )
    .unwrap();
    let mut world = field_replay_world(&data);
    for seat in 0..world.entities.len().min(6) {
        let actor = &mut world.entities[seat];
        actor.position = Position {
            x: seat as f32 * 0.6,
            y: 0.,
        };
        actor.max_hp = 4000.;
        actor.hp = 2000.;
        actor.energy = 500.;
        actor.skill_points = 3;
        actor.level = 6;
    }
    for actor in 1..=4 {
        let (slot, target) = if actor % 2 == 1 {
            (AbilitySlot::B, CastTarget::SelfCast)
        } else {
            (AbilitySlot::A, CastTarget::Entity(actor))
        };
        world.command(actor, Command::Learn(slot)).unwrap();
        world
            .command(actor, Command::Cast { slot, target })
            .unwrap();
    }
    let mut trace = Sha256::new();
    let mut casts = 0;
    for tick in 1..=150 {
        if tick == 20 || tick == 60 {
            world.entities[1].position.x = if tick == 20 { 9. } else { 1. };
        }
        let events = world.step();
        casts += events
            .iter()
            .filter(|event| matches!(event, Event::Cast { .. }))
            .count();
        serde_json::to_writer(&mut trace, &events).unwrap();
        trace.update(world.state_digest().as_bytes());
    }
    assert_eq!(casts, 4);
    let actual = format!("{:x}", trace.finalize());
    assert_eq!(
        actual,
        "3439a8fc70ff48cdbb44b18d3a7258cf5997a56f667e03a24b588cd209411532"
    );
}
