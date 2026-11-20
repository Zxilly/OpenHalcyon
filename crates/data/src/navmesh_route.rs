use super::{NavMesh, Polygon, Tile, Vertex};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BinaryHeap};
use std::ops::Range;

type Cell = (i32, i32);
type Key = (Cell, usize);

#[derive(Debug, Default)]
pub(super) struct Graph {
    nodes: Vec<Node>,
    columns: BTreeMap<i32, BTreeMap<i32, CellNodes>>,
}

#[derive(Debug)]
struct CellNodes {
    nodes: Range<usize>,
    bounds: [f32; 4],
}

#[derive(Debug)]
struct Node {
    center: [f32; 2],
    key: Key,
    edges: Vec<Edge>,
    component: Option<usize>,
}

#[derive(Debug)]
struct Edge {
    next: usize,
    point: [f32; 2],
    cost: f32,
}

/// Reusable A* scratch space. Give each independently simulated world its own
/// scratch space and share the immutable mesh.
#[derive(Default)]
pub struct RouteSearch {
    generation: u32,
    visits: Vec<SearchNode>,
    frontier: BinaryHeap<Visit>,
}

#[derive(Clone, Default)]
struct SearchNode {
    generation: u32,
    cost: f32,
    previous: usize,
    portal: [f32; 2],
}

impl RouteSearch {
    fn begin(&mut self, count: usize) {
        self.visits.resize(count, SearchNode::default());
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.visits.fill(SearchNode::default());
            self.generation = 1;
        }
        self.frontier.clear();
    }
}

