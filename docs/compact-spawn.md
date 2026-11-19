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
3–5. On the first wave the central route on each side used 1 and 4. Slots
changed between waves and were reused by later actors. The capture supplies
neither a general slot allocator nor a mapping from OpenHalcyon's simulated
formations to the six native routes. The simulation also scales HP at creation.
The compact reader retains all original bytes for both masks and both known
record lengths. Live minion replication continues using the existing encoder.
