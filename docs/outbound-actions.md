# Outbound packers for ignored Android tags

The Android 147219 inbound reader ignores 48 opcode values. The [outbound
inventory](../crates/protocol/src/outbound.rs) covers the same 48 values. The
nine local captures and 14 VGR chunks contain examples of 23 of them. Static
Android send wrappers establish body widths for 43, and an independent Windows
formatter establishes matching widths for 36. Eighteen of the matching
formatters also have a Windows action-class association. A writer on one
client does not establish an original server's acceptance or simulation rules.

The Android scan accepted a candidate only when the opcode literal was stored
with the length and payload in a call path using the shared packet writer. It
excluded the `040C` literal in an unrelated instruction sequence. The
[Windows formatter report](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/vg-binary-event-candidates-2026-09-09-details.md)
and companion `packet-emitters.json` and `outbound-candidates.json` files
supply the independent references.

| Tags absent from the nine captures and 14 VGR chunks | Result |
| --- | --- |
| `03F6` | Android wrapper `0x826F54` moves two float registers into BE words. Windows associates the 8-byte third writer with `ActionRequestMoveTo_Client` through `0052A420 → 004D63F0 → 004CF4F0`. The alternative trigger and coordinate interpretation in an original match still require verification. |
| `0413` | Android wrapper near `0xBE6970` converts four BE32 words and copies a trailing byte. Windows links the 17-byte serializer `008183C0` to mode 2 of `ActionRequestActivateAbility`. The meanings of the active operands remain unverified. |
| `0449` | Android wrapper near `0xBE87E8` and Windows helper `0095BEA0` agree on two BE32 words. The existing item decoder takes precedence. |
| `0455`, `0465`, `0474`, `047B` | Both clients write one byte. A separate Android draft capture contains `0455`, and its existing control view takes precedence. Windows identifies `0474` as `ActionSpectatorExitMatchRequest`; the Android wrapper supplies zero. The other raw byte roles are unresolved. |
| `045D`, `0464`, `0486` | Matching 4-byte bodies and Android BE32 conversion at `0x8259DC`, `0x82624C` and `0xBE78DC`. The wrapper for `045D` first masks its operand to eight bits. Windows identifies `0486` as `ActionRequestLevelLogicEvent_Client`. |
| `0467`, `0468` | Matching 2-byte bodies. Android wrappers near `0x8265D0` and `0x826700` write two separate bytes. |
| `0466` | Matching 4-byte body. The Android wrapper copies a prepacked operand. Its byte order and purpose remain unresolved. |
| `03F7`, `0450`, `0457`, `0460`, `0469`, `046A`, `048E` | Android writer width established. Independent formatter corroboration is unavailable in the local bundle. These seven retain opaque payload views. |
| `040C`, `0426`, `042A`, `042D`, `047A` | No confirmed writer width in either source. |

For the 43 known widths, the library accepts the native body length and the
matching Blowfish-padded size. It reproduces opaque operands and nonzero
padding byte-for-byte. Its separate outbound fallback leaves existing capture
decoders in place and adds diagnostic views only. No game dispatch or replay
emission is attached to these views. The [tests](../crates/protocol/tests/outbound.rs)
compare every tag against the inbound ignore inventory and exercise encrypted
round trips for every known outbound width.
