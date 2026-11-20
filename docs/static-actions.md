# Fixed game actions with static field evidence

The [Android receiver length table](../crates/protocol/src/metadata.rs) has 38 fixed-size entries absent from both the nine local live streams and the 14 VGR chunks in the coverage inventory. The [static action table](../crates/protocol/src/static_actions.rs) includes those entries. Its class names and primitive conversions are sourced from the Windows receiver branches in the offline action-class evidence bundle, whose opcode notes are indexed by the [independent action-class report](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/vg-binary-event-candidates-2026-09-09-details.md). Where the recovered Windows copy width agrees with Android, the table exposes visible word, half-word and float conversions. It keeps byte operands only where the static consumer reads them. The action names document the static reader classification. Direction, field meaning and simulated behavior still require corroborating traffic.

| Entries | Static coverage |
| --- | --- |
| `03EB`, `03FC`–`0401`, `0404`, `0405`, `0408`, `040A` | Converted movement floats, reference words and individual byte operands. In particular, `03FC`, `03FE`, and `040A` read four floats beginning at body offset 1. |
| `0417`, `0418`, `0420`, `0422`, `0427`, `0437`, `043B`, `0442`, `0444` | Known fixed primitive offsets for abilities, zones, item and buff actions. Operands dropped by the decompiler or omitted in a branch stay opaque. |
| `045B` | Four BE32 words at +4, then sixty at +24. Ten rows have pairs of BE16 values beginning at +273 with stride 166. The floating-point operands are at +2124 and +2128. The layout's role is unknown. |
| `0475`, `0476`, `0477`, `047E`, `047F`, `0482`, `0483` | Tutorial or state, speed, spectator and talent action operands. |
| `0487`, `0489`, `048B`, `0490` | Build path, ability behavior, buff variable and swap-target operands. In `0487` the count byte at +96 bounds BE32 words beginning at +4. Counts above 23 leave the word region opaque. |
| `0429`, `047C` | The existing specialized ability-charge and control-byte decoders take precedence. |
| `0005`, `0006`, `03F0`, `047D` | Length or name only in this table. `0006` retains its established heartbeat decoder. Its copied Windows byte disagrees with the Android body length, as does the copy for `03F0`. The `047D` branch copies four bytes without a verified byte-order conversion. |

The 32 new views accept exactly the declared body size or its Blowfish block-padded size. They enumerate typed primitives by body offset while retaining all original bytes. Float variants carry IEEE-754 bits so NaNs and signed zeros compare consistently. Re-encoding preserves opaque fields and nonzero padding. These views are diagnostic and create no game commands or server emissions.

The [round-trip tests](../crates/protocol/tests/static_actions.rs) exercise all 38 entries using synthetic input, with checks for malformed widths, preservation of the original ciphertext after decoding, distinct BE16 and BE32 operands, float bit patterns and out-of-range count handling.
