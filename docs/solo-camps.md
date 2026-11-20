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

Eight of the fourteen Solo replay chunks record 248 `0x042C` (`CampVisibility`) frames across twelve camp markers (`6580..=6591`, actor slots `40..=51`, declared at 5.116 seconds with `team = 0` and zero health): eight `MinimapIconActor_SpawnCamp` (`349`) and four `MinimapIconActor_BuffCamp` (`350`) actors. The two boss markers (`6566` Blackclaw `351` and `6567` Ghostwing `352`, slots `38..=39`) emit `0x0478` (`0.0s`, phase `1`) at scene setup and zero `0x042C` frames; the remaining two markers have no `0x042C` records. Each 6-byte `0x042C` body carries big-endian `actor`, viewer `index` (`0..=7`, matching `0x042B`), and `state` encoding `(populated << 1) | visible` across `0..=3`:

- Countdown initialization (96 frames at 24.597s): all 12 camps emit `None -> 0` (`EmptyHidden`) across `index 0..=7` immediately after `0x0478` (`20.0s`, phase `2`).
- First spawn activation (96 frames at 44.590s): after neutral monsters appear (`0x03F2` at 44.402s) and activate (`0x042B`), each camp transitions `0 -> 3` (`PopulatedVisible`) on global `index = 0` and `0 -> 2` (`PopulatedHidden`) on `index = 1..=7`.
- Camp clear (12 frames across 6 camps): when the last living monster in a camp dies (`0x0430`), `0x042C` emits `3 -> 1` (`EmptyVisible`) to `index = 0` and the clearing team's index (`1` four times, `2` twice). Partial kills in two-monster camps emit no `0x042C`.
- Fog-of-war toggles (44 frames on `index = 1, 2`): `0 <-> 1` (`0 -> 1` x13, `1 -> 0` x19) and `2 <-> 3` (`2 -> 3` x10, `3 -> 2` x2) co-occur with `0x042B` visibility bitmasks; unvisited enemies retain `2` (`PopulatedHidden`) until observing the cleared camp.

Android 147219 receiver disassembly corroborates this encoding: `0x82d1e0 -> 0x82aee0` extracts only the low two state bits via `bfxil w3, w21, #0, #2`, and `vt[3]` (`0x8844dc`) filters strictly on `index == GetLocalTeamId()` (`0xbedf10`, `[local_player + 0xc2] & 0xf`) before writing `[minimap_entry + 0x6c]` across the 200-entry minimap icon pool at `[0x2b0a360]` via `0x820a7c`.

Snapshots use the observed neutral full-state compact shape, with the world position and current/max health and the room's stable actor slot. A recipient sees the corresponding snapshot when the room can disclose that actor. The current compact profile uses a project-default facing, and the server does not synthesize any unresolved neutral attack animation variants.

```powershell
$env:HALCYON_DELIVERY = (Resolve-Path ../vg/_ana/delivery).Path
$env:HALCYON_DATA_EXTRA = (Resolve-Path ../vg/server_data_extra).Path
cargo test -p halcyon-sim solo_camps -- --ignored
cargo test -p halcyon-game solo_camp_snapshot -- --ignored
cargo run --release -p halcyon-sim --example solo_sides -- $env:HALCYON_DELIVERY --native-map-diagnostics $env:HALCYON_DATA_EXTRA
```
