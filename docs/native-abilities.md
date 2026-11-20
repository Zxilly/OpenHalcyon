# Native ability requests, grants and releases

## Native learning

In 24 captured 0x0436 requests, the first nearby 0x043A from the same
actor followed after 22–124 ms, with a median of 82 ms. The table gives
the grant operand and the number of observations for each request index.

| Hero template | Index 0 (A) | Index 1 (B) | Index 2 (C) |
| --- | --- | --- | --- |
| 254 Hero009 | 0 (3) | 1 (2) | No sample |
| 439 Yates | 0 (2) | 2 (2) | 3 (1) |
| 273 Idris | 0 (2) | 3 (4) | 4 (1) |
| 409 Lorelai | 0 (2) | 1 (4) | 2 (1) |

A valid request on a registered native seat spends one skill point and
increments the requested rank before the server emits the original input echo
and the 0x043A grant. Normal level and rank limits apply. The first successful
request per simulation tick is the only one that spends a point. A rejected
request consumes no point and a later eligible request can succeed.

Learning can persist a rank even where the active spell is unimplemented.
The native cast handlers still cover Idris B and Lorelai A/B/C. Bots spend
points only on abilities whose combat behavior the simulator implements.
The recorded timing describes the captures, without a server-side delay
requirement.

## Idris B

The Android catalog identifies Idris as template 273. In the played ARAL
archive for that seat, four 0x0436 requests with index 1 each received a
0x0436 echo and 0x043A grant with actor ID and operand 3. The stock room
can select Idris when its configured game data includes that name and
template. The default validation roster uses Ringo.

Four 0x0412 requests from the same seat used selector 3. Every request
contained a three-float vector and one zero padding byte. The full vector
and selector reappeared in a 0x0416 response from the same actor after
39–99 ms. The signed-zero middle component is preserved.

| Vector X, Y, Z | Nearest reported actor X, Z | Approximate planar distance |
| --- | --- | ---: |
| (-40.8939, 0, 1.4944) | (-38.417, 0.785) | 2.58 |
| (-58.2402, -0, 14.9417) | (-65.077, 12.327) | 7.32 |
| (-44.1787, -0, 3.5794) | (-46.192, 8.335) | 5.16 |
| (-34.1510, 0, -0.7185) | (-36.317, 5.817) | 6.88 |

The positions cluster around the caster in world space. The vectors
themselves have lengths 34–60 and do not represent unit directions.
OpenHalcyon uses X/Z as the authored simulation point for Idris B. The
server models the measured ARAL lane within explicit bounds. Tests use
points inside those bounds and keep the historical vectors as codec fixtures.

Learning index 1 on an authenticated, registered native control seat
spends an available skill point before confirming. Repeated learning in
the same simulation tick receives no additional grant. Point casting uses
the simulator's target, range, resource and cooldown checks. Only an
authoritative release emits 0x0416 to the originating connection. All 15 observed accepted point-cast inputs first received a byte-identical 0x0412 on their originating connection. The room confirms that input at acceptance, including when a later interruption cancels the windup.
Canceled windups and rejected inputs do not emit it. The capture does not establish the exact native stamina, collision or
damage rules.

## Lorelai

Template 409 used learning indices 0, 1 and 2. In seven successful grants the
0x043A operand equaled the requested index (two A, four B, one C). Six A and
five B point requests had matching 0x0416 releases after 35–142 ms. Their
distances from the latest reported caster position ranged from 5.27 to 8.00
and 4.71 to 10.00 units respectively. The C request named another hero on
the caster's team. A 0x0415 carried caster, identical target and selector 2
55 ms later. The native input handler supports that allied-hero form.

## Witnessed grants and releases

Across seven complete ARAL recordings, the actor in the client's 0x0471
identifies its controlled seat. Initial full hero declarations supply the
other seats and their teams. Counts below include repeated actions.

| Server message | Controlled hero | Other allied hero | Opposing hero | Outside six hero IDs |
| --- | ---: | ---: | ---: | ---: |
| 0x0415 targeted release | 21 | 1,607 | 2,135 | 5,343 |
| 0x0416 point release | 15 | 215 | 654 | 2 |
| 0x043A learn grant | 24 | 104 | 189 | 0 |

An allied Idris accounts for 34 witnessed selector-3 point releases and five
operand-3 grants. Lorelai appears only on the controlled seat. The room
applies the shared release format to mapped skills from successful human and
bot simulation events. A team's current world view must contain the caster
and, for a targeted release, the target. A live authorized observer receives
the full view. The requesting control generation receives one confirmation
with the original point bytes. A replacement socket for that seat receives
no confirmation of the former connection's input. Skill-point expenditure
produces the public grant. Only the requesting seat receives its input echo.

## Ability charges

