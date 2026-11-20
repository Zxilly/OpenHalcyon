use crate::{Entity, Position, TICK_HZ};
use std::collections::BTreeMap;
use std::time::Duration;

/// One row per actor, even if several bot seats use the same hero template.
#[derive(Default)]
pub struct BotHeroDiagnostics {
    pub template: u16,
    pub team: u8,
    pub decisions: u64,
    pub decision_time: Duration,
    pub max_decision_time: Duration,
    pub rejected_orders: BTreeMap<&'static str, u64>,
    pub walk_attempt_ticks: u64,
    pub stalled_walk_ticks: u64,
    pub longest_stall_ticks: u64,
    last_stalled_tick: Option<u64>,
    stall_ticks: u64,
}

#[derive(Default)]
pub struct BotDiagnostics {
    heroes: BTreeMap<u32, BotHeroDiagnostics>,
}

impl BotDiagnostics {
    /// Start with every current hero bot, including actors with zero decisions.
    pub fn new(world: &crate::World) -> Self {
        let mut diagnostics = Self::default();
        for actor in &world.entities {
            if actor.kind == crate::Kind::Hero && actor.bot {
                diagnostics.hero(actor);
            }
        }
        diagnostics
    }

    pub fn heroes(&self) -> &BTreeMap<u32, BotHeroDiagnostics> {
        &self.heroes
    }

    fn hero(&mut self, actor: &Entity) -> &mut BotHeroDiagnostics {
        let hero = self.heroes.entry(actor.id).or_default();
        hero.template = actor.type_id;
        hero.team = actor.team;
        hero
    }

    pub(super) fn decision(
        &mut self,
        actor: &Entity,
        elapsed: Duration,
        result: Result<(), &'static str>,
    ) {
        let hero = self.hero(actor);
        hero.decisions += 1;
        hero.decision_time += elapsed;
        hero.max_decision_time = hero.max_decision_time.max(elapsed);
        if let Err(reason) = result {
            *hero.rejected_orders.entry(reason).or_default() += 1;
        }
    }

    pub(super) fn walk(&mut self, actor: &Entity, tick: u64, before: Position, goal: Position) {
        let hero = self.hero(actor);
        hero.walk_attempt_ticks += 1;
        if before.dist(goal) <= 0.1 || before.dist(actor.position) >= 0.001 {
            hero.last_stalled_tick = None;
            return;
        }
        hero.stall_ticks = if hero.last_stalled_tick == Some(tick - 1) {
            hero.stall_ticks + 1
        } else {
            1
        };
        hero.last_stalled_tick = Some(tick);
        if hero.stall_ticks >= TICK_HZ {
            // Once a streak qualifies, include its initial ticks in the total.
            hero.stalled_walk_ticks += if hero.stall_ticks == TICK_HZ {
                TICK_HZ
            } else {
                1
            };
            hero.longest_stall_ticks = hero.longest_stall_ticks.max(hero.stall_ticks);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::World;

    #[test]
    fn reports_only_sustained_stalls_and_counts_rejected_orders_by_reason() {
        let world = World::new(0, 0);
        let actor = &world.entities[0];
        let before = actor.position;
        let goal = Position {
            x: before.x + 1.,
            y: before.y,
        };
        let mut diagnostics = BotDiagnostics::new(&world);
        diagnostics.decision(actor, Duration::from_nanos(10), Ok(()));
        diagnostics.decision(actor, Duration::from_nanos(30), Err("invalid target"));
        for tick in 1..TICK_HZ {
            diagnostics.walk(actor, tick, before, goal);
        }
        assert_eq!(diagnostics.heroes()[&actor.id].stalled_walk_ticks, 0);
        for tick in TICK_HZ + 1..=2 * TICK_HZ + 1 {
            diagnostics.walk(actor, tick, before, goal);
        }
        diagnostics.walk(actor, 2 * TICK_HZ + 2, before, before);
        let row = &diagnostics.heroes()[&actor.id];
        assert_eq!(row.decisions, 2);
        assert_eq!(row.decision_time, Duration::from_nanos(40));
        assert_eq!(row.max_decision_time, Duration::from_nanos(30));
        assert_eq!(row.rejected_orders["invalid target"], 1);
        assert_eq!(row.walk_attempt_ticks, 2 * TICK_HZ + 1);
        assert_eq!(row.stalled_walk_ticks, TICK_HZ + 1);
        assert_eq!(row.longest_stall_ticks, TICK_HZ + 1);
    }

    #[test]
    fn diagnostic_steps_preserve_three_lane_state() {
        let mut baseline = World::new(4, 0);
        baseline.use_solo_mid_lane().unwrap();
        let mut measured = World::new(4, 0);
        measured.use_solo_mid_lane().unwrap();
        let mut diagnostics = BotDiagnostics::new(&measured);
        for _ in 0..1_000 {
            assert_eq!(
                baseline.step().len(),
                measured.step_diagnosed(&mut diagnostics).len()
            );
        }
        assert_eq!(baseline.state_digest(), measured.state_digest());
        assert_eq!(diagnostics.heroes().len(), 10);
        assert!(diagnostics.heroes().values().all(|row| row.decisions > 0));
    }
}
