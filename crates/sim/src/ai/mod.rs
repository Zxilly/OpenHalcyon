use crate::Command;
use crate::Entity;
use crate::Kind;
use crate::Position;
use crate::Room;
use crate::TICK_HZ;
use crate::ecs::rules::Layout;
use halcyon_data::EntityTemplate;

pub(crate) mod bot_diagnostics;
pub mod macro_bt;
mod skills;
mod targeting;
pub mod threat;
pub mod utility;

pub use bot_diagnostics::{BotDiagnostics, BotHeroDiagnostics};
pub(crate) use macro_bt::{BotBrains, MacroBlackboard};
pub use macro_bt::{MacroMode, ObjectiveInfo, ObjectiveKind};
pub use threat::ThreatAssessment;
pub use utility::{
    AbilityRole, ActiveItemCategory, ActiveItemIntent, ClassifiedAbility, HeroIntentCatalog,
    HeroKitIntent, TargetPattern,
};

impl Room<'_> {
    pub(crate) fn update_rally(&mut self) {
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
        let waypoint = if self.map_5v5.is_none() && matches!(self.layout, Layout::SoloMid { .. }) {
            crate::maps::solo_side::steer(e.position, home).unwrap_or(e.position)
        } else {
            home
        };
        e.position.advance(waypoint, 1.)
    }

    pub(crate) fn decide(&self, i: usize) -> Command {
        if !self.uses_modern_ai() || self.entities[i].kind != Kind::Hero {
            return self.decide_legacy(i);
        }
        let threat = self.evaluate_threat(i);
        let bb = self.evaluate_macro_bt_readonly(i, &threat);
        self.decide_with_context(i, &bb, &threat)
    }

    pub(crate) fn decide_with_context(
        &self,
        i: usize,
        bb: &MacroBlackboard,
        threat: &ThreatAssessment,
    ) -> Command {
        let e = &self.entities[i];
        let home = self.layout.home(e);
        let enemy_goal = if matches!(
            self.layout,
            Layout::Aral { .. } | Layout::HalcyonFold { .. } | Layout::Blitz { .. }
        ) {
            self.aral_march_goal(e)
        } else {
            self.layout.objective(1 - e.team)
        };

        // Always allow high-priority support healing (e.g. Adagio A) before movement.
        if e.type_id == EntityTemplate::Adagio.as_raw()
            && let Some(heal) = self.adagio_heal_order(i)
        {
            return heal;
        }

        if bb.mode == MacroMode::Retreat {
            if self.map_5v5.is_some()
                && !threat.in_enemy_turret_range
                && threat.nearby_enemy_heroes == 0
            {
                if self
                    .hero_5v5_status(e.id)
                    .is_some_and(|s| s.recalling_until.is_some())
                {
                    return Command::Stop;
                }
                if bb.recall_started_tick.is_some()
                    && e.status.stun_until <= self.tick
                    && e.status.root_until <= self.tick
                    && e.status.silence_until <= self.tick
                {
                    return Command::Recall;
                }
            }
            if let Some(t) = self.choose_target_modern(i, bb.mode, bb.focus_entity, threat) {
                if let Some(cast) = self.hero_utility_cast_order(i, Some(t), threat, bb) {
                    return cast;
                }
                return Command::Attack(t.id);
            }
            if let Some(escape_cast) = self.hero_utility_cast_order(i, None, threat, bb) {
                return escape_cast;
            }
            if threat.in_enemy_turret_range && !threat.allied_minion_tanking_turret {
                return Command::MoveTo(self.safe_turret_exit_point(
                    e,
                    threat.nearest_turret_pos,
                    threat.nearest_turret_range,
                    home,
                ));
            }
            return Command::MoveTo(home);
        }

        if let Some(t) = self.choose_target_modern(i, bb.mode, bb.focus_entity, threat) {
            if t.kind == Kind::Tower
                && !self.entities.iter().any(|a| {
                    a.team == e.team
                        && a.kind == Kind::Minion
                        && a.hp > 0.
                        && a.position.dist(t.position) <= t.stats.range
                })
            {
                return Command::MoveTo(self.safe_turret_exit_point(
                    e,
                    t.position,
                    t.stats.range,
                    home,
                ));
            }
            // Avoid taking undefended enemy turret shots when injured or chasing out of range.
            if threat.in_enemy_turret_range
                && !threat.allied_minion_tanking_turret
                && !(t.kind == Kind::Hero && t.hp < t.max_hp * 0.22)
                && (e.hp < e.max_hp * 0.55 || e.position.dist(t.position) > e.stats.range)
            {
                let has_cast = (t.kind == Kind::Hero || bb.mode == MacroMode::ObjectiveContest)
                    && self
                        .hero_utility_cast_order(i, Some(t), threat, bb)
                        .is_some();
                if e.hp < e.max_hp * 0.55 || !has_cast {
                    return Command::MoveTo(self.safe_turret_exit_point(
                        e,
                        threat.nearest_turret_pos,
                        threat.nearest_turret_range,
                        home,
                    ));
                }
            }

            if (t.kind == Kind::Hero || bb.mode == MacroMode::ObjectiveContest)
                && let Some(cast) = self.hero_utility_cast_order(i, Some(t), threat, bb)
            {
                return cast;
            }
            if t.kind == Kind::Hero
                && e.next_attack > self.tick + 4
                && e.position.dist(t.position) < 4.
            {
                return Command::MoveTo(self.safe_kite_point(e, threat, home));
            }
            return Command::Attack(t.id);
        }

        if let Some(support_cast) = self.hero_utility_cast_order(i, None, threat, bb) {
            return support_cast;
        }

        // If Layer 3 directed us to base defense, objective contest, teamfight, or camp farm, move to anchor.
        if matches!(
            bb.mode,
            MacroMode::BaseDefense
                | MacroMode::ObjectiveContest
                | MacroMode::Teamfight
                | MacroMode::JungleFarm
        ) && e.position.dist(bb.anchor) > 1.0
            && self.is_position_walkable(bb.anchor)
        {
            return Command::MoveTo(bb.anchor);
        }

        if let Some((_, point, at)) = self.entities.last_seen(i)
            && self.tick.saturating_sub(at) <= 2 * TICK_HZ
            && e.position.dist(point) > 1.
            && self.is_position_walkable(point)
            && !self.point_inside_undefended_enemy_turret(e.team, point)
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
                .max_by(|a, b| {
                    a.position
                        .dist(home)
                        .total_cmp(&b.position.dist(home))
                        .then_with(|| a.id.cmp(&b.id))
                })
            {
                return Command::MoveTo(front.position);
            }
            return Command::MoveTo(if side == 0 {
                enemy_goal
            } else {
                crate::maps::solo_side::anchor(e.team, side)
            });
        }

        if let Some(front) = self.rally[e.team as usize] {
            let dir = if e.team == 0 { 1.0 } else { -1.0 };
            let forward_rally = !matches!(
                self.layout,
                Layout::Aral { .. } | Layout::HalcyonFold { .. } | Layout::Blitz { .. }
            ) || (dir * front.x >= dir * e.position.x - 6.0
                && front.dist(home) > 25.0);
            let candidate = front.advance(home, 1.5);
            if forward_rally && self.is_position_walkable(candidate) {
                return Command::MoveTo(candidate);
            }
        }

        if e.stats.speed > 0. {
            Command::MoveTo(enemy_goal)
        } else {
            Command::Stop
        }
    }

    fn decide_legacy(&self, i: usize) -> Command {
        let e = &self.entities[i];
        let home = self.layout.home(e);
        let enemy = if matches!(
            self.layout,
            Layout::Aral { .. } | Layout::HalcyonFold { .. } | Layout::Blitz { .. }
        ) {
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
                && let Some(cast) = self.hero_combat_order_legacy(i, t)
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
                    crate::maps::solo_side::anchor(e.team, side)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BasicItem;
    use crate::Event;
    use crate::World;
    use halcyon_data::BotDifficulty;

    #[test]
    fn modern_ai_zero_divergence_state_digest_across_solomid_aral_and_opted_in_validation() {
        // 1. SoloMid with Hard difficulty (3-tick BT cadence)
        let mut s1 = World::new(11, 0);
        let mut s2 = World::new(11, 0);
        s1.use_solo_mid_lane().unwrap();
        s2.use_solo_mid_lane().unwrap();
        s1.set_bot_difficulty(Some(BotDifficulty::Hard));
        s2.set_bot_difficulty(Some(BotDifficulty::Hard));
        for tick in 1..=1_500 {
            assert_eq!(s1.step(), s2.step(), "solomid events at tick {tick}");
            if tick % 250 == 0 {
                assert_eq!(
                    s1.state_digest(),
                    s2.state_digest(),
                    "solomid digest at {tick}"
                );
            }
        }
        assert_eq!(s1.state_digest(), s2.state_digest());

        // 2. ARAL with VeryHard difficulty (2-tick BT cadence)
        let mut a1 = World::new(19, 6);
        let mut a2 = World::new(19, 6);
        a1.use_six_human_roster().unwrap();
        a2.use_six_human_roster().unwrap();
        for w in [&mut a1, &mut a2] {
            for idx in 0..w.entities.len() {
                if w.entities[idx].kind == Kind::Hero {
                    w.entities[idx].bot = true;
                }
            }
            w.use_aral_lane().unwrap();
            w.set_bot_difficulty(Some(BotDifficulty::VeryHard));
        }
        for tick in 1..=1_500 {
            assert_eq!(a1.step(), a2.step(), "aral events at tick {tick}");
            if tick % 250 == 0 {
                assert_eq!(
                    a1.state_digest(),
                    a2.state_digest(),
                    "aral digest at {tick}"
                );
            }
        }
        assert_eq!(a1.state_digest(), a2.state_digest());

        // 3. Opted-in Validation with Medium difficulty (5-tick BT cadence)
        let mut v1 = World::new(5, 0);
        let mut v2 = World::new(5, 0);
        for w in [&mut v1, &mut v2] {
            w.set_modern_ai(true);
            w.set_bot_difficulty(Some(BotDifficulty::Medium));
        }
        for tick in 1..=1_500 {
            assert_eq!(v1.step(), v2.step(), "validation events at tick {tick}");
        }
        assert_eq!(v1.state_digest(), v2.state_digest());
    }

    #[test]
    fn modern_ai_performance_sanity_with_bot_diagnostics() {
        let mut w = World::new(23, 0);
        w.use_solo_mid_lane().unwrap();
        w.set_bot_difficulty(Some(BotDifficulty::VeryHard));
        let mut diag = BotDiagnostics::new(&w);
        let started = std::time::Instant::now();
        for _ in 0..1_000 {
            w.step_diagnosed(&mut diag);
        }
        let elapsed = started.elapsed();
        assert_eq!(diag.heroes().len(), 10);
        for row in diag.heroes().values() {
            assert!(row.decisions >= 400);
            assert!(
                row.rejected_orders.is_empty(),
                "unexpected rejected orders: {:?}",
                row.rejected_orders
            );
        }
        assert!(
            elapsed < std::time::Duration::from_secs(10),
            "1000 VeryHard SoloMid ticks took {elapsed:?}"
        );
    }

    #[test]
    fn cross_feature_61_hero_kits_active_items_5v5_recall_and_objectives_work_deterministically() {
        // 1. Batch B HeroKit (61-hero kit + level-1 C) + AI utility + Command::ActivateItem (`Aegis`).
        let run_hero_and_item = || {
            let mut w = World::new(31, 0);
            w.use_solo_mid_lane().unwrap();
            w.set_bot_difficulty(Some(BotDifficulty::VeryHard));
            w.enable_builtin_hero_kits();

            w.entities[0].type_id = EntityTemplate::Malene.as_raw();
            w.entities[0].ability_ranks = [1, 1, 0];
            w.entities[0].energy = 400.0;
            w.entities[0].max_energy = 400.0;
            w.entities[0].hp = w.entities[0].max_hp * 0.50;
            w.entities[0].status.slow_until = 50;
            w.entities[0].status.slow_factor = 0.75;
            w.entities[0].gold = 5000;
            w.shop.enable_all_store_for_test();
            w.purchase(1, BasicItem::Aegis).unwrap();
            w.entities[0].position = Position { x: 17.0, y: 0.0 };
            w.entities[1].position = Position { x: 20.0, y: 0.0 };
            // Place a second enemy hero close by to guarantee high teamfight threat for Aegis.
            w.entities[3].position = Position { x: 20.5, y: 0.5 };

            let mut saw_cast = false;
            let mut saw_item = false;
            for _ in 0..20 {
                for ev in w.step() {
                    if matches!(ev, Event::Cast { source: 1, .. }) {
                        saw_cast = true;
                    }
                    if matches!(
                        ev,
                        Event::ItemActivated {
                            actor: 1,
                            kind: BasicItem::Aegis,
                            ..
                        }
                    ) {
                        saw_item = true;
                    }
                }
            }
            (saw_cast, saw_item, w.state_digest())
        };
        let (cast_a, item_a, dig_a) = run_hero_and_item();
        let (cast_b, item_b, dig_b) = run_hero_and_item();
        assert!(cast_a, "expected AI + HeroKit cast for Batch B hero");
        assert!(item_a, "expected AI + item_actives Aegis activation");
        assert_eq!((cast_a, item_a, dig_a), (cast_b, item_b, dig_b));

        // 2. 5v5 free-roam AI bounds (Blackclaw/Ghostwing pits) + safe Recall channel completion.
        let mut w5 = World::new(37, 0);
        w5.use_sovereigns_rise_5v5(None).unwrap();
        w5.set_bot_difficulty(Some(BotDifficulty::VeryHard));
        assert!(w5.is_position_walkable(Position { x: 2.85, y: -29.77 }));
        assert!(w5.is_position_reachable(w5.entities[0].position, Position { x: -2.85, y: 29.77 }));
        w5.entities[0].position = Position { x: -2.85, y: 29.77 };
        w5.entities[0].hp = w5.entities[0].max_hp * 0.20;
        let mut recall_started = false;
        let mut recall_completed = false;
        for _ in 0..200 {
            let events = w5.step();
            if w5
                .hero_5v5_status(1)
                .is_some_and(|st| st.recalling_until.is_some())
            {
                recall_started = true;
            }
            if events
                .iter()
                .any(|ev| matches!(ev, Event::RecallComplete { id: 1, .. }))
            {
                recall_completed = true;
                break;
            }
        }
        assert!(
            recall_started,
            "expected 5v5 bot to initiate Command::Recall"
        );
        assert!(
            recall_completed,
            "expected 5v5 bot to complete Command::Recall"
        );
        assert!(w5.hero_5v5_status(1).unwrap().homeguard_active);

        // 3. Mode-agnostic objective template classification across 5v5, 3v3, and Blitz.
        assert_eq!(
            World::classify_objective(332),
            Some(ObjectiveKind::Blackclaw)
        );
        assert_eq!(
            World::classify_objective(331),
            Some(ObjectiveKind::Ghostwing)
        );
        assert_eq!(World::classify_objective(363), Some(ObjectiveKind::Kraken));
        assert_eq!(
            World::classify_objective(362),
            Some(ObjectiveKind::GoldMine)
        );
        assert_eq!(
            World::classify_objective(361),
            Some(ObjectiveKind::MinionMine)
        );
        assert_eq!(World::classify_objective(311), Some(ObjectiveKind::Sentry));
    }
}
