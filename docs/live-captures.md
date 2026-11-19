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

The available point and entity ability requests are from Lorelai and Idris.
Neither has an authoritative spell implementation in the current simulation.
The capture does not bind those heroes' selectors to the implemented Ringo,
Adagio, Lyra, SAW, Karas or Shin casts.

## Recommended build path

Seven archived ARAL streams contain 40 server-side `0x0485` one-byte path announcements and 40 `0x0488` setters. A setter carries the player-info actor as BE32 followed by the path byte. Of the 40 setters, 27 carry `0xfe`, four carry 0, six carry 1 and three carry 2. The receiving seat could see 19 setters for its own team (including itself) and 21 for the opposing team. Six recordings also include the local client's `0x0485` request. In each case a setter with the same byte and the local `0x0471` actor followed after 29–100 ms. Other server records can intervene.

The host accepts those four values from the authenticated world connection and derives the actor from its seat. A valid Solo choice following the flushed native opening can complete the world handoff just like the existing movement or clock inputs. The requester receives an announcement and actor-tagged setter. Other admitted ARAL seats receive changes for both teams. Other supported rooms share changes within the selecting team. Reconnect and observer snapshots replay the latest actor-tagged state; repeated requests receive confirmation without another peer broadcast. These path bytes are retained independently of shop contents and the end report's item-build classification.

The four ARAL client attack-target selections (0x0424) were followed by identical six-byte server bodies 20, 44, 46 and 51 ms later. The authenticated ARAL seat receives that confirmation after its simulator accepts a living, visible opponent. A friendly, hidden or absent target produces no confirmation.
