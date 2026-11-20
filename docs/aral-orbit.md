# ARAL bounce and orbit actions

The nine complete TCP recordings contain 25 server records each of `0x0402`, `0x0406` and `0x0407`. The orbit and cancel records all came from one ARAL match on the Lorelai player's stream. The role names have independent native action-class evidence. [Action-class study](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/vg-binary-event-candidates-2026-09-09-details.md)

| Orbiting actor (+0) | Center actor (+12) | Starts and matching cancels | Elapsed seconds in stream |
|---|---|---:|---:|
| Lyra_Bulwark (272) | Lyra (269) | 17 | 7.826–8.125 |
| Idris (273) | Alpha, Lyra or Lance | 4, 3 and 1 | 0.524–1.192 |

Every orbit start had zero words at +4 and +8. For all 17 Bulwarks, the X/Z words at +20/+28 matched the actor's most recent declaration or position exactly. The float bits at +16 were `0x3e3270a0` for every Bulwark and zero for every Idris. The typed orbit reader and encoder reproduce all 25 starts and 25 cancels byte for byte, including the two unknown start words, coordinate bits and six alignment bytes. All 25 cancels referenced the same actor as their start, each once. Only three final XYZ tuples at +4..+15 repeated the start XYZ. These timings are differences between recorded stream timestamps.

Lyra's authoritative simulation models Imperial Sigil and Bright Bulwark. The recorded orbit uses a separate Bulwark actor. Idris himself orbits a target here; his modeled Chakram has a separate projectile trajectory. Three of the eight Idris starts followed an Idris targeted release with selector 4 within 260 ms. The data do not supply a general start/cancel rule for a currently hosted spell.

For bounce, the first word names the affected actor. The 25 records affected 8 Reim, 4 Ylva, 3 Ringo, 3 ranged minions, 2 lead minions, 2 tank minions, 1 Inara, 1 Lance and 1 Amael. The float at +4 was 10 in 23 records and 6 in two. The third word varied, the fourth was always 1 and all six padding bytes were zero. In 24 of 25 cases `0x0403` for that same actor preceded the bounce within 122 ms and 60 records. In 15 cases an Amael-origin application of definition 62 to that actor also fell in the window. The displacement frame carries no source actor. A nearby release alone cannot assign every bounce to one skill. The bounce attribution remains open. The Mobile Bulwark orbit projection is described below.

## Actor-anchored zones

All 22 ARAL `0421` records name a fresh zone in the first word. The next two words refer to declared actors. Seventeen pairs are Lyra (269) and her Bulwark (272). The other five are Ardan (257) and his arena (377). The first three parameter words follow at offsets 12, 16 and 20. Interpreted as floats, the first is 5.5 for Bulwark and 7 for the arena, the second is −1 for both and the third varies. The flag is 1 and all five alignment bytes are zero. The codec preserves all parameter bits and returns the original suffix.

Each zone has exactly one subsequent `0423` naming its ID, with two zero alignment bytes. Bulwark lifetimes range from about 7.8 to 8.1 seconds. Four arenas persist for about six seconds and one for 2.4 seconds. The capture reader verifies all 44 records by rebuilding them from the typed representation. The host models the three-second combat field of Lyra B separately from the eight-second presentation actor. [Wire codec](../crates/protocol/src/zone.rs)


All 17 observed Lyra no-target releases with selector 2 preceded a distinct Bulwark declaration by 0.193–0.367 seconds. The opening order was always actor declaration (03F2), orbit (0406), zone (0421). All 17 endings sent 0407 followed by 0423 at the same timestamp. The initial orbit X/Z equaled Lyra's last 042E position in every case. Ten zone orientations at +20 agreed with her last distinct movement direction to within 0.0001 radians. The other seven differed by 0.029–1.552 radians. The final orbit coordinates and the zone orientation during those unobserved turns still lack a reproducible source. All these releases came from one remote witness. The recorded presentation lifetime does not determine the damage or slow duration.

