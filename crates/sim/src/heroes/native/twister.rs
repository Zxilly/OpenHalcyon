use crate::Event;
use crate::Kind;
use crate::Position;
use crate::Room;
use crate::TICK_HZ;
use crate::combat::native_combat::{Damage, ranks};
use crate::ecs::rules::Layout;
use crate::ecs::score::DamageSchool;
use halcyon_data::GameData;
use halcyon_data::{AbilityParameter as P, DamageOperand};
use serde::Serialize;
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Rules {
    damage: Damage<5>,
    distance: [f32; 5],
    duration: [f32; 5],
}
impl Rules {
    pub fn load(d: &GameData) -> Result<Option<Self>, String> {
        let Some(a) = d.ability("Karas", "Ability__Karas__B") else {
            return Ok(None);
        };
        Ok(Some(Self {
            damage: Damage::named(a, DamageOperand::UppercaseDamage)?,
            distance: ranks(a, P::ForwardDashDistance)?,
            duration: ranks(a, P::ForwardDashTime)?,
        }))
    }
}
#[derive(bevy_ecs::prelude::Component, Clone, Debug, Serialize)]
pub(crate) struct Dash {
    pub source: u32,
    team: u8,
    goal: Position,
    step: f32,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    announce: bool,
    damage: f32,
    pierce: f32,
    hit: Vec<u32>,
}
impl Room<'_> {
    pub(crate) fn begin_twister(&mut self, i: usize, r: usize, point: Position) {
        let state = &mut *self.state;
        let rules = state.abilities.twister.as_ref().unwrap();
        let e = &self.entities[i];
        let goal = e.position.advance(point, rules.distance[r]);
        let aral = matches!(state.layout, Layout::Aral { .. });
        let distance = if aral {
            e.position.dist(goal)
        } else {
            rules.distance[r]
        };
        self.entities.spawn_spell(Dash {
            source: e.id,
            team: e.team,
            goal,
            announce: aral,
            step: distance / (rules.duration[r] * TICK_HZ as f32),
            damage: rules.damage.at(r, e.item_stats.crystal, 0.),
            pierce: e.item_stats.shield_pierce,
            hit: Vec::new(),
        });
    }
}
impl crate::combat::persistent_spells::TimedSpell for Dash {
    fn expired_before_step(&self, _: u64) -> bool {
        false
    }
    fn begin_impact(&mut self) -> Self {
        let hit = std::mem::take(&mut self.hit);
        Self {
            hit,
            ..self.clone()
        }
    }
    fn advance(&mut self, world: &mut Room<'_>, events: &mut Vec<Event>) -> bool {
        let Some(i) = world.index(self.source) else {
            return false;
        };
        let e = &world.entities[i];
        if e.hp <= 0. || e.status.stun_until > world.tick || e.status.root_until > world.tick {
            return false;
        }
        let start = e.position;
        let next = start.advance(self.goal, self.step);
        if !world.spell_motion_clear(start, next) || world.bulwark_blocks_leap(i, next) {
            return false;
        }
        if std::mem::take(&mut self.announce) && start != next {
            events.push(Event::TwisterDash {
                source: self.source,
                goal: self.goal,
                speed: self.step * TICK_HZ as f32,
            });
        }
        let radius = e.stats.physiology.map_or(0., |p| p.collision_radius);
        world.place_spell_actor(i, next, events);
        for j in 0..world.entities.len() {
            let target = &world.entities[j];
            if target.hp <= 0.
                || target.team == self.team
                || !matches!(target.kind, Kind::Hero | Kind::Minion | Kind::Neutral)
                || self.hit.contains(&target.id)
            {
                continue;
            }
            if crate::combat::line_distance(target.position, start, next)
                > radius + target.stats.physiology.map_or(0., |p| p.collision_radius)
                || !world.spell_motion_clear(next, target.position)
            {
                continue;
            }
            self.hit.push(target.id);
            let amount =
                crate::items::shop::mitigated(self.damage, target.stats.shield, self.pierce);
            world.hurt(
                self.source,
                self.team,
                j,
                amount,
                DamageSchool::Crystal,
                events,
            );
        }
        next != self.goal
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AbilitySlot;
    use crate::CastTarget;
    use crate::Command;
    use crate::World;
    #[test]
    #[ignore = "requires original delivery and supplemental combat data"]
    fn twister_traverses_the_requested_path_and_hits_each_crossed_body_once() {
        let data = GameData::load_with_extra(
            std::env::var_os("HALCYON_DELIVERY").unwrap(),
            Some(std::path::Path::new(
                &std::env::var_os("HALCYON_DATA_EXTRA").unwrap(),
            )),
        )
        .unwrap();
        let mut w = World::from_data(12, 6, &data).unwrap();
        w.use_six_human_roster().unwrap();
        w.select_hero(0, "Karas", &data).unwrap();
        w.use_aral_lane().unwrap();
        for idx in 0..w.entities.len().min(6) {
            let e = &mut w.entities[idx];
            e.position = Position {
                x: 25. + idx as f32,
                y: 0.,
            };
            e.hp = 1000.;
            e.max_hp = 1000.;
            e.stats.shield = 0.;
        }
        w.entities[0].position = Position::default();
        w.entities[1].position = Position { x: 2., y: 0. };
        w.entities[3].position = Position { x: 7., y: 0. };
        w.entities[5].position = Position { x: 7., y: 1.95 };
        w.entities[0].energy = 200.;
        w.entities[0].skill_points = 1;
        w.command(1, Command::Learn(AbilitySlot::B)).unwrap();
        let p = Position { x: 7., y: 0. };
        let cmd = |p| Command::Cast {
            slot: AbilitySlot::B,
            target: CastTarget::Point(p),
        };
        assert_eq!(w.decide(0), cmd(w.entities[1].position));
        assert!(w.command(1, cmd(Position { x: 7.1, y: 0. })).is_err());
        w.command(1, cmd(p)).unwrap();
        let mut ev = Vec::new();
        w.tick = 5;
        w.advance_cast(0, &mut ev);
        for t in 6..13 {
            w.tick = t;
            w.advance_spell_family::<Dash>(&mut ev);
        }
        assert_eq!(ev.iter().filter(|e| matches!(e,Event::TwisterDash{source:1,goal,speed} if *goal==p && (*speed-21.875).abs()<0.001)).count(),1);
        assert_eq!(w.entities[0].position, p);
        assert_eq!(w.entities[0].energy, 140.);
        assert_eq!(w.entities[1].hp, 900.);
        assert_eq!(w.entities[3].hp, 900.);
        assert_eq!(w.entities[5].hp, 1000.);
        assert!(w.entities.spells::<Dash>().next().is_none());
    }
}
