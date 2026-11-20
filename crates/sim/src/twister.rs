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
#[derive(Debug, Serialize)]
pub(super) struct Dash {
    pub source: u32,
    team: u8,
    goal: Position,
    step: f32,
    damage: f32,
    pierce: f32,
    hit: Vec<u32>,
}
impl World {
    pub(super) fn begin_twister(&mut self, i: usize, r: usize, point: Position) {
        let rules = self.abilities.twister.as_ref().unwrap();
        let e = &self.entities[i];
        self.twisters.push(Dash {
            source: e.id,
            team: e.team,
            goal: e.position.advance(point, rules.distance[r]),
            step: rules.distance[r] / (rules.duration[r] * TICK_HZ as f32),
            damage: rules.damage.at(r, e.item_stats.crystal, 0.),
            pierce: e.item_stats.shield_pierce,
            hit: Vec::new(),
        });
    }
    pub(super) fn advance_twisters(&mut self, events: &mut Vec<Event>) {
        if self.twisters.is_empty() {
            return;
        }
        let mut active = std::mem::take(&mut self.twisters);
        active.retain_mut(|d| {
            let Some(i) = self.index(d.source) else {
                return false;
            };
            let e = &self.entities[i];
            if e.hp <= 0. || e.status.stun_until > self.tick || e.status.root_until > self.tick {
                return false;
            }
            let start = e.position;
            let next = start.advance(d.goal, d.step);
            if !self.spell_motion_clear(start, next) || self.bulwark_blocks_leap(i, next) {
                return false;
            }
            let radius = e.stats.physiology.map_or(0., |p| p.collision_radius);
            self.place_spell_actor(i, next, events);
            for j in 0..self.entities.len() {
                let target = &self.entities[j];
                if target.hp <= 0.
                    || target.team == d.team
                    || !matches!(target.kind, Kind::Hero | Kind::Minion | Kind::Neutral)
                    || d.hit.contains(&target.id)
                {
                    continue;
                }
                if combat::line_distance(target.position, start, next)
                    > radius + target.stats.physiology.map_or(0., |p| p.collision_radius)
                    || !self.spell_motion_clear(next, target.position)
                {
                    continue;
                }
                d.hit.push(target.id);
                let amount = shop::mitigated(d.damage, target.stats.shield, d.pierce);
                self.hurt(d.source, d.team, j, amount, DamageSchool::Crystal, events);
            }
            next != d.goal
        });
        self.twisters = active;
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
        for (idx, e) in w.entities.iter_mut().take(6).enumerate() {
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
            w.advance_twisters(&mut ev);
        }
        assert_eq!(w.entities[0].position, p);
        assert_eq!(w.entities[0].energy, 140.);
        assert_eq!(w.entities[1].hp, 900.);
        assert_eq!(w.entities[3].hp, 900.);
        assert_eq!(w.entities[5].hp, 1000.);
        assert!(w.twisters.is_empty());
    }
}
