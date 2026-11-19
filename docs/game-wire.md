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

The local Android replay contains 43,160 records across 14 chunks. The original client also rendered a live Solo Bots battlefield and produced touchscreen movement on its draft connection. The repository records the live exchange separately from the offline evidence. [Game encoders](../crates/game/src/messages.rs) · [Client verification](protocol-coverage.md)
An independent decoder documents actor 1505 with the same template across a roster and multiple 0x03F3 spawns. Its measured first-spawn skin differs from the later spawn and roster values. Consumers should preserve these per-record values. The decoder's separate VGReborn report concerns HTTP APIs and supplies no live draft-to-world exchange. [First-spawn evidence](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/H5672_SPAWN_SKIN_2026-10-14.md) · [VGReborn analysis](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/VGREBORN_PROTOCOL_ANALYSIS.md)
A successful base-shop purchase records the exact price paid, the new item instance and any consumed instance in the simulation. Players receive the observed 0x0439 acknowledgement first. Bot decisions produce the same currency and equipment changes directly. The server uses signed 0x041D stat 6 for purchase debits and for each actual killer or nearby ally gold award. A visible actor's initial spawn and reconnect snapshot include its current equipment and combat stats. If a purchase coincides with first visibility, the snapshot grants each held instance once. Authorized observers receive the full view. The supported upgrades consume one Weapon Blade for Heavy Steel or Six Sins when present.

The local VGR export contains 153 modifier records (0x041C) and one item removal (0x044B). The typed reader exposes the actor, source, float value, stat and trailing flag bytes for modifiers. For removal it exposes the actor, item instance and trailing 16-bit field. Tests check synthetic big-endian values and truncated records. The flags retain their raw values.

The staged same-socket handoff can be tested alongside the separate game connection. The original post-lock hellos alone do not establish their role. [Solo Bots transport and regression tests](../crates/game/src/scene.rs)
## Read-only record variants

A full pass over all 43,160 local VGR records using `ReplayReader` and `Message::fields` produced 43,160 records with decoded structural fields or a confirmed header-only form and zero wholly opaque records, with zero decoding errors. A typed record may expose only a prefix. `Message::body` retains the complete body, including padding.

| Opcode | Recovered fields and limits |
| --- | --- |
| 0x03ED, 0x046F | Fixed 64-byte leading text. The 0x03ED reader also consumes big-endian words at offsets 64 and 68. The latter participates in a lookup. |
| 0x03F2, 0x03F3 | The local 0x03F2 replay family contains 1,208 bodies of 122 bytes and 215 of 126 bytes. Its definition, skin and actor are 32-bit big-endian values at body offsets 0, 4 and 8, shared with the longer 0x03F3 spawn. Other 0x03F2 lengths remain opaque. The earlier scoreboard label conflicts with the measured records and native receiver trace. |
| 0x03F8 | A selector byte followed by two unaligned big-endian floats. The local replay contains both 9- and 14-byte bodies. |
| 0x042B, 0x043E, 0x043F | Leading big-endian entity reference. The remaining bytes are retained. The second word in 0x043E and 0x043F sometimes differs from the first. Its role is unresolved. |
| 0x045A | Full six-player ARAL rosters use sixteen 161-byte slots, two leading timing floats and a bounded tail. The native archives contain complete frames. [ARAL layouts](aral.md) |
| 0x045C, 0x0471 | Sixteen six-byte entries in 0x045C, each with a reference and two raw bytes. The 0x0471 prefix contains one reference and one flag byte. The local replay and the Android reader sizes agree on these prefixes. |
| 0x0458, 0x046D, 0x046E, 0x0470, 0x047C | The receiver/emitter evidence supports one control byte. Its gameplay meaning remains unresolved. |

Independent source details include the receiver's actor resolution, leading-text conversion, control offsets and length variants. [Native roster study](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/NATIVE_ROSTER_2026-10-04.md) · [Receiver and record families](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/EVENT_SEMANTICS_2026-10-04.md)

