//! Per-connection projection of legal Solo bot navigation.
use crate::Message;
use crate::replication::{Batch, slots::point_move};
use halcyon_protocol::Fields;
use halcyon_sim::{Entity, Event, Kind, Position, World};
use std::collections::{HashMap, HashSet};

const SOLO_HEIGHT: f32 = f32::from_bits(0x3d75be29);
const ACTIVE_TICKS: u64 = 2;
const MIN_ORDER_DISTANCE: f32 = 0.10;

fn near(a: Position, b: Position) -> bool {
    let (dx, dz) = (a.x - b.x, a.y - b.y);
    dx * dx + dz * dz < MIN_ORDER_DISTANCE * MIN_ORDER_DISTANCE
}

#[derive(Clone, Copy)]
struct Path {
    waypoint: Position,
    goal: Position,
    target: Option<u32>,
    tick: u64,
}

#[derive(Default)]
pub(crate) struct Paths([Option<Path>; 10]);

impl Paths {
    pub(crate) fn append(
        &mut self,
        world: &World,
        viewer: &Entity,
        visible: &HashSet<u32>,
        registered: &HashMap<u32, u8>,
        events: &[Event],
        out: &mut Batch,
    ) {
        for (index, prior) in self.0.iter_mut().enumerate() {
            if prior.is_some() && !visible.contains(&(index as u32 + 1)) {
                *prior = None;
            }
        }
        for event in events {
            let Event::SoloBotPath {
                id,
                waypoint,
                goal,
                target,
            } = *event
            else {
                continue;
            };
            let Some(index) = id
                .checked_sub(1)
                .and_then(|i| (i < 10).then_some(i as usize))
            else {
                continue;
            };
            let actor = &world.entities[index];
            let last = &mut self.0[index];
            let Some(&slot) = registered.get(&id).filter(|&&s| (14..=23).contains(&s)) else {
                *last = None;
                continue;
            };
            if actor.id != id
                || actor.id == viewer.id
                || actor.kind != Kind::Hero
                || !actor.bot
                || actor.hp <= 0.
                || !visible.contains(&id)
                || target.is_some_and(|t| !visible.contains(&t) || !registered.contains_key(&t))
            {
                *last = None;
                continue;
            }
            let prior = last.filter(|p| world.tick.saturating_sub(p.tick) <= ACTIVE_TICKS);
            if let Some(prior) = prior
                && near(prior.waypoint, waypoint)
                && prior.target == target
            {
                if let Some(last) = last {
                    last.tick = world.tick;
                }
                continue;
            }
            if near(actor.position, waypoint) || !world.sees_position(viewer.team, waypoint) {
                *last = None;
                continue;
            }
            let authoring = prior.is_some_and(|p| {
                !near(p.waypoint, waypoint)
                    && (p.target.is_some() && target.is_none()
                        || p.target == target && p.goal == goal)
            });
            out.push(if authoring {
                authoritative_goal(slot, waypoint)
            } else {
                point_move::destination(slot, waypoint)
            });
            *last = Some(Path {
                waypoint,
                goal,
                target,
                tick: world.tick,
            });
        }
    }
}

fn authoritative_goal(slot: u8, destination: Position) -> Message {
    halcyon_protocol::ServerMoveBody {
        selector: slot,
        values: [destination.x, SOLO_HEIGHT, destination.y],
        flag: 1,
    }
    .encode()
}

