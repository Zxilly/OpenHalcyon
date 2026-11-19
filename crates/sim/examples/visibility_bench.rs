//! cargo run --release -p halcyon-sim --example visibility_bench
use halcyon_sim::World;
use std::{hint::black_box, time::Instant};

fn main() {
    let snapshots: Vec<_> = (0..12)
        .map(|i| {
            let mut world = World::new(1 + i % 4, 0);
            for _ in 0..(300 + 240 * i) {
                world.step();
            }
            world
        })
        .collect();
    let start = Instant::now();
    let mut visible = 0u64;
    for _ in 0..40_000 {
        for world in black_box(&snapshots) {
            for entity in &world.entities {
                for team in 0..2 {
                    visible += u64::from(world.visible(team, entity));
                }
            }
        }
    }
    println!("ns={} visible={visible}", start.elapsed().as_nanos());
}