## Health and death

The local replay contains 1,735 health impacts (0x041E) and 102 death actions (0x0430). The 147219 reader consumes 16 structural bytes for an impact and eight for a death. The replay bodies contain 22 and 14 bytes respectively. The impact prefix contains target and source references, a signed float operand and three presentation fields. Death contains the dying actor and the raw source. The codec retains each suffix. [Native impact serializer and receiver](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/impact_wire_2026-10-16/01_NATIVE.md) · [Death evidence](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/DEATH_EVENTS_2026-09-09.md)

The server sends negative applied damage and positive healing using the locally observed presentation tuple (5, 0, 0). Both actor references must be visible to the recipient. A victim visible on the previous tick remains eligible for its lethal impact and death. Spectators receive the full field of view, and per-player VGR recordings retain only combat events delivered to that player. The decoder's attack-target request (0x0424) and target-change action (0x0425) describe target selection. A corresponding CE input for a completed attack or ability cast still needs a trace linking the command to the action.

Hero deaths also emit the local modifier form with operand +1, layer 0 and operation 0. Attribute 42 increments the victim's death counter. Attribute 41 increments a hero killer's kill counter. The local replay contains six death modifiers and four kill modifiers with this layout. These counters follow the same visibility rule as the associated death. [Native counter fields](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/NATIVE_COUNTER_FIELDS_2026-10-04.md)

## Additional action records

The native receiver's field conversions and the local replay agree on the following layouts. These paths are read-only. They preserve raw selectors, keys and unused bytes. [Native receiver branches and class associations](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/vg-binary-event-candidates-2026-09-09-details.md) · [Cooldown layouts](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/COOLDOWN_EVENT_FIELDS_2026-10-14.md)

| Records | Local evidence and interpretation |
| --- | --- |
| 0x040B, 0x0431, 0x042F, 0x0432 | Destroy, dead-state and undead-state records carry the actor reference. The respawn record also carries three position floats. In 55 same-chunk cases, the dead-state action followed the same actor's death by 1.73 to 4.25 seconds. The simulator has no separate dead-state transition at those delays. |
| 0x040D, 0x0415, 0x043C | The projectile action has two reference words and a float. The ability action has an actor, another reference and a selector. Local selectors include 8 through 15. A cancellation exposes two reference words. These server-side records cannot identify the native client's ability-button command. |
| 0x0428, 0x048A | Both cooldown records have an owner, lookup key and value. The shorter form adds a mode byte. The longer form adds a second float and six raw bytes. The key has no validated mapping to a specific local ability. |
| 0x041A, 0x041B, 0x0441, 0x0445 | All 12 actor/key pairs from ability override records recur in clear-override records. Buff-duration and buff-cancel records share 26 actor/key pairs. All corresponding actor references occur in local spawn records. |
| 0x042C, 0x048C | The camp visibility prefix has an actor and two bytes. All 12 referenced actors occur in local spawns. The aggro record has two leading bytes and an unaligned float at offset 2. Its first four bytes cannot generally be treated as an actor. |
| 0x03F9, 0x03FA, 0x03FB | The native movement receivers convert three floats after a selector, two references or one actor respectively. The decoder keeps the three values in wire order. |
| 0x0434, 0x043A | Level and grant-ability records carry an actor and a 32-bit operand. The local level samples alone cannot distinguish an increment from an absolute level. |

## The final 99 replay records

These counts cover one 14-chunk Android replay. The dispatch addresses refer to the original Android 147219 arm64 image. In the table, W denotes a 32-bit word explicitly converted from network order, H a converted 16-bit word, and B an unconverted byte. Values appear in wire order. The action reader exposes raw operands and preserves remaining bytes.

