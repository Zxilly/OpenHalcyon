# Map pings and HUD quick messages

The nine complete TCP captures contain 84 server map pings and 48 server HUD quick messages. Neither opcode occurs in the client direction or in the separately examined Android Solo VGR. Android 147219 establishes the field boundaries. The Windows action readers independently distinguish the numeric words from the floats. The project codec preserves unknown selector values, float bits and HUD padding. [Capture inventory](live-captures.md) · [Independent action readers](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/vg-binary-event-candidates-2026-09-09-details.md) · [Rust codec](../crates/protocol/src/hud.rs)

| Opcode | Fields in wire order | Recorded variants |
|---|---|---|
| `044E` | Actor at +0, target at +4, three big-endian float coordinates at +8/+12/+16, selector at +20, qualifier at +21 | Selectors 2–5 account for 54 map pings. Of those, 41 used the all-ones absent target and 13 named another declared actor. Selectors 7–11 account for the 30 self-targeted records. Qualifiers ranged from 1 to 3. |
| `044F` | Actor at +0, selector at +4, target at +5, big-endian float at +9, word at +13, five transport padding bytes | Selector 0 named another hero, a zero float and an item template in all six records. Selector 1 had an absent target, positive float and item template in all four. Selector 2 had an absent target, positive float and operand 2 in all five. The remaining 33 had absent target, zero float and zero operand. |

All 132 sender words resolved to declared heroes. The six item template IDs match the archived entity manifest entries for Healing Flask (457), Blazing Salvo (463), Chronograph (475), Reflex Block (485), Vision Totem (526) and Seraphic Shell (539). The selected HUD text, meaning of its float and the on-screen ping labels have no synchronized UI observation in this corpus.

The authenticated receiver saw pings from 62 allied and 22 opposing senders, and HUD messages from 36 allied and 12 opposing senders. The project uses the actor-first layout from the retained records and their fixture. The position-first field order in the supplemental notes conflicts with those bytes.

An authenticated ARAL or Solo world connection can send tactical selectors
2–5 with qualifiers 1–3. The host returns accepted pings to the sender and
declared teammates. An actor-targeted ping needs a currently visible,
declared target. The host substitutes its own horizontal coordinates.
Cosmetic ping packs are not relayed.

The host relays operand-free quick-message selectors 3–13 to declared
teammates. GLHF (12) also reaches the opposite team. The supplied table and
prose disagree on the enemy flag for GG (13), so its project scope remains
team-only. Item recommendations and cooldown claims (0–2) remain decode-only.
Pings and quick messages share a per-seat 750 ms interval and are omitted
from spectator streams and server recordings. The native inbound packet flow
remains statically identified; the archive has no inbound samples.
