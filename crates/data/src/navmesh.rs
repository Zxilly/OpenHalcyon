use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    ops::Range,
};

#[path = "navmesh_route.rs"]
mod route;
pub use route::RouteSearch;

const NAV_MAGIC: u32 = 0x444e_4156;
const NAV_VERSION: u32 = 7;
const TILE_HEADER_BYTES: usize = 100;
const MAX_VERTICES_PER_POLYGON: usize = 6;
const EDGE_TOLERANCE: f64 = 1e-6;

#[derive(Debug)]
pub struct NavMeshError(&'static str);

impl fmt::Display for NavMeshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for NavMeshError {}

type Result<T> = std::result::Result<T, NavMeshError>;
type Vertex = [f32; 3];
type Triangle = [Vertex; 3];

/// A single-layer 32-bit Detour mesh with ground containment and routing.
/// The graph excludes off-mesh links and dynamic obstacles.
#[derive(Debug)]
pub struct NavMesh {
    origin: Vertex,
    tile_width: f32,
    tile_depth: f32,
    tiles: BTreeMap<(i32, i32), Tile>,
    graph: route::Graph,
}

#[derive(Debug)]
struct Tile {
    climb: f32,
    bounds: [Vertex; 2],
    vertices: Vec<Vertex>,
    polygons: Vec<Polygon>,
    detail: Vec<Triangle>,
}

#[derive(Debug)]
struct Polygon {
    row: u16,
    neighbors: [u16; MAX_VERTICES_PER_POLYGON],
    vertices: [u16; MAX_VERTICES_PER_POLYGON],
    count: usize,
    detail: Range<usize>,
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let (value, remaining) = self
            .0
            .split_at_checked(n)
            .ok_or(NavMeshError("truncated navmesh"))?;
        self.0 = remaining;
        Ok(value)
    }

    fn array(&mut self, count: usize, stride: usize) -> Result<&'a [u8]> {
        self.take(
            count
                .checked_mul(stride)
                .ok_or(NavMeshError("navmesh array overflow"))?,
        )
    }

    fn word(&mut self) -> Result<u32> {
        Ok(u32_at(self.take(4)?, 0))
    }
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn float_at(bytes: &[u8], offset: usize) -> f32 {
    f32::from_bits(u32_at(bytes, offset))
}

fn vertex_at(bytes: &[u8], offset: usize) -> Result<Vertex> {
    let v = [
        float_at(bytes, offset),
        float_at(bytes, offset + 4),
        float_at(bytes, offset + 8),
    ];
    if v.iter().all(|p| p.is_finite()) {
        Ok(v)
    } else {
        Err(NavMeshError("non-finite navmesh vertex"))
    }
}

struct Wrapper {
    count: usize,
    origin: Vertex,
    tile_width: f32,
    tile_depth: f32,
    max_tiles: usize,
    max_polygons: usize,
}

impl Wrapper {
    fn read(file: &mut Reader<'_>) -> Result<Self> {
        let count = file.word()? as usize;
        let origin = vertex_at(file.take(12)?, 0)?;
        let tile_width = f32::from_bits(file.word()?);
        let tile_depth = f32::from_bits(file.word()?);
        let max_tiles = file.word()? as usize;
        let max_polygons = file.word()? as usize;
        if count == 0
            || count > max_tiles
            || count > file.0.len() / (TILE_HEADER_BYTES + 8)
            || !(1..=usize::from(u16::MAX)).contains(&max_polygons)
            || ![tile_width, tile_depth]
                .iter()
                .all(|d| d.is_finite() && *d > 0.)
        {
            return Err(NavMeshError("invalid navmesh header"));
        }
        Ok(Self {
            count,
            origin,
            tile_width,
            tile_depth,
            max_tiles,
            max_polygons,
        })
    }
}

impl NavMesh {
    pub(super) fn check_index(bytes: &[u8], index: &crate::MeshHeader) -> Result<()> {
        let mut file = Reader(bytes);
        let header = Wrapper::read(&mut file)?;
        let _tile_reference = file.word()?;
        let size = file.word()? as usize;
        let first = file.take(size)?;
        let signature = first
            .first_chunk::<8>()
            .ok_or(NavMeshError("missing first tile signature"))?;
        // The extraction tool rounded geometry in the index to four decimals.
        let matches = |a: f32, b: f32| (a - b).abs() <= 0.00006;
        if header.count != index.tile_count
            || header.max_tiles != index.max_tiles
            || header.max_polygons != index.max_polys
            || !matches(header.tile_width, index.tile_width)
            || !matches(header.tile_depth, index.tile_height)
            || !header
                .origin
                .into_iter()
                .zip(index.origin_xyz)
                .all(|(a, b)| matches(a, b))
            || signature[..4] != *index.tile0_magic.as_bytes()
            || u32_at(signature, 4) != index.tile0_version
        {
            return Err(NavMeshError("mesh header differs from its index"));
        }
        Ok(())
    }

