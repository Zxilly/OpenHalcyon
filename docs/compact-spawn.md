# Android 5v5 Solo compact structure spawn

A local Android 147219 Solo Bots replay starts its world replication with 26
compact structure spawns. The first 26 records contain 18 towers, two crystals
and six VainNodes. Subsequent compact records also describe other entities. The
original replay is required to inspect those records. In the local
vg analysis tree, the decoded records are in _ana/vgr_decoded.jsonl, chunk zero,
records 112–137 (zero-based).

The Android ARM64 opcode handler at 0x82BFD4 in libGameKindred.so copies a
122-byte body and converts the big-endian words at offsets 0 through 84
and 112. The receiver at 0x828854 consumes type, skin, actor and separate
vector triples at offsets 12 and 24. The native replay writer at 0x821B58
writes opcode 0x03F2 and a 122-byte body. Replay records can retain four
additional padding bytes. All offsets here start after the two-byte opcode.

| Body offset | Encoding |
| --- | --- |
| 0, 4, 8 | u32 template, opaque observed skin word, actor ID |
| 12, 16, 20 | f32 X, vertical coordinate, Z |
| 24, 28, 32 | f32 forward vector |
| 36, 40 | f32 current and maximum health |
| 44–87 | Other converted words. Initial forms have 1.0 at 84 and towers also have 1.0 at 64. Other words are zero. |
| 88–111 | Byte fields. Initial forms use sixteen 1 bytes with 15 at offset 97 for team 1 or 98 for team 2, followed by eight zero bytes. |
| 112 | u32 value 0xffffffff in these forms |
| 116–121 | Placement byte, numeric-field mask (zero includes stats), observed zero flag, writer's constant 1, observed 255, team |

The encoder covers the simulation's existing two crystals and two outer
mid-lane towers. With wire teams 1 and 2, the home and away crystal templates
are 295 and 296 and their placement bytes are 6 and 7. Outer towers share
template 340 and use placement bytes 9 and 12. These four forms share the
observed skin word 0xc10b41da. The encoder uses authoritative actor IDs, current
and maximum health, and the simulation's planar coordinates for X/Z. The four
observed forms use vertical coordinate 0.06 and face toward the opposing
side along X. The meanings of the skin word and several ancillary fields
remain unresolved.

Across all 14 Solo replay chunks, the two crystal slots occur once per chunk.
Their first 122 bytes are identical across chunks for each slot separately.
The server uses that compact shape for a visible crystal on fresh world entry
and on later introduction to a team, with current health in the measured
health field. The same comparison on the two mid-lane towers shows changing
words at offsets 76 and 80 on both teams. The home tower also changes heading,
health and several flags. The tower encoder covers the opening state.

## Lane minions in the Solo recording

Across all 14 VGR chunks of one Android 147219 match, template 335 appears
190 times, 336 appears 255 times, 337 appears 118 times and 338 appears
24 times in compact spawns. These 587 records cover 130 actor IDs. Each
actor has one creation record with mask 1 at +117. Those 130 records omit
the numeric stats and carry zero bytes for HP, maximum HP and speed. The
457 records with mask 0 carry current and maximum HP at +36 and +40.
Maximum HP was respectively 475, 575, 880 and 950 for the four templates.
All records use the skin word 0xeb39ce55.

The reader exposes the slot at +116, numeric mask at +117, route at +120 and
wire team at +121. Wire team 1 used routes 0–2 and wire team 2 used routes
3–5. On the first wave the central route on each side used 1 and 4. Across 130 minion identities the route and slot stayed constant for every
subsequent compact declaration of the same actor. Of the 130 initial records,
25 used the central spawn on each side (Z = 0, routes 1 and 4). Each outer
spawn (Z = -5 or +5) accounted for 20 per side.

### Shared actor registration

The Android compact receiver at 0x828930 reads +116. The spawn action at
0xc024f4 copies it to the new actor via 0xc5ba80. Its registration path at
0xc025c4 passes the same value and an actor handle to 0x7ff4d0. The client
initializes a table of 200 handle entries at 0x7ff424 and assigns the received
index at 0x7ff614. The corresponding clear and lookup operations are at
0x7ff654 and 0x7ff670. The byte at +120 has a separate setter at 0xc5cb88.

The full 0x03F3 receiver reads its registration byte at +745 (0x828d60).
The full spawn action uses the same actor field and registration table
(0xc03944, 0xc03b00). All ten hero declarations in each of the 14 Solo
chunks retained their original indices. Actor 1500 used slot 14 and actors
1515 through 1523 used slots 15 through 23 respectively.

Lane minions used 64 distinct indices in the range 52–132. Fifty-two indices
were reused for another minion. Every one of the 66 successive minion
assignments to an already-used index followed the previous actor's recorded
0x0430 death. Some indices also served other compact entity templates.
A host allocator therefore needs one stable mapping for active registered
actors across both message formats. Fog-of-war disappearance alone provides
no evidence that a slot can be reassigned. The capture establishes ordering
after death, but supplies no exact mandatory recycling delay.

## Server emission

The Android Solo scene registers heroes by seat at 14–23 and keeps the four
supported structures at 6, 7, 9 and 12. Full declarations also carry their
slot at +745. The simulator's lane minions use routes 1 and 4 and carry
authoritative X/Z coordinates, current HP and maximum HP. The server emits
the measured mask-0 shape even on first visibility so the client can use
the simulator's scaled HP. The fixed facing and height use the observed
opening presentation values.

The room claims actor slots as entities are first exposed. It fills
52–132 first and may use 133–199 in the same registration table if needed.
Opposing actors remain subject to the usual visibility checks. Leaving view
does not free the actor's slot. The allocator releases a slot after the
server emits the corresponding death or on a fresh scene when the previous
actor is dead. Living actor indices survive a reconnect. If no slot is
available, the simulator keeps the actor alive and replication defers its
declaration and related updates until it can assign a slot. The allocator
does not wrap. This emission requires the native Solo scene transport.
Compact minion rendering on the original Android client remains unverified.

## Full hero declarations

The ten heroes at the start of the Solo recording and all 42 heroes in the
seven played live ARAL openings used eight bytes of value 1 at +713 and a
second eight-byte array at +721 with value 15 at the wire team's index. The
other entries were zero. The Solo implementation applies this team profile at
tick zero. Later Solo checkpoints contain different byte arrays.

Word +709 reaches the same actor selector setter at 0xc5cb88 as compact
field +120. Across all 140 Solo declarations, the human used 0xffffffff,
and the nine bots used their zero-based team. The 42 human ARAL declarations
also used 0xffffffff. Word +741 resolves an optional actor reference at
0xc5daec. All 182 declarations carried 0xffffffff there. These words are
separate from the network actor ID at +8 and registration slot at +745.
The observed selector values alone do not establish authentication or input
permissions.

In the Solo opening, slots 14–18 belong to wire team 1 and slots 19–23 to wire team 2. The allocator groups the reserved heroes by team and preserves roster order within each team. Simulation entity IDs can alternate teams.
