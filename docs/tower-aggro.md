# Tower target states

Every one of the 738 ARAL server frames for `048C` maps its first byte to a previously declared tower slot. Template 300 accounts for 537 frames and template 301 for 201. An active target occupies the second slot byte in all 299 assignments. Its latest declaration identifies an opposing hero or lane minion in every case, with 201 team-one towers targeting team two and 98 in the other direction. Each assignment changes the target held in the preceding record for that tower. Of 439 clears, 264 follow an active target, 70 initialize a tower and 105 repeat a clear. The aggregate uses all seven played ARAL captures, updating slot ownership at each compact or full declaration. [Capture method](live-captures.md)

All 111 records in the separate Android 147219 Solo VGR have the same form. Template 340 has 43 target assignments and 56 clears. Templates 339 and 341 each have six clears. The independent Windows action-class study identifies the action and the unaligned float conversion. Neither the symbol nor the data establishes a unit for the float. [Independent action evidence](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/vg-binary-event-candidates-2026-09-09-details.md)

| Offset | Representation | Observed values |
|---|---|---|
| +0 | Byte | Tower scene registration slot |
| +1 | Byte | Opposing target slot when active, 255 when clear |
| +2 | Big-endian float | 9.0 on all 849 records |
| +6 | Byte | 1 when active, 0 when clear |
| +7..13 | Seven bytes | Zero on all 849 framed bodies |

The host emits the current target after the tower and target have been declared to that receiver. It clears the state when the target dies, becomes hidden or loses its registered slot. It restores the state when the tower becomes visible again or a fresh native scene registers both actors. Each recipient sees changes to its own view. Authorized observers use independent scene registrations and can see both sides. The original trigger for 105 repeated ARAL clears remains unresolved. [Room replication](../crates/game/src/tower_aggro.rs) · [Wire codec](../crates/protocol/src/tower_aggro.rs)

The 2,268 server `0424` records contain a target word and a flag. In 1,736 records the word resolves to a declared actor and the flag is zero. Another 520 reference declared actors with flag one. The remaining 12 use the all-ones word and zero flag. Four originating client selections received byte-identical acknowledgments. The server perspective supplies no attacker field for the other records. There are only five `0425` frames. None immediately follows a matching `0424` in the next 15 records within 120 ms. The host therefore limits `0424` output to the authenticated, accepted originating selection. [Client request inventory](live-captures.md)
