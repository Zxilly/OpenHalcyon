# Native map data and ground queries

The `navmesh_query` example reads an operator-supplied 32-bit Detour mesh with the single-layer wrapper used by the local data supplement. It reports the tile count and checks each X/Z pair against ground polygons. Queries on polygon boundaries are accepted. Disabled polygons and off-mesh links are excluded. A successful query reports ground Y, using detail triangles where available.

```powershell
cargo run -p halcyon-data --example navmesh_query -- ../vg/server_data_extra/maps/navmesh/A001__navMesh_aramWSpawn.navmesh -78.18 0.88 -95 0
```

Use the walk mesh for a ground query. The F002 walk mesh has 1,777 tiles and the separate F002 visibility mesh has 195. The optional checks cover all 10 Solo hero pads, 90 Solo flank centerline points and all six ARAL hero pads. The six ARAL pads are covered by the 511-tile `WSpawn` mesh and lie outside the 459-tile `WOSpawn` mesh.

```powershell
$env:HALCYON_DATA_EXTRA = (Resolve-Path ../vg/server_data_extra).Path
cargo test -p halcyon-data imported_solo -- --ignored
cargo test -p halcyon-sim recorded_flank -- --ignored
```

With the map supplement configured, native Solo and ARAL rooms use the ground mesh for hero movement. The planner uses Detour ground-edge adjacency and matched cross-tile portals, projects destinations at most eight map units and follows polygon corridors. Room movement keeps the existing command validation and limits route construction to six searches per tick. Each hero holds one cached route. Walk meshes are shared across rooms and parsed lazily from the frozen, hashed input. The 16 binary inputs account for roughly 3.6 MB before any mesh is parsed.

ARAL takes spawn pads and matching turret/crystal placements from `TestingGrounds_ARAL`. Solo takes matching turret/crystal placements from `SovereignsRiseLevel_Base`, which has no hero pads. Its replay-confirmed pads and armories retain their measured placement. Fountain recovery follows each team's spawn-stage triangles. Separate brush geometry controls concealment. The visibility meshes remain individually queryable. The available extracts do not establish what their polygons mean for line-of-sight rules. Lane minions retain their own route scripts. Camp local offsets and named rules are indexed as map metadata.

A trial check that only rejected off-mesh destinations recorded 4,222 cumulative rejected tactical commands over 28,511 ticks. The existing movement policy recorded zero over 29,177 ticks. Both seed-0 games reached a natural crystal kill. Routing and bounded projection replace the trial rejection check.

```powershell
$env:HALCYON_DELIVERY = (Resolve-Path ../vg/_ana/delivery).Path
cargo test -p halcyon-data loads_supplement -- --ignored
cargo test -p halcyon-sim supplemental_native -- --ignored
```
