# Solo lane simulation

The Android CE 147219 Solo recording provides positions in native X/Z world units. The two-dimensional simulator stores these as `Position.x` and `Position.y`. A host can select the layout with `World::use_solo_mid_lane()` after assigning the five heroes on each team, before the first simulation tick. A second call before the first tick preserves the existing layout. Reconnection uses the running world's existing positions.

| Actor | Team 0 opening X/Z | Team 1 opening X/Z |
| --- | --- | --- |
| Heroes, in order within their team | (−90, 0), (−94, −6), (−101, −3), (−101, 3), (−94, 6) | (90, 0), (94, −6), (101, −3), (101, 3), (94, 6) |
| Home crystal | (−75, 0) | (75, 0) |
| Outer mid tower | (−17.5, 0) | (17.5, 0) |
| Middle mid tower | (−34.5, 0) | (34.5, 0) |
| Armory mid tower | (−53.04, 0) | (53.04, 0) |
| Mid-lane minion creation | (−70, 0) | (70, 0) |

The opening hero placements correspond to slots 14–18 and 19–23. Ownership and actor IDs remain those of the matchmaking roster, including when seats from the two teams alternate. In all 14 local replay chunks combined, compact minion declarations with mask 1 at body offset 117 and routes 1 or 4 at offset 120 contained 25 creation records per side. Their Z was always zero and the recorded vertical height was approximately 0.06. Mask-0 hydration records carry a moving actor's current position and are excluded from the spawn measurement.

Navigation uses a straight, unobstructed mid-lane corridor bounded by X ±101 and Z ±6. These bounds are a project approximation based on the measured opening extrema. The simulator implements all three mid towers and the crystal on each side. Vision, movement checks, bot rallying and retreat, ability range checks and attacks use these same world positions. Each hero respawns at its assigned opening slot. Base regeneration and the shop use the existing three-unit radius around each friendly opening pad. Mid-lane wave timing follows the native replay clock. Side roads follow the paths below. Original jungle geometry and terrain collision remain unverified.

Run `cargo test --locked -p halcyon-sim --lib layout::tests` to check the team slots, shop range, movement, vision, cast cooldown and respawn. The bot test follows the first wave and advances both teams until one actually destroys the opposing crystal.

The fourteen Solo replay chunks register `OuterTurret5v5` (340, 3000 HP,
190 weapon), `Turret5v5` (339, 4000 HP, 250 weapon) and `ArmoryTurret5v5`
(341, 5000 HP, 280 weapon). All three have 100 armor, 100 shield and range 9
in the delivered unit definitions. The middle and armory towers use slots
26/27 at home and 32/33 away. The host reserves the original static actor
IDs, gives each tower its own attacks and health, and counts each destruction.
Only an enemy outer-tower kill starts the first-outer-tower milestone. The
existing 300-gold local turret bounty applies across tiers.

A native player receives only friendly or visible structures. Opening packets
use the measured terrain height and signed heading components. A late player
gets a full declaration for each visible living tower with current health,
native slot and actor ID. The fallback uses a team-facing heading. The
original rotating aim and timer fields for arbitrary runtime states remain
unverified. Crystals retain their compact geometry across all fourteen chunks.

At the first running tick the server sends the compact declarations, then the
self-sourced `043F` applications for the structures visible to that player.
The pristine home side receives 38 applications across 13 structures. Across
both sides the recording contains 76 applications on 26 structures. Their
34-byte prefixes match in all 14 chunks. The recorder accepts the matching
actor, definition, instance, duration, stack and extension fields. Another
18 opening applications use definition 205 and source `FFFFFFFF`; those
instances and their lifetimes are still unimplemented.

### Mid-lane waves

The Solo replay's `03EF` start marker is at replay time 5.116 s. Routes 1
and 4 each show 25 mask-1 minion births in five waves. Relative to the marker,
the first home-side births were at 19.113, 44.093, 69.404, 94.269 and
119.401 s. The host starts each side at game second 19, repeats every 25 s,
and emits one minion per side per second for five seconds. Waves 1, 2, 3
and 5 contained templates 336, 336, 337, 335, 335. Wave 4 contained 336,
336, 335, 335, 338. The host repeats that four-wave cycle beyond the observed
five waves. The single recording ends before a sixth wave is due.

Of 457 mask-0 minion declarations across all lanes, 156 belong to these two
routes. All 156 have level byte 1. Every included template had its delivered
base maximum HP (475, 575, 880 and 950 for 335 through 338). The host retains
those stats after the observed period. A fresh, pristine mid-lane birth can
use mask 1 at X −70 or +70, Z 0. An already moving or wounded minion first
seen later, or restored after reconnect, instead receives mask 0 with its
current position, HP and maximum HP. A nonstandard maximum HP also requires
explicit values at birth. The two routes consume no side-lane spawn slots.

### Mid-lane march destinations

All 25 recorded minions per team received the same first eight directed
`03F8` destinations. Both start from X ±70 and first aim at ±69. Team 0
turns to Z +2.11, then +3 around its towers. Team 1 turns to −2.11 and −3.
Both encode center-line Z as positive zero. The host follows the first 16
repeated home-side destinations through (−13.17, 0), observed in sequence
for nine home units. For the away side, 20 units continued to (+6.26, 0)
and (0, 0), and 15 continued to (−6.26, 0). At those evidence limits the
units resume the hosted objective approach.

