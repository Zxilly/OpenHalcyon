# Offline live TCP capture inventory

Run the example against one private archive or its parent directory from the repository root.

```sh
cargo run --release -p halcyon-protocol --example capture_inventory -- /path/to/matches
```

The example reads `match_meta.json` for key derivation and reassembles `packets.bin` separately by direction. It verifies the plaintext client hello and server acknowledgement before counting encrypted frames. A split length prefix, body spanning multiple segments, or multiple messages in one segment is supported. It prints archive ordinals, numeric build/revision metadata when supplied, frame statistics, and boundary diagnostics. A trailing partial segment or frame makes the audit exit unsuccessfully after reporting complete frames. A metadata-only directory is reported separately. Raw bodies, match IDs, player names and endpoint strings stay off stdout.

## April 2026 archive

Nine captures had a complete plaintext hello/ack pair, zero truncated records or trailing frame bytes, and zero field errors. One additional directory had metadata only. No captured archive declared a numeric build in its metadata. Android 147219 is the separately examined interoperability reference.

| Direction | Encrypted frames | Distinct encrypted opcodes | Short Android receiver prefixes |
| --- | ---: | ---: | ---: |
| Client to server | 1,824 | 20 | Receiver check applies to server traffic |
| Server to client | 398,293 | 103 | 0 |

The inventory has 107 distinct encrypted opcodes across both directions. All observed frames expose at least a structural field or a verified header-only shape. Many operands remain unnamed. The decoder retains complete bodies including surplus bytes. The Android receiver explicitly ignores several request opcodes that its own sender can serialize. Applying a field decoder to a captured request confirms its shape; it cannot establish how an official server handled that request.

### Client request crosswalk

Each count is a complete encrypted client frame in these nine recordings. A
reply in this table is backed by traffic where indicated. Host policies for
scope, visibility, rate and game rules are implemented separately.

| Opcode | Count | Request, host behavior and verification |
| --- | ---: | --- |
| 0000 | 1,182 | Clock. The room echoes the frame. On an authenticated Solo scene, a finite nonnegative clock can confirm readiness. There were 1,175 server clocks, so the archive does not establish a one-to-one pairing. |
| 03E8 | 9 | Seat registration following plaintext hello/ACK. The room binds a reserved UUID and emits settings. All nine archives have the handshake and settings. |
| 03F4 | 485 | Directional movement carries two floats. The simulator applies its movement and geometry checks to the authenticated actor. Original Android Solo sent this after joystick input. |
| 03F5 | 22 | Point movement. Every archived request had a matching 03F8 destination and registered selector. The host checks the world registration, padding and legal destination. |
| 0411 | 1 | Lorelai C targets an allied hero. The one observed request resulted in a 0415 after 55 ms; the room emits the confirmation on authoritative release. |
| 0412 | 15 | Point casts. All 15 coordinate/selector pairs reappeared in 0416. Supported mappings are Lorelai A/B and Idris B. |
| 0424 | 4 | Select an attack target. All four received identical six-byte confirmations. Current visibility, hostility and actor life gate acceptance. |
| 0436 | 24 | Learn a slot. The room checks hero-specific grant mapping, available skill points, level and rank. Known requests on templates 254, 439, 273 and 409 have matching 043A operands. |
| 0439 | 18 | Buy an item. The room requires the owned actor, base access, sufficient gold and inventory space. Measured receipts cover 12 templates including a two-component upgrade. |
| 0458 | 15 | Zero-filled draft refresh. All 15 returned mode, catalog, roster and identical echo. Every valid ARAL, HF Ranked or Solo draft refresh receives the current catalog and roster, in addition to the initial setup. |
| 045E | 2 | Hero and skin selection. The native draft validates catalog membership and updates the roster; legacy ARAL uses its assigned roster. |
| 045F | 3 | Skin selection. The room validates the skin against the selected hero and sends a roster update and echo before loading. The stock Solo VS page displayed the selected nondefault skin. |
| 0463 | 1 | HF Ranked draft lock. The single request received a 0459 roster and then identical 0463. The first non-ARAL 0463 lock follows that order. |
| 046B | 9 | One zero input byte plus padding, echoed identically in all nine cases (27–146 ms). The draft handler echoes it. A distinct resulting state change remains unverified. |
| 046D | 7 | Zero-filled world readiness. The ARAL preamble requires the scene request and matching actor before accepting it. Native Android Solo also demonstrated readiness via world input. |
| 046E | 7 | Scene request paired with 0471 on the existing stream. The supported native preamble produces mode, roster and scene contents. |
| 0471 | 7 | Finishes the scene request with the seat's registered actor and flags. Mismatched actors cannot confirm it. All seven played ARAL captures contain the pair. |
| 0472 | 1 | Surrender vote. The confirmed ARAL actor can vote on its own team; the single captured client vote was 1. Team boards and two reason-2 endings corroborate the lifecycle. |
| 0484 | 6 | Talent slot selection. All six requests got an actor-tagged 0481. A confirmed ARAL seat can clear at level zero or use a positive slot when the host has a verified numeric grant for its current hero. |
| 0485 | 6 | Recommended build selection. All six had a matching 0488 for the requesting actor. The room bounds path values and scopes broadcast by mode. |

