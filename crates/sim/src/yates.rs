use super::*;
use crate::native_combat::{Damage, ranks};
use halcyon_data::{AbilityParameter as P, DamageOperand};
#[derive(Clone, Debug, Serialize)]
pub(super) struct Flail {
    damage: Damage<5>,
    range: [f32; 5],
    speed: [f32; 5],
    pull: [f32; 5],
    offset: [f32; 5],
}
#[derive(Clone, Copy, Debug, Serialize)]
pub(super) struct Shot {
    position: Position,
    end: Position,
    speed: f32,
    pull: f32,
    offset: f32,
    damage: f32,
    hooked: Option<u32>,
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct Leap {
    damage: Damage<3>,
    channel: [f32; 3],
    radius: [f32; 3],
    slow: [f32; 3],
    slow_duration: [f32; 3],
    barrier: [f32; 3],
    barrier_cp: f32,
    barrier_duration: [f32; 3],
}
impl Flail {
    pub fn load(d: &GameData) -> Result<Option<Self>, String> {
        let Some(a) = d.ability("Yates", "Ability__Yates__A") else {
            return Ok(None);
        };
        Ok(Some(Self {
            damage: Damage::named(a, DamageOperand::UppercaseDamage)?,
            range: ranks(a, P::Range)?,
            speed: ranks(a, P::ShotSpeed)?,
            pull: ranks(a, P::PullSpeed)?,
            offset: ranks(a, P::PullDistance)?,
        }))
    }
}
impl Leap {
    pub fn load(d: &GameData) -> Result<Option<Self>, String> {
        let Some(a) = d.ability("Yates", "Ability__Yates__C") else {
            return Ok(None);
        };
        Ok(Some(Self {
            damage: Damage::named(a, DamageOperand::UppercaseDamage)?,
            channel: ranks(a, P::ChannelTime)?,
            radius: ranks(a, P::LandingRadius)?,
            slow: ranks(a, P::LandingSlow)?,
            slow_duration: ranks(a, P::LandingSlowDuration)?,
            barrier: ranks(a, P::LandingBarrier)?,
            barrier_cp: a
                .variable(P::LandingBarrier.as_str())
                .ok_or("missing barrier strength")?
                .cp_ratio,
            barrier_duration: ranks(a, P::LandingBarrierDuration)?,
        }))
    }
    pub fn windup(&self, r: usize) -> u64 {
        (self.channel[r] * TICK_HZ as f32).round() as u64
    }
}
impl Room<'_> {
    pub(super) fn spell_motion_clear(&self, a: Position, b: Position) -> bool {
        self.native_navigation
            .as_ref()
            .map_or_else(|| self.layout.clear(a, b), |m| m.ground_segment(a, b))
    }
    pub(super) fn place_spell_actor(&mut self, i: usize, point: Position, events: &mut Vec<Event>) {
        let e = &mut self.entities[i];
        if e.position == point {
            return;
        }
        e.position = point;
        if let Some(map) = &mut self.state.native_navigation {
            map.forget_route(e.id);
            map.moved(e.id, point);
        }
        events.push(Event::Move {
            id: e.id,
            position: point,
        });
    }
    pub(super) fn launch_flail(&mut self, i: usize, r: usize, aim: Position) {
        let a = self.state.abilities.flail.as_ref().unwrap();
        let e = &mut self.entities[i];
        let start = e.position;
        let distance = start.dist(aim);
        let scale = a.range[r] / distance;
        let end = Position {
            x: start.x + (aim.x - start.x) * scale,
            y: start.y + (aim.y - start.y) * scale,
        };
        e.status.captain_effects.arm().flail = Some(Shot {
            position: start,
            end,
            speed: a.speed[r],
            pull: a.pull[r],
            offset: a.offset[r].abs(),
            damage: a.damage.at(r, e.item_stats.crystal, 0.),
            hooked: None,
        });
    }
    pub(super) fn advance_flail(&mut self, i: usize, events: &mut Vec<Event>) {
        let Some(mut s) = self.entities[i]
            .status
            .captain_effects
            .active_mut()
            .and_then(|state| state.flail.take())
        else {
            return;
        };
        let e = &self.entities[i];
        let (team, src, origin, pierce) = (e.team, e.id, e.position, e.item_stats.shield_pierce);
        if let Some(id) = s.hooked {
            let Some(j) = self.index(id) else { return };
            let t = &self.entities[j];
            let start = t.position;
            if t.hp <= 0. {
                return;
            }
            let remaining = (start.dist(origin) - s.offset).max(0.);
            let next = start.advance(origin, (s.pull / TICK_HZ as f32).min(remaining));
            if next == start || !self.spell_motion_clear(start, next) {
                return;
            }
            self.place_spell_actor(j, next, events);
            if next.dist(origin) > s.offset + 0.001 {
                self.entities[i].status.captain_effects.arm().flail = Some(s)
            }
            return;
        }
        let next = s.position.advance(s.end, s.speed / TICK_HZ as f32);
        if !self.spell_motion_clear(s.position, next) {
            return;
        }
        // The first circle entered along the segment wins, including overlapping circles.
        let segment = s.position.dist(next);
        if segment <= 0. {
            return;
        }
        let dx = (next.x - s.position.x) / segment;
        let dy = (next.y - s.position.y) / segment;
        let hit = self
            .entities
            .iter()
            .enumerate()
            .filter_map(|(j, t)| {
                if t.team == team
                    || t.hp <= 0.
                    || !matches!(t.kind, Kind::Hero | Kind::Minion | Kind::Neutral)
                {
                    return None;
                }
                let x = t.position.x - s.position.x;
                let y = t.position.y - s.position.y;
                let along = x * dx + y * dy;
                let side = x * dy - y * dx;
                let radius = t.stats.physiology.map_or(0., |p| p.collision_radius);
                if side.abs() > radius {
                    return None;
                }
                let chord = (radius * radius - side * side).sqrt();
                let enter = (along - chord).max(0.);
                (along + chord >= 0.
                    && enter <= segment
                    && self.spell_motion_clear(s.position, t.position))
                .then_some((j, enter))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((j, _)) = hit {
            s.hooked = Some(self.entities[j].id);
            let actual = shop::mitigated(s.damage, self.entities[j].stats.shield, pierce);
            self.hurt(src, team, j, actual, DamageSchool::Crystal, events);
            if self.entities[j].hp > 0. && !self.entities[j].status.blocks_debuffs(self.tick) {
                self.entities[j].order = Command::Stop;
                self.entities[j].winding = None;
                self.entities[i].status.captain_effects.arm().flail = Some(s);
            }
        } else if next != s.end {
            s.position = next;
            self.entities[i].status.captain_effects.arm().flail = Some(s);
        }
    }
    pub(super) fn land_mandate(
        &mut self,
        i: usize,
        r: usize,
        pos: Position,
        events: &mut Vec<Event>,
    ) {
        let pos = self
            .native_navigation
            .as_ref()
            .and_then(|map| map.project(pos))
            .unwrap_or(pos);
        let a = self.abilities.leap.as_ref().unwrap();
        let e = &self.entities[i];
        let (src, team, cp, pierce) = (
            e.id,
            e.team,
            e.item_stats.crystal,
            e.item_stats.shield_pierce,
        );
        let (dmg, rad, slow, barrier) = (
            a.damage.at(r, cp, 0.),
            a.radius[r],
            1. - a.slow[r],
            a.barrier[r] + cp * a.barrier_cp,
        );
        let slow_end = self.tick + (a.slow_duration[r] * TICK_HZ as f32).round() as u64;
        let barrier_end = self.tick + (a.barrier_duration[r] * TICK_HZ as f32).round() as u64;
        self.place_spell_actor(i, pos, events);
        for j in 0..self.entities.len() {
            let t = &self.entities[j];
            if t.hp <= 0. || t.position.dist(pos) > rad || !self.spell_motion_clear(pos, t.position)
            {
                continue;
            }
            let t = &mut self.entities[j];
            if t.team == team {
                if t.kind == Kind::Hero {
                    let previous = if t.status.barrier_until > self.state.tick {
                        t.status.barrier
                    } else {
                        0.
                    };
                    t.status.barrier = barrier.max(previous);
                    t.status.barrier_until = t.status.barrier_until.max(barrier_end);
                }
            } else if matches!(t.kind, Kind::Hero | Kind::Minion | Kind::Neutral) {
                if !t.status.blocks_movement_impairment(self.state.tick) {
                    t.status.slow_until = slow_end;
                    t.status.slow_factor = slow;
                }
                let amount = shop::mitigated(dmg, t.stats.shield, pierce);
                self.hurt(src, team, j, amount, DamageSchool::Crystal, events);
            }
        }
        if self.entities[i]
            .status
            .captain_effects
            .read()
            .overwhelm
            .is_some()
        {
            self.advance_overwhelm(i, events)
        }
    }
}
