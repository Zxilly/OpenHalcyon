# Native ability requests, grants and releases

## Native learning

In 24 captured 0x0436 requests, the first nearby 0x043A from the same
actor followed after 22–124 ms, with a median of 82 ms. The table gives
the grant operand and the number of observations for each request index.

| Hero template | Index 0 (A) | Index 1 (B) | Index 2 (C) |
| --- | --- | --- | --- |
| 254 Hero009 | 0 (3) | 1 (2) | No sample |
| 439 Yates | 0 (2) | 2 (2) | 3 (1) |
| 273 Idris | 0 (2) | 3 (4) | 4 (1) |
| 409 Lorelai | 0 (2) | 1 (4) | 2 (1) |

A valid request on a registered native seat spends one skill point and
increments the requested rank before the server emits the original input echo
and the 0x043A grant. Normal level and rank limits apply. The first successful
request per simulation tick is the only one that spends a point. A rejected
request consumes no point and a later eligible request can succeed.

Learning can persist a rank even where the active spell is unimplemented.
The native cast handlers still cover Idris B and Lorelai A/B/C. Bots spend
points only on abilities whose combat behavior the simulator implements.
The recorded timing describes the captures, without a server-side delay
requirement.

## Idris B

The Android catalog identifies Idris as template 273. In the played ARAL
archive for that seat, four 0x0436 requests with index 1 each received a
0x0436 echo and 0x043A grant with actor ID and operand 3. The stock room
can select Idris when its configured game data includes that name and
template. The default validation roster uses Ringo.

Four 0x0412 requests from the same seat used selector 3. Every request
contained a three-float vector and one zero padding byte. The full vector
and selector reappeared in a 0x0416 response from the same actor after
39–99 ms. The signed-zero middle component is preserved.

| Vector X, Y, Z | Nearest reported actor X, Z | Approximate planar distance |
| --- | --- | ---: |
| (-40.8939, 0, 1.4944) | (-38.417, 0.785) | 2.58 |
| (-58.2402, -0, 14.9417) | (-65.077, 12.327) | 7.32 |
| (-44.1787, -0, 3.5794) | (-46.192, 8.335) | 5.16 |
| (-34.1510, 0, -0.7185) | (-36.317, 5.817) | 6.88 |

The positions cluster around the caster in world space. The vectors
themselves have lengths 34–60 and do not represent unit directions.
OpenHalcyon uses X/Z as the authored simulation point for Idris B. The
server models the measured ARAL lane within explicit bounds. Tests use
points inside those bounds and keep the historical vectors as codec fixtures.

Learning index 1 on an authenticated, registered native control seat
spends an available skill point before confirming. Repeated learning in
the same simulation tick receives no additional grant. Point casting uses
the simulator's target, range, resource and cooldown checks. Only an
authoritative release emits 0x0416 to the originating connection. All 15 observed accepted point-cast inputs first received a byte-identical 0x0412 on their originating connection. The room confirms that input at acceptance, including when a later interruption cancels the windup.
Canceled windups and rejected inputs do not emit it. The capture does not establish the exact native stamina, collision or
damage rules.

## Lorelai

Template 409 used learning indices 0, 1 and 2. In seven successful grants the
0x043A operand equaled the requested index (two A, four B, one C). Six A and
five B point requests had matching 0x0416 releases after 35–142 ms. Their
distances from the latest reported caster position ranged from 5.27 to 8.00
and 4.71 to 10.00 units respectively. The C request named another hero on
the caster's team. A 0x0415 carried caster, identical target and selector 2
55 ms later. The native input handler supports that allied-hero form.

## Witnessed grants and releases

Across seven complete ARAL recordings, the actor in the client's 0x0471
identifies its controlled seat. Initial full hero declarations supply the
other seats and their teams. Counts below include repeated actions.

