# ARAL single lane

Call `World::use_aral_lane` after assigning three seats to each side and before advancing the 20 Hz clock. Each player's seat retains its actor ID. Within each team, roster order assigns the three pads in the order below. The simulator uses native X and Z as the two horizontal coordinates.

| Object | Home coordinates (X, Z) | Away coordinates (X, Z) | Initial HP |
| --- | --- | --- | --- |
| Hero pads | (−78.18, 0.88), (−81.81, 1.97), (−77.03, −1.94) | (79.66, −0.91), (78.60, −5.06), (83.22, 0.72) | Per hero |
| Lane turrets, outer to inner | (−17.06, 1.93), (−35.78, 1.17), (−54, 2.92) | Reflect X | 3,500 |
| Crystal defenses | (−75.48, 11.96), (−68.59, 19.97) | Reflect X | 2,600 |
| Crystal | (−76.12, 19.90) | Reflect X | 10,000 |
| Lane minion spawn | (−71.28, 12.93) | Reflect X | Per unit and wave level |

Seven archived ARAL openings agree on structure templates, positions and HP. The home crystal uses template 292 and the away crystal uses 293. Both teams use 300 for lane turrets and 301 for crystal defenses. The archive also places a shop and a vision beacon on each side. The simulation includes the twelve combat structures and applies existing base services at the hero pads.

Across 784 minion births, the archives use route 0 for home and route 1 for away. The first pair appears about 15 seconds after start. All 92 observed intervals between waves round to 23 seconds. The simulator spawns a tank (289), another tank, a lead minion (288) and a ranged minion (287) at one-second intervals. The birth records carry zero HP fields. Combat uses unit-table stats and the project's existing wave-level progression.

The playable envelope spans X −95 to 95 and Z −7 to 21. This local approximation covers the recorded objectives, movement samples and four Idris aim points tested by the simulation. The route uses straight segments through measured turret positions. The source records establish spawn anchors and route IDs. They leave the native navigation mesh, obstacles, route splines and base service radius unresolved. The simulation retains its existing visibility, combat and victory rules.
