use halcyon_data::GameData;
use halcyon_sim::{World, ecs_experiment::Matrix};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    path::Path,
    sync::atomic::{AtomicU64, Ordering::Relaxed},
    time::Instant,
};
struct Count;
static BYTES: AtomicU64 = AtomicU64::new(0);
static CALLS: AtomicU64 = AtomicU64::new(0);
unsafe impl GlobalAlloc for Count {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            BYTES.fetch_add(l.size() as u64, Relaxed);
            CALLS.fetch_add(1, Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        BYTES.fetch_sub(l.size() as u64, Relaxed);
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static GLOBAL: Count = Count;
fn make(data: &GameData, solo: bool, seed: u64) -> World {
    let mut w = World::from_data(seed, 0, data).unwrap();
    if solo {
        for (i, h) in [
            "Amael", "Skye", "Inara", "Alpha", "Ardan", "Tony", "Tony", "Idris", "Varya", "Baron",
        ]
        .iter()
        .enumerate()
        {
            w.select_hero(i, h, data).unwrap();
        }
        w.use_solo_mid_lane().unwrap();
        w.install_native_navigation(data.native_maps().unwrap())
            .unwrap();
        w.install_native_camps(data).unwrap();
    }
    w
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let d = GameData::load_with_extra(&args[1], Some(Path::new(&args[2]))).unwrap();
    let iterations = args.get(3).map_or(2000, |x| x.parse().unwrap());
    println!("sizes {:?}", Matrix::sizes());
    if args.get(4).is_some_and(|a| a == "paired") {
        paired(&d, iterations);
        return;
    }
    for solo in [false, true] {
        for seed in [1, 2] {
            let mut truth = Vec::new();
            let modes: &[u8] = match args.get(4).map(String::as_str) {
                Some("brush") => &[0, 5],
                Some("persistent") => &[0, 2, 6],
                _ => &[0, 1, 2, 3, 4],
            };
            for round in 0..6 {
                for k in 0..modes.len() {
                    let m = modes[(round + k) % modes.len()];
                    let mut w = make(&d, solo, seed);
                    let mut x = Matrix::default();
                    let mut mask = Vec::with_capacity(256);
                    let mut maxmem = 0;
                    let mut alloc = 0;
                    let mut ticks = Vec::new();
                    let mut v = Vec::new();
                    let mut counts = Vec::new();
                    let mut event_hash = 0u64;
                    for tick in 0..iterations {
                        let z = Instant::now();
                        let events = w.step();
                        let elapsed = z.elapsed().as_nanos();
                        let a = BYTES.load(Relaxed);
                        let c = CALLS.load(Relaxed);
                        let t = Instant::now();
                        x.run(m, &w, black_box(&mut mask));
                        let vis = t.elapsed().as_nanos();
                        maxmem += BYTES.load(Relaxed) as i64 - a as i64;
                        alloc += CALLS.load(Relaxed) - c;
                        counts.push(mask.len());
                        v.push(vis);
                        ticks.push(elapsed + vis);
                        let mut h = 14695981039346656037u64;
                        for b in serde_json::to_vec(&events).unwrap() {
                            h = (h ^ u64::from(b)).wrapping_mul(1099511628211)
                        }
                        event_hash = event_hash.wrapping_mul(137) ^ h;
                        if round == 0 && k == 0 {
                            truth.push((mask.clone(), h));
                        } else {
                            assert_eq!(mask, truth[tick].0);
                            assert_eq!(h, truth[tick].1);
                        }
                        if w.winner.is_some() {
                            break;
                        }
                    }
                    let sum: u128 = v.iter().sum();
                    let total: u128 = ticks.iter().sum();
                    v.sort_unstable();
                    ticks.sort_unstable();
                    counts.sort_unstable();
                    println!(
                        "{} {seed} {round} {m} {} {} {} {} {} {} {} {maxmem} {alloc} {event_hash:016x} {}",
                        if solo { "solo" } else { "validation" },
                        v.len(),
                        counts[0],
                        counts[counts.len() / 2],
                        counts[counts.len() - 1],
                        sum,
                        total,
                        v[v.len() * 99 / 100],
                        w.state_digest()
                    );
                }
            }
        }
    }
}

fn paired(d: &GameData, nticks: usize) {
    let modes = [0u8, 1, 2, 3, 4, 5, 6];
    for solo in [false, true] {
        for seed in [1, 2] {
            let mut w = make(d, solo, seed);
            let mut engine: Vec<_> = modes.iter().map(|_| Matrix::default()).collect();
            let mut buf: Vec<_> = modes.iter().map(|_| Vec::with_capacity(256)).collect();
            let mut time: Vec<Vec<u128>> = (0..7).map(|_| Vec::with_capacity(nticks)).collect();
            let mut step_ns = 0;
            let mut retained = [0i64; 7];
            let mut allocations = [0u64; 7];
            for tick in 0..nticks {
                let t = Instant::now();
                black_box(w.step());
                step_ns += t.elapsed().as_nanos();
                for j in 0..7 {
                    let m = (tick + j) % 7;
                    let b = BYTES.load(Relaxed);
                    let c = CALLS.load(Relaxed);
                    let t = Instant::now();
                    engine[m].run(m as u8, black_box(&w), black_box(&mut buf[m]));
                    time[m].push(t.elapsed().as_nanos());
                    retained[m] += BYTES.load(Relaxed) as i64 - b as i64;
                    allocations[m] += CALLS.load(Relaxed) - c;
                }
                for output in &buf[1..] {
                    assert_eq!(&buf[0], output);
                }
            }
            for (m, t) in time.iter_mut().enumerate() {
                let sum: u128 = t.iter().sum();
                t.sort_unstable();
                println!(
                    "pair {} {seed} {m} {nticks} {step_ns} {sum} {} {} {} {}",
                    if solo { "solo" } else { "validation" },
                    t[t.len() * 99 / 100],
                    retained[m],
                    allocations[m],
                    w.state_digest()
                );
            }
        }
    }
}
