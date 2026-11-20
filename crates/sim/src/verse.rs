use super::*;
use crate::native_combat::{Damage, ranks};
use halcyon_data::{AbilityParameter as P, DamageOperand};

#[derive(Clone, Debug, Serialize)]
pub(super) struct Rules {
    damage: Damage<3>,
    radius: [f32; 3],
    stun: [f32; 3],
    channel: [f32; 3],
    pub fortified: [f32; 3],
    cooldown: [f32; 3],
    cost: [f32; 3],
}

#[derive(Clone, Copy, Debug, Serialize)]
pub(super) struct Channel {
    pub rank: usize,
    pub fortified: f32,
}

impl Rules {
    pub fn load(data: &GameData) -> Result<Option<Self>, String> {
        let Some(a) = data.ability("Adagio", "Ability__Adagio__C") else {
            return Ok(None);
        };
        Ok(Some(Self {
            damage: Damage::named(a, DamageOperand::Damage)?,
            radius: ranks(a, P::EffectRadius)?,
            stun: ranks(a, P::VerseStunDuration)?,
            channel: ranks(a, P::ChannelDuration)?,
            fortified: ranks(a, P::FortifiedHealthStrength)?,
            cooldown: ranks(a, P::Cooldown)?,
            cost: ranks(a, P::EnergyCost)?,
        }))
    }

    pub fn cast(&self, rank: usize) -> (f32, f32, u64, f32) {
        (
            0.,
            self.cost[rank],
            ticks(self.channel[rank]),
            self.cooldown[rank],
        )
    }
}

fn ticks(seconds: f32) -> u64 {
    (seconds * TICK_HZ as f32).round() as u64
}

