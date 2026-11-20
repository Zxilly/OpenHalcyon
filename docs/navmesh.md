# Offline ground queries

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

These are point queries. Route connectivity, clearance and line of sight require separate checks. A seed-0 Solo experiment with the real local data reached a natural crystal kill both with and without a trial destination check. The trial recorded 4,222 cumulative rejected tactical commands over 28,511 ticks. The existing movement policy recorded zero over 29,177 ticks. The importer is therefore available for offline inspection and the server uses its existing movement policy.
