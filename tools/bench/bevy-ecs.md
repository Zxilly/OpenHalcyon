# Bevy ECS room measurements

## Authoritative room migration

The original [vector-based run](https://github.com/Zxilly/OpenHalcyon/tree/321e354) and the [production Bevy room](https://github.com/Zxilly/OpenHalcyon/tree/ba98acb) used the same benchmark sources, release profile, Rust 1.97.1, installed data and seed. On a Windows Ryzen 9 9950X3D, both builds ran on logical processor 16. Seven pairs alternated which build ran first. Other processes were active on the host.

| Scenario | Original median | Bevy median | Median change within pairs |
|---|---:|---:|---:|
| Complete native Solo Bot match | 1.979 s | 2.793 s | +37% |
| Complete native Solo Bot match with Lyra and Adagio | 2.891 s | 4.332 s | +48% |
| Two-team visibility across twelve frozen rooms | 311 ms | 640 ms | +98% |

Every ordinary match ended at tick 35,445 with team 1 winning. Every Lyra/Adagio match ended at tick 52,648 with team 0 winning, 95 Gifts of Fire and 56 Bright Bulwarks. The checkpoint traces and final state digests matched for each pair, and each visibility run returned 13,840,000 positives. The extra ECS work increased elapsed simulation time per match on this machine. For timings and dispersion, see the [42 individual runs](bevy-authoritative-room.csv).

Build the unchanged `native_nav_bench` and `visibility_bench` examples in each checkout with `cargo build --locked --release -p halcyon-sim --examples`. Run `native_nav_bench DELIVERY EXTRA room`, `native_nav_bench DELIVERY EXTRA room-effects` and `visibility_bench` once per build in each pair. The native room timers exclude pack loading.

## Historical Bevy ECS trials

The two experiments exercise different ownership models. The replication experiment copies visible actor properties into Bevy. The combat experiment makes Bevy the owner of two persistent spell types, Lyra's Bright Bulwark and Adagio's Arcane Fire, and runs their systems inside the existing room tick. Both use standalone `bevy_ecs` 0.20.1 with its `std` feature.

### Replication visibility

Each implementation produced the same ordered actor IDs and two-team visibility masks on all 28,800 ticks. The following values are for Solo on one logical CPU. The replication experiment includes both the system that synchronizes actor changes and the system that computes the snapshot.

| Visibility implementation | Mean per tick | p99 |
| --- | ---: | ---: |
| Direct scan of authoritative actors | 32.17 µs | 93.35 µs |
| Bevy component queries after synchronizing actors | 38.77 µs | 122.30 µs |

The direct scan reads the existing room state and has no second actor lifecycle to maintain. That direct scan was the production implementation at the time of the experiment. Raw samples, verification and reproduction commands are available on the [visibility benchmark branch](https://github.com/Zxilly/OpenHalcyon/tree/b3150179d647ea35e39cf0871830d289b35ca102/tools/bench/ecs-visibility).

### Authoritative spell effects

The [runnable server branch](https://github.com/Zxilly/OpenHalcyon/tree/7a377c48eed4bb9d5e7a13572feaa2875b8e557d) is based on [the measured main implementation](https://github.com/Zxilly/OpenHalcyon/tree/2175f76f0538078bd2d6df3984d105b72aee2297). The branch builds the ordinary server with the same configuration. A room creates its effect world on first use. Each spell gets a component and a cast ordinal. Ordered jobs let one spell kill, heal or replace an entity before the next impact. A generational handle prevents a stale result from overwriting a spell that was replaced during the callback.

The `room-effects` scenario runs ten Bots on the native Solo map with the installed supplemental data. It keeps the seed, initial roster, navigation, difficulty, AI policy and rules identical between the two builds. Adagio and Lyra play on opposite teams. Each run checks that both abilities actually fired and that one side destroyed the enemy crystal. The comparison also checks the periodic checkpoint trace, final state digest and cast counts.

Every measured game ended at tick 52,648 with team 0 winning, 95 Gifts of Fire and 56 Bright Bulwarks. All games matched the same checkpoint trace and final state digest. An independent 150-tick encounter with two casters of each hero matched the baseline's ordered events and complete state on every tick, including movement across a field boundary.

A release microbenchmark on one logical CPU of a Ryzen 9 9950X3D exercised 30,000 effect phases and 13,200 events per run. Across nine alternating runs per build, the median of the run medians was 121 ns for the two vectors and 623 ns for the Bevy implementation. The measured cost difference is roughly 0.5 µs per phase for this workload. End-to-end wall times are more sensitive to other processes. Neither benchmark measures how many networked players the server can host.

In the [full-match comparison](bevy-room-effects.csv), both builds used logical CPU 14 on the same Windows host. During eleven alternating pairs, other build and test processes were also running. The baseline wall-time median was 2.165 s and Bevy's was 2.270 s. Within individual pairs the change ranged from −35% to +39%, so these wall-time results do not establish a whole-match speed difference.

The pilot separates expiry and impact resolution and gives both effects a common lifecycle. It also introduces a second entity world alongside the authoritative actors, an ordered work list and rules for writing results back across those worlds. The initial comparison retained the typed vectors on main. The production migration subsequently put actors, projectiles, persistent effects and vision devices in one room world. The recorded digests and fixtures provide a baseline for that migration.

Build `native_nav_bench` in release mode in each checkout with separate target directories. The comparison runner warms both binaries, alternates which runs first and writes a row per measured match. The seconds column measures the loop inside the server simulation. It excludes static data loading. For single-core measurement, restrict the parent process to one logical CPU before launching the runner.

```text
cargo build --release --locked -p halcyon-sim --example native_nav_bench
python tools/bench/compare-room-effects.py BASELINE_EXE CANDIDATE_EXE DELIVERY_DIR SERVER_DATA_EXTRA --pairs 7
```