The 15 draft refreshes all began with six zero body bytes. Between each request
and its matching echo (20–55 ms), the server sent exactly 275 catalog entries.
Thirteen refreshes used the ARAL roster and two used the HF Ranked roster.
For the first ARAL refresh in each of eight streams, a 0480 and a second roster
also appeared. Five later ARAL refreshes omitted that pair. Fourteen intervals
began with 0454; the other began with 045F followed by 0454. All eight 0480 bodies were identical across their complete 86 bytes.
The big-endian words at +0 and +4 were 1 and 5, the byte at +76 was 1,
and the unaligned big-endian word at +77 was 3. All other bytes were zero.
The surrounding rosters matched outside the two timer words. The ARAL host
sends this marker and a freshly timed second roster on the first valid
refresh of each draft control connection. Its gameplay meaning is unknown. The 0463 interval showed the roster after 28 ms and the echo after
36 ms. The independent Windows action-class listing leaves 0458, 0463 and 046B
unnamed. [Independent action-class table](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/vg-binary-event-candidates-2026-09-09-details.md)

Comparing the server inventory with that listing also identifies 0433 as
ActionStartRespawnTimer. All 103 captured records reference a declared hero
and carry a big-endian float duration followed by six zero padding bytes. For
all 63 cases whose next same-actor lifecycle marker was 0432, the observed
respawn occurred within 0.18 seconds of the timer prediction. The one 048F
(ActionRejectBuyItem_Client) followed a server-side 0439. That client stream
contained no purchase request in the preceding three seconds, so it does not
establish a rejection response to the controlled player's own purchase.
Other named server-only actions include projectile, buff, effect, zone, item
and announcement events. Their names alone do not identify additional required
acknowledgements for this client request inventory.

### Live opcodes absent from the 14 local VGR chunks

The counts below exclude the 18 plaintext handshake frames. A dash indicates no encrypted frame in that direction.

| Opcode | Client to server | Server to client |
| --- | ---: | ---: |
| 0x0000 | 1182 | 1175 |
| 0x03E8 | 9 | 16 |
| 0x03EA | – | 10 |
| 0x03EC | – | 6 |
| 0x03ED | – | 18 |
| 0x03F4 | 485 | – |
| 0x03F5 | 22 | – |
| 0x0402 | – | 25 |
| 0x040F | – | 585 |
| 0x0411 | 1 | – |
| 0x0412 | 15 | 842 |
| 0x0414 | – | 48 |
| 0x0421 | – | 22 |
| 0x0423 | – | 22 |
| 0x0424 | 4 | 2268 |
| 0x0425 | – | 5 |
| 0x042E | – | 62855 |
| 0x0433 | – | 103 |
| 0x0435 | – | 210 |
| 0x0436 | 24 | 329 |
| 0x0438 | – | 4 |
| 0x0439 | 18 | 342 |
| 0x0440 | – | 60 |
| 0x0447 | – | 41 |
| 0x0448 | – | 150 |
| 0x044A | – | 66 |
| 0x044D | – | 4 |
| 0x044E | – | 84 |
| 0x044F | – | 48 |
| 0x0451 | – | 248 |
| 0x0452 | – | 5 |
| 0x0453 | – | 4125 |
| 0x0454 | – | 15 |
| 0x0456 | – | 26 |
| 0x0458 | 15 | 32 |
| 0x0459 | – | 92 |
| 0x045A | – | 1085 |
| 0x045E | 2 | 6 |
| 0x045F | 3 | 65 |
| 0x0461 | – | 2 |
| 0x0462 | – | 6 |
| 0x0463 | 1 | 4 |
| 0x046B | 9 | 24 |
| 0x046C | – | 7 |
| 0x0472 | 1 | – |
| 0x0473 | – | 22 |
| 0x0479 | – | 1 |
| 0x0480 | – | 8 |
| 0x0481 | – | 41 |
| 0x0484 | 6 | 41 |
| 0x0485 | 6 | 40 |
| 0x0488 | – | 40 |
| 0x048F | – | 1 |