| Server message | Controlled hero | Other allied hero | Opposing hero | Outside six hero IDs |
| --- | ---: | ---: | ---: | ---: |
| 0x0415 targeted release | 21 | 1,607 | 2,135 | 5,343 |
| 0x0416 point release | 15 | 215 | 654 | 2 |
| 0x043A learn grant | 24 | 104 | 189 | 0 |

An allied Idris accounts for 34 witnessed selector-3 point releases and five
operand-3 grants. Lorelai appears only on the controlled seat. The room
applies the shared release format to mapped skills from successful human and
bot simulation events. A team's current world view must contain the caster
and, for a targeted release, the target. A live authorized observer receives
the full view. The requesting control generation receives one confirmation
with the original point bytes. A replacement socket for that seat receives
no confirmation of the former connection's input. Skill-point expenditure
produces the public grant. Only the requesting seat receives its input echo.

## Ability charges

The independent H5810 replay audit observed 109 `0x0429` actions with the same
owner, ability slot 0, unsigned operand 4 and mode 1. The protocol crate reads
and writes its 14-byte recorded body and retains the trailing byte when decoding
a recording. The slot is an ability index. The neighboring `0x0428` timer action
uses a separate lookup key and a floating-point operand. The local Solo and ARAL
corpora contain no `0x0429` samples. The hosted abilities currently have no
verified charge-change trigger. [Replay field evidence](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/COOLDOWN_EVENT_FIELDS_2026-10-14.md)

## Lorelai pool registration and lifetime

Across seven ARAL streams, all six observed Lorelai A point releases created a
compact `0x03F2` actor of template 410 at the exact release X/Z. The owner at
body offset 112 matched the caster. The six slots at offset 116 were 34, 29,
29, 34, 25 and 33. No other compact or full declaration reused any of those
slots during that pool's lifetime. Each pool received two `0x043E` applications
from the caster. Definition 949 arrived at release with duration bits
`0x3a66` (about 0.8 seconds). Definition 950 arrived 0.70–0.80 seconds later
with duration bits `0x4800` (8 seconds). Each pool received `0x0430` with
source `0xffffffff` at 9.02–9.28 seconds after release, followed by both
`0x0431` and `0x040B` at 12.82–13.05 seconds. The recordings show actor
destruction without a corresponding explicit buff removal.

All six belonged to one caster, observed from that player's connection in one
match. Each declaration had eight per-view `0x042B` records with different
mask operands. The capture establishes the caster's slots and timestamps.
The other viewers' streams, registration on first sight, and the binding of
the two opaque birth words at offsets 88 and 92 remain unverified. The hosted
simulation applies Fish Food locally. Native registration and buff emission
remain pending evidence for those fields and recipients.

Five observed Lorelai B releases each preceded a template 411 at the cast X/Z
by 0.9–1.1 seconds. Six template 411 actors appeared in total, including one
without a matching B request. All six died 8.38–8.47 seconds after birth.
The single observed C release targeted another living ally. In the caster's
stream the targeted `0x0415` preceded `0x043E` definition 954 in the same
timestamp group. The application repeated the release's source and target,
carried a distinct instance ID, duration bits `0x4400` (four seconds), and six
zero padding bytes. The next record increased target stat 4 by 500.
No target impact or death occurred during the following 4.1 seconds.

For ARAL the host emits this application on the authenticated caster's
issuing connection after the successful release, using a fresh world
instance ID. The simulation reports it when a base four-second veil takes
effect on another ally with no remaining barrier. The archive supplies no
comparison for refresh, empowered veil, early shield depletion, self-cast or
the target's own view. No explicit duration update, cancel or timeout in the
seven matches referenced any instance of definition 954.

Across all nine TCP captures the lossless codecs reproduced 44,346 short and
17,018 extended applications, 385 duration updates, 1,398 explicit cancels
and 18 timeouts. Matching by target and instance also yielded zero mutations
for definitions 820 and 949 through 953. Across two matches, all 38 witnessed
Idris B releases coincided with definition 820 on Idris. It also appeared
25 times outside those releases, leaving its general trigger unresolved.

