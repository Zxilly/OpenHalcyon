# ARAL tower opening applications

The seven archived ARAL matches each declare all ten towers, then issue the same three extended applications (`043F`) on every tower before the initial mode variables (`041F`). All 210 applications are self-sourced. Every tower receives definitions 194, 198 and 204 in that order. All carry raw duration `0xbc00`, stack count 1, four zero words, and zero transport padding.

Both teams use the same five local slots, 3–7. Across all seven matches the attachment instance is the actor ID plus `19 + 5 × (local slot − 3) + 27 × side + ordinal`, using zero-based side and ordinal. All 30 instance values within each opening are distinct. The tower blocks run in descending scene slot order.

The room emits these applications once in each seat's initial unfiltered scene batch, using only towers declared in that batch. Restored scenes and visibility refreshes leave them out. The archive provides no gameplay effect names or gameplay semantics for these definitions, and the simulation adds no tower modifiers. The encrypted scene test checks field bytes, attachment order, and reentry behavior.

## Tower death and structure stack decrements

Across the same seven openings, definition 205 appeared on ten structures per match, after that structure's other opening effects and before the first mode variables. Each application was self-sourced with duration bits `bc00` and four zero words. Each of the two crystals started with two stacks. The other eight structures started with one. The instance IDs below are for local side zero. Side one adds 8 to each slot and 35 to each instance.

| Fallen local slot | Target local slot | Application instance | Recorded decrements |
| ---: | ---: | ---: | ---: |
| 7 | 6 | 2045 | 9 |
| 6 | 5 | 2039 | 8 |
| 5 | 3 | 2027 | 6 |
| 5 | 4 | 2033 | 6 |
| 3 | 1 | 2021 | 6 |
| 4 | 1 | 2021 | 6 |

All 41 ARAL `0447` actions followed the corresponding same-side `0430` tower death at the same receive timestamp. Target and instance matched the prior `043F` application. Each decrement carried the 32-bit operand 1 and two zero transport-padding bytes. To reproduce the join, register structures by their `03F2` actor ID and slot, index definition-205 applications by target and instance, and group the subsequent decrement by the last tower death and team. The two VainTurrets each decrement the crystal once.

A confirmed native player receives decrements for deaths witnessed on that socket's team when the source and affected structure were registered and the introduction for that instance was delivered. Reconnecting players start with fresh scene registration. A restored player scene introduces the current remaining count only when the target has been declared and that team can see every upstream tower used to compute it. An authorized full-map observer initializes the remaining count from its declared live structures and the surviving upstream towers. The room suppresses duplicate death notifications per scene. The evidence establishes the stack transition; damage immunity, resistance and visual interpretation are unverified.

The 40 opening attachment IDs are distinct and end at 2083. The shared simulation allocator starts dynamic actors, shots and effect instances at wire ID 2084 or above. The seven captures first declare lane minions at IDs 3363–4536, after additional initialization by the original host.

All 1,430 negative health impacts targeting these ten structures occurred with zero remaining stacks in the receiving stream. Three target-play actions named structures with one remaining stack. The observations do not give a controlled pre-unlock damage attempt or establish whether original protection acts on targeting, damage, or both. This projection therefore leaves damage resolution to the existing authoritative combat rules.
