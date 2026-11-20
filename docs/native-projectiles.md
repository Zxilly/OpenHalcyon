# Projectile actions

The seven played ARAL streams contain 5,719 fire actions. Their first 32-bit words are unique within each match. All 376 detonation records identify one earlier fire by that word. Each of those fires has exactly one recorded detonation. The first two fire words never identify a previously declared actor in these streams. Actor registration slots occur at the end of the fire body. The table uses meaningful body lengths before transport padding. [Capture inventory](live-captures.md) · [Android field widths](ce-game.tsv) · [Independent action readers](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/vg-binary-event-candidates-2026-09-09-details.md)

| Class | Body in wire order | Native / padded bytes | ARAL / Solo occurrences | Later detonation in ARAL |
|---|---|---:|---:|---:|
| `040D` | Projectile ID, profile word, float, halfword, three scene slots | 17 / 22 | 4,751 / 854 | 58 with flag 0 |
| `040E` | Projectile ID, profile word, four floats, halfword, two scene slots | 28 / 30 | 383 / 15 | 207 with flag 1 |
| `040F` | Projectile ID, profile word, eight floats, halfword, two scene slots | 44 / 46 | 585 / 0 | 67 with flag 1 and 44 with flag 0 |
| `0410` | Projectile ID, three floats, flag | 17 / 22 | 376 / 3 | All three Solo detonations match an earlier `040E` and use flag 1 |

All trailing slot bytes in the fire forms resolve to previous declarations at the time of the packet. The first two slots agree in 4,612 of 4,751 ARAL `040D`, 382 of 383 `040E`, and all 585 `040F`. All 869 Solo fires repeat the slot. The independent reader converts the scalar operands as floats. The typed codec preserves their bits, both copies of the slot, and padding. The capture inventory verifies decode–serialize equality on all 6,095 live records. [Projectile codec](../crates/protocol/src/projectile.rs)

## Tower and lane-minion attacks

For ARAL `040D`, the first and final slots resolve to opposing teams in 4,677 records and to the same team in 74. Template 300 occurs in the first slot in 287 fires and template 301 in 81. Every one of those tower fires uses profile word 403962522, positive-zero float, halfword 112, and a repeated source slot. The 71 Solo template-340 fires use the same profile word and float, with halfword 113. The slot that follows the repeated tower slot identifies a declared actor. In 315 of 368 ARAL tower fires the next health impact with matching source and target IDs occurred within one second. None of these 368 tower fires has a matching detonation. All 58 detonated tracked fires came from hero templates. This test permits several projectiles to match one impact and cannot establish a unique damage event for each shot.

For 1,317 ARAL fires from template 287 and 1,094 from template 288, the profile word was 6148364 and the halfword 79. Every one carried a nonzero float whose meaning is unresolved. The single receiving perspective does not establish what another player could see. A missing `0410` does not identify a miss or a hit.

The simulator applies tower and lane-minion damage on attack release. Its delayed basic-attack impacts are restricted to ranged heroes. The archives establish the tower fire body but do not define a travel-time or collision rule for implementing this lifecycle. The four actions remain available for offline decoding and serialization. [Simulation attack resolution](../crates/sim/src/lib.rs)
