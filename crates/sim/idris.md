# Idris B on the hosted lane

Template 273 accepts `World::command(actor, Command::Learn(AbilitySlot::B))` and a subsequent `Command::Cast` with `CastTarget::Point` in world X/Z coordinates. The cast becomes authoritative on release. At that tick the simulation debits stamina, starts cooldown and emits one `Event::Cast`. The normal combat path emits damage and kill events for enemies actually hit.

## Evidence

The local 4.13 rules entry `patch_413_ability_deltas.changes[hero=Idris].B_weapon_ratio` is **0.8**. The content-backed world reads that field directly. The same entry assigns its cooldown rank table to **A**. The local Idris unit has zero energy and zero energy growth. Android strings include `Ability02_ThrowChakram`, `Effect_Idris_B_Chakram`, `Buff_Idris_B_ChakramImpact`, `Buff_Idris_DontCatch`, `Buff_Idris_Stamina` and `Buff_Idris_StaminaPaused`. These support a thrown projectile with an impact, catch behavior and separate stamina. The nearby “Damage on Passthrough” string is supporting context. The archive contains four accepted point casts with selector 3 attributed to Idris; the observed aim distances were approximately 2.58–7.32 world units. It establishes neither the maximum range nor the exact collision and damage formulas.

## Hosted rules

* A cast can target a finite, walkable point in clear line of sight up to 8.5 units away. A four-tick windup can be interrupted. Range and line of sight are rechecked at release. A point request supplies no hidden target identity. Bot Idris aims at a visible enemy hero.
* The projectile advances 18 units per second to the selected point and then tracks the caster's current position on its return. Each leg can hit each living enemy hero or lane minion once within 0.75 units of its swept segment. Terrain blocks travel and damage across a wall. Owner death ends the flight, so respawn cannot recall a projectile from the previous life.
* Each hit uses the observed coefficient times the caster's current total weapon power, sampled at release, with the hosted armor mitigation. Applying that coefficient on both passes, using total weapon power, and omitting independent base or crystal contributions are local choices. The hosted damage model currently has no additional rank scaling for B.
* The local stamina pool starts at 100, costs 10 per cast and regenerates 4 per second up to capacity. Its eight-second cooldown uses the 20 Hz simulation clock. Mana remains at its data-defined value. Raw ability-block floats lack validated row semantics for the full B rank progression. The stamina values, cooldown, projectile parameters and one-hit-per-leg policy are validation rules.

Run `cargo test --locked -p halcyon-sim --lib chakram::tests` to exercise the authenticated command boundary exposed to the room, both flight legs, interruption, resource and cooldown gates, terrain and the owner's lifetime.
