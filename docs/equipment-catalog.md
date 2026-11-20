# Equipment catalog and authority

An optional data supplement supplies 93 equipment definitions and five ordered store lists. The catalog exposes both localized descriptions and names, component order, tier, both price columns, modifier layer and field, active parameters, passive tuning, and each definition's store memberships. Unknown fields remain available when a definition is serialized. The catalog includes developer entries. Only a supported simulation item can enter an inventory.

## Store and price selection

The host resolves a store through the gameplay settings record's entity references. Standard five-player-per-team queues and five-player practice use their 5v5 settings. Ranked Halcyon Fold uses Standard HF. The archive does not establish which settings identifier a native ARAL reservation selects. Its purchase policy therefore retains the existing ARAL rules. The complete catalog also exposes the dedicated revamp Blitz and both tutorial stores. The original unqualified Blitz settings reference the standard 3v3 store.

For supported equipment the first price column is the combine charge. The full price adds the recursive cost of the component instances that a buyer still needs. Purchases consume each eligible held instance once. The loaded recipes must descend through at most three tiers. An unavailable component prevents purchasing an incomplete recipe. For the four archived ARAL sales, the second price column equals all four observed refunds.

| Sold item | Template | Observed refund |
| --- | ---: | ---: |
| Hourglass | 473 | 125 |
| Blazing Salvo | 463 | 350 |
| Stormcrown | 519 | 1,200 |
| Scout Trap | 495 | 25 |

The project simulation can sell an owned, equipped instance at base while the hero is alive. A sale removes its modifiers and inventory slot once and credits the configured refund. Four original sale notifications traveled from server to client. The capture contains no corresponding client sell opcode.

## Coverage of the 93 definitions

| Catalog group | Count | Server behavior |
| --- | ---: | --- |
| Ordinary store items with positive combine prices and entity templates | 76 | The host admits eligible purchases by template, consumes matching components by instance, applies supported modifiers and supports owned-instance resale. The optional catalog supplies all 76 refunds. |
| Definitions absent from every store | 5 | Healing Flask and Vision Totem have separate starter inventory roles. Ironguard Contract, Minion Candy and Nullwave Gauntlet have no regular shop admission. |
| Internal, development or unpriced special entries | 12 | Boombox, the three Cheater variants, Cooldown Damage Self, Get Rid Of Gold, Gold Candy, Immortal Stun Self, Level Candy, Refund Ability Point, Template and Video Helper have no purchase route. Template has a nominal price of 1 and a refund of 30,000. |

The additional 17 ordinary templates comprise Aegis, Capacitor Plate, Dragonblood Contract, Protector Contract, Rook’s Decree, Shiversteel, Tension Bow and Tornado Trigger in both standard stores and Blitz Revamp. Both infusions appear in the two standard stores. Flare and Scout Trap appear only in Standard 3v3. Flare Loader, Scout Pak, Scout Tuff, SuperScout 2000 and Teleport Boots appear only in Standard 5v5. Shiversteel also appears in Tutorial 2. The original ARAL transactions independently establish Capacitor Plate, both infusions, Flare and Scout Trap.

Purchase and resale cover the economy of consumables. Their deployment, infusion consumption and temporary scaling, Tension Bow's conditional proc, and equipment-specific activated abilities need separate simulation and native input evidence. The full catalog exposes the original tuning for these effects. The four captured ARAL sale broadcasts include Scout Trap's modifier-free removal and 25-gold refund.

## Boots

The supplement's nonstacking movement bonus uses the largest equipped boot bonus. Sprint Boots supply a further 2 movement speed for 2 seconds and have a 150-second cooldown in the original data. The simulator validates the actor and instance before activation. Resale and purchase of another pair preserve the cooldown. With Sprint Boots available, the local bot strategy can buy a pair and activate it during a long move. Other active and passive variables remain available through the catalog alongside their descriptions and units. Their presence alone does not define a native input or reproduce their combat effects.

The ARAL Healing Flask starter grants a 115-point barrier, 200 health and 30% maximum energy over five seconds, with a 120-second cooldown. The simulation validates the actor’s own native inventory slot. It distributes recovery across server ticks, caps both resources, and absorbs only damage that remains after other barriers. The archive records 62 activations and establishes neither the original tick cadence nor how overlapping barriers are ordered.

The independent optional test in the simulation crate loads both operator directories and checks catalog counts, mapped stores, Sprint Boots parameters, and the four captured sale prices. Set `HALCYON_DELIVERY` and `HALCYON_DATA_EXTRA`, then run `cargo test -p halcyon-sim original_supplement_supplies -- --ignored`.

For the observed Healing Flask and completed-sale messages, witness eligibility and replay scope, see [ARAL transaction witnesses](aral-item-activation.md#aral-transaction-witnesses).