| Opcode | Records | Android dispatch | Layout and corroboration |
| --- | ---: | --- | --- |
| 0x03EF | 1 | 0x82bfa8 | Header only. The Android constructor takes no data from the body. The single padded example has six zero bytes. Windows reports a one-byte formatter, so other padded variants stay opaque. |
| 0x03F1 | 1 | 0x82bfc0 | One team word and a reason byte. Android takes the low byte of the word and the reason. The end-request class in Windows and the live Android control capture have different contexts. Recording continues after this record. |
| 0x0403 | 11 | 0x82c92c | 5W + 1B, auto-move. Word 0 matches a spawned actor in all 11 records. |
| 0x0406 | 10 | 0x82d048 | 8W, auto-orbit. Words 0 and 3 both match spawned actors in all 10 records. |
| 0x0407 | 4 | 0x82ca1c | 4W, cancel-orbit. Word 0 matches a spawned actor in all four records. |
| 0x0409 | 7 | 0x82ca60 | 4W + 1B, teleport. Word 0 matches a spawned actor in all seven records. |
| 0x040E | 15 | 0x82bb60 | 6W + 1H + 2B, fire projectile. |
| 0x0410 | 3 | 0x82cb58 | 4W + 1B, detonate projectile. |
| 0x0416 | 21 | 0x82cba4 | 4W + 1B, play ability. Word 0 matches a spawned actor in all 21 records. |
| 0x0419 | 1 | 0x82cc0c | 6W, location effect. Word 1 matches a spawned actor in this example. |
| 0x041F | 6 | 0x82ccc0 | 2W passed separately to the game-mode-variable constructor. |
| 0x0443 | 1 | 0x82d760 | 3W passed separately to the buff-stack constructor. |
| 0x0446 | 1 | 0x82baa4 | 2W passed separately to the buff-timeout constructor. |
| 0x044C | 2 | 0x82d3e0 | 5W + 1B, item activation. |
| 0x0478 | 14 | 0x82cf5c | 2W + 1B, minimap phase. Word 0 matches a spawned actor in all 14 records. |
| 0x048D | 1 | 0x82d8fc | 1,612 structural bytes. Android consumer 0x81DFC0 reads 16 player slots by column with ten BE32 item identifiers per slot. The decoder exposes verified player, team, damage, farm and net-worth columns, including unused slots. |

The projectile owners, movement targets, ability inputs and remaining aggregate columns are unresolved. Actor matches establish value correspondence. Windows class names provide context for the independent Android field-width and endian-conversion evidence. The named statistics agree with Android offsets, strides, field widths and destination layout. [Action-family research](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/vg-binary-event-candidates-2026-09-09-details.md) · [Statistics consumer research](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/evidence/2026-09-08-endgame/native-static-analysis.md)

## Android draft settings and roster consumers

In the original Android 147219 arm64 library, the 0x03E9 dispatcher at image address `0x82B8D8` copies 101 bytes and converts five big-endian words. Offsets are relative to the message body, after the opcode. The Rust decoder exposes the original bits and retains the suffix starting at +100.

| Body offset | Android draft capture (1 record) | VGR world headers (14 records) | Evidence |
| --- | --- | --- | --- |
| +0 | 1500 | 1500 | The consumer copies this word into a local structure. |
| +4 | 1 | 0 | The dispatcher can clear this word depending on the result of a local check at `0x823FDC`. The consumer tests the resulting value for zero. |
| +8 | 0x0000FFFF | 0 | Converted 32-bit word. Its application meaning is unresolved. |
| +12 | 0 | 0 | Converted 32-bit word. Its application meaning is unresolved. |
| +96 | 0x3F800000 | 0 | Converted 32-bit word. The decoder preserves its bits; the consumer evidence does not establish a float unit. |

The +4 consumer at `0x8292E4` passes the zero test to `0x81A3DC`. On one state-dependent path, nonzero selects state 1 and zero selects state 2. Other current states take different branches. The transition routine at `0x819C84` can dispatch a registered entry callback. The captures establish the values during draft and world initialization. They do not identify which entry callback opens the next game connection.

