# Legacy ARAL wire and room lifecycle

The iOS-era platform traffic records `joinLobby` with the queue ID `casual_aral`. The server reserves six human seats for this exact ID. Android 147219 contains the separate static string `aral_casual`. This implementation leaves that spelling and the 5v5 queues on their existing transport paths.

## Full 0x045A roster

Seven live ARAL capture files contain 928 complete roster frames. An additional ARAL draft-only capture contains 157. The original encrypted frames survive in the local iOS-era `HackedGlory/mitm/matches/*/packets.bin` archives. The generated JSONL logs truncate large plaintext messages. The following sizes exclude the TCP length and opcode.

| Field | Body location |
| --- | --- |
| Remaining and allotted seconds | Big-endian floats at 0 and 4. Allotted time was 20 seconds. |
| Slot records | 16 records of 161 bytes starting at 0. The timer occupies the first eight bytes of slot zero. |
| Occupancy, slot ID, selected, locked | Slot offsets 8, 9, 10 and 11. Slots 0–5 were occupied throughout. |
| Team, hero template, actor, skin | A byte at slot offset 16, big-endian u16s at 17 and 19, and a big-endian u32 at 21. All occupied players had actor IDs 1500–1505 and teams 1 or 2. |
| Display name and player identity | NUL-padded spans at slot offsets 25–56 and 105–140. |
| Uninterpreted bytes | Each slot retains its unknown fields. The structural record ends with eight raw bytes, then optional six-byte transport padding. |

The decoder exposes the entire 2,584-byte structural body or 2,590-byte padded body, including empty slots. It can still read the two-timer prefix of older truncated exports. Reproduction sources are the local three April 3 captures and April 4 captures ending in `8454884c`, `36d93919`, `2becf48d` and `2f7085e8`. The additional draft-only archive ends in `f46a96d0`. No captured identities or raw bodies are included in the repository.

## Same-connection loading

The captures use one authenticated TCP stream through draft and play. In all seven, the initial 0x03E9 preceded the first 0x0454 and catalog. The measured 0x0454 prefix was BE u32 3. The first 0x03E9 advertised three seats on each side, and byte 100 agreed with the authenticated player's team in 0x045A. There was no second 0x03E9 at the transition.

1. At the end of the draft, the server sent one wave of six full 0x03EE entries and 0x046C with a zero structural byte.
2. The client sent 0x046E and 0x0471 on the same stream. In all seven captures they followed 0x046C by approximately 7–10 seconds. The 0x0471 carries the sender's own actor ID, followed by 1 and transport padding.
3. The server sent 0x046F, six full 0x03EE entries, 0x0451 with zero float and the first 0x03F3 about 14–26 ms after those client frames. The mode string was `*GameMode_Aral_Casual*` with zero in the final structural byte. No 0x0470 appears between the roster waves.
4. After the first spawns the client sent 0x046D. A later 0x03EF preceded active play. The server uses six authenticated readiness sequences as its start condition and sends 0x03EF once to the connected players before enabling input.

The server sends the two loading waves in this order and runs a six-hero simulation with zero synthetic heroes. Encrypted loopback tests exercise all six streams, including rejection of another actor's readiness ID, the load barrier and movement on the original socket. The authored lane layout shares the server combat and ability rules. Native map parity, mode-specific rerolls, native reconnect behavior and iOS rendering against this implementation remain unverified. The captured per-hero 0x048A records immediately after spawn use keys and tails that vary across heroes. The initial configuration for Lorelai A/B/C and Idris B is verified. [Cooldown key and scope](native-abilities.md#cooldown-keys-and-initial-state)

The Android 147219 reader copies 0x046C's one-byte body in the case at `0x82cf48` and dispatches to the registered listener through `0x8e0854`. The 0x0451 case at `0x82d490` byte-swaps the first four bytes. Its reader at `0x82b300` loads a float and calls `0x8bd5c0`, which stores it in the active object. All 248 captured 0x0451 frames were server-to-client. After the initial zero values their elapsed-time floats and arrival timestamps advanced approximately every 10 seconds. The legacy ARAL path reports time every 10 simulated seconds. The ARAL wire order does not establish the full Android 5v5 transition.

All five archived crystal defeats include a header-only 0x0452 immediately before the 0x0430 crystal death (one intervening position update in one match). The statistics report follows 6.05–6.10 seconds later. The two other archived endings contain no 0x0452. The ARAL host announces postgame on the authoritative crystal-death tick to all six seats and authorized observers, before publishing the final report and winning team after the delay.

## Authoritative lane scene

