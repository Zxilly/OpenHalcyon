use super::*;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Priority {
    Preferred,
    Hero,
    Minion,
    Structure,
    Crystal,
}

impl Room<'_> {
    pub(super) fn choose_target(&self, e: &Entity) -> Option<&Entity> {
        let sensor = if matches!(e.kind, Kind::Tower | Kind::Crystal) {
            e.stats.range.min(e.stats.sight())
        } else {
            e.stats.sight()
        };
        let mut candidate = None;
        let mut best = None;
        // The final stable actor ID in the rank resolves equal-distance targets.
        self.entities.for_each_unordered(|t| {
            if t.hp <= 0. || t.team == e.team {
                return;
            }
            let d = e.position.dist(t.position);
            if d > sensor || !self.layout.clear(e.position, t.position) {
                return;
            }
            // A teammate, reveal or vision device can expose a target across a brush boundary.
            if self
                .native_navigation
                .as_ref()
                .is_some_and(|map| !map.can_see(e.position, t.position) && !self.visible(e.team, t))
            {
                return;
            }
            let hit_tick = self.tick + 3 + (d * TICK_HZ as f32 / 18.).ceil() as u64;
            let predicted = if t.kind == Kind::Minion && e.kind == Kind::Hero {
                self.predicted_hp(t, hit_tick)
            } else {
                t.hp
            };
            if predicted <= 0. {
                return;
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
                Kind::Neutral => return,
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
        });
        candidate
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(bevy_ecs::component::Component)]
    struct TestTag;
    #[derive(bevy_ecs::component::Component)]
    #[component(storage = "SparseSet")]
    struct SparseBotMemory;

    #[test]
    fn equal_rank_uses_gameplay_id_after_archetype_move_and_reads_live_position() {
        let mut world = World::new(2, 4);
        world.entities.truncate(3);
        for i in 0..3 {
            let actor = &mut world.entities[i];
            actor.team = u8::from(i != 0);
            actor.position = Position {
                x: i.min(1) as f32,
                y: 0.,
            };
        }
        let low = world.entities[1].id;
        let high = world.entities[2].id;
        let handle = world
            .entities
            .ecs
            .query::<(bevy_ecs::entity::Entity, &Entity)>()
            .iter(&world.entities.ecs)
            .find(|(_, e)| e.id == low)
            .unwrap()
            .0;
        world
            .entities
            .ecs
            .entity_mut(handle)
            .insert((TestTag, SparseBotMemory));
        // Refresh the query cache after the new archetype; the lower ID is now
        // physically visited after the higher ID in the actor-only table.
        let mut unrelated = world.entities[0].clone();
        unrelated.id = high + 1;
        world.entities.push(unrelated);
        assert_eq!(
            world.choose_target(&world.entities[0]).map(|e| e.id),
            Some(low)
        );
        world.entities[1].position.x = 1000.;
        assert_eq!(
            world.choose_target(&world.entities[0]).map(|e| e.id),
            Some(high)
        );
        assert!(!world.visible(0, &world.entities[1]));
        world.entities[1].position.x = 1.;
        assert!(world.visible(0, &world.entities[1]));
    }
}
