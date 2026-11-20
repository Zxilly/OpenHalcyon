use super::*;
use crate::native_combat::{Damage, ranks};
use halcyon_data::{AbilityParameter as P, DamageOperand};
#[derive(Clone, Debug, Serialize)]
pub(super) struct Rules {
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
pub(super) struct Dash {
    pub source: u32,
    team: u8,
    goal: Position,
    step: f32,
    damage: f32,
    pierce: f32,
    hit: Vec<u32>,
}
impl Room<'_> {
    pub(super) fn begin_twister(&mut self, i: usize, r: usize, point: Position) {
        let state = &mut *self.state;
        let rules = state.abilities.twister.as_ref().unwrap();
        let e = &self.entities[i];
        self.entities.spawn_spell(Dash {
            source: e.id,
            team: e.team,
            goal: e.position.advance(point, rules.distance[r]),
            step: rules.distance[r] / (rules.duration[r] * TICK_HZ as f32),
            damage: rules.damage.at(r, e.item_stats.crystal, 0.),
            pierce: e.item_stats.shield_pierce,
            hit: Vec::new(),
        });
    }
}
impl crate::persistent_spells::TimedSpell for Dash {
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
            if combat::line_distance(target.position, start, next)
                > radius + target.stats.physiology.map_or(0., |p| p.collision_radius)
                || !world.spell_motion_clear(next, target.position)
            {
                continue;
            }
            self.hit.push(target.id);
            let amount = shop::mitigated(self.damage, target.stats.shield, self.pierce);
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
        assert_eq!(w.entities[0].position, p);
        assert_eq!(w.entities[0].energy, 140.);
        assert_eq!(w.entities[1].hp, 900.);
        assert_eq!(w.entities[3].hp, 900.);
        assert_eq!(w.entities[5].hp, 1000.);
        assert!(w.entities.spells::<Dash>().next().is_none());
    }
}
