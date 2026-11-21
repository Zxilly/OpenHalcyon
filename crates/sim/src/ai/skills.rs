//! Layer 4 skill intent selection combining data-driven 61-hero utility scoring
//! with preserved hero-specific tactical mechanics.

use super::macro_bt::{MacroBlackboard, MacroMode};
use super::threat::ThreatAssessment;
use super::utility::{
    AbilityRole, ClassifiedAbility, TargetPattern, UtilityContext, score_classified_ability,
};
use crate::AbilitySlot;
use crate::CastTarget;
use crate::Command;
use crate::Entity;
use crate::Kind;
use crate::Position;
use crate::Room;
use halcyon_data::EntityTemplate;

impl Room<'_> {
    /// Evaluates support/escape/combat abilities using Layer 4 response-curve utility
    /// while preserving all modeled hero-specific triggers.
    pub(crate) fn hero_utility_cast_order(
        &self,
        caster: usize,
        enemy: Option<&Entity>,
        threat: &ThreatAssessment,
        bb: &MacroBlackboard,
    ) -> Option<Command> {
        use AbilitySlot::{A, B, C};
        use CastTarget::SelfCast;

        let hero = &self.entities[caster];
        if hero.status.silence_until > self.tick {
            return None;
        }

        // 1. High-priority modeled hero support/signature triggers (e.g. Adagio heal, Lorelai veil).
        if hero.type_id == EntityTemplate::Adagio.as_raw()
            && let Some(heal) = self.adagio_heal_order(caster)
        {
            return Some(heal);
        }
        if let Some(enemy_entity) = enemy {
            if let Some(specific) = self.modeled_hero_specific_cast(caster, enemy_entity, bb.mode) {
                return Some(specific);
            }
        } else if bb.mode == MacroMode::Retreat
            && hero.type_id == EntityTemplate::Lorelai.as_raw()
            && hero.hp < 0.45 * hero.max_hp
            && self.can_cast(caster, C, CastTarget::Entity(hero.id))
        {
            return Some(Command::Cast {
                slot: C,
                target: CastTarget::Entity(hero.id),
            });
        }

        // 2. Data-driven 61-hero utility scoring across all primary slots [A, B, C].
        let kit = self.classified_kit_for(hero.type_id, hero.stats.range);
        let hero_hp_ratio = (hero.hp / hero.max_hp.max(1.0)).clamp(0.0, 1.0);
        let hero_energy_ratio = if hero.max_energy > 0.0 {
            (hero.energy / hero.max_energy).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let (lowest_ally_id, lowest_ally_hp_ratio, lowest_ally_dist) =
            self.lowest_ally_snapshot(caster);
        let (enemy_kind, enemy_dist, enemy_hp_ratio, enemy_cluster_count) = match enemy {
            Some(e) => (
                e.kind,
                hero.position.dist(e.position),
                (e.hp / e.max_hp.max(1.0)).clamp(0.0, 1.0),
                self.count_enemies_near(hero.team, e.position, 4.0),
            ),
            None => (Kind::Minion, f32::INFINITY, 1.0, 0),
        };

        let ctx = UtilityContext {
            hero_hp_ratio,
            hero_energy_ratio,
            hero_attack_range: hero.stats.range,
            macro_mode: bb.mode,
            threat: *threat,
            enemy_kind,
            enemy_dist,
            enemy_hp_ratio,
            enemy_cluster_count,
            lowest_ally_hp_ratio,
            lowest_ally_dist,
        };

        let mut best_cast: Option<(f32, u8, Command)> = None;
        for classified in &kit.primary {
            let slot = classified.slot;
            let kit_recast = hero
                .hero_kit
                .as_deref()
                .is_some_and(|k| self.tick < k.recast_until[slot.idx()]);
            let allow_lvl1_ult = slot == AbilitySlot::C
                && self.hero_kit_enabled()
                && crate::heroes::kits::kit_spec(hero.type_id)
                    .is_some_and(|s| s.ult_from_level_one);
            if (hero.ability_ranks[slot.idx()] == 0 && !allow_lvl1_ult)
                || (hero.cooldown[slot.idx()] > self.tick && !kit_recast)
            {
                continue;
            }
            let Some(target) =
                self.resolve_cast_target(caster, classified, enemy, lowest_ally_id, hero.id)
            else {
                continue;
            };
            if !self.can_cast(caster, slot, target) {
                continue;
            }
            let mut score = score_classified_ability(classified, &ctx);
            if enemy.is_none()
                && !classified.roles.contains(AbilityRole::Heal)
                && !classified.roles.contains(AbilityRole::Barrier)
                && !classified.roles.contains(AbilityRole::Escape)
            {
                score = 0.0;
            }
            // Fallback synergy for basic Ringo-style generic kits when no GameData was loaded.
            if enemy.is_some()
                && self.ability_intents.is_empty()
                && slot == C
                && enemy_hp_ratio < 0.35
                && enemy_dist > hero.stats.range
            {
                score = score.max(0.82);
            }
            let min_threshold = if enemy.is_none() {
                0.55
            } else if slot == C {
                0.42
            } else {
                0.28
            };
            if score < min_threshold {
                continue;
            }
            let slot_tie = slot.idx() as u8;
            let better = best_cast.is_none_or(|(best_s, best_slot, _)| {
                score
                    .total_cmp(&best_s)
                    .reverse()
                    .then_with(|| slot_tie.cmp(&best_slot))
                    == std::cmp::Ordering::Less
            });
            if better {
                best_cast = Some((score, slot_tie, Command::Cast { slot, target }));
            }
        }
        if let Some((_, _, cmd)) = best_cast {
            return Some(cmd);
        }

        // 3. Preserve generic modeled fallback casts for heroes without supplemental GameData.
        if let Some(enemy_entity) = enemy {
            let distance = hero.position.dist(enemy_entity.position);
            let target = CastTarget::Entity(enemy_entity.id);
            let cast = |eligible, slot, t| {
                (eligible && self.can_cast(caster, slot, t))
                    .then_some(Command::Cast { slot, target: t })
            };
            if bb.mode != MacroMode::Retreat {
                return cast(
                    enemy_entity.kind == Kind::Hero
                        && enemy_entity.hp < enemy_entity.max_hp * 0.35
                        && distance > hero.stats.range,
                    C,
                    target,
                )
                .or_else(|| cast(enemy_entity.kind == Kind::Hero, A, target))
                .or_else(|| {
                    cast(
                        enemy_entity.kind == Kind::Hero && distance <= hero.stats.range,
                        B,
                        SelfCast,
                    )
                });
            }
        }
        None
    }

    /// Preserves exact hero-specific combat cast rules for the 9 bespoke modeled heroes.
    fn modeled_hero_specific_cast(
        &self,
        caster: usize,
        enemy: &Entity,
        mode: MacroMode,
    ) -> Option<Command> {
        use AbilitySlot::{A, B, C};
        use CastTarget::SelfCast;

        if enemy.kind != Kind::Hero && !matches!(mode, MacroMode::ObjectiveContest) {
            return None;
        }
        let hero = &self.entities[caster];
        let distance = hero.position.dist(enemy.position);
        let point = CastTarget::Point(enemy.position);
        let cast = |eligible, slot, target| {
            (eligible && self.can_cast(caster, slot, target))
                .then_some(Command::Cast { slot, target })
        };

        // In Retreat mode, avoid diving forward with aggressive non-escape hero signatures.
        if mode == MacroMode::Retreat {
            return match EntityTemplate::from_raw(hero.type_id) {
                Some(EntityTemplate::Catherine) => cast(
                    distance <= enemy.stats.range.max(hero.stats.range) + 1.0,
                    B,
                    SelfCast,
                ),
                Some(EntityTemplate::Gwen) => cast(true, B, SelfCast),
                Some(EntityTemplate::Lorelai) => self.lorelai_action(caster, enemy.position),
                _ => None,
            };
        }

        match EntityTemplate::from_raw(hero.type_id) {
            Some(EntityTemplate::Adagio) => {
                cast(self.verse_reaches(caster, enemy.position), C, SelfCast)
            }
            Some(EntityTemplate::Lyra) => self
                .sigil_order(caster, enemy)
                .or_else(|| cast(self.bulwark_reaches(caster, enemy.position), B, SelfCast)),
            Some(EntityTemplate::Amael) => self.amael_kick_order(caster, enemy),
            Some(EntityTemplate::Karas) => cast(self.abilities.twister.is_some(), B, point),
            Some(EntityTemplate::Catherine) => {
                cast(self.pursuit_reaches(caster, enemy.position), A, SelfCast)
                    .or_else(|| {
                        cast(
                            distance <= enemy.stats.range.max(hero.stats.range) + 1.0,
                            B,
                            SelfCast,
                        )
                    })
                    .or_else(|| cast(true, C, point))
            }
            Some(EntityTemplate::Yates) => {
                cast(self.overwhelm_reaches(caster, enemy.position), B, SelfCast)
                    .or_else(|| cast(true, A, point))
                    .or_else(|| cast(true, C, point))
            }
            Some(EntityTemplate::Lorelai) => self.lorelai_action(caster, enemy.position),
            Some(EntityTemplate::Reim) => cast(
                self.chill_winds_radius(caster)
                    .is_some_and(|radius| distance <= radius),
                B,
                SelfCast,
            )
            .or_else(|| cast(true, A, point)),
            Some(EntityTemplate::Idris) => cast(true, B, point),
            _ => None,
        }
    }

    fn resolve_cast_target(
        &self,
        caster: usize,
        classified: &ClassifiedAbility,
        enemy: Option<&Entity>,
        lowest_ally_id: Option<u32>,
        self_id: u32,
    ) -> Option<CastTarget> {
        let slot = classified.slot;
        match classified.targeting {
            TargetPattern::AllyHeroOrSelf => {
                let ally_target =
                    lowest_ally_id.map_or(CastTarget::Entity(self_id), CastTarget::Entity);
                if self.can_cast(caster, slot, ally_target) {
                    return Some(ally_target);
                }
                if self.can_cast(caster, slot, CastTarget::Entity(self_id)) {
                    return Some(CastTarget::Entity(self_id));
                }
                if self.can_cast(caster, slot, CastTarget::SelfCast) {
                    return Some(CastTarget::SelfCast);
                }
                if let Some(e) = enemy
                    && self.can_cast(caster, slot, CastTarget::Point(e.position))
                {
                    return Some(CastTarget::Point(e.position));
                }
                None
            }
            TargetPattern::SelfOnly => {
                if self.can_cast(caster, slot, CastTarget::SelfCast) {
                    return Some(CastTarget::SelfCast);
                }
                if let Some(e) = enemy {
                    for cand in [CastTarget::Point(e.position), CastTarget::Entity(e.id)] {
                        if self.can_cast(caster, slot, cand) {
                            return Some(cand);
                        }
                    }
                }
                None
            }
            TargetPattern::EnemyEntity => {
                let e = enemy?;
                for cand in [
                    CastTarget::Entity(e.id),
                    CastTarget::Point(e.position),
                    CastTarget::SelfCast,
                ] {
                    if self.can_cast(caster, slot, cand) {
                        return Some(cand);
                    }
                }
                None
            }
            TargetPattern::PointOrDirection => {
                let e = enemy?;
                for cand in [
                    CastTarget::Point(e.position),
                    CastTarget::Entity(e.id),
                    CastTarget::SelfCast,
                ] {
                    if self.can_cast(caster, slot, cand) {
                        return Some(cand);
                    }
                }
                None
            }
        }
    }

    fn lowest_ally_snapshot(&self, caster: usize) -> (Option<u32>, f32, f32) {
        let hero = &self.entities[caster];
        let mut best: Option<(f32, u32, f32)> = None;
        for ally in &self.entities {
            if ally.kind != Kind::Hero || ally.team != hero.team || ally.hp <= 0.0 {
                continue;
            }
            let ratio = (ally.hp / ally.max_hp.max(1.0)).clamp(0.0, 1.0);
            let dist = hero.position.dist(ally.position);
            let better = best.is_none_or(|(br, bid, _)| {
                ratio.total_cmp(&br).then_with(|| ally.id.cmp(&bid)) == std::cmp::Ordering::Less
            });
            if better {
                best = Some((ratio, ally.id, dist));
            }
        }
        match best {
            Some((ratio, id, dist)) => (Some(id), ratio, dist),
            None => (Some(hero.id), 1.0, 0.0),
        }
    }

    fn count_enemies_near(&self, my_team: u8, center: Position, radius: f32) -> u8 {
        let mut count = 0_u8;
        for e in &self.entities {
            if e.kind == Kind::Hero
                && e.team != my_team
                && e.hp > 0.0
                && self.visible(my_team, e)
                && center.dist(e.position) <= radius
            {
                count = count.saturating_add(1);
            }
        }
        count
    }

    /// Legacy hero combat order retained for `Layout::Validation` baseline parity.
    pub(crate) fn hero_combat_order_legacy(
        &self,
        caster: usize,
        enemy: &Entity,
    ) -> Option<Command> {
        use AbilitySlot::{A, B, C};
        use CastTarget::SelfCast;
        let hero = &self.entities[caster];
        let distance = hero.position.dist(enemy.position);
        let point = CastTarget::Point(enemy.position);
        let target = CastTarget::Entity(enemy.id);
        let cast = |eligible, slot, target| {
            (eligible && self.can_cast(caster, slot, target))
                .then_some(Command::Cast { slot, target })
        };
        let specific = match EntityTemplate::from_raw(hero.type_id) {
            Some(EntityTemplate::Adagio) => {
                cast(self.verse_reaches(caster, enemy.position), C, SelfCast)
            }
            Some(EntityTemplate::Lyra) => self
                .sigil_order(caster, enemy)
                .or_else(|| cast(self.bulwark_reaches(caster, enemy.position), B, SelfCast)),
            Some(EntityTemplate::Amael) => self.amael_kick_order(caster, enemy),
            Some(EntityTemplate::Karas) => cast(self.abilities.twister.is_some(), B, point),
            Some(EntityTemplate::Catherine) => {
                cast(self.pursuit_reaches(caster, enemy.position), A, SelfCast)
                    .or_else(|| {
                        cast(
                            distance <= enemy.stats.range.max(hero.stats.range) + 1.,
                            B,
                            SelfCast,
                        )
                    })
                    .or_else(|| cast(true, C, point))
            }
            Some(EntityTemplate::Yates) => {
                cast(self.overwhelm_reaches(caster, enemy.position), B, SelfCast)
                    .or_else(|| cast(true, A, point))
                    .or_else(|| cast(true, C, point))
            }
            Some(EntityTemplate::Lorelai) => self.lorelai_action(caster, enemy.position),
            Some(EntityTemplate::Reim) => cast(
                self.chill_winds_radius(caster)
                    .is_some_and(|radius| distance <= radius),
                B,
                SelfCast,
            )
            .or_else(|| cast(true, A, point)),
            Some(EntityTemplate::Idris) => cast(true, B, point),
            _ => None,
        };
        specific
            .or_else(|| {
                cast(
                    enemy.hp < enemy.max_hp * 0.35 && distance > hero.stats.range,
                    C,
                    target,
                )
            })
            .or_else(|| cast(true, A, target))
            .or_else(|| cast(distance <= hero.stats.range, B, SelfCast))
    }
}
