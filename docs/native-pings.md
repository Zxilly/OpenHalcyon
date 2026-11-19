# Map pings and HUD quick messages

The nine complete TCP captures contain 84 server map pings and 48 server HUD quick messages. Neither opcode occurs in the client direction or in the separately examined Android Solo VGR. Android 147219 establishes the field boundaries. The Windows action readers independently distinguish the numeric words from the floats. The project codec preserves unknown selector values, float bits and HUD padding. [Capture inventory](live-captures.md) · [Independent action readers](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/vg-binary-event-candidates-2026-09-09-details.md) · [Rust codec](../crates/protocol/src/hud.rs)

| Opcode | Fields in wire order | Recorded variants |
|---|---|---|
| `044E` | Actor at +0, target at +4, three big-endian float coordinates at +8/+12/+16, selector at +20, qualifier at +21 | Selectors 2–5 account for 54 map pings. Of those, 41 used the all-ones absent target and 13 named another declared actor. Selectors 7–11 account for the 30 self-targeted records. Qualifiers ranged from 1 to 3. |
| `044F` | Actor at +0, selector at +4, target at +5, big-endian float at +9, word at +13, five transport padding bytes | Selector 0 named another hero, a zero float and an item template in all six records. Selector 1 had an absent target, positive float and item template in all four. Selector 2 had an absent target, positive float and operand 2 in all five. The remaining 33 had absent target, zero float and zero operand. |

All 132 sender words resolved to declared heroes. The six item template IDs match the archived entity manifest entries for Healing Flask (457), Blazing Salvo (463), Chronograph (475), Reflex Block (485), Vision Totem (526) and Seraphic Shell (539). The selected HUD text, meaning of its float and the on-screen ping labels have no synchronized UI observation in this corpus.

The authenticated receiver saw pings from 62 allied and 22 opposing senders, and HUD messages from 36 allied and 12 opposing senders. This single-receiver evidence leaves recipient selection and the original input path unresolved. Both actions are available for offline decoding and lossless serialization. The host currently emits neither.
