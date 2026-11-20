use super::*;

mod skills;
mod targeting;
impl Room<'_> {
    pub(super) fn update_rally(&mut self) {
        self.rally = [None; 2];
        for e in &self.entities {
            if e.kind != Kind::Minion || e.hp <= 0. {
                continue;
            }
            let state = &mut *self.state;
            let p = &mut state.rally[e.team as usize];
            if p.is_none_or(|old| {
                old.dist(state.layout.home(e)) < e.position.dist(state.layout.home(e))
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
        if e.type_id == EntityTemplate::Adagio.as_raw()
            && let Some(heal) = self.adagio_heal_order(i)
        {
            return heal;
        }
        if e.kind == Kind::Hero && e.hp < e.max_hp * 0.23 {
            return Command::MoveTo(home);
        }
        if let Some(t) = self.choose_target(e) {
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
            if e.kind == Kind::Hero
                && t.kind == Kind::Hero
                && let Some(cast) = self.hero_combat_order(i, t)
            {
                return cast;
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
            if let Some((_, point, at)) = self.entities.last_seen(i)
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