The independent H5810 replay audit observed 109 `0x0429` actions with the same
owner, ability slot 0, unsigned operand 4 and mode 1. The protocol crate reads
and writes its 14-byte recorded body and retains the trailing byte when decoding
a recording. The slot is an ability index. The neighboring `0x0428` timer action
uses a separate lookup key and a floating-point operand. The local Solo and ARAL
corpora contain no `0x0429` samples. The hosted abilities currently have no
verified charge-change trigger. [Replay field evidence](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/COOLDOWN_EVENT_FIELDS_2026-10-14.md)

## Lorelai pool registration and lifetime

Across seven ARAL streams, all six observed Lorelai A point releases created a
compact `0x03F2` actor of template 410 at the exact release X/Z. The owner at
body offset 112 matched the caster. The six slots at offset 116 were 34, 29,
29, 34, 25 and 33. No other compact or full declaration reused any of those
slots during that pool's lifetime. Each pool received two `0x043E` applications
from the caster. Definition 949 arrived at release with duration bits
`0x3a66` (about 0.8 seconds). Definition 950 arrived 0.70–0.80 seconds later
with duration bits `0x4800` (8 seconds). Each pool received `0x0430` with
source `0xffffffff` at 9.02–9.28 seconds after release, followed by both
`0x0431` and `0x040B` at 12.82–13.05 seconds. The recordings show actor
destruction without a corresponding explicit buff removal.

All six belonged to one caster, observed from that player's connection in one
match. Each declaration had eight per-view `0x042B` records with different
mask operands. The capture establishes the caster's slots and timestamps.
The other viewers' streams, registration on first sight, and the binding of
the two opaque birth words at offsets 88 and 92 remain unverified. The hosted
simulation applies Fish Food locally. Native registration and buff emission
remain pending evidence for those fields and recipients.

Five observed Lorelai B releases each preceded a template 411 at the cast X/Z
by 0.9–1.1 seconds. Six template 411 actors appeared in total, including one
without a matching B request. All six died 8.38–8.47 seconds after birth.
The single observed C release targeted another living ally. In the caster's
stream the targeted `0x0415` preceded `0x043E` definition 954 in the same
timestamp group. The application repeated the release's source and target,
carried a distinct instance ID, duration bits `0x4400` (four seconds), and six
zero padding bytes. The next record increased target stat 4 by 500.
No target impact or death occurred during the following 4.1 seconds.

For ARAL the host emits this application on the authenticated caster's
issuing connection after the successful release, using a fresh world
instance ID. The simulation reports it when a base four-second veil takes
effect on another ally with no remaining barrier. The archive supplies no
comparison for refresh, empowered veil, early shield depletion, self-cast or
the target's own view. No explicit duration update, cancel or timeout in the
seven matches referenced any instance of definition 954.

Across all nine TCP captures the lossless codecs reproduced 44,346 short and
17,018 extended applications, 385 duration updates, 1,398 explicit cancels
and 18 timeouts. Matching by target and instance also yielded zero mutations
for definitions 820 and 949 through 953. Across two matches, all 38 witnessed
Idris B releases coincided with definition 820 on Idris. It also appeared
25 times outside those releases, leaving its general trigger unresolved.

## Cooldown keys and initial state

The keys in the original ARAL records equal 32-bit FNV-1a over the case-sensitive ASCII definition names in the Android string table. The hosted opening sends the following records immediately after the corresponding hero declaration. Both big-endian floats are positive zero. Lorelai's original C/B/A relative order is retained.

| Definition name | Key in hex | Six configuration bytes in hex |
|---|---|---|
| Ability__Lorelai__C | 72761226 | 00 00 01 01 01 00 |
| Ability__Lorelai__B | 737613b9 | 00 00 01 00 01 00 |
| Ability__Lorelai__A | 70760f00 | 00 00 01 00 01 00 |
| Ability__Idris__B | 5cb5e744 | 00 00 01 00 01 00 |

The seven ARAL matches contained 322 syncs. Two Lorelai openings and two Idris openings repeat these profiles. Idris was observed on his own stream and another player's stream. Android Solo independently repeated Idris B's zero-valued profile in all fourteen replay sections without an intervening B release. The host emits these rows only on the original scene opening, with Solo limited to the Idris B profile. No further actor reference is disclosed.

The same seven matches contained 2,000 modify-cooldown records. Across six Lorelai A, five B and one C releases, the caster received no such records for her actor throughout either match. No Idris B key occurred in the 44 changes on his own actor or the 127 on the remote Idris. The only changes coinciding with any of the 38 B releases within 120 ms were two relative zero operands on Idris **A and C**. No same-owner sync occurred within 100 ms of any of the 50 releases. The static receiver selects a relative operation for mode zero and a direct operation for nonzero mode. The charge opcode uses an unsigned slot index and operand. None of these observations supplies a B/ABC start or expiry packet, a late-join nonzero countdown formula, or a teammate cooldown subscription.

The simulation still writes an independent tick deadline on successful release. It charges the effective cooldown after equipped reduction and rejects a new cast until that tick. Interrupted or rejected releases leave the deadline unchanged. [Independent cooldown and charge receiver study](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/COOLDOWN_EVENT_FIELDS_2026-10-14.md)
