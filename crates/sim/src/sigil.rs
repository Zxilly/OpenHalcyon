use super::*;
use crate::native_combat::{Damage, ranks};
use halcyon_data::AbilityParameter as P;
#[derive(Debug, Clone, Serialize)]
pub(super) struct Rules {
    startup: [f32; 5],
    duration: [f32; 5],
    radius: [f32; 5],
    passive_decay: [f32; 5],
    active_decay: [f32; 5],
    dps: Damage<5>,
    heal: [f32; 5],
    hp_ratio: [f32; 5],
    detonate: Damage<5>,
    ratio: [f32; 5],
    speed: [f32; 5],
    speed_time: [f32; 5],
    minion_reduction: [f32; 5],
}
impl Rules {
    pub fn load(d: &GameData) -> Result<Option<Self>, String> {
        let Some(a) = d.ability("Lyra", "Ability__Lyra__A") else {
            return Ok(None);
        };
        Ok(Some(Self {
            startup: ranks(a, P::Startup)?,
            duration: ranks(a, P::Duration)?,
            radius: ranks(a, P::Radius)?,
            passive_decay: ranks(a, P::PassiveDecay)?,
            active_decay: ranks(a, P::ActiveDecay)?,
            dps: Damage::variable(a, P::SigilDamagePerSecond)?,
            heal: ranks(a, P::HealPerSecond)?,
            hp_ratio: ranks(a, P::HealPerSecondHealthRatio)?,
            detonate: Damage::variable(a, P::DetonateDamageHeroes)?,
            ratio: ranks(a, P::DetonationEffectRatio)?,
            speed: ranks(a, P::DecayingSpeedBoost)?,
            speed_time: ranks(a, P::SpeedBoostDuration)?,
            minion_reduction: ranks(a, P::MinionDamageReduction)?,
        }))
    }
    pub fn startup(&self, r: usize) -> u64 {
        (self.startup[r] * TICK_HZ as f32).round() as u64
    }
}
#[derive(bevy_ecs::prelude::Component, Clone, Copy, Debug, Serialize)]
pub(super) struct Field {
    pub source: u32,
    team: u8,
    center: Position,
    radius: f32,
    charge: f32,
    passive: f32,
    active: f32,
    heal: f32,
    damage: f32,
    pierce: f32,
    burst: f32,
    ratio: f32,
    speed: f32,
    speed_ticks: u64,
    minion_factor: f32,
    next: u64,
}
impl Field {
    fn contains(&self, w: &World, p: Position) -> bool {
        p.dist(self.center) <= self.radius && w.layout.clear(self.center, p)
    }
}
impl World {
    pub(super) fn start_sigil(&mut self, i: usize, r: usize, center: Position) {
        let a = self.abilities.sigil.as_ref().unwrap();
        let e = &self.entities[i];
        let cp = e.item_stats.crystal;
        self.entities.spawn_spell(Field {
            source: e.id,
            team: e.team,
            center,
            radius: a.radius[r],
            charge: a.duration[r] * TICK_HZ as f32,
            passive: a.passive_decay[r],
            active: a.active_decay[r],
            heal: a.heal[r] + a.hp_ratio[r] * e.bonus_hp,
            damage: a.dps.at(r, cp, 0.),
            pierce: e.item_stats.shield_pierce,
            burst: a.detonate.at(r, cp, 0.),
            ratio: a.ratio[r],
            speed: a.speed[r],
            speed_ticks: (a.speed_time[r] * TICK_HZ as f32).round() as u64,
            minion_factor: 1. - a.minion_reduction[r],
            next: self.tick + TICK_HZ,
        });
    }
    pub(super) fn sigil_sight(&self, team: u8, p: Position) -> bool {
        self.entities.spells::<Field>().any(|f| {
            f.team == team
                && f.contains(self, p)
                && self
                    .native_navigation
                    .as_ref()
                    .is_none_or(|map| map.can_see(f.center, p))
        })
    }
    pub(super) fn sigil_order(&self, i: usize, enemy: &Entity) -> Option<Command> {
        let e = &self.entities[i];
        self.abilities.sigil.as_ref()?;
        let target = if let Some(f) = self.entities.spells::<Field>().find(|f| f.source == e.id) {
            if !f.contains(self, enemy.position)
                || enemy.hp > f.burst && f.charge > 3. * TICK_HZ as f32
            {
                return None;
            }
            CastTarget::SelfCast
        } else {
            CastTarget::Point(enemy.position)
        };
        self.can_cast(i, AbilitySlot::A, target)
            .then_some(Command::Cast {
                slot: AbilitySlot::A,
                target,
            })
    }
    pub(super) fn advance_sigils(&mut self, events: &mut Vec<Event>) {
        self.advance_spell_family::<Field>(events);
    }
    pub(super) fn detonate_sigil(&mut self, i: usize, events: &mut Vec<Event>) {
        let source = self.entities[i].id;
        let Some(f) = self.entities.remove_spell::<Field>(|f| f.source == source) else {
            return;
        };
        events.push(Event::Cast {
            source: f.source,
            slot: AbilitySlot::A,
            target: CastTarget::SelfCast,
        });
        for j in 0..self.entities.len() {
            let t = &self.entities[j];
            if t.hp <= 0. || !f.contains(self, t.position) {
                continue;
            }
            if t.team == f.team && t.kind == Kind::Hero {
                let s = &mut self.entities[j].status;
                s.sigil_speed = f.speed;
                s.sigil_speed_until = self.tick + f.speed_ticks;
                s.sigil_speed_duration = f.speed_ticks;
                self.heal(
                    f.source,
                    j,
                    f.heal * f.charge / TICK_HZ as f32 * f.ratio,
                    events,
                );
            } else if t.team != f.team && matches!(t.kind, Kind::Hero | Kind::Minion) {
                let amount = shop::mitigated(
                    if t.kind == Kind::Minion {
                        f.burst * f.minion_factor
                    } else {
                        f.burst
                    },
                    t.stats.shield,
                    f.pierce,
                );
                self.hurt(f.source, f.team, j, amount, DamageSchool::Crystal, events);
            }
        }
    }
}
impl crate::persistent_spells::TimedSpell for Field {
    fn expired_before_step(&self, _: u64) -> bool {
        false
    }
    fn begin_impact(&mut self) -> Self {
        *self
    }
    fn advance(&mut self, world: &mut World, events: &mut Vec<Event>) -> bool {
        let pulse = world.tick >= self.next;
        let mut count = 0;
        for j in 0..world.entities.len() {
            let t = &world.entities[j];
            if t.kind != Kind::Hero || t.hp <= 0. || !self.contains(world, t.position) {
                continue;
            }
            if self.team == t.team {
                if t.hp >= t.max_hp {
                    continue;
                }
                count += 1;
                if pulse {
                    world.heal(self.source, j, self.heal, events)
                }
            } else {
                count += 1;
                if pulse {
                    let damage = shop::mitigated(self.damage, t.stats.shield, self.pierce);
                    world.hurt(
                        self.source,
                        self.team,
                        j,
                        damage,
                        DamageSchool::Crystal,
                        events,
                    )
                }
            }
        }
        if pulse {
            self.next += TICK_HZ;
        }
        self.charge -= if count == 0 {
            self.passive
        } else {
            self.active * count as f32
        };
        self.charge > 0.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original delivery and supplemental combat data"]
    fn sigil_charges_deplete_by_hero_exposure_and_reactivation_consumes_only_its_remaining_healing()
    {
        let d = GameData::load_with_extra(
            std::env::var_os("HALCYON_DELIVERY").unwrap(),
            Some(std::path::Path::new(
                &std::env::var_os("HALCYON_DATA_EXTRA").unwrap(),
            )),
        )
        .unwrap();
        let mut w = World::from_data(12, 6, &d).unwrap();
        w.use_six_human_roster().unwrap();
        w.select_hero(0, "Lyra", &d).unwrap();
        w.use_aral_lane().unwrap();
        for idx in 0..w.entities.len().min(6) {
            let t = &mut w.entities[idx];
            t.position = Position {
                x: 35. + idx as f32,
                y: 0.,
            };
            t.hp = 1000.;
            t.max_hp = 1000.;
            t.stats.shield = 0.;
        }
        w.entities[0].position = Position::default();
        w.entities[0].skill_points = 1;
        w.entities[0].energy = 200.;
        w.entities[0].item_stats.crystal = 20.;
        w.entities[1].position = Position { x: 9., y: 0. };
        w.entities[2].position = Position { x: 8., y: 0. };
        w.entities[2].hp = 500.;
        let point = w.entities[1].position;
        let cast = Command::Cast {
            slot: AbilitySlot::A,
            target: CastTarget::Point(point),
        };
        let again = Command::Cast {
            slot: AbilitySlot::A,
            target: CastTarget::SelfCast,
        };
        w.command(1, Command::Learn(AbilitySlot::A)).unwrap();
        assert!(w.command(1, again).is_err());
        assert_eq!(w.decide(0), cast);
        w.command(1, cast).unwrap();
        w.tick = 4;
        let mut events = vec![];
        w.advance_cast(0, &mut events);
        assert!(w.sigil_sight(0, point));
        assert!(!w.sigil_sight(1, point));
        assert_eq!(w.entities[0].energy, 120.);
        assert!(w.command(1, cast).is_err());
        let cooldown = w.entities[0].cooldown[0];
        for t in 5..=24 {
            w.tick = t;
            w.advance_sigils(&mut events);
        }
        assert_eq!(w.entities[1].hp, 925.);
        assert_eq!(w.entities[2].hp, 525.);
        assert_eq!(w.entities.spells::<Field>().next().unwrap().charge, 80.);
        let minion = w.spawn(1, Kind::Minion, 4, point, false);
        let mi = w.index(minion).unwrap();
        w.entities[mi].stats.shield = 0.;
        let hp = w.entities[mi].hp;
        w.command(1, again).unwrap();
        w.tick = 25;
        w.advance_cast(0, &mut events);
        assert_eq!(w.entities[1].hp, 833.);
        assert_eq!(w.entities[2].hp, 555.);
        assert_eq!(w.entities[mi].hp, hp - 46.);
        assert_eq!(w.entities[0].cooldown[0], cooldown);
        assert_eq!(w.entities[0].energy, 120.);
        assert!(w.entities.spells::<Field>().next().is_none());
        assert!(!w.sigil_sight(0, point));
        assert!(w.command(1, again).is_err());
    }
}
