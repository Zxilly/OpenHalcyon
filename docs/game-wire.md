# Android CE game wire coverage

The Android reader table supplies 171 opcode entries. Fixed sizes in that table include the length and opcode and precede encryption padding. Some entries have variable sizes, and recorded server messages can have multiple forms. A reader entry establishes that the client recognizes a frame shape. The typed decoder extracts the fields listed below and preserves opaque bodies for other opcodes. [Reader table](../crates/protocol/src/metadata.rs) 路 [Typed decoder](../crates/protocol/src/lib.rs)

| Opcodes | Message | Implemented behavior and evidence |
| --- | --- | --- |
| 0x0005, 0x03E8 | Hello and player UUID | Control and game sockets are associated with the reserved player. The native client connected for the draft. |
| 0x0000, 0x0006 | Clock and heartbeat | Clock float and heartbeat are decoded and echoed. |
| 0x03E9, 0x0454, 0x0453 | Setup, game mode, hero and skin catalog | Server sends mode settings and catalog strings before the draft roster. The Android client displayed the picker. |
| 0x0459 | 5v5 draft roster | The server encodes timers and 16 seat records, including occupied seats, team, player or bot identity, template, skin and lock state. The native client displayed selections and lock confirmation. |
| 0x045E, 0x045F, 0x0455, 0x0463 | Hero, skin and lock | The server validates available choices, updates the roster and acknowledges the control message. |
| 0x03EE, 0x03F3 | World roster and actor spawn | Replay-derived roster and spawn fields include entity and template IDs. Spawns also carry skin, team, position and health. Unidentified fields use defaults. |
| 0x03F4, 0x042E | Move and position | On a running game channel, movement updates the owning seat's simulation command. The server emits visible positions. Covered by local game-socket tests. |
| 0x0439, 0x043D, 0x044B | Shop request, grant and removal | The game checks actor ownership, price and inventory. Upgrading a component emits removal and grant messages. |
| 0x041D, 0x041C | Entity stat and modifier | Stat updates include entity, value and stat type. The shop also sends modifier changes when adding or replacing items. |
| 0x0458, 0x046B, 0x046D, 0x046E, 0x0471 | Draft control traffic | The server echoes these observed messages. Their individual fields and effects need further evidence. |

The replay reader consumed 43,160 records across 14 chunks. The archive contains a post-lock client hello but no paired successful CE server handoff, and replay chunks begin after the transition. The original Android client reached **LOCKED IN**. Rendering the battlefield, native touch movement, ability inputs and a complete native match remain unverified. The simulator's support for an action alone cannot identify its native opcode. [Game encoders](../crates/game/src/messages.rs) 路 [Compatibility evidence](protocol-coverage.md)
An independent decoder documents actor 1505 with the same template across a roster and multiple 0x03F3 spawns. Its measured first-spawn skin differs from the later spawn and roster values. Consumers should preserve these per-record values. The decoder's separate VGReborn report concerns HTTP APIs and supplies no live draft-to-world exchange. [First-spawn evidence](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/H5672_SPAWN_SKIN_2026-10-14.md) 路 [VGReborn analysis](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/VGREBORN_PROTOCOL_ANALYSIS.md)
The local VGR export contains 153 modifier records (0x041C) and one item removal (0x044B). The typed reader exposes the actor, source, float value, stat and trailing flag bytes for modifiers. For removal it exposes the actor, item instance and trailing 16-bit field. Tests check synthetic big-endian values and truncated records. The flags retain their raw values.

To verify the native handoff, a matching successful Android 147219 session would need to cover the lock acknowledgement, connection and channel order, initial replies and the first rendered battlefield. Complete message bodies, their directions and channel association are needed to compare that transition with replay initialization. The operator can decode locally and substitute consistent anonymous player identifiers.
## Read-only record variants

A full pass over all 43,160 local VGR records using `ReplayReader` and `Message::fields` produced 43,061 typed records and 99 opaque records, with zero decoding errors. A typed record may expose only a prefix. `Message::body` retains the complete body, including padding.

