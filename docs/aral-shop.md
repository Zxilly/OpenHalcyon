# ARAL shop receipts

The seven played ARAL recordings contain 18 client shop requests for 12
templates. The six initial full hero declarations in each game all carry
2,000 gold in the spendable and cumulative resource slots at +286 and +290.
One seat spent all 2,000 before receiving its first income. The native reader
uses these big-endian floats when the resource mask at +326 is zero.
Positive gold awards increase both balances. A debit affects spendable gold.
[Native gold dataflow](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/NATIVE_GOLD_TIME_2026-10-03.md)

| Template | Item | Observed debit | Granted 0x041C modifiers (stat, value) |
| ---: | --- | ---: | --- |
| 458 | Weapon Blade | 300 | (4, 10) |
| 459 | Crystal Bit | 300 | (5, 15) |
| 460 | Swift Shooter | 300 | (15, 0.1) |
| 461 | Six Sins | 650 | (4, 25) |
| 463 | Blazing Salvo | 400 with Swift Shooter | (15, 0.2) |
| 473 | Hourglass | 250 | (25, 0.075), (3, 0.25) |
| 475 | Chronograph | 800 | (25, 0.15), (3, 0.75) |
| 477 | Sprint Boots | 300 | none observed |
| 478 | Travel Boots | 350 with Sprint Boots | (0, 100) |
| 505 | Heavy Steel | 1,150 directly, 850 with Weapon Blade | (4, 45) |
| 509 | Alternating Current | 1,800 with Blazing Salvo and Crystal Bit | (5, 45), (15, 0.4) |
| 513 | Stormguard Banner | 600 | (0, 150) |

For a successful player purchase, the server echoes 0x0439, debits stat 6
with 0x041D, removes each consumed instance, grants the new instance with
0x043D, then applies its modifiers. Each component's negative modifiers
precede its own 0x044B removal. The removal carries the instance ID returned
by the original grant and a big-endian u16 flag of 1. The 0x041C records
use source 0xffffffff, a big-endian float, one stat byte and flag 1.
Other nearby health and energy changes remain independent combat events.

The two-component receipt reversed Blazing Salvo's 0.2 attack-speed
modifier, removed its instance, reversed Crystal Bit's 15 crystal power,
removed its instance, then granted Alternating Current. A subsequent boots
upgrade removed the owned Sprint Boots instance and granted Travel Boots.
The resulting inventory and native gold balances survive a fresh scene on
the same room. The currency baseline and current equipment replace local
state on first visibility. Subsequent gold and item changes apply once.

All seven controlled seats received a repeating 15-gold award at roughly
one-second intervals. Their first awards arrived 30.13–30.98 seconds after
the recorded start action. There were 1,653 same-value hero awards between
recorded death and respawn actions. Visibility and logging limit what one
client records of other seats. The server uses simulation seconds for this
income, including while a hero awaits respawn.

The encrypted six-seat regression buys all 12 templates at an actual
starting pad, checks the actor and affordability gates, earns the late
upgrade through simulation steps, and verifies the reconnecting player
receives the two remaining instances with restored balances and modifiers.
This uses an automated game client. Native shop UI presentation and item
passive effects beyond these receipts are outside this verification.
[Encrypted-room regression](../crates/game/src/aral_shop_test.rs)
