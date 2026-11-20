# Native room load across the Bevy migration

Each release hosted 32 simultaneous rooms on one logical CPU under a 1 GiB Windows Job commit limit. Solo used 32 synthetic native clients and 288 server-controlled heroes. ARAL used 192 native clients and zero bots. Clients authenticated, entered the native game channel, confirmed the scene, sent movement and clocks, and continuously decrypted server frames. These two-minute measurements cover admission and early combat. Sustainable late-game load and maximum room count require separate measurements.

| Mode and revision | Active room-time | CPU, one core | Peak sampled RSS (MiB) | Mean room work (µs/tick) | Worst sampled room p99 (ms) | Ticks >40 ms / measured |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Solo, pre-Bevy | 100.0% | 9.32% | 42.30 | 91.57 | 17.803 | 0 / 77,381 |
| Solo, intermediate | 100.0% | 14.41% | 82.33 | 170.58 | 19.567 | 0 / 77,444 |
| Solo, final | 100.0% | 11.27% | 80.02 | 117.42 | 17.893 | 0 / 77,426 |
| ARAL, pre-Bevy | 100.0% | 23.48% | 43.14 | 48.25 | 27.343 | 0 / 77,605 |
| ARAL, final | 100.0% | 22.16% | 81.43 | 64.07 | 121.918 | 0 / 77,532 |

The p99 column takes the highest sampled 1,024-tick room-window p99. The deadline column counts ticks across all rooms in the measured interval. Room work excludes wake latency and lock wait. CPU is the server process CPU time divided by elapsed time. RSS is sampled working set.

| Mode and revision | Frames received | GS bytes | Damage frames | Hero kill increments received | Clock echoes / sent | Sequence errors | Slow disconnects |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Solo, pre-Bevy | 2,595,737 | 48,929,138 | 71,045 | 31 | 1,921 / 1,921 | 0 | 0 |
| Solo, intermediate | 2,659,029 | 50,200,162 | 72,150 | 18 | 1,952 / 1,952 | 0 | 0 |
| Solo, final | 2,683,353 | 50,598,794 | 73,154 | 24 | 1,952 / 1,952 | 0 | 0 |
| ARAL, pre-Bevy | 4,725,502 | 94,952,676 | 119,262 | 0 | 11,514 / 11,514 | 0 | 0 |
| ARAL, final | 4,619,677 | 92,669,826 | 117,435 | 0 | 11,520 / 11,520 | 0 | 0 |

Client counts are last-minus-first steady samples. A clock sent near a sampling boundary can be echoed in the following interval. Hero kill increments are messages received per seat. Solo clients issued movement and clocks, so the kills also exercise server-controlled combat. The workload uses ordinary room seeds and real visibility filtering, which vary the number of frames per room.

The final Solo run changed CPU by +1.96 percentage points, working set by +37.71 MiB and mean room work by +25.85 µs/tick relative to the pre-Bevy release.

The final ARAL run changed CPU by -1.32 percentage points, working set by +38.29 MiB and mean room work by +15.82 µs/tick relative to the pre-Bevy release.

| Mode and revision | Server CPU (µs/decoded frame) | Dominant stage of >40 ms ticks (wake / lock / work) | Peak room fanout queue-full count |
| --- | ---: | ---: | ---: |
| Solo, pre-Bevy | 4.340 | 0 / 0 / 0 | unreported |
| Solo, intermediate | 6.558 | 0 / 0 / 0 | unreported |
| Solo, final | 5.083 | 0 / 0 / 0 | unreported |
| ARAL, pre-Bevy | 6.024 | 0 / 0 / 0 | unreported |
| ARAL, final | 5.811 | 0 / 0 / 0 | unreported |

CPU per decoded frame divides total server CPU by received frame count and includes simulation, replication and service work. The room overview did not report its queue-full counter in these samples. Client slow disconnects and clock sequence errors are measured separately.

| Mode and revision | Ticks >40 ms accumulated by first steady sample / ticks | Dominant stage (wake / lock / work) | Worst room p99 at last sample (ms) |
| --- | ---: | ---: | ---: |
| Solo, pre-Bevy | 0 / 16,390 | 0 / 0 / 0 | 16.372 |
| Solo, intermediate | 0 / 16,383 | 0 / 0 / 0 | 16.367 |
| Solo, final | 0 / 16,356 | 0 / 0 / 0 | 16.003 |
| ARAL, pre-Bevy | 0 / 19,460 | 0 / 0 / 0 | 15.484 |
| ARAL, final | 129 / 17,112 | 129 / 0 / 0 | 15.452 |

The final ARAL run accumulated 129 late ticks by the first steady sample, all wake-delay dominant. The first rolling room windows retained that startup delay. The following 77,532 measured ticks added no misses; the worst final room p99 was 15.452 ms. The stage counters locate the delay before room work begins. They do not identify the cause of the delayed wake.


## Current Solo comparison after fixed-tick cleanup

