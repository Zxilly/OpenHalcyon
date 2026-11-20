use super::*;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Priority {
    Preferred,
    Hero,
    Minion,
    Structure,
    Crystal,
}

impl World {
    pub(super) fn choose_target(&self, e: &Entity) -> Option<&Entity> {
        let sensor = if matches!(e.kind, Kind::Tower | Kind::Crystal) {
            e.stats.range.min(e.stats.sight())
        } else {
            e.stats.sight()
        };
        let mut candidate = None;
        let mut best = None;
        for t in &self.entities {
            if t.hp <= 0. || t.team == e.team {
                continue;
            }
            let d = e.position.dist(t.position);
            if d > sensor || !self.layout.clear(e.position, t.position) {
                continue;
            }
            // A teammate, reveal or vision device can expose a target across a brush boundary.
            if self
                .native_navigation
                .as_ref()
                .is_some_and(|map| !map.can_see(e.position, t.position) && !self.visible(e.team, t))
            {
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
                Kind::Minion if e.kind == Kind::Hero && predicted <= e.stats.weapon => {
                    Priority::Preferred
                }
                Kind::Minion if e.kind == Kind::Tower => Priority::Preferred,
                Kind::Tower | Kind::Crystal if e.kind == Kind::Minion => Priority::Preferred,
                Kind::Hero => Priority::Hero,
                Kind::Minion => Priority::Minion,
                Kind::Tower | Kind::Armory => Priority::Structure,
                Kind::Crystal => Priority::Crystal,
                Kind::Neutral => continue,
            };
            let stick = if e.order == Command::Attack(t.id) {
                0.6
            } else {
                0.
            };
            let rank = (priority, d - stick, t.id);
            if best.is_none_or(|current| rank < current) {
                best = Some(rank);
                candidate = Some(t);
            }
        }
        candidate
    }
}
