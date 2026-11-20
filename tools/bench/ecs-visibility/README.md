# Visibility storage experiment

The example compares the two-team visibility pass after each real simulation tick. It exercises two seeds (1 and 2) in both a validation room and a Solo room with native map data and bots. Each method produces an ordered list of gameplay actor IDs and team visibility masks. The paired run checks that all seven lists match on every tick, rotating execution order. Both archetype-based approaches use hecs 0.11.2. The persistent variant includes the cost of maintaining the mapping between stable simulation IDs and hecs handles.

Set HALCYON_DELIVERY and HALCYON_DATA_EXTRA to your existing operator data directories. From the checkout root in PowerShell 7:

```powershell
cargo run --release --locked -p halcyon-sim --features ecs-visibility-bench --example ecs_bench -- "$env:HALCYON_DELIVERY" "$env:HALCYON_DATA_EXTRA" 2400 paired > my-paired.txt
python tools/bench/ecs-visibility/summarize.py my-paired.txt
```

To reproduce the published aggregation directly, run `python tools/bench/ecs-visibility/summarize.py`. The checked-in values came from a Windows release build on one logical CPU (index 16, AMD Ryzen 9 9950X3D) with simulation baseline f3f49fdf5a0801614d76af0edc9f9777e3f8ce69. For comparable Windows timings, set CPU affinity on the process before the measured run. The host was shared.

Each result line contains scenario, seed, mode, tick count, cumulative step nanoseconds, cumulative visibility nanoseconds, run visibility p99 nanoseconds (sorted index floor(0.99 × ticks)), net retained heap bytes, allocation calls during the visibility pass and final world digest. The header gives Rust sizes in bytes for Entity, compact visibility site and combat status. The summary averages each seed's per-tick time and p99 with equal weight. Retained bytes measure live allocator-requested storage accumulated during the pass. Transient calls can occur even with zero retained bytes. Network, scheduling across rooms and mutable gameplay systems are outside the timed visibility pass.
