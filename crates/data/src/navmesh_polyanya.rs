use crate::{NavMesh, NavMeshError};
use std::collections::BTreeMap;

/// Spatial bucket size (in world units) for deterministic boundary vertex welding.
const WELD_CELL_SIZE: f32 = 0.01;
/// Maximum horizontal squared distance (`0.1 mm`) between coincident Detour tile vertices.
const WELD_DIST_SQ: f32 = 1e-8;
/// Maximum vertical difference allowed when welding coincident `(x, z)` vertices.
const WELD_MAX_Y_DIFF: f32 = 0.5;
/// Minimum absolute 2D signed double-area required for a non-degenerate polygon.
const MIN_DOUBLE_AREA: f64 = 1e-6;

struct VertexWelder {
    buckets: BTreeMap<(i32, i32), Vec<u32>>,
    coords: Vec<[f32; 2]>,
    heights: Vec<f32>,
    polygon_refs: Vec<Vec<u32>>,
}

impl VertexWelder {
    fn new() -> Self {
        Self {
            buckets: BTreeMap::new(),
            coords: Vec::new(),
            heights: Vec::new(),
            polygon_refs: Vec::new(),
        }
    }

    fn cell(x: f32, z: f32) -> (i32, i32) {
        (
            (x / WELD_CELL_SIZE).floor() as i32,
            (z / WELD_CELL_SIZE).floor() as i32,
        )
    }

    fn intern(&mut self, pos: [f32; 3]) -> u32 {
        let [x, y, z] = pos;
        let (cx, cz) = Self::cell(x, z);
        for dx in -1..=1 {
            for dz in -1..=1 {
                if let Some(candidates) = self
                    .buckets
                    .get(&(cx.saturating_add(dx), cz.saturating_add(dz)))
                {
                    for &idx in candidates {
                        let [ex, ez] = self.coords[idx as usize];
                        let dist_sq = (ex - x) * (ex - x) + (ez - z) * (ez - z);
                        if dist_sq <= WELD_DIST_SQ
                            && (self.heights[idx as usize] - y).abs() <= WELD_MAX_Y_DIFF
                        {
                            return idx;
                        }
                    }
                }
            }
        }
        let idx = self.coords.len() as u32;
        self.coords.push([x, z]);
        self.heights.push(y);
        self.polygon_refs.push(Vec::new());
        self.buckets.entry((cx, cz)).or_default().push(idx);
        idx
    }
}

fn signed_double_area_xz(vertices: &[[f32; 3]]) -> f64 {
    let mut sum = 0.0_f64;
    for i in 0..vertices.len() {
        let a = vertices[i];
        let b = vertices[(i + 1) % vertices.len()];
        sum += f64::from(a[0]) * f64::from(b[2]) - f64::from(b[0]) * f64::from(a[2]);
    }
    sum
}

