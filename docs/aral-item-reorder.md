# ARAL item slot reorder

The seven archived ARAL streams contain four server-to-client `0438`
records and no matching client-to-server requests. All four bodies are
six bytes and name a different hero than the recorder. The slot pairs are
`(4,3)`, `(3,2)`, `(6,5)`, and `(3,2)`. Each referenced pair can be
reconstructed as occupied from the preceding grants and removals for the
same actor. The four reconstructed occupancies are five, four, eight and
four slots. The join uses actor and instance together. Instances alone can
collide across actors, and the grant itself carries no slot number.
[Item activation and grant audit](aral-item-activation.md)

The Android writer at `0xBE7408` serializes one BE32 actor and two separate
slot bytes. The independent Windows serialization routine `0094C0B0`
indexes live and deferred arrays directly with action fields `+0x10` and
`+0x14`, swaps the entries in both arrays, and emits their low bytes in
that same order. This establishes zero-based indices and a symmetric
swap. The bytes have an order in the packet. A separate source/destination
effect is unverified. The original Windows `GameMode_Aral_Casual` allows
eight native array positions, which includes entries that may be hidden
from the HUD. [Independent native inventory report](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/NATIVE_INVENTORY_2026-10-04.md)

The original opening in all seven played matches grants Healing Flask
(457, instance 2000) and Vision Totem (526, instance 2001) in that order
to every hero. All 42 opening hero declarations have an empty inventory
baseline before these grants. The server initializes the first two native positions with these starter
items. Six positions are initially free for purchased equipment. Native item use is a separate contract.

The server's ARAL inventory keeps eight fixed slots. When a recipe uses
components, one transaction removes those exact instances, keeps gaps,
and then fills the first free slot. Other modes keep their existing
inventory rules. A native swap requires the authenticated owner's world
connection, the owner's actor and two distinct occupied indices. A
matching peer notification is sent only to connected seats with a
currently visible declaration of that actor. The requester's original UI
may already have applied a local swap. The server sends no owner echo.
The observed recipients were other players, and the original Android
receiver ignores inbound `0438`. The inspected Windows apply routine
calls its reorder callback in server mode. Peer broadcasts therefore
carry the observed packet shape, while native peer UI updates are unverified.

The six-byte form contains no application request ID. Two consecutive
identical messages on the authenticated stream perform two swaps. A
reconnect never replays the previous swap. It receives only current
items in ascending authoritative slot order, with the original instances.
First-free grants can reconstruct a dense prefix exactly. A live
inventory with an internal gap remains sparse on the server. The fresh
client's grant projection may compact that gap, so the server disables
slot swaps for that connection. It emits no transient grants, removals
or extra stat or currency changes to fill holes. The seven played streams contain 42 hero declarations, all during opening.
No later hero declaration demonstrates a native slot-addressed snapshot.
