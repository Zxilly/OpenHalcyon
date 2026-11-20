use halcyon_data::GameData;
use halcyon_sim::{World, WorldRules};
use std::{hint::black_box, path::Path, time::Instant};

fn sample(rooms: u64, mut create: impl FnMut(u64) -> World) -> u128 {
    let start = Instant::now();
    for seed in 0..rooms {
        black_box(create(seed));
    }
    start.elapsed().as_nanos() / u128::from(rooms)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let delivery = args.next().ok_or("expected delivery directory")?;
    let extra = args.next();
    let data = GameData::load_with_extra(delivery, extra.as_deref().map(Path::new))?;
    let rules = WorldRules::load(&data)?;
    assert_eq!(
        World::from_data(0, 6, &data)?.state_digest(),
        rules.world(0, 6).state_digest()
    );
    let fresh = |seed| World::from_data(seed, 6, &data).expect("validated rules");
    let shared = |seed| rules.world(seed, 6);
    let rooms = 128;
    println!("round,rooms,recompile_ns_per_room,shared_ns_per_room");
    for round in 0..7 {
        let (cold, warm) = if round % 2 == 0 {
            (sample(rooms, fresh), sample(rooms, shared))
        } else {
            let warm = sample(rooms, shared);
            (sample(rooms, fresh), warm)
        };
        println!("{round},{rooms},{cold},{warm}");
    }
    Ok(())
}
