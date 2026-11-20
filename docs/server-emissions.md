# Observed server emissions

Nine complete TCP streams contain 398,293 encrypted server frames in 103 opcode classes. The plaintext handshake acknowledgment is outside these counts. Each count includes every captured form of the opcode. The scope column describes the subset the host currently emits.

**A** is authoritative room output. **H** is setup, draft or scene output. **E** is a conditional response to the originating request. **R** is a read-only class with a documented action role. **U** has readable fields and an unresolved server role. Action names come from the independently labeled Windows action table. Counts come from the full captured streams. [Capture method](live-captures.md) · [Independent action labels](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/vg-binary-event-candidates-2026-09-09-details.md)

The host paths are in [room dispatch](../crates/game/src/lib.rs), [transport](../crates/game/src/transport.rs), [wire messages](../crates/game/src/messages.rs), [ARAL](../crates/game/src/aral.rs), [native abilities](../crates/game/src/native_abilities.rs), [Solo scene loading](../crates/game/src/scene.rs) and [settlement](../crates/game/src/result_frames.rs). The table excludes test fixtures and replay-only encoders.

| Opcode | Frames | Path | Action and emitted scope |
|---|---:|:---:|---|
| `0000` | 1,175 | E | Session clock |
| `03E8` | 16 | H | First authenticated peer attachment during ARAL draft. Fourteen early notifications carry another roster member’s UUID. The two later triggers remain unresolved. [Draft membership](aral.md) |
| `03E9` | 9 | H | Actor and mode settings |
| `03EA` | 10 | U | Word |
| `03EC` | 6 | U | Word |
| `03ED` | 18 | H | Other ARAL seat entering an already registered peer’s scene |
| `03EE` | 84 | H | Participant declarations at load or restore |
| `03EF` | 7 | H | ActionStartMatch · Room start or readiness confirmation |
| `03F1` | 9 | A | ActionEndMatch · Natural crystal or ARAL surrender result; also historical non-ARAL draft synchronization form |
| `03F2` | 1,376 | A | ActionEntitySpawn · Known compact spawn profiles for the selected mode |
| `03F3` | 42 | A | ActionHeroSpawn · Full actor declaration for simulated heroes and fallback profiles |
| `03F8` | 37,568 | A | ActionMoveTo · Accepted Solo 03F5 destination, visible registered ARAL hero route from 03F4/03F5, and [visible native lane-minion waypoints](native-motion.md#lane-minion-destinations) |
| `03F9` | 2,548 | R | [ActionMoveToServerAuthoritative](native-motion.md) |
| `03FA` | 1,978 | R | [ActionMoveToAndFace](native-motion.md) |
| `03FB` | 1,829 | R | [ActionStopActor](native-motion.md) |
| `0402` | 25 | R | ActionAutoActorBounce |
| `0403` | 525 | R | ActionAutoMoveTo |
| `0406` | 25 | R | ActionAutoOrbit |
| `0407` | 25 | R | ActionCancelAutoOrbit |
| `0409` | 67 | R | ActionTeleportTo |
| `040B` | 1,111 | A | ActionEntityDestroy · Delayed removal of eligible ARAL lane minions |
| `040D` | 4,751 | A | [ActionFireProjectile](native-projectiles.md#authored-aral-tracked-fire) · ARAL tower and level-2 ranged lane-minion tracked fire |
| `040E` | 383 | R | [ActionFireProjectile](native-projectiles.md) |
| `040F` | 585 | R | [ActionFireProjectile](native-projectiles.md) |
| `0410` | 376 | R | [ActionDetonateProjectile](native-projectiles.md) |
| `0412` | 842 | E | ActionRequestActivateAbility · Accepted Idris B or Lorelai A/B point request on its originating connection |
| `0414` | 48 | R | ActionRequestCancelAbility_Client |
| `0415` | 9,106 | A | ActionPlayAbility · Released Lorelai C and native lane-minion/tower basic attacks with visible actors |
| `0416` | 886 | A | ActionPlayAbility · Released Idris B or Lorelai A/B with visible caster |
| `0419` | 12 | R | ActionPlayPfxAtLocation |
| `041A` | 534 | R | ActionOverrideAbility |
| `041B` | 414 | R | ActionClearAbilityOverride |
| `041C` | 6,668 | A | ActionModifyActorAttribute · Supported inventory modifiers and hero progression fields |
| `041D` | 109,869 | A | ActionModifyActorResource · Simulated resource deltas including earned gold and purchase debits |
| `041E` | 16,559 | A | ActionImpactHealth · Applied damage or healing with visible participants |
| `041F` | 42 | R | ActionModifyGameModeVar |
| `0421` | 22 | R | ActionCreateZoneOfControl |
| `0423` | 22 | R | ActionDestroyZoneOfControl |
| `0424` | 2,268 | E | ActionRequestModifyBasicAttackTarget · Accepted ARAL basic attack target |
| `0425` | 5 | R | ActionModifyBasicAttackTarget |
| `0428` | 2,000 | R | ActionModifyCooldown |
| `042B` | 53,111 | A | ActionModifyVisibility · [Initial ARAL lane-minion deltas](native-visibility.md) after the first waypoint |
| `042E` | 62,855 | A | ActionMoveToCorrection · Simulated positions visible to the recipient |
| `042F` | 3 | R | ActionActorUndead |
| `0430` | 1,032 | A | ActionActorDie · Visible death with its authoritative source |
| `0431` | 958 | A | ActionActorDead · Delayed ARAL hero or minion death lifecycle |
| `0432` | 63 | A | ActionActorRespawn · Simulated hero respawn |
| `0433` | 103 | A | ActionStartRespawnTimer · Eligible ARAL hero respawn timer |
| `0434` | 315 | A | ActionLevelUp · Hero level increase |
| `0435` | 210 | A | ARAL first lane wave and first hero-on-hero death |
| `0436` | 329 | E | ActionRequestUpgradeAbility_Client · Accepted supported ability learn request |
| `0438` | 4 | A | [ActionReorderItem](aral-item-reorder.md) · Authenticated swaps of occupied ARAL slots, notified to visible peers |
| `0439` | 342 | E | ActionBuyItem · Successful supported purchase |
| `043A` | 317 | A | ActionGrantAbility · Granted rank for mapped native hero ability |
| `043C` | 1,351 | R | ActionCancelAbility |
| `043D` | 425 | A | ActionGrantItem · Acquired or restored held item instance |
| `043E` | 44,346 | A | ActionApplyBuff · [Lorelai’s four-second Waterwall on an ally](native-abilities.md), reported to the caster |
| `043F` | 17,018 | R | ActionApplyBuff |
| `0440` | 60 | R | ActionShowFlyoutText |
| `0441` | 385 | R | ActionModifyBuffDuration |
| `0443` | 1,831 | R | ActionModifyBuffStack |
| `0445` | 1,398 | R | ActionCancelBuff |
| `0446` | 18 | R | ActionTimeoutBuff |
| `0447` | 41 | R | ActionDecrementBuffStack |
| `0448` | 150 | R | [ActionRequestActivateItem](aral-item-activation.md) |
| `044A` | 66 | R | [ActionRequestActivateItem](aral-item-activation.md) |
| `044B` | 150 | A | ActionConsumeItem · Consumed recipe component instance |
| `044C` | 218 | R | [ActionItemActivated](aral-item-activation.md) · All 218 broadcasts link to a prior grant for the same actor and instance |
| `044D` | 4 | R | ActionSellItem |
| `044E` | 84 | R | [ActionPing](native-pings.md) · Actor, optional target, map position and two numeric selectors |
| `044F` | 48 | R | [ActionHUDQuickMessage](native-pings.md) · Actor, message selector, optional target, float and operand |
| `0451` | 248 | H | Opening world clock |
| `0452` | 5 | A | Crystal ending on supported result path |
| `0453` | 4,125 | H | Catalog entries on setup or supported refresh |
| `0454` | 15 | H | Mode announcement on setup or supported refresh |
| `0456` | 26 | U | Control byte |
| `0458` | 32 | E | Draft refresh request completion |
| `0459` | 92 | H | Participant selection state for supported full roster modes |
| `045A` | 1,085 | H | Six-seat ARAL roster |
| `045C` | 95 | H | Solo scene readiness rows |
| `045E` | 6 | E | Native hero selection request |
| `045F` | 65 | E | Native skin selection request |
| `0461` | 2 | U | Control byte |
| `0462` | 6 | U | Control byte |
| `0463` | 4 | E | Draft lock with first-lock roster ordering |
| `046B` | 24 | E | Draft control byte |
| `046C` | 7 | H | Loading transition |
| `046D` | 42 | E | Mode-specific scene confirmation |
| `046E` | 25 | H | Scene request acknowledgment |
| `046F` | 7 | H | Scene settings |
| `0470` | 16 | H | Scene transition marker |
| `0471` | 25 | E | Scene request with authenticated owner on native opening path |
| `0473` | 22 | A | ActionTeamSurrenderStateChanged · ARAL team surrender board |
| `0479` | 1 | U | 64-byte block and halfword |
| `0480` | 8 | H | First ARAL draft refresh marker |
| `0481` | 41 | A | ActionEquipTalent · Authorized numeric ARAL talent level or explicit clear |
| `0484` | 41 | E | ActionRequestEquipTalent · Accepted ARAL talent selection change |
| `0485` | 40 | E | ActionRequestRecommendedBuildPath · Accepted recommended build choice |
| `0488` | 40 | A | ActionSetRecommendedBuildPath · Actor's applied recommended build choice |
| `048A` | 322 | H | [Initial zero-state sync for ARAL Lorelai A/B/C and Idris B, plus Solo Idris B](native-abilities.md#cooldown-keys-and-initial-state) |
| `048C` | 738 | A | ActionSendAggroState_Client · Visible registered native tower target or clear, in ARAL and Solo |
| `048D` | 7 | A | End-of-match statistics from supported room outcomes |
| `048F` | 1 | A | [Declined ARAL shop order](aral-shop.md#declined-order) |

All 15 captured originating point-cast requests have a byte-identical server acknowledgment before the actor-bearing release. The 842 server-side point requests include other traffic. A canceled accepted cast can therefore have an acknowledgment without a release in the host. The sole originating target cast has an actor-bearing release and no corresponding target-request echo.
