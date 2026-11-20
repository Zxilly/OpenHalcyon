# ARAL item activation evidence

Seven archived ARAL streams contain 150 server-to-client `0448` records,
66 server-to-client `044A` records and 218 server-to-client `044C` records.
There are no client-to-server `0448`, `0449`, `044A` or `044C` records
in these streams. All 218 activation actors are other seats from the
captured client's perspective.

The inventory audit keys records by actor and instance. A `043D` grant
names the actor, item template and instance. A `044B` changes the actor's
visible holdings. A `044D` names an actor and an instance, with removal
tracked separately by `044B`. The four observed server sell notices all
reference a historical grant for the same actor. The grant has no slot
operand. The two slot bytes in `0438` carry separate information.

Of 218 activations, 195 match a grant currently present in the visible
holding history. All 218 have a unique historical template for the exact
actor and instance. The other 23 follow a removal for that actor and
instance, 20 for Scout Trap and three for Flare. The last removal in those
cases precedes activation by 4.28–112.45 seconds. Purchase, sale and absolute
charge or inventory-slot state cannot be reconstructed from these messages
alone.

| Correlation | Observations |
| --- | --- |
| `0448` instance to `044C` | 142 of 150 records are followed within 250 ms by a same-instance mode-2 activation. The instance alone can match multiple actor inventories. In 142 of 150 cases it has multiple currently recorded holders, including 66 cases with all six seats. |
| `044A` word at +12 to `044C` | All 66 are followed within 250 ms by a same-instance mode-1 activation. One has two temporal candidates. The first and third words in `044A` exactly match the first candidate's position at `044C` +12 and +16 in 63 of 66 cases. |
| Vision Totem item template 526 | All 48 observed activations are followed by a compact spawn of entity template 392 carrying the activating actor reference at +112, at the same recorded segment timestamp. The spawn's X/Z exactly matches the mode-1 position in 30 cases. The other 18 differ by about 0.02–3.62 units. |
| Scout Trap item template 495 | All 29 observed activations have a compact spawn of entity template 388 with the same actor reference at +112 and the same recorded segment timestamp. This includes the 20 activations after a visible inventory removal. |
| Healing Flask item template 457 | 62 observed mode-2 activations. In 38 cases the same actor has a positive self-sourced `041E` within 250 ms. The first observed amounts vary from approximately 12.06 to 31.95 and do not establish an authoritative healing formula. |

The local delivery metadata independently identifies both vision item
templates and spawned entity templates. These records establish the
observed spawn and its owner reference. The original client request,
placement adjustments, charge use and authoritative repeat rules remain
unverified. The [existing codecs](../crates/protocol/src/item_actions.rs)
retain all original words and mode-dependent fields. The host uses the witnessed remote activation prefixes for its own successful deployments.
## Extended applications at match opening and on purchase

Across seven ARAL matches, 741 of the 17,018 extended applications (`043F`) arrived before the initial six `041F` game-mode variables. Each opening contained 103–112 applications. Definitions 194, 198 and 204 each appeared exactly once on every tower (70 applications per definition), always self-sourced, with duration bits `bc00`, one stack and four zero words. Those three definitions never appeared again as extended applications. Their effect names and the tower properties they control remain unresolved. On all 42 opening hero seats, definition 255 had first extra float 15. Its 1,122 later applications had fresh actor/instance keys; in a window of three adjacent actions either side and 120 ms there was no matching stat-6 gold record on the same actor. A literal 15 is insufficient to identify this effect with ARAL's gold income.

For the 50 verified Idris B and Lorelai A/B/C releases, no extended application to the caster or cast target appeared within the next 31 records and 120 ms. Two shop effects have a stronger association. All five Serpent Mask grants were followed by self-application of definition 305, and both Eve of Harvest grants by definition 307. Every application carried a new instance, half-duration bits `bc00`, an initial stack count of 800, four zero words and four zero suffix bytes. The delivered items both declare CAP_MIN 400, CAP_MAX 800, RECHARGE_TIME 120 and OMNI_VAMP 0.15. An independent analysis of the Serpent controller also identifies definition 305 and an initial stack operand taken from CAP_MAX. It distinguishes a separate conditional capacity-backed contribution from the base 0.15 coefficient. The current simulation supplies fixed item stats; the missing damage eligibility and recharge clock preclude publishing a persistent 800-stack display. [Serpent control and selected native callbacks](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/stat_stacking_2026-10-16/02_LIFESTEAL.md)

Every one of the 1,831 `0443` stack updates in the nine-stream inventory joined to a prior application by (target actor, buff instance). They covered 14 definitions. Serpent instances accounted for 593 updates with values 0–800, including 187 repetitions of the current count. Eve instances accounted for 558 updates with values 0–733 and 216 repetitions. The typed reader `Message::buff_stack()` exposes the unsigned absolute count at +8 and retains the original suffix. Its writer accepts the native 12-byte body or the observed 14-byte form. The existing `Fields::ActionWords` projection remains available.

## ARAL transaction witnesses