pub(crate) fn recordable(message: &Message) -> bool {
    matches!(
        message.fields(),
        Ok(Fields::ServerMove { selector: 14..=23, values: [x, y, z], flag: 1, remaining: [] })
            if y == SOLO_HEIGHT && x.abs() <= 512. && z.abs() <= 512.
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use halcyon_protocol::{MatchCipher, Opcode};

    #[test]
    fn solo_simulation_reports_a_bot_waypoint_before_the_step_coordinate() {
        let mut world = World::new(73, 1);
        world.use_solo_mid_lane().unwrap();
        let observed = (0..60).find_map(|_| {
            let events = world.step();
            events.windows(2).find_map(|pair| match pair {
                [
                    Event::SoloBotPath { id, waypoint, .. },
                    Event::Move {
                        id: moved,
                        position,
                    },
                ] if id == moved && waypoint != position => Some((*id, *position)),
                _ => None,
            })
        });
        let (id, position) = observed.expect("a moving Solo bot must expose its current route");
        let actor = &world.entities[id as usize - 1];
        assert!(actor.bot && actor.kind == Kind::Hero);
        assert_eq!(actor.position, position);
    }

    #[test]
    fn solo_replan_replaces_only_a_registered_visible_old_route() {
        let mut world = World::new(71, 1);
        world.use_solo_mid_lane().unwrap();
        let viewer = world.entities[0].position;
        world.entities[1].position = viewer;
        let one = Position {
            x: viewer.x + 2.,
            y: viewer.y,
        };
        let two = Position {
            x: viewer.x + 2.,
            y: viewer.y + 1.,
        };
        let goal = Position {
            x: viewer.x + 5.,
            y: viewer.y,
        };
        let path = |waypoint| Event::SoloBotPath {
            id: 2,
            waypoint,
            goal,
            target: None,
        };
        let visible = HashSet::from([1, 2]);
        let registry = HashMap::from([(1, 14), (2, 15)]);
        let mut projection = Paths::default();
        let mut out = Vec::new();
        world.tick = 1;
        projection.append(
            &world,
            &world.entities[0],
            &visible,
            &HashMap::new(),
            &[path(one)],
            &mut out,
        );
        assert!(out.is_empty());
        projection.append(
            &world,
            &world.entities[0],
            &visible,
            &registry,
            &[path(one)],
            &mut out,
        );
        assert_eq!(out, [point_move::destination(15, one)]);
        out.clear();
        world.tick = 2;
        projection.append(
            &world,
            &world.entities[0],
            &visible,
            &registry,
            &[path(two)],
            &mut out,
        );
        assert_eq!(out, [authoritative_goal(15, two)]);
        assert!(recordable(&out[0]));
        let cipher = MatchCipher::new("solo authoritative waypoint");
        let encoded = cipher.encode(&out[0]).unwrap();
        let decoded = cipher.decode(&encoded[2..]).unwrap();
        assert!(matches!(decoded.fields().unwrap(), Fields::ServerMove {
            selector: 15, values: [x, y, z], flag: 1, remaining: []
        } if x == two.x && y == SOLO_HEIGHT && z == two.y));
        out.clear();
        world.tick = 3;
        projection.append(
            &world,
            &world.entities[0],
            &visible,
            &registry,
            &[path(two)],
            &mut out,
        );
        assert!(out.is_empty());
        projection.append(
            &world,
            &world.entities[0],
            &HashSet::from([1]),
            &registry,
            &[path(one)],
            &mut out,
        );
        assert!(out.is_empty());
        world.tick = 4;
        projection.append(
            &world,
            &world.entities[0],
            &visible,
            &registry,
            &[path(one)],
            &mut out,
        );
        assert_eq!(out[0].known_opcode(), Some(Opcode::MoveTo));
    }

    #[test]
    fn alternating_team_roster_routes_all_nine_bots_using_registered_slots() {
        let mut world = World::new(73, 1);
        world.assign_teams(&[]).unwrap();
        world.use_solo_mid_lane().unwrap();
        let mut slots =
            crate::replication::slots::actor_slots::ActorSlots::from_roster(&world.entities);
        let mut registered = HashMap::new();
        for actor in world.entities.iter().take(10) {
            let spawn = slots.declaration(&world, actor, &[], None).unwrap();
            registered.insert(actor.id, spawn.body[745]);
        }
        let expected_slots: Vec<u8> = (2..=10).map(|id| registered[&id]).collect();
        assert_eq!(expected_slots, [19, 15, 20, 16, 21, 17, 22, 18, 23]);

        // Place every bot within the viewer's sight so real SoloBotPath steps cover all nine.
        let origin = Position { x: 0.0, y: 0.0 };
        world.entities[0].position = origin;
        for i in 1..10 {
            world.entities[i].position = Position {
                x: (i as f32) * 0.5,
                y: 0.0,
            };
        }
        let visible: HashSet<u32> = (1..=10).collect();
        let mut projection = Paths::default();
        let mut seen_slots = HashSet::new();
        for _ in 0..20 {
            let events = world.step();
            let mut out = Vec::new();
            projection.append(
                &world,
                &world.entities[0],
                &visible,
                &registered,
                &events,
                &mut out,
            );
            for message in out {
                assert_eq!(message.known_opcode(), Some(Opcode::MoveTo));
                seen_slots.insert(message.body[0]);
            }
            if seen_slots.len() == 9 {
                break;
            }
        }
        assert_eq!(seen_slots, expected_slots.into_iter().collect());

        // Non-hero slots outside 14..=23 remain excluded even if present in the map.
        for bad_slot in [13_u8, 24] {
            let mut bad_projection = Paths::default();
            let mut out = Vec::new();
            let bad_registered = HashMap::from([(2, bad_slot)]);
            let waypoint = Position {
                x: origin.x + 2.0,
                y: origin.y,
            };
            bad_projection.append(
                &world,
                &world.entities[0],
                &visible,
                &bad_registered,
                &[Event::SoloBotPath {
                    id: 2,
                    waypoint,
                    goal: waypoint,
                    target: None,
                }],
                &mut out,
            );
            assert!(out.is_empty(), "slot {bad_slot} must not emit bot path");
        }
    }
}
