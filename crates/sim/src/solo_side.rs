use super::{Entity, Position};

// Absolute X,Z of the west-side route. The east opens with the same reflected
// first 17 points. Continuing on the other side's recorded road is host policy.
const HALF: [(f32, f32); 23] = [
    (75., 5.),
    (72.61, 6.63),
    (73.44, 11.31),
    (71.61, 14.24),
    (68.75, 15.37),
    (66.49, 19.53),
    (67.08, 22.85),
    (63.2, 26.79),
    (59.1, 27.07),
    (55.68, 31.06),
    (52.35, 34.94),
    (53.13, 38.21),
    (50.04, 41.81),
    (46.04, 41.36),
    (41.08, 45.13),
    (35.79, 49.13),
    (30.23, 53.35),
    (24.84, 57.45),
    (23.32, 61.1),
    (18.65, 60.93),
    (16.85, 56.97),
    (8.21, 56.97),
    (0., 56.97),
];
const LAST: usize = 2 * (HALF.len() - 1);

fn point(index: usize, team: u8, side: i8) -> Position {
    let (x, z) = HALF[index.min(LAST - index)];
    Position {
        x: if x != 0. && ((index < HALF.len()) == (team == 0)) {
            -x
        } else {
            x
        },
        y: z * f32::from(side),
    }
}

pub(super) fn anchor(team: u8, side: i8) -> Position {
    point(0, team, side)
}

pub(super) fn march(actor: &mut Entity, fallback: Position) -> Position {
    let mut i = usize::from(actor.solo_route_index).max(1);
    while i <= LAST {
        let goal = point(i, actor.team, actor.solo_side);
        let prev = point(i - 1, actor.team, actor.solo_side);
        let dx = goal.x - prev.x;
        let dy = goal.y - prev.y;
        if (goal.x - actor.position.x) * dx + (goal.y - actor.position.y) * dy > 0.001 {
            actor.solo_route_index = i as u8;
            return goal;
        }
        i += 1;
    }
    actor.solo_route_index = (LAST + 1) as u8;
    fallback
}

fn mid(p: Position) -> bool {
    p.x.abs() <= 101. && p.y.abs() <= 6.
}

pub(super) fn walkable(p: Position) -> bool {
    if mid(p) {
        return true;
    }
    let x = p.x.abs();
    let z = p.y.abs();
    if x > 81. || z > 66. {
        return false;
    }
    HALF.windows(2).any(|pair| inside_section(x, z, pair))
}

fn inside_section(x: f32, z: f32, pair: &[(f32, f32)]) -> bool {
    let [(a, b), (c, d)] = [pair[0], pair[1]];
    if x < a.min(c) - 5. || x > a.max(c) + 5. || z < b.min(d) - 5. || z > b.max(d) + 5. {
        return false;
    }
    let (dx, dz) = (c - a, d - b);
    let t = (((x - a) * dx + (z - b) * dz) / (dx * dx + dz * dz)).clamp(0., 1.);
    (x - a - t * dx).powi(2) + (z - b - t * dz).powi(2) <= 25.
}

pub(super) fn clear(a: Position, b: Position) -> bool {
    if mid(a) && mid(b) {
        return true;
    }
    if !walkable(a) || !walkable(b) {
        return false;
    }
    let steps = (a.dist(b) / 1.5).ceil().max(1.) as u32;
    (0..=steps).all(|i| {
        let t = i as f32 / steps as f32;
        walkable(Position {
            x: a.x + (b.x - a.x) * t,
            y: a.y + (b.y - a.y) * t,
        })
    })
}

// Within one quadrant each corridor section is convex: its endpoints
// certify the entire segment. Subdivide at overlaps, and reject if coverage
// cannot be established within the depth bound.
fn sections(p: Position) -> u32 {
    let mut mask = u32::from(mid(p));
    let (x, z) = (p.x.abs(), p.y.abs());
    if x <= 81. && z <= 66. {
        for (i, pair) in HALF.windows(2).enumerate() {
            if inside_section(x, z, pair) {
                mask |= 1 << (i + 1);
            }
        }
    }
    mask
}

fn covered(a: Position, b: Position, a_sections: u32, b_sections: u32, depth: u8) -> bool {
    if a_sections & b_sections != 0 {
        return true;
    }
    if a_sections == 0 || b_sections == 0 || depth == 0 {
        return false;
    }
    let middle = Position {
        x: (a.x + b.x) * 0.5,
        y: (a.y + b.y) * 0.5,
    };
    let mask = sections(middle);
    covered(a, middle, a_sections, mask, depth - 1)
        && covered(middle, b, mask, b_sections, depth - 1)
}

fn movement_clear(a: Position, b: Position) -> bool {
    if mid(a) && mid(b) {
        return true;
    }
    // Fit each piece to a single reflected quadrant before comparing masks.
    if (a.x < 0. && b.x > 0.) || (a.x > 0. && b.x < 0.) {
        let axis = Position {
            x: 0.,
            y: a.y + (b.y - a.y) * (a.x / (a.x - b.x)),
        };
        return movement_clear(a, axis) && movement_clear(axis, b);
    }
    if (a.y < 0. && b.y > 0.) || (a.y > 0. && b.y < 0.) {
        let axis = Position {
            x: a.x + (b.x - a.x) * (a.y / (a.y - b.y)),
            y: 0.,
        };
        return movement_clear(a, axis) && movement_clear(axis, b);
    }
    covered(a, b, sections(a), sections(b), 8)
}

