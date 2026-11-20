//! Layer 3 deterministic macro Behavior Tree built on `bonsai-bt 0.14.0`.

use super::super::*;
use super::threat::ThreatAssessment;
use bonsai_bt::{
    Action, BT, Behavior, Event as BtEvent, Failure, Running, Select, Status, UpdateArgs,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Macro tactical mode selected by the Layer 3 behavior tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MacroMode {
    Retreat,
    BaseDefense,
    ObjectiveContest,
    Teamfight,
    JungleFarm,
    LanePush,
}

/// Mode-agnostic major map objective classifications across 5v5, 3v3, and Blitz.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ObjectiveKind {
    Blackclaw,
    Ghostwing,
    Kraken,
    GoldMine,
    MinionMine,
    Sentry,
}

/// Snapshot of a live map objective discovered via mode-agnostic room queries.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ObjectiveInfo {
    pub entity_id: u32,
    pub kind: ObjectiveKind,
    pub position: Position,
    pub team: u8,
    pub hp: f32,
    pub max_hp: f32,
    pub contesting_allies: u8,
    pub contesting_enemies: u8,
}

/// Leaf action labels inside the Layer 3 `bonsai-bt` reactive selector.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MacroAction {
    RetreatAndRecall,
    BaseDefense,
    ObjectiveContest,
    TeamfightSupport,
    JungleCampFarm,
    LanePushAndLastHit,
}

/// Persistent per-bot blackboard tracked inside `BT<MacroAction, MacroBlackboard>`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MacroBlackboard {
    pub mode: MacroMode,
    pub focus_entity: Option<u32>,
    pub anchor: Position,
    pub mode_entered_tick: u64,
    pub recall_started_tick: Option<u64>,
    pub retreat_committed: bool,
    pub last_step_tick: u64,
}

impl MacroBlackboard {
    fn new(home: Position) -> Self {
        Self {
            mode: MacroMode::LanePush,
            focus_entity: None,
            anchor: home,
            mode_entered_tick: 0,
            recall_started_tick: None,
            retreat_committed: false,
            last_step_tick: 0,
        }
    }
}

/// Compact boxed per-bot BT runtime stored outside `Entity` inline fields.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct MacroBtRuntime {
    bt: BT<MacroAction, MacroBlackboard>,
}

impl MacroBtRuntime {
    fn new(home: Position) -> Self {
        Self {
            bt: BT::new(build_macro_behavior(), MacroBlackboard::new(home)),
        }
    }
}

/// Sparse per-room storage for bot Behavior Tree runtimes, keyed deterministically by `entity.id`.
#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct BotBrains {
    runtimes: BTreeMap<u32, Box<MacroBtRuntime>>,
}

impl BotBrains {
    pub(crate) fn is_empty(&self) -> bool {
        self.runtimes.is_empty()
    }

    pub(crate) fn remove(&mut self, actor_id: u32) {
        self.runtimes.remove(&actor_id);
    }

    pub(crate) fn reset_hero(&mut self, actor_id: u32, home: Position) {
        if let Some(runtime) = self.runtimes.get_mut(&actor_id) {
            runtime.bt.reset_bt();
            *runtime.bt.blackboard_mut() = MacroBlackboard::new(home);
        }
    }

    pub(crate) fn blackboard(&self, actor_id: u32) -> Option<&MacroBlackboard> {
        self.runtimes.get(&actor_id).map(|rt| rt.bt.blackboard())
    }

    pub(crate) fn tick_count(&self, actor_id: u32) -> Option<u64> {
        self.runtimes.get(&actor_id).map(|rt| rt.bt.tick_count())
    }
}

fn build_macro_behavior() -> Behavior<MacroAction> {
    Select(vec![
        Action(MacroAction::RetreatAndRecall),
        Action(MacroAction::BaseDefense),
        Action(MacroAction::ObjectiveContest),
        Action(MacroAction::TeamfightSupport),
        Action(MacroAction::JungleCampFarm),
        Action(MacroAction::LanePushAndLastHit),
    ])
    .memory(false)
}