All 17 Bulwarks used Y bits `0x3d192e1f` at orbit start and `0x3f86630b` at cancel. Comparing cancel X/Z against every `042E` position for the center Lyra found 12 matches within 0.00001 world units. The closest matching sample could be up to 807 ms from cancel. The five remaining cancels were 0.418 to 0.932 world units from the nearest recorded center position within 350 ms. Three fell along a nearby movement segment or its extension. No common rule reproduced all endpoints.

For the zone angle at `0421` +20, compare the shortest angular difference to `atan2(delta_z, delta_x)` from Lyra's consecutive distinct `042E` positions. The last preceding direction matched 10 of 17 within 0.0001 radians. The next observed direction within 500 ms matched none. Taking the direction from the last preceding position to a `03F8` waypoint for Lyra's registered slot within 500 ms on either side matched one of 17. The release-facing source remains unverified.

The separate named Mobile Bulwark talent explicitly causes the field to move with Lyra and adds 1.4 seconds at level one plus 0.4 per subsequent level. Level ten therefore yields exactly eight seconds including the ordinary three seconds. That agrees with the recordings. Lyra's sole 0481 equipped selector 1 at level ten. Together with the hero-specific movement and duration, this supports that one selector/level combination. The host enables the named talent when the home grants that level. Other selector/level combinations retain their numeric meaning. The untalented field stays at its cast location.

All 17 0415 records were observed by an opposing Lorelai, with Lyra as source, a no-target word of ffffffff, selector 2 and five zero framing bytes. The host sends that release to a visible opponent whose current scene knows Lyra, and to a live full observer after Lyra's declaration. The originating native command remains open. The level-ten Mobile Bulwark orbit and zone sequence is described below.

## Hosted Mobile Bulwark

For all 17 level-ten mobile fields, the facing vector in the child actor's
03F2 at +24/+32 gives the subsequent 0421 angle directly via atan2(Z,X).
The child's +112 owner is Lyra. Its skin word, fixed height, movement
profile and both initial effect definitions (66, 709) agree throughout.
All 17 matching ends send 0407, 0423 and 040B for the same anchor/zone at
the same timestamp. The host reserves a room-unique actor and zone, shares
the dynamic slot allocator with minions and derives the orientation from
its own caster movement. A known heading and authorized level-ten mobile
talent are required. Its five-tick presentation delay falls inside the
observed 0.193–0.367 seconds. The combat field retains its own timer.

Opening batches go to visible opposing witnesses with an existing Lyra
declaration and to authorized observers. A recipient that missed the
opening gets no terminal reference. A witness that loses sight receives
zone and actor cleanup without an endpoint revealing Lyra's position.
Normal expiration follows the caster's current authoritative position.
Early cleanup on caster death follows the hosted field lifetime.

## Amael's ordinary Super Kick

Two ARAL records distinguish the ordinary targeted kick against Reim and
Ylva. In both, an opponent saw Amael's 0415 with selector 2, then the victim's
0402 with values 6, 5 and enabled 1 after 0.10–0.11 seconds. Definition 62
revealed the victim for 1.5 seconds and the following damage used family
74, fields 1/0. The preceding caster 0403 destinations were each 1.8 units
short of the selected hero. Distance from Amael's correction to that
destination divided by the transmitted speed gives 0.30 seconds in both.
One victim also had a separate short horizontal displacement.

The hosted ordinary branch requires a living, visible enemy hero within
seven units and a clear route. The Bevy effect advances the caster for six
ticks and impacts once on tick two, interrupts the victim's pending action,
and reveals them for 30 ticks. Physical damage uses the named DAMAGE block
(300/400/500, 1.8 CP and 1.5 WP), the victim's armor and the caster's armor
pierce. The ranked cost is 80/95/110 and the base cooldown 80/65/50
seconds. The bounce operands supply the native presentation. They provide
no measured duration for a five-second status lock. An already rooted,
stunned or movement-immune victim is outside this branch. The elbow drop
and its area damage require a separate trigger.

The hosted controller uses the build's targeted cast request with selector
2 and ultimate learn index 2. Two remote perspectives include Amael's
043A operand 2 and the targeted release; neither perspective controlled
Amael. Tests cover the originating hosted request, registered target and
release confirmation. Captured owner-side request or reply timing remains
unverified.
