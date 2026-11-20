# Solo neutral camps

The fourteen chunks from one Android 147219 Solo replay share a match clock. Their first eighteen ordinary jungle monsters appear at 44.402 seconds. The scene starts at 5.116 seconds, so the server schedules the first generation at 39.3 simulation seconds. This is the first observed declaration, with a replay capture and tick rounding margin.

| Layout family | Camps | Units per camp |
|---|---:|---|
| A, C | 4 | Healing Treant, Small Bear |
| B | 2 | Big Bear, Big Bear |
| D | 2 | Gold Treant |
| Buff A | 2 | Weapon Treant |
| Buff B | 2 | Crystal Treant |

The world positions use the supplement's named `Spawn1` and `Spawn2` offsets, translated and rotated by their linked placements. Unit templates and stats come from the local delivery. Each of the ten observed neutral deaths can be compared against the same-timestamp resource updates. Five templates have observed gold and XP awards consistent with their unit definitions. Weapon Treant had no observed death, so its award uses the same local definition policy. Only the killing hero receives these base amounts. Extra group shares and team buffs require further evidence.

Neutral actors use the ordinary damage and death path and can be attacked by either team. They retaliate in range when damaged and stay at their spawn positions under the current server policy. The recorded neutral units can change position. Through the last observed frame at 134.340 seconds, the recording has zero neutral respawn records and no second generation. Exact pursuit, respawn, dragon, buff and camp-marker gameplay remains unestablished. The two boss locations and fourteen camp markers are separate from the eighteen ordinary actors observed at first generation.

Snapshots use the observed neutral full-state compact shape, with the world position and current/max health and the room's stable actor slot. A recipient sees the corresponding snapshot when the room can disclose that actor. The current compact profile uses a project-default facing, and the server does not synthesize any unresolved neutral attack animation variants.

```powershell
$env:HALCYON_DELIVERY = (Resolve-Path ../vg/_ana/delivery).Path
$env:HALCYON_DATA_EXTRA = (Resolve-Path ../vg/server_data_extra).Path
cargo test -p halcyon-sim solo_camps -- --ignored
cargo test -p halcyon-game solo_camp_snapshot -- --ignored
cargo run --release -p halcyon-sim --example solo_sides -- $env:HALCYON_DELIVERY --native-map-diagnostics $env:HALCYON_DATA_EXTRA
```