Two clean Windows Rust 1.100.0 releases ran serially on the same Ryzen 9 9950X3D, using the loopback harness below and the same load-client executable. Each hosted 32 native Solo rooms (32 client seats, 288 server Bots), using the full delivery and supplemental data. The server used logical CPU 14 with one Tokio worker and a 1 GiB process commit limit. The client used CPUs 26, 28 and 30. The ramp was 45 seconds and the requested steady interval was 60 seconds. Sampled intervals spanned 65.48 and 65.50 seconds.

| Build | Mean work per room tick | Server CPU, one core | Peak sampled working set | Highest sampled room-window p99 | Ticks over 40 ms | Steady clock echoes / sends |
|---|---:|---:|---:|---:|---:|---:|
| 08cc383 | 87.289 µs | 10.02% | 64.50 MiB | 16.979 ms | 0 / 41,906 | 992 / 992 |
| 510c218 | 84.994 µs | 8.78% | 51.93 MiB | 16.271 ms | 0 / 41,917 | 992 / 992 |

Both had 100% sampled active room-time, zero slow-client disconnects and zero clock sequence errors. The host chose independent room seeds, producing 1,176,454 versus 1,167,043 decoded frames. This one pair measured 2.63% less mean room work at this load. Its CPU, sampled memory and latency changes need more independent trials before extrapolation to machine capacity. The [paired deterministic room measurements](bevy-ecs.md#fixed-tick-persistent-effects-and-target-memory) isolate the tick and component-storage changes.

Server executable SHA-256 was `ea758a52f1de4ca2e174a278fe2cc8c7d75c82665968b79b892a073e3af3a257` at 08cc383 and `f7996cee70bd0ab732271ee9aeef9350738b67152d4a921ae27edcc8457ca2b5` at 510c218. Both used the 08cc383 driver with SHA-256 `928dd8074e8318c16e0f5b333bd70f83e7a1130116891eeff8ae2adbfe46507f`. Use the Solo reproduction command below with `--seconds 60`.

## Interleaved 32-room and room-scaling check (`4bc4da6`)

Eight quiet-machine trials ran serially on Windows 11 (`AMD Ryzen 9 9950X3D`) using release binaries from commit `4bc4da63e9eef49b058eccd2b5cecf0a268e6d74` (`openhalcyon.exe` SHA-256 `88f54e83071668147044537eef0a4c11d416ef3aca53a28625020a4e2cb59f25`, `halcyon-loadtest.exe` SHA-256 `6b06ab68a29e5f20627588ea944e2292c0dd163f86dd6261632cdb8983d15a0c`). Every run used full delivery and supplemental data, one Tokio worker pinned to logical CPU `14`, a `1 GiB` Job commit limit, three client workers on CPUs `26`, `28`, and `30`, a `45 s` login ramp, and a `120 s` steady window (`--room-capacity` = rooms + 1). Two interleaved 32-room rounds ran first, followed by single-variable room scaling to `48` and `64` rooms for both native Solo (`1` human + `9` bots per room) and native 6-seat ARAL (`6` humans + `0` bots per room).

| Run | Mode & rooms | Clients / bots | Active room-time | CPU, one core (60 s max) | Peak RSS (MiB) | Mean `roomWork` / `wakeDelay` / `lockWait` (µs/tick) | Worst window / last-sample p99 (ms) | Ticks >40 ms (startup / steady) |
| --- | --- | ---: | ---: | ---: | ---: | --- | ---: | ---: |
| Solo 32 r1 | Solo, 32 | 32 / 288 | 100.0% | 10.71% (11.69%) | 56.41 | 111.73 / 7,565.26 / 0.07 | 15.982 / 15.714 | 0 / 16,382 · 0 / 77,339 |
| ARAL 32 r1 | ARAL, 32 | 192 / 0 | 100.0% | 16.46% (17.31%) | 57.01 | 41.03 / 6,554.86 / 0.08 | 16.112 / 15.546 | 0 / 17,367 · 0 / 77,438 |
| Solo 32 r2 | Solo, 32 | 32 / 288 | 100.0% | 12.38% (14.10%) | 56.22 | 113.02 / 7,540.03 / 0.07 | 16.391 / 15.779 | 0 / 16,428 · 0 / 77,403 |
| ARAL 32 r2 | ARAL, 32 | 192 / 0 | 100.0% | 18.15% (19.58%) | 56.64 | 42.04 / 6,447.68 / 0.08 | 15.409 / 15.259 | 0 / 17,252 · 0 / 77,448 |
| Solo 48 | Solo, 48 | 48 / 432 | 100.0% | 14.77% (16.48%) | 67.89 | 100.90 / 7,247.35 / 0.07 | 17.646 / 15.567 | 0 / 24,832 · 0 / 116,184 |
| ARAL 48 | ARAL, 48 | 288 / 0 | 100.0% | 25.03% (27.38%) | 70.79 | 35.96 / 5,860.14 / 0.08 | 15.720 / 15.120 | 0 / 21,588 · 0 / 116,367 |
| Solo 64 | Solo, 64 | 64 / 576 | 100.0% | 19.27% (21.75%) | 78.64 | 97.72 / 7,062.01 / 0.07 | 16.156 / 15.117 | 0 / 32,986 · 0 / 154,984 |
| ARAL 64 | ARAL, 64 | 384 / 0 | 100.0% | 32.98% (33.38%) | 84.77 | 35.82 / 5,029.58 / 0.09 | 15.072 / 13.726 | 0 / 36,931 · 0 / 155,211 |

| Run | Decoded GS frames | Server CPU (µs/frame) | Dominant >40 ms (wake / lock / work) | Peak `fanoutQueueFull` | Steady clock echoes / sent | Total clock echoes / sent | Sequence errors | Slow disconnects | 120 s tier status |
| --- | ---: | ---: | --- | ---: | ---: | ---: | ---: | ---: | --- |
| Solo 32 r1 | 2,658,175 | 4.867 | 0 / 0 / 0 | 0 | 1,920 / 1,920 | 2,298 / 2,298 | 0 | 0 | Pass |
| ARAL 32 r1 | 4,587,220 | 4.343 | 0 / 0 / 0 | 0 | 11,328 / 11,328 | 13,806 / 13,806 | 0 | 0 | Pass |
| Solo 32 r2 | 2,687,783 | 5.569 | 0 / 0 / 0 | 0 | 1,951 / 1,951 | 2,328 / 2,328 | 0 | 0 | Pass |
| ARAL 32 r2 | 4,574,406 | 4.803 | 0 / 0 / 0 | 0 | 11,328 / 11,328 | 13,794 / 13,794 | 0 | 0 | Pass |
| Solo 48 | 4,098,411 | 4.361 | 0 / 0 / 0 | 0 | 2,928 / 2,928 | 3,494 / 3,494 | 0 | 0 | Pass |
| ARAL 48 | 6,854,445 | 4.427 | 0 / 0 / 0 | 0 | 17,280 / 17,286 | 20,232 / 20,238 | 0 | 0 | Pass |
| Solo 64 | 5,410,415 | 4.312 | 0 / 0 / 0 | 0 | 3,904 / 3,904 | 4,663 / 4,663 | 0 | 0 | Pass |
| ARAL 64 | 9,490,560 | 4.215 | 0 / 0 / 0 | 0 | 23,424 / 23,430 | 28,698 / 28,704 | 0 | 0 | Pass |

Every trial passed all `120 s` steady regression and scaling criteria (CPU <= 75%, RSS <= 768 MiB, worst room-window P99 <= 40 ms, >40 ms <= 1%, zero `fanoutQueueFull`, zero clock sequence errors, and zero slow disconnects) up to **64 native Solo rooms** (64 humans + 576 bots) and **64 native 6-seat ARAL rooms** (384 humans). Formal `30 min` and `2 hr` critical endurance acceptance tiers remain separate long-duration runs.

## Reproduction

Tests ran serially on Windows on a Ryzen 9 9950X3D, one server worker on logical CPU 14 and a separate load process on CPUs 26, 28 and 30. Every trial used a fresh instance and dynamic loopback ports, full base and supplementary game data, a 33-room configured cap, a 45-second login ramp and the standard host admission policy. Other processes shared the physical machine. The final release compilation overlapped the intermediate Solo trial on different logical CPUs. The intermediate run was already underway when the final candidate became available.

| Release | Git revision | Server executable SHA-256 |
| --- | --- | --- |
| Pre-Bevy | `2175f76f0538078bd2d6df3984d105b72aee2297` | `19adc29a75e924d068f4f1e91442e33b6d82ed62cd59ddff17e5960a61658e63` |
| Intermediate | `ba98acb1f589c42183181bf0cc50161671e34471` | `d9014da044495edf9a0dded8f2315a65c952e0b7bd2a709588d20db0d3e5f227` |
| Final | `913ad9aaa6f5f00478a80b37df1b4f67def9b160` | `ef330eee8c27dc7963dbbd2905e49bf594685aedc140d112bd47055381de04db` |

In each clean release checkout, build with `cargo build --release --locked -p openhalcyon -p halcyon-loadtest`. Place that revision’s server executable and the same pre-Bevy driver in a fresh binary directory. All measured trials used load driver SHA-256 `6fcea310b9606bdcf7fbefff48fe02d15ae144c040b8d1107d537734d6f5d860`. Set the data, binary and fresh output directories in the environment, then run each workload serially.

```powershell
$common = @('--rooms','32','--room-capacity','33','--seconds','120','--ramp-seconds','45',
            '--host-telemetry','--server-cpus','14','--client-cpus','26','28','30',
            '--data-dir',$env:HALCYON_DELIVERY,'--data-extra-dir',$env:HALCYON_DATA_EXTRA,
            '--binary-dir',$env:HALCYON_BENCH_BIN)
python tools/bench/windows.py --native-solo --humans 1 @common --output (Join-Path $env:HALCYON_BENCH_OUTPUT 'solo')
python tools/bench/windows.py --native-aral --humans 6 @common --output (Join-Path $env:HALCYON_BENCH_OUTPUT 'aral')
```
