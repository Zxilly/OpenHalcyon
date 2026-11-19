# Android CE game wire coverage

The Android reader table supplies 171 opcode entries. Fixed sizes in that table include the length and opcode and precede encryption padding. Some entries have variable sizes, and recorded server messages can have multiple forms. A reader entry establishes that the client recognizes a frame shape. The typed decoder extracts the fields listed below and preserves opaque bodies for other opcodes. [Reader table](../crates/protocol/src/metadata.rs) · [Typed decoder](../crates/protocol/src/lib.rs)

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

The replay reader consumed 43,160 records across 14 chunks. The archive contains a post-lock client hello but no paired successful CE server handoff, and replay chunks begin after the transition. The original Android client reached **LOCKED IN**. Rendering the battlefield, native touch movement, ability inputs and a complete native match remain unverified. The simulator's support for an action alone cannot identify its native opcode. [Game encoders](../crates/game/src/messages.rs) · [Compatibility evidence](protocol-coverage.md)
An independent decoder documents actor 1505 with the same template across a roster and multiple 0x03F3 spawns. Its measured first-spawn skin differs from the later spawn and roster values. Consumers should preserve these per-record values. The decoder's separate VGReborn report concerns HTTP APIs and supplies no live draft-to-world exchange. [First-spawn evidence](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/H5672_SPAWN_SKIN_2026-10-14.md) · [VGReborn analysis](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/VGREBORN_PROTOCOL_ANALYSIS.md)