fn nearest_reachable(p: Position, side: i8) -> Option<usize> {
    let mut best = None;
    let mut distance = f32::INFINITY;
    for i in 0..=LAST {
        let waypoint = point(i, 0, side);
        let d = p.dist(waypoint);
        if d < distance && movement_clear(p, waypoint) {
            best = Some(i);
            distance = d;
        }
    }
    best
}

// Players may click ahead along a road. Crossing between roads goes via the
// junctions at the bases, and every chosen segment stays inside the corridor.
pub(super) fn steer(from: Position, to: Position) -> Option<Position> {
    if movement_clear(from, to) {
        return Some(to);
    }
    if !walkable(from) || !walkable(to) {
        return None;
    }
    let from_side = if mid(from) { 0 } else { from.y.signum() as i8 };
    let to_side = if mid(to) { 0 } else { to.y.signum() as i8 };
    let side = if from_side == 0 { to_side } else { from_side };
    let start = nearest_reachable(from, side)?;
    let end = if from_side != 0 && from_side != to_side {
        if start < HALF.len() { 0 } else { LAST }
    } else {
        nearest_reachable(to, side)?
    };
    let current = point(start, 0, side);
    if start == end {
        return Some(current);
    }
    let next = if start < end { start + 1 } else { start - 1 };
    let waypoint = point(next, 0, side);
    if movement_clear(from, waypoint) {
        Some(waypoint)
    } else {
        Some(current)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Event, Kind, TICK_HZ, World};

    #[test]
    fn both_flanks_follow_the_first_native_turns_and_fourth_wave_replaces_the_leader() {
        let mut w = World::new(7, 10);
        w.use_solo_mid_lane().unwrap();
        let mut count = [[0usize; 2]; 2];
        let mut tracks = std::collections::BTreeMap::<u32, Vec<Position>>::new();
        for _ in 1..144 * TICK_HZ {
            let events = w.step();
            for ev in events {
                if let Event::Spawn { id } = ev {
                    let e = &w.entities[w.index(id).unwrap()];
                    if e.kind != Kind::Minion || e.solo_side == 0 {
                        continue;
                    }
                    let n = &mut count[e.team as usize][usize::from(e.solo_side > 0)];
                    let side_slot = *n % 4;
                    assert_eq!(
                        w.tick,
                        (19 + 25 * (*n / 4) as u64 + side_slot as u64) * TICK_HZ
                    );
                    let lineup = if *n / 4 == 3 {
                        [336, 336, 335, 338]
                    } else {
                        [336, 336, 337, 335]
                    };
                    assert_eq!(e.type_id, lineup[side_slot]);
                    if *n == 0 {
                        tracks.insert(id, Vec::new());
                    }
                    *n += 1;
                }
            }
            for (id, points) in &mut tracks {
                if points.len() >= 8 {
                    continue;
                }
                let e = &w.entities[w.index(*id).unwrap()];
                if let Some((p, None)) = e.lane_step()
                    && points.last() != Some(&p)
                {
                    let expected = point(points.len() + 1, e.team, e.solo_side);
                    assert_eq!(
                        (p.x.to_be_bytes(), p.y.to_be_bytes()),
                        (expected.x.to_be_bytes(), expected.y.to_be_bytes())
                    );
                    points.push(p);
                }
            }
        }
        assert_eq!(count, [[20; 2]; 2]);
        assert!(tracks.values().all(|points| points.len() == 8));
    }

    #[test]
    fn bot_kites_toward_a_reachable_retreat_point() {
        let mut world = World::new(0, 0);
        world.use_solo_mid_lane().unwrap();
        let i = 4;
        let before = Position {
            x: -1.5873325,
            y: 52.13896,
        };
        world.entities[i].position = before;
        world.entities[i].next_attack = 10;
        world.entities[1].position = Position { x: 0., y: 53. };
        let home = world.layout.home(&world.entities[i]);
        assert!(!walkable(before.advance(home, 1.)));
        let order = world.decide(i);
        let Command::MoveTo(goal) = order else {
            panic!("expected a retreat order, received {order:?}");
        };
        assert!(movement_clear(before, goal));
        assert_ne!(before, goal);
        assert_eq!(world.order(i, order), Ok(()));
    }

    #[test]
    fn hero_enters_a_flank_without_crossing_the_gap_near_the_base() {
        let from = Position { x: 80.125, y: 5.98 };
        let to = Position { x: 73.44, y: 11.31 };
        let mut world = World::new(8, 10);
        world.use_solo_mid_lane().unwrap();
        world.entities[0].position = from;
        world.command(1, Command::MoveTo(to)).unwrap();
        for _ in 0..200 {
            world.walk(0, to, &mut Vec::new());
            assert!(
                walkable(world.entities[0].position),
                "{:?}",
                world.entities[0].position
            );
            if world.entities[0].position == to {
                return;
            }
        }
        panic!("did not reach the flank");
    }

    #[test]
    fn hero_can_select_either_flank_without_crossing_the_interior() {
        let mut w = World::new(8, 10);
        w.use_solo_mid_lane().unwrap();
        let home = w.entities[0].position;
        let interior = Position { x: -35., y: 23. };
        assert!(!walkable(interior));
        assert!(w.command(1, Command::MoveTo(interior)).is_err());
        for side in [-1, 1] {
            let destination = point(18, 0, side);
            assert!(!clear(home, destination));
            w.entities[0].position = home;
            w.command(1, Command::MoveTo(destination)).unwrap();
            for _ in 0..1200 {
                let old = w.entities[0].position;
                w.walk(0, destination, &mut Vec::new());
                let new = w.entities[0].position;
                assert!(clear(old, new));
                if new == destination {
                    break;
                }
            }
            assert_eq!(w.entities[0].position, destination);
        }
    }
}