### Android structural evidence

The arm64 receiver dispatch and emitter prepare operands in network order. W denotes a converted 32-bit word, H a converted 16-bit word, and B a raw byte in the following tables. Each listed family had corresponding whole frames of the expected padded length in the archived traffic. Fixed blocks without a verified interpretation remain accessible in the original message.

| Receiver opcodes | Structure and Android dispatch |
| --- | --- |
| 03EA, 03EC, 0423, 0425, 0451 | 1W. Branches 82BE74, 82BEBC, 82CD44, 82D730 and 82D490. |
| 0402 | W, two BE float32 values, then a 4-byte nonzero test at 82C8F4. |
| 040F, 0421, 0435 | 10W + H + 2B, 6W + B, and 6W respectively. Branches 82CAAC, 82CCE0 and 82D23C. |
| 0433, 0440, 0447, 044D | 2W, W + H + W, 3W, and 2W. Branches 82BA28, 82BD7C, 82CEB8 and 82D7B8. |
| 044E, 044F | 5W + 2B and W + B + 3W. Branches 82D438 and 82D7D4. |
| 0452, 048F | Header only at 82BDCC and 82D710. All observed six-byte transport tails were zero. |
| 046C, 0481, 0488 | B, W + 2B, and W + B. Branches 82CF48, 82CFB8 and 82D6F8. |
| 0473 | Branch 82D5FC converts the leading word and two arrays of sixteen BE32 operands beginning at +4 and +84. |
| 0479, 0480 | Branch 82D864 copies a 64-byte block and converts H at +64. Branch 82D890 converts W at +0, +4, +8 and +77. |

| Outbound opcodes ignored by the Android receiver | Structure from the Android writer |
| --- | --- |
| 03F5 | Two BE float32 operands prepared before the call at 826E50. |
| 0411, 0412, 0414 | W + B, 3W + B, and two bytes (second masked to one bit). Call sites BE66E8, BE6844 and 827448. |
| 0424, 0436, 0438 | W + one boolean byte, one byte, and W + 2B. Call sites 826BD4, 827314 and BE7408. |
| 0448, 044A | W and 4W. Call sites BE86C8 and BE8958. |
| 0456, 0461, 0462, 046B, 0472, 0485 | Each writer sends a single byte. Writer header constants at 8256B0, 825F24, 826050, 826AE8, BE92C8 and 827234. |
| 0484 | A byte widened into a little-endian four-byte slot after a 0xff mask at BE77BC. |

The independent Windows action-class study associates 03F5 with a movement request, 0411/0412 with ability requests, 0424 with attack-target selection and 0436 with learning an ability. The tables above establish field boundaries. Slot identity, coordinate meanings and server decisions require independent evidence. [Android-compatible structural views](../crates/protocol/src/live.rs) · [Independent action-class study](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/vg-binary-event-candidates-2026-09-09-details.md)


### Request and world-action pairs

All 22 client 03F5 two-float movement requests recurred at offsets +1 and +5 in subsequent server 03F8 movement records. All 22 server records carried selector 19. All 15 client 0412 requests had their complete three-word vector and selector reproduced in a server 0416 at offsets +4 through +16, 35–142 ms later. The local spawn manifest identified Lorelai for selectors 0 and 1 and Idris for selector 3. Lorelai's single client 0411 entity-target request with selector 2 recurred in 0415 at +4 through +8 after 55 ms. All four client 0424 requests named a spawned actor and carried flag zero. The 24 client 0436 learning requests used 0, 1 or 2; nearby server 043A grant operands could differ by hero. These correlations do not establish a universal mapping from network selectors to the simulator's ability slots.

### Authoritative hero respawn

