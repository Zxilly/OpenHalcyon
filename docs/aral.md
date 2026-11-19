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

The server sends the two loading waves in this order and runs a six-hero simulation with zero synthetic heroes. Encrypted loopback tests exercise all six streams, including rejection of another actor's readiness ID, the load barrier and movement on the original socket. The server still uses its existing lane, combat, and ability rules. Native map parity, mode-specific rerolls, native reconnect behavior and iOS rendering against this implementation remain unverified. The captured per-hero 0x048A records immediately after spawn use keys and tails that vary across heroes. Their authored maps remain unresolved.

The Android 147219 reader copies 0x046C's one-byte body in the case at `0x82cf48` and dispatches to the registered listener through `0x8e0854`. The 0x0451 case at `0x82d490` byte-swaps the first four bytes. Its reader at `0x82b300` loads a float and calls `0x8bd5c0`, which stores it in the active object. All 248 captured 0x0451 frames were server-to-client. After the initial zero values their elapsed-time floats and arrival timestamps advanced approximately every 10 seconds. The legacy ARAL path reports time every 10 simulated seconds. The ARAL wire order does not establish the full Android 5v5 transition.
