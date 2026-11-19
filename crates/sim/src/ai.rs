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
    pub(super) fn decide(&self, i: usize) -> Command {
        let e = &self.entities[i];
        let home = self.layout.home(e);
        let enemy = self.layout.objective(1 - e.team);
        if e.kind == Kind::Hero && e.hp < e.max_hp * 0.23 {
            return Command::MoveTo(home);
        }
        let sensor = if matches!(e.kind, Kind::Tower | Kind::Crystal) {
            e.stats.range.min(SIGHT)
        } else {
            SIGHT
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
                Kind::Tower => 3,
                Kind::Crystal => 4,
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
                return Command::MoveTo(p);
            }
            if e.kind == Kind::Hero && t.kind == Kind::Hero {
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
                return Command::MoveTo(e.position.advance(home, 1.));
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