/// Stack-only, zero-allocation snapshot evaluated by the Behavior Tree leaf rules.
#[derive(Clone, Copy, Debug)]
struct MacroContext {
    tick: u64,
    hero_pos: Position,
    home: Position,
    lane_goal: Position,
    hp_ratio: f32,
    at_fountain: bool,
    supports_recall: bool,
    total_threat: f32,
    nearby_enemy_heroes: u8,
    nearby_allied_heroes: u8,
    bush_concealed: bool,
    finishable_duel: bool,
    finishing_enemy_crystal: bool,
    base_threat: Option<(u32, Position)>,
    objective: Option<ObjectiveInfo>,
    teamfight_target: Option<(u32, Position)>,
    jungle_camp: Option<(u32, Position)>,
}

fn enter_mode(
    bb: &mut MacroBlackboard,
    ctx: &MacroContext,
    mode: MacroMode,
    focus_entity: Option<u32>,
    anchor: Position,
) {
    if bb.mode != mode {
        bb.mode = mode;
        bb.mode_entered_tick = ctx.tick;
    }
    bb.focus_entity = focus_entity;
    bb.anchor = anchor;
    bb.last_step_tick = ctx.tick;
}

fn eval_macro_action(action: MacroAction, bb: &mut MacroBlackboard, ctx: &MacroContext) -> Status {
    match action {
        MacroAction::RetreatAndRecall => {
            let critical_low = ctx.hp_ratio < 0.23;
            let team_support = f32::from(ctx.nearby_allied_heroes.min(2)) * 0.12;
            let threatened_low = ctx.hp_ratio < 0.34
                && ctx.total_threat > (0.65 + team_support)
                && !ctx.finishable_duel;
            let fountain_recover =
                ctx.at_fountain && ctx.hp_ratio < 0.70 && ctx.base_threat.is_none();
            let stay_committed = bb.retreat_committed
                && if ctx.at_fountain {
                    ctx.hp_ratio < 0.72
                } else {
                    ctx.hp_ratio < 0.48 || ctx.total_threat >= 0.20
                };

            if critical_low || threatened_low || fountain_recover || stay_committed {
                bb.retreat_committed = true;
                if ctx.supports_recall
                    && ctx.hp_ratio < 0.30
                    && ctx.hero_pos.dist(ctx.home) > 24.0
                    && ctx.nearby_enemy_heroes == 0
                    && ctx.bush_concealed
                {
                    bb.recall_started_tick = bb.recall_started_tick.or(Some(ctx.tick));
                } else {
                    bb.recall_started_tick = None;
                }
                enter_mode(bb, ctx, MacroMode::Retreat, None, ctx.home);
                Running
            } else {
                bb.retreat_committed = false;
                bb.recall_started_tick = None;
                Failure
            }
        }
        MacroAction::BaseDefense => {
            if let Some((threat_id, threat_pos)) = ctx.base_threat
                && !ctx.finishing_enemy_crystal
            {
                enter_mode(bb, ctx, MacroMode::BaseDefense, Some(threat_id), threat_pos);
                Running
            } else {
                Failure
            }
        }
        MacroAction::ObjectiveContest => {
            if let Some(obj) = ctx.objective
                && !ctx.finishing_enemy_crystal
            {
                enter_mode(
                    bb,
                    ctx,
                    MacroMode::ObjectiveContest,
                    Some(obj.entity_id),
                    obj.position,
                );
                Running
            } else {
                Failure
            }
        }
        MacroAction::TeamfightSupport => {
            if let Some((target_id, target_pos)) = ctx.teamfight_target
                && ctx.hp_ratio >= 0.34
            {
                enter_mode(bb, ctx, MacroMode::Teamfight, Some(target_id), target_pos);
                Running
            } else {
                Failure
            }
        }
        MacroAction::JungleCampFarm => {
            if let Some((camp_id, camp_pos)) = ctx.jungle_camp
                && ctx.hp_ratio >= 0.40
                && ctx.nearby_enemy_heroes == 0
            {
                enter_mode(bb, ctx, MacroMode::JungleFarm, Some(camp_id), camp_pos);
                Running
            } else {
                Failure
            }
        }
        MacroAction::LanePushAndLastHit => {
            enter_mode(bb, ctx, MacroMode::LanePush, None, ctx.lane_goal);
            Running
        }
    }
}

