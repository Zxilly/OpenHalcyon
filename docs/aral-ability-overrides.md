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

## Ringo Double Down

All 60 override blocks follow a death credited to Ringo. Each contains three `041A` actions on the killer with selectors 8, 9 and 10 replaced by selector 3. A definition-379 buff accompanies the blocks. Two more last hits while already primed produced no second block. All 59 matching clear blocks follow Ringo's selector-3 targeted `0415` within 215 ms. They clear selectors 3, 8, 9 and 10 in that order. The receiver saw 47 grants and 46 clears for allied Ringos, and 13 of each for enemy Ringos. The archive has no first-person Ringo stream.

The Android strings `Buff_Ringo_DoubleDown_CritNext` and `Ability__Ringo__CritAttack_DoubleDown` identify the perk and the attack. The ARAL simulation primes a living Ringo once on an enemy last hit. A canceled windup preserves it. The next released basic attack consumes it, including a shot at a structure. The guaranteed critical multiplier applies to the weapon component against heroes and minions. Eight consumed attacks targeted towers, but the structure damage multiplier is unresolved. Death resets the simulated perk.

The room emits the observed override and clear transitions to other registered, scene-ready ARAL players who see Ringo. The targeted release additionally requires sight and registration of the victim. The filtered frames are eligible for that recipient's recording. First-person override projection, dedicated observer projection and restoration of an already-active override at reconnect remain unverified. The authoritative combat result applies independently of presentation.
