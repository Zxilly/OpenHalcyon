//! Deterministic 2D ORCA / RVO2 local collision avoidance via `dodgy_2d`.
//!
//! All agent and obstacle inputs are sorted strictly by stable `entity.id`
//! before invoking `dodgy_2d`. Scratch buffers live inside `SteeringState`
//! (retained on `ActorMetadata`) so steady-state ticks perform zero heap
//! allocations on the simulation side.

use super::{Command, Entity, Kind, Position, TICK_HZ};
use bevy_ecs::{entity::Entity as Handle, world::World as Ecs};
use dodgy_2d::{Agent, AvoidanceOptions, Obstacle, Vec2};
use serde::Serialize;
use std::borrow::Cow;

const DEFAULT_HERO_RADIUS: f32 = 0.7;
const MINION_RADIUS: f32 = 0.38;
const NEUTRAL_RADIUS: f32 = 0.55;
const TOWER_RADIUS: f32 = 0.65;
const CRYSTAL_RADIUS: f32 = 0.95;

const AGENT_TIME_HORIZON: f32 = 1.2;
const OBSTACLE_TIME_HORIZON: f32 = 0.75;
const QUERY_PADDING: f32 = 0.75;
const MAX_NEIGHBOR_RADIUS: f32 = 1.0;
const MAX_OBSTACLE_RADIUS: f32 = 1.0;

const HERO_RESPONSIBILITY: f32 = 0.6;
const UNIT_RESPONSIBILITY: f32 = 1.0;
const OCTAGON_SIDES: usize = 8;

#[derive(Clone, Copy, Debug)]
struct AgentEntry {
    id: u32,
    pos: Vec2,
    vel: Vec2,
    radius: f32,
    responsibility: f32,
}

#[derive(Clone, Copy, Debug)]
struct ObstacleEntry {
    id: u32,
    center: Vec2,
    radius: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
struct VelocityRecord {
    id: u32,
    last_pos: Position,
    vel: [f32; 2],
}

/// Reusable per-room ORCA steering snapshot and scratch buffers.
#[derive(Default)]
pub(super) struct SteeringState {
    in_step: bool,
    agents: Vec<AgentEntry>,
    obstacles: Vec<ObstacleEntry>,
    velocities: Vec<VelocityRecord>,
    next_velocities: Vec<VelocityRecord>,
    neighbor_scratch: Vec<Cow<'static, Agent>>,
    nearby_obstacles: Vec<Cow<'static, Obstacle>>,
    nearby_obstacle_entries: Vec<ObstacleEntry>,
    obstacle_vertex_pool: Vec<Vec<Vec2>>,
}

#[derive(Clone, Copy)]
pub(super) struct SteerRequest {
    pub id: u32,
    pub position: Position,
    pub waypoint: Position,
    pub speed: f32,
    pub radius: f32,
    pub responsibility: f32,
    pub attack_target: Option<u32>,
    pub attack_range: f32,
}

impl SteeringState {
    pub(super) fn in_step(&self) -> bool {
        self.in_step
    }

    pub(super) fn begin_step(
        &mut self,
        ecs: &Ecs,
        order: &[Handle],
        gameplay_ids: &[u32],
        tick: u64,
    ) {
        self.prepare(ecs, order, gameplay_ids, tick);
        self.in_step = true;
    }

    pub(super) fn end_step(&mut self) {
        self.in_step = false;
    }

    pub(super) fn forget(&mut self, id: u32) {
        if let Ok(idx) = self.velocities.binary_search_by_key(&id, |r| r.id) {
            self.velocities.remove(idx);
        }
    }

    pub(super) fn digest(&self, hash: &mut impl std::io::Write) {
        if self.velocities.iter().any(|r| r.vel != [0.0, 0.0]) {
            let active: Vec<_> = self
                .velocities
                .iter()
                .filter(|r| r.vel != [0.0, 0.0])
                .collect();
            serde_json::to_writer(hash, &active).expect("finite steering velocity state");
        }
    }

