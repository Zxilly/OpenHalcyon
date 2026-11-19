# Native hero progression

All 42 opening ARAL hero declarations across seven local played matches
start at level 4 with zero progress and a 90-point next-level threshold.
The simulator applies three units of the selected hero's stat growth and
starts each seat with four spendable skill points. Draft reselection
reloads the hero's base stats and reapplies growth once. The measured
Lorelai level-four maximums are 1116.76 HP and 450 energy, and Idris's
level-four maximum HP is 1122.46.

The initial Solo declarations use level 1 and a 68-point threshold.
Local Solo level-two snapshots show 84, and the setter for level three
uses 100.
The separate H5810 replay recurrence check listed all threshold setters
from 84 to 244 in steps of 16. The ARAL captures use thresholds from
130 to 200 in steps of 10 on levels 5–12. The server limits these curves
to their corresponding native scene layouts.

| 0x03F3 body offset | Field |
| --- | --- |
| 294 | Current level, big-endian float |
| 318 | Current XP progress, big-endian float |
| 322 | XP threshold at the current level, big-endian float |
| 326 | Baseline restore gate, zero |
| 705 | Post-snapshot loop bound, one |

For actors already declared and visible, an XP gain is written as a
0x041D resource-8 addition. Each actual level increase then writes one
0x0434 increment and a 0x041C base-layer SET for attribute 39. The setter
follows the increment because the client's level action consumes the old
threshold. An actor declared in the same tick already carries the updated
XP and level, so the player replication path suppresses duplicate deltas.
Native XP still accumulates at level 12 and while a hero is dead. A level
gain while dead does not revive the actor.

Generic layouts retain their existing economy and thresholds. The
replication ordering and snapshot field values are covered by local
tests. The captures and offline reader evidence do not establish exact
native behavior for every modifier, mode or client build.