All 62 archived Healing Flask activations name another hero, comprising 23 allies and 39 enemies. Each mode-2 activation has an all-ones unused target word and zero coordinate words. For 61, an instance-matching `0448` preceded `044C` by at most 250 ms within the previous 30 records. Several inventories share the same instance number. The host emits the pair together for a successful activation bound to an actor and slot. A severely injured bot may use its own available flask on a decision tick. An authorized observer needs an existing actor declaration. A player needs the live world stream, a prior declaration and continued visibility. The actor's own stream receives the regular authoritative healing events.

The four archived sales involve one opponent and three allies. Hourglass, Blazing Salvo and Stormcrown each produced `044D`, reversed the item's modifiers with `041C`, removed the held instance with `044B` (reason 1), then set the actor's absolute gold with `041D` (index 6, mode 1). The host emits all four sale variants. Scout Trap has no modifier reversal. Its owned-instance deployment and limited witness output are described below. A purchase grants the instance earlier through `043D`.

## ARAL vision devices

The home-authenticated player item action endpoint submits these commands to the current room host. Its private context returns the caller’s inventory and exposed enemy cameras visible to the caller’s team. [Player API](player-api.yaml)

A local actor command deploys a held Scout Trap at the hero’s feet or spends a charge on that hero’s starter Vision Totem to place a Scout Cam. All 48 original cam activations used mode 1 with an all-ones target and a matching instance in a preceding `044A`. All 29 trap activations used mode 2 with an all-ones target, two zero coordinate words and a preceding `0448`. Both used `044C`. Instance 2001 belongs separately to each hero. The server allocates every deployed object a fresh world actor ID and records its owner and team.

The server joins the equipped item’s template to its mode and checks ownership, range, inventory or charges before deployment. A camera provides 10 units of allied sight for 150 seconds, becomes hidden after two seconds, and takes five visible enemy hits. Its item definition gives 7.5 placement range and two charges. The host follows the bilingual description’s 180-second sequential recharge. The separate active field named Cooldown is 150 seconds. A trap grants six units of allied sight, arms after two seconds, triggers within 1.9 units of an enemy hero and detonates 1.3 seconds later. Enemies within 3.2 units are revealed for five seconds. Trap damage falloff and damage school remain unresolved. Sight requires the local map visibility check. A room bounds live devices at 256, with admission checked before spending an instance or charge.

Live witness delivery requires the owner’s prior declaration and current visibility. An opposing seat must also see the placement point. The owner is omitted from these remote-only prefixes. The full-view observer must be authorized and have the owner’s declaration. The 77 archived `03F2` bodies were all 126 bytes with the owner at +112 and a dynamic slot at +116. The two words at +88/+92 had seven session-specific pairs for the cam and four for the trap. The trap used twelve different height values. Sixteen of 48 cam IDs had a later `0430`; no trap had one, and none of the 77 IDs had a corresponding `0431`. Compact client declarations and slot reuse are deferred pending support for that device-specific removal path. No native client-originated use request was present in these archives.

## Reflex Block (485)

The ARAL host accepts an authenticated owner's instant item press (`0448`) or the player API's `activateReflexBlock` with the owned instance. A successful activation grants a level-scaled 100–600 barrier and blocks new debuffs for 1.5 seconds. Existing debuffs continue. The installed item table supplies the two endpoints and duration. The 90-second cooldown follows the [official 4.5 item changes](https://pt.vainglorygame.com/news/update-4-5-hero-item-balance-changes/).

In seven complete ARAL streams, all four witnessed activations of item 485 carry the same sequence: instant press, untargeted `044C`, three self-applied `043E` effects with consecutive IDs (61 Revealed, 268 Reflex Block, 18 Block Debuffs), with the barrier amount in `041D` immediately before 18. Observed barrier values were 509.09, 600, 402 and 600. Canonical protocol specification §2.1, layouts and the Reflex Block transaction in fixture 03 independently supply the HUD writer form and the representative wire prefixes. The captures show other actors' presses via server relay. The host scopes activation to the owner's item and its current cooldown, skips the echoed button press for that owner, and filters other recipients by declaration and visibility.

Five of six observed 268 instances have matching `0446` timeouts after 1.395–1.496 seconds. Three belong to Reflex Block, two to one Crucible activation. Recording stopped 0.66 seconds after the fourth Reflex Block activation. The other 13 timeouts are for Baptiste effect 936. The host expires the witnessed 268 instance once, for a living actor and only on connections that received its application. The traces establish no separate timeout for 61 or 18. Native HUD rendering and use from a stock client have not been tested for this feature.

## Infusion level updates

Seven complete ARAL battles contain 15 self-applied weapon/crystal infusion declarations (definitions 283/284). Thirteen immediately precede a same-instance `0443` with the owner's level. A fourteenth matching update follows a crystal infusion by 107.19 seconds, when that actor levels up. One additional adjacent stack update addresses a different instance. One recorded replacement cancels the older instance after declaring the new one. Six untargeted, mode-2 activations have a matching `044B` removal of the held item. Nine other declarations follow mode-0 self-targeted activation; those observations do not establish a second player input.

On an authorized native item press or player API consumption, the room emits the held-item activation, a 150-second self application, its absolute level, and inventory removal. It sends subsequent level changes and replacement cancellation only to scene connections that saw the matching instance. The simulation expires the effect at its deadline. The captures establish no separate natural timeout message for these instances. The broadcast currently covers the level and lifecycle; the individual native stat modifier frames are outside this projection.