    /// Parse the 32-byte game wrapper followed by native (tileRef, size) records.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let mut file = Reader(bytes);
        let Wrapper {
            count,
            origin,
            tile_width,
            tile_depth,
            max_polygons,
            ..
        } = Wrapper::read(&mut file)?;

        let mut mesh = Self {
            origin,
            tile_width,
            tile_depth,
            tiles: BTreeMap::new(),
            graph: route::Graph::default(),
        };
        let mut references = BTreeSet::new();
        for _ in 0..count {
            let reference = file.word()?;
            let size = file.word()? as usize;
            if reference == 0 || !references.insert(reference) {
                return Err(NavMeshError("invalid or repeated navmesh tile reference"));
            }
            let (xy, tile) = Tile::parse(file.take(size)?, max_polygons)?;
            if !mesh.bounds_match_grid(xy, &tile.bounds) || mesh.tiles.insert(xy, tile).is_some() {
                return Err(NavMeshError("invalid or repeated navmesh tile coordinates"));
            }
        }
        if !file.0.is_empty() {
            return Err(NavMeshError("trailing navmesh data"));
        }
        mesh.graph = route::Graph::build(&mesh.tiles);
        Ok(mesh)
    }

    fn bounds_match_grid(&self, (x, z): (i32, i32), bounds: &[Vertex; 2]) -> bool {
        let min_x = f64::from(self.origin[0]) + f64::from(self.tile_width) * f64::from(x);
        let min_z = f64::from(self.origin[2]) + f64::from(self.tile_depth) * f64::from(z);
        [
            (bounds[0][0], min_x),
            (bounds[0][2], min_z),
            (bounds[1][0], min_x + f64::from(self.tile_width)),
            (bounds[1][2], min_z + f64::from(self.tile_depth)),
        ]
        .into_iter()
        .all(|(recorded, expected)| (f64::from(recorded) - expected).abs() <= 0.001)
    }

    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    /// Query the horizontal game coordinates (X/Z) and return the native Y.
    /// Off-mesh connections and empty cells yield no ground.
    pub fn height_at(&self, x: f32, z: f32) -> Option<f32> {
        if !x.is_finite() || !z.is_finite() {
            return None;
        }
        let x = f64::from(x);
        let z = f64::from(z);
        let col = ((x - f64::from(self.origin[0])) / f64::from(self.tile_width)).floor() as i32;
        let row = ((z - f64::from(self.origin[2])) / f64::from(self.tile_depth)).floor() as i32;
        for (dx, dz) in [
            (0, 0),
            (-1, 0),
            (0, -1),
            (-1, -1),
            (1, 0),
            (0, 1),
            (1, 1),
            (1, -1),
            (-1, 1),
        ] {
            let xy = (col.saturating_add(dx), row.saturating_add(dz));
            if let Some(tile) = self.tiles.get(&xy)
                && let Some(height) = tile.height_at(x, z)
            {
                return Some(height);
            }
        }
        None
    }
}