Seven played ARAL captures agree on six hero pads in team roster order.
Home pads are (-78.18, 0.88), (-81.81, 1.97), and (-77.03, -1.94).
Away pads are (79.66, -0.91), (78.60, -5.06), and (83.22, 0.72).
The full hero declaration carries these X/Z coordinates from the simulator,
team height 1.2973974 or 1.7773974, and actor-order slots 16–21.
Movement requests and authoritative updates share these world coordinates.

The initial scene sends 16 map objects in 0x03F2 before 0x03EF. Slots
1 and 9 use crystals 292 and 293. Slots 3–7 and 11–15 use tower templates
300/301. Their positions and health come from the simulated actors.
Shops are template 314 in slots 0 and 8. Beacons are template 298 in slots
2 and 10. The four scenery records are stable on rejoin, where simulated
actors still obey the team view.

All 112 static declarations across seven matches use actor ID
2000 + slot, in descending slot order. The room reserves that entire
actor range and starts dynamic allocation after it. Shops and beacons
are presentation-only. They do not take damage, grant buffs or vision,
or alter purchase authorization.

Successful purchase records include the request acknowledgement,
a gold debit and an item grant from hero pads 9.02–12.13 units away from
the visual shop. The captures do not establish a shop-centered purchase
radius. The server checks its existing ARAL home-pad purchase zone in
the authoritative transaction.

Across the seven captures, 784 first wave-minion declarations use mask 1,
routes 0/1, and the observed templates 287–289. All omit numeric HP
fields. Their wave markers at +119 are 2, 3, or 4. The server uses this
shape for pristine simulated wave creation with the corresponding levels.
Its full 0x03F3 snapshot preserves actual HP when reintroducing a damaged
minion or restoring a scene. Those two declaration forms share the actor
slot. Dynamic slots start at 22 and never overlap the fixed room actors.
The allocator recycles only after the delayed 0x040B destroy for all teams
that were sent its declaration. Where a death cannot be disclosed, its slot
stays unavailable. Capacity exhaustion defers the declaration and associated
updates, while the actor remains in the simulation.

A six-client Rust socket test verifies the measured pad declarations,
all 16 compact map slots, authenticated readiness and point
acknowledgement, actual motion in those coordinates, and a rejoining seat.
The terrain and server-side replication are a supported subset. Rendering
of the new scene and wave packets on the original client remains unverified.

### Delayed actor death and lane slot reuse

In seven ARAL game streams, all 958 server 0x0431 records followed a
same-actor 0x0430 once per death. Each body is a BE32 actor and two zero
padding bytes. The observed delay depends on the kind of actor.

| Actor | Deaths with 0x0431 / observed 0x0430 | Time from 0x0430 |
| --- | ---: | --- |
| Hero | 73 / 74 | 1.726–1.917 s, median 1.818 s |
| Lane minion 287 | 173 / 174 | 3.717–3.932 s, median 3.803 s |
| Lane minion 288 | 170 / 173 | 3.668–3.876 s, median 3.804 s |
| Lane minion 289 | 347 / 355 | 3.674–3.925 s, median 3.810 s |
| Tower or crystal | 0 / 40 | No delayed record observed |

All 690 lane-minion 0x0431 records preceded a same-actor 0x040B in the
same timestamped receive segment. Other actors' events could intervene.
Each of the 607 observed subsequent lane-minion slot reuses followed that
destroy. After 63 of the 73 hero markers the same hero respawned via 0x0432.
The remaining ten had no later recorded death, destroy or respawn for that hero.

The room uses 1.8 seconds for heroes and 3.8 seconds for its three lane
minion templates. A hero retains its registration. A lane minion's
registration becomes reusable when every team that saw it has completed
the destroy sequence. A hidden death can keep the slot reserved. The
notification goes to the world connections that received the death and to
the same still-authorized observer streams. Reconnection begins from the
current scene. These timings are hosting rules derived from recorded
network arrival times. Other transient templates exhibited different delays.


### Draft seat registration

Sixteen archived server `0x03E8` records carry another seat's UUID already
present in the preceding draft roster. Each decoded body is 70 bytes, with
zeros after the 36-byte UUID. Fourteen first occurrences across three streams
arrive 0.174–1.194 seconds after connection setup. Each is followed by another
registration or roster refresh. An independent Windows sender supplies the
64-byte original body size, which pads to 70 in the encrypted frame. Two
later live records are outside the draft, including one repeated UUID. Their
trigger remains open.

The host sends the draft shape to currently connected seats on the first
successful control registration of a different reserved seat. It never
replays past registrations to a newcomer. Rejected, duplicate and resumed
registrations do not produce a new draft notification. Spectators and seats
in other rooms do not receive it.
