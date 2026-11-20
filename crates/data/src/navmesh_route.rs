use super::{NavMesh, Polygon, Tile, Vertex};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BinaryHeap};

type Cell = (i32, i32);
type Key = (Cell, usize);

#[derive(Debug, Default)]
pub(super) struct Graph {
    nodes: Vec<Node>,
    cells: BTreeMap<Cell, Vec<usize>>,
}

#[derive(Debug)]
struct Node {
    center: [f32; 2],
    key: Key,
    edges: Vec<(usize, [f32; 2])>,
    component: Option<usize>,
}

impl Graph {
    pub(super) fn build(tiles: &BTreeMap<Cell, Tile>) -> Self {
        let mut graph = Self::default();
        let mut indices = BTreeMap::new();
        for (&cell, tile) in tiles {
            for (index, poly) in tile.polygons.iter().enumerate() {
                let mut center = [0.; 2];
                for &v in &poly.vertices[..poly.count] {
                    let p = tile.vertices[usize::from(v)];
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
                graph.cells.entry(cell).or_default().push(id);
                indices.insert((cell, poly.row), id);
            }
        }
        let keys: Vec<_> = graph.nodes.iter().map(|node| node.key).collect();
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
                            node.edges
                                .push((id, [(a[0] + b[0]) * 0.5, (a[2] + b[2]) * 0.5]));
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
                                            node.edges.push((indices[&(adjacent, p.row)], point));
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
                    let next = graph.nodes[id].edges[edge].0;
                    if graph.nodes[next].component.is_none() {
                        graph.nodes[next].component = Some(origin);
                        stack.push(next);
                    }
                }
            }
        }
        graph
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
        let mut best = None;
        for (&at, ids) in self
            .graph
            .cells
            .range((low.0.saturating_sub(1), i32::MIN)..=(high.0.saturating_add(1), i32::MAX))
        {
            if at.1 < low.1.saturating_sub(1) || at.1 > high.1.saturating_add(1) {
                continue;
            }
            let tile = &self.tiles[&at];
            for &id in ids {
                if component.is_some() && self.graph.nodes[id].component != component {
                    continue;
                }
                let point = closest(tile, &tile.polygons[self.graph.nodes[id].key.1], p);
                let dist = distance(p, point);
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
    /// Each returned point is a portal midpoint or the projected destination.
    pub fn route(
        &self,
        start: [f32; 2],
        destination: [f32; 2],
        radius: f32,
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
        let mut frontier = BinaryHeap::new();
        let mut cost = vec![f32::INFINITY; self.graph.nodes.len()];
        let mut predecessor = vec![None; self.graph.nodes.len()];
        let nodes = &self.graph.nodes;
        cost[from] = 0.;
        frontier.push(Visit(from, 0., 0.));
        while let Some(Visit(id, dist, _)) = frontier.pop() {
            if dist > cost[id] {
                continue;
            }
            if id == to {
                let mut route = vec![goal];
                let mut current = id;
                while let Some((previous, point)) = predecessor[current] {
                    route.push(point);
                    current = previous;
                }
                if entry != start {
                    route.push(entry);
                }
                route.reverse();
                return Some(route);
            }
            for &(next, point) in &nodes[id].edges {
                let new_cost = dist + distance(nodes[id].center, nodes[next].center);
                if new_cost < cost[next] {
                    cost[next] = new_cost;
                    predecessor[next] = Some((id, point));
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
