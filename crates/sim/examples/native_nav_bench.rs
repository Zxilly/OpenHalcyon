use halcyon_data::{BotDifficulty, GameData, NavMesh};
use halcyon_sim::{AbilitySlot, Event, World};
use std::{hint::black_box, path::Path, time::Instant};

fn mix(hash: &mut u64, bits: u32) {
    *hash = (*hash ^ u64::from(bits)).wrapping_mul(1_099_511_628_211);
}
fn run(mesh: &NavMesh, work: &str, reps: usize) {
    // Deterministic mix of central lanes, side terrain, and off-ground queries.
    let corners = [
        [-90., 0.],
        [90., 0.],
        [-78.18, 0.88],
        [79.66, -0.91],
        [-68., 12.93],
        [68., 12.93],
        [-34.21, 14.71],
        [34.47, 18.42],
        [0., 0.],
        [0., 40.],
        [-24., -44.],
        [24., 44.],
    ];
    let inputs: Vec<_> = (0..252)
        .map(|i| {
            let coord = |j: usize| {
                [
                    corners[j % 12][0] + (((i * 193 + j * 33) % 31) as f32 - 15.) * 0.27,
                    corners[j % 12][1] + (((i * 83 + j * 47) % 31) as f32 - 15.) * 0.31,
                ]
            };
            (coord(i * 7), coord(i * 13 + 4))
        })
        .collect();
    let mut digest = 0;
    let start = Instant::now();
    for _ in 0..reps {
        for &(a, b) in black_box(&inputs) {
            match work {
                "nearest" => {
                    if let Some(p) = black_box(mesh.nearest_ground(a, 8.)) {
                        for x in p {
                            mix(&mut digest, x.to_bits());
                        }
                    } else {
                        mix(&mut digest, u32::MAX);
                    }
                }
                "route" => {
                    if let Some(points) = black_box(mesh.route(a, b, 8.)) {
                        mix(&mut digest, points.len() as u32);
                        for p in points {
                            for x in p {
                                mix(&mut digest, x.to_bits());
                            }
                        }
                    } else {
                        mix(&mut digest, u32::MAX);
                    }
                }
                "segment" => mix(
                    &mut digest,
                    u32::from(black_box(mesh.ground_segment(a, [a[0] + 1.5, a[1] + 1.5]))),
                ),
                _ => panic!("unknown workload"),
            }
        }
    }
    println!(
        "mesh,{work},{},{:.3},{digest:016x}",
        inputs.len() * reps,
        start.elapsed().as_secs_f64() * 1e9 / (inputs.len() * reps) as f64
    );
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Roster {
    Navigation,
    Effects,
}

fn match_run(data: &GameData, roster: Roster) {
    let mut world = World::from_data(0, 0, data).unwrap();
    let mut teams = [
        ["Amael", "Inara", "Ardan", "Tony", "Varya"],
        ["Skye", "Alpha", "Tony", "Idris", "Baron"],
    ];
    if roster == Roster::Effects {
        teams[0][0] = "Adagio";
        teams[1][0] = "Lyra";
    }
    for i in 0..10 {
        world.select_hero(i, teams[i % 2][i / 2], data).unwrap();
    }
    world.use_solo_mid_lane().unwrap();
    world
        .install_native_navigation(data.native_maps().unwrap())
        .unwrap();
    world.install_native_camps(data).unwrap();
    world.set_bot_difficulty(Some(BotDifficulty::Medium));
    let mut digest = String::new();
    let adagio = world.entities[0].id;
    let lyra = world.entities[1].id;
    let (mut gifts, mut bulwarks) = (0, 0);
    let start = Instant::now();
    while world.tick < 200_000 && world.winner.is_none() {
        let events = black_box(world.step());
        if roster == Roster::Effects {
            for event in events {
                match event {
                    Event::Cast {
                        source,
                        slot: AbilitySlot::A,
                        ..
                    } if source == adagio => gifts += 1,
                    Event::Cast {
                        source,
                        slot: AbilitySlot::B,
                        ..
                    } if source == lyra => bulwarks += 1,
                    _ => {}
                }
            }
        }
        if world.tick.is_multiple_of(1000) {
            digest.push_str(&world.state_digest());
        }
    }
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut path = DefaultHasher::new();
    digest.hash(&mut path);
    let travel = path.finish();
    println!(
        "room,{},{:?},{:.6},{travel:016x},{}",
        world.tick,
        world.winner,
        start.elapsed().as_secs_f64(),
        world.state_digest()
    );
    assert!(world.winner.is_some());
    if roster == Roster::Effects {
        assert!(
            gifts > 0 && bulwarks > 0,
            "expected both effect families to be exercised"
        );
        println!("effect_casts,gift_of_fire,{gifts},bright_bulwark,{bulwarks}");
    }
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let data = GameData::load_with_extra(&args[1], Some(Path::new(&args[2]))).unwrap();
    match args[3].as_str() {
        "room" => return match_run(&data, Roster::Navigation),
        "room-effects" => return match_run(&data, Roster::Effects),
        _ => {}
    }
    let maps = data.native_maps().unwrap();
    for layout in [maps.solo().unwrap(), maps.aral().unwrap()] {
        let mesh = maps.mesh(&layout.navmesh).unwrap().unwrap();
        println!("layout,{}", layout.level_id);
        for (task, reps) in [("nearest", 180), ("route", 60), ("segment", 160)] {
            run(&mesh, task, 3);
            run(&mesh, task, reps);
        }
    }
}
