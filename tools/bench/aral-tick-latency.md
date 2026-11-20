# ARAL tick tail attribution

The per-room admin summary at `/api/admin/v1/matches` exposes `tickLatency.stages`. The reported timings are elapsed wall time in three consecutive segments.

| Stage | Boundaries |
|---|---|
| `wakeDelay` | Scheduled interval deadline to the beginning of its task branch |
| `lockWait` | Task branch entry to acquisition of the room mutex |
| `roomWork` | Lock acquisition to completion of the room tick, including simulation and fanout enqueue |

Each stage has a lifetime sum, maximum and count above 40 ms. Each tick exceeding 40 ms also contributes one `dominantOver40ms` sample to its longest stage. Ties go to the earlier stage. Stage sums equal the total measured duration. The benchmark samples cumulative counters at the boundaries of its steady window and subtracts them. Its maximum fields still describe the room's lifetime through the final sample.

`wakeDelay` includes timer delivery, Tokio task scheduling, earlier work on the same runtime and host scheduler effects. The stage alone cannot assign those contributions. `roomWork` can likewise include thread preemption and covers broadcast enqueue, while socket write completion happens asynchronously.

The room interval retains burst catch-up. A single paused window can therefore contribute several late ticks per room. A count of late ticks describes the room deadlines affected and does not establish how many scheduler interruptions occurred.

The historical slow samples clustered across rooms. The `5de47c9` run reported 1,291 late ticks in six five-second sampling windows, with a peak window of 498 across all 32 rooms. The `d38ac80` run reported 690 in three sampled windows, with a peak of 537 across all 32 rooms. The two `d38ac80` comparisons recorded zero late ticks. The historical total combined all three stages.

## Instrumented live run

The server was built from `6171282f8e221e258ed9280989f7e63a3bfb83ef` with the stage instrumentation. Its binary SHA-256 was `ee332401980773f58d4f6effb46fe30f00dcef98166d3ae07c4576f93ee3a5f2`. The workload used the local base and supplemental data, 32 six-seat ARAL rooms, one server Tokio worker on logical CPU 14, three client workers on CPUs 26, 28 and 30, and the 1 GiB process commit limit. Two 45-second ramp attempts stopped before steady state with CE `-3` during `update` and `acceptMatch`. The repeat extended the ramp to 90 seconds and held the same active width.

The 121.079-second steady window measured 77,484 ticks with no samples over 40 ms. Average process CPU was 18.58%, maximum rolling minute CPU was 19.16%, and peak sampled RSS was 42.67 MiB. The largest sampled room p99 was 18.046 ms and the largest tick since opening was 31.865 ms. All 192 seats were connected, with no slow disconnects or clock sequence errors.

| Stage | Steady mean | Lifetime maximum |
|---|---:|---:|
| Wake delay | 6,177.47 µs | 31,839 µs |
| Lock wait | 0.16 µs | 95 µs |
| Room work | 32.44 µs | 5,769 µs |

Wake delay accounts for most elapsed time in this completed run. The historical incident did not occur in its steady window, so these results place no tighter bound on the historical split or host scheduler's share.

```powershell
python tools/bench/windows.py --native-aral --humans 6 --rooms 32 --room-capacity 33 --seconds 120 --ramp-seconds 90 --host-telemetry --server-cpus 14 --client-cpus 26 28 30 --data-dir $env:HALCYON_DELIVERY --data-extra-dir $env:HALCYON_DATA_EXTRA --binary-dir target/release --output runtime/aral-tick-attribution
```