All 63 live ARAL 0x0432 records and all six Android Solo 0x0432 records
follow a prior death of the same actor. The wire reader consumes the actor
and three big-endian position floats. The independent Windows formatter
for ActionActorRespawn writes those same 16 bytes. OpenHalcyon emits this
action when its respawn timer restores a hero's health and position. Its
planar world supplies X and Z and uses zero for height. Visible players,
authorized observers and their replay records receive the same position.
Ordinary minion births and reconnect declarations do not trigger this action.

The room supports the captured Lorelai and Idris point forms and Lorelai's targeted ally form. Hero-specific learning is broader than the observed cast inventory. [Ability evidence and release rules](native-abilities.md) · [Shop receipts](aral-shop.md) · [Surrender boards](aral-surrender.md)

## Recommended build path

Seven archived ARAL streams contain 40 server-side `0x0485` one-byte path announcements and 40 `0x0488` setters. A setter carries the player-info actor as BE32 followed by the path byte. Of the 40 setters, 27 carry `0xfe`, four carry 0, six carry 1 and three carry 2. The receiving seat could see 19 setters for its own team (including itself) and 21 for the opposing team. Six recordings also include the local client's `0x0485` request. In each case a setter with the same byte and the local `0x0471` actor followed after 29–100 ms. Other server records can intervene.

The host accepts those four values from the authenticated world connection and derives the actor from its seat. A valid Solo choice following the flushed native opening can complete the world handoff just like the existing movement or clock inputs. The requester receives an announcement and actor-tagged setter. Other admitted ARAL seats receive changes for both teams. Other supported rooms share changes within the selecting team. Reconnect and observer snapshots replay the latest actor-tagged state; repeated requests receive confirmation without another peer broadcast. These path bytes are retained independently of shop contents and the end report's item-build classification.

The four ARAL client attack-target selections (0x0424) were followed by identical six-byte server bodies 20, 44, 46 and 51 ms later. The authenticated ARAL seat receives that confirmation after its simulator accepts a living, visible opponent. A friendly, hidden or absent target produces no confirmation.

### ARAL talent equipment

Six 0x0484 requests and 41 0x0481 equipment notifications appear in the seven archives. Every notification refers to a previously declared hero. The six requesting players received an actor-tagged reply with their choice within 44–122 ms. The request carries a selector. The notification carries the actor, the selector and its level. For selector 255 all five server notifications had level zero. For selectors 0, 1 and 2 the archives contain several distinct levels for each selection. A confirmed ARAL connection can clear the player's talent and receive its own 0x0481 NoTalent action.

The exported public definitions contain 171 talents for 57 heroes. Ringo has the three public IDs `ringo_rare`, `ringo_epic` and `ringo_legendary`, associated with LongShot, BulletSplit and SpikedBrew respectively. A fourth Ringo resource appears in the extracted definitions and has no matching entry in the public manifest. The two observed Ringo equipment records contain slot/level pairs (1,10) and (2,1), without a public talent ID. The order of resource strings also varies between hero definitions. Mapping a positive selector to a named talent requires a matching runtime catalog or an observation linking the selector to its equipped name. Applying the original per-level modifiers requires a verified scaling rule. The platform serves the player's local balances separately. [Local talent progression](local-content.md)


Three positive equipment broadcasts can be linked to a same-session
getTalentsData response for the controlled hero: Krul (slot 2, level 1),
Yates (slot 2, level 1) and Idris (slot 0, level 1). Each response lists
all three named talents for that hero at level 1. The other 33 positive
broadcasts refer to other heroes without linked account inventories. The
31 archived notifyPlayerAction requests reported leaderboard views. A named
equip event tied to a selector, or a matched inventory with a unique level,
would distinguish the corresponding talent.

All 41 actor-tagged notifications had a corresponding preceding server 0484
with the same selector. Six applied to the receiving seat, 14 to other
allied seats and 21 to opposing seats. The room shares a changed selection
across both teams and live authorized observers. A duplicate receives its
confirmation on the issuing connection; it produces no second public change.
Fresh player and observer scenes restore the actor-tagged selections. The
game recorder retains those authoritative 0481 records. A positive selection
uses the immutable numeric level bound to that seat and its current hero in
the reservation. Equal verified ownership levels for all three candidate
talents suffice to establish that number without choosing a named effect.

### Item reorder and activation