The 0x0459 receiver at `0x8E07C4` forwards to a registered listener. One resolved virtual target, `0xA7D890`, returns immediately. Another, `0x8E1018`, walks up to 16 records with a 161-byte stride and checks the occupied flag and entity reference. For the matching record it saves team and slot, and caches the hero template when the lock flag is set. It then notifies subscribers. This callback does not read either leading timer or directly open a socket. The existing Android log establishes lock confirmation and a later connection attempt. Identifying the dial trigger requires the active subscriber or state-entry binding and its call order during a successful transition.

## Queue IDs and game mode labels

The room uses the same mode label in 0x03E9 on both sockets, 0x0454 on the control socket and 0x046F on the game socket. The original queue ID continues to identify the matchmaking pool.

| Queue ID or local alias | Wire label | Local evidence |
| --- | --- | --- |
| `5v5_bots_solo`, alias `solo` | `*GameMode_5v5_SoloBots*` | Android 147219 `joinLobby` and control capture, plus the VGR game setup. |
| `5v5_pvp_casual`, aliases `5v5_casual`, `casual_5v5` | `*GameMode_5v5_Casual*` | Android binary queue string and game manifest. A local server test follows the queue ID from CE RPC through the three wire messages. |
| `5v5_pvp_ranked`, aliases `5v5_ranked`, `ranked_5v5` | `*GameMode_5v5_Ranked*` | Android binary string and game manifest. The iOS parser also uses the native ID for ranked profile data. |
| `5v5_practice` | `*GameMode_5v5_Practice*` | Android binary string and game manifest. |
| `5v5_bots_coop`, `5v5_coop` | `*GameMode_5v5_CoopBots*` | Manifest label. The accepted queue spellings are local aliases. |
| `5v5_private`, `5v5_pvp_private`, `5v5_private_draft`, `5v5_private_bots` | `*GameMode_5v5_Private*`, `*GameMode_5v5_PrivateDraft*`, `*GameMode_5v5_PrivateBots*` | Manifest labels and local queue aliases. The first two aliases select the same private label. |
| `aral_casual`, alias `aral` | `*GameMode_Aral_Casual*` | Android binary string, game manifest and mode name in three archived ARAL game streams. |

An unrecognized queue ID uses the 5v5 Casual label. This mapping only changes the label. The existing queue roster and seat policy determine which seats contain humans. The legacy casual_aral queue uses six seats and full 0x045A rosters. The simulation uses the project lane. [ARAL mode](aral.md)

## Bot difficulty

The Android 147219 Solo Bots menu sent all five difficulty values to a loopback server. The native labels Very Easy, Easy, Medium, Hard and Very Hard sent `very_easy`, `easy`, `medium`, `hard` and `very_hard` respectively in the `joinLobby` payload. The server accepts either a JSON object or a JSON-encoded string. The original queue name remains unchanged.

OpenHalcyon's hero-bot decision intervals are 16, 10, 5, 3 and 2 ticks respectively at 20 Hz. These intervals are server policy. Omitting difficulty preserves the five-tick default. The setting leaves human stats, economy, ability learning and non-hero decision frequency unchanged.

The equivalent optional field is available on `POST /api/v1/queue` for supported 5v5 bot queues. Admin room responses expose the selected difficulty. Local queues match only identical difficulty selections. In a pool, the selected difficulty and cadence are included in the existing rules digest while the Raft queue schema and default digest are preserved. Only new workers advertise the variant digests. Drain active variant rooms and let capacity offers expire before downgrading a host.

## Draft acknowledgement and natural match results

Android 147219's captured Solo Bots draft returns a 0x0459 roster, echoes the first 0x045E selection, then sends 0x03F1 with team zero and reason 8. OpenHalcyon follows this ordering once per control connection. The Android consumer at `0x81AB88` returns directly for reason 8. This capture supplies the message order but does not identify a required battlefield loading signal.

