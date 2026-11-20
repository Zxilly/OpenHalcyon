# Visibility storage experiment

The example compares the two-team visibility pass after each real simulation tick. It exercises two seeds (1 and 2) in both a validation room and a Solo room with native map data and bots. Each method produces an ordered list of gameplay actor IDs and team visibility masks. The paired run checks that all seven lists match on every tick, rotating execution order. Both archetype-based approaches use hecs 0.11.2. The persistent variant includes the cost of maintaining the mapping between stable simulation IDs and hecs handles.

Set HALCYON_DELIVERY and HALCYON_DATA_EXTRA to your existing operator data directories. From the checkout root in PowerShell 7:

```powershell
cargo run --release --locked -p halcyon-sim --features ecs-visibility-bench --example ecs_bench -- "$env:HALCYON_DELIVERY" "$env:HALCYON_DATA_EXTRA" 2400 paired > my-paired.txt
python tools/bench/ecs-visibility/summarize.py my-paired.txt
```

To reproduce the published aggregation directly, run `python tools/bench/ecs-visibility/summarize.py`. The checked-in values came from a Windows release build on one logical CPU (index 16, AMD Ryzen 9 9950X3D) with simulation baseline f3f49fdf5a0801614d76af0edc9f9777e3f8ce69. For comparable Windows timings, set CPU affinity on the process before the measured run. The host was shared.

Each result line contains scenario, seed, mode, tick count, cumulative step nanoseconds, cumulative visibility nanoseconds, run visibility p99 nanoseconds (sorted index floor(0.99 × ticks)), net retained heap bytes, allocation calls during the visibility pass and final world digest. The header gives Rust sizes in bytes for Entity, compact visibility site and combat status. The summary averages each seed's per-tick time and p99 with equal weight. Retained bytes measure live allocator-requested storage accumulated during the pass. Transient calls can occur even with zero retained bytes. Network, scheduling across rooms and mutable gameplay systems are outside the timed visibility pass.

## Bevy scheduled replication visibility

The optional `bevy-visibility-bench` feature adds a persistent Bevy ECS world with two ordered systems. The first synchronizes living actors by stable game ID and handles component updates, team changes, spawns and despawns. The second uses queries over targets and team-specific viewer archetypes to compute each team's replication visibility. Deferred commands become visible between the systems. Output uses the original actor order. Both implementations read the same world tick, geometry, brush and reveal rules. The Bevy dependency uses only the std feature, with a single-thread executor. [Bevy ECS 0.20.1 Schedule](https://docs.rs/bevy_ecs/0.20.1/bevy_ecs/schedule/struct.Schedule.html)

Across 28,800 paired ticks (two seeds per map, 2,400 ticks each, three runs), every ordered actor ID and team mask matched both the production direct scan and the compact team-vector scan. Methods rotate each tick. Median of six seed/run samples per map, on Windows, Ryzen 9 9950X3D logical CPU 16, rustc 1.97.1 release:

| Map | Method | Mean visibility µs/tick | p99 µs | Net pass heap bytes | Allocation calls per 2,400 ticks |
|---|---|---:|---:|---:|---:|
| Validation | Direct | 0.63 | 1.00 | 0 | 0 |
| Validation | Team vectors | 0.67 | 1.05 | 512 | 6 |
| Validation | Bevy | 1.36 | 2.80 | 37,459 | 452 |
| Native Solo | Direct | 32.17 | 93.35 | 0 | 23,775.5 |
| Native Solo | Team vectors | 32.85 | 106.85 | 2,048 | 23,785.5 |
| Native Solo | Bevy | 38.77 | 122.30 | 61,939 | 24,301.5 |

Net pass heap counts allocator-requested live bytes accumulated inside visibility calls. All methods preallocate a 256-row output. Bevy initialization separately retained 284,544–284,712 bytes, including its output, schedule and storage of the simulation resource. Some allocation calls in Solo arise inside shared navigation. Whole-process user + kernel CPU medians, six rotating runs each covering all four scenarios, were 335.94 ms direct, 328.13 ms team vectors and 359.38 ms Bevy (+7.0% vs direct). These include data loading, simulation steps and digest generation. Windows reported CPU in 15.625 ms increments. Pass latency samples measure elapsed wall time on the pinned core.

Persistent query handles and explicit synchronization order give the replication subsystem a place to enforce actor lifecycle and team membership. Maintaining both the authoritative simulation and its ECS projection added work on both maps. The result supports keeping the current direct visibility pass. The experiment does not measure an ECS-owned gameplay world, batching across rooms, or threaded scheduling.

To replay the benchmark, build from the checkout root, then run from an output directory in PowerShell. The script inherits the selected CPU affinity for each process.

```powershell
cargo build --release --locked -p halcyon-sim --features bevy-visibility-bench --example ecs_bench
./tools/bench/ecs-visibility/run-bevy.ps1 -Binary (Resolve-Path target/release/examples/ecs_bench.exe) -Delivery $env:HALCYON_DELIVERY -Extra $env:HALCYON_DATA_EXTRA
```

The three bevy-run text files record map, seed, method (0 direct, 1 team vectors, 2 Bevy), ticks, total visibility ns, p99 ns, net pass heap bytes, allocation calls, constructor bytes and world digest. bevy-cpu.csv contains the independent process CPU samples.
