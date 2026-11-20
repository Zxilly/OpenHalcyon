# ARAL tower opening applications

The seven archived ARAL matches each declare all ten towers, then issue the same three extended applications (`043F`) on every tower before the initial mode variables (`041F`). All 210 applications are self-sourced. Every tower receives definitions 194, 198 and 204 in that order. All carry raw duration `0xbc00`, stack count 1, four zero words, and zero transport padding.

Both teams use the same five local slots, 3–7. Across all seven matches the attachment instance is the actor ID plus `19 + 5 × (local slot − 3) + 27 × side + ordinal`, using zero-based side and ordinal. All 30 instance values within each opening are distinct. The tower blocks run in descending scene slot order.

The room emits these applications once in each seat's initial unfiltered scene batch, using only towers declared in that batch. Restored scenes and visibility refreshes leave them out. The archive provides no gameplay effect names or gameplay semantics for these definitions, and the simulation adds no tower modifiers. The encrypted scene test checks field bytes, attachment order, and reentry behavior.
