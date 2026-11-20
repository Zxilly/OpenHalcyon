//! Rank parameters for modeled releases whose damage block has a single verified role.
use halcyon_data::{
    AbilityParameter, DamageOperand, GameData, ModeledTalent, TalentParameter,
    combat_content::Ability,
};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(bound(serialize = "[f32; N]: Serialize"))]
pub(super) struct Damage<const N: usize> {
    ranks: [f32; N],
    crystal: f32,
    weapon: f32,
}
impl<const N: usize> Damage<N> {
    pub(super) fn named(a: &Ability, operand: DamageOperand) -> Result<Self, String> {
        let label = operand.as_str();
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
pub(super) fn ranks<const N: usize>(
    a: &Ability,
    parameter: AbilityParameter,
) -> Result<[f32; N], String> {
    let key = parameter.as_str();
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
            damage: Damage::named(a, DamageOperand::Damage)?,
            duration: ranks(a, AbilityParameter::SlowDuration)?,
            slow: ranks(a, AbilityParameter::SlowStrength)?,
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
            damage: Damage::named(a, DamageOperand::DamageOnPassthrough)?,
            stamina_from_hero: ranks(a, AbilityParameter::StaminaFromHeroImpacts)?,
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
            e.mobile_bulwark_bonus = None;
            return Ok(());
        };
        let talent = data.talent_definition(id).ok_or("unknown talent")?;
        let modeled = ModeledTalent::parse(id).ok_or("unsupported talent for hero")?;
        let template = modeled.hero().as_raw();
        if template != e.type_id
            || talent.hero.as_deref().and_then(|name| data.type_id(name)) != Some(template)
        {
            return Err("unsupported talent for hero");
        }
        let value = |label: TalentParameter| {
            talent
                .modifier(label.as_str())
                .and_then(|m| m.value_at(level, e.item_stats.crystal, e.stats.weapon))
                .ok_or("unsupported talent modifier")
        };
        match modeled {
            ModeledTalent::LongShot => {
                e.long_shot = Some(LongShot {
                    range: value(TalentParameter::BonusRange)?,
                    damage: value(TalentParameter::DamageModifier)?,
                    cooldown: value(TalentParameter::CooldownModifier)?,
                });
                e.mobile_bulwark_bonus = None;
            }
            ModeledTalent::MobileBulwark => {
                let bonus = value(TalentParameter::BonusDuration)?;
                if bonus < 0. {
                    return Err("unsupported talent duration");
                }
                e.mobile_bulwark_bonus = Some(bonus);
                e.long_shot = None;
            }
        }
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
            damage: Damage::named(a, DamageOperand::BonusCrystalDamage)?,
            duration: ranks(a, AbilityParameter::SpeedBuffDuration)?,
            attack_speed: ranks(a, AbilityParameter::BonusAttackSpeedStrength)?,
            move_speed: ranks(a, AbilityParameter::BonusMoveSpeedStrength)?,
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
            impact: Damage::named(a, DamageOperand::LorelaiImpactDamage)?,
            per_second: Damage::named(a, DamageOperand::LorelaiDamagePerSecond)?,
            stun: ranks(a, AbilityParameter::AreaStunDuration)?,
            hp_to_stun: ranks(a, AbilityParameter::StunDurationRatio)?,
            stun_cap: ranks(a, AbilityParameter::StunDurationCap)?,
            empowered_stun: ranks(a, AbilityParameter::EmpoweredStunMultiplier)?,
            snap_delay: ranks(a, AbilityParameter::RegularDelayTime)?,
            splash_delay: ranks(b, AbilityParameter::ShotSpeed)?,
            empowered_delay: ranks(b, AbilityParameter::EmpoweredShotSpeed)?,
            slow: ranks(b, AbilityParameter::Slow)?,
            hp_to_slow: ranks(b, AbilityParameter::SlowRatio)?,
            slow_cap: ranks(b, AbilityParameter::SlowCap)?,
            speed: ranks(b, AbilityParameter::SpeedBoost)?,
            barrier: ranks(c, AbilityParameter::BarrierAmount)?,
            empowered_barrier: ranks(c, AbilityParameter::EmpoweredBarrierAmount)?,
            hp_to_barrier: ranks(c, AbilityParameter::BarrierRatio)?,
            crystal_to_barrier: c
                .variable(AbilityParameter::BarrierAmount.as_str())
                .unwrap()
                .cp_ratio,
            crystal_to_empowered_barrier: c
                .variable(AbilityParameter::EmpoweredBarrierAmount.as_str())
                .unwrap()
                .cp_ratio,
            duration: ranks(c, AbilityParameter::Duration)?,
            empowered_duration: ranks(c, AbilityParameter::EmpoweredDuration)?,
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
        let frequency: [f32; 3] = ranks(c, AbilityParameter::TicksPerSecond)?;
        if frequency != [1.; 3] {
            return Err("unsupported Hellfire tick frequency".into());
        }
        Ok(Some(Self {
            impact: Damage::named(c, DamageOperand::ImpactDamage)?,
            burn: Damage::named(c, DamageOperand::BurnDamagePerSecond)?,
            radius: ranks(c, AbilityParameter::ImpactSplashRadius)?,
            burn_radius: ranks(c, AbilityParameter::BurnRadius)?,
            duration: ranks(c, AbilityParameter::BurnDuration)?,
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
    fn variable(a: &Ability, key: AbilityParameter) -> Result<Self, String> {
        let ranks = ranks(a, key)?;
        let v = a.variable(key.as_str()).expect("rank table already found");
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
        let saw_ticks = ranks(saw, AbilityParameter::TicksDuringChannel)?;
        if saw_ticks
            .iter()
            .any(|&x| !(1. ..=255.).contains(&x) || x.fract() != 0.)
        {
            return Err("invalid SAW channel pulse count".into());
        }
        let shin = get("Shin", "Ability__Shin__C")?;
        let wind = get("Karas", "Ability__Karas__B")?;
        Ok(Some(Self {
            gift: Damage::variable(gift, AbilityParameter::BurstHealStrength)?,
            agent: Damage::variable(agent, AbilityParameter::BaseBonusDamage)?,
            self_agent: Damage::variable(agent, AbilityParameter::SelfBonusDamage)?,
            agent_duration: ranks(agent, AbilityParameter::StackDuration)?,
            sigil_dps: Damage::variable(sigil, AbilityParameter::SigilDamagePerSecond)?,
            sigil_radius: ranks(sigil, AbilityParameter::Radius)?,
            saw_dps: Damage::variable(saw, AbilityParameter::SuppressingFireDamagePerSecond)?,
            saw_duration: ranks(saw, AbilityParameter::ChannelDuration)?,
            saw_ticks,
            saw_slow: ranks(saw, AbilityParameter::SlowStrength)?,
            saw_slow_duration: ranks(saw, AbilityParameter::SlowDuration)?,
            saw_minion_reduction: ranks(saw, AbilityParameter::MinionDamageReduction)?,
            shin: Damage::named(shin, DamageOperand::Damage)?,
            karas: Damage::named(wind, DamageOperand::UppercaseDamage)?,
        }))
    }
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct WinterSpire {
    pub first: Damage<5>,
    pub shatter: Damage<5>,
    pub radius: [f32; 5],
    pub first_delay: [f32; 5],
    pub shatter_delay: [f32; 5],
}
impl WinterSpire {
    pub fn load(data: &GameData) -> Result<Option<Self>, String> {
        let Some(a) = data.ability("Reim", "Ability__Reim__A") else {
            return Ok(None);
        };
        Ok(Some(Self {
            first: Damage::named(a, DamageOperand::DamageFirstHit)?,
            shatter: Damage::named(a, DamageOperand::DamageSecondHit)?,
            radius: ranks(a, AbilityParameter::Radius)?,
            first_delay: ranks(a, AbilityParameter::FirstHitDelay)?,
            shatter_delay: ranks(a, AbilityParameter::SecondHitDelay)?,
        }))
    }
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct ChillWinds {
    pub damage: Damage<5>,
    pub radius: [f32; 5],
    pub root_seconds: [f32; 5],
}
impl ChillWinds {
    pub fn load(data: &GameData) -> Result<Option<Self>, String> {
        let Some(b) = data.ability("Reim", "Ability__Reim__B") else {
            return Ok(None);
        };
        Ok(Some(Self {
            damage: Damage::named(b, DamageOperand::Damage)?,
            radius: ranks(b, AbilityParameter::Range)?,
            root_seconds: ranks(b, AbilityParameter::RootDuration)?,
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
    fn named_talents_follow_the_selected_hero_and_clear_without_leaking_modifiers() {
        let d = original();
        let mut w = room(&d, "Ringo");
        let level = NonZeroU8::new(1).unwrap();
        let shot = Some((ModeledTalent::LongShot.as_str(), level));
        let mobile = Some((ModeledTalent::MobileBulwark.as_str(), level));
        w.set_named_talent(1, shot, &d).unwrap();
        assert!(w.set_named_talent(1, mobile, &d).is_err());
        assert!(
            w.set_named_talent(1, Some(("extension/unknown", level)), &d)
                .is_err()
        );
        assert!(w.entities[0].long_shot.is_some());
        assert!(w.entities[0].mobile_bulwark_bonus.is_none());
        w.select_hero(0, "Lyra", &d).unwrap();
        assert!(w.entities[0].long_shot.is_none());
        w.set_named_talent(1, mobile, &d).unwrap();
        assert!(w.set_named_talent(1, shot, &d).is_err());
        assert_eq!(w.entities[0].mobile_bulwark_bonus, Some(1.4));
        w.set_named_talent(1, None, &d).unwrap();
        assert!(w.entities[0].mobile_bulwark_bonus.is_none());
        w.set_named_talent(1, mobile, &d).unwrap();
        w.select_hero(0, "Ringo", &d).unwrap();
        assert!(w.entities[0].mobile_bulwark_bonus.is_none());
        assert!(w.entities[0].long_shot.is_none());
        w.set_named_talent(1, shot, &d).unwrap();
        w.set_named_talent(1, None, &d).unwrap();
        assert!(w.entities[0].long_shot.is_none());
    }

    #[test]
    #[ignore = "requires original delivery and supplemental combat data"]
    fn catherine_pursuit_resets_the_basic_and_empowers_exactly_one_melee_hit() {
        let data = original();
        let mut w = room(&data, "Catherine");
        w.entities[0].skill_points = 1;
        w.entities[0].item_stats.crystal = 40.;
        w.command(1, Command::Learn(AbilitySlot::A)).unwrap();
        w.entities[1].position = Position {
            x: w.entities[0].stats.range + 0.8,
            y: 0.,
        };
        w.entities[1].stats.shield = 0.;
        w.entities[0].energy = 29.;
        let cast = Command::Cast {
            slot: AbilitySlot::A,
            target: CastTarget::SelfCast,
        };
        assert!(w.command(1, cast).is_err());
        w.entities[0].energy = 100.;
        assert!(
            w.command(
                1,
                Command::Cast {
                    slot: AbilitySlot::A,
                    target: CastTarget::Entity(2)
                }
            )
            .is_err()
        );
        assert_eq!(w.decide(0), cast);
        w.entities[0].next_attack = 900;
        w.command(1, cast).unwrap();
        w.tick = 1;
        w.advance_cast(0, &mut Vec::new());
        assert_eq!(w.entities[0].energy, 70.);
        assert_eq!(w.entities[0].cooldown[0], 321);
        assert_eq!(w.entities[0].next_attack, 1);
        let origin = w.entities[0].position;
        w.command(1, Command::Attack(2)).unwrap();
        let first: Vec<_> = (0..4).flat_map(|_| w.step()).collect();
        assert!(first.iter().any(
            |event| matches!(event,Event::Damage{source:1,target:2,amount,..} if *amount==75.)
        ));
        assert_eq!(w.entities[1].status.stun_until, 19);
        assert_eq!(w.entities[0].status.pursuit.unwrap().armed_until, 0);
        assert_eq!(w.entities[0].position, origin);
        w.entities[1].position = origin;
        let next: Vec<_> = (0..30).flat_map(|_| w.step()).collect();
        assert!(next.iter().any(|e| matches!(
            e,
            Event::Attack {
                source: 1,
                target: 2
            }
        )));
        assert!(!next.iter().any(|e| matches!(
            e,
            Event::Damage {
                source: 1,
                amount,
                ..
            } if *amount==75.
        )));
    }

    #[test]
    #[ignore = "requires original delivery and supplemental combat data"]
    fn yates_overwhelm_checks_energy_then_hits_at_the_moving_caster_after_charge() {
        let data = original();
        let mut w = room(&data, "Yates");
        w.entities[0].position = Position::default();
        w.entities[1].position = Position { x: 3.5, y: 0. };
        w.entities[0].energy = 59.;
        w.entities[0].skill_points = 1;
        w.command(1, Command::Learn(AbilitySlot::B)).unwrap();
        let cast = Command::Cast {
            slot: AbilitySlot::B,
            target: CastTarget::SelfCast,
        };
        assert!(w.command(1, cast).is_err());
        w.entities[0].energy = 100.;
        w.entities[0].item_stats.crystal = 40.;
        assert!(
            w.command(
                1,
                Command::Cast {
                    slot: AbilitySlot::B,
                    target: CastTarget::Entity(2)
                }
            )
            .is_err()
        );
        assert_eq!(w.decide(0), cast);
        w.command(1, cast).unwrap();
        let mut events = Vec::new();
        w.tick = 1;
        w.advance_cast(0, &mut events);
        assert_eq!(w.entities[0].energy, 40.);
        assert_eq!(w.entities[0].cooldown[1], 321);
        w.entities[0].energy = 100.;
        assert!(w.command(1, cast).is_err());
        let old_hp = w.entities[1].hp;
        w.entities[1].stats.shield = 0.;
        w.entities[0].position = Position { x: 10., y: 0. };
        w.entities[1].position = Position { x: 13.5, y: 0. };
        w.entities[2].position = w.entities[0].position;
        w.entities[3].position = Position { x: 13.51, y: 0. };
        w.tick = 30;
        w.advance_overwhelm(0, &mut events);
        assert_eq!(w.entities[1].hp, old_hp);
        w.tick = 31;
        w.advance_overwhelm(0, &mut events);
        assert_eq!(w.entities[1].hp, old_hp - 110.);
        assert_eq!(w.entities[1].status.stun_until, 47);
        assert_eq!(w.entities[2].status.stun_until, 0);
        assert_eq!(w.entities[3].status.stun_until, 0);
        w.advance_overwhelm(0, &mut events);
        assert_eq!(w.entities[1].hp, old_hp - 110.);
    }

    #[test]
    #[ignore = "requires original delivery and supplemental combat data"]
    fn chill_winds_roots_movement_only_and_matches_observed_lane_damage() {
        let data = original();
        let mut w = room(&data, "Reim");
        w.select_hero(1, "Ringo", &data).unwrap();
        let victim_pos = Position { x: 3., y: 0. };
        w.entities[1].position = victim_pos;
        w.entities[1].stats.shield = 0.;
        w.entities[0].item_stats.crystal = 41.25;
        w.entities[0].level = 4;
        w.entities[0].skill_points = 3;
        w.entities[0].position = Position::default();
        w.walk(0, Position { x: 1., y: 0. }, &mut Vec::new());
        let origin = w.entities[0].position;
        for _ in 0..2 {
            w.command(1, Command::Learn(AbilitySlot::B)).unwrap();
        }
        let minion = w.spawn(1, crate::Kind::Minion, 4, victim_pos, false);
        let mi = w.index(minion).unwrap();
        w.entities[mi].stats.shield = 0.;
        let cast = Command::Cast {
            slot: AbilitySlot::B,
            target: CastTarget::SelfCast,
        };
        assert_eq!(w.decide(0), cast);
        let energy = w.entities[0].energy;
        w.command(1, cast).unwrap();
        w.tick = 1;
        let mut events = Vec::new();
        w.advance_cast(0, &mut events);
        assert!(events.contains(&Event::ChillWinds {
            source: 1,
            position: origin,
            heading: Some(0.)
        }));
        assert_eq!(w.entities[0].energy, energy - 65.);
        assert_eq!(w.entities[0].cooldown[1], 231);
        for (id, expected) in [(2, 124.75), (minion, 62.375)] {
            assert!(events.iter().any(|e|matches!(e,Event::Damage{target,amount,effect:crate::ImpactEffect::ChillWinds,..} if *target==id && *amount==expected)));
            assert!(
                events
                    .iter()
                    .any(|e| matches!(e,Event::ChillWindsRoot{target,..} if *target==id))
            );
        }
        w.command(2, Command::Learn(AbilitySlot::A)).unwrap();
        w.command(
            2,
            Command::Cast {
                slot: AbilitySlot::A,
                target: CastTarget::Entity(1),
            },
        )
        .unwrap();
        w.tick = 20;
        w.walk(1, Position { x: 5., y: 0. }, &mut events);
        assert_eq!(w.entities[1].position, victim_pos);
        w.tick = 21;
        w.walk(1, Position { x: 5., y: 0. }, &mut events);
        assert_ne!(w.entities[1].position, victim_pos);
    }

    #[test]
    #[ignore = "requires the original delivery and supplemental combat data"]
    fn winter_spire_bot_and_player_use_the_same_two_ranked_area_hits() {
        let data = original();
        let mut w = room(&data, "Reim");
        w.command(1, Command::Learn(AbilitySlot::A)).unwrap();
        let p = Position { x: 4., y: 0. };
        w.entities[1].position = p;
        w.entities[1].stats.shield = 0.;
        let minion = w.spawn(1, crate::Kind::Minion, 4, p, false);
        let mi = w.index(minion).unwrap();
        w.entities[mi].stats.shield = 0.;
        w.entities[0].item_stats.crystal = 25.;
        let bot_order = w.decide(0);
        assert_eq!(
            bot_order,
            Command::Cast {
                slot: AbilitySlot::A,
                target: CastTarget::Point(p)
            }
        );
        w.command(1, bot_order).unwrap();
        w.tick = 2;
        let mut events = Vec::new();
        w.advance_cast(0, &mut events);
        assert!(events.contains(&Event::Cast {
            source: 1,
            slot: AbilitySlot::A,
            target: CastTarget::Point(p)
        }));
        for (tick, hero_dmg, minion_dmg) in
            [(9, 0., 0.), (10, 80., 40.), (29, 0., 0.), (30, 120., 60.)]
        {
            w.tick = tick;
            events.clear();
            w.advance_effects(&mut events);
            let hits = |id| {
                events
                    .iter()
                    .filter_map(|e| {
                        if let Event::Damage { target, amount, .. } = e
                            && *target == id
                        {
                            Some(*amount)
                        } else {
                            None
                        }
                    })
                    .sum::<f32>()
            };
            assert_eq!((hits(2), hits(minion)), (hero_dmg, minion_dmg));
        }
        assert_eq!(w.entities[0].cooldown[0], 82);
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
        w.entities[0].position = w.layout.home(&w.entities[0]);
        let grant = w
            .purchase(1, crate::BasicItem::WeaponInfusion)
            .unwrap()
            .granted;
        w.command(
            1,
            Command::ConsumeInfusion {
                instance: grant.instance,
            },
        )
        .unwrap();
        w.entities[0].position = Position::default();
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
        let level_four_infusion = 20. + 20. * 3. / 11.;
        let expected = 85. + 0.8 * level_four_infusion;
        assert_eq!(hits.len(), 2);
        assert!(hits.iter().all(|hit| (hit - expected).abs() < 0.001));
        assert!((crate::chakram::stamina(&w.entities[0], w.tick) - 94.).abs() < 0.01);
    }
}
