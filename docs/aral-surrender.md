# ARAL surrender

Seven full historical ARAL TCP recordings included six games with 22
server boards and one client request. The controlled client sent state 1 in
0x0472. The stock Android outbound writer sends one byte. An independent
action-class analysis identifies this message as ActionSetSurrenderStateRequest
and 0x0473 as ActionTeamSurrenderStateChanged. [Action-class analysis](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/vg-binary-event-candidates-2026-09-09-details.md)

All 22 boards had 158 body bytes after transport decryption. The Android
receiver converts the leading word and two sixteen-word arrays. The three
slots declared by these ARAL boards occupy the beginning of each array.

| Body offset | ARAL field |
| --- | --- |
| 0 | Poll deadline, big-endian f32 in game-clock seconds |
| 4 + 4i | Actor wire ID, big-endian u32 |
| 68 + i | Vote state byte |
| 84 + 4i | Per-seat cooldown time, big-endian f32 |
| 148 | Polling team's wire number, 1 or 2 |
| 149 | Board phase, 0 through 3 observed |
| 150 | Number of slots, always 3 |

Occupied positions contained distinct actors from the polling team. All 24
unoccupied positions among the 66 declared slots carried actor 0xffffffff,
vote 0 and cooldown 0. The encoder writes zeros outside the declared fields.
The decoder preserves the complete input for lossless forwarding, including
uninterpreted bytes. Observed phase counts were 14 idle (0), four open (1),
two passed (2) and two failed (3). The capture establishes no client encoding
for a negative vote and no universal voting threshold across modes.

Both phase-2 boards preceded a surrender result by about 15 seconds on the
recording clock. In both games the immediate predecessor of 0x03F1 was the
0x048D statistics report. Across each entire game there was no 0x0452 crystal
death marker. The first poll came from team 2 and produced wire winner 1.
The other came from team 1 and produced wire winner 2. The six terminal body
bytes were a big-endian winner word, reason 2 and a zero trailing byte. Each
of the other five games had one crystal marker, an immediate 0x048D report
before 0x03F1, reason 0 and a zero trailing byte. These counts describe the
seven recordings. They do not establish an exact presentation delay for
other builds or modes. [Protocol codec and synthetic fixtures](../crates/protocol/src/surrender.rs)

## Hosted ARAL rules

A registered player can start a ballot on the existing game connection. The
room broadcasts the board to the player's three-person team and restores it
on that team's reconnect. It interprets 1 as yes and supports 2 as a local
no vote. Each seat may respond once. The host requires three affirmative
votes and keeps disconnected seats in the electorate. These are local policy
choices for cases the captured client requests do not cover.

An open vote has a 30-second deadline. A rejection or timeout closes the vote
and displays the failed phase for five seconds. A team can start again after
120 simulation seconds from opening. On passage, both crystals retain their
health, combat stops, and the host settles the actual scoreboard and opponent's
victory after a 15-second presentation interval. The ordinary settlement
callback reports completion and includes the zero-based surrendered team.
