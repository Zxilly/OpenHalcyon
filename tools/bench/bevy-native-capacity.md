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