All 38 Chakram applications had source equal to target. The four caster-view
and 34 teammate-view releases shared the same stream timestamp and order:
`0416`, `042E`, `043E` definition 61, `043E` definition 820. All 63
instances of 820 had unique IDs within their own match. The 38 matched
applications carried duration bits `0x4066` (2.19921875 seconds) and six
zero padding bytes. No paired duration update, cancellation or timeout for
these instances was observed.

In ARAL the host attaches 820 to a successfully released Chakram using a
fresh world instance ID. It follows the release on the issuing generation's
connection and on the world streams of teammates who have the actor in view
and registered. The host records the self-application from those streams.
Opposing seats, observers and Solo lack an original recipient sample for
this action. Early interruption consumes no instance, and late entry does
not recreate expired applications. The simulation gains no extra combat
effect from the visual application. Definition 61's gameplay role remains
unverified.

## Cooldown keys and initial state

On the first scene, the host sends the observed `048A` cooldown definitions immediately after each supported hero's `03F3` declaration. The records include emotes, withdrawal, hero abilities and additional action keys, in the order recorded for that template. Recognized keys use FNV-1a over the case-sensitive Android definition name. Unknown keys retain their exact integer value. Cooldown initialization does not grant ranks, activate a talent or start a simulated cooldown.

| Original source | Hero declarations | Distinct templates | Original rows | Rows after deduplication by template |
|---|---:|---:|---:|---:|
| Seven bidirectional ARAL matches | 42 | 30 | 322 | 228 |
| First chunk of one Solo match | 10 | 9 | 79 | 72 |

Every hero's records immediately followed its declaration at the same recorded timestamp. Repeated templates agreed byte for byte within each mode. Seven templates also appeared in both modes with identical initializations. Subsequent Solo chunks can include items and changed timers. The host sends the template's initialization only during the initial scene, before live grants and item use. The replay writer accepts only the observed combinations of key, initial value, configuration value and flags for a hero actor.

The regression reconstructs the arrays from the production encoder. In ascending order by template, it hashes the two-byte big-endian template ID, one byte of row count, and bytes 4 through 21 from each consecutive same-actor `048A`. The SHA-256 values from the original recordings are `7f7c078337111ee4803bac3b030dcd18884f1443353db2e7db08db48c1a239bd` (ARAL) and `ca7626305d8c7a483cdb0acf6bd448eb5398fa581e300fbbf12ea592953dd05b` (Solo). [Template profiles](../crates/game/src/native_cooldown/profiles.rs) · [Production encoder and regression](../crates/game/src/native_cooldown.rs)

The same seven matches contained 2,000 modify-cooldown records. Across six Lorelai A, five B and one C releases, the caster received no such records for her actor throughout either match. No Idris B key occurred in the 44 changes on his own actor or the 127 on the remote Idris. The only changes coinciding with any of the 38 B releases within 120 ms were two relative zero operands on Idris **A and C**. No same-owner sync occurred within 100 ms of any of the 50 releases. The static receiver selects a relative operation for mode zero and a direct operation for nonzero mode. The charge opcode uses an unsigned slot index and operand. None of these observations supplies a B/ABC start or expiry packet, a late-join nonzero countdown formula, or a teammate cooldown subscription.