    pub(super) fn prepare(&mut self, ecs: &Ecs, order: &[Handle], gameplay_ids: &[u32], tick: u64) {
        self.agents.clear();
        self.obstacles.clear();
        self.next_velocities.clear();

        for &handle in order {
            let actor = ecs.get::<Entity>(handle).expect("live actor handle");
            if actor.hp <= 0.0 {
                continue;
            }
            let pos = Vec2::new(actor.position.x, actor.position.y);
            match actor.kind {
                Kind::Tower | Kind::Armory => {
                    self.obstacles.push(ObstacleEntry {
                        id: actor.id,
                        center: pos,
                        radius: TOWER_RADIUS,
                    });
                }
                Kind::Crystal => {
                    self.obstacles.push(ObstacleEntry {
                        id: actor.id,
                        center: pos,
                        radius: CRYSTAL_RADIUS,
                    });
                }
                Kind::Hero | Kind::Minion | Kind::Neutral => {
                    let vel = self
                        .velocities
                        .binary_search_by_key(&actor.id, |r| r.id)
                        .ok()
                        .and_then(|idx| {
                            let rec = self.velocities[idx];
                            (rec.last_pos == actor.position)
                                .then_some(Vec2::new(rec.vel[0], rec.vel[1]))
                        })
                        .unwrap_or(Vec2::ZERO);

                    let mobile = is_likely_mobile(actor, ecs, order, gameplay_ids, tick);
                    let responsibility = if !mobile {
                        0.0
                    } else if actor.kind == Kind::Hero {
                        HERO_RESPONSIBILITY
                    } else {
                        UNIT_RESPONSIBILITY
                    };
                    let effective_vel = if mobile { vel } else { Vec2::ZERO };

                    self.agents.push(AgentEntry {
                        id: actor.id,
                        pos,
                        vel: effective_vel,
                        radius: agent_radius(actor),
                        responsibility,
                    });
                    // Reset next tick's default velocity to zero; `record_move` populates
                    // the committed velocity only for actors that actually move this tick.
                    self.next_velocities.push(VelocityRecord {
                        id: actor.id,
                        last_pos: actor.position,
                        vel: [0.0, 0.0],
                    });
                }
            }
        }

        self.agents.sort_unstable_by_key(|a| a.id);
        self.obstacles.sort_unstable_by_key(|o| o.id);
        self.next_velocities.sort_unstable_by_key(|r| r.id);
        std::mem::swap(&mut self.velocities, &mut self.next_velocities);
    }

