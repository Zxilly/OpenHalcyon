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

## Purchases visible to other seats

The seven room streams contain 342 server `0439` purchase records and 425
`043D` grants. Pairing each purchase to its subsequent same-actor,
same-template grant before the next purchase yields 341 transactions across
63 templates. One Weapon Blade record preceded an infusion purchase without
an intervening Weapon Blade grant. A grant alone does not establish a debit.
The table uses mode-0 stat-6 debits, identifies each removed `044B` instance
by its earlier grant, and compares the ordered equipment modifiers with the
delivered item definitions. The 50 witnessed catalog entries cover 256
transactions and 91 distinct debit/component profiles. For all 256, the
ordered negative modifiers in the supported stat categories matched the
bonuses of exactly the removed component instances.

The full price is the published combine fee plus the value of each required
component. Each held instance can apply to one branch. A completed branch
takes precedence over its leaves. Two branches can consume two separately
owned copies of the same leaf. The purchase checks capacity after removing
components. Failure preserves the balance, instances and modifiers. First
visibility and reconnection project the authoritative inventory.

| Template | Equipment | Full price | Matched receipts | Observed debits |
|---:|---|---:|---:|---|
| 458 | Weapon Blade | 300 | 11 | 300 |
| 459 | Crystal Bit | 300 | 19 | 300 |
| 460 | Swift Shooter | 300 | 10 | 300 |
| 461 | Six Sins | 650 | 8 | 350, 650 |
| 462 | Eclipse Prism | 650 | 10 | 350, 650 |
| 463 | Blazing Salvo | 700 | 12 | 400, 700 |
| 464 | Sorrowblade | 3100 | 5 | 1300, 2150, 2500, 2800, 3100 |
| 465 | Shatterglass | 3000 | 10 | 1300, 1650, 1950, 2400, 3000 |
| 467 | Oakheart | 300 | 3 | 300 |
| 468 | Dragonheart | 650 | 0 | component definition only |
| 469 | Light Armor | 300 | 4 | 300 |
| 470 | Coat Of Plates | 750 | 1 | 750 |
| 471 | Metal Jacket | 1900 | 1 | 1900 |
| 472 | Energy Battery | 300 | 8 | 300 |
| 473 | Hourglass | 250 | 3 | 250 |
| 474 | Void Battery | 700 | 3 | 400, 700 |
| 475 | Chronograph | 800 | 6 | 800 |
| 476 | Clockwork | 2400 | 2 | 1600, 2100 |
| 477 | Sprint Boots | 300 | 16 | 300 |
| 478 | Travel Boots | 650 | 7 | 350, 650 |
| 479 | Serpent Mask | 2800 | 5 | 850, 1350, 2000, 2800 |
| 485 | Reflex Block | 700 | 4 | 400, 700 |
| 486 | Frostburn | 2700 | 3 | 1350, 2700 |
| 487 | Fountain of Renewal | 2100 | 2 | 500, 700 |
| 488 | Crucible | 2000 | 1 | 1300 |
| 489 | Journey Boots | 1400 | 1 | 1100 |
| 490 | Halcyon Chargers | 1400 | 9 | 50, 800, 1100, 1400 |
| 492 | Aftershock | 2600 | 1 | 2600 |
| 497 | War Treads | 1600 | 3 | 950, 1300, 1600 |
| 498 | Atlas Pauldron | 1700 | 1 | 1700 |
| 499 | Book Of Eulogies | 300 | 6 | 300 |
| 500 | Barbed Needle | 800 | 10 | 500, 800 |
| 501 | Light Shield | 300 | 3 | 300 |
| 502 | Kinetic Shield | 750 | 3 | 750 |
| 504 | Lifespring | 800 | 3 | 500, 800 |
| 505 | Heavy Steel | 1150 | 13 | 850, 1150 |
| 507 | Breaking Point | 2700 | 2 | 1250, 2700 |
| 509 | Alternating Current | 2800 | 6 | 1750, 1800, 2800 |
| 511 | Eve Of Harvest | 2600 | 2 | 1900, 2300 |
| 512 | Heavy Prism | 1050 | 11 | 750, 1050 |
| 513 | Stormguard Banner | 600 | 5 | 600 |
| 516 | Contraption | 2100 | 1 | 1500 |
| 518 | Flaregun | 600 | 2 | 600 |
| 519 | Stormcrown | 2400 | 1 | 1000 |
| 520 | Poisoned Shiv | 2750 | 1 | 1950 |
| 522 | Spellfire | 2700 | 3 | 1000, 2100, 2700 |
| 523 | Dragon's Eye | 3000 | 2 | 2350, 3000 |
| 524 | Spellsword | 2800 | 1 | 2800 |
| 525 | Slumbering Husk | 2100 | 2 | 1050, 2100 |
| 538 | Warmail | 800 | 4 | 500, 800 |
| 539 | Seraphic Shell | 1900 | 6 | 1150, 1200, 1600 |

Heavy Prism is the intermediate branch of Alternating Current. All eight
observed Energy Battery purchases granted +1.5 energy regeneration as well
as +100 maximum energy. Dragonheart enters the table through its published
recipe and attributes. Its full value reconciles the final-item debits.

The simulator applies the equipment's supported HP, energy, regeneration,
weapon power, crystal power, armor, shield, attack speed and cooldown
modifiers. Items requiring timed infusions, consumable charges, critical
hits, penetration or additional unmodeled modifier types remain outside
this table. Passive procs, deployables and on-use effects need independently
established triggers.