/// Template IDs from `units_full.json` for major map objectives across 5v5, 3v3, and Blitz.
const OBJECTIVE_TYPE_BLACKCLAW_UNCAPTURED: u16 = 332;
const OBJECTIVE_TYPE_BLACKCLAW_CAPTURED: u16 = 333;
const OBJECTIVE_TYPE_GHOSTWING: u16 = 331;
const OBJECTIVE_TYPE_KRAKEN_JUNGLE: u16 = 320;
const OBJECTIVE_TYPE_KRAKEN_CAPTURED: u16 = 321;
const OBJECTIVE_TYPE_KRAKEN_5V5: u16 = 322;
const OBJECTIVE_TYPE_KRAKEN_GENERIC_NEUTRAL: u16 = 345;
const OBJECTIVE_TYPE_KRAKEN_GENERIC_HOSTILE: u16 = 346;
const OBJECTIVE_TYPE_HF_KRAKEN_JUNGLE: u16 = 363;
const OBJECTIVE_TYPE_HF_KRAKEN_CAPTURED: u16 = 364;
const OBJECTIVE_TYPE_GOLD_MINER_GENERIC: u16 = 309;
const OBJECTIVE_TYPE_GOLD_MINER_HF: u16 = 362;
const OBJECTIVE_TYPE_CRYSTAL_MINER_GENERIC: u16 = 310;
const OBJECTIVE_TYPE_CRYSTAL_MINER_HF: u16 = 361;
const OBJECTIVE_TYPE_BLITZ_MIDDLE_SENTRY: u16 = 311;
const OBJECTIVE_TYPE_BLITZ_REVAMP_MIDDLE_SENTRY: u16 = 312;
const OBJECTIVE_TYPE_BLITZ_ALLY_SENTRY: u16 = 313;

