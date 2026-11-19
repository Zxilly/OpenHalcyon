//! Visibility-graph A* for the validation lane. The two authored side rocks
//! have fixed clearance waypoints. The central marching lane stays traversable.
use super::{LANE, Position};
use std::collections::VecDeque;
#[derive(Clone, Copy)]
struct Rock {
    left: f32,
    right: f32,
    bottom: f32,
    top: f32,
}
const ROCKS: [Rock; 2] = [
    Rock {
        left: 18.,
        right: 23.,
        bottom: 2.,
        top: 5.,
    },
    Rock {
        left: 37.,
        right: 42.,
        bottom: -5.,
        top: -2.,
    },
];
const CORNERS: [Position; 8] = [
    Position { x: 17.8, y: 1.8 },
    Position { x: 23.2, y: 1.8 },
    Position { x: 17.8, y: 5. },
    Position { x: 23.2, y: 5. },
    Position { x: 36.8, y: -1.8 },
    Position { x: 42.2, y: -1.8 },
    Position { x: 36.8, y: -5. },
    Position { x: 42.2, y: -5. },
];
pub(super) fn walkable(p: Position) -> bool {
    (0.0..=LANE).contains(&p.x)
        && (-5.0..=5.0).contains(&p.y)
        && !ROCKS
            .iter()
            .any(|r| p.x >= r.left && p.x <= r.right && p.y >= r.bottom && p.y <= r.top)
}
fn intersects(a: Position, b: Position, r: Rock) -> bool {
    let (mut enter, mut leave) = (0f32, 1f32);
    for (origin, d, lo, hi) in [
        (a.x, b.x - a.x, r.left, r.right),
        (a.y, b.y - a.y, r.bottom, r.top),
    ] {
        if d.abs() < f32::EPSILON {
            if origin < lo || origin > hi {
                return false;
            }
            continue;
        }
        let u = (lo - origin) / d;
        let v = (hi - origin) / d;
        enter = enter.max(u.min(v));
        leave = leave.min(u.max(v));
        if enter > leave {
            return false;
        }
    }
    true
}
pub(super) fn clear(a: Position, b: Position) -> bool {
    !ROCKS.iter().any(|r| intersects(a, b, *r))
}
#[derive(Debug, Clone, Default, serde::Serialize)]
pub(super) struct Path {
    goal: Option<Position>,
    steps: VecDeque<Position>,
}
impl Path {
    pub fn next(&mut self, from: Position, to: Position, budget: &mut u8) -> Option<Position> {
        if clear(from, to) {
            self.steps.clear();
            self.goal = None;
            return Some(to);
        }
        if self.goal.is_none_or(|old| old.dist(to) > 0.5) || self.steps.is_empty() {
            if *budget == 0 {
                return None;
            }
            *budget -= 1;
            self.steps = search(from, to)?;
            self.goal = Some(to);
        }
        while self.steps.front().is_some_and(|v| v.dist(from) < 0.01) {
            self.steps.pop_front();
        }
        self.steps.front().copied()
    }
}
fn search(start: Position, end: Position) -> Option<VecDeque<Position>> {
    // Each of the ten nodes is expanded at most once. ID breaks equal-cost ties.
    let mut p = [start; 10];
    p[1..9].copy_from_slice(&CORNERS);
    p[9] = end;
    let mut used = [false; 10];
    let mut cost = [f32::INFINITY; 10];
    let mut prev = [0usize; 10];
    cost[0] = 0.;
    for _ in 0..10 {
        let at = (0..10)
            .filter(|i| !used[*i] && cost[*i].is_finite())
            .min_by(|a, b| {
                (cost[*a] + p[*a].dist(end))
                    .total_cmp(&(cost[*b] + p[*b].dist(end)))
                    .then(a.cmp(b))
            })?;
        if at == 9 {
            let mut result = VecDeque::new();
            let mut i = 9;
            while i != 0 {
                result.push_front(p[i]);
                i = prev[i]
            }
            return Some(result);
        }
        used[at] = true;
        for i in 1..10 {
            if used[i] || !clear(p[at], p[i]) {
                continue;
            }
            let d = cost[at] + p[at].dist(p[i]);
            if d < cost[i] {
                cost[i] = d;
                prev[i] = at
            }
        }
    }
    None
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn routes_around_side_rock_with_bounded_work() {
        let from = Position { x: 15., y: 4. };
        let to = Position { x: 25., y: 4. };
        assert!(!clear(from, to));
        let mut path = Path::default();
        assert!(path.next(from, to, &mut 0).is_none());
        let mut at = from;
        let mut budget = 1;
        for _ in 0..100 {
            let next = path.next(at, to, &mut budget).unwrap();
            assert!(clear(at, next));
            at = at.advance(next, 0.25);
            assert!(walkable(at));
            if at == to {
                return;
            }
        }
        panic!("path stalled")
    }
}
