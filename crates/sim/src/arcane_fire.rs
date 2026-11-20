use super::*;
use crate::native_combat::{Damage, ranks};
use halcyon_data::{AbilityParameter as P, DamageOperand};
#[derive(Debug, Clone, Serialize)]
pub(super) struct Rules {
    heal: Damage<5>,
    burst: Damage<5>,
    burn: Damage<5>,
    pub agent: Damage<5>,
    bonus_hp: [f32; 5],
    radius: [f32; 5],
    duration: [f32; 5],
    heal_duration: [f32; 5],
    hz: [f32; 5],
    slow: [f32; 5],
    slow_time: [f32; 5],
}
impl Rules {
    pub fn load(d: &GameData) -> Result<Option<Self>, String> {
        let Some(a) = d.ability("Adagio", "Ability__Adagio__A") else {
            return Ok(None);
        };
        let b = d
            .ability("Adagio", "Ability__Adagio__B")
            .ok_or("missing Adagio B")?;
        Ok(Some(Self {
            heal: Damage::variable(a, P::HealPerSecond)?,
            burst: Damage::variable(a, P::BurstHealStrength)?,
            burn: Damage::named(a, DamageOperand::BurnDamagePerSecond)?,
            agent: Damage::variable(b, P::BurnBonusDamage)?,
            bonus_hp: ranks(a, P::BurstHealHealthRatio)?,
            radius: ranks(a, P::BurnRadius)?,
            duration: ranks(a, P::ArcaneFireDuration)?,
            heal_duration: ranks(a, P::HealDuration)?,
            hz: ranks(a, P::TicksPerSecond)?,
            slow: ranks(a, P::ArcaneSlowStrength)?,
            slow_time: ranks(a, P::ArcaneSlowDuration)?,
        }))
    }
}
#[derive(Clone, Copy, Debug, Serialize)]
pub(super) struct Pulse {
    src: u32,
    team: u8,
    target: u32,
    heal: bool,
    amount: f32,
    pierce: f32,
    period: u64,
    next: u64,
    end: u64,
}
impl World {
    pub(super) fn adagio_heal_order(&self, i: usize) -> Option<Command> {
        self.abilities.fire.as_ref()?;
        let hero = &self.entities[i];
        self.entities
            .iter()
            .filter(|t| {
                t.kind == Kind::Hero && t.team == hero.team && t.hp > 0. && t.hp < t.max_hp * 0.8
            })
            .filter_map(|t| {
                let target = CastTarget::Entity(t.id);
                self.can_cast(i, AbilitySlot::A, target)
                    .then_some((t.hp / t.max_hp, target))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, target)| Command::Cast {
                slot: AbilitySlot::A,
                target,
            })
    }
    pub(super) fn has_arcane_fire(&self, target: u32, caster: u32) -> bool {
        self.arcane_fire
            .iter()
            .any(|p| !p.heal && p.target == target && p.src == caster && self.tick < p.end)
    }
    pub(super) fn gift_of_fire(&mut self, i: usize, j: usize, r: usize, events: &mut Vec<Event>) {
        let a = self.abilities.fire.as_ref().unwrap();
        let caster = &self.entities[i];
        let (src, team, cp, pierce) = (
            caster.id,
            caster.team,
            caster.item_stats.crystal,
            caster.item_stats.shield_pierce,
        );
        let burst = a.burst.at(r, cp, 0.) + caster.bonus_hp * a.bonus_hp[r];
        let pos = self.entities[j].position;
        let period = (TICK_HZ as f32 / a.hz[r]).round().max(1.) as u64;
        let heal = Pulse {
            src,
            team,
            target: self.entities[j].id,
            heal: true,
            amount: a.heal.at(r, cp, 0.) * period as f32 / TICK_HZ as f32,
            pierce,
            period,
            next: self.tick + period,
            end: self.tick + (a.heal_duration[r] * TICK_HZ as f32).round() as u64,
        };
        let burn = Pulse {
            heal: false,
            amount: a.burn.at(r, cp, 0.),
            period: TICK_HZ,
            next: self.tick + TICK_HZ,
            end: self.tick + (a.duration[r] * TICK_HZ as f32).round() as u64,
            ..heal
        };
        let radius = a.radius[r];
        let slow = 1. - a.slow[r];
        let slow_end = self.tick + (a.slow_time[r] * TICK_HZ as f32).round() as u64;
        self.entities[i].next_attack = self.tick;
        self.heal(src, j, burst, events);
        self.arcane_fire
            .retain(|p| !(p.heal && p.src == src && p.target == heal.target));
        self.arcane_fire.push(heal);
        for t in &mut self.entities {
            if t.hp <= 0.
                || t.team == team
                || !matches!(t.kind, Kind::Hero | Kind::Minion | Kind::Neutral)
                || t.position.dist(pos) > radius
                || !self.layout.clear(pos, t.position)
            {
                continue;
            }
            if i == j {
                t.status.slow_factor = slow;
                t.status.slow_until = slow_end;
            }
            self.arcane_fire
                .retain(|p| p.heal || p.src != src || p.target != t.id);
            self.arcane_fire.push(Pulse {
                target: t.id,
                ..burn
            });
        }
    }
    pub(super) fn advance_arcane_fire(&mut self, events: &mut Vec<Event>) {
        if self.arcane_fire.is_empty() {
            return;
        }
        let mut active = std::mem::take(&mut self.arcane_fire);
        active.retain_mut(|p| {
            let Some(j) = self.index(p.target) else {
                return false;
            };
            if self.entities[j].hp <= 0. || self.tick > p.end {
                return false;
            }
            if self.tick >= p.next {
                if p.heal {
                    self.heal(p.src, j, p.amount, events)
                } else {
                    let dmg = shop::mitigated(p.amount, self.entities[j].stats.shield, p.pierce);
                    self.hurt(p.src, p.team, j, dmg, DamageSchool::Crystal, events)
                }
                p.next += p.period;
            }
            self.tick < p.end
        });
        self.arcane_fire = active;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original delivery and supplemental combat data"]
    fn gift_resets_attacks_burns_and_heals_and_empowered_hits_use_the_casters_fire() {
        let d = GameData::load_with_extra(
            std::env::var_os("HALCYON_DELIVERY").unwrap(),
            Some(std::path::Path::new(
                &std::env::var_os("HALCYON_DATA_EXTRA").unwrap(),
            )),
        )
        .unwrap();
        let mut w = World::from_data(12, 6, &d).unwrap();
        w.use_six_human_roster().unwrap();
        w.select_hero(0, "Adagio", &d).unwrap();
        w.use_aral_lane().unwrap();
        for (idx, e) in w.entities.iter_mut().take(6).enumerate() {
            e.position = Position {
                x: idx as f32 * 2.,
                y: 0.,
            };
            e.stats.shield = 0.;
            e.hp = 2000.;
            e.max_hp = 2000.;
        }
        w.entities[0].hp = 800.;
        w.entities[0].energy = 500.;
        w.entities[0].skill_points = 2;
        w.entities[0].item_stats.crystal = 20.;
        w.entities[0].next_attack = 90;
        w.command(1, Command::Learn(AbilitySlot::A)).unwrap();
        w.command(1, Command::Learn(AbilitySlot::B)).unwrap();
        let gift = w.decide(0);
        assert_eq!(
            gift,
            Command::Cast {
                slot: AbilitySlot::A,
                target: CastTarget::Entity(1)
            }
        );
        w.command(1, gift).unwrap();
        w.tick = 4;
        let mut events = vec![];
        w.advance_cast(0, &mut events);
        assert_eq!(w.entities[0].hp, 846.);
        assert_eq!(w.entities[0].next_attack, 4);
        assert!(w.has_arcane_fire(2, 1));
        assert!(!w.has_arcane_fire(2, 3));
        assert_eq!(w.entities[1].status.slow_until, 22);
        assert!(!w.has_arcane_fire(4, 1));
        for t in 5..=64 {
            w.tick = t;
            w.advance_arcane_fire(&mut events);
        }
        assert_eq!(w.entities[0].hp, 882.);
        assert_eq!(w.entities[1].hp, 1952.);
        w.command(
            1,
            Command::Cast {
                slot: AbilitySlot::B,
                target: CastTarget::SelfCast,
            },
        )
        .unwrap();
        w.tick = 67;
        w.advance_cast(0, &mut events);
        w.command(1, Command::Attack(2)).unwrap();
        let more: Vec<_> = (0..14).flat_map(|_| w.step()).collect();
        assert!(more.iter().any(
            |e| matches!(e,Event::Damage{source:1,target:2,amount,..} if (*amount-70.).abs()<0.01)
        ));
    }
}
