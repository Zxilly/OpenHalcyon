# Navmesh query and simulation measurements

The baseline was `cfdfc549901d3225c3d7ec1f719d19cc5df07742`, including first-generation Solo camps. The candidate used the same revision with changes in `crates/data/src/lib.rs`, `navmesh.rs`, `navmesh_route.rs` and `crates/sim/src/native_navigation.rs`. The candidate files have combined SHA-256 `9f4d9d994925b917da36a2a186b0e5cb3e9feb655ffbf1caa2fcdd0b2b981bc4`. To compute it, normalize line endings to LF and stream each repository-relative path in the listed order, a NUL byte, the decimal byte length, another NUL byte and the file content.

Both binaries used Rust 1.100.0, Windows release mode, thin LTO, one codegen unit, the same operator data and logical CPU 8. Five pairs alternated execution order. The [benchmark example](../crates/sim/examples/native_nav_bench.rs) warms each workload and runs 45,360 nearest queries, 15,120 routes and 40,320 segment-cover tests per mesh. Coordinates are generated deterministically and include lane, jungle and off-ground points. Timings include hashing complete results into a checksum. The source SHA-256 after LF normalization is `a5ae85f58aaa7e4c859e59113116f358ae683caaa0d5bfd6e190b4e3a92b12a1`.

| Mesh | Query | Baseline µs, median (range) | Candidate µs, median (range) |
|---|---|---:|---:|
| F002 | Nearest ground | 8.313 (7.773–9.502) | 1.728 (1.654–1.845) |
| F002 | Route | 33.632 (29.791–38.863) | 14.040 (13.572–15.362) |
| F002 | Segment coverage | 0.769 (0.724–0.823) | 0.776 (0.615–0.948) |
| ARAL | Nearest ground | 2.849 (2.612–2.996) | 0.700 (0.623–0.792) |
| ARAL | Route | 8.237 (7.621–9.876) | 3.602 (3.175–4.691) |
| ARAL | Segment coverage | 0.278 (0.269–0.363) | 0.313 (0.285–0.422) |

The index ranges over occupied columns and rows, and cell bounds skip unnecessary polygon projections. A* reuses search scratch memory in each world and precomputes immutable edge costs. The public `route` benchmark includes scratch construction. The room reuses its `RouteSearch` across replans. Short ARAL segment coverage was slower in the median, with overlapping timing ranges.

The headless Solo room ran the same seed, 10 recorded hero picks, settings, mesh and camps until a crystal was destroyed. The benchmark measures authoritative simulation without socket processing. Process CPU covers startup and all ticks. Peak working set is the Windows process high-water reading sampled while running. Every paired run ended at tick 34,679 with the same winner and digest at all 1,000-tick checkpoints and the final tick.

| Complete room | Baseline median (range) | Candidate median (range) |
|---|---:|---:|
| CPU seconds | 1.250 (1.234–1.422) | 1.094 (1.000–1.125) |
| Wall seconds | 1.325 (1.280–1.818) | 1.147 (1.090–6.541) |
| Simulation loop seconds | 1.271 (1.223–1.760) | 1.090 (1.043–1.171) |
| Peak working set, MiB | 21.148 (21.082–21.180) | 20.988 (20.984–21.207) |

The process wall range includes one long scheduling or collection outlier. Median process CPU fell 12.5%. All mesh benchmark checksums and the room trajectory checksum `e85cdaca1c918998` matched.

```powershell
cargo +1.100.0 build --release -p halcyon-sim --example native_nav_bench
target/release/examples/native_nav_bench.exe $env:HALCYON_DELIVERY $env:HALCYON_DATA_EXTRA mesh
target/release/examples/native_nav_bench.exe $env:HALCYON_DELIVERY $env:HALCYON_DATA_EXTRA room
```
