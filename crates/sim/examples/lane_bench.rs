//! cargo run --release -p halcyon-sim --example lane_bench -- [delivery-directory]
use halcyon_data::GameData;
use halcyon_sim::World;
use std::time::Instant;
fn main() {
    let data = std::env::args()
        .nth(1)
        .map(GameData::load)
        .transpose()
        .expect("read delivery");
    for seed in [1, 2, 3, 4] {
        let mut w = match &data {
            Some(data) => World::from_data(seed, 0, data).expect("resolve units"),
            None => World::new(seed, 0),
        };
        let mut times = Vec::new();
        let mut events = 0;
        while w.winner.is_none() && w.tick < 20 * 60 * 40 {
            let t = Instant::now();
            events += std::hint::black_box(w.step()).len();
            times.push(t.elapsed().as_nanos());
        }
        times.sort_unstable();
        println!(
            "seed={seed} ticks={} winner={:?} events={events} tick_p99_ns={} tick_max_ns={}",
            w.tick,
            w.winner,
            times[times.len() * 99 / 100],
            times.last().unwrap()
        );
        assert!(
            w.winner.is_some(),
            "40 simulated minutes elapsed without a crystal kill"
        );
    }
}