Seven ARAL recordings establish the incoming network direction independently of the serializer names. Across the seven captured client perspectives, all four `0438` actions refer to a different hero from that connection's `03E9` actor. The slot byte pairs were (4,3), (3,2), (6,5) and (3,2). All 150 `0448` messages name item instances previously granted by `043D` to another hero. In 142 cases a later `044C` for the same instance appeared within 250 milliseconds. The other eight have no such nearby match. Those comparisons use stream order and the observed grant owner. They do not establish successful use, cooldown or item consumption.

| Opcode | Client to server | Server to client | Decrypted body |
| --- | ---: | ---: | --- |
| 0438 | 0 | 4 | Actor and two slot bytes (6 bytes) |
| 0448 | 0 | 150 | Item instance and two opaque trailing bytes (6 bytes) |
| 0449 | 0 | 0 | Two-word request prefix from independent serializer research |
| 044A | 0 | 66 | Three vector words, one argument, six trailing bytes (22 bytes) |
| 044C | 0 | 218 | Actor, item instance, target word, two coordinate words, mode and a trailing byte (22 bytes) |

The protocol reader retains raw argument and coordinate bits and all trailing bytes. For `044C`, the independent receiver description in `REA_ITEM_ACTIVATION_2026-10-14.md` identifies a target in mode 0 and a position in mode 1. Mode 2 consumes neither. Other modes retain their raw operands. Independent documentation also describes compact outgoing request serializers. Implementing native player item use or reordering still requires a controlled client request tied to an owned item or slot and its resulting state change.

### Item sales

All four ARAL `044D` records were server-to-client. In every case the two
big-endian words matched the actor and item instance of an earlier `043D`
grant. Within 70 ms the same actor lost that instance via `044B`, followed by
a stat-6 `041D` carrying the new balance (mode 1). The six trailing bytes
were zero in these records. The typed reader also preserves nonzero suffixes.

| Sold template | Balance immediately before sale | Reported balance | Difference |
|---|---:|---:|---:|
| 473 | 1661.25 | 1786.25 | 125 |
| 463 | 1275 | 1625 | 350 |
| 519 | 3695 | 4895 | 1200 |
| 495 | 2210 | 2235 | 25 |

The pre-sale balances use the 2000 ARAL opening balance and all preceding
mode-0 stat-6 deltas for the same actor. The first three sales also reversed
item stats through `041C`. The fourth had no accompanying stat reversal.
No same-actor reorder occurred within two seconds of any sale. The seven
client streams contain zero `044D` requests. These results support decoding
a completed sale by owned instance. The client request path, sale eligibility
and refund policy for other items remain unverified.

[Server emission matrix for all 103 observed opcodes](server-emissions.md)

### ARAL announcements and flyout text

All seven ARAL connections received seven consecutive `0435` rows per
announcement. Word 0 ran through native hero IDs 1505 down to 1500, then
`ffffffff`. Each connection saw all seven rows, including the opponents'
rows. Two triggers repeated in all seven matches:

| Trigger | Word 1 | Word 2 | Word 3 | Words 4–5 |
|---|---|---|---|---|
| First two template-289 lane spawns, about 15.1 seconds after start | Same as word 0 | `140be4b6` | `ffffffff` | Both `ffffffff` |
| First hero death, killed by another hero in all seven | Killer's native actor ID | `46482c5d` | 125 | Both `ffffffff` |

The original rows followed the spawn or death records. The host emits these
two sequences once per ARAL room, after normal replication, to registered
native world connections and active authorized observers. A reconnect receives
future announcements. If a nonhero causes the first hero death, the simulator
uses up first blood. The host therefore skips its announcement rather than
assigning a later hero kill the bonus. Announcement delivery does not modify
gold. Other captured keys lack equivalent coverage of their triggering rule.

The 60 `0440` flyouts had three operands: actor, 16-bit selector, and
`ffffffff`. Selectors 5, 7, 8, 13 and 16 appeared 8, 33, 9, 8 and 2 times.
All 60 were within 250 ms of a buff application to the same actor. For
selector 5 the nearest such application by record order used definition 740
in 8/8 cases, and for selector 13 it used 806 in 8/8. Only 2/60 flyouts
were within that window of a death involving the actor, and 33/60 of a
same-actor gold change. No rendered text or second simultaneous player
perspective establishes the label or recipients. The host reads these flyouts.
All 14 Solo VGR chunks (43,160 records) contained zero `0435` and `0440`.
