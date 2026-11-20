# Native movement actions

The Android 147219 reader sizes for `03F9`, `03FA` and `03FB` are 18, 24 and 21 bytes including the four-byte action header. The Windows action readers independently consume 14, 20 and 17 body bytes and convert three big-endian floats in each case. The Rust decoder accepts both the original body and its padded transport length. The typed writer preserves the flag, float bits and any trailing bytes. [Android sizes](ce-game.tsv) · [Independent action readers](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/vg-binary-event-candidates-2026-09-09-details.md) · [Typed writer](../crates/protocol/src/native_motion.rs)

| Action | Fields at body offsets | Native / padded bytes | Live / Solo records |
|---|---|---:|---:|
| `03F9` ActionMoveToServerAuthoritative | Registration slot +0, float triple +1, raw flag +13 | 14 / 14 | 2,548 / 24 |
| `03FA` ActionMoveToAndFace | Actor +0, referenced actor +4, float triple +8 | 20 / 22 | 1,978 / 48 |
| `03FB` ActionStopActor | Actor +0, float triple +4, raw flag +16 | 17 / 22 | 1,829 / 34 |

At each record in the seven played ARAL captures and in the Android Solo VGR, the `03F9` slot resolved to an earlier declaration. Both `03FA` actor IDs and every `03FB` actor ID also resolved. All 1,978 ARAL move-and-face records reference an actor on the opposing team. Of the ARAL stops, 1,206 repeated the exact three float words of the same actor's most recent move-and-face action. The middle float in every Solo record was approximately 0.06. ARAL stop records used values ranging from 0.04 to 3.81. These observations support a position triple and leave the rule for choosing the middle value unresolved.

The `03F9` final byte was 1 and the `03FB` final byte was 0 throughout both corpora. Their meanings and the server's selection among these actions remain unverified. The live corpus contains these opcodes only in the server direction. [Capture inventory](live-captures.md)