/// Converts a parsed Detour `NavMesh` into a deterministic, baked `polyanya::Mesh`.
pub fn build_polyanya_mesh(navmesh: &NavMesh) -> Result<polyanya::Mesh, NavMeshError> {
    let mut welder = VertexWelder::new();
    let mut raw_polygons: Vec<Vec<u32>> = Vec::new();
    let mut scratch_positions: Vec<[f32; 3]> = Vec::new();

    for tile in navmesh.tiles.values() {
        for poly in &tile.polygons {
            scratch_positions.clear();
            for &v_idx in &poly.vertices[..poly.count] {
                scratch_positions.push(tile.vertices[usize::from(v_idx)]);
            }
            let double_area = signed_double_area_xz(&scratch_positions);
            if double_area.abs() <= MIN_DOUBLE_AREA {
                continue;
            }
            if double_area < 0.0 {
                // Detour ground polygons are typically CW in (x, z); polyanya requires CCW.
                scratch_positions.reverse();
            }

            let mut indices = Vec::with_capacity(scratch_positions.len());
            for &pos in &scratch_positions {
                let idx = welder.intern(pos);
                if indices.last().copied() != Some(idx) {
                    indices.push(idx);
                }
            }
            if indices.len() >= 2 && indices.first() == indices.last() {
                indices.pop();
            }
            if indices.len() < 3 {
                continue;
            }
            let poly_id = raw_polygons.len() as u32;
            for &idx in &indices {
                welder.polygon_refs[idx as usize].push(poly_id);
            }
            raw_polygons.push(indices);
        }
    }

    if raw_polygons.is_empty() || welder.coords.is_empty() {
        return Err(NavMeshError("empty polyanya mesh"));
    }

    let mut edge_counts: BTreeMap<(u32, u32), u8> = BTreeMap::new();
    for poly_verts in &raw_polygons {
        for i in 0..poly_verts.len() {
            let a = poly_verts[i];
            let b = poly_verts[(i + 1) % poly_verts.len()];
            let key = if a <= b { (a, b) } else { (b, a) };
            let entry = edge_counts.entry(key).or_insert(0);
            *entry = entry.saturating_add(1);
        }
    }

    let polyanya_vertices: Vec<polyanya::Vertex> = welder
        .coords
        .into_iter()
        .zip(welder.polygon_refs)
        .map(|([x, z], polys)| {
            let mut coords = polyanya::Layer::default().offset;
            coords.x = x;
            coords.y = z;
            polyanya::Vertex::new(coords, polys)
        })
        .collect();

    let polyanya_polygons: Vec<polyanya::Polygon> = raw_polygons
        .into_iter()
        .map(|verts| {
            let shared_edges = (0..verts.len())
                .filter(|&i| {
                    let a = verts[i];
                    let b = verts[(i + 1) % verts.len()];
                    let key = if a <= b { (a, b) } else { (b, a) };
                    edge_counts.get(&key).copied().unwrap_or(0) > 1
                })
                .count();
            polyanya::Polygon::new(verts, shared_edges <= 1)
        })
        .collect();

    let mut layer = polyanya::Layer::new(polyanya_vertices, polyanya_polygons)
        .map_err(|_| NavMeshError("failed to construct polyanya layer"))?;
    layer
        .set_heights(welder.heights)
        .map_err(|_| NavMeshError("mismatched polyanya vertex heights"))?;

    let mut mesh = polyanya::Mesh {
        layers: vec![layer],
        search_delta: 0.01,
        search_steps: 2,
    };
    mesh.reorder_neighbors_ccw_and_fix_corners();
    mesh.bake();
    Ok(mesh)
}

