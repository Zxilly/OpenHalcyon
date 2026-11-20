# Observed server emissions

Nine complete TCP streams contain 398,293 encrypted server frames in 103 opcode classes. The plaintext handshake acknowledgment is outside these counts. Each count includes every captured form of the opcode. The scope column describes the subset the host currently emits.

All 103 classes have a host send path in at least one scenario. 62 are driven by the authoritative room, 19 by setup or draft, 21 by an accepted request, and one by an administrator terminating a room. The 173-entry opcode registry also includes other directions and actions absent from these recordings. Captured variants and their original triggering conditions are documented separately where known.

**A** is authoritative room output. **H** is setup, draft or scene output. **E** is a conditional response or relay of an originating request. **O** is an operator action. Action names use the Android build 147219 specification and the independent Windows action table. Counts come from the full captured streams. [Capture method](live-captures.md) · [Independent action labels](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/vg-binary-event-candidates-2026-09-09-details.md)

The host paths are in [room dispatch](../crates/game/src/lib.rs), [transport](../crates/game/src/transport.rs), [wire messages](../crates/game/src/messages.rs), [ARAL](../crates/game/src/aral.rs), [native abilities](../crates/game/src/native_abilities.rs), [Solo scene loading](../crates/game/src/scene.rs) and [settlement](../crates/game/src/result_frames.rs). The table excludes test fixtures and replay-only encoders.

