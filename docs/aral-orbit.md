# ARAL bounce and orbit actions

The nine complete TCP recordings contain 25 server records each of `0x0402`, `0x0406` and `0x0407`. The orbit and cancel records all came from one ARAL match on the Lorelai player's stream. The role names have independent native action-class evidence. [Action-class study](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/vg-binary-event-candidates-2026-09-09-details.md)

| Orbiting actor (+0) | Center actor (+12) | Starts and matching cancels | Elapsed seconds in stream |
|---|---|---:|---:|
| Lyra_Bulwark (272) | Lyra (269) | 17 | 7.826–8.125 |
| Idris (273) | Alpha, Lyra or Lance | 4, 3 and 1 | 0.524–1.192 |

Every orbit start had zero words at +4 and +8. For all 17 Bulwarks, the X/Z words at +20/+28 matched the actor's most recent declaration or position exactly. The float bits at +16 were `0x3e3270a0` for every Bulwark and zero for every Idris. All 25 cancels referenced the same actor as their start, each once. Only three final XYZ tuples at +4..+15 repeated the start XYZ. These timings are differences between recorded stream timestamps.

Lyra's authoritative simulation models Imperial Sigil and Bright Bulwark. The recorded orbit uses a separate Bulwark actor. Idris himself orbits a target here; his modeled Chakram has a separate projectile trajectory. Three of the eight Idris starts followed an Idris targeted release with selector 4 within 260 ms. The data do not supply a general start/cancel rule for a currently hosted spell.

For bounce, the first word names the affected actor. The 25 records affected 8 Reim, 4 Ylva, 3 Ringo, 3 ranged minions, 2 lead minions, 2 tank minions, 1 Inara, 1 Lance and 1 Amael. The float at +4 was 10 in 23 records and 6 in two. The third word varied, the fourth was always 1 and all six padding bytes were zero. In 24 of 25 cases `0x0403` for that same actor preceded the bounce within 122 ms and 60 records. In 15 cases an Amael-origin application of definition 62 to that actor also fell in the window. The displacement frame carries no source actor. A nearby release alone cannot assign every bounce to one skill. The host preserves these three actions as read-only.

## Actor-anchored zones

All 22 ARAL `0421` records name a fresh zone in the first word. The next two words refer to declared actors. Seventeen pairs are Lyra (269) and her Bulwark (272). The other five are Ardan (257) and his arena (377). The first three parameter words follow at offsets 12, 16 and 20. Interpreted as floats, the first is 5.5 for Bulwark and 7 for the arena, the second is −1 for both and the third varies. The flag is 1 and all five alignment bytes are zero. The codec preserves all parameter bits and returns the original suffix.

Each zone has exactly one subsequent `0423` naming its ID, with two zero alignment bytes. Bulwark lifetimes range from about 7.8 to 8.1 seconds. Four arenas persist for about six seconds and one for 2.4 seconds. The capture reader verifies all 44 records by rebuilding them from the typed representation. The host models the three-second combat field of Lyra B separately from the eight-second presentation actor. [Wire codec](../crates/protocol/src/zone.rs)


All 17 observed Lyra no-target releases with selector 2 preceded a distinct Bulwark declaration by 0.193–0.367 seconds. The opening order was always actor declaration (03F2), orbit (0406), zone (0421). All 17 endings sent 0407 followed by 0423 at the same timestamp. The initial orbit X/Z equaled Lyra's last 042E position in every case. Ten zone orientations at +20 agreed with her last distinct movement direction to within 0.0001 radians. The other seven differed by 0.029–1.552 radians. The final orbit coordinates and the zone orientation during those unobserved turns still lack a reproducible source. All these releases came from one remote witness. The recorded presentation lifetime does not determine the damage or slow duration.

The separate named Mobile Bulwark talent explicitly causes the field to move with Lyra and adds 1.4 seconds at level one plus 0.4 per subsequent level. Level ten therefore yields exactly eight seconds including the ordinary three seconds. That agrees with the recordings. Lyra's sole 0481 equipped selector 1 at level ten. Together with the hero-specific movement and duration, this supports that one selector/level combination. The host enables the named talent when the home grants that level. Other selector/level combinations retain their numeric meaning. The untalented field stays at its cast location.

All 17 0415 records were observed by an opposing Lorelai, with Lyra as source, a no-target word of ffffffff, selector 2 and five zero framing bytes. The host sends that release to a visible opponent whose current scene knows Lyra, and to a live full observer after Lyra's declaration. The actor orbit, zone sequence, and an originating native command remain unimplemented.
