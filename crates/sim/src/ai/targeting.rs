//! Layer 4 deterministic target selection integrated with Layer 3 macro modes.

use super::macro_bt::MacroMode;
use super::threat::ThreatAssessment;
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
    /// Deterministic baseline target selector (used by non-hero actors, legacy validation,
    /// and direct baseline tests).
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
            if d > sensor
                || !self.layout.clear(e.position, t.position)
                || self.solo_structure_protected(t)
            {
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

    /// Macro-mode and threat-aware target selection for Layer 3 + Layer 4 hero bots.
    /// Guarantees deterministic `entity.id` tie-breaking across all candidate targets.
    pub(super) fn choose_target_modern(
        &self,
        caster: usize,
        mode: MacroMode,
        focus_entity: Option<u32>,
        threat: &ThreatAssessment,
    ) -> Option<&Entity> {
        let e = &self.entities[caster];
        let sensor = e.stats.sight();
        let mut candidate = None;
        let mut best: Option<(u8, f32, u32)> = None;

        self.entities.for_each_unordered(|t| {
            if t.hp <= 0. || t.team == e.team {
                return;
            }
            let is_focus = focus_entity == Some(t.id);
            let is_objective = Self::classify_objective(t.type_id).is_some();
            if mode == MacroMode::LanePush && t.kind == Kind::Neutral && !is_objective {
                return;
            }
            let d = e.position.dist(t.position);
            let effective_sensor = if is_focus { sensor.max(18.0) } else { sensor };
            if d > effective_sensor || self.solo_structure_protected(t) {
                return;
            }
            if d <= e.stats.sight() {
                if !self.combat_clear(e, t) {
                    return;
                }
                if self.native_navigation.as_ref().is_some_and(|map| {
                    !map.can_see(e.position, t.position) && !self.visible(e.team, t)
                }) {
                    return;
                }
            } else if !self.combat_clear(e, t) || !self.visible(e.team, t) {
                return;
            }
            let hit_tick = self.tick + 3 + (d * TICK_HZ as f32 / 18.).ceil() as u64;
            let predicted = if t.kind == Kind::Minion {
                self.predicted_hp(t, hit_tick)
            } else {
                t.hp
            };
            if predicted <= 0. {
                return;
            }

            let tier: u8 = match mode {
                MacroMode::Retreat => {
                    // Only finish an immediate lethal hero target while escaping if safe.
                    if t.kind == Kind::Hero
                        && d <= e.stats.range
                        && t.hp <= e.stats.weapon * 1.5
                        && !threat.in_enemy_turret_range
                    {
                        0
                    } else {
                        return;
                    }
                }
                MacroMode::BaseDefense => {
                    if is_focus {
                        0
                    } else if is_objective {
                        1
                    } else if t.kind == Kind::Hero {
                        2
                    } else if t.kind == Kind::Minion {
                        3
                    } else {
                        return;
                    }
                }
                MacroMode::ObjectiveContest => {
                    if t.kind == Kind::Hero && t.hp < t.max_hp * 0.45 {
                        0
                    } else if is_focus || is_objective {
                        1
                    } else if t.kind == Kind::Hero {
                        2
                    } else {
                        return;
                    }
                }
                MacroMode::Teamfight => match t.kind {
                    Kind::Hero if is_focus => 0,
                    Kind::Hero => 1,
                    Kind::Minion
                        if predicted <= e.stats.weapon && threat.lowest_ally_hp_ratio >= 0.45 =>
                    {
                        2
                    }
                    Kind::Minion => 3,
                    Kind::Tower | Kind::Armory => 4,
                    Kind::Crystal => 5,
                    Kind::Neutral => return,
                },
                MacroMode::JungleFarm => {
                    if t.kind == Kind::Hero {
                        0
                    } else if is_focus || (t.kind == Kind::Neutral && !is_objective) {
                        1
                    } else {
                        return;
                    }
                }
                MacroMode::LanePush => match t.kind {
                    Kind::Minion if predicted <= e.stats.weapon => 0,
                    Kind::Hero => 1,
                    Kind::Minion => 2,
                    Kind::Tower | Kind::Armory => 3,
                    Kind::Crystal => 4,
                    Kind::Neutral if is_objective => 5,
                    Kind::Neutral => return,
                },
            };

            let stick = if e.order == Command::Attack(t.id) {
                0.6
            } else {
                0.0
            };
            let hp_bias = if t.kind == Kind::Hero {
                (t.hp / t.max_hp.max(1.0)) * 2.0
            } else {
                0.0
            };
            let score = d - stick + hp_bias;
            let better = best.is_none_or(|(best_tier, best_score, best_id)| {
                tier.cmp(&best_tier)
                    .then_with(|| score.total_cmp(&best_score))
                    .then_with(|| t.id.cmp(&best_id))
                    == std::cmp::Ordering::Less
            });
            if better {
                best = Some((tier, score, t.id));
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