| Opcode | Frames | Path | Action and emitted scope |
|---|---:|:---:|---|
| `0000` | 1,175 | E | Session clock |
| `03E8` | 16 | H | First authenticated peer attachment during ARAL draft. Fourteen early notifications carry another roster member’s UUID. The two later triggers remain unresolved. [Draft membership](aral.md) |
| `03E9` | 9 | H | Actor and mode settings |
| `03EA` | 10 | A | Scoreboard AFK on loss of the last world channel during play |
| `03EC` | 6 | A | Clear AFK and deserter flags after the replacement world stream is ready |
| `03ED` | 18 | H | Other ARAL seat entering an already registered peer’s scene |
| `03EE` | 84 | H | Participant declarations at load or restore |
| `03EF` | 7 | H | ActionStartMatch · Room start or readiness confirmation |
| `03F1` | 9 | A | ActionEndMatch · Natural crystal or ARAL surrender result; also historical non-ARAL draft synchronization form |
| `03F2` | 1,376 | A | ActionEntitySpawn · Known compact spawn profiles for the selected mode |
| `03F3` | 42 | A | ActionHeroSpawn · Full actor declaration for simulated heroes and fallback profiles |
| `03F8` | 37,568 | A | ActionMoveTo · Accepted Solo 03F5 destination, visible registered ARAL hero route from 03F4/03F5, and [visible native lane-minion waypoints](native-motion.md#lane-minion-destinations) |
| `03F9` | 2,548 | A | ActionMoveToServerAuthoritative · A visible Solo bot takes a new waypoint after an earlier order reached that connection. The host chooses when to replace a route. [Bot navigation](native-motion.md) |
| `03FA` | 1,978 | A | [Ringo facing the target of a released Double Down attack](native-motion.md#ringo-empowered-attacks) |
| `03FB` | 1,829 | A | [ActionStopActor](native-motion.md#aral-chakram-movement-interruption) for the registered teammate view of an ARAL Idris B release that interrupted walking |
| `0402` | 25 | A | ActionAutoActorBounce · Amael’s ordinary ARAL Super Kick hits a visible enemy hero. [Recorded profile](aral-orbit.md#amaels-ordinary-super-kick) |
| `0403` | 525 | A | ActionAutoMoveTo · Karas starts his Twister dash, or Amael releases a Super Kick with a nonzero lunge. [Karas](native-motion.md#aral-karas-twister) · [Amael](aral-orbit.md#amaels-ordinary-super-kick) |
| `0406` | 25 | A | ActionAutoOrbit · A level-ten Mobile Bulwark begins orbiting Lyra. [Opening sequence](aral-orbit.md#hosted-mobile-bulwark) |
| `0407` | 25 | A | ActionCancelAutoOrbit · A previously declared Mobile Bulwark stops orbiting. [Cleanup](aral-orbit.md#hosted-mobile-bulwark) |
| `0409` | 67 | A | ActionTeleportTo · Six ticks before ARAL respawn, a registered viewer sees the hero approach its visible pad, paired with `042E`. The four flag-zero relocations have separate triggers. [Respawn sequence](game-wire.md) |
| `040B` | 1,111 | A | ActionEntityDestroy · Delayed removal of eligible ARAL lane minions |
| `040D` | 4,751 | A | [ActionFireProjectile](native-projectiles.md#authored-aral-tracked-fire) · ARAL tower and level-2 ranged lane-minion tracked fire; [Solo outer towers against three minion templates](native-projectiles.md#solo-outer-towers) |
| `040E` | 383 | A | Winter Spire point projectile for registered ARAL witnesses. [Wire sequence](native-projectiles.md) |
| `040F` | 585 | A | ARAL Chakram outgoing flight. [Wire sequence](native-projectiles.md) |
| `0410` | 376 | A | Chakram catch on a connection that received the launch. [Wire sequence](native-projectiles.md) |
| `0412` | 842 | E | ActionRequestActivateAbility · Accepted Idris B or Lorelai A/B point request on its originating connection |
| `0414` | 48 | E | ActionRequestCancelAbility_Client · Private echo after canceling the issuer’s own pending ARAL single-ability windup. The archived shared-group blocks have different triggers. [Cancellation](aral-ability-overrides.md) |
| `0415` | 9,106 | A | ActionPlayAbility · Released Lorelai C, witnessed Reim and Lyra B self-casts and native lane-minion/tower basic attacks with visible actors |
| `0416` | 886 | A | ActionPlayAbility · Released Idris B or Lorelai A/B with visible caster |
| `0419` | 12 | A | [ARAL Reim B area effect after an authoritative release](native-abilities.md#reim-b-chill-winds) |
| `041A` | 534 | A | ActionOverrideAbility · [ARAL Ringo Double Down after a last hit](aral-ability-overrides.md) |
| `041B` | 414 | A | ActionClearAbilityOverride · [ARAL Ringo Double Down consumed by an attack](aral-ability-overrides.md) |
| `041C` | 6,668 | A | ActionModifyActorAttribute · Supported inventory modifiers and hero progression fields |
| `041D` | 109,869 | A | ActionModifyActorResource · Simulated resource deltas including earned gold and purchase debits |
| `041E` | 16,559 | A | ActionImpactHealth · Applied damage or healing with visible participants, including Reim B damage family 74 |
| `041F` | 42 | H | ActionModifyGameModeVar · Six initial keys after the authenticated native ARAL or Solo scene request. [Wire order](game-wire.md#initial-game-mode-variables) |
| `0421` | 22 | A | ActionCreateZoneOfControl · Open the Mobile Bulwark zone around its declared anchor. [Lifecycle](aral-orbit.md#hosted-mobile-bulwark) |
| `0423` | 22 | A | ActionDestroyZoneOfControl · Remove a Mobile Bulwark zone from a connection that received its opening. |
| `0424` | 2,268 | E | ActionRequestModifyBasicAttackTarget · Accepted ARAL basic attack target |
| `0425` | 5 | A | ActionModifyBasicAttackTarget · When an ARAL Idris player last-hits their current minion target, select the sole legal visible enemy in range. [Target transfer](aral-ability-overrides.md) |
| `0428` | 2,000 | A | Gwen A absolute cooldown reset after Skedaddle with the verified Speed Loader talent. [Cast sequence](gwen-skedaddle.md) |
| `042B` | 53,111 | A | ActionModifyVisibility · [Initial ARAL lane-minion deltas](native-visibility.md) after the first waypoint |
| `042E` | 62,855 | A | ActionMoveToCorrection · Simulated positions visible to the recipient |
| `042F` | 3 | A | ActionActorUndead · ARAL Alpha takes lethal damage with Infinite Reboot available and enters the targetable reboot state. |
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
| `043C` | 1,351 | E | ActionCancelAbility · Confirm the specific actor and selector on the originating connection after an accepted cancellation. [Cancellation](aral-ability-overrides.md) |
| `043D` | 425 | A | ActionGrantItem · Acquired or restored held item instance |
| `043E` | 44,346 | A | [Three alternating effect pairs on each declared Solo crystal at match start](solo-lane.md). [ARAL Waterwall on an ally (issuer), Chakram self-application (issuer and registered teammates), and the one-second Chill Winds root (registered teammates)](native-abilities.md) |
| `043F` | 17,018 | H | [Solo opening applications on visible structures](solo-lane.md). [Three initial self-sourced applications per declared ARAL tower](aral-tower-opening.md). [ActionApplyBuff opening and equipment instances](aral-item-activation.md#extended-applications-at-match-opening-and-on-purchase) |
| `0440` | 60 | A | CLEANSED on witnessed Gwen Skedaddle. [Flyout and effect order](gwen-skedaddle.md) |
| `0441` | 385 | A | ActionModifyBuffDuration · Set the duration of Alpha’s cooldown effect when Infinite Reboot succeeds. |
| `0443` | 1,831 | A | ActionModifyBuffStack · Set an active ARAL weapon or crystal infusion instance to the hero’s current level on activation or level-up. [Infusions](aral-item-activation.md) |
| `0445` | 1,398 | A | [Cancellation of Ringo’s previously delivered Double Down instance](aral-ability-overrides.md#definition-379) |
| `0446` | 18 | A | Reflex Block natural timeout on an authorized connection that saw the instance. [Item activation](aral-item-activation.md) |
| `0447` | 41 | A | [ARAL same-side structure stack decrement following a witnessed tower death](aral-tower-opening.md#tower-death-and-structure-stack-decrements) |
| `0448` | 150 | A | ARAL Healing Flask and Scout Trap instance prefixes for eligible witnesses.  [ActionRequestActivateItem](aral-item-activation.md) |
| `044A` | 66 | A | ARAL Scout Cam placement for eligible witnesses. [ActionRequestActivateItem](aral-item-activation.md) |
| `044B` | 150 | A | ActionConsumeItem · Consumed recipe component instance |
| `044C` | 218 | A | ARAL Healing Flask and Scout Trap mode 2, and Scout Cam mode 1.  [ActionItemActivated](aral-item-activation.md) · All 218 broadcasts link to a prior grant for the same actor and instance |
| `044D` | 4 | A | ARAL completed sales of Hourglass, Blazing Salvo, Stormcrown or Scout Trap, witnessed by other visible players or an authorized observer. |
| `044E` | 84 | E | ActionPing · Relay an authenticated native tactical map or visible-target ping to eligible teammates. [Accepted selectors](native-pings.md) |
| `044F` | 48 | E | ActionHUDQuickMessage · Relay supported presets from an authenticated seat. GLHF also reaches opponents. [Scope](native-pings.md) |
| `0451` | 248 | H | Opening world clock |
| `0452` | 5 | A | Crystal ending on supported result path |
| `0453` | 4,125 | H | Catalog entries on setup or supported refresh |
| `0454` | 15 | H | Mode announcement on setup or supported refresh |
| `0456` | 26 | E | ARAL reroll confirmation to the requesting player when free rerolls are enabled and their unlocked seat has a reroll available. [Free reroll rules](aral.md) |
| `0458` | 32 | E | Draft refresh request completion |
| `0459` | 92 | H | Participant selection state for supported full roster modes |
| `045A` | 1,085 | H | Six-seat ARAL roster |
| `045C` | 95 | H | Solo scene readiness rows |
| `045E` | 6 | E | Native hero selection request |
| `045F` | 65 | E | Native skin selection request |
| `0461` | 2 | E | Draft lane preference or deselection, with the updated seat roster. [Draft](aral.md) |
| `0462` | 6 | E | Draft build preference or deselection, with the updated seat roster. [Draft](aral.md) |
| `0463` | 4 | E | Draft lock with first-lock roster ordering |
| `046B` | 24 | E | Draft control byte |
| `046C` | 7 | H | Loading transition |
| `046D` | 42 | E | Mode-specific scene confirmation |
| `046E` | 25 | H | Scene request acknowledgment |
| `046F` | 7 | H | Scene settings |
| `0470` | 16 | H | Scene transition marker |
| `0471` | 25 | E | Scene request with authenticated owner on native opening path |
| `0473` | 22 | A | ActionTeamSurrenderStateChanged · ARAL team surrender board |
| `0479` | 1 | O | An administrator terminates a room. The host attempts a final encrypted exit frame with the recipient’s own UUID and code zero before closing the connection. |
| `0480` | 8 | H | First ARAL draft refresh marker |
| `0481` | 41 | A | ActionEquipTalent · Authorized numeric ARAL talent level or explicit clear |
| `0484` | 41 | E | ActionRequestEquipTalent · Accepted ARAL talent selection change |
| `0485` | 40 | E | ActionRequestRecommendedBuildPath · Accepted recommended build choice |
| `0488` | 40 | A | ActionSetRecommendedBuildPath · Actor's applied recommended build choice |
| `048A` | 322 | H | [Complete opening cooldown syncs for the 30 observed ARAL templates and nine Solo templates](native-abilities.md#cooldown-keys-and-initial-state) |
| `048C` | 738 | A | ActionSendAggroState_Client · Visible registered native tower target or clear, in ARAL and Solo |
| `048D` | 7 | A | End-of-match statistics from supported room outcomes |
| `048F` | 1 | A | [Declined ARAL shop order](aral-shop.md#declined-order) |

All 15 captured originating point-cast requests have a byte-identical server acknowledgment before the actor-bearing release. The 842 server-side point requests include other traffic. A canceled accepted cast can therefore have an acknowledgment without a release in the host. The sole originating target cast has an actor-bearing release and no corresponding target-request echo.

The Android specification identifies all six previously unnamed action roles. Its actor flags address roster seats; the targeted disconnect action compares the supplied UUID with the local player. The three draft requests can appear as server echoes even though the Android inbound dispatcher ignores them. The roster carries the resulting draft state. Of 26 reroll echoes, 17 fall between two skin-selection records when periodic traffic is excluded. [Draft evidence](aral.md)

## Outside the nine encrypted server streams

The remaining 70 registry entries comprise 22 server-to-client, 26 client-to-server, 17 bidirectional and five reserved entries. Three of the 22 server-only entries have named roles in the operator specification.

| Opcode | Evidence and host behavior |
|---|---|
| `0006` | The host writes the plaintext handshake acknowledgment before the encrypted stream begins. |
| `03EB` | Android sets the deserter scoreboard flag. An absent human ARAL seat receives this status after five minutes of simulation time, under an OpenHalcyon rule. Reconnection clears it. [Seat presence](game-wire.md) |
| `0422` | The native client creates a rectangular or segment control zone (`ActionCreateZoneOfControl_Rect`, 24-byte body: `zone_id`, `owner_actor_id`, `min_or_p0_x`, `min_or_p0_z`, `max_or_p1_x`, `max_or_p1_z`; shares vtable `0x2711280` with `0421`, sets `zone+0x78 \|= 4` in constructor `0xc7e1e0`, destroyed by `0423`). Spec `impl_status = confirmed_layout` is `binary_only` (`0422_disasm_canonical` fixture), with 0 captured frames in `protocol_empirical/opcodes_observed.json` (3 ARAL matches and 5v5 Solo replay only observe `0421`/`0423`). `abilities.json` (61 heroes, 277 abilities) exposes no explicit rect dimension variables; Silvernail A (`Stake`: `TRIPWIRE_ACTIVE_RANGE = 8.0`, `TRIPWIRE_MAX_ALLOWED = 3.0`, `TRIPWIRE_ARM_TIME = 0.4`, `Silvernail_GroundedBolt` in `cff0_complete_index.json`) is the closest endpoint-pair ability, alongside barrier/wall actors (`VainCrystal_LightWall` unit `356`, `TestingGrounds_5v5_MidLaneBarriers_*`). `maps/level_layouts.json` (25 maps) encodes arbitrary 2D `polygon_xz` polygons rather than rectangles. Disassembly of Tx `0xbe501c` / Rx `0xc7e1e0` and live Silvernail/barrier captures remain required to establish AABB vs segment geometry and collision semantics. [Zone actions](../crates/protocol/src/zone.rs) |
| `042C` | `CampVisibility` emits 248 records across 8 of the 14 Android 147219 Solo replay chunks (`chunks 2, 3, 4, 6, 8, 10, 11, 13`; 0 in chunks `0, 1, 5, 7, 9, 12` and ARAL captures). `protocol_spec/layouts.json` omits `042C` (`spec/catalog.rs` keeps `layout: None`), while [`CampVisibilityBody`](../crates/protocol/src/fixed_body.rs) models the 6-byte body (8-byte VGR frame, zero cipher padding in the 10-byte Blowfish slot): BE `actor: u32`, `index: u8` (`0..=7`, matching `042B`), and `state: u8` (`(populated << 1) \| visible`: `0` `EmptyHidden` [115], `1` `EmptyVisible` [25], `2` `PopulatedHidden` [86], `3` `PopulatedVisible` [22]). All 12 referenced actors (`6580..=6591`, slots `40..=51`) spawn at `5.116s` via `0x03F2` as `MinimapIconActor_SpawnCamp` (`349`, 8 actors) and `MinimapIconActor_BuffCamp` (`350`, 4 actors) with `team = 0, hp = 0/0`. Boss markers `6566`/`6567` (`351`/`352`) emit `0x0478` and zero `0x042C`. Observed transitions (0 no-ops, 0 chunk-snapshot repeats): `None -> 0` (96 at `24.597s`, all 12 camps x `0..=7` after `0x0478` `20.0s` phase `2`); `0 -> 3` (12, `index = 0`) and `0 -> 2` (84, `index = 1..=7`) at `44.590s` after monster `0x03F2`/`0x042B` spawn activation; `3 -> 1` (12 across 6 cleared camps) to `index = 0` (6) and the killer team's index (`1` x4, `2` x2) strictly on final camp monster `0x0430` death (partial kills emit nothing); and `0<->1` (`0->1` x13, `1->0` x19) / `2<->3` (`2->3` x10, `3->2` x2) fog toggles paired with `0x042B` visibility bitmasks. Replays fan out `index 0..=7`; live connections filter per viewer/team index (`index 0` tracks global population, teams track local fog + population). [Solo neutral camps](solo-camps.md) |

The specification assigns placeholder names to the other 18 server-only entries. The inputs, bidirectional entries and reserved slots keep their own direction and layout in the [registry](../crates/protocol/src/spec/catalog.rs).
