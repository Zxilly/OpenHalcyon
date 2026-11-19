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

This encoding applies to the initial Android 5v5 Solo structure profiles.
The caller must use one entity per slot and select the matching opening.
Respawns, other modes, and reconnects continue to use their existing
replication paths. The byte layout was checked offline and does not establish
native placement or rendering for the simulation's map.