    pub(super) fn steer(
        &mut self,
        req: SteerRequest,
        ground_ok: &mut dyn FnMut(Position) -> bool,
    ) -> Position {
        if req.speed <= 0.0 || !req.speed.is_finite() {
            return req.position;
        }
        let current = Vec2::new(req.position.x, req.position.y);
        let goal = Vec2::new(req.waypoint.x, req.waypoint.y);
        let delta = goal - current;
        let dist = delta.length();
        if dist <= 1e-6 || !dist.is_finite() {
            return req.position;
        }

        let dt = 1.0 / TICK_HZ as f32;
        let max_step = req.speed * dt;
        let desired_speed = req.speed.min(dist * TICK_HZ as f32);
        let preferred_vel = (delta / dist) * desired_speed;

        let self_vel = self
            .agents
            .binary_search_by_key(&req.id, |a| a.id)
            .ok()
            .map_or(Vec2::ZERO, |idx| self.agents[idx].vel);

        let agent_range =
            req.speed * AGENT_TIME_HORIZON + req.radius + MAX_NEIGHBOR_RADIUS + QUERY_PADDING;
        let agent_range_sq = agent_range * agent_range;
        let obs_range =
            req.speed * OBSTACLE_TIME_HORIZON + req.radius + MAX_OBSTACLE_RADIUS + QUERY_PADDING;
        let obs_range_sq = obs_range * obs_range;

        let obstacle_margin = req.radius + 0.08;

        self.neighbor_scratch.clear();
        for other in &self.agents {
            if other.id == req.id {
                continue;
            }
            if other.pos.distance_squared(current) > agent_range_sq {
                continue;
            }
            let mut other_radius = other.radius;
            if req.attack_target == Some(other.id) {
                let max_allowed = req.attack_range - 0.15 - req.radius;
                if max_allowed < 0.1 {
                    continue;
                }
                other_radius = other_radius.min(max_allowed);
            }
            let safe_agent = safe_neighbor_for_orca(
                req.id,
                current,
                self_vel,
                req.radius,
                other.id,
                other.pos,
                other.vel,
                other_radius,
                other.responsibility,
                dt,
            );
            self.neighbor_scratch.push(Cow::Owned(safe_agent));
        }

        self.nearby_obstacles.clear();
        self.nearby_obstacle_entries.clear();
        for obs in &self.obstacles {
            if obs.center.distance_squared(current) > obs_range_sq {
                continue;
            }
            let effective_radius = if req.attack_target == Some(obs.id) {
                let max_allowed = req.attack_range - obstacle_margin - 0.15;
                if max_allowed < 0.15 {
                    continue;
                }
                obs.radius.min(max_allowed)
            } else {
                obs.radius
            };
            self.nearby_obstacle_entries.push(ObstacleEntry {
                id: obs.id,
                center: obs.center,
                radius: effective_radius,
            });
            let mut vertices = self
                .obstacle_vertex_pool
                .pop()
                .unwrap_or_else(|| Vec::with_capacity(OCTAGON_SIDES));
            fill_octagon(&mut vertices, obs.center, effective_radius);
            self.nearby_obstacles
                .push(Cow::Owned(Obstacle::Closed { vertices }));
        }

        if self.neighbor_scratch.is_empty() && self.nearby_obstacles.is_empty() {
            return advance_pos(req.position, req.waypoint, max_step);
        }

        let self_agent = Agent {
            position: current,
            velocity: self_vel,
            radius: req.radius,
            avoidance_responsibility: req.responsibility.max(0.1),
        };
        let options = AvoidanceOptions {
            obstacle_margin,
            time_horizon: AGENT_TIME_HORIZON,
            obstacle_time_horizon: OBSTACLE_TIME_HORIZON,
        };
        let steered_preferred = if self.nearby_obstacle_entries.is_empty() {
            preferred_vel
        } else {
            steer_around_blocking_obstacle(
                current,
                goal,
                preferred_vel,
                desired_speed,
                obstacle_margin,
                &self.nearby_obstacle_entries,
                ground_ok,
            )
        };
        let mut avoided = self_agent.compute_avoiding_velocity(
            &self.neighbor_scratch,
            &self.nearby_obstacles,
            steered_preferred,
            req.speed,
            dt,
            &options,
        );
        // Break symmetric head-on stagnation while still enforcing all ORCA constraints.
        if dist > max_step && avoided.length_squared() < (0.05 * desired_speed).powi(2) {
            let sign = if req.id.is_multiple_of(2) {
                1.0_f32
            } else {
                -1.0_f32
            };
            let sidestep = (steered_preferred + steered_preferred.perp() * (0.85 * sign))
                .normalize_or_zero()
                * desired_speed;
            if sidestep.length_squared() > 0.0 {
                let alt = self_agent.compute_avoiding_velocity(
                    &self.neighbor_scratch,
                    &self.nearby_obstacles,
                    sidestep,
                    req.speed,
                    dt,
                    &options,
                );
                if alt.length_squared() > avoided.length_squared() {
                    avoided = alt;
                }
            }
        }

        for obs_cow in self.nearby_obstacles.drain(..) {
            if let Cow::Owned(Obstacle::Closed { mut vertices }) = obs_cow {
                vertices.clear();
                self.obstacle_vertex_pool.push(vertices);
            }
        }

        if !avoided.is_finite() {
            avoided = preferred_vel;
        }
        let avoided_speed = avoided.length();
        if avoided_speed > req.speed && avoided_speed > 0.0 {
            avoided *= req.speed / avoided_speed;
        }

        if dist <= max_step && avoided.distance_squared(preferred_vel) <= 1e-4 {
            return req.waypoint;
        }

        let step = avoided * dt;
        if step.length_squared() <= 1e-12 {
            return req.position;
        }
        Position {
            x: current.x + step.x,
            y: current.y + step.y,
        }
    }

