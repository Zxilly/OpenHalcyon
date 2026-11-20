use super::*;
use std::{hint::black_box, path::Path, time::Instant};

#[test]
#[ignore = "release microbenchmark with original delivery and supplemental data"]
fn effect_phase_cost() {
    let data = GameData::load_with_extra(
        std::env::var_os("HALCYON_DELIVERY").unwrap(),
        Some(Path::new(&std::env::var_os("HALCYON_DATA_EXTRA").unwrap())),
    )
    .unwrap();
    let mut w = World::from_data(23, 6, &data).unwrap();
    w.use_six_human_roster().unwrap();
    for (i, name) in [(0, "Lyra"), (1, "Adagio"), (2, "Lyra"), (3, "Adagio")] {
        w.select_hero(i, name, &data).unwrap();
    }
    w.use_aral_lane().unwrap();
    for i in 0..w.entities.len().min(6) {
        let a = &mut w.entities[i];
        a.position = Position {
            x: i as f32 * 0.6,
            y: 0.,
        };
        a.max_hp = 4000.;
        a.hp = 2000.;
    }
    let mut events = vec![];
    let mut costs = Vec::new();
    let mut event_count = 0u64;
    for block in 0..25 {
        let mut ns = 0;
        for index in 0..1200 {
            w.tick += 1;
            if index % 100 == 0 {
                for idx in 0..w.entities.len().min(6) {
                    let a = &mut w.entities[idx];
                    a.hp = 2000.;
                }
                w.gift_of_fire(1, 1, 0, &mut events);
                w.gift_of_fire(3, 3, 0, &mut events);
                w.create_bulwark(0, 0, &mut events);
                w.create_bulwark(2, 0, &mut events);
            }
            let t = Instant::now();
            w.advance_persistent_spells(&mut events);
            ns += t.elapsed().as_nanos();
            event_count += black_box(events.len()) as u64;
            events.clear();
        }
        if block > 0 {
            costs.push(ns / 1200);
        }
    }
    assert_eq!(event_count, 13_200);
    assert_eq!(
        w.state_digest(),
        "ca376a3efa419bf364c4d5a2cb5ebf23d9ef1f99f74ced77908fde8fbb9f63c8"
    );
    costs.sort_unstable();
    println!(
        "phase median_block_ns {} min {} max {} events {} final {}",
        costs[12],
        costs[0],
        costs[23],
        event_count,
        w.state_digest()
    );
}