The simulation still writes an independent tick deadline on successful release. It charges the effective cooldown after equipped reduction and rejects a new cast until that tick. Interrupted or rejected releases leave the deadline unchanged. [Independent cooldown and charge receiver study](https://github.com/TurnaboutHero/vainglory-replay-decoder/blob/f4743be743564e3aadef1566499fe25d9d0f9ffc/vg/docs/COOLDOWN_EVENT_FIELDS_2026-10-14.md)

## Reim B (Chill Winds)

The Android ability text identifies an area burst centered on Reim, with damage and immobilization on nearby enemies and reduced damage to minions. The local ability-definition study independently links B (ability index 1) to the damage and `Buff_Reim_Immobilize` definitions. This index belongs to the ability table. The archived clients supply no Reim input selector.

All twelve `0419` records in the seven ARAL streams came from a single Kinetic perspective watching Reim (template 266). Each effect referenced Reim and carried the FNV-1a key of `Effect_Reim_B_AOE_Range`. Every effect paired, in order, with a distinct preceding Reim `0415` using target `ffffffff` and selector 1. These were all twelve such releases in that stream. Nine pairs shared a receive timestamp and the remaining three were 93–101 ms apart. The nearest Reim position update lay within 1.12 world units of each effect center. Eight effect timestamps also had one or more damage records credited to Reim with impact code 74. Four had none, so a successful hit cannot be required to display the range effect.

With the supplement installed, players through the simulation command API and bots can learn and cast Chill Winds on themselves. At ranks 1–5 its self-centered radius is 4.5, cooldown is 12/11.5/11/10.5/10 seconds, cost is 60/65/70/75/80 and root duration is 1/1/1/1/1.5 seconds. The named damage is 60/100/140/180/220 plus 60% crystal power. Rooted actors can still attack in range or use stationary abilities. Waterwall clears the movement impairment. The six rank-two impacts on lane minions deal half the base damage. Nearby Winter Spire damage independently establishes crystal power of 41.25 and 178.75 at those two purchases, yielding exactly 62.375 and 103.625 respectively.

All twelve effects reached a teammate. Their height has identical bits (3d192e1f). In ten cases the final float agrees within 0.00015 radians with a nearby caster displacement angle atan2(ΔZ, ΔX). The two remaining paths do not yield an equally close heading. The host sends the no-target 0415 to registered, visible teammates and full-field observers. When the simulation has a measured movement heading it also sends 0419 before damage. The source archives establish neither enemy delivery nor the caster's input.

All fourteen definition-654 043E applications in seven sessions come from Reim and share a B effect timestamp and 1.0-second duration. Thirteen have same-source, same-target impact code 74; the remaining target has no damage record. None has a matching explicit removal. The host emits fresh one-second instances for surviving rooted targets, after damage and subject to both actors' registration and visibility. Rank five uses the 1.5-second impairment from the extracted table. Its wire buff form remains unverified.

## Reim Winter Spire and current playable scope

In one of the seven ARAL archives the recording seat witnessed 38 point releases
from Reim (template 266) with selector 0. Of those, 24 had a preceding server
0x0412 within 0.5 seconds containing the identical 13 coordinate/selector bytes.
The seat received five 0x043A grants for this actor with operand 0. The native
position vectors are world points, with planar lengths from 2.62 to 69.74.
Their vertical component includes zero and small terrain heights. The local
simulation uses the observed zero-height form.

Winter Spire is the first ability entry in the Reim asset. Its tooltip names
two damage blocks, whose 32-bit FNV-1a headers match `Damage First Hit` and
`Damage Second Hit`. The first hit deals 60/100/140/180/220 plus 80% crystal
power. The shatter deals 80/140/200/260/320 plus 160%. Both test the current
occupants of a two-unit circle. The asset gives 0.5 seconds to the first hit
and another 1.0 second to shatter. Reim-attributed 0x041E damage in the
recording peaks around 0.4 and 1.4 seconds after the 0x0416 release. The host
uses a two-tick windup, then eight ticks to impact and twenty more to shatter.

For a lane minion with zero shield, subtracting twice the first hit from
the second cancels the crystal power term. Fifteen paired same-minion samples
at ranks 3–5 give a damage multiplier of 0.5; one yields 0.213. These pairs
require a single same-source impact on that minion in each of the 0.30–0.57
and 1.25–1.58 second windows after a release. This supports the ordinary
half-damage rule. The outlier's individual cause is unknown.

With the supplement installed, an authoritative Reim actor can learn and cast
A. Bots use the same range, resource, cooldown and target checks. The host
sends the witnessed 0x0416 and 0x043A forms to registered, visible ARAL peers.
The historical recording contains no originating Reim 0x0412 or 0x0436.
A complete native Reim client interaction still needs that evidence.

| Templates | Authoritative hosted skills | Native observations |
| --- | --- | --- |
| Idris, Lorelai | Idris B; Lorelai A, B, C | All 16 originating casts in the nine TCP archives belong to these four skills. |
| Reim | A and B with the supplement | ARAL witness releases and grants. |
| Ringo, Adagio, Lyra, SAW, Karas, Shin | Ringo A/B/C; Adagio A/B/C; Lyra A/B; SAW B; Karas B; Shin C | Modeled subsets of their asset effects. Numeric weapon and gear effects also apply to basic attacks. |
| Other advertised heroes | Shared movement, basic attacks, items and progression | Hero009 and Yates have confirmed native learn grants. A rank grant alone supplies no cast mechanism. |

The 57-hero advertisement and the static definitions for 61 hero entries are
catalog counts. The separate platform inventory has 155 RPC names and the
Android reader table has 171 entries. Each describes its own interface.
Numeric native talent admission records a template and a shared authorized
level. It cannot distinguish a named talent in a triplet. The authoritative
named LongShot path needs an independently authenticated talent ID and level;
the current native room does not infer it from selector or rarity.

### Supplemental Bot skills

The room selects heroes from the advertised hero list by seed and seat. With the supplemental definitions installed, the normal learn and cast commands drive these Bot skills.

| Hero | Authoritative skill | Other modeled active slots |
| --- | --- | --- |
| Catherine | **A, Merciless Pursuit.** Self-cast resets the basic-attack timer and grants timed movement and melee reach bonuses. The next released basic against a hero, lane minion or neutral inside the window deals bonus crystal damage and stuns once. Overdrive increases the window and stun duration. | B Stormguard and C Blast Tremor, detailed below |
| Yates | **B, Overwhelm.** After charging for 1.5 seconds, Yates damages and stuns enemies within 3.5 units of his position at detonation. Allies, structures and units across a blocking wall are excluded. Overdrive increases the stun to one second. | A Wolf's Maw and C Iron Mandate, detailed below |

Both use the supplied rank costs, cooldowns and crystal ratios. The host uses its one-tick self-cast release, stun interruption rules and shield mitigation. Yates's asset lists an additional movement-strength variable whose timing and units are absent from both B tooltips. The model uses his normal movement during the charge. Native client selectors and dedicated presentation messages still require directional wire evidence.


* **Catherine B, Stormguard.** For four seconds it burns nearby enemy units. A single incoming hit exceeding 7.5% of unbonused maximum health caps at that threshold, reflects the excess across up to three nearby enemies, and subtracts 0.2 seconds from the remaining duration. Base health includes level growth. The host samples the burn once per second, checks the hit after mitigation and before temporary barriers, selects the nearest three eligible targets and mitigates the reflected share as crystal damage. Reflected damage cannot recurse. The tooltip establishes the cap, split and duration loss; these ordering and tie-breaking choices are local.
* **Catherine C, Blast Tremor.** The directional release uses the shipped range, windup, damage, silence, post-cast lock and self-reveal durations. A silenced target may still move or make basic attacks. The description specifies a forward cone; the host uses a 90-degree opening because the supplied fields contain no angle.
* **Yates A, Wolf's Maw.** The flail advances along the aim direction at the shipped speed and stops at the first enemy body it intersects. The first hit deals crystal damage once. Successive ticks pull the victim toward Yates at the supplied pull speed, bounded by the walkable segment. The host uses each target's collision circle and interprets the magnitude of the signed pull offset as the stopping distance in front of Yates. The supplement supplies no flail width.
* **Yates C, Iron Mandate.** The jump channels for one second and can be interrupted before release. Landing uses the chosen ground location, bypasses intervening ground obstacles, damages and slows enemies, and grants nearby allied heroes a barrier. A pending Overwhelm whose delay expires during the jump waits for landing. The barrier uses its named rank amount and 125% crystal-power ratio. The separate 8% health-ratio field has no identified base in either tooltip and is excluded from the barrier calculation.

These four skills share the existing Bot command, energy, cooldown, visibility and damage paths. Their dedicated native presentation and client input bindings still lack independently verified selectors.


### Lyra B, Bright Bulwark

The supplemental ability activates eight simulation ticks after self-cast and maintains a fixed circle of radius 5.5 at her release position for 60 ticks. It deals 80/160/240/320/400 plus 30% crystal power on activation or a subsequent eligible boundary crossing. Nearby enemies remain slowed by 30%, or 50% on overdrive. The slow lingers for 1.5 seconds, or 2.5 seconds on overdrive. The host applies the asset's 2.5-second immunity per enemy to repeated boundary damage. Walk orders remain valid. An opposing Yates cannot leap from inside or across the active wall. Bots use the same learned-rank, energy, cooldown and proximity checks. Lyra C remains outside the authoritative skill set. The separate orbit and zone lifetime and their capture coverage are documented in [ARAL orbit actions](aral-orbit.md).

Mobile Bulwark moves the combat circle with Lyra and extends it using its named duration modifier. In the one hostile witness recording, Lyra equipped selector 1 at level ten. The host accepts that binding only for a matching authenticated home grant. Clearing the selection restores the stationary field for new casts. The talent's separate passive crystal-power variable has numeric unit tag 6 without an independently verified calculation basis, so the damage calculation continues to use equipped crystal power.
### Adagio, Karas and Lyra with supplemental combat data

* Adagio's Gift of Fire resets his basic-attack timer, heals the selected ally immediately and over three seconds, and ignites nearby enemies. Casting on himself also slows them. The host checks the burn radius at release, then follows each affected unit. Agent of Wrath resets the caster's basic timer and applies the named burn bonus only when its wielder attacks a target marked by that Adagio. Reapplication from the same caster refreshes the effect.
* Verse of Judgement uses the supplemental `Ability__Adagio__C` rank table. It channels for 40 ticks, then deals 800/1000/1200 + 100% crystal power within 9 units. The host checks enemy heroes, lane units and neutrals at release. Enemies still burning from that caster's Gift of Fire are stunned for 32/46/60 ticks. The caster fortifies up to 300/550/800 current HP during the channel. Each fortified HP covers two incoming damage after ordinary mitigation and barriers. The localized full description is keyed as `HERO_ABILITY_ADAGIO_FRIENDSHIP_DESC`. The publisher explained fortified health when it introduced this effect. [Update 1.14 mechanics](https://www.vainglorygame.com/vainglorylive/lunar-new-year-update-1-14-year-of-the-monkey/)
* The host charges 140/190/240 energy and starts the 100/80/60-second cooldown when protection begins. Interrupting or cancelling removes protection immediately and prevents the blast. This payment timing is a host policy. The normal simulator command and Bot decision both use a self-cast. A native client request binding for Verse remains unverified.
* Karas's Twister travels towards the selected point at the speed implied by seven units in 0.32 seconds. A wall, hostile Bulwark, root or stun can interrupt movement. Each crossed enemy takes the ranked damage at most once. The host tests swept collision circles for Karas and the target because the supplement gives no separate blade width.
* Lyra's Imperial Sigil tests allied and enemy heroes inside its 3.5-unit circle. The host uses the circle as its sight radius, checks healing and damage once per second, and tracks a twelve-second charge budget. Passive drain is one unit per second. While occupied, four units per second are consumed per affected hero, counting allied heroes only while injured. Those timing and drain-combination choices implement the tooltip and named values. An authoritative self-targeted A reactivates the current sigil without another energy debit or cooldown reset. It converts remaining charge to healing at 30%, grants a decaying movement bonus, and deals the named burst to enemy heroes and half damage to lane minions. Natural depletion ends the field. A standalone additional detonation-heal amount has no independently specified additive relationship with the remaining-charge formula.