    pub(super) fn record_move(&mut self, id: u32, old_pos: Position, new_pos: Position) {
        let vel = [
            (new_pos.x - old_pos.x) * TICK_HZ as f32,
            (new_pos.y - old_pos.y) * TICK_HZ as f32,
        ];
        match self.velocities.binary_search_by_key(&id, |r| r.id) {
            Ok(idx) => {
                self.velocities[idx].last_pos = new_pos;
                self.velocities[idx].vel = vel;
            }
            Err(idx) => {
                self.velocities.insert(
                    idx,
                    VelocityRecord {
                        id,
                        last_pos: new_pos,
                        vel,
                    },
                );
            }
        }
    }
}

pub(super) fn agent_radius(actor: &Entity) -> f32 {
    match actor.kind {
        Kind::Hero => actor
            .stats
            .physiology
            .map(|p| p.collision_radius)
            .filter(|r| *r > 0.0)
            .unwrap_or(DEFAULT_HERO_RADIUS),
        Kind::Minion => MINION_RADIUS,
        Kind::Neutral => NEUTRAL_RADIUS,
        Kind::Tower | Kind::Armory => TOWER_RADIUS,
        Kind::Crystal => CRYSTAL_RADIUS,
    }
}

pub(super) fn agent_responsibility(actor: &Entity) -> f32 {
    if actor.kind == Kind::Hero {
        HERO_RESPONSIBILITY
    } else {
        UNIT_RESPONSIBILITY
    }
}

fn is_likely_mobile(
    actor: &Entity,
    ecs: &Ecs,
    order: &[Handle],
    gameplay_ids: &[u32],
    tick: u64,
) -> bool {
    if actor.stats.speed <= 0.0
        || actor.status.stun_until > tick
        || actor.status.root_until > tick
        || actor.status.channel_until >= tick
        || actor.cast.is_some()
        || actor.winding.is_some()
    {
        return false;
    }
    match actor.order {
        Command::MoveTo(goal) => actor.position.dist(goal) > 1e-4,
        Command::Attack(target_id) => {
            if let Ok(idx) = gameplay_ids.binary_search(&target_id)
                && let Some(target) = ecs.get::<Entity>(order[idx])
                && target.id == target_id
            {
                return target.hp > 0.0 && actor.position.dist(target.position) > actor.stats.range;
            }
            false
        }
        _ => false,
    }
}

fn fill_octagon(out: &mut Vec<Vec2>, center: Vec2, radius: f32) {
    out.clear();
    for k in 0..OCTAGON_SIDES {
        let angle = (k as f32) * (std::f32::consts::TAU / OCTAGON_SIDES as f32);
        let (sin, cos) = angle.sin_cos();
        out.push(Vec2::new(center.x + cos * radius, center.y + sin * radius));
    }
}

#[allow(clippy::too_many_arguments)]
fn safe_neighbor_for_orca(
    self_id: u32,
    self_pos: Vec2,
    self_vel: Vec2,
    self_radius: f32,
    nbr_id: u32,
    mut nbr_pos: Vec2,
    mut nbr_vel: Vec2,
    nbr_radius: f32,
    nbr_resp: f32,
    time_step: f32,
) -> Agent {
    let rel_pos = nbr_pos - self_pos;
    let sum_r = self_radius + nbr_radius;
    if rel_pos.length_squared() <= sum_r * sum_r {
        // dodgy_2d falls back to `rand::random()` if `(self_vel - nbr_vel) == (nbr_pos - self_pos) / dt`.
        // Inject a tiny deterministic separation derived from the ordered ID pair so RNG is never touched.
        let (lo, hi, sign) = if self_id < nbr_id {
            (self_id, nbr_id, 1.0_f32)
        } else {
            (nbr_id, self_id, -1.0_f32)
        };
        let hash = lo.wrapping_mul(2_654_435_761) ^ hi.wrapping_mul(2_246_822_519);
        let angle = ((hash & 0xfff) as f32) * (std::f32::consts::TAU / 4096.0);
        let (sin, cos) = angle.sin_cos();
        let dir = Vec2::new(cos, sin) * sign;

        if rel_pos.length_squared() <= 1e-8 {
            nbr_pos += dir * 1e-3;
        }
        let cutoff_center = (nbr_pos - self_pos) / time_step;
        let rel_vel = self_vel - nbr_vel;
        if (rel_vel - cutoff_center).length_squared() <= 1e-8 {
            nbr_vel -= dir * 1e-3;
        }
    }
    Agent {
        position: nbr_pos,
        velocity: nbr_vel,
        radius: nbr_radius,
        avoidance_responsibility: nbr_resp,
    }
}

fn advance_pos(from: Position, to: Position, max_step: f32) -> Position {
    from.advance(to, max_step)
}

#[allow(clippy::too_many_arguments)]
fn steer_around_blocking_obstacle(
    current: Vec2,
    goal: Vec2,
    preferred_vel: Vec2,
    speed: f32,
    obstacle_margin: f32,
    nearby_obstacles: &[ObstacleEntry],
    ground_ok: &mut dyn FnMut(Position) -> bool,
) -> Vec2 {
    let to_goal = goal - current;
    let dist_to_goal = to_goal.length();
    if dist_to_goal <= 1e-5 {
        return preferred_vel;
    }
    let dir = to_goal / dist_to_goal;
    let lookahead = dist_to_goal.min(speed * OBSTACLE_TIME_HORIZON + 1.5);

    let mut nearest_hit: Option<(f32, Vec2, f32)> = None;
    for obs in nearby_obstacles {
        let clearance = obs.radius + obstacle_margin + 0.06;
        let rel = obs.center - current;
        let proj = rel.dot(dir);
        if proj <= -clearance || proj >= lookahead {
            continue;
        }
        let closest_on_ray = current + dir * proj.clamp(0.0, dist_to_goal);
        let perp_dist_sq = closest_on_ray.distance_squared(obs.center);
        if perp_dist_sq < clearance * clearance
            && nearest_hit.is_none_or(|(best_proj, _, _)| proj < best_proj)
        {
            nearest_hit = Some((proj, obs.center, clearance));
        }
    }

    let Some((_, center, clearance)) = nearest_hit else {
        return preferred_vel;
    };

    let rel_center = center - current;
    let cross = dir.perp_dot(rel_center);
    let default_sign = if cross >= 0.0 { -1.0_f32 } else { 1.0_f32 };
    let left = dir.perp();
    let probe_side = |sign: f32, ground_ok: &mut dyn FnMut(Position) -> bool| -> bool {
        let p1 = center + left * (sign * (clearance + 0.4));
        let p2 = center + left * (sign * (clearance + 0.8));
        ground_ok(Position { x: p1.x, y: p1.y }) && ground_ok(Position { x: p2.x, y: p2.y })
    };
    let default_clear = probe_side(default_sign, ground_ok);
    let side_sign = if !default_clear && probe_side(-default_sign, ground_ok) {
        -default_sign
    } else {
        default_sign
    };
    let tangent = left * side_sign;
    let away = (current - center).normalize_or_zero();
    let blended = if current.distance_squared(center) <= clearance * clearance {
        (tangent * 1.4 + away * 0.6).normalize_or_zero()
    } else {
        let dist = rel_center.length().max(clearance + 0.01);
        let sin_a = (clearance / dist).clamp(0.0, 0.99);
        let cos_a = (1.0 - sin_a * sin_a).sqrt();
        (dir * cos_a + tangent * sin_a).normalize_or_zero()
    };
    if blended.length_squared() > 0.0 {
        blended * speed
    } else {
        preferred_vel
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BotDiagnostics, World};

    #[test]
    fn steering_opt_in_gating_and_entity_layout_stay_lean() {
        let mut validation = World::new(8, 10);
        assert!(!validation.steering_active());
        validation.set_steering(true);
        assert!(validation.steering_active());

        let mut solo = World::new(8, 10);
        solo.use_solo_mid_lane().unwrap();
        assert!(solo.steering_active());

        let mut aral = World::new(8, 6);
        aral.use_six_human_roster().unwrap();
        aral.use_aral_lane().unwrap();
        assert!(aral.steering_active());
        assert!(!std::mem::needs_drop::<SteerRequest>());
    }

    #[test]
    fn crowd_avoidance_separates_heroes_and_clears_turret_without_deadlock() {
        let mut w = World::new(8, 10);
        w.use_solo_mid_lane().unwrap();

        let team0_heroes: Vec<usize> = w
            .entities
            .iter()
            .enumerate()
            .filter(|(_, e)| e.kind == Kind::Hero && e.team == 0)
            .map(|(i, _)| i)
            .take(4)
            .collect();
        for idx in 0..w.entities.len() {
            let e = &mut w.entities[idx];
            if e.kind == Kind::Hero && !team0_heroes.contains(&idx) {
                e.hp = 0.0;
            }
        }

        // Line 4 heroes behind friendly turret (-53.04, 0.0) marching straight down mid lane.
        let starts = [
            Position { x: -64.0, y: 0.0 },
            Position { x: -65.2, y: 0.2 },
            Position { x: -65.2, y: -0.2 },
            Position { x: -66.4, y: 0.0 },
        ];
        let dest = Position { x: -38.0, y: 0.0 };
        let ids: Vec<u32> = team0_heroes
            .iter()
            .enumerate()
            .map(|(k, &idx)| {
                w.entities[idx].position = starts[k];
                w.entities[idx].id
            })
            .collect();
        for &id in &ids {
            w.command(id, Command::MoveTo(dest)).unwrap();
        }

        let turret_pos = Position { x: -53.04, y: 0.0 };
        let mut min_turret_dist = f32::INFINITY;
        for _ in 0..260 {
            w.step();
            for &idx in &team0_heroes {
                let p = w.entities[idx].position;
                min_turret_dist = min_turret_dist.min(p.dist(turret_pos));
                assert!(w.state.layout.walkable(p), "stepped off ground: {p:?}");
            }
        }

        // All 4 heroes passed well beyond turret (-53.04, 0.0) without intersecting its core
        // and settled around dest without collapsing onto the exact same point.
        assert!(
            min_turret_dist > 1.15,
            "heroes clipped turret center: {min_turret_dist}"
        );
        for &idx in &team0_heroes {
            let p = w.entities[idx].position;
            assert!(
                p.x > -43.0,
                "hero {} failed to advance: {p:?}",
                w.entities[idx].id
            );
        }
        for a in 0..team0_heroes.len() {
            for b in (a + 1)..team0_heroes.len() {
                let d = w.entities[team0_heroes[a]]
                    .position
                    .dist(w.entities[team0_heroes[b]].position);
                assert!(d > 0.45, "heroes stacked at distance {d}");
            }
        }
    }

    #[test]
    fn head_on_symmetric_heroes_sidestep_and_swap_positions() {
        let mut w = World::new(8, 10);
        w.use_solo_mid_lane().unwrap();
        let a = w
            .entities
            .iter()
            .position(|e| e.kind == Kind::Hero && e.team == 0)
            .unwrap();
        let b = w
            .entities
            .iter()
            .position(|e| e.kind == Kind::Hero && e.team == 1)
            .unwrap();
        for idx in 0..w.entities.len() {
            if idx != a && idx != b && w.entities[idx].kind == Kind::Hero {
                w.entities[idx].hp = 0.0;
            }
        }
        let left = Position { x: -8.0, y: 0.0 };
        let right = Position { x: 8.0, y: 0.0 };
        w.entities[a].position = left;
        w.entities[b].position = right;
        let id_a = w.entities[a].id;
        let id_b = w.entities[b].id;
        w.command(id_a, Command::MoveTo(right)).unwrap();
        w.command(id_b, Command::MoveTo(left)).unwrap();

        let mut max_lateral = 0.0f32;
        let mut min_pair_dist = f32::INFINITY;
        for _ in 0..180 {
            w.step();
            let pa = w.entities[a].position;
            let pb = w.entities[b].position;
            max_lateral = max_lateral.max(pa.y.abs()).max(pb.y.abs());
            min_pair_dist = min_pair_dist.min(pa.dist(pb));
        }

        assert!(max_lateral > 0.2, "no lateral ORCA sidestep: {max_lateral}");
        assert!(min_pair_dist > 0.4, "head-on overlap: {min_pair_dist}");
        assert!(w.entities[a].position.dist(right) < 1.5);
        assert!(w.entities[b].position.dist(left) < 1.5);
    }

    #[test]
    fn overlapping_zero_velocity_agents_never_touch_rng_and_match_digest() {
        let run_once = || {
            let mut w = World::new(3, 10);
            w.set_steering(true);
            for idx in 2..w.entities.len() {
                w.entities[idx].hp = 0.0;
            }
            let same = Position { x: 0.0, y: 0.0 };
            w.entities[0].position = same;
            w.entities[1].position = same;
            let id0 = w.entities[0].id;
            let id1 = w.entities[1].id;
            let goal = Position { x: 8.0, y: 0.0 };
            w.command(id0, Command::MoveTo(goal)).unwrap();
            w.command(id1, Command::MoveTo(goal)).unwrap();
            for _ in 0..80 {
                w.step();
            }
            (
                w.state_digest(),
                w.entities[0].position,
                w.entities[1].position,
            )
        };

        let baseline = run_once();
        assert!(
            baseline.1.dist(baseline.2) > 0.4,
            "overlapping zero-vel agents failed to separate: {:?} {:?}",
            baseline.1,
            baseline.2
        );
        for run in 1..8 {
            assert_eq!(
                run_once(),
                baseline,
                "overlapping tiebreak diverged on run {run}"
            );
        }
    }

    #[test]
    fn large_melee_hero_reaches_attack_range_of_enemy_turret() {
        let mut w = World::new(8, 10);
        w.use_solo_mid_lane().unwrap();

        let hero_idx = w
            .entities
            .iter()
            .position(|e| e.kind == Kind::Hero && e.team == 0)
            .unwrap();
        let turret_idx = w
            .entities
            .iter()
            .position(|e| e.kind == Kind::Tower && e.team == 1 && e.position.y == 0.0)
            .unwrap();
        let turret_id = w.entities[turret_idx].id;
        let turret_pos = w.entities[turret_idx].position;

        // Simulate Phinn-sized melee hero (`collision_radius = 0.95`, melee `range = 1.5`).
        w.entities[hero_idx].stats.range = 1.5;
        if let Some(p) = &mut w.entities[hero_idx].stats.physiology {
            p.collision_radius = 0.95;
        }
        w.entities[hero_idx].position = Position {
            x: turret_pos.x - 5.0,
            y: 0.0,
        };
        let hero_id = w.entities[hero_idx].id;
        w.command(hero_id, Command::Attack(turret_id)).unwrap();

        for _ in 0..80 {
            w.step();
            if w.entities[hero_idx].position.dist(turret_pos) <= 1.5 {
                break;
            }
        }
        assert!(
            w.entities[hero_idx].position.dist(turret_pos) <= 1.5,
            "melee hero stopped at dist={}",
            w.entities[hero_idx].position.dist(turret_pos)
        );
    }

    #[test]
    fn solo_mid_and_aral_determinism_and_zero_sustained_bot_stalls() {
        let run_solo = |seed| {
            let mut w = World::new(seed, 0);
            w.use_solo_mid_lane().unwrap();
            let mut diag = BotDiagnostics::new(&w);
            let mut digests = Vec::new();
            for tick in 1..=900 {
                w.step_diagnosed(&mut diag);
                if matches!(tick, 1 | 300 | 600 | 900) {
                    digests.push(w.state_digest());
                }
            }
            (digests, diag)
        };

        let (solo_a, diag_a) = run_solo(17);
        let (solo_b, _) = run_solo(17);
        assert_eq!(solo_a, solo_b);
        for (id, row) in diag_a.heroes() {
            assert_eq!(
                row.stalled_walk_ticks, 0,
                "bot {id} stalled for {} ticks (longest {})",
                row.stalled_walk_ticks, row.longest_stall_ticks
            );
        }

        let run_aral = |seed| {
            let mut w = World::new(seed, 6);
            w.use_six_human_roster().unwrap();
            w.use_aral_lane().unwrap();
            let mut digests = Vec::new();
            for tick in 1..=900 {
                w.step();
                if matches!(tick, 1 | 300 | 600 | 900) {
                    digests.push(w.state_digest());
                }
            }
            digests
        };
        assert_eq!(run_aral(17), run_aral(17));
    }
}
