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

Purchase and resale cover the economy of consumables. Scout Trap and starter Vision Totem have local ARAL deployment commands. Most equipment-specific activated abilities remain catalog-only. The full catalog exposes the original tuning for these effects. The four captured ARAL sale broadcasts include Scout Trap's modifier-free removal and 25-gold refund.

## Infusions

The standard 5v5 catalog contains two infusions. Both cost 500 gold. Buying one adds a held instance. The simulation command `ConsumeInfusion` consumes that instance once and starts a 150-second effect. A new infusion replaces the previous effect, including a fresh duration for the same type. Armor and shield each range from 10 to 20. Weapon Infusion adds 20–40 weapon power and 15–35% attack speed. Crystal Infusion adds 30–60 crystal power and 7.5–15% cooldown speed. The host uses the current level on a linear level 1–12 scale and removes only the active infusion's contribution at expiry.

The two languages both describe consumption and a single active infusion. Of 24 archived grants, nine had immediate mode-0 activation, six had delayed mode-2 activation, and nine had no observed activation. A precise automatic consumption rule and a client-originated activation opcode remain unverified. Players can consume an owned instance through the home-authenticated item action endpoint. The same endpoint supports selling and ARAL vision devices. [Player API](player-api.yaml) Attack-speed endpoints follow the 4.13 catalog; the historical weapon receipts also show a 20% base on some activations.

## Per-item effect coverage

The middle column lists the static catalog attributes. The right column describes additional implemented behavior. “Catalog only” retains the named tuning and description for lookup. Boot movement and named passives use the supplemental catalog where configured.

| Item | Catalog attributes | Separate active or passive behavior |
| --- | --- | --- |
| Aegis | HEALTH, ARMOR, SHIELD | No separate handler |
| Aftershock | CRYSTAL_POWER, ENERGY_REGEN, COOLDOWN_SPEED | Catalog only |
| Alternating Current | CRYSTAL_POWER, ATTACK_SPEED_DISPLAY | Every second landed basic attack adds 50% crystal power as crystal damage |
| Atlas Pauldron | ARMOR | Catalog only |
| Barbed Needle | WEAPON_POWER | 10% Vampirism and the Book of Eulogies basic-hit kill heal |
| Blazing Salvo | ATTACK_SPEED_DISPLAY | No separate handler |
| Bonesaw | WEAPON_POWER, ATTACK_SPEED_DISPLAY | Armor pierce; stacking attack passive is catalog-only |
| Book of Eulogies | WEAPON_POWER | 5% Vampirism on basic weapon HP loss; basic-hit minion/monster kills restore 10 HP for ranged heroes or 30 for melee |
| Breaking Point | WEAPON_POWER, ATTACK_SPEED_DISPLAY | Catalog only |
| Broken Myth | CRYSTAL_POWER | Shield pierce |
| Capacitor Plate | HEALTH, ARMOR, SHIELD, COOLDOWN_SPEED | Catalog only |
| Celestial Shroud | SHIELD, HEALTH | Catalog only |
| Chronograph | COOLDOWN_SPEED, ENERGY_REGEN | No separate handler |
| Clockwork | CRYSTAL_POWER, ENERGY, ENERGY_REGEN, COOLDOWN_SPEED | Catalog only |
| Coat of Plates | ARMOR | No separate handler |
| Contraption | HEALTH, ENERGY_REGEN, COOLDOWN_SPEED | Catalog only |
| Crucible | HEALTH | Catalog only |
| Crystal Bit | CRYSTAL_POWER | No separate handler |
| Crystal Infusion | — | Owned-instance consumption; timed crystal power, cooldown, armor and shield |
| Dragon's Eye | CRYSTAL_POWER | Catalog only |
| Dragonblood Contract | HEALTH | Catalog only |
| Dragonheart | HEALTH | No separate handler |
| Eclipse Prism | CRYSTAL_POWER | No separate handler |
| Energy Battery | ENERGY, ENERGY_REGEN | No separate handler |
| Eve of Harvest | CRYSTAL_POWER, ENERGY, ENERGY_REGEN | Catalog only |
| Flare | — | Catalog only |
| Flare Gun | HEALTH | Catalog only |
| Flare Loader | HEALTH | Catalog only |
| Fountain of Renewal | HEALTH, ARMOR, SHIELD | Catalog only |
| Frostburn | CRYSTAL_POWER | Catalog only |
| Halcyon Chargers | HEALTH, COOLDOWN_SPEED, ENERGY, ENERGY_REGEN | Nonstacking boot speed; boot-specific active is catalog-only |
| Heavy Prism | CRYSTAL_POWER | No separate handler |
| Heavy Steel | WEAPON_POWER | No separate handler |
| Hourglass | COOLDOWN_SPEED, ENERGY_REGEN | No separate handler |
| Journey Boots | HEALTH | Nonstacking boot speed; boot-specific active is catalog-only |
| Kinetic Shield | SHIELD | No separate handler |
| Lifespring | HEALTH | Catalog only |
| Light Armor | ARMOR | No separate handler |
| Light Shield | SHIELD | No separate handler |
| Lucky Strike | CRIT_CHANCE, CRIT_DAMAGE | Catalog only |
| Metal Jacket | ARMOR | Catalog only |
| Minion's Foot | CRIT_CHANCE, CRIT_DAMAGE | No separate handler |
| Oakheart | HEALTH | No separate handler |
| Piercing Shard | CRYSTAL_POWER | Shield pierce |
| Piercing Spear | WEAPON_POWER | Armor pierce |
| Poisoned Shiv | WEAPON_POWER, ATTACK_SPEED_DISPLAY | Catalog only |
| Protector Contract | HEALTH | Catalog only |
| Pulseweave | HEALTH | Base movement amplification; triggered effects are catalog-only |
| Reflex Block | HEALTH | Catalog only |
| Rook's Decree | HEALTH, ARMOR, SHIELD, COOLDOWN_SPEED | Catalog only |
| Scout Trap | — | Owned-instance deployment, ally sight and delayed reveal in ARAL |
| ScoutPak | COOLDOWN_SPEED, ENERGY_REGEN | Catalog only |
| ScoutTuff | HEALTH | Catalog only |
| Serpent Mask | WEAPON_POWER | 15% base Vampirism and Book of Eulogies basic-hit kill heal; rechargeable hero damage pool remains catalog-only |
| Shatterglass | CRYSTAL_POWER | No separate handler |
| Shiversteel | HEALTH, ATTACK_SPEED_DISPLAY | Catalog only |
| Six Sins | WEAPON_POWER | No separate handler |
| Slumbering Husk | ARMOR, SHIELD | Catalog only |
| Sorrowblade | WEAPON_POWER | No separate handler |
| Spellfire | CRYSTAL_POWER | Catalog only |
| Spellsword | WEAPON_POWER, COOLDOWN_SPEED, ENERGY_REGEN | Catalog only |
| Sprint Boots | — | Nonstacking boot speed and Sprint activation |
| Stormcrown | HEALTH, ENERGY_REGEN, COOLDOWN_SPEED | Catalog only |
| Stormguard Banner | HEALTH | Catalog only |
| SuperScout 2000 | HEALTH, ENERGY_REGEN, COOLDOWN_SPEED | Catalog only |
| Swift Shooter | ATTACK_SPEED_DISPLAY | No separate handler |
| Teleport Boots  | HEALTH, SHIELD, ARMOR | Nonstacking boot speed; boot-specific active is catalog-only |
| Tension Bow | WEAPON_POWER | Every six seconds a landed basic attack adds 100 + bonus weapon power as weapon damage; strongest Armorbreaker wins |
| Tornado Trigger | ATTACK_SPEED_DISPLAY, CRIT_CHANCE, CRIT_DAMAGE | Catalog only |
| Travel Boots | HEALTH | Nonstacking boot speed; boot-specific active is catalog-only |
| Tyrant's Monocle | WEAPON_POWER, CRIT_CHANCE, CRIT_DAMAGE | No separate handler |
| Void Battery | ENERGY, ENERGY_REGEN | No separate handler |
| War Treads | HEALTH | Nonstacking boot speed; boot-specific active is catalog-only |
| Warmail | ARMOR, SHIELD | Catalog only |
| Weapon Blade | WEAPON_POWER | No separate handler |
| Weapon Infusion | — | Owned-instance consumption; timed weapon power, attack speed, armor and shield |