impl Room<'_> {
    /// Fast visibility check from `observer` to `target` at precomputed distance `dist`,
    /// avoiding O(allies) fallback scans when `observer` itself has direct sight.
    #[inline]
    pub(crate) fn hero_sees(&self, observer: &Entity, target: &Entity, dist: f32) -> bool {
        if dist <= observer.stats.sight()
            && self.combat_clear(observer, target)
            && self
                .native_navigation
                .as_ref()
                .is_none_or(|map| map.can_see(observer.position, target.position))
        {
            return true;
        }
        self.visible(observer.team, target)
    }

    /// Enables or disables modern Layer 1-4 AI explicitly (defaults to `true` on native/live layouts
    /// and `false` on `Layout::Validation` to keep historical validation checkpoints intact).
    pub fn set_modern_ai(&mut self, enabled: bool) {
        self.modern_ai_override = Some(enabled);
    }

    /// Returns whether this room currently uses the modern Layer 3 + Layer 4 AI stack.
    pub fn uses_modern_ai(&self) -> bool {
        self.modern_ai_override
            .unwrap_or(!matches!(self.layout, Layout::Validation))
    }

    /// Inspects the persisted Layer 3 macro mode for a bot actor, if initialized.
    pub fn bot_macro_mode(&self, actor_id: u32) -> Option<MacroMode> {
        self.bot_brains.blackboard(actor_id).map(|bb| bb.mode)
    }

    /// Inspects the number of times the Layer 3 `bonsai-bt` tree stepped for `actor_id`.
    pub fn bot_bt_tick_count(&self, actor_id: u32) -> Option<u64> {
        self.bot_brains.tick_count(actor_id)
    }

    /// Mode-agnostic classification of major map objective templates (5v5, 3v3, Blitz).
    pub fn classify_objective(type_id: u16) -> Option<ObjectiveKind> {
        match type_id {
            OBJECTIVE_TYPE_BLACKCLAW_UNCAPTURED | OBJECTIVE_TYPE_BLACKCLAW_CAPTURED => {
                Some(ObjectiveKind::Blackclaw)
            }
            OBJECTIVE_TYPE_GHOSTWING => Some(ObjectiveKind::Ghostwing),
            OBJECTIVE_TYPE_KRAKEN_JUNGLE
            | OBJECTIVE_TYPE_KRAKEN_CAPTURED
            | OBJECTIVE_TYPE_KRAKEN_5V5
            | OBJECTIVE_TYPE_KRAKEN_GENERIC_NEUTRAL
            | OBJECTIVE_TYPE_KRAKEN_GENERIC_HOSTILE
            | OBJECTIVE_TYPE_HF_KRAKEN_JUNGLE
            | OBJECTIVE_TYPE_HF_KRAKEN_CAPTURED => Some(ObjectiveKind::Kraken),
            OBJECTIVE_TYPE_GOLD_MINER_GENERIC | OBJECTIVE_TYPE_GOLD_MINER_HF => {
                Some(ObjectiveKind::GoldMine)
            }
            OBJECTIVE_TYPE_CRYSTAL_MINER_GENERIC | OBJECTIVE_TYPE_CRYSTAL_MINER_HF => {
                Some(ObjectiveKind::MinionMine)
            }
            OBJECTIVE_TYPE_BLITZ_MIDDLE_SENTRY
            | OBJECTIVE_TYPE_BLITZ_REVAMP_MIDDLE_SENTRY
            | OBJECTIVE_TYPE_BLITZ_ALLY_SENTRY => Some(ObjectiveKind::Sentry),
            _ => None,
        }
    }

    /// Counts living map objectives currently active in the room.
    pub fn count_active_objectives(&self) -> usize {
        self.entities
            .iter()
            .filter(|e| e.hp > 0.0 && Self::classify_objective(e.type_id).is_some())
            .count()
    }

    /// Mode-agnostic query returning the highest-priority contestable or threatening objective
    /// for hero `hero_idx`, deterministically broken by `entity.id`.
    pub fn best_objective_for(&self, hero_idx: usize) -> Option<ObjectiveInfo> {
        let hero = &self.entities[hero_idx];
        let hp_ratio = (hero.hp / hero.max_hp.max(1.0)).clamp(0.0, 1.0);
        let enemy_crystal_pos = self.layout.objective(1 - hero.team);
        // Never abandon an active crystal finish to walk across the map for an objective.
        if hero.position.dist(enemy_crystal_pos) <= 18.0 {
            return None;
        }

        let mut best: Option<(f32, u32, ObjectiveInfo)> = None;
        for e in &self.entities {
            if e.hp <= 0.0 || e.team == hero.team {
                continue;
            }
            let Some(kind) = Self::classify_objective(e.type_id) else {
                continue;
            };
            let dist = hero.position.dist(e.position);
            let is_captured_enemy_boss = e.team == 1 - hero.team
                && matches!(kind, ObjectiveKind::Blackclaw | ObjectiveKind::Kraken);
            if (!is_captured_enemy_boss && dist > 42.0) || dist > 55.0 {
                continue;
            }
            if !self.is_position_reachable(hero.position, e.position) {
                continue;
            }
            let visible_or_global_boss = self.visible(hero.team, e) || is_captured_enemy_boss;
            if !visible_or_global_boss && dist > 32.0 {
                continue;
            }

            let mut allies = 0_u8;
            let mut enemies = 0_u8;
            for h in &self.entities {
                if h.kind != Kind::Hero || h.hp <= 0.0 || h.id == hero.id {
                    continue;
                }
                if h.position.dist(e.position) <= 14.0 {
                    if h.team == hero.team {
                        allies = allies.saturating_add(1);
                    } else if self.visible(hero.team, h) {
                        enemies = enemies.saturating_add(1);
                    }
                }
            }

            if !is_captured_enemy_boss {
                if hp_ratio < 0.45 {
                    continue;
                }
                let max_range = if allies > 0 || self.visible(hero.team, e) {
                    42.0
                } else {
                    26.0
                };
                if dist > max_range {
                    continue;
                }
                if enemies > allies + 1 {
                    continue;
                }
            } else if dist > 55.0 {
                continue;
            }

            let priority = match kind {
                ObjectiveKind::Blackclaw | ObjectiveKind::Kraken if is_captured_enemy_boss => 120.0,
                ObjectiveKind::Blackclaw | ObjectiveKind::Kraken => 95.0,
                ObjectiveKind::Ghostwing => 85.0,
                ObjectiveKind::GoldMine => 72.0,
                ObjectiveKind::MinionMine | ObjectiveKind::Sentry => 65.0,
            } + f32::from(allies) * 14.0
                - dist * 0.8
                - f32::from(enemies) * 6.0;

            let info = ObjectiveInfo {
                entity_id: e.id,
                kind,
                position: e.position,
                team: e.team,
                hp: e.hp,
                max_hp: e.max_hp,
                contesting_allies: allies,
                contesting_enemies: enemies,
            };
            // Maximize priority, break ties by lowest entity.id.
            let better = best.is_none_or(|(best_p, best_id, _)| {
                priority
                    .total_cmp(&best_p)
                    .reverse()
                    .then_with(|| e.id.cmp(&best_id))
                    == std::cmp::Ordering::Less
            });
            if better {
                best = Some((priority, e.id, info));
            }
        }
        best.map(|(_, _, info)| info)
    }

    fn find_base_defense_threat(&self, hero: &Entity, home: Position) -> Option<(u32, Position)> {
        let home_crystal_pos = self
            .entities
            .iter()
            .find(|e| e.kind == Kind::Crystal && e.team == hero.team && e.hp > 0.0)
            .map_or(home, |e| e.position);

        let mut best: Option<(u8, f32, u32, Position)> = None;
        for enemy in &self.entities {
            if enemy.hp <= 0.0 || enemy.team != 1 - hero.team {
                continue;
            }
            let is_enemy_boss = Self::classify_objective(enemy.type_id).is_some();
            if !matches!(enemy.kind, Kind::Hero | Kind::Minion) && !is_enemy_boss {
                continue;
            }
            let dist_crystal = enemy.position.dist(home_crystal_pos);
            let dist_hero = hero.position.dist(enemy.position);
            if dist_crystal > 24.0 {
                continue;
            }
            if dist_hero > 32.0
                && !(dist_crystal <= 16.0 && (enemy.kind == Kind::Hero || is_enemy_boss))
            {
                continue;
            }
            let near_inner_defense = dist_crystal <= 18.0
                || self.entities.iter().any(|s| {
                    s.team == hero.team
                        && s.hp > 0.0
                        && matches!(s.kind, Kind::Armory | Kind::Crystal | Kind::Tower)
                        && s.position.dist(home_crystal_pos) <= 26.0
                        && enemy.position.dist(s.position) <= 12.0
                });
            if !near_inner_defense {
                continue;
            }
            if !self.visible(hero.team, enemy)
                || !self.is_position_reachable(hero.position, enemy.position)
            {
                continue;
            }
            let tier = if is_enemy_boss {
                0
            } else if enemy.kind == Kind::Hero {
                1
            } else {
                2
            };
            let better = best.is_none_or(|(bt, bd, bid, _)| {
                tier.cmp(&bt)
                    .then_with(|| dist_crystal.total_cmp(&bd))
                    .then_with(|| enemy.id.cmp(&bid))
                    == std::cmp::Ordering::Less
            });
            if better {
                best = Some((tier, dist_crystal, enemy.id, enemy.position));
            }
        }
        best.map(|(_, _, id, pos)| (id, pos))
    }

    fn find_teamfight_target(
        &self,
        hero: &Entity,
        threat: &ThreatAssessment,
    ) -> Option<(u32, Position)> {
        let early_side_laner = matches!(self.layout, Layout::SoloMid { .. })
            && hero.solo_side != 0
            && self.tick < 240 * TICK_HZ;
        let assist_radius = if early_side_laner { 16.0 } else { 24.0 };

        let mut best: Option<(f32, u32, Position)> = None;
        for enemy in &self.entities {
            if enemy.kind != Kind::Hero || enemy.team == hero.team || enemy.hp <= 0.0 {
                continue;
            }
            let d = hero.position.dist(enemy.position);
            if d > assist_radius {
                continue;
            }
            if matches!(self.layout, Layout::SoloMid { .. })
                && self.native_navigation.is_none()
                && (hero.position.y - enemy.position.y).abs() > 14.0
            {
                continue;
            }
            if !self.visible(hero.team, enemy)
                || !self.is_position_reachable(hero.position, enemy.position)
            {
                continue;
            }
            let ally_engaged = self.entities.iter().any(|ally| {
                ally.kind == Kind::Hero
                    && ally.team == hero.team
                    && ally.hp > 0.0
                    && ally.id != hero.id
                    && ally.position.dist(enemy.position) <= 13.0
            });
            let multi_enemy = threat.nearby_enemy_heroes >= 2;
            let ally_injured =
                threat.nearby_allied_heroes >= 1 && threat.lowest_ally_hp_ratio < 0.72;
            if !(ally_engaged || multi_enemy || ally_injured) {
                continue;
            }
            let enemy_hp_ratio = (enemy.hp / enemy.max_hp.max(1.0)).clamp(0.0, 1.0);
            let score =
                (1.0 - enemy_hp_ratio) * 40.0 + if ally_engaged { 25.0 } else { 0.0 } - d * 1.1;
            let better = best.is_none_or(|(bs, bid, _)| {
                score
                    .total_cmp(&bs)
                    .reverse()
                    .then_with(|| enemy.id.cmp(&bid))
                    == std::cmp::Ordering::Less
            });
            if better {
                best = Some((score, enemy.id, enemy.position));
            }
        }
        best.map(|(_, id, pos)| (id, pos))
    }

    fn find_jungle_camp(
        &self,
        hero: &Entity,
        threat: &ThreatAssessment,
    ) -> Option<(u32, Position)> {
        if threat.nearby_enemy_heroes > 0 || threat.in_enemy_turret_range {
            return None;
        }
        let mut best: Option<(f32, u32, Position)> = None;
        for camp in &self.entities {
            if camp.kind != Kind::Neutral || camp.hp <= 0.0 {
                continue;
            }
            if Self::classify_objective(camp.type_id).is_some() {
                continue;
            }
            let d = hero.position.dist(camp.position);
            if d > 22.0 {
                continue;
            }
            if !self.is_position_reachable(hero.position, camp.position) {
                continue;
            }
            if !self.visible(hero.team, camp) && d > 16.0 {
                continue;
            }
            // Don't leave an active close minion wave right next to us to farm a far camp.
            if d > 10.0
                && self.entities.iter().any(|m| {
                    m.team != hero.team
                        && m.kind == Kind::Minion
                        && m.hp > 0.0
                        && m.position.dist(hero.position) <= 10.0
                })
            {
                continue;
            }
            let hp_ratio = (camp.hp / camp.max_hp.max(1.0)).clamp(0.0, 1.0);
            let rank = d + hp_ratio * 3.0;
            if best.is_none_or(|(bd, bid, _)| (rank, camp.id) < (bd, bid)) {
                best = Some((rank, camp.id, camp.position));
            }
        }
        best.map(|(_, id, pos)| (id, pos))
    }

    fn build_macro_context(&self, hero_idx: usize, threat: &ThreatAssessment) -> MacroContext {
        let hero = &self.entities[hero_idx];
        let home = self.layout.home(hero);
        let lane_goal = if matches!(self.layout, Layout::Aral { .. }) {
            self.aral_march_goal(hero)
        } else {
            self.layout.objective(1 - hero.team)
        };
        let hp_ratio = (hero.hp / hero.max_hp.max(1.0)).clamp(0.0, 1.0);
        let at_fountain = self.native_navigation.as_ref().map_or_else(
            || self.layout.at_base(hero, true),
            |map| map.at_fountain(hero),
        );
        let finishable_duel = !threat.in_enemy_turret_range
            && self.entities.iter().any(|eh| {
                eh.kind == Kind::Hero
                    && eh.team != hero.team
                    && eh.hp > 0.0
                    && self.visible(hero.team, eh)
                    && eh.position.dist(hero.position) <= hero.stats.range + 2.5
                    && (eh.hp / eh.max_hp.max(1.0)) < hp_ratio * 0.65
                    && (eh.hp / eh.max_hp.max(1.0)) < 0.22
            });
        let finishing_enemy_crystal = self.entities.iter().any(|c| {
            c.kind == Kind::Crystal
                && c.team != hero.team
                && c.hp > 0.0
                && c.hp < c.max_hp * 0.25
                && hero.position.dist(c.position) <= 16.0
        });

        MacroContext {
            tick: self.tick,
            hero_pos: hero.position,
            home,
            lane_goal,
            hp_ratio,
            at_fountain,
            supports_recall: !matches!(self.layout, Layout::Aral { .. } | Layout::Validation),
            total_threat: threat.total_threat,
            nearby_enemy_heroes: threat.nearby_enemy_heroes,
            nearby_allied_heroes: threat.nearby_allied_heroes,
            bush_concealed: threat.bush_concealed,
            finishable_duel,
            finishing_enemy_crystal,
            base_threat: self.find_base_defense_threat(hero, home),
            objective: self.best_objective_for(hero_idx),
            teamfight_target: self.find_teamfight_target(hero, threat),
            jungle_camp: self.find_jungle_camp(hero, threat),
        }
    }

    /// Steps the bot's Layer 3 `bonsai-bt` Behavior Tree on its difficulty tick cadence.
    pub(crate) fn step_bot_macro_bt(
        &mut self,
        hero_idx: usize,
        threat: &ThreatAssessment,
    ) -> MacroBlackboard {
        let hero_id = self.entities[hero_idx].id;
        let current_tick = self.state.tick;
        let ctx = self.build_macro_context(hero_idx, threat);
        let interval = self.decision_interval(&self.entities[hero_idx]);
        let runtime = self
            .state
            .bot_brains
            .runtimes
            .entry(hero_id)
            .or_insert_with(|| Box::new(MacroBtRuntime::new(ctx.home)));
        if runtime.bt.is_finished() {
            runtime.bt.reset_bt();
        }
        let elapsed_ticks = current_tick
            .saturating_sub(runtime.bt.blackboard().last_step_tick)
            .max(interval);
        let dt = elapsed_ticks as f32 / TICK_HZ as f32;
        let event: BtEvent = UpdateArgs { dt }.into();
        let _ = runtime.bt.tick(&event, &mut |args, bb| {
            let status = eval_macro_action(*args.action, bb, &ctx);
            (status, 0.0)
        });
        *runtime.bt.blackboard()
    }

    /// Evaluates Layer 3 macro state read-only (for direct `Room::decide(i)` unit tests).
    pub(crate) fn evaluate_macro_bt_readonly(
        &self,
        hero_idx: usize,
        threat: &ThreatAssessment,
    ) -> MacroBlackboard {
        let hero = &self.entities[hero_idx];
        let ctx = self.build_macro_context(hero_idx, threat);
        let mut bb = self
            .bot_brains
            .blackboard(hero.id)
            .copied()
            .unwrap_or_else(|| MacroBlackboard::new(ctx.home));
        for action in [
            MacroAction::RetreatAndRecall,
            MacroAction::BaseDefense,
            MacroAction::ObjectiveContest,
            MacroAction::TeamfightSupport,
            MacroAction::JungleCampFarm,
            MacroAction::LanePushAndLastHit,
        ] {
            if eval_macro_action(action, &mut bb, &ctx) != Failure {
                break;
            }
        }
        bb
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use halcyon_data::BotDifficulty;

    #[test]
    fn difficulty_cadence_steps_bt_exclusively_on_decision_intervals() {
        for difficulty in BotDifficulty::ALL {
            let interval = difficulty.decision_ticks();
            let mut w = World::new(2, 1);
            w.set_modern_ai(true);
            w.set_bot_difficulty(Some(difficulty));
            let bot_id = w.entities[1].id;
            for tick in 1..=64 {
                w.step();
                let expected_steps = (1..=tick)
                    .filter(|t| (*t + u64::from(bot_id)).is_multiple_of(interval))
                    .count() as u64;
                assert_eq!(
                    w.bot_bt_tick_count(bot_id).unwrap_or(0),
                    expected_steps,
                    "{difficulty:?} at tick {tick}"
                );
            }
        }
    }

    #[test]
    fn bt_transitions_across_retreat_defense_objective_teamfight_farm_and_push() {
        let mut w = World::new(9, 0);
        w.use_solo_mid_lane().unwrap();

        // Keep only hero 0 (team 0, mid), hero 1 (team 1), hero 2 (team 0 ally), and crystals.
        w.entities
            .retain(|e| matches!(e.id, 1..=3) || e.kind == Kind::Crystal);
        let h0 = w.index(1).unwrap();
        let h1 = w.index(2).unwrap();
        let h2 = w.index(3).unwrap();

        // Place everyone in mid corridor (walkable in SoloMid).
        w.entities[h0].position = Position { x: -10.0, y: 0.0 };
        w.entities[h2].position = Position { x: -60.0, y: 0.0 };
        w.entities[h1].position = Position { x: 60.0, y: 0.0 };

        let eval = |world: &mut World| {
            let idx = world.index(1).unwrap();
            let threat = world.evaluate_threat(idx);
            world.step_bot_macro_bt(idx, &threat).mode
        };

        // 1. Default clean state -> LanePush
        assert_eq!(eval(&mut w), MacroMode::LanePush);

        // 2. Spawn reachable neutral camp nearby -> JungleFarm
        let camp_stats = w.entities[h1].stats;
        let camp_id = w.spawn_with_stats(
            native_camps::NEUTRAL_TEAM,
            Kind::Neutral,
            camp_stats,
            Position { x: -6.0, y: 0.0 },
            false,
        );
        assert_eq!(eval(&mut w), MacroMode::JungleFarm);

        // 3. Move ally + enemy hero into skirmish range -> Teamfight preempts JungleFarm
        let h2 = w.index(3).unwrap();
        w.entities[h2].position = Position { x: -5.0, y: 0.0 };
        w.entities[h2].hp = w.entities[h2].max_hp * 0.5;
        let h1 = w.index(2).unwrap();
        w.entities[h1].position = Position { x: -2.0, y: 0.0 };
        assert_eq!(eval(&mut w), MacroMode::Teamfight);

        // 4. Spawn mode-agnostic Objective (Blackclaw, Ghostwing, Kraken, GoldMine, MinionMine, Sentry)
        for (obj_type, expected_kind) in [
            (
                OBJECTIVE_TYPE_BLACKCLAW_UNCAPTURED,
                ObjectiveKind::Blackclaw,
            ),
            (OBJECTIVE_TYPE_GHOSTWING, ObjectiveKind::Ghostwing),
            (OBJECTIVE_TYPE_KRAKEN_JUNGLE, ObjectiveKind::Kraken),
            (OBJECTIVE_TYPE_GOLD_MINER_HF, ObjectiveKind::GoldMine),
            (OBJECTIVE_TYPE_CRYSTAL_MINER_HF, ObjectiveKind::MinionMine),
            (OBJECTIVE_TYPE_BLITZ_MIDDLE_SENTRY, ObjectiveKind::Sentry),
        ] {
            assert_eq!(Room::classify_objective(obj_type), Some(expected_kind));
            let mut obj_stats = camp_stats;
            obj_stats.id = obj_type;
            let obj_id = w.spawn_with_stats(
                native_camps::NEUTRAL_TEAM,
                Kind::Neutral,
                obj_stats,
                Position { x: 5.0, y: 0.0 },
                false,
            );
            let h0 = w.index(1).unwrap();
            let info = w.best_objective_for(h0).expect("objective discovered");
            assert_eq!(info.kind, expected_kind);
            assert_eq!(eval(&mut w), MacroMode::ObjectiveContest);
            w.entities.retain(|e| e.id != obj_id);
        }

        // 5. Threaten home crystal (-75, 0) -> BaseDefense preempts Objective/Teamfight
        let h0 = w.index(1).unwrap();
        w.entities[h0].position = Position { x: -50.0, y: 0.0 };
        let h1 = w.index(2).unwrap();
        w.entities[h1].position = Position { x: -64.0, y: 0.0 };
        assert_eq!(eval(&mut w), MacroMode::BaseDefense);

        // 6. Drop HP < 23% -> Retreat preempts BaseDefense, and recall/hysteresis persists
        let h0 = w.index(1).unwrap();
        w.entities[h0].hp = w.entities[h0].max_hp * 0.18;
        assert_eq!(eval(&mut w), MacroMode::Retreat);
        // Hysteresis keeps Retreat active at 38% HP while still outside fountain.
        w.entities[h0].hp = w.entities[h0].max_hp * 0.38;
        assert_eq!(eval(&mut w), MacroMode::Retreat);

        // Clean up camp and verify reset on respawn.
        w.entities.retain(|e| e.id != camp_id);
        let h0 = w.index(1).unwrap();
        let home = w.layout.home(&w.entities[h0]);
        w.state.bot_brains.reset_hero(1, home);
        assert_eq!(w.bot_macro_mode(1), Some(MacroMode::LanePush));
    }
}
