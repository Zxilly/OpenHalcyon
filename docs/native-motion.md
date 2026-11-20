# Native movement actions

The Android 147219 reader sizes for `03F9`, `03FA` and `03FB` are 18, 24 and 21 bytes including the four-byte action header. The Windows action readers independently consume 14, 20 and 17 body bytes and convert three big-endian floats in each case. The Rust decoder accepts both the original body and its padded transport length. The typed writer preserves the flag, float bits and any trailing bytes. [Android sizes](ce-game.tsv) · [Independent action readers](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/vg-binary-event-candidates-2026-09-09-details.md) · [Typed writer](../crates/protocol/src/native_motion.rs)

| Action | Fields at body offsets | Native / padded bytes | Live / Solo records |
|---|---|---:|---:|
| `03F9` ActionMoveToServerAuthoritative | Registration slot +0, float triple +1, raw flag +13 | 14 / 14 | 2,548 / 24 |
| `03FA` ActionMoveToAndFace | Actor +0, referenced actor +4, float triple +8 | 20 / 22 | 1,978 / 48 |
| `03FB` ActionStopActor | Actor +0, float triple +4, raw flag +16 | 17 / 22 | 1,829 / 34 |

At each record in the seven played ARAL captures and in the Android Solo VGR, the `03F9` slot resolved to an earlier declaration. Both `03FA` actor IDs and every `03FB` actor ID also resolved. All 1,978 ARAL move-and-face records reference an actor on the opposing team. Of the ARAL stops, 1,206 repeated the exact three float words of the same actor's most recent move-and-face action. The middle float in every Solo record was approximately 0.06. ARAL stop records used values ranging from 0.04 to 3.81. These observations support a position triple and leave the rule for choosing the middle value unresolved.

The `03F9` final byte was 1 and the `03FB` final byte was 0 throughout both corpora. Their meanings and the server's selection among these actions remain unverified. The live corpus contains these opcodes only in the server direction. [Capture inventory](live-captures.md)

In the Android Solo recording, all 24 `03F9` records resolve to seven bot
heroes. Six share a timestamp with a `03F8` for the same slot. In all six,
the horizontal position differs from the ordered destination. None of the
24 shares a timestamp with a same-actor `0415`, `0416`, `03FA`, `03FB`,
`0430` or `0431`. Idris accounts for eight records. Six arrived within
0.470 seconds, during which X moved from 13.4 to 11.7. His preceding
targeted plays used selectors 14 and 15 and occurred 0.819–1.289 seconds
before the position updates.

The complete ARAL records contain 1,450 opponent, 1,090 ally and eight
own-controller views of this action. Resolve each slot through the preceding
spawn and pair it with the first following same-actor `042E`. Within 51 ms,
2,476 pairs exist; the two X/Z values always differ, with a median separation
of 2.56 world units. Only 243 actions repeat the preceding `03F8` destination.
This distinguishes a forced goal from the actor's current coordinate. The
original conditions and cadence for choosing a forced goal remain open.

OpenHalcyon uses `03F9` for a narrow Solo bot navigation policy. After the
confirmed local control connection receives a bot move order, a different
visible waypoint for the same route, or a new route after pursuit ends, can
replace that order with a forced goal. The bot must take a real step on the
new segment. A recipient needs declarations for the bot and any pursued target.
Hidden actors, hidden waypoints, expired prior orders and reconnects start a
fresh route. The host uses the Solo flat height observed in all 24 archived
records and sends the goal before the same tick's independent position update.
Ordinary orders use `03F8`; this policy is specific to the hosted Solo bot
scene.

## Solo stop timing

All 34 Solo stop records are immediately followed by an ability play for the same actor at the same replay timestamp. There are 33 entity-targeted plays (`0415`) and one point play (`0416`). Seven bot heroes account for every stop. The human actor 1500 has no stops in this recording.

Resolving the most recent `03F8` destination through its registration slot gives a horizontal separation of 0.330 to 30.197 world units from the recorded stop. None of the 34 stop positions equals that destination. The replay has zero `042E` corrections against which to check an intervening position. All 34 stop records carry middle float bits `0x3D75BE29` (0.059995803982019424) and final byte zero.

The sequence identifies an interruption of movement associated with some ability plays. For example, four Idris stops precede entity-targeted plays with selectors 14 and 15. Two of 21 plays with selector 14 and two of 17 with selector 15 have a preceding stop. The implemented Idris B uses a point play with selector 3. The sole point play after a stop in this capture belongs to template 413, selector 0. The recording provides no positive example of a stop caused by completion of a move order or by a currently mapped Solo hero ability. A native output trigger needs a synchronized example of the relevant simulated action and its movement state.

## Lane-minion destinations

