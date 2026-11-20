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

## Authored ARAL tracked fire

In the seven played ARAL matches, no fire ID collided with any declared actor. The project allocates a fresh ID from the simulation's actor allocator when the deferred projectile actually launches. All receivers use that same projectile ID. The three scene indices are resolved separately on each receiver. For all 2,779 fires from templates 287, 288, 300 and 301, the first two slots were equal and the third identified a declared opponent. Each released ARAL tower fire had a preceding same-source, same-target selector-zero ability play within 1.5 seconds.

| Source | Target | Captured fires | Profile word / halfword | Float at +8 |
|---|---|---:|---|---:|
| ARAL tower 300 or 301 | Hero or lane minion | 368 | 403962522 / 112 | 0 |
| Level-2 minion 287 | Hero / lane minion / structure | 383 / 591 / 125 | 6148364 / 79 | 14 / 46.5 / 100 |
| Level-2 minion 288 | Hero / lane minion / structure | 303 / 534 / 86 | 6148364 / 79 | 47 / 62 / 100 |

The level here is the source's compact spawn level. All 2,022 level-2 minion records match these six combinations. The codec keeps the float as an operand. For example, only 31 of 1,005 template-287 fires followed within one second by a same-pair impact had any such impact equal to that operand in magnitude. Higher-level minion records showed multiple operands for the same source template and target kind.

For the supported profiles, the host sends `040D` at authoritative launch to active ARAL scenes where both participants are visible and registered. The read-only observer uses its own registered slots. A reconnect receives subsequent launches. The VGR writer retains the supported profiles within the player's delivered stream. Damage remains a separate simulation event. None of the captured tower or lane-minion fires matched a detonation, so the host emits no `0410` for these attacks. The point and directed fire variants remain outside this production path. [Network projection](../crates/game/src/npc_projectile.rs) · [Simulation attack resolution](../crates/sim/src/lib.rs)

## Solo outer towers

Among the 71 tracked launches by template 340, 58 targeted the three lane
minion templates below. Every one came from a tower declared at slot 9, 11
or 13. The wire uses the ARAL tower profile word 403962522 and positive zero
scalar, with Solo operand 113. Each recipient must currently see both actors
and have declarations for both on its active scene. The two source slots
repeat that tower's slot, and the last names the particular target. A reused
minion slot loses its previous binding. The recorder accepts this specific
Solo shape. No corresponding detonation was observed.

| Minion | Releases matched to launch | Unique paired negative impacts | Median release to launch (s) | Median launch to impact (s) | Host ticks before/after launch |
|---|---:|---:|---:|---:|---:|
| 335 ranged | 14 | 12 | 0.091 | 0.357 | 2 / 7 |
| 336 melee | 28 | 26 | 0.091 | 0.342 | 2 / 7 |
| 337 siege | 16 | 16 | 0.096 | 0.274 | 2 / 5 |

These are host timing approximations rounded to the nearest 50 ms from the
paired receive-time medians. Target position samples are sparse. The host
uses these target classes without estimating projectile speed or borrowing
ARAL's distance curve. A shot gets its ID at launch and can impact after its
tower dies. A dead or removed victim takes no subsequent damage. The 13
recorded shots against three hero templates and towers outside these three
source slots retain their existing simulation timing.