impl Tile {
    fn parse(bytes: &[u8], max_polygons: usize) -> Result<((i32, i32), Self)> {
        let mut body = Reader(bytes);
        let header = body.take(TILE_HEADER_BYTES)?;
        if u32_at(header, 0) != NAV_MAGIC
            || u32_at(header, 4) != NAV_VERSION
            || u32_at(header, 16) != 0
        {
            return Err(NavMeshError("unsupported Detour tile version or layer"));
        }
        let x = u32_at(header, 8) as i32;
        let z = u32_at(header, 12) as i32;
        let bounds = [vertex_at(header, 72)?, vertex_at(header, 84)?];
        let climb = float_at(header, 68);
        if (0..3).any(|axis| bounds[0][axis] > bounds[1][axis]) || !climb.is_finite() || climb < 0.
        {
            return Err(NavMeshError("invalid Detour tile bounds or climb"));
        }

        let count = |offset| u32_at(header, offset) as usize;
        let polygons = count(24);
        let vertices = count(28);
        let detail_count = count(36);
        let off_mesh_count = count(52);
        if polygons == 0
            || polygons > max_polygons
            || vertices > usize::from(u16::MAX)
            || detail_count.checked_add(off_mesh_count) != Some(polygons)
            || count(56) != detail_count
        {
            return Err(NavMeshError("invalid Detour tile array counts"));
        }
        let vertices = body
            .array(vertices, 12)?
            .as_chunks::<12>()
            .0
            .iter()
            .map(|raw| vertex_at(raw, 0))
            .collect::<Result<Vec<_>>>()?;
        let polygon_rows = body.array(polygons, 32)?.as_chunks::<32>().0;
        body.array(count(32), 12)?;
        let details = body.array(detail_count, 12)?.as_chunks::<12>().0;
        let detail_vertices = body
            .array(count(40), 12)?
            .as_chunks::<12>()
            .0
            .iter()
            .map(|raw| vertex_at(raw, 0))
            .collect::<Result<Vec<_>>>()?;
        let detail_triangles = body.array(count(44), 4)?.as_chunks::<4>().0;
        body.array(count(48), 16)?;
        body.array(off_mesh_count, 36)?;
        if !body.0.is_empty() {
            return Err(NavMeshError("unexpected Detour tile section length"));
        }

        let mut tile = Self {
            climb,
            bounds,
            vertices,
            polygons: Vec::with_capacity(detail_count),
            detail: Vec::new(),
        };
        for (i, row) in polygon_rows.iter().enumerate() {
            let count = usize::from(row[30]);
            let kind = row[31] >> 6;
            let ground = i < detail_count;
            if (ground && (kind != 0 || !(3..=MAX_VERTICES_PER_POLYGON).contains(&count)))
                || (!ground && (kind != 1 || count != 2))
            {
                return Err(NavMeshError("invalid Detour polygon type or size"));
            }
            let indices =
                std::array::from_fn(|i| u16::from_le_bytes([row[4 + i * 2], row[5 + i * 2]]));
            if indices[..count]
                .iter()
                .any(|&i| usize::from(i) >= tile.vertices.len())
            {
                return Err(NavMeshError("out-of-range Detour vertex index"));
            }
            let flags = u16::from_le_bytes([row[28], row[29]]);
            if !ground {
                continue;
            }
            let mut positions = [[0.; 3]; MAX_VERTICES_PER_POLYGON];
            for (position, &index) in positions.iter_mut().zip(&indices[..count]) {
                *position = tile.vertices[usize::from(index)];
            }
            if !convex_polygon(&positions[..count]) {
                return Err(NavMeshError(
                    "non-convex or degenerate Detour ground polygon",
                ));
            }
            let detail = tile.push_detail(
                indices,
                count,
                &details[i],
                &detail_vertices,
                detail_triangles,
            )?;
            if flags != 0 {
                tile.polygons.push(Polygon {
                    row: i as u16,
                    neighbors: std::array::from_fn(|i| {
                        u16::from_le_bytes([row[16 + i * 2], row[17 + i * 2]])
                    }),
                    vertices: indices,
                    count,
                    detail,
                });
            }
        }
        Ok(((x, z), tile))
    }

    fn push_detail(
        &mut self,
        polygon: [u16; MAX_VERTICES_PER_POLYGON],
        count: usize,
        header: &[u8; 12],
        vertices: &[Vertex],
        triangles: &[[u8; 4]],
    ) -> Result<Range<usize>> {
        let start = u32_at(header, 0) as usize;
        let end = start.saturating_add(header[8].into());
        let extra = vertices
            .get(start..end)
            .ok_or(NavMeshError("invalid Detour detail vertex range"))?;
        let start = u32_at(header, 4) as usize;
        let end = start.saturating_add(header[9].into());
        let faces = triangles
            .get(start..end)
            .ok_or(NavMeshError("invalid Detour detail triangle range"))?;
        let first = self.detail.len();
        for row in faces {
            let get_vertex = |index: u8| {
                let index = usize::from(index);
                if index < count {
                    Some(self.vertices[usize::from(polygon[index])])
                } else {
                    extra.get(index - count).copied()
                }
                .ok_or(NavMeshError("out-of-range Detour detail triangle index"))
            };
            self.detail.push([
                get_vertex(row[0])?,
                get_vertex(row[1])?,
                get_vertex(row[2])?,
            ]);
        }
        Ok(first..self.detail.len())
    }

