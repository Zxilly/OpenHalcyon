# Bevy ECS room measurements

## Fixed tick, persistent effects and target memory

The [baseline](https://github.com/Zxilly/OpenHalcyon/tree/08cc383) and [combined build](https://github.com/Zxilly/OpenHalcyon/tree/510c218) played complete native Solo matches with ten Bots. Eight rounds used Rust 1.100.0 release builds on logical processor 16 of a Windows Ryzen 9 9950X3D. Each round ran four builds on both rosters. Their order rotated to balance first and last positions, and the roster order alternated. Other compilation and verification processes shared the machine. The timer covers simulated play from the first tick to a crystal defeat, excluding asset loading and network traffic.

| Build | Navigation roster, median seconds | Paired change | Lyra/Adagio roster, median seconds | Paired change |
|---|---:|---:|---:|---:|
| Baseline | 3.066 | — | 5.739 | — |
| Baseline + shared fixed-tick scope only | 2.944 | −1.86% | 5.532 | −3.26% |
| Combined production changes | 2.957 | −3.19% | 5.471 | −4.71% |
| Baseline with table-stored Bot target memory | 3.336 | +7.78% | 6.165 | +8.28% |

Paired change is the median of the eight within-round ratios. The combined build was faster in seven of eight navigation rounds and all eight Lyra/Adagio rounds. Every navigation run ended on tick 67,186 and every Lyra/Adagio run on tick 137,328. The latter produced 230 Gifts of Fire and 129 Bright Bulwarks. Within each roster all four builds agreed on the winner, the hash over 1,000-tick checkpoints and the final state digest. [All 64 samples](bevy-tick-room.csv)

- **Tick scheduling.** Six serial stages used to remove and restore the same frame and actor metadata, and re-establish the same exclusive world access. The simulation now obtains that access once for device updates, spell resolution, waves, upkeep and actor turns. The cleanup query still applies deferred despawns and the milestone system retains its tick condition. A match-ending effect still stops the turn sequence at the same point. [Fixed tick](../../crates/sim/src/fixed_tick.rs)
- **Persistent spells.** A family now removes expired handles while collecting active handles in cast order. It rechecks generations after callbacks, and newly cast spells wait until the next pass. The actor loop also skips a Twister lookup for templates that cannot own it. A separate test exercised 30,000 effect phases per run and verified the same 13,200 events and final state digest. Across ten alternating pairs the median reported block cost was 664 ns at baseline and 510.5 ns combined. The median paired change was −22.40%, and the combined run was faster in 10 of ten pairs. [Spell lifecycle](../../crates/sim/src/persistent_spells.rs) · [Phase samples](bevy-tick-effects.csv)
- **Component storage.** Removing just `#[component(storage = "SparseSet")]` from `LastSeenTarget` on the baseline produced the table-storage row above. A Bot gains and loses this component during pursuit. Sparse storage lets its main actor component stay in the same table during those changes. The room keeps a single-thread executor for ordered combat. Different rooms can run concurrently. The headless app and ECS use their `std` features. [Actor storage](../../crates/sim/src/actors.rs) · [Dependency configuration](../../Cargo.toml)
- **Replication.** Observer paths share the active-scene check, queue delivery and slow-observer removal. Callers supply their own eligibility rule and derive frames from each observer’s declared actors, projectiles and effects. The simulation benchmarks above do not exercise observer transport. [Observer delivery](../../crates/game/src/observer.rs)

We also tried fetching the ordered actor handles through one cached Bevy query per iterator on top of 510c218. The existing API still requires stable handle order, generation checks, reverse iteration and an exact remaining length. Six alternating pairs measured **+3.99%** on the navigation roster and **+1.89%** on the spell roster, with matching state traces. Production retains the direct ordered iterator. Existing unordered scans can reuse the cached table query where gameplay permits arbitrary iteration order. [Ordered-read trial samples](bevy-ordered-read.csv)

For each release checkout, build `cargo +1.100.0 build --locked --release -p halcyon-sim --example native_nav_bench`. Pin the parent process to one logical CPU, then use the comparison command at the end of this page with `--scenario room` or `--scenario room-effects`. To reproduce the isolated scope build, apply only the fixed_tick.rs change from 510c218 to 08cc383. For the phase benchmark, build each test executable with `cargo +1.100.0 test --locked --release -p halcyon-sim --lib --no-run` in separate target directories. Run the comparison command with those executables and `--scenario phase --pairs 10`. The runner checks that the binary hashes differ before sampling. The verified phase hashes are `e1f74cb23ef29e240af61c696d7ade4ac66c763c456323175bbc990995146b12` (baseline) and `a9c9df5449e3d62583bb2d25d20b1800c9ab2609774c79dc7a285dbfa4dc3b9b` (combined). Historical migration timings below use different rules and match lengths.

## Authoritative room migration

Seven rounds on logical processor 16 of a Windows Ryzen 9 9950X3D used Rust 1.97.1 release builds with identical benchmark source, data, seed and roster. Builds rotated within each workload. Other processes were active on the host. The [original room](https://github.com/Zxilly/OpenHalcyon/tree/321e354), [initial Bevy migration](https://github.com/Zxilly/OpenHalcyon/tree/ba98acb) and [optimized room](https://github.com/Zxilly/OpenHalcyon/tree/913ad9a) ran all three scenarios.

| Scenario, median elapsed | Original | Initial Bevy | Optimized Bevy |
|---|---:|---:|---:|
| Complete native Solo Bot match | 1.403 s | 2.782 s | 2.260 s |
| Complete native Solo Bot match with Lyra and Adagio | 2.306 s | 3.810 s | 3.166 s |
| Two-team visibility across twelve frozen rooms | 244 ms | 469 ms | 519 ms |

Every ordinary match ended at tick 35,445 with team 1 winning. Every Lyra/Adagio match ended at tick 52,648 with team 0 winning, 95 Gifts of Fire and 56 Bright Bulwarks. Checkpoint traces and final state digests agreed in every run. Every visibility run returned 13,840,000 positives.

Both complete matches ran faster in all seven rounds after adding the actor ID index and using a scheduled query to remove defeated units. An additional [index-only build](https://github.com/Zxilly/OpenHalcyon/tree/8b2bf13) took 3.024 s in the Lyra/Adagio scenario. Adding the removal query brought that to 3.166 s and took longer in five of the seven rounds. The query places despawning and index maintenance together in the fixed schedule. The isolated visibility scan also took longer in the optimized build. The [70 individual measurements](bevy-authoritative-room.csv) include process CPU time for the whole invocation. The CSV labels the builds `orig`, `full`, `id` and `combined` respectively for the original, initial Bevy, index-only and optimized variants.

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