impl Graph {
    pub(super) fn build(tiles: &BTreeMap<Cell, Tile>) -> Self {
        let mut graph = Self::default();
        let mut indices = BTreeMap::new();
        for (&cell, tile) in tiles {
            if tile.polygons.is_empty() {
                continue;
            }
            let first = graph.nodes.len();
            let mut bounds = [
                f32::INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
            ];
            for (index, poly) in tile.polygons.iter().enumerate() {
                let mut center = [0.; 2];
                for &v in &poly.vertices[..poly.count] {
                    let p = tile.vertices[usize::from(v)];
                    bounds[0] = bounds[0].min(p[0]);
                    bounds[1] = bounds[1].min(p[2]);
                    bounds[2] = bounds[2].max(p[0]);
                    bounds[3] = bounds[3].max(p[2]);
                    center[0] += p[0] / poly.count as f32;
                    center[1] += p[2] / poly.count as f32;
                }
                let id = graph.nodes.len();
                graph.nodes.push(Node {
                    center,
                    key: (cell, index),
                    edges: Vec::new(),
                    component: None,
                });
                indices.insert((cell, poly.row), id);
            }
            graph.columns.entry(cell.0).or_default().insert(
                cell.1,
                CellNodes {
                    nodes: first..graph.nodes.len(),
                    bounds,
                },
            );
        }
        let keys: Vec<_> = graph.nodes.iter().map(|node| node.key).collect();
        let centers: Vec<_> = graph.nodes.iter().map(|node| node.center).collect();
        for node in &mut graph.nodes {
            let (cell, i) = node.key;
            let tile = &tiles[&cell];
            let poly = &tile.polygons[i];
            for (edge, next) in poly.neighbors[..poly.count].iter().copied().enumerate() {
                let (a, b) = endpoints(tile, poly, edge);
                match next {
                    0 => {}
                    1..=0x7fff => {
                        if let Some(&id) = indices.get(&(cell, next - 1))
                            && let other = &tile.polygons[keys[id].1]
                            && other.neighbors[..other.count].iter().enumerate().any(
                                |(j, &neighbor)| {
                                    let (c, d) = endpoints(tile, other, j);
                                    neighbor == poly.row + 1
                                        && ((c == a && d == b) || (c == b && d == a))
                                },
                            )
                        {
                            node.edges.push(Edge {
                                next: id,
                                point: [(a[0] + b[0]) * 0.5, (a[2] + b[2]) * 0.5],
                                cost: distance(node.center, centers[id]),
                            });
                        }
                    }
                    _ => {
                        let side = next & 0x7fff;
                        let (dx, dz, axis) = match side {
                            0 => (1, 0, 2),
                            2 => (0, 1, 0),
                            4 => (-1, 0, 2),
                            6 => (0, -1, 0),
                            _ => continue,
                        };
                        let adjacent = (cell.0.saturating_add(dx), cell.1.saturating_add(dz));
                        if let Some(other) = tiles.get(&adjacent) {
                            for p in &other.polygons {
                                for (j, &n) in p.neighbors[..p.count].iter().enumerate() {
                                    if n == 0x8000 | ((side + 4) % 8) {
                                        let (c, d) = endpoints(other, p, j);
                                        if let Some(point) =
                                            portal(a, b, c, d, axis, tile.climb.min(other.climb))
                                        {
                                            let id = indices[&(adjacent, p.row)];
                                            node.edges.push(Edge {
                                                next: id,
                                                point,
                                                cost: distance(node.center, centers[id]),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        let mut stack = Vec::new();
        for origin in 0..graph.nodes.len() {
            if graph.nodes[origin].component.is_some() {
                continue;
            }
            graph.nodes[origin].component = Some(origin);
            stack.push(origin);
            while let Some(id) = stack.pop() {
                let degree = graph.nodes[id].edges.len();
                for edge in 0..degree {
                    let next = graph.nodes[id].edges[edge].next;
                    if graph.nodes[next].component.is_none() {
                        graph.nodes[next].component = Some(origin);
                        stack.push(next);
                    }
                }
            }
        }
        graph
    }

    fn in_bounds(&self, low: Cell, high: Cell) -> impl Iterator<Item = (Cell, &CellNodes)> {
        self.columns
            .range(low.0.saturating_sub(1)..=high.0.saturating_add(1))
            .flat_map(move |(&x, rows)| {
                rows.range(low.1.saturating_sub(1)..=high.1.saturating_add(1))
                    .map(move |(&z, nodes)| ((x, z), nodes))
            })
    }
}

fn endpoints(tile: &Tile, poly: &Polygon, edge: usize) -> (Vertex, Vertex) {
    (
        tile.vertices[usize::from(poly.vertices[edge])],
        tile.vertices[usize::from(poly.vertices[(edge + 1) % poly.count])],
    )
}

fn portal(a: Vertex, b: Vertex, c: Vertex, d: Vertex, axis: usize, climb: f32) -> Option<[f32; 2]> {
    let across = 2 - axis;
    let low = a[axis].min(b[axis]).max(c[axis].min(d[axis]));
    let high = a[axis].max(b[axis]).min(c[axis].max(d[axis]));
    if high - low <= 0.001 || (a[across] - c[across]).abs() > 0.001 {
        return None;
    }
    let middle = (low + high) * 0.5;
    let level =
        |a: Vertex, b: Vertex| a[1] + (b[1] - a[1]) * (middle - a[axis]) / (b[axis] - a[axis]);
    if (level(a, b) - level(c, d)).abs() > climb {
        return None;
    }
    let mut point = [0.; 2];
    point[axis / 2] = middle;
    point[across / 2] = a[across];
    Some(point)
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn closest(tile: &Tile, poly: &Polygon, p: [f32; 2]) -> [f32; 2] {
    let mut positive = false;
    let mut negative = false;
    let mut best = (f32::INFINITY, p);
    for edge in 0..poly.count {
        let (a, b) = endpoints(tile, poly, edge);
        let cross = super::cross_xz(a, b, p[0].into(), p[1].into());
        positive |= cross > super::EDGE_TOLERANCE;
        negative |= cross < -super::EDGE_TOLERANCE;
        let dx = b[0] - a[0];
        let dz = b[2] - a[2];
        let length = dx * dx + dz * dz;
        if length > 0. {
            let t = ((p[0] - a[0]) * dx + (p[1] - a[2]) * dz) / length;
            let t = t.clamp(0., 1.);
            let candidate = [a[0] + t * dx, a[2] + t * dz];
            let dist = distance(p, candidate);
            if dist < best.0 {
                best = (dist, candidate);
            }
        }
    }
    if positive && negative { best.1 } else { p }
}

impl NavMesh {
    /// Checks that ground polygons cover the complete short segment.
    /// This answers ground continuity and does not model projectile occlusion.
    pub fn ground_segment(&self, a: [f32; 2], b: [f32; 2]) -> bool {
        if distance(a, b) > 24. || ![a, b].into_iter().flatten().all(f32::is_finite) {
            return false;
        }
        let cell = |p: [f32; 2]| {
            (
                ((p[0] - self.origin[0]) / self.tile_width).floor() as i32,
                ((p[1] - self.origin[2]) / self.tile_depth).floor() as i32,
            )
        };
        let (x, z) = cell(a);
        let (u, v) = cell(b);
        let mut covered = Vec::new();
        for (key, cell) in self
            .graph
            .in_bounds((x.min(u), z.min(v)), (x.max(u), z.max(v)))
        {
            let tile = &self.tiles[&key];
            for (poly, node) in tile.polygons.iter().zip(cell.nodes.clone()) {
                let c = self.graph.nodes[node].center;
                let mut interval = (0f64, 1f64);
                for edge in 0..poly.count {
                    let (v, w) = endpoints(tile, poly, edge);
                    let cross = |p: [f32; 2]| super::cross_xz(v, w, p[0].into(), p[1].into());
                    let sign = cross(c).signum();
                    let lo = sign * cross(a) + super::EDGE_TOLERANCE;
                    let hi = sign * cross(b) + super::EDGE_TOLERANCE;
                    if lo < 0. && hi < 0. {
                        interval.0 = 2.;
                        break;
                    }
                    if lo < 0. {
                        interval.0 = interval.0.max(lo / (lo - hi));
                    }
                    if hi < 0. {
                        interval.1 = interval.1.min(lo / (lo - hi));
                    }
                }
                if interval.0 <= interval.1 {
                    covered.push(interval);
                }
            }
        }
        covered.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
        let mut end = 0f64;
        for (start, finish) in covered {
            if start > end + super::EDGE_TOLERANCE {
                return false;
            }
            end = end.max(finish);
            if end >= 1. {
                return true;
            }
        }
        false
    }
    fn closest_node(
        &self,
        p: [f32; 2],
        radius: f32,
        component: Option<usize>,
    ) -> Option<(usize, [f32; 2])> {
        if !p.iter().all(|x| x.is_finite()) || !radius.is_finite() || radius < 0. {
            return None;
        }
        let cell =
            |coordinate: f32, axis: usize, step: f32| (coordinate - self.origin[axis]) / step;
        let grid = |p: [f32; 2]| {
            (
                cell(p[0], 0, self.tile_width).floor() as i32,
                cell(p[1], 2, self.tile_depth).floor() as i32,
            )
        };
        let low = grid([p[0] - radius, p[1] - radius]);
        let high = grid([p[0] + radius, p[1] + radius]);
        let mut best: Option<(f32, usize, [f32; 2])> = None;
        for (at, cell) in self.graph.in_bounds(low, high) {
            let [lo_x, lo_z, hi_x, hi_z] = cell.bounds;
            let distance_bound = (lo_x - p[0])
                .max(p[0] - hi_x)
                .max(lo_z - p[1])
                .max(p[1] - hi_z);
            // Allow room for the polygon's inclusive edge tolerance.
            if distance_bound > best.map_or(radius, |(d, _, _)| d) + 0.001 {
                continue;
            }
            let tile = &self.tiles[&at];
            for id in cell.nodes.clone() {
                if component.is_some() && self.graph.nodes[id].component != component {
                    continue;
                }
                let point = closest(tile, &tile.polygons[self.graph.nodes[id].key.1], p);
                let dist = distance(p, point);
                if dist == 0. {
                    return Some((id, point));
                }
                if dist <= radius && best.is_none_or(|(d, _, _)| dist < d) {
                    best = Some((dist, id, point));
                }
            }
        }
        best.map(|(_, id, point)| (id, point))
    }

    /// Closest walkable X/Z within a maximum horizontal distance, and ground Y.
    pub fn nearest_ground(&self, point: [f32; 2], radius: f32) -> Option<[f32; 3]> {
        let (id, [x, z]) = self.closest_node(point, radius, None)?;
        let tile = &self.tiles[&self.graph.nodes[id].key.0];
        let y = tile.height_at(x.into(), z.into())?;
        Some([x, y, z])
    }

    /// Finds a ground-only corridor between projected start and destination.
    /// Uses `polyanya::Mesh` any-angle optimal pathfinding when available,
    /// falling back to `RouteSearch` for disconnected or degenerate segments.
    pub fn route(
        &self,
        start: [f32; 2],
        destination: [f32; 2],
        radius: f32,
    ) -> Option<Vec<[f32; 2]>> {
        self.route_with(start, destination, radius, &mut RouteSearch::default())
    }

    /// The same corridor query with scratch storage retained across calls.
    pub fn route_with(
        &self,
        start: [f32; 2],
        destination: [f32; 2],
        radius: f32,
        search: &mut RouteSearch,
    ) -> Option<Vec<[f32; 2]>> {
        if let Some(path) = self.polyanya_route(start, destination, radius) {
            return Some(path);
        }
        self.route_with_graph(start, destination, radius, search)
    }

    /// Fallback corridor A* query over Detour polygon portals with retained scratch storage.
    pub fn route_with_graph(
        &self,
        start: [f32; 2],
        destination: [f32; 2],
        radius: f32,
        search: &mut RouteSearch,
    ) -> Option<Vec<[f32; 2]>> {
        let (from, entry) = self.closest_node(start, radius, None)?;
        let (to, goal) =
            self.closest_node(destination, radius, self.graph.nodes[from].component)?;
        if from == to {
            return Some(if entry == start || entry == goal {
                vec![goal]
            } else {
                vec![entry, goal]
            });
        }
        let nodes = &self.graph.nodes;
        search.begin(nodes.len());
        let generation = search.generation;
        let visits = &mut search.visits;
        let frontier = &mut search.frontier;
        visits[from] = SearchNode {
            generation,
            cost: 0.,
            previous: from,
            portal: entry,
        };
        frontier.push(Visit(from, 0., 0.));
        while let Some(Visit(id, dist, _)) = frontier.pop() {
            if dist > visits[id].cost {
                continue;
            }
            if id == to {
                let mut route = vec![goal];
                let mut current = id;
                while current != from {
                    let row = &visits[current];
                    route.push(row.portal);
                    current = row.previous;
                }
                if entry != start {
                    route.push(entry);
                }
                route.reverse();
                return Some(route);
            }
            for &Edge { next, point, cost } in &nodes[id].edges {
                let new_cost = dist + cost;
                let old_cost = if visits[next].generation == generation {
                    visits[next].cost
                } else {
                    f32::INFINITY
                };
                if new_cost < old_cost {
                    visits[next] = SearchNode {
                        generation,
                        cost: new_cost,
                        previous: id,
                        portal: point,
                    };
                    frontier.push(Visit(
                        next,
                        new_cost,
                        new_cost + distance(nodes[next].center, nodes[to].center),
                    ));
                }
            }
        }
        None
    }
}

struct Visit(usize, f32, f32);
impl PartialEq for Visit {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}
impl Eq for Visit {}
impl PartialOrd for Visit {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Visit {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .2
            .total_cmp(&self.2)
            .then_with(|| other.0.cmp(&self.0))
    }
}