Enemy acquisition and pursuit take precedence over marching. Each living
minion keeps its next waypoint in the authoritative world. Resuming after
combat skips markers it has passed, and reconnecting uses its current state.
A march step reads one destination in steady state. Each waypoint can be
retired only once, at most 16 or 19 per actor over its lifetime; marching
spends no pathfinding search budget.

## Side lanes

The 14 Solo chunks contain 80 side-lane births, 20 each on routes 0, 2, 3 and 5. Their anchors are X ±75 and Z ±5. At 19 seconds and every 25 seconds thereafter, each side lane creates one minion per second for four seconds. The first three waves and the fifth use templates 336, 336, 337, 335. The fourth uses 336, 336, 335, 338. The host repeats this four-wave pattern beyond the five recorded waves. Each minion retains its lane assignment when chasing, reentering sight and reconnecting.

All 20 minions on each route agree on the first eight directed destinations. The two west routes share a longer 22-point sequence with opposite signs for Z. The east routes share the reflected first 17. Beyond the observed movement, the host continues along the opposite recorded road in reverse. A five-unit corridor around each polyline is a local movement and sight rule. It keeps lane fights reachable without opening paths through the unmeasured interior. Heroes can click ahead on either road. Bot heroes split across lanes and rally to a living minion on their assigned lane.

The 12 side towers use their own original actors, slots and tower tier. Each team has outer towers at absolute X/Z (21.30, 57.50), middle towers at (49.24, 37.84) and armory towers at (63.00, 22.41), on both sides of mid. All 12 can fire, take damage and be destroyed. Opening declarations use the measured poses. Reconnection carries current health for each visible tower.

### Reproducing the Solo bot side comparison

Run `cargo run --locked --release -p halcyon-sim --example solo_sides -- ../vg/_ana/delivery` with the local 147219 delivery. The command writes one CSV row per match with seed, winner, ending tick, each side's enemy tower kills, and the actor ID and tick of the first destroyed tower. All 128 matches use Medium difficulty, alternating seats, 10 bots and seeds 0 through 31 inclusive in each group. The clock runs at 20 Hz.

| Roster at west / east | West wins | East wins | Ending tick, min / median / max |
| --- | ---: | ---: | --- |
| Built-in Ringo / Ringo | 14 | 18 | 24,406 / 33,101 / 38,275 |
| Delivery west five / same five | 20 | 12 | 23,361 / 26,850.5 / 31,995 |
| Recorded west five / recorded east five | 19 | 13 | 21,985 / 29,381 / 34,877 |
| Recorded east five / recorded west five | 19 | 13 | 20,090 / 27,977 / 33,637 |

Native team slots 14–18 contained Amael, Inara, Ardan, Tony and Varya. Slots 19–23 contained Skye, Alpha, Tony, Idris and Baron. Recorded picks here use local bot logic and the host's implemented skills. All 128 games ended by crystal destruction. In the recorded-roster run, the first tower fell on the negative-Z road 14 times, mid 9 and positive-Z 9. Swapping those rosters yielded 1, 3 and 28 first tower losses respectively.

Team-local seats determine the host's lane assignments (mid, negative Z, positive Z, mid, negative Z). The former global actor-ID formula could pair different heroes with the same lane or concentrate a side's heroes there after party-based team assignment. The alternating-seat controls already had equivalent assignments. The native mid-lane routes differ near center, actor IDs stagger decisions, and simultaneous actions use deterministic scheduling. Exact left/right trajectories need not coincide. These four samples characterize the measured rosters and seeds and establish no general win-rate target.

Use `--bot-diagnostics` after the delivery path to run the recorded lineup on
all three lanes at seed 0 and print one row for each of the ten Bot actors.
The example checks the state digest against an uninstrumented copy every
1,000 ticks and at match end. Only the decision function is timed. Rejections
count tactical orders rejected by the shared validator, divided by those
decision calls. Periodic shop and skill-learning probes are outside both
counts. A walking attempt is a tick that executes movement or chase for that
actor. A stall is at least 20 consecutive walking ticks with a goal more than
0.1 world units away and less than 0.001 world unit of movement per tick.
Total stalled ticks include the whole qualifying run, and the report gives
both total stalled ticks per walking attempt and the longest run. Combat,
arrival and ticks without a walking attempt do not count toward a stall.
Nanoseconds vary by machine. The 20 Hz tick statistics and state digest
describe the simulation, independently of wall-clock decision timings.

The route solver checks movement segments against convex pieces of the
project's corridors around the recorded centerlines. It splits at corridor joins and follows a reachable
centerline waypoint when a direct segment cannot be verified. Bot retreat
steps follow the same routes. In the recorded lineup at seed 0, the route
fix reduced rejected tactical orders from 2,401 of 58,376 decisions to
0 of 52,044. Stalled walking fell from 41,743 of 273,621 attempts to
0 of 229,251. The east team destroyed the west crystal at tick 29,901
before the fix and tick 29,177 after it.
