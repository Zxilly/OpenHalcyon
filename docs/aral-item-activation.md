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
retain all original words and mode-dependent fields. The host adds no item
activation behavior from these correlations.
