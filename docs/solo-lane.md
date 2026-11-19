# Solo mid-lane simulation

The Android CE 147219 Solo recording provides positions in native X/Z world units. The two-dimensional simulator stores these as `Position.x` and `Position.y`. A host can select the layout with `World::use_solo_mid_lane()` after assigning the five heroes on each team, before the first simulation tick. A second call before the first tick preserves the existing layout. Reconnection uses the running world's existing positions.

| Actor | Team 0 opening X/Z | Team 1 opening X/Z |
| --- | --- | --- |
| Heroes, in order within their team | (−90, 0), (−94, −6), (−101, −3), (−101, 3), (−94, 6) | (90, 0), (94, −6), (101, −3), (101, 3), (94, 6) |
| Home crystal | (−75, 0) | (75, 0) |
| Outer mid tower | (−17.5, 0) | (17.5, 0) |
| Mid-lane minion creation | (−70, 0) | (70, 0) |

The opening hero placements correspond to slots 14–18 and 19–23. Ownership and actor IDs remain those of the matchmaking roster, including when seats from the two teams alternate. In all 14 local replay chunks combined, compact minion declarations with mask 1 at body offset 117 and routes 1 or 4 at offset 120 contained 25 creation records per side. Their Z was always zero and the recorded vertical height was approximately 0.06. Mask-0 hydration records carry a moving actor's current position and are excluded from the spawn measurement.

Navigation uses a straight, unobstructed mid-lane corridor bounded by X ±101 and Z ±6. These bounds are a project approximation based on the measured opening extrema. The simulator implements the one outer mid tower and crystal on each side. Vision, movement checks, bot rallying and retreat, ability range checks and attacks use these same world positions. Each hero respawns at its assigned opening slot. Base regeneration and the shop use the existing three-unit radius around each friendly opening pad. Combat timings and wave composition continue to follow the simulation rules. Side lanes, jungle geometry and original terrain collision are outside this layout.

Run `cargo test --locked -p halcyon-sim --lib layout::tests` to check the team slots, shop range, movement, vision, cast cooldown and respawn. The bot test follows the first wave and advances both teams until one actually destroys the opposing crystal.