Tension Bow, Alternating Current and Vampirism use actual hero basic impacts against heroes, lane minions and neutral monsters. Human and bot attacks use the same path. For ranged attacks the victim must still be alive on impact. Host policy counts every second landed hit from first equipping Alternating Current and preserves Tension Bow's cooldown across resale. Bonus hits use their own defense school and do not receive a second critical roll. Vampirism uses the strongest supported rate on actual weapon HP loss, excluding absorbed damage and overkill. The separate Book heal fires once per qualifying basic-hit kill. Building interactions, spell vamp, and Serpent Mask's rechargeable pool remain outside this implementation.

## Boots

The supplement's nonstacking movement bonus uses the largest equipped boot bonus. Sprint Boots supply a further 2 movement speed for 2 seconds and have a 150-second cooldown in the original data. The simulator validates the actor and instance before activation. Resale and purchase of another pair preserve the cooldown. With Sprint Boots available, the local bot strategy can buy a pair and activate it during a long move. Other active and passive variables remain available through the catalog alongside their descriptions and units. Their presence alone does not define a native input or reproduce their combat effects.

The ARAL Healing Flask starter grants a 115-point barrier, 200 health and 30% maximum energy over five seconds, with a 120-second cooldown. The simulation validates the actor’s own native inventory slot. It distributes recovery across server ticks, caps both resources, and absorbs only damage that remains after other barriers. The archive records 62 activations and establishes neither the original tick cadence nor how overlapping barriers are ordered.

The independent optional test in the simulation crate loads both operator directories and checks catalog counts, mapped stores, Sprint Boots parameters, and the four captured sale prices. Set `HALCYON_DELIVERY` and `HALCYON_DATA_EXTRA`, then run `cargo test -p halcyon-sim original_supplement_supplies -- --ignored`.

For the observed Healing Flask and completed-sale messages, witness eligibility and replay scope, see [ARAL transaction witnesses](aral-item-activation.md#aral-transaction-witnesses).