impl Room<'_> {
    pub(super) fn verse_reaches(&self, i: usize, p: Position) -> bool {
        let e = &self.entities[i];
        self.abilities
            .verse
            .as_ref()
            .zip(e.ability_ranks[2].checked_sub(1))
            .is_some_and(|(rules, rank)| e.position.dist(p) <= rules.radius[usize::from(rank)])
    }

    pub(super) fn release_verse(&mut self, i: usize, rank: usize, events: &mut Vec<Event>) {
        let e = &self.entities[i];
        let rules = self.abilities.verse.as_ref().unwrap();
        let damage = rules.damage.at(rank, e.item_stats.crystal, e.stats.weapon);
        let radius = rules.radius[rank];
        let stun_end = self.tick + ticks(rules.stun[rank]);
        let (source, team, origin, pierce) = (e.id, e.team, e.position, e.item_stats.shield_pierce);
        events.push(Event::Cast {
            source,
            slot: AbilitySlot::C,
            target: CastTarget::SelfCast,
        });
        for j in 0..self.entities.len() {
            let t = &self.entities[j];
            if t.hp <= 0.
                || t.team == team
                || !matches!(t.kind, Kind::Hero | Kind::Minion | Kind::Neutral)
                || t.position.dist(origin) > radius
            {
                continue;
            }
            let amount = shop::mitigated(damage, t.stats.shield, pierce);
            if self.has_arcane_fire(t.id, source) {
                let t = &mut self.entities[j];
                t.status.stun_until = t.status.stun_until.max(stun_end);
                t.winding = None;
                t.cast = None;
                t.status.channel = 0;
                t.status.channel_until = 0;
            }
            self.hurt(source, team, j, amount, DamageSchool::Crystal, events);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires original delivery and supplementary combat data"]
    fn original_verse_channels_fortifies_and_stuns_only_burning_targets() {
        let data = GameData::load_with_extra(
            std::env::var_os("HALCYON_DELIVERY").unwrap(),
            Some(std::path::Path::new(
                &std::env::var_os("HALCYON_DATA_EXTRA").unwrap(),
            )),
        )
        .unwrap();
        let rules = Rules::load(&data).unwrap().unwrap();
        for (rank, (cost, cooldown, damage, fortified, stun)) in [
            (140., 100., 820., 300., 32),
            (190., 80., 1020., 550., 46),
            (240., 60., 1220., 800., 60),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(rules.cast(rank), (0., cost, 40, cooldown));
            assert_eq!(rules.damage.at(rank, 20., 1000.), damage);
            assert_eq!(rules.fortified[rank], fortified);
            assert_eq!(ticks(rules.stun[rank]), stun);
        }
        let mut w = World::from_data(12, 6, &data).unwrap();
        w.use_six_human_roster().unwrap();
        w.select_hero(0, "Adagio", &data).unwrap();
        w.use_aral_lane().unwrap();
        for (i, x) in [0., 1., -1., 9., -12., 9.01].into_iter().enumerate() {
            let e = &mut w.entities[i];
            e.position = Position { x, y: 0. };
            e.hp = 5000.;
            e.max_hp = 5000.;
            e.stats.shield = 0.;
        }
        w.entities[0].level = 12;
        w.entities[0].skill_points = 5;
        w.entities[0].energy = 1000.;
        w.entities[0].max_energy = 1000.;
        w.entities[0].item_stats.crystal = 20.;
        let cast = |slot| Command::Cast {
            slot,
            target: CastTarget::SelfCast,
        };
        w.command(1, Command::Learn(AbilitySlot::A)).unwrap();
        w.command(1, Command::Learn(AbilitySlot::C)).unwrap();
        assert!(
            w.command(
                1,
                Command::Cast {
                    slot: AbilitySlot::C,
                    target: CastTarget::Entity(2)
                }
            )
            .is_err()
        );
        w.command(1, cast(AbilitySlot::A)).unwrap();
        for _ in 0..4 {
            w.step();
        }
        assert!(w.has_arcane_fire(2, 1));
        assert_eq!(w.decide(0), cast(AbilitySlot::C));
        let energy = w.entities[0].energy;
        w.command(1, cast(AbilitySlot::C)).unwrap();
        assert_eq!(w.entities[0].energy, energy - 140.);
        assert_eq!(w.entities[0].cooldown[2], 2004);
        let mut events = vec![];
        w.hurt(2, 1, 0, 100., DamageSchool::Crystal, &mut events);
        assert_eq!(w.entities[0].hp, 4950.);
        w.hurt(2, 1, 0, 550., DamageSchool::Crystal, &mut events);
        assert_eq!(w.entities[0].hp, 4650.);
        w.command(1, Command::Learn(AbilitySlot::C)).unwrap();
        for _ in 0..39 {
            assert!(!w.step().iter().any(|e| matches!(
                e,
                Event::Cast {
                    source: 1,
                    slot: AbilitySlot::C,
                    ..
                }
            )));
        }
        let events = w.step();
        let impacts: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Event::Damage {
                    source: 1,
                    target,
                    amount,
                    ..
                } if *amount == 820. => Some(*target),
                _ => None,
            })
            .collect();
        assert_eq!(impacts, [2, 4]);
        assert_eq!(w.entities[1].status.stun_until, 76);
        assert_eq!(w.entities[3].status.stun_until, 0);
        assert!(w.command(1, cast(AbilitySlot::C)).is_err());

        w.entities[0].cooldown[2] = 0;
        w.entities[0].energy = 1000.;
        w.command(1, cast(AbilitySlot::C)).unwrap();
        assert_eq!(w.entities[0].energy, 810.);
        w.command(1, Command::Stop).unwrap();
        let hp = w.entities[0].hp;
        w.hurt(2, 1, 0, 100., DamageSchool::Crystal, &mut vec![]);
        assert_eq!(w.entities[0].hp, hp - 100.);
        for _ in 0..42 {
            assert!(!w.step().iter().any(|e| matches!(
                e,
                Event::Cast {
                    source: 1,
                    slot: AbilitySlot::C,
                    ..
                }
            )));
        }
        assert!(w.command(1, cast(AbilitySlot::C)).is_err());
    }
}
