//! Repeated directed destinations on Solo's two mid lanes before units diverge in combat.
use crate::Entity;
use crate::Position;

// Distance from origin and absolute Z. Team zero travels from negative X.
// Both sides encode +0.0 for Z on the center line.
const COMMON: [(f32, f32); 16] = [
    (69., 0.),
    (65.08, 2.11),
    (61.56, 2.11),
    (59.8, 0.),
    (56.87, 0.),
    (55.45, 3.),
    (50.51, 3.),
    (49., 0.),
    (38.96, 0.),
    (36.61, 3.),
    (32.44, 3.),
    (30.43, 0.),
    (21.88, 0.),
    (19.87, 3.),
    (14.85, 3.),
    (13.17, 0.),
];
const AWAY_EXTENSION: [(f32, f32); 3] = [(6.26, 0.), (0., 0.), (-6.26, 0.)];

fn waypoint(team: u8, index: usize) -> Option<Position> {
    if let Some(&(x, z)) = COMMON.get(index) {
        return Some(Position {
            x: if team == 0 { -x } else { x },
            y: if team == 0 || z == 0. { z } else { -z },
        });
    }
    if team != 1 {
        return None;
    }
    let &(x, y) = AWAY_EXTENSION.get(index - COMMON.len())?;
    Some(Position { x, y })
}

pub(crate) fn march(actor: &mut Entity, fallback: Position) -> Position {
    // Each marker is retired once. Pursuit can advance a unit past several
    // markers; resuming its march must never take it back to a retired one.
    while let Some(goal) = waypoint(actor.team, usize::from(actor.solo_route_index)) {
        let ahead = if actor.team == 0 {
            goal.x > actor.position.x
        } else {
            goal.x < actor.position.x
        };
        if ahead {
            return goal;
        }
        actor.solo_route_index += 1;
    }
    fallback
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Kind, TICK_HZ, World};

    #[test]
    fn opening_destinations_preserve_both_turns_and_positive_zero() {
        let mut w = World::new(2, 10);
        w.use_solo_mid_lane().unwrap();
        let mut observed = [Vec::new(), Vec::new()];
        let mut first = [None, None];
        for _ in 0..(19 + 30) * TICK_HZ {
            w.step();
            for team in 0..2 {
                if first[team].is_none() {
                    first[team] = w
                        .entities
                        .iter()
                        .find(|e| e.kind == Kind::Minion && usize::from(e.team) == team)
                        .map(|e| e.id);
                }
                if let Some((p, None)) = first[team]
                    .and_then(|id| w.index(id))
                    .and_then(|i| w.entities[i].lane_step())
                {
                    let b = (p.x.to_bits(), p.y.to_bits());
                    if observed[team].last() != Some(&b) {
                        observed[team].push(b);
                    }
                }
            }
            if observed.iter().all(|r| r.len() >= 4) {
                break;
            }
        }
        assert_eq!(
            observed[0][..4],
            [
                (0xc28a0000, 0),
                (0xc28228f6, 0x40070a3d),
                (0xc2763d71, 0x40070a3d),
                (0xc26f3333, 0)
            ]
        );
        assert_eq!(
            observed[1][..4],
            [
                (0x428a0000, 0),
                (0x428228f6, 0xc0070a3d),
                (0x42763d71, 0xc0070a3d),
                (0x426f3333, 0)
            ]
        );

        let i = w.index(first[0].unwrap()).unwrap();
        let enemy = w.entities[1].id;
        let close = Position { x: -55., y: 1. };
        w.entities[1].position = close;
        assert_eq!(w.decide(i), Command::Attack(enemy));
        w.entities[i].order = Command::Attack(enemy);
        let budget = w.nav_budget;
        w.walk(i, close, &mut Vec::new());
        assert_eq!(w.nav_budget, budget);
        assert_eq!(w.entities[i].lane_step(), Some((close, Some(enemy))));
        let fallback = w.layout.objective(1);
        w.entities[i].position = Position { x: 0., y: 0. };
        assert_eq!(march(&mut w.entities[i], fallback), fallback);
        w.entities[i].position = Position { x: -70., y: 0. };
        assert_eq!(march(&mut w.entities[i], fallback), fallback);
        assert_eq!(w.entities[i].solo_route_index, 16);
        let away = w.index(first[1].unwrap()).unwrap();
        w.entities[away].position = Position { x: -7., y: 0. };
        let away_goal = w.layout.objective(0);
        w.entities[away].order = Command::MoveTo(away_goal);
        w.walk(away, away_goal, &mut Vec::new());
        assert_eq!(w.entities[away].solo_route_index, 19);
        assert_eq!(w.entities[away].lane_step(), Some((away_goal, None)));
        assert_eq!(w.nav_budget, budget);
    }
}