/// Computes an optimal any-angle path on `polyanya` after projecting endpoints onto Detour ground.
pub fn polyanya_route(
    detour: &NavMesh,
    polyanya: &polyanya::Mesh,
    start: [f32; 2],
    destination: [f32; 2],
    radius: f32,
) -> Option<Vec<[f32; 2]>> {
    if ![start[0], start[1], destination[0], destination[1], radius]
        .into_iter()
        .all(f32::is_finite)
        || radius < 0.0
    {
        return None;
    }

    let (from, start_snapped) = if detour.height_at(start[0], start[1]).is_some() {
        (start, false)
    } else {
        let [x, _, z] = detour.nearest_ground(start, radius)?;
        ([x, z], true)
    };

    let to = if detour.height_at(destination[0], destination[1]).is_some() {
        destination
    } else {
        let [x, _, z] = detour.nearest_ground(destination, radius)?;
        [x, z]
    };

    let mut from_coords = polyanya::Layer::default().offset;
    from_coords.x = from[0];
    from_coords.y = from[1];
    let mut to_coords = polyanya::Layer::default().offset;
    to_coords.x = to[0];
    to_coords.y = to[1];

    let path = polyanya.path(
        polyanya::Coords::on_mesh(from_coords),
        polyanya::Coords::on_mesh(to_coords),
    )?;
    let mut route = Vec::with_capacity(path.path.len() + usize::from(start_snapped));
    if start_snapped && from != start && from != to {
        route.push(from);
    }
    for step in path.path {
        let pt = [step.x, step.y];
        if route.last().copied() != Some(pt) {
            route.push(pt);
        }
    }
    if route.is_empty() {
        route.push(to);
    }
    Some(route)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NativeMaps;
    use sha2::{Digest, Sha256};
    use std::sync::Arc;

    fn synthetic_navmesh(
        verts: &[[f32; 3]],
        polys: &[([u16; 4], [u16; 4])],
        bounds_max: [f32; 2],
    ) -> NavMesh {
        let mut blob = vec![0u8; 100];
        let put = |b: &mut [u8], o: usize, v: u32| b[o..o + 4].copy_from_slice(&v.to_le_bytes());
        for (off, val) in [
            (0, 0x444e_4156u32),
            (4, 7),
            (24, polys.len() as u32),
            (28, verts.len() as u32),
            (36, polys.len() as u32),
            (44, (polys.len() * 2) as u32),
            (56, polys.len() as u32),
            (84, bounds_max[0].to_bits()),
            (88, 1.0f32.to_bits()),
            (92, bounds_max[1].to_bits()),
        ] {
            put(&mut blob, off, val);
        }
        for v in verts {
            for f in v {
                blob.extend(f.to_le_bytes());
            }
        }
        for (vs, ns) in polys {
            let mut row = [0u8; 32];
            for (i, v) in vs.iter().enumerate() {
                row[4 + i * 2..6 + i * 2].copy_from_slice(&v.to_le_bytes());
            }
            for (i, n) in ns.iter().enumerate() {
                row[16 + i * 2..18 + i * 2].copy_from_slice(&n.to_le_bytes());
            }
            row[28] = 1;
            row[30] = 4;
            blob.extend(row);
        }
        for i in 0..polys.len() as u32 {
            let mut d = [0u8; 12];
            d[4..8].copy_from_slice(&(i * 2).to_le_bytes());
            d[9] = 2;
            blob.extend(d);
        }
        for _ in 0..polys.len() {
            blob.extend([0, 1, 2, 0, 0, 2, 3, 0]);
        }

        let mut file: Vec<u8> = [
            1u32,
            0,
            0,
            0,
            bounds_max[0].to_bits(),
            bounds_max[1].to_bits(),
            polys.len() as u32,
            16,
        ]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
        file.extend(1u32.to_le_bytes());
        file.extend((blob.len() as u32).to_le_bytes());
        file.extend(blob);
        NavMesh::parse(&file).unwrap()
    }

    #[test]
    fn polyanya_builds_from_l_shaped_detour_mesh_and_finds_any_angle_corner() {
        // L-shape made of 3 CW convex quads in one tile (bounds [0..4, 0..4]):
        // 0:(0,0), 1:(2,0), 2:(4,0), 3:(0,2), 4:(2,2), 5:(4,2), 6:(0,4), 7:(2,4)
        let verts = [
            [0.0f32, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [4.0, 0.0, 0.0],
            [0.0, 0.0, 2.0],
            [2.0, 0.0, 2.0],
            [4.0, 0.0, 2.0],
            [0.0, 0.0, 4.0],
            [2.0, 0.0, 4.0],
        ];
        // P0: [0, 3, 4, 1], neighbors [0, P2=3, P1=2, 0]
        // P1: [1, 4, 5, 2], neighbors [P0=1, 0, 0, 0]
        // P2: [3, 6, 7, 4], neighbors [0, 0, 0, P0=1]
        let nav = synthetic_navmesh(
            &verts,
            &[
                ([0, 3, 4, 1], [0, 3, 2, 0]),
                ([1, 4, 5, 2], [1, 0, 0, 0]),
                ([3, 6, 7, 4], [0, 0, 0, 1]),
            ],
            [4.0, 4.0],
        );
        let poly_a = Arc::clone(nav.polyanya().unwrap());
        let poly_b = Arc::clone(nav.polyanya().unwrap());
        assert!(Arc::ptr_eq(&poly_a, &poly_b));

        // Path around the inner reflex corner (2, 2) from (3.5, 1.5) to (1.5, 3.5).
        // A straight line passes through (2.5, 2.5) outside the L-shape, so the
        // optimal any-angle path turns exactly at vertex (2.0, 2.0).
        let route = nav.route([3.5, 1.5], [1.5, 3.5], 1.0).unwrap();
        assert_eq!(route.len(), 2);
        assert!((route[0][0] - 2.0).abs() < 1e-4 && (route[0][1] - 2.0).abs() < 1e-4);
        assert!((route[1][0] - 1.5).abs() < 1e-4 && (route[1][1] - 3.5).abs() < 1e-4);
        assert_eq!(route, nav.route([3.5, 1.5], [1.5, 3.5], 1.0).unwrap());

        // Off-mesh endpoint within projection radius snaps onto walkable ground.
        let projected = nav.polyanya_route([3.5, 1.5], [2.5, 2.5], 1.0).unwrap();
        let last = *projected.last().unwrap();
        assert!(nav.height_at(last[0], last[1]).is_some());
    }

    #[test]
    fn disconnected_islands_return_none_from_polyanya_and_match_routesearch_fallback() {
        let verts = [
            [0.0f32, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [0.0, 0.0, 2.0],
            [2.0, 0.0, 2.0],
            [10.0, 0.0, 0.0],
            [12.0, 0.0, 0.0],
            [10.0, 0.0, 2.0],
            [12.0, 0.0, 2.0],
        ];
        let nav = synthetic_navmesh(
            &verts,
            &[([0, 2, 3, 1], [0; 4]), ([4, 6, 7, 5], [0; 4])],
            [12.0, 2.0],
        );
        let mut search = crate::RouteSearch::default();
        assert!(nav.polyanya().is_ok());
        assert!(nav.polyanya_route([1.0, 1.0], [11.0, 1.0], 1.0).is_none());
        assert_eq!(
            nav.route_with([1.0, 1.0], [11.0, 1.0], 1.0, &mut search),
            nav.route_with_graph([1.0, 1.0], [11.0, 1.0], 1.0, &mut search)
        );
        assert_eq!(
            nav.route_with([0.5, 0.5], [1.5, 1.5], 1.0, &mut search),
            Some(vec![[1.5, 1.5]])
        );
    }

    #[test]
    #[ignore = "requires the operator's local server-data supplement"]
    fn polyanya_converts_and_routes_f002_a001_f003_deterministically() {
        let dir = std::path::PathBuf::from(std::env::var_os("HALCYON_DATA_EXTRA").unwrap());
        let maps = NativeMaps::load(&dir, &mut Sha256::new()).unwrap();

        let all_paths: Vec<_> = maps.mesh_paths().collect();
        assert!(
            all_paths.len() >= 16,
            "unexpected navmesh count: {}",
            all_paths.len()
        );
        for path in &all_paths {
            let nav = maps.mesh(path).unwrap().unwrap();
            let p1 = maps.polyanya_mesh(path).unwrap().unwrap();
            let p2 = nav.polyanya().unwrap();
            assert!(Arc::ptr_eq(&p1, p2), "{path}");
        }

        let cases = [
            (
                "build://Environment/F002/F002_navMesh.navmesh",
                [[-90.0f32, 0.0], [90.0, 0.0]],
                [-23.32, 61.1],
            ),
            (
                "build://Environment/A001/A001_nav.navmesh",
                [[-78.18, 0.88], [79.66, -0.91]],
                [-26.02, 31.2],
            ),
            (
                "build://Environment/A001/navMesh_aramWSpawn.navmesh",
                [[-78.18, 0.88], [79.66, -0.91]],
                [0.0, 1.0],
            ),
            (
                "build://Environment/F003/navMeshes/ARAL_5v5_W_Platforms_andMidJungle.navmesh",
                [[-90.0, 0.0], [90.0, 0.0]],
                [0.0, 8.0],
            ),
        ];

        for (path, [start, end], mid) in cases {
            let nav = maps.mesh(path).unwrap().unwrap();
            let p1 = maps.polyanya_mesh(path).unwrap().unwrap();
            let p2 = nav.polyanya().unwrap();
            assert!(Arc::ptr_eq(&p1, p2), "{path}");

            for (a, b) in [(start, end), (end, start), (start, mid), (mid, end)] {
                let r1 = nav.polyanya_route(a, b, 8.0).unwrap_or_else(|| {
                    panic!("expected polyanya route on {path} from {a:?} to {b:?}")
                });
                let r2 = nav.polyanya_route(a, b, 8.0).unwrap();
                assert_eq!(r1, r2, "non-deterministic route on {path}");
                assert!(!r1.is_empty());
                for pt in &r1 {
                    assert!(
                        nav.height_at(pt[0], pt[1]).is_some(),
                        "{path}: waypoint {pt:?} off ground"
                    );
                }
            }
        }
    }
}