    fn height_at(&self, x: f64, z: f64) -> Option<f32> {
        let [min, max] = self.bounds;
        if x < f64::from(min[0]) - EDGE_TOLERANCE
            || x > f64::from(max[0]) + EDGE_TOLERANCE
            || z < f64::from(min[2]) - EDGE_TOLERANCE
            || z > f64::from(max[2]) + EDGE_TOLERANCE
        {
            return None;
        }
        for polygon in &self.polygons {
            let indices = &polygon.vertices[..polygon.count];
            let vertex = |index| self.vertices[usize::from(index)];
            let mut positive = false;
            let mut negative = false;
            for (&a, &b) in indices.iter().zip(indices.iter().cycle().skip(1)) {
                let cross = cross_xz(vertex(a), vertex(b), x, z);
                positive |= cross > EDGE_TOLERANCE;
                negative |= cross < -EDGE_TOLERANCE;
            }
            if positive && negative {
                continue;
            }
            if let Some(y) = self.detail[polygon.detail.clone()]
                .iter()
                .find_map(|t| triangle_height(*t, x, z))
            {
                return Some(y);
            }
            // Some clipped edge points have no matching detail triangle.
            // Interpolate the containing convex base polygon in that case.
            if let Some(y) = indices[1..].windows(2).find_map(|pair| {
                triangle_height([vertex(indices[0]), vertex(pair[0]), vertex(pair[1])], x, z)
            }) {
                return Some(y);
            }
        }
        None
    }
}

fn convex_polygon(vertices: &[Vertex]) -> bool {
    let mut signed = 0.0;
    for (&a, &b) in vertices.iter().zip(vertices.iter().cycle().skip(1)) {
        for &p in vertices {
            let cross = cross_xz(a, b, p[0].into(), p[2].into());
            if cross.abs() <= EDGE_TOLERANCE {
                continue;
            }
            if signed == 0. {
                signed = cross;
            } else if signed.signum() != cross.signum() {
                return false;
            }
        }
    }
    signed != 0.
}

fn cross_xz(a: Vertex, b: Vertex, x: f64, z: f64) -> f64 {
    (f64::from(b[0]) - f64::from(a[0])) * (z - f64::from(a[2]))
        - (f64::from(b[2]) - f64::from(a[2])) * (x - f64::from(a[0]))
}

