use super::*;
impl World {
    pub(super) fn update_rally(&mut self) {
        self.rally = [None; 2];
        for e in &self.entities {
            if e.kind != Kind::Minion || e.hp <= 0. {
                continue;
            }
            let p = &mut self.rally[e.team as usize];
            if p.is_none_or(|old| {
                old.dist(self.layout.home(e)) < e.position.dist(self.layout.home(e))
            }) {
                *p = Some(e.position)
            }
        }
    }
    fn retreat_step(&self, e: &Entity, home: Position) -> Position {
        let waypoint = if matches!(self.layout, Layout::SoloMid { .. }) {
            solo_side::steer(e.position, home).unwrap_or(e.position)
        } else {
            home
        };
        e.position.advance(waypoint, 1.)
    }

    pub(super) fn decide(&self, i: usize) -> Command {
        let e = &self.entities[i];
        let home = self.layout.home(e);
        let enemy = if matches!(self.layout, Layout::Aral { .. }) {
            self.aral_march_goal(e)
        } else {
            self.layout.objective(1 - e.team)
        };
        if e.kind == Kind::Hero && e.hp < e.max_hp * 0.23 {
            return Command::MoveTo(home);
        }
        let sensor = if matches!(e.kind, Kind::Tower | Kind::Crystal) {
            e.stats.range.min(e.stats.sight())
        } else {
            e.stats.sight()
        };
        let mut candidate = None;
        let mut best = (u8::MAX, f32::MAX, u32::MAX);
        for t in &self.entities {
            if t.hp <= 0. || t.team == e.team {
                continue;
            }
            let d = e.position.dist(t.position);
            if d > sensor || !self.layout.clear(e.position, t.position) {
                continue;
            }
            let hit_tick = self.tick + 3 + (d * TICK_HZ as f32 / 18.).ceil() as u64;
            let predicted = if t.kind == Kind::Minion && e.kind == Kind::Hero {
                self.predicted_hp(t, hit_tick)
            } else {
                t.hp
            };
            if predicted <= 0. {
                continue;
            }
            let priority = match t.kind {
                Kind::Minion if e.kind == Kind::Hero && predicted <= e.stats.weapon => 0,
                Kind::Minion if e.kind == Kind::Tower => 0,
                Kind::Tower | Kind::Crystal if e.kind == Kind::Minion => 0,
                Kind::Hero => 1,
                Kind::Minion => 2,
                Kind::Tower | Kind::Armory => 3,
                Kind::Crystal => 4,
                Kind::Neutral => continue,
            };
            let stick = if e.order == Command::Attack(t.id) {
                0.6
            } else {
                0.
            };
            let rank = (priority, d - stick, t.id);
            if rank < best {
                best = rank;
                candidate = Some(t);
            }
        }
        if let Some(t) = candidate {
            if e.kind == Kind::Hero
                && t.kind == Kind::Tower
                && !self.entities.iter().any(|a| {
                    a.team == e.team
                        && a.kind == Kind::Minion
                        && a.hp > 0.
                        && a.position.dist(t.position) <= t.stats.range
                })
            {
                let mut p = t.position;
                p.x += (if e.team == 0 { -1. } else { 1. }) * (t.stats.range + 1.);
                if matches!(self.layout, Layout::SoloMid { .. }) && !self.layout.walkable(p) {
                    p = self.retreat_step(e, home);
                }
                return Command::MoveTo(p);
            }
            if e.kind == Kind::Hero && t.kind == Kind::Hero {
                if e.type_id == EntityTemplate::Lyra.as_raw()
                    && self.bulwark_reaches(i, t.position)
                    && self.can_cast(i, AbilitySlot::B, CastTarget::SelfCast)
                {
                    return Command::Cast {
                        slot: AbilitySlot::B,
                        target: CastTarget::SelfCast,
                    };
                }
                if e.type_id == EntityTemplate::Catherine.as_raw()
                    && self.pursuit_reaches(i, t.position)
                    && self.can_cast(i, AbilitySlot::A, CastTarget::SelfCast)
                {
                    return Command::Cast {
                        slot: AbilitySlot::A,
                        target: CastTarget::SelfCast,
                    };
                }
                if e.type_id == EntityTemplate::Catherine.as_raw() {
                    for (slot, target) in [
                        (AbilitySlot::B, CastTarget::SelfCast),
                        (AbilitySlot::C, CastTarget::Point(t.position)),
                    ] {
                        if (slot != AbilitySlot::B
                            || e.position.dist(t.position) <= t.stats.range.max(e.stats.range) + 1.)
                            && self.can_cast(i, slot, target)
                        {
                            return Command::Cast { slot, target };
                        }
                    }
                }
                if e.type_id == EntityTemplate::Yates.as_raw()
                    && self.overwhelm_reaches(i, t.position)
                    && self.can_cast(i, AbilitySlot::B, CastTarget::SelfCast)
                {
                    return Command::Cast {
                        slot: AbilitySlot::B,
                        target: CastTarget::SelfCast,
                    };
                }
                if e.type_id == EntityTemplate::Yates.as_raw() {
                    for slot in [AbilitySlot::A, AbilitySlot::C] {
                        let target = CastTarget::Point(t.position);
                        if self.can_cast(i, slot, target) {
                            return Command::Cast { slot, target };
                        }
                    }
                }
                if e.type_id == lorelai::HERO
                    && let Some(cmd) = self.lorelai_action(i, t.position)
                {
                    return cmd;
                }
                if e.type_id == EntityTemplate::Reim.as_raw()
                    && self
                        .chill_winds_radius(i)
                        .is_some_and(|radius| e.position.dist(t.position) <= radius)
                    && self.can_cast(i, AbilitySlot::B, CastTarget::SelfCast)
                {
                    return Command::Cast {
                        slot: AbilitySlot::B,
                        target: CastTarget::SelfCast,
                    };
                }
                if e.type_id == EntityTemplate::Reim.as_raw()
                    && self.can_cast(i, AbilitySlot::A, CastTarget::Point(t.position))
                {
                    return Command::Cast {
                        slot: AbilitySlot::A,
                        target: CastTarget::Point(t.position),
                    };
                }
                if e.type_id == EntityTemplate::Idris.as_raw()
                    && self.can_cast(i, AbilitySlot::B, CastTarget::Point(t.position))
                {
                    return Command::Cast {
                        slot: AbilitySlot::B,
                        target: CastTarget::Point(t.position),
                    };
                }
                let target = CastTarget::Entity(t.id);
                let finisher =
                    t.hp < t.max_hp * 0.35 && e.position.dist(t.position) > e.stats.range;
                if finisher && self.can_cast(i, AbilitySlot::C, target) {
                    return Command::Cast {
                        slot: AbilitySlot::C,
                        target,
                    };
                }
                if self.can_cast(i, AbilitySlot::A, target) {
                    return Command::Cast {
                        slot: AbilitySlot::A,
                        target,
                    };
                }
                if e.position.dist(t.position) <= e.stats.range
                    && self.can_cast(i, AbilitySlot::B, CastTarget::SelfCast)
                {
                    return Command::Cast {
                        slot: AbilitySlot::B,
                        target: CastTarget::SelfCast,
                    };
                }
            }
            if e.kind == Kind::Hero
                && t.kind == Kind::Hero
                && e.next_attack > self.tick + 4
                && e.position.dist(t.position) < 4.
            {
                return Command::MoveTo(self.retreat_step(e, home));
            }
            return Command::Attack(t.id);
        }
        if e.kind == Kind::Hero {
            if let Some((_, point, at)) = e.last_seen
                && self.tick.saturating_sub(at) <= 2 * TICK_HZ
                && e.position.dist(point) > 1.
            {
                return Command::MoveTo(point);
            }
            if matches!(self.layout, Layout::SoloMid { .. }) {
                let side = e.solo_side;
                if let Some(front) = self
                    .entities
                    .iter()
                    .filter(|ally| {
                        ally.kind == Kind::Minion
                            && ally.team == e.team
                            && ally.hp > 0.
                            && ally.solo_side == side
                    })
                    .max_by(|a, b| a.position.dist(home).total_cmp(&b.position.dist(home)))
                {
                    return Command::MoveTo(front.position);
                }
                return Command::MoveTo(if side == 0 {
                    enemy
                } else {
                    solo_side::anchor(e.team, side)
                });
            }
            if let Some(front) = self.rally[e.team as usize] {
                return Command::MoveTo(front.advance(home, 1.5));
            }
        }
        if e.stats.speed > 0. {
            Command::MoveTo(enemy)
        } else {
            Command::Stop
        }
    }
}