All 22,468 non-hero `03F8` records in the seven played ARAL captures resolve by registration slot to lane-minion templates 287, 288 and 289 (5,552, 5,971 and 10,945 records). Every one of the 784 distinct minions received its first destination within 250 ms of its first declaration; 779 shared its capture timestamp. No first destination equaled the spawn position. There were 1,086 distinct horizontal destinations and 7,968 repeat orders to the same minion's preceding destination.

ARAL has 29,434 lane-minion `042E` position corrections. Among 20,061 orders with a subsequent correction before the next order, 2,584 first corrections equaled the destination. Thus an order and a correction carry different positions for much of a route. The host sends both the actual simulation waypoint and its separate authoritative motion updates.

Solo has 4,102 lane orders for templates 335–338, respectively 1,259, 1,870, 786 and 187. All 130 minions had an order within 250 ms of first declaration, including 129 at the same timestamp. No first order equaled the spawn position. This replay had 327 distinct destinations, 1,180 repeats and zero `042E` records.

The host records the waypoint actually used by a moving lane minion each simulation tick. Each native scene receives that waypoint using its own registered minion slot. A pursuit requires that the pursued actor is also registered and visible to the recipient. Hidden or newly registered actors and recycled slots have independent state. For continuous visibility, the host suppresses unchanged destinations and limits changes per minion to five per second. This cadence is a host policy. Authorized observer streams resolve slots independently. [Simulation movement](../crates/sim/src/lib.rs) · [Per-recipient projection](../crates/game/src/tower_aggro.rs)

## ARAL Chakram movement interruption

Of 38 Idris B releases in two played ARAL records, 24 were immediately
preceded by `03FB` for the same actor at the same timestamp. All 24 were
viewed from a teammate's connection. The other ten teammate-view releases
and all four caster-view releases had no preceding stop. All 24 carried
flag zero and height bits `0x3D192E1F`. Their X and Z words matched the
first following same-actor `042E` correction. Each had a same-actor
`03F8` movement order 0.2–0.8 seconds earlier. For the 14 releases
without a stop, the previous correction and the release correction had
identical X/Z. Lorelai's six A and five B releases had no such stops.

The server emits `03FB`, `0416` and `042E` in that order to registered
teammates with sight of Idris when an authenticated ARAL Chakram both
interrupts actual walking and successfully releases. The host checks
for a real movement step on the latest completed simulation tick and
uses the actor's position at release. This authoring is limited to the
recorded stop profile's horizontal extent, X −58.21 to 23.38 and Z −1.83
to 9.47. The fixed height comes from that captured action, and the
simulation models no vertical variation for this profile. Stationary
releases, interrupted casts, replaced issuing connections, and other
views or abilities do not produce the new stop.

In all 1,978 live `03FA` actions the next same-actor correction had the
same X/Z at the same capture timestamp. A matching entity-targeted release
followed within 0.5 seconds in 1,832 cases. This establishes the
move-and-face position and reference. The Ringo comparison below identifies
one hosted release transition. None of the 525 `0403` actions matched
the next correction's X/Z when treated as the `03FB` position layout.
The independent reader converts five words and a byte. The host retains
this action as read-only pending operand and displacement semantics.

## Ringo empowered attacks

Two ARAL streams contain 63 target releases from Ringo with selector 3.
Compare the target and the last reported X/Z with Ringo's preceding targeted
release. All 50 target changes and all 10 same-target releases from a changed
position had a preceding `03FA` for that victim at the release timestamp.
The next same-actor `042E` repeated its X/Z. Of the three remaining releases
from an unchanged position against the same target, only one had a face action.

The host emits `03FA`, an X/Z correction and the empowered release to
scene-ready recipients that saw the perk and have both actors registered.
It compares consecutive basic releases and drops the comparison on a cast or
death. Fifty-nine of the 60 matching faces use height bits `3d192e1f` in
the lane region X −69.44 to 52.15 and Z −2.13 to 14.30. The remaining one,
near (−73.25, 21.23), has a different elevation. The host projects the flat
profile inside the measured region. It omits the elevated and ambiguous
stationary cases.

### ARAL Karas Twister

All 16 Karas 0403 records in the seven played streams follow a same-actor
0416 point release with selector 1. The witness was an opponent. Offsets
+4/+8/+12/+16 are **speed, destination X, height, destination Z** for this
profile. Height is always the float with bits 3d192e1f and the final byte
is zero. Distance from the preceding caster correction to this destination
divided by speed was 0.31978–0.32000 seconds. Thirteen paths were seven
units long; the other three were approximately 5.807, 4 and 1. The supplied
Twister rules give seven units and 0.32 seconds. The host uses its own
legal destination and scales speed to actual distance. On its first legal
dash step it sends the motion to opponents with the caster registered and
visible, and to authorized observers with the same declaration. An
interrupted dash that never moves has no announcement.
