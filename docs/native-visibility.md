# Initial ARAL minion visibility

The seven played ARAL streams contain 784 compact declarations of lane-minion templates 287–289 with mask 1 at body offset 117. Four declarations occur at the final timestamp of their respective streams. For the other 780, the first `03F8` waypoint for the declared slot precedes the first `042B` for the same actor in every case. Within half a second, 771 minions have all eight indices, five have seven and four have three. Complete eight-row runs are contiguous in the stream, in ascending index order, and begin 1–230 ms after the declaration. [Capture inventory](live-captures.md)

The declaration provides three eight-byte tables at offsets 88, 96 and 104. Compare the three bytes at the same index with the desired state and masks. The actor's wire team is at +121, either 1 or 2. For the seven indices below, all 5,440 changed rows appeared in the first update and the 20 entries already equal to the desired state were omitted.

| Index | State byte | First mask | Second mask | Emitted rows |
|---|---:|---:|---:|---:|
| Actor's team | 1 | 15 | 0 | 780 |
| 0, 3, 4, 5, 6, 7 | 1 | 0 | 0 | 4,660 |

The opposing index cannot be reconstructed this way. Its 775 observed first rows had masks (0,0) in 701 cases, (1,0) in 66, (5,0) in four and (1,1) in four. Identical declared mask tables produced different outcomes. None of the 70 ARAL tower, 14 crystal or 42 opening full-hero declarations yielded a contiguous eight-index initial update within half a second. The separate Android Solo corpus also had no matching immediate full run across the examined structure, hero and minion declarations. These checks do not resolve later visibility mutations.

The host queues only the evidenced changed rows on the receiving stream that holds the fresh compact declaration, immediately after its first actual lane waypoint. An enemy seat outside vision receives neither the declaration nor its mutations. Authorized observers track their own declarations and registrations. A snapshot on rejoin, slot replacement or loss of visibility discards pending rows. The opponent index remains outside the authored subset. [Declaration projection](../crates/game/src/aral_spawn.rs) · [Per-stream ordering](../crates/game/src/tower_aggro.rs)
