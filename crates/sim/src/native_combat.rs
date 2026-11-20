//! Rank parameters for modeled releases whose damage block has a single verified role.
use halcyon_data::{GameData, combat_content::Ability};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(bound(serialize = "[f32; N]: Serialize"))]
pub(super) struct Damage<const N: usize> {
    ranks: [f32; N],
    crystal: f32,
    weapon: f32,
}
impl<const N: usize> Damage<N> {
    fn named(a: &Ability, label: &str) -> Result<Self, String> {
        let block = a
            .damage(label)
            .ok_or_else(|| format!("missing or ambiguous {} damage {label}", a.ability_id))?;
        Ok(Self {
            ranks: block
                .ranks
                .as_slice()
                .try_into()
                .map_err(|_| format!("invalid damage rank table {}", a.ability_id))?,
            crystal: block.cp_ratio,
            weapon: block.wp_ratio,
        })
    }
    pub fn at(&self, rank: usize, crystal: f32, weapon: f32) -> f32 {
        self.ranks[rank] + self.crystal * crystal + self.weapon * weapon
    }
}
fn ranks<const N: usize>(a: &Ability, key: &str) -> Result<[f32; N], String> {
    a.variable(key)
        .and_then(|v| v.ranks.as_deref())
        .and_then(|r| r.try_into().ok())
        .ok_or_else(|| format!("invalid {}.{key} ranks", a.ability_id))
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct Achilles {
    pub damage: Damage<5>,
    pub duration: [f32; 5],
    pub slow: [f32; 5],
}
impl Achilles {
    pub fn load(data: &GameData) -> Result<Option<Self>, String> {
        let Some(a) = data.ability("Ringo", "Ability__Ringo__A") else {
            return Ok(None);
        };
        Ok(Some(Self {
            damage: Damage::named(a, "Damage")?,
            duration: ranks(a, "SlowDuration")?,
            slow: ranks(a, "SlowStrength")?,
        }))
    }
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct ChakramRanks {
    pub damage: Damage<5>,
    pub stamina_from_hero: [f32; 5],
}
impl ChakramRanks {
    pub fn load(data: &GameData) -> Result<Option<Self>, String> {
        let Some(a) = data.ability("Idris", "Ability__Idris__B") else {
            return Ok(None);
        };
        Ok(Some(Self {
            damage: Damage::named(a, "Damage on Passthrough")?,
            stamina_from_hero: ranks(a, "Stamina From Hero Impacts")?,
        }))
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub(super) struct LongShot {
    pub damage: f32,
    pub range: f32,
    pub cooldown: f32,
}
impl super::World {
    /// Applies a trusted seat's already authenticated, named talent. Numeric wire slots
    /// do not identify a talent by themselves. Callers should clear on unequip.
    pub fn set_named_talent(
        &mut self,
        actor: u32,
        equipped: Option<(&str, std::num::NonZeroU8)>,
        data: &GameData,
    ) -> Result<(), &'static str> {
        if !matches!(self.layout, super::Layout::Aral { .. }) {
            return Err("talents unavailable for mode");
        }
        let i = self.index(actor).ok_or("unknown actor")?;
        let e = &mut self.entities[i];
        if e.kind != super::Kind::Hero {
            return Err("not a hero");
        }
        let Some((id, level)) = equipped else {
            e.long_shot = None;
            return Ok(());
        };
        let talent = data.talent_definition(id).ok_or("unknown talent")?;
        if id != "Talent_Ringo_LongShot"
            || talent.hero.as_deref() != Some("Ringo")
            || e.type_id != super::EntityTemplate::Ringo.as_raw()
        {
            return Err("unsupported talent for hero");
        }
        let value = |label: &str| {
            talent
                .modifier(label)
                .and_then(|m| m.value_at(level, e.item_stats.crystal, e.stats.weapon))
                .ok_or("unsupported talent modifier")
        };
        e.long_shot = Some(LongShot {
            range: value("BONUS_RANGE")?,
            damage: value("DAMAGE_MODIFIER")?,
            cooldown: value("COOLDOWN_MODIFIER")?,
        });
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct TwirlingSilver {
    pub damage: Damage<5>,
    pub duration: [f32; 5],
    pub attack_speed: [f32; 5],
    pub move_speed: [f32; 5],
}
impl TwirlingSilver {
    pub fn load(data: &GameData) -> Result<Option<Self>, String> {
        let Some(a) = data.ability("Ringo", "Ability__Ringo__B") else {
            return Ok(None);
        };
        Ok(Some(Self {
            damage: Damage::named(a, "BonusCrystalDamage")?,
            duration: ranks(a, "SpeedBuffDuration")?,
            attack_speed: ranks(a, "BonusASPDStrength")?,
            move_speed: ranks(a, "BonusMSPDStrength")?,
        }))
    }
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct LorelaiRanks {
    pub impact: Damage<5>,
    pub per_second: Damage<5>,
    pub stun: [f32; 5],
    pub hp_to_stun: [f32; 5],
    pub stun_cap: [f32; 5],
    pub empowered_stun: [f32; 5],
    pub snap_delay: [f32; 5],
    pub splash_delay: [f32; 5],
    pub empowered_delay: [f32; 5],
    pub slow: [f32; 5],
    pub hp_to_slow: [f32; 5],
    pub slow_cap: [f32; 5],
    pub speed: [f32; 5],
    pub barrier: [f32; 3],
    pub empowered_barrier: [f32; 3],
    pub hp_to_barrier: [f32; 3],
    pub crystal_to_barrier: f32,
    pub crystal_to_empowered_barrier: f32,
    pub duration: [f32; 3],
    pub empowered_duration: [f32; 3],
}
impl LorelaiRanks {
    pub fn load(data: &GameData) -> Result<Option<Self>, String> {
        let Some(a) = data.ability("Lorelai", "Ability__Lorelai__A") else {
            return Ok(None);
        };
        let b = data
            .ability("Lorelai", "Ability__Lorelai__B")
            .ok_or("missing Lorelai B")?;
        let c = data
            .ability("Lorelai", "Ability__Lorelai__C")
            .ok_or("missing Lorelai C")?;
        Ok(Some(Self {
            impact: Damage::named(a, "IMPACT_DAMAGE")?,
            per_second: Damage::named(a, "DAMAGE_PER_SEC")?,
            stun: ranks(a, "STUN_DURATION")?,
            hp_to_stun: ranks(a, "STUN_DURATION_RATIO")?,
            stun_cap: ranks(a, "STUN_DURATION_CAP")?,
            empowered_stun: ranks(a, "EMP_STUN_MULTIPLIER")?,
            snap_delay: ranks(a, "REG_DELAY_TIME")?,
            splash_delay: ranks(b, "SHOT_SPEED")?,
            empowered_delay: ranks(b, "SHOT_SPEED_EMPOWERED")?,
            slow: ranks(b, "SLOW")?,
            hp_to_slow: ranks(b, "SLOW_RATIO")?,
            slow_cap: ranks(b, "SLOW_CAP")?,
            speed: ranks(b, "SPEED_BOOST")?,
            barrier: ranks(c, "BARRIER_AMOUNT")?,
            empowered_barrier: ranks(c, "E_BARRIER_AMOUNT")?,
            hp_to_barrier: ranks(c, "BARRIER_RATIO")?,
            crystal_to_barrier: c.variable("BARRIER_AMOUNT").unwrap().cp_ratio,
            crystal_to_empowered_barrier: c.variable("E_BARRIER_AMOUNT").unwrap().cp_ratio,
            duration: ranks(c, "DURATION")?,
            empowered_duration: ranks(c, "E_DURATION")?,
        }))
    }
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct Hellfire {
    pub impact: Damage<3>,
    pub burn: Damage<3>,
    pub radius: [f32; 3],
    pub burn_radius: [f32; 3],
    pub duration: [f32; 3],
}
impl Hellfire {
    pub fn load(data: &GameData) -> Result<Option<Self>, String> {
        let Some(c) = data.ability("Ringo", "Ability__Ringo__C") else {
            return Ok(None);
        };
        let frequency: [f32; 3] = ranks(c, "TicksPerSecond")?;
        if frequency != [1.; 3] {
            return Err("unsupported Hellfire tick frequency".into());
        }
        Ok(Some(Self {
            impact: Damage::named(c, "ImpactDamage")?,
            burn: Damage::named(c, "BurnDamagePerSecond")?,
            radius: ranks(c, "ImpactSplashRadius")?,
            burn_radius: ranks(c, "BurnRadius")?,
            duration: ranks(c, "Burn_Duration")?,
        }))
    }
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct OtherSpells {
    pub gift: Damage<5>,
    pub agent: Damage<5>,
    pub self_agent: Damage<5>,
    pub agent_duration: [f32; 5],
    pub sigil_dps: Damage<5>,
    pub sigil_radius: [f32; 5],
    pub saw_dps: Damage<5>,
    pub saw_duration: [f32; 5],
    pub saw_ticks: [f32; 5],
    pub saw_slow: [f32; 5],
    pub saw_slow_duration: [f32; 5],
    pub saw_minion_reduction: [f32; 5],
    pub shin: Damage<3>,
    pub karas: Damage<5>,
}
impl<const N: usize> Damage<N> {
    fn variable(a: &Ability, key: &str) -> Result<Self, String> {
        let ranks = ranks(a, key)?;
        let v = a.variable(key).expect("rank table already found");
        Ok(Self {
            ranks,
            crystal: v.cp_ratio,
            weapon: v.wp_ratio,
        })
    }
}
impl OtherSpells {
    pub fn load(data: &GameData) -> Result<Option<Self>, String> {
        if data.extra.combat_content().heroes().len() == 0 {
            return Ok(None);
        }
        let get = |h: &str, id: &str| {
            data.ability(h, id)
                .ok_or_else(|| format!("missing ability {id}"))
        };
        let gift = get("Adagio", "Ability__Adagio__A")?;
        let agent = get("Adagio", "Ability__Adagio__B")?;
        let sigil = get("Lyra", "Ability__Lyra__A")?;
        let saw = get("SAW", "Ability__SAW__B")?;
        let saw_ticks = ranks(saw, "TicksDuringChannel")?;
        if saw_ticks
            .iter()
            .any(|&x| !(1. ..=255.).contains(&x) || x.fract() != 0.)
        {
            return Err("invalid SAW channel pulse count".into());
        }
        let shin = get("Shin", "Ability__Shin__C")?;
        let wind = get("Karas", "Ability__Karas__B")?;
        Ok(Some(Self {
            gift: Damage::variable(gift, "BurstHealStrength")?,
            agent: Damage::variable(agent, "BaseBonusDamage")?,
            self_agent: Damage::variable(agent, "SelfBonusDamage")?,
            agent_duration: ranks(agent, "Stack_Duration")?,
            sigil_dps: Damage::variable(sigil, "DamagePerSecond")?,
            sigil_radius: ranks(sigil, "Radius")?,
            saw_dps: Damage::variable(saw, "DAMAGE_PER_SECOND")?,
            saw_duration: ranks(saw, "Channel_Duration")?,
            saw_ticks,
            saw_slow: ranks(saw, "SlowStrength")?,
            saw_slow_duration: ranks(saw, "SlowDuration")?,
            saw_minion_reduction: ranks(saw, "MINION_DAMAGE_REDUCTION")?,
            shin: Damage::named(shin, "Damage")?,
            karas: Damage::named(wind, "DAMAGE")?,
        }))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AbilitySlot, CastTarget, Command, Event, Position, World};
    use std::{num::NonZeroU8, path::Path};
    fn original() -> GameData {
        let base = std::env::var_os("HALCYON_DELIVERY").unwrap();
        let extra = std::env::var_os("HALCYON_DATA_EXTRA").unwrap();
        GameData::load_with_extra(base, Some(Path::new(&extra))).unwrap()
    }
    fn room(data: &GameData, hero: &str) -> World {
        let mut w = World::from_data(42, 6, data).unwrap();
        w.use_six_human_roster().unwrap();
        w.select_hero(0, hero, data).unwrap();
        w.use_aral_lane().unwrap();
        w.entities[0].position = Position { x: 0., y: 0. };
        w.entities[1].position = Position { x: 10., y: 0. };
        w
    }
    #[test]
    #[ignore = "requires original delivery and supplemental combat data"]
    fn waterwall_reads_rank_crystal_and_bonus_hp_without_unlearned_splash() {
        let data = original();
        let mut w = room(&data, "Lorelai");
        w.entities[0].level = 6;
        w.entities[0].bonus_hp = 200.;
        w.entities[0].item_stats.crystal = 25.;
        w.entities[2].position = w.entities[0].position;
        w.command(1, Command::Learn(AbilitySlot::C)).unwrap();
        w.command(
            1,
            Command::Cast {
                slot: AbilitySlot::C,
                target: CastTarget::Entity(3),
            },
        )
        .unwrap();
        w.tick = 1;
        w.advance_cast(0, &mut Vec::new());
        assert_eq!(w.entities[2].status.barrier, 555.);
        assert_eq!(w.entities[2].status.barrier_until, 81);
        assert!(w.water.is_empty());
    }
    #[test]
    #[ignore = "requires original delivery and supplemental combat data"]
    fn hellfire_impact_pierces_shield_and_four_burns_follow_the_target() {
        let data = original();
        let mut w = room(&data, "Ringo");
        w.entities[0].level = 6;
        w.command(1, Command::Learn(AbilitySlot::C)).unwrap();
        w.entities[1].stats.shield = 100.;
        w.entities[1].max_hp = 3000.;
        w.entities[1].hp = 3000.;
        w.entities[3].position = w.entities[1].position;
        w.entities[3].stats.shield = 100.;
        w.entities[3].max_hp = 3000.;
        w.entities[3].hp = 3000.;
        w.command(
            1,
            Command::Cast {
                slot: AbilitySlot::C,
                target: CastTarget::Entity(2),
            },
        )
        .unwrap();
        w.tick = 22;
        w.advance_cast(0, &mut Vec::new());
        let mut events = Vec::new();
        for _ in 0..100 {
            w.tick += 1;
            w.advance_effects(&mut events);
        }
        for target in [2, 4] {
            let hits: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    Event::Damage {
                        source: 1,
                        target: id,
                        amount,
                        ..
                    } if *id == target => Some(*amount),
                    _ => None,
                })
                .collect();
            assert_eq!(hits, [250., 27.5, 27.5, 27.5, 27.5]);
        }
    }

    #[test]
    #[ignore = "requires original delivery and supplemental combat data"]
    fn recorded_aral_lane_rewards_use_mode_multiplier_before_splitting() {
        let data = original();
        let mut w = room(&data, "Ringo");
        for (template, gold, xp) in [
            (crate::EntityTemplate::RangedMinion, 70, 12.),
            (crate::EntityTemplate::LeadMinion, 160, 24.),
            (crate::EntityTemplate::TankMinion, 70, 12.),
        ] {
            let unit = data
                .units
                .iter()
                .find(|u| data.type_id(&u.name) == Some(template.as_raw()))
                .unwrap();
            let stats = crate::Stats::load(template.as_raw(), unit, &data);
            let id = w.spawn_with_stats(1, crate::Kind::Minion, stats, Position::default(), false);
            let idx = w.index(id).unwrap();
            let mut events = Vec::new();
            w.hurt(1, 0, idx, 10000., crate::DamageSchool::True, &mut events);
            assert!(events.contains(&Event::GoldAward {
                id: 1,
                amount: gold
            }));
            assert!(events.contains(&Event::XpGain { id: 1, amount: xp }));
        }
    }

    #[test]
    #[ignore = "requires original delivery and supplemental combat data"]
    fn named_ringo_talent_changes_an_actual_targeted_release() {
        let data = original();
        let mut w = room(&data, "Ringo");
        let t = CastTarget::Entity(2);
        let cast = Command::Cast {
            slot: AbilitySlot::A,
            target: t,
        };
        w.command(1, Command::Learn(AbilitySlot::A)).unwrap();
        assert_eq!(w.command(1, cast), Err("cast out of range"));
        let lvl = NonZeroU8::new(4).unwrap();
        w.set_named_talent(1, Some(("Talent_Ringo_LongShot", lvl)), &data)
            .unwrap();
        w.entities[0].item_stats.crystal = 20.;
        w.entities[1].stats.shield = 0.;
        let hp = w.entities[1].hp;
        w.command(1, cast).unwrap();
        w.tick = 4;
        w.advance_cast(0, &mut Vec::new());
        assert!((hp - w.entities[1].hp - (80. + 1.25 * 20.) * 1.09).abs() < 0.001);
        assert_eq!(
            w.entities[0].cooldown[0],
            4 + (180_f32 * 1.29).ceil() as u64
        );
        w.set_named_talent(1, None, &data).unwrap();
        w.tick = 500;
        assert_eq!(w.command(1, cast), Err("cast out of range"));
        w.entities[1].position.x = 17.5;
        assert!(w.visible(0, &w.entities[1]));
        w.entities[1].position.x = 18.5;
        assert!(!w.visible(0, &w.entities[1]));
        w.entities[0].hp = 100.;
        w.entities[0].energy = 100.;
        w.step();
        assert!((w.entities[0].hp - 100. - (3.15 + 3. * 0.23) / 20.).abs() < 0.001);
        assert!((w.entities[0].energy - 100. - (2.2 + 3. * 0.15) / 20.).abs() < 0.001);
    }

    #[test]
    #[ignore = "requires original delivery and supplemental combat data"]
    fn supplemented_chakram_refunds_each_hero_impact_and_debits_stamina_at_release() {
        let data = original();
        let mut w = room(&data, "Idris");
        w.entities[1].position.x = 3.;
        w.entities[1].stats.shield = 0.;
        w.entities[0].item_stats.crystal = 25.;
        w.command(1, Command::Learn(AbilitySlot::B)).unwrap();
        w.command(
            1,
            Command::Cast {
                slot: AbilitySlot::B,
                target: CastTarget::Point(Position { x: 7., y: 0. }),
            },
        )
        .unwrap();
        w.tick = 4;
        w.advance_cast(0, &mut Vec::new());
        assert_eq!(crate::chakram::stamina(&w.entities[0], w.tick), 70.);
        assert_eq!(w.entities[0].cooldown[1], 174);
        let mut events = Vec::new();
        for _ in 0..20 {
            w.tick += 1;
            w.advance_chakrams(&mut events);
        }
        let hits: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Event::Damage {
                    source: 1,
                    target: 2,
                    amount,
                    ..
                } => Some(*amount),
                _ => None,
            })
            .collect();
        assert_eq!(hits, [85., 85.]);
        assert!((crate::chakram::stamina(&w.entities[0], w.tick) - 94.).abs() < 0.01);
    }
}