fn triangle_height([a, b, c]: Triangle, x: f64, z: f64) -> Option<f32> {
    let area = cross_xz(a, b, c[0].into(), c[2].into());
    if area.abs() <= f64::EPSILON {
        return None;
    }
    let weights = [
        cross_xz(b, c, x, z),
        cross_xz(c, a, x, z),
        cross_xz(a, b, x, z),
    ];
    if weights
        .into_iter()
        .any(|v| v * area.signum() < -EDGE_TOLERANCE)
    {
        return None;
    }
    Some(
        ((weights[0] * f64::from(a[1])
            + weights[1] * f64::from(b[1])
            + weights[2] * f64::from(c[1]))
            / area) as f32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(col: u32) -> Vec<u8> {
        let mut blob = vec![0; TILE_HEADER_BYTES];
        let put = |blob: &mut [u8], offset, value: u32| {
            blob[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        };
        let x = col as f32 * 2.;
        for (offset, word) in [
            (0, NAV_MAGIC),
            (4, NAV_VERSION),
            (8, col),
            (24, 2), // one ground polygon and one off-mesh link
            (28, 5),
            (36, 1),
            (40, 1), // one detail vertex above the base plane
            (44, 3),
            (52, 1),
            (56, 1),
            (72, x.to_bits()),
            (84, (x + 2.).to_bits()),
            (88, 4f32.to_bits()),
            (92, 2f32.to_bits()),
        ] {
            put(&mut blob, offset, word);
        }
        for vertex in [
            [x, 1., 0.],
            [x + 2., 1., 0.],
            [x, 1., 2.],
            [x + 1., 1., 1.5],
            [x + 2., 1., 2.],
        ] {
            for f in vertex {
                blob.extend(f.to_le_bytes());
            }
        }
        let mut ground = [0; 32];
        ground[6] = 1;
        ground[8] = 2;
        ground[28] = 1;
        ground[30] = 3;
        blob.extend(ground);
        let mut link = [0; 32];
        link[4] = 3;
        link[6] = 4;
        link[28] = 1;
        link[30] = 2;
        link[31] = 1 << 6;
        blob.extend(link);
        let mut detail = [0; 12];
        detail[8] = 1;
        detail[9] = 3;
        blob.extend(detail);
        for f in [x + 0.5, 3., 0.5] {
            blob.extend(f.to_le_bytes());
        }
        blob.extend([0, 1, 3, 0, 1, 2, 3, 0, 2, 0, 3, 0]);
        blob.extend([0; 36]);
        blob
    }

    fn file() -> Vec<u8> {
        let mut file: Vec<u8> = [2u32, 0, 0, 0, 2f32.to_bits(), 2f32.to_bits(), 8, 16]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        for col in 0..2u32 {
            let tile = tile(col);
            file.extend((col + 1).to_le_bytes());
            file.extend((tile.len() as u32).to_le_bytes());
            file.extend(tile);
        }
        file
    }

    #[test]
    fn indexed_ground_keeps_height_and_rejects_empty_corner_and_links() {
        let mesh = NavMesh::parse(&file()).unwrap();
        assert_eq!(mesh.tile_count(), 2);
        assert_eq!(mesh.height_at(0.5, 0.5), Some(3.));
        assert_eq!(mesh.height_at(2.5, 0.5), Some(3.));
        assert_eq!(mesh.height_at(2., 0.), Some(1.));
        assert_eq!(mesh.height_at(1.5, 1.5), None);
        assert!(mesh.nearest_ground([1.5, 1.5], 0.1).is_none());
        let [x, _, z] = mesh.nearest_ground([1.5, 1.5], 1.).unwrap();
        assert_eq!([x, z], [2., 1.5]);
        assert!(mesh.route([0.1, 0.1], [2.1, 0.1], 0.).is_none());
        let route = mesh.route([0.1, 0.1], [2.1, 0.1], 1.2).unwrap();
        assert_eq!(route.len(), 1);
        assert!((route[0][0] - 2.).abs() < 0.001 && route[0][1].abs() < 0.001);
        assert_eq!(mesh.height_at(1., 1.5), None);
        assert_eq!(mesh.height_at(f32::NAN, 0.), None);
        assert_eq!(mesh.height_at(f32::MAX, 0.), None);
    }

    #[test]
    fn rejects_truncation_duplicates_and_invalid_polygon_indices() {
        let bytes = file();
        for end in 0..bytes.len() {
            assert!(NavMesh::parse(&bytes[..end]).is_err(), "{end}");
        }
        let mut duplicated = bytes.clone();
        let second_tile = 40 + u32_at(&bytes, 36) as usize;
        duplicated[second_tile..second_tile + 4].copy_from_slice(&1u32.to_le_bytes());
        assert!(NavMesh::parse(&duplicated).is_err());
        let mut invalid = bytes;
        let first_polygon_vertex = 40 + TILE_HEADER_BYTES + 5 * 12 + 4;
        invalid[first_polygon_vertex..first_polygon_vertex + 2]
            .copy_from_slice(&5u16.to_le_bytes());
        assert!(NavMesh::parse(&invalid).is_err());
    }

    #[test]
    #[ignore = "requires the operator's local server-data supplement"]
    fn imported_solo_and_aral_meshes_cover_original_spawn_positions() {
        let directory = std::path::PathBuf::from(std::env::var_os("HALCYON_DATA_EXTRA").unwrap())
            .join("maps/navmesh");
        let open = |name| NavMesh::parse(&std::fs::read(directory.join(name)).unwrap()).unwrap();
        let solo = open("F002__F002_navMesh.navmesh");
        let aral = open("A001__navMesh_aramWSpawn.navmesh");
        let aral_without_pads = open("A001__navMesh_aramWOSpawn.navmesh");
        assert_eq!(solo.tile_count(), 1_777);
        assert_eq!(aral.tile_count(), 511);
        assert!(solo.route([-90., 0.], [90., 0.], 1.).is_some());
        assert!(aral.route([-78.18, 0.88], [79.66, -0.91], 1.).is_some());
        let mut scratch = RouteSearch::default();
        for (mesh, start, destination) in [
            (&solo, [-90., 0.], [90., 0.]),
            (&aral, [-78.18, 0.88], [79.66, -0.91]),
            (&solo, [90., 0.], [-90., 0.]),
        ] {
            assert_eq!(
                mesh.route_with(start, destination, 1., &mut scratch),
                mesh.route(start, destination, 1.)
            );
        }
        for (x, z) in [(90., 0.), (94., -6.), (101., -3.), (101., 3.), (94., 6.)] {
            assert!(solo.height_at(-x, z).is_some());
            assert!(solo.height_at(x, z).is_some());
        }
        for (x, z) in [
            (-78.18, 0.88),
            (-81.81, 1.97),
            (-77.03, -1.94),
            (79.66, -0.91),
            (78.6, -5.06),
            (83.22, 0.72),
        ] {
            assert!(aral.height_at(x, z).is_some());
            assert!(aral_without_pads.height_at(x, z).is_none());
        }
    }
}