| Opcode | Recovered fields and limits |
| --- | --- |
| 0x03ED, 0x046F | Fixed 64-byte leading text. The 0x03ED reader also consumes big-endian words at offsets 64 and 68. The latter participates in a lookup. |
| 0x03F2, 0x03F3 | The local 0x03F2 replay family contains 1,208 bodies of 122 bytes and 215 of 126 bytes. Its definition, skin and actor are 32-bit big-endian values at body offsets 0, 4 and 8, shared with the longer 0x03F3 spawn. Other 0x03F2 lengths remain opaque. The earlier scoreboard label conflicts with the measured records and native receiver trace. |
| 0x03F8 | A selector byte followed by two unaligned big-endian floats. The local replay contains both 9- and 14-byte bodies. |
| 0x042B, 0x043E, 0x043F | Leading big-endian entity reference. The remaining bytes are retained. The second word in 0x043E and 0x043F sometimes differs from the first. Its role is unresolved. |
| 0x045A | Two timing floats in the ARAL roster header. All 696 records across three archived ARAL sessions report 2,592 bytes of plaintext, but the logs preserve only the first 48 bytes. Subsequent slot contents and the end of the roster have no full local specimen. |
| 0x045C, 0x0471 | Sixteen six-byte entries in 0x045C, each with a reference and two raw bytes. The 0x0471 prefix contains one reference and one flag byte. The local replay and the Android reader sizes agree on these prefixes. |
| 0x0458, 0x046D, 0x046E, 0x0470, 0x047C | The receiver/emitter evidence supports one control byte. Its gameplay meaning remains unresolved. |

Independent source details include the receiver's actor resolution, leading-text conversion, control offsets and length variants. [Native roster study](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/NATIVE_ROSTER_2026-10-04.md) 路 [Receiver and record families](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/EVENT_SEMANTICS_2026-10-04.md)

## Health and death

The local replay contains 1,735 health impacts (0x041E) and 102 death actions (0x0430). The 147219 reader consumes 16 structural bytes for an impact and eight for a death. The replay bodies contain 22 and 14 bytes respectively. The impact prefix contains target and source references, a signed float operand and three presentation fields. Death contains the dying actor and the raw source. The codec retains each suffix. [Native impact serializer and receiver](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/impact_wire_2026-10-16/01_NATIVE.md) · [Death evidence](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/DEATH_EVENTS_2026-09-09.md)

The server sends negative applied damage and positive healing using the locally observed presentation tuple (5, 0, 0). Both actor references must be visible to the recipient. A victim visible on the previous tick remains eligible for its lethal impact and death. Spectators receive the full field of view, and per-player VGR recordings retain only combat events delivered to that player. The decoder's attack-target request (0x0424) and target-change action (0x0425) describe target selection. A corresponding CE input for a completed attack or ability cast still needs a trace linking the command to the action.

Hero deaths also emit the local modifier form with operand +1, layer 0 and operation 0. Attribute 42 increments the victim's death counter. Attribute 41 increments a hero killer's kill counter. The local replay contains six death modifiers and four kill modifiers with this layout. These counters follow the same visibility rule as the associated death. [Native counter fields](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/NATIVE_COUNTER_FIELDS_2026-10-04.md)

## Additional action records

The native receiver's field conversions and the local replay agree on the following layouts. These paths are read-only. They preserve raw selectors, keys and unused bytes. [Native receiver branches and class associations](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/vg-binary-event-candidates-2026-09-09-details.md) · [Cooldown layouts](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/COOLDOWN_EVENT_FIELDS_2026-10-14.md)

| Records | Local evidence and interpretation |
| --- | --- |
| 0x040B, 0x0431, 0x042F, 0x0432 | Destroy, dead-state and undead-state records carry the actor reference. The respawn record also carries three position floats. In 55 same-chunk cases, the dead-state action followed the same actor's death by 1.73–4.25 seconds. The simulator has no separate dead-state transition at those delays. |
| 0x040D, 0x0415, 0x043C | The projectile action has two reference words and a float. The ability action has an actor, another reference and a selector. Local selectors include 8 through 15. A cancellation exposes two reference words. These server-side records cannot identify the native client's ability-button command. |
| 0x0428, 0x048A | Both cooldown records have an owner, lookup key and value. The shorter form adds a mode byte. The longer form adds a second float and six raw bytes. The key has no validated mapping to a specific local ability. |
| 0x041A, 0x041B, 0x0441, 0x0445 | All 12 actor/key pairs from ability override records recur in clear-override records. Buff-duration and buff-cancel records share 26 actor/key pairs. All corresponding actor references occur in local spawn records. |
| 0x042C, 0x048C | The camp visibility prefix has an actor and two bytes. All 12 referenced actors occur in local spawns. The aggro record has two leading bytes and an unaligned float at offset 2. Its first four bytes cannot generally be treated as an actor. |
| 0x03F9, 0x03FA, 0x03FB | The native movement receivers convert three floats after a selector, two references or one actor respectively. The decoder keeps the three values in wire order. |
| 0x0434, 0x043A | Level and grant-ability records carry an actor and a 32-bit operand. The local level samples alone cannot distinguish an increment from an absolute level. |

Remaining opaque records include less common projectile, scripted movement, buff, effects, match-state and aggregate statistics variants. A native class label and copied byte count alone cannot establish the assignment of application fields. In particular, the cumulative statistics record in the local corpus has only one example.
