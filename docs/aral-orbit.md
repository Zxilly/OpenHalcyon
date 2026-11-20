# ARAL bounce and orbit actions

The nine complete TCP recordings contain 25 server records each of `0x0402`, `0x0406` and `0x0407`. The orbit and cancel records all came from one ARAL match on the Lorelai player's stream. The role names have independent native action-class evidence. [Action-class study](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/vg-binary-event-candidates-2026-09-09-details.md)

| Orbiting actor (+0) | Center actor (+12) | Starts and matching cancels | Elapsed seconds in stream |
|---|---|---:|---:|
| Lyra_Bulwark (272) | Lyra (269) | 17 | 7.826–8.125 |
| Idris (273) | Alpha, Lyra or Lance | 4, 3 and 1 | 0.524–1.192 |

Every orbit start had zero words at +4 and +8. For all 17 Bulwarks, the X/Z words at +20/+28 matched the actor's most recent declaration or position exactly. The float bits at +16 were `0x3e3270a0` for every Bulwark and zero for every Idris. All 25 cancels referenced the same actor as their start, each once. Only three final XYZ tuples at +4..+15 repeated the start XYZ. These timings are differences between recorded stream timestamps.

Lyra's authoritative simulation models the Sigil ability. The orbit uses the separate Bulwark actor. Idris himself orbits a target here; his modeled Chakram has a separate projectile trajectory. Three of the eight Idris starts followed an Idris targeted release with selector 4 within 260 ms. The data do not supply a general start/cancel rule for a currently hosted spell.

For bounce, the first word names the affected actor. The 25 records affected 8 Reim, 4 Ylva, 3 Ringo, 3 ranged minions, 2 lead minions, 2 tank minions, 1 Inara, 1 Lance and 1 Amael. The float at +4 was 10 in 23 records and 6 in two. The third word varied, the fourth was always 1 and all six padding bytes were zero. In 24 of 25 cases `0x0403` for that same actor preceded the bounce within 122 ms and 60 records. In 15 cases an Amael-origin application of definition 62 to that actor also fell in the window. The displacement frame carries no source actor. A nearby release alone cannot assign every bounce to one skill. The host preserves these three actions as read-only.