Seven archived live ARAL matches each contain exactly one 0x03F1 immediately after the 0x048D statistics report. In five games the losing side's crystal died 6.049–6.104 seconds before this pair, the reported team was the opponent, and the reason was zero. The other two had reason 2 and no crystal death. A further 8–93 world updates follow the request. Held-out 5v5 playback also shows a reason-zero request in a crystal victory. [Independent playback results](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/ENDGAME_HELDOUT_2026-10-05.md)

On authoritative crystal defeat the game host sends the winning team's number (1 or 2) and reason zero to active world connections and observers in ARAL, 5v5 PvP and 5v5 Solo Bots. The Android Solo Bots recording carries the same result action with team 2 and reason 2. Reason zero for a destroyed crystal follows the verified 5v5 crystal path. ARAL waits six seconds after the death. The local 5v5 host delivers the action in the final flush. The service only has a subset of the original 0x048D counters, so it currently decodes that report offline. The CE platform commits its own results independently of a client's socket read. Display of this result on an unmodified Android client still needs a complete native match.

**Final statistics coverage.** All seven archived ARAL endings and the Android Solo ending contain one 1,614-byte report body immediately before 0x03F1. The native reader consumes the first 1,612 bytes. Across these eight reports the 52 occupied player slots have team 1 or 2 and byte 255 at +0x50 plus the slot index. The 76 unused slots have player ID 0xffffffff, team 255 and byte zero in that column. Both report team IDs at +0x62c are present in all eight. The reader also exposes the four BE32 float bit patterns beginning at +0x63c. These feed minion damage ratios in the published Windows consumer. Their exact numerator and denominator assignments are unresolved. [Consumer analysis](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/evidence/2026-09-08-endgame/native-static-analysis.md)

Every report has 18–31 nonzero bytes in the additional player columns at +0x240 through +0x2ff and 25–67 nonzero bytes in +0x580 through +0x64b. The host lacks five-minute per-player farm and net-worth snapshots, the complete damage-type breakdown, and several team objective counters used by the native statistics consumer. Generating this report from the final roster or ordinary kills/deaths totals would lose these measurements.
The protocol library can edit a caller-supplied, complete report through
`MatchStatisticsMut`. Its checked setters update a selected occupied slot,
team IDs or raw ratio inputs. Re-encoding retains unused slots, unidentified
columns and the original padding. The live host has no complete source for
these reports. [Statistics codec](../crates/protocol/src/statistics.rs)

## Solo replay and live position / actor declarations

The 14 local Android Solo VGR chunks contain 140 full actor declarations
(0x03F3). Each chunk repeats one full declaration for each of the same ten
heroes, with no repeated full declaration for an actor within a chunk. There
are 130 bodies of 746 bytes and ten bodies of 750 bytes. The first compact
structure declarations and later entity updates use other opcodes. These
chunk-local roster snapshots do not show a full reintroduction at a visibility
transition.

Seven played April live ARAL captures contain one opening set of six distinct
hero 0x03F3 declarations each, all with 750-byte decrypted bodies. The
server used 0x042E for 62,855 position
updates in those matches. Each decrypted body is 14 bytes, with a big-endian
actor ID at offset 0, two big-endian floats at offsets 4 and 8, and two zero
bytes at offsets 12 and 13. Every referenced ID was declared earlier in the same capture.
The current position encoder writes the actor as a zero-extended 16-bit ID,
then the two floats. Transport padding produces the observed two zero bytes.
This is evidence for the live shape and not evidence of the Solo transport's
choice of opcode.

The Solo replay contains no 0x042E. It contains 4,397 0x03F8 records with a
one-byte selector and big-endian floats at offsets 1 and 5. All 4,045 14-byte
bodies have five trailing zero bytes, and 352 bodies are nine bytes. Live
0x03F8 can echo a point-command destination, as documented in the live capture
audit. A destination and a current position represent different state, and
the selector has no verified general mapping to the simulator's actor IDs.
The server continues to replicate authoritative positions with 0x042E and
send a declaration before updates for actors that enter its visibility set.
Verified live Solo visibility-transition traffic would be needed to replace
that policy with the native one.
