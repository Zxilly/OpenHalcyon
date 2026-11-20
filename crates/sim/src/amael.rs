use super::*;
use crate::native_combat::{Damage, ranks};
use halcyon_data::{AbilityParameter as P, DamageOperand};
#[derive(Clone, Debug, Serialize)]
pub(super) struct Rules {
    pub damage: Damage<3>,
    range: [f32; 3],
    cost: [f32; 3],
    cooldown: [f32; 3],
}
impl Rules {
    pub fn load(data: &GameData) -> Result<Option<Self>, String> {
        let Some(a) = data.ability("Amael", "Ability__Amael__C") else {
            return Ok(None);
        };
        Ok(Some(Self {
            damage: Damage::named(a, DamageOperand::UppercaseDamage)?,
            range: ranks(a, P::Range)?,
            cost: ranks(a, P::UppercaseEnergyCost)?,
            cooldown: ranks(a, P::Cooldown)?,
        }))
    }
    pub fn cast(&self, r: usize) -> (f32, f32, u64, f32) {
        (self.range[r], self.cost[r], 1, self.cooldown[r])
    }
}
#[derive(Clone, Copy, Debug, Serialize)]
pub(super) struct Flight {
    pub target: u32,
    pub goal: Position,
    pub range: f32,
    pub step: f32,
    pub elapsed: u8,
}
impl Flight {
    pub fn advance(
        &mut self,
        w: &mut Room<'_>,
        source: u32,
        team: u8,
        raw: f32,
        events: &mut Vec<Event>,
    ) -> bool {
        let (Some(i), Some(j)) = (w.index(source), w.index(self.target)) else {
            return false;
        };
        let (caster, target) = (&w.entities[i], &w.entities[j]);
        if caster.hp <= 0.
            || caster.status.stun_until > w.tick
            || caster.status.root_until > w.tick
            || target.hp <= 0.
        {
            return false;
        }
        let start = caster.position;
        let next = start.advance(self.goal, self.step);
        if !w.spell_motion_clear(start, next) || w.bulwark_blocks_leap(i, next) {
            return false;
        }
        self.elapsed += 1;
        if next != start {
            w.place_spell_actor(i, next, events);
        }
        if self.elapsed == 2 {
            let target = &w.entities[j];
            if !w.visible(team, target)
                || next.dist(target.position) > self.range
                || target.status.stun_until > w.tick
                || target.status.root_until > w.tick
                || target.status.blocks_movement_impairment(w.tick)
                || !w.spell_motion_clear(next, target.position)
            {
                return false;
            }
            let damage =
                shop::mitigated(raw, target.armor(), w.entities[i].item_stats.armor_pierce);
            let until = w.tick + 30;
            let target = &mut w.entities[j];
            target.status.reveal_until = target.status.reveal_until.max(until);
            target.cast = None;
            target.winding = None;
            target.order = Command::Stop;
            let instance = w.next_id;
            w.next_id += 1;
            events.push(Event::AmaelKick {
                source,
                target: self.target,
                instance,
            });
            w.hurt_with_effect(
                source,
                team,
                j,
                damage,
                (DamageSchool::Weapon, ImpactEffect::AmaelKick),
                events,
            );
        }
        self.elapsed < 6
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    #[ignore = "requires original delivery and supplemental combat data"]
    fn ordinary_kick_learns_costs_lunges_and_hits_only_the_visible_enemy() {
        let data = GameData::load_with_extra(
            std::env::var_os("HALCYON_DELIVERY").unwrap(),
            Some(Path::new(&std::env::var_os("HALCYON_DATA_EXTRA").unwrap())),
        )
        .unwrap();
        let mut w = World::from_data(9, 6, &data).unwrap();
        w.use_six_human_roster().unwrap();
        w.select_hero(0, "Amael", &data).unwrap();
        w.use_aral_lane().unwrap();
        let start = Position {
            x: 20.01865,
            y: 0.72592,
        };
        let victim = Position { x: 23.5, y: 5.5 };
        w.entities[0].position = start;
        w.entities[1].position = victim;
        w.entities[0].level = 6;
        w.entities[0].skill_points = 1;
        w.entities[0].max_energy = 400.;
        w.entities[0].energy = 400.;
        w.entities[1].stats.armor = 0.;
        w.entities[0].stats.weapon = 100.;
        w.entities[0].item_stats.crystal = 50.;
        w.entities[1].hp = 2000.;
        w.entities[1].max_hp = 2000.;
        let skill = AbilitySlot::C;
        w.command(1, Command::Learn(skill)).unwrap();
        let cast = |target| Command::Cast {
            slot: skill,
            target: CastTarget::Entity(target),
        };
        assert!(w.command(1, cast(1)).is_err());
        assert!(w.command(1, cast(3)).is_err());
        w.entities[1].status.root_until = 6;
        assert!(w.command(1, cast(2)).is_err());
        w.entities[1].status.root_until = 0;
        w.entities[1].position.x = 40.;
        assert!(w.command(1, cast(2)).is_err());
        w.entities[1].position = victim;
        w.entities[0].energy = 79.;
        assert!(w.command(1, cast(2)).is_err());
        w.entities[0].energy = 400.;
        assert_eq!(w.decide(0), cast(2));
        w.command(1, cast(2)).unwrap();
        let mut ev = vec![];
        w.tick = 1;
        w.advance_cast(0, &mut ev);
        assert_eq!(w.entities[0].energy, 320.);
        assert_eq!(w.entities[0].cooldown[2], 1601);
        for t in 2..=7 {
            w.tick = t;
            w.advance_spell_family::<crate::combat::SpellEffect>(&mut ev);
            if t == 2 {
                assert_eq!(w.entities[1].hp, 2000.);
            }
        }
        let i = ev
            .iter()
            .position(|e| {
                matches!(
                    e,
                    Event::AmaelKick {
                        source: 1,
                        target: 2,
                        ..
                    }
                )
            })
            .unwrap();
        assert!(
            matches!(ev[i+1],Event::Damage{effect:ImpactEffect::AmaelKick,source:1,target:2,amount,..} if (amount-540.).abs()<0.001)
        );
        assert_eq!(w.entities[1].hp, 1460.);
        assert_eq!(w.entities[1].status.reveal_until, 33);
        assert_eq!(w.entities[1].status.stun_until, 0);
        assert!((w.entities[0].position.dist(victim) - 1.8).abs() < 0.001);
        assert!(w.command(1, cast(2)).is_err());
    }
}
