# ARAL ability overrides and related actions

Seven battlefield streams among nine complete archived connections contain the following actions. The counts below use complete server frames and resolve actor templates from that stream's own declarations.

| Action | Frames | Observed association |
|---|---:|---|
| `0414` | 48 | Twelve private blocks immediately precede target-selection messages. Kinetic has four blocks of selectors 8–10, Yates five of 13–15 and Idris three of 9–15. The six-byte bodies contain no actor ID. No block identifies a cancellation of the hosted Idris B, selector 3. |
| `0419` | 12 | All reference Reim. The transmitted key `e91e17d5` equals the FNV-1a hash of the Android string `Effect_Reim_B_AOE_Range`. A server simulation of Reim B would provide the trigger. |
| `041A` | 534 | Ringo accounts for 180 frames. Other override blocks belong to Lance, Hero016, Gwen, Caine and Glaive. |
| `041B` | 414 | Ringo accounts for 236 frames. The other clear blocks belong to Gwen, Caine and Glaive. |
| `0425` | 5 | One Idris view references four lane minions and Blackfeather. All five immediately follow `042E`. None has a client request within 300 ms, and the body identifies only one actor. The target-selection trigger remains unresolved. |
| `042F` | 3 | All identify a remote Alpha. None has a matching death, dead-state or respawn action for Alpha within four seconds. |
| `043C` | 1,351 | The trailing 16-bit flag is zero 1,149 times and one 202 times. Multi-selector blocks differ by hero, and flag-one operands also include values 2000 and 2001. A generic stun or death does not establish which selectors the receiver should cancel. |

## Idris attack target after a kill

Five server `0425` records occur in one of seven ARAL battlefield captures. For each record, take the preceding and following server `0415` from the authenticated Idris with selector 14 or 15. Match the previous attack target to the victim in `0430` and Idris to the killer. All five kills attributed to Idris in that stream appear below. Each `0425` actor word equals the next attack target.

| Time in stream (s) | New target | Kill to `0425` (ms) | `0425` to next attack (ms) | Next selector |
| ---: | --- | ---: | ---: | ---: |
| 195.267 | Lead minion | 97 | 314 | 14 |
| 217.669 | Tank minion | 100 | 315 | 14 |
| 218.793 | Ranged minion | 95 | 414 | 14 |
| 223.012 | Tank minion | 2507 | 2351 | 15 |
| 226.870 | Blackfeather | 97 | 298 | 15 |

The latest client `0424` before each record names a different actor. The lead minion had been requested 1.508 s earlier, followed by a request for another actor 0.908 s before the change. At the Blackfeather change, the last reported X/Z positions put an opposing tank minion 4.038 units from Idris and Blackfeather 5.538 units away, using spawn positions updated by `042E`. Position samples are asynchronous and one receiver observes only part of the battlefield. The five records establish ordering on the attacker's connection. Target priority and delivery to other seats remain unknown.

## Ringo Double Down

All 60 override blocks follow a death credited to Ringo. Each contains three `041A` actions on the killer with selectors 8, 9 and 10 replaced by selector 3. A definition-379 buff accompanies the blocks. Two more last hits while already primed produced no second block. All 59 matching clear blocks follow Ringo's selector-3 targeted `0415` within 215 ms. They clear selectors 3, 8, 9 and 10 in that order. The receiver saw 47 grants and 46 clears for allied Ringos, and 13 of each for enemy Ringos. The archive has no first-person Ringo stream.

The Android strings `Buff_Ringo_DoubleDown_CritNext` and `Ability__Ringo__CritAttack_DoubleDown` identify the perk and the attack. The ARAL simulation primes a living Ringo once on an enemy last hit. A canceled windup preserves it. The next released basic attack consumes it, including a shot at a structure. The guaranteed critical multiplier applies to the weapon component against heroes and minions. Eight consumed attacks targeted towers, but the structure damage multiplier is unresolved. Death resets the simulated perk.

The room emits the observed override and clear transitions to other registered, scene-ready ARAL players who see Ringo. The targeted release additionally requires sight and registration of the victim. The filtered frames are eligible for that recipient's recording. First-person override projection, dedicated observer projection and restoration of an already-active override at reconnect remain unverified. The authoritative combat result applies independently of presentation.

## Definition 379

The 60 grants follow 46 lane-minion deaths (24 tank, 13 lead, nine ranged), eight hero deaths, four tower deaths, one crystal death and one vision-object death. The archive also contains two further tank-minion deaths while an override was active. All 62 kills produced a self-applied definition-379 instance with binary16 duration bits `bc00` (−1) and six trailing zero bytes. Sixty applications immediately precede a new override triplet. Of those 60, 58 have an explicit `0445` addressed to the identical actor and instance immediately before the four clears. Of the other two, one actor died while primed and one match ended while primed.

The host emits the ordinary first application with a fresh world ID and cancels that specific instance when the empowered attack releases. Each receiving scene tracks which instance it received. A reconnect begins with an empty instance table. The two overlapping native applications have no matching observed instance cancellation. The host models a single primed attack and emits no additional instance while it is already primed. The observed death and end-of-match cases have no explicit cancellation generated by those events.

The build-147219 protocol specification, topic 4, distinguishes the two bytes of `0414` (selector, shared-group flag) and the last two bytes of `043C` (item branch, shared-group flag). The host accepts a single-ability cancellation from the originating ARAL gameplay connection for its own matching, still-pending native windup. A successful request drops the windup. The host privately echoes the accepted selector/flag (`0414`), then confirms the actor/selector cancellation (`043C`) in the same batch. This order is host policy. All 48 archived server `0414` frames have shared flag one and form 12 blocks immediately followed by `0424`. No block is adjacent to `043C`, and none of the three recording connections sent `0414`. Each stream establishes one receiver. Delivery to additional players or spectators is unknown. The fixture establishes the operation and field order. The seven streams contain no direct client request for Idris B cancellation. Group membership and public delivery for this single-windup case remain unverified.

For an ARAL human Idris, the host transfers an existing attack order after a killing basic hit on that same lane minion when there is exactly one living, visible enemy in attack range with a clear attack line and that enemy is a hero or lane minion. This single-candidate policy avoids choosing between the conflicting nearest and queued targets above. The room sends `0425` only to the controlling gameplay connection, after declaration and while the target remains visible and selected. Topic 4 of the build-147219 specification identifies the receiver's local-controller scope.

All three ARAL `042F` records name wire actor 1501. Each is immediately followed by a self-applied `043E` definition 683, binary16 duration bits `4300` (3.5 seconds). Their effect instances in stream order are 5363, 11126 and 15121. The host sends the pair to registered players with current sight, including Alpha's controller, and to authorized observers with the declaration. The capture shows a remote witness. The controller receives the same state transition under host policy.

A surviving reboot recovers in place. Alpha's native content sets the reboot pool to 600–2600 HP across levels, plus 65% of bonus maximum energy, and specifies a recovery fraction of 0.4. The separate pool can take lethal damage during the 3.5-second window. All three recoveries reset Prime Directive and Core Charge, set the Reboot cooldown and apply self-effect 684. In two recoveries a subsequent `0441` names the new 684 instance and sets the same duration (85 or 75 seconds). The host sends the absolute duration after its application in the recovery batch. The third recorded `0441` sets 26.4475 seconds on an existing 684 instance after a skill upgrade.
