//! A small authored ability set. Official 4.13 deltas supply the named rank
//! tables. Optional client tables supply Ringo cast costs, cooldowns and target ranges.
//! Windups and skill effects remain independently modeled.
use super::*;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AbilitySlot {
    A,
    B,
    C,
}
impl AbilitySlot {
    pub(super) fn idx(self) -> usize {
        self as usize
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum CastTarget {
    SelfCast,
    Entity(u32),
    Point(Position),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Skill {
    Achilles,
    Twirl,
    Hellfire,
    Gift,
    Agent,
    Sigil,
    Suppress,
    Wind,
    Strike,
    Chakram,
    Bulwark,
    Stormguard,
    BlastTremor,
    WolfMaw,
    IronMandate,
    Pursuit,
    Overwhelm,
    ChillWinds,
    WinterSpire,
    FishFood,
    Splashdown,
    Waterwall,
    Verse,
    Skedaddle,
}
fn skill(hero: u16, slot: AbilitySlot) -> Option<Skill> {
    use AbilitySlot::*;
    use Skill::*;
    Some(match (EntityTemplate::from_raw(hero)?, slot) {
        (EntityTemplate::Idris, B) => Chakram,
        (EntityTemplate::Gwen, B) => Skedaddle,
        (EntityTemplate::Catherine, B) => Stormguard,
        (EntityTemplate::Catherine, C) => BlastTremor,
        (EntityTemplate::Catherine, A) => Pursuit,
        (EntityTemplate::Yates, A) => WolfMaw,
        (EntityTemplate::Yates, C) => IronMandate,
        (EntityTemplate::Yates, B) => Overwhelm,
        (EntityTemplate::Reim, A) => WinterSpire,
        (EntityTemplate::Reim, B) => ChillWinds,
        (EntityTemplate::Lorelai, A) => FishFood,
        (EntityTemplate::Lorelai, B) => Splashdown,
        (EntityTemplate::Lorelai, C) => Waterwall,
        (EntityTemplate::Ringo, A) => Achilles,
        (EntityTemplate::Ringo, B) => Twirl,
        (EntityTemplate::Ringo, C) => Hellfire,
        (EntityTemplate::Adagio, C) => Verse,
        (EntityTemplate::Adagio, A) => Gift,
        (EntityTemplate::Adagio, B) => Agent,
        (EntityTemplate::Lyra, B) => Bulwark,
        (EntityTemplate::Lyra, A) => Sigil,
        (EntityTemplate::Saw, B) => Suppress,
        (EntityTemplate::Karas, B) => Wind,
        (EntityTemplate::Shin, C) => Strike,
        _ => return None,
    })
}
#[derive(Debug, Clone, Copy, Serialize)]
pub(super) struct Cast {
    slot: AbilitySlot,
    target: CastTarget,
    release: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) verse: Option<verse::Channel>,
}
/// These controllers are only armed by Catherine and Yates. Lane units keep a single empty
/// slot. The serialized fields stay in their original order for replay checkpoints.
#[derive(Clone, Debug, Default)]
pub(super) struct CaptainEffects(Option<Box<CaptainState>>);

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub(super) struct CaptainState {
    pub(super) stormguard: Option<super::catherine::Bubble>,
    pub(super) flail: Option<super::yates::Shot>,
    pub(super) overwhelm: Option<super::overwhelm::Charge>,
    pub(super) pursuit: Option<super::pursuit::PursuitAttack>,
}

impl CaptainEffects {
    pub(super) fn read(&self) -> &CaptainState {
        static EMPTY: CaptainState = CaptainState {
            stormguard: None,
            flail: None,
            overwhelm: None,
            pursuit: None,
        };
        self.0.as_deref().unwrap_or(&EMPTY)
    }

    pub(super) fn active_mut(&mut self) -> Option<&mut CaptainState> {
        self.0.as_deref_mut()
    }

    pub(super) fn arm(&mut self) -> &mut CaptainState {
        self.0.get_or_insert_with(Default::default)
    }
}

impl Serialize for CaptainEffects {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.read().serialize(serializer)
    }
}

#[derive(Clone, Default, Debug, Serialize)]
pub(super) struct Status {
    pub(super) sigil_speed: f32,
    pub(super) sigil_speed_until: u64,
    pub(super) sigil_speed_duration: u64,
    pub(super) bulwark_slow_until: u64,
    pub(super) bulwark_slow_factor: f32,
    pub(super) silence_until: u64,
    pub(super) reveal_until: u64,
    #[serde(flatten)]
    pub(super) captain_effects: CaptainEffects,
    pub(super) double_down: Option<u32>,
    pub(super) stamina_spent: f32,
    pub(super) stamina_spent_at: u64,
    pub(super) stun_until: u64,
    pub(super) root_until: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) skedaddle: Option<super::skedaddle::Active>,
    #[serde(rename = "reim_heading")]
    pub(super) motion_heading: Option<f32>,
    pub(super) water_speed_until: u64,
    pub(super) water_speed_bonus: Option<f32>,
    pub(super) water_slow_until: u64,
    pub(super) water_slow_factor: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) reflex: Option<crate::reflex::Active>,
    pub(super) flask_until: u64,
    pub(super) flask_barrier: f32,
    pub(super) flask_heal_per_tick: f32,
    pub(super) flask_energy_per_tick: f32,
    pub(super) barrier: f32,
    pub(super) barrier_until: u64,
    pub slow_until: u64,
    pub slow_factor: f32,
    pub haste_until: u64,
    pub(super) twirl_rank: Option<usize>,
    pub empowered: u8,
    pub empower_damage: f32,
    pub(super) agent_burn_bonus: f32,
    pub(super) agent_source: u32,
    pub(super) agent_crystal_until: Option<u64>,
    pub(super) channel: u64,
    pub(super) channel_until: u64,
}
#[derive(Clone, Debug, Serialize)]
#[serde(bound(serialize = "[f32; N]: Serialize"))]
struct CastRanks<const N: usize> {
    range: [f32; N],
    energy: [f32; N],
    seconds: [f32; N],
}

impl<const N: usize> CastRanks<N> {
    fn at(&self, rank: usize, windup: u64) -> (f32, f32, u64, f32) {
        (
            self.range[rank],
            self.energy[rank],
            windup,
            self.seconds[rank],
        )
    }

    fn load(&mut self, data: &GameData, ability: &str, targeted: bool) -> Result<(), String> {
        let read = |parameter: halcyon_data::AbilityParameter, defaults: [f32; N]| {
            let name = parameter.as_str();
            let Some(values) = data.ability_ranks("Ringo", ability, name) else {
                return Ok(defaults);
            };
            if values.len() != N || values.iter().any(|v| !v.is_finite() || *v < 0.) {
                return Err(format!("invalid {ability} {name} rank table"));
            }
            let mut ranks = defaults;
            ranks.copy_from_slice(values);
            Ok(ranks)
        };
        use halcyon_data::AbilityParameter;
        self.energy = read(AbilityParameter::EnergyCost, self.energy)?;
        self.seconds = read(AbilityParameter::Cooldown, self.seconds)?;
        if targeted {
            self.range = read(AbilityParameter::Range, self.range)?;
        }
        Ok(())
    }
}

#[derive(Clone, Default, Debug, Serialize)]
struct ExtraCast {
    range: Option<Vec<f32>>,
    cost: Option<Vec<f32>>,
    cooldown: Option<Vec<f32>>,
}
impl ExtraCast {
    fn read(
        data: &GameData,
        hero: &str,
        id: &str,
        rank_count: usize,
        alt: bool,
    ) -> Result<Self, String> {
        let values = |name: &str| -> Result<Option<Vec<f32>>, String> {
            let Some(v) = data.ability_ranks(hero, id, name) else {
                return Ok(None);
            };
            if v.len() != rank_count || v.iter().any(|x| !x.is_finite() || *x < 0.) {
                return Err(format!("invalid {id}.{name} ranks"));
            }
            Ok(Some(v.to_vec()))
        };
        Ok(Self {
            range: values(halcyon_data::AbilityParameter::Range.as_str())?,
            cost: values(if alt {
                "Alt Energy Cost"
            } else {
                halcyon_data::AbilityParameter::EnergyCost.as_str()
            })?,
            cooldown: values(halcyon_data::AbilityParameter::Cooldown.as_str())?,
        })
    }
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct AbilityBook {
    extra_casts: [ExtraCast; Skill::Waterwall as usize + 1],
    pub(super) sigil: Option<super::sigil::Rules>,
    pub(super) twister: Option<super::twister::Rules>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) verse: Option<super::verse::Rules>,
    pub(super) fire: Option<super::arcane_fire::Rules>,
    pub(super) bulwark: Option<super::bulwark::Rules>,
    pub(super) stormguard: Option<super::catherine::Stormguard>,
    pub(super) tremor: Option<super::catherine::Tremor>,
    pub(super) flail: Option<super::yates::Flail>,
    pub(super) leap: Option<super::yates::Leap>,
    pub(super) pursuit: Option<super::pursuit::Pursuit>,
    pub(super) overwhelm: Option<super::overwhelm::Overwhelm>,
    chill_winds: Option<super::native_combat::ChillWinds>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) skedaddle: Option<super::skedaddle::Rules>,
    winter_spire: Option<super::native_combat::WinterSpire>,
    other: Option<super::native_combat::OtherSpells>,
    pub(super) lorelai: Option<super::native_combat::LorelaiRanks>,
    pub(super) ringo_b: Option<super::native_combat::TwirlingSilver>,
    ringo_c: Option<super::native_combat::Hellfire>,
    ringo_a: Option<super::native_combat::Achilles>,
    pub(super) idris_b: Option<super::native_combat::ChakramRanks>,
    achilles: CastRanks<5>,
    twirl: CastRanks<5>,
    hellfire: CastRanks<3>,
    gift_cd: [f32; 5],
    gift_hp: f32,
    empower: [f32; 5],
    sigil_heal: [f32; 5],
    sigil_cd: [f32; 5],
    sigil_hp: f32,
    suppress_dps: [f32; 5],
    suppress_cd: [f32; 5],
    wind_dmg: [f32; 5],
    strike_dmg: f32,
    strike_hits: u8,
    chakram_weapon_ratio: f32,
    pub(super) karas_basic_crystal_ratio: f32,
}
impl Default for AbilityBook {
    fn default() -> Self {
        Self {
            extra_casts: Default::default(),
            sigil: None,
            twister: None,
            verse: None,
            fire: None,
            bulwark: None,
            stormguard: None,
            tremor: None,
            flail: None,
            leap: None,
            pursuit: None,
            overwhelm: None,
            chill_winds: None,
            skedaddle: None,
            winter_spire: None,
            other: None,
            lorelai: None,
            ringo_b: None,
            ringo_c: None,
            ringo_a: None,
            idris_b: None,
            achilles: CastRanks {
                range: [7.; 5],
                energy: [20.; 5],
                seconds: [6.; 5],
            },
            twirl: CastRanks {
                range: [0.; 5],
                energy: [18.; 5],
                seconds: [9.; 5],
            },
            hellfire: CastRanks {
                range: [12.; 3],
                energy: [45.; 3],
                seconds: [28.; 3],
            },
            gift_cd: [12., 11., 10., 9., 8.],
            gift_hp: 0.09,
            empower: [5., 5., 5., 5., 7.],
            sigil_heal: [25., 35., 45., 55., 75.],
            sigil_cd: [14., 13., 12., 11., 10.],
            sigil_hp: 0.15,
            suppress_dps: [140., 175., 210., 245., 315.],
            suppress_cd: [15., 15., 15., 15., 13.],
            wind_dmg: [100., 120., 140., 160., 200.],
            strike_dmg: 200.,
            strike_hits: 3,
            chakram_weapon_ratio: 0.8,
            karas_basic_crystal_ratio: 0.6,
        }
    }
}
impl AbilityBook {
    pub(super) fn from_data(data: &GameData) -> Result<Self, String> {
        let changes = data.official_rules["patch_413_ability_deltas"]["changes"]
            .as_array()
            .ok_or("missing patch 4.13 changes")?;
        let field = |hero: &str, key: &str| {
            changes
                .iter()
                .find(|v| v["hero"] == hero && v.get(key).is_some())
                .and_then(|v| v.get(key))
                .ok_or_else(|| format!("missing 4.13 field {hero}.{key}"))
        };
        let five = |hero: &str, key: &str| -> Result<[f32; 5], String> {
            serde_json::from_value(field(hero, key)?.clone())
                .map_err(|e| format!("{hero}.{key}: {e}"))
        };
        let num = |hero: &str, key: &str| {
            field(hero, key)?
                .as_f64()
                .map(|v| v as f32)
                .ok_or_else(|| format!("invalid {hero}.{key}"))
        };
        let Self {
            mut achilles,
            mut twirl,
            mut hellfire,
            ..
        } = Self::default();
        achilles.load(data, "Ability__Ringo__A", true)?;
        twirl.load(data, "Ability__Ringo__B", false)?;
        hellfire.load(data, "Ability__Ringo__C", true)?;
        let mut extra_casts: [ExtraCast; Skill::Waterwall as usize + 1] = Default::default();
        for (skill, hero, slot, ranks) in [
            (Skill::Chakram, "Idris", "B", 5),
            (Skill::WinterSpire, "Reim", "A", 5),
            (Skill::Bulwark, "Lyra", "B", 5),
            (Skill::Stormguard, "Catherine", "B", 5),
            (Skill::BlastTremor, "Catherine", "C", 3),
            (Skill::WolfMaw, "Yates", "A", 5),
            (Skill::IronMandate, "Yates", "C", 3),
            (Skill::Pursuit, "Catherine", "A", 5),
            (Skill::Overwhelm, "Yates", "B", 5),
            (Skill::ChillWinds, "Reim", "B", 5),
            (Skill::FishFood, "Lorelai", "A", 5),
            (Skill::Splashdown, "Lorelai", "B", 5),
            (Skill::Waterwall, "Lorelai", "C", 3),
            (Skill::Gift, "Adagio", "A", 5),
            (Skill::Agent, "Adagio", "B", 5),
            (Skill::Sigil, "Lyra", "A", 5),
            (Skill::Suppress, "SAW", "B", 5),
            (Skill::Wind, "Karas", "B", 5),
            (Skill::Strike, "Shin", "C", 3),
        ] {
            extra_casts[skill as usize] = ExtraCast::read(
                data,
                hero,
                &format!("Ability__{hero}__{slot}"),
                ranks,
                skill == Skill::Chakram,
            )?;
        }
        Ok(Self {
            extra_casts,
            sigil: super::sigil::Rules::load(data)?,
            twister: super::twister::Rules::load(data)?,
            verse: super::verse::Rules::load(data)?,
            fire: super::arcane_fire::Rules::load(data)?,
            bulwark: super::bulwark::Rules::load(data)?,
            stormguard: super::catherine::Stormguard::load(data)?,
            tremor: super::catherine::Tremor::load(data)?,
            flail: super::yates::Flail::load(data)?,
            leap: super::yates::Leap::load(data)?,
            pursuit: super::pursuit::Pursuit::load(data)?,
            overwhelm: super::overwhelm::Overwhelm::load(data)?,
            chill_winds: super::native_combat::ChillWinds::load(data)?,
            skedaddle: super::skedaddle::Rules::load(data)?,
            winter_spire: super::native_combat::WinterSpire::load(data)?,
            other: super::native_combat::OtherSpells::load(data)?,
            lorelai: super::native_combat::LorelaiRanks::load(data)?,
            ringo_b: super::native_combat::TwirlingSilver::load(data)?,
            ringo_c: super::native_combat::Hellfire::load(data)?,
            ringo_a: super::native_combat::Achilles::load(data)?,
            idris_b: super::native_combat::ChakramRanks::load(data)?,
            achilles,
            twirl,
            hellfire,
            chakram_weapon_ratio: num("Idris", "B_weapon_ratio")?,
            karas_basic_crystal_ratio: num("Karas", "basic_crystal_ratio").unwrap_or(0.6),
            gift_cd: five("Adagio", "cooldown_s")?,
            gift_hp: num("Adagio", "burst_heal_health_ratio")?,
            empower: five("Adagio", "empowered_attacks")?,
            sigil_heal: five("Lyra", "A_heal_per_second")?,
            sigil_cd: five("Lyra", "A_cooldown_s")?,
            sigil_hp: num("Lyra", "A_hot_health_ratio")?,
            suppress_dps: five("SAW", "B_dps")?,
            suppress_cd: five("SAW", "B_cooldown_s")?,
            wind_dmg: five("Karas", "B_damage")?,
            strike_dmg: num("Shin", "C_damage_per_hit")?,
            strike_hits: field("Shin", "C_hits")?
                .as_u64()
                .and_then(|n| n.try_into().ok())
                .ok_or("invalid Shin.C_hits")?,
        })
    }
}
#[derive(Clone, Copy, Debug)]
struct Rules {
    range: f32,
    cost: f32,
    windup: u64,
    cooldown: u64,
}
impl Room<'_> {
    pub(super) fn yates_is_leaping(&self, i: usize) -> bool {
        self.entities[i].type_id == EntityTemplate::Yates.as_raw()
            && self.entities[i]
                .cast
                .is_some_and(|c| c.slot == AbilitySlot::C)
    }
    fn rules(&self, s: Skill, rank: usize) -> Rules {
        let (range, cost, windup, seconds) = match s {
            Skill::WinterSpire => (0., 0., 2, 0.),
            Skill::Bulwark => (
                0.,
                0.,
                self.abilities.bulwark.as_ref().unwrap().windup(rank),
                0.,
            ),
            Skill::Stormguard => (0., 0., 1, 0.),
            Skill::BlastTremor => (
                0.,
                0.,
                self.abilities.tremor.as_ref().unwrap().windup(rank),
                0.,
            ),
            Skill::WolfMaw => (0., 0., 1, 0.),
            Skill::IronMandate => (
                0.,
                0.,
                self.abilities.leap.as_ref().unwrap().windup(rank),
                0.,
            ),
            Skill::Pursuit | Skill::Overwhelm | Skill::ChillWinds => (0., 0., 1, 0.),
            Skill::Chakram => (8.5, 10., 4, 8.),
            Skill::FishFood => (
                if rank == 4 { 9.5 } else { 8. },
                60. + 10. * rank as f32,
                1,
                12. - 0.5 * rank as f32,
            ),
            Skill::Splashdown => (
                10.5,
                [50., 60., 70., 80., 40.][rank],
                1,
                8. - 0.5 * rank as f32,
            ),
            Skill::Waterwall => (10., 100. + 25. * rank as f32, 1, 90. - 15. * rank as f32),
            Skill::Achilles => self.abilities.achilles.at(rank, 4),
            Skill::Twirl => self.abilities.twirl.at(rank, 1),
            Skill::Hellfire => self.abilities.hellfire.at(rank, 22),
            Skill::Verse => self.abilities.verse.as_ref().unwrap().cast(rank),
            Skill::Skedaddle => self.abilities.skedaddle.as_ref().unwrap().cast(rank),
            Skill::Gift => (7., 26., 4, self.abilities.gift_cd[rank]),
            Skill::Agent => (7., 24., 3, 10.),
            Skill::Sigil if self.abilities.sigil.is_some() => (
                0.,
                0.,
                self.abilities.sigil.as_ref().unwrap().startup(rank),
                0.,
            ),
            Skill::Sigil => (8., 30., 5, self.abilities.sigil_cd[rank]),
            Skill::Suppress => (8., 30., 6, self.abilities.suppress_cd[rank]),
            Skill::Wind => (7., 24., 5, 8.),
            Skill::Strike => (7., 36., 7, 20.),
        };
        let extra = self.abilities.extra_casts.get(s as usize);
        let seconds = extra
            .and_then(|r| r.cooldown.as_ref())
            .map_or(seconds, |r| r[rank]);
        Rules {
            range: extra
                .and_then(|r| r.range.as_ref())
                .map_or(range, |r| r[rank]),
            cost: extra
                .and_then(|r| r.cost.as_ref())
                .map_or(cost, |r| r[rank]),
            windup,
            cooldown: (seconds * TICK_HZ as f32).ceil() as u64,
        }
    }
    fn modeled_skill(&self, hero: u16, slot: AbilitySlot) -> Option<Skill> {
        skill(hero, slot).filter(|s| match s {
            Skill::Verse => self.abilities.verse.is_some(),
            Skill::Skedaddle => self.abilities.skedaddle.is_some(),
            Skill::WinterSpire => self.abilities.winter_spire.is_some(),
            Skill::Bulwark => self.abilities.bulwark.is_some(),
            Skill::Stormguard => self.abilities.stormguard.is_some(),
            Skill::BlastTremor => self.abilities.tremor.is_some(),
            Skill::WolfMaw => self.abilities.flail.is_some(),
            Skill::IronMandate => self.abilities.leap.is_some(),
            Skill::Pursuit => self.abilities.pursuit.is_some(),
            Skill::Overwhelm => self.abilities.overwhelm.is_some(),
            Skill::ChillWinds => self.abilities.chill_winds.is_some(),
            _ => true,
        })
    }
    pub(super) fn can_learn(&self, i: usize, slot: AbilitySlot) -> bool {
        let e = &self.entities[i];
        let rank = e.ability_ranks[slot.idx()];
        let needed = if slot == AbilitySlot::C {
            6 + 3 * rank
        } else {
            1 + 2 * rank
        };
        e.skill_points > 0
            && (self.modeled_skill(e.type_id, slot).is_some()
                || (!e.bot && native_learn_operand(e.type_id, slot).is_some()))
            && rank < if slot == AbilitySlot::C { 3 } else { 5 }
            && e.level >= needed
    }
    pub(super) fn learn(&mut self, i: usize, slot: AbilitySlot) -> Result<(), &'static str> {
        if !self.can_learn(i, slot) {
            return Err("rank unavailable");
        }
        self.entities[i].skill_points -= 1;
        self.entities[i].ability_ranks[slot.idx()] += 1;
        Ok(())
    }
    pub(super) fn chill_winds_radius(&self, i: usize) -> Option<f32> {
        let rank = self.entities[i].ability_ranks[AbilitySlot::B.idx()].checked_sub(1)?;
        self.abilities
            .chill_winds
            .as_ref()?
            .radius
            .get(usize::from(rank))
            .copied()
    }

    pub(super) fn can_cast(&self, i: usize, slot: AbilitySlot, target: CastTarget) -> bool {
        self.castable(i, slot, target).is_ok()
    }
    fn castable(
        &self,
        i: usize,
        slot: AbilitySlot,
        target: CastTarget,
    ) -> Result<(Skill, usize, Rules), &'static str> {
        let e = &self.entities[i];
        if e.status.silence_until > self.tick {
            return Err("silenced");
        }
        let s = self
            .modeled_skill(e.type_id, slot)
            .ok_or("ability unavailable")?;
        let rank = e.ability_ranks[slot.idx()]
            .checked_sub(1)
            .ok_or("ability not learned")? as usize;
        if s == Skill::Sigil
            && self
                .entities
                .spells::<sigil::Field>()
                .any(|f| f.source == e.id)
        {
            if target != CastTarget::SelfCast {
                return Err("a sigil is already active");
            }
            return Ok((
                s,
                rank,
                Rules {
                    range: 0.,
                    cost: 0.,
                    windup: 1,
                    cooldown: 0,
                },
            ));
        }
        let mut r = self.rules(s, rank);
        if s == Skill::Achilles
            && let Some(t) = e.long_shot
        {
            r.range += t.range;
            r.cooldown = (r.cooldown as f32 * t.cooldown).ceil() as u64;
        }
        let resource = if s == Skill::Chakram {
            chakram::stamina(e, self.tick)
        } else {
            e.energy
        };
        if e.cooldown[slot.idx()] > self.tick || resource < r.cost {
            return Err("ability not ready");
        }
        let point = match (s, target) {
            (
                Skill::Verse
                | Skill::Skedaddle
                | Skill::Bulwark
                | Skill::Stormguard
                | Skill::Pursuit
                | Skill::Overwhelm
                | Skill::ChillWinds
                | Skill::Twirl
                | Skill::Gift
                | Skill::Agent,
                CastTarget::SelfCast,
            ) => e.position,
            (
                Skill::Achilles | Skill::Hellfire | Skill::Gift | Skill::Agent | Skill::Waterwall,
                CastTarget::Entity(id),
            ) => {
                let t = &self.entities[self.index(id).ok_or("unknown target")?];
                let ally = matches!(s, Skill::Gift | Skill::Agent | Skill::Waterwall);
                if t.hp <= 0.
                    || (matches!(s, Skill::Waterwall | Skill::Agent) && t.kind != Kind::Hero)
                    || (t.team == e.team) != ally
                    || !matches!(t.kind, Kind::Hero | Kind::Minion)
                    || !self.visible(e.team, t)
                {
                    return Err("invalid cast target");
                }
                t.position
            }
            (
                Skill::BlastTremor
                | Skill::Sigil
                | Skill::WolfMaw
                | Skill::Suppress
                | Skill::Wind
                | Skill::Strike
                | Skill::Chakram
                | Skill::WinterSpire
                | Skill::FishFood
                | Skill::Splashdown,
                CastTarget::Point(p),
            ) if self.layout.walkable(p) => p,
            (Skill::IronMandate, CastTarget::Point(p))
                if self
                    .native_navigation
                    .as_ref()
                    .map_or_else(|| self.layout.walkable(p), |map| map.can_project(p)) =>
            {
                p
            }
            _ => return Err("invalid cast target"),
        };
        if (s == Skill::IronMandate || (s == Skill::Wind && self.abilities.twister.is_some()))
            && self.bulwark_blocks_leap(i, point)
        {
            return Err("bulwark blocks leap");
        }
        if (matches!(s, Skill::BlastTremor | Skill::WolfMaw | Skill::Chakram)
            && e.position.dist(point) < 0.001)
            || point.dist(e.position) > r.range
            || (s != Skill::IronMandate && !self.layout.clear(e.position, point))
        {
            return Err("cast out of range");
        }
        Ok((s, rank, r))
    }
    pub(super) fn begin_cast(
        &mut self,
        i: usize,
        slot: AbilitySlot,
        target: CastTarget,
    ) -> Result<(), &'static str> {
        if self.entities[i].cast.is_some() {
            return Err("cast in progress");
        }
        let (skill, rank, rules) = self.castable(i, slot, target)?;
        let fortification =
            (skill == Skill::Verse).then(|| self.abilities.verse.as_ref().unwrap().fortified[rank]);
        let e = &mut self.entities[i];
        e.winding = None;
        e.order = Command::Stop;
        e.status.channel = 0;
        e.status.channel_until = 0;
        // Verse grants protection immediately. A cancelled channel has already used its cast.
        if fortification.is_some() {
            e.energy -= rules.cost;
            e.cooldown[slot.idx()] = self.state.tick
                + (rules.cooldown as f32 / (1. + e.item_stats.cooldown)).ceil() as u64;
        }
        e.cast = Some(Cast {
            verse: fortification.map(|v| verse::Channel {
                rank,
                fortified: v.min(e.hp),
            }),
            slot,
            target,
            release: self.state.tick + rules.windup,
        });
        Ok(())
    }
    /// Cancel only the still-pending windup selected by its issuing player.
    pub fn cancel_cast(&mut self, actor: u32, slot: AbilitySlot, target: CastTarget) -> bool {
        let Some(i) = self.index(actor) else {
            return false;
        };
        let e = &mut self.entities[i];
        if e.hp > 0. && e.cast.is_some_and(|c| c.slot == slot && c.target == target) {
            e.cast = None;
            return true;
        }
        false
    }

    pub(super) fn advance_cast(&mut self, i: usize, events: &mut Vec<Event>) {
        let e = &self.entities[i];
        let c = e.cast.unwrap();
        if self.tick < c.release {
            return;
        }
        self.entities[i].cast = None;
        if let Some(channel) = c.verse {
            self.release_verse(i, channel.rank, events);
            return;
        }
        let Ok((s, r, rule)) = self.castable(i, c.slot, c.target) else {
            return;
        };
        if s == Skill::Sigil && c.target == CastTarget::SelfCast {
            self.detonate_sigil(i, events);
            return;
        }
        let e = &mut self.entities[i];
        if s == Skill::Chakram {
            e.status.stamina_spent = 100. - chakram::stamina(e, self.state.tick) + rule.cost;
            e.status.stamina_spent_at = self.state.tick;
        } else {
            e.energy -= rule.cost;
        }
        e.cooldown[c.slot.idx()] =
            self.state.tick + (rule.cooldown as f32 / (1. + e.item_stats.cooldown)).ceil() as u64;
        let src = e.id;
        let team = e.team;
        let origin = e.position;
        let bonus_hp = e.bonus_hp;
        let weapon = e.stats.weapon;
        let cp = e.item_stats.crystal;
        events.push(Event::Cast {
            source: src,
            slot: c.slot,
            target: c.target,
        });
        let id = match c.target {
            CastTarget::SelfCast => src,
            CastTarget::Entity(t) => t,
            CastTarget::Point(_) => 0,
        };
        let aimed = self.index(id);
        let point = match c.target {
            CastTarget::Point(p) => p,
            _ => origin,
        };
        match s {
            Skill::Verse => unreachable!("Verse releases its paid channel separately"),
            Skill::Skedaddle => self.release_skedaddle(i, r, events),
            Skill::Bulwark => self.create_bulwark(i, r, events),
            Skill::Stormguard => self.raise_stormguard(i, r),
            Skill::BlastTremor => self.blast_tremor(i, r, point, events),
            Skill::WolfMaw => self.launch_flail(i, r, point),
            Skill::IronMandate => self.land_mandate(i, r, point, events),
            Skill::Pursuit => {
                let rules = self.abilities.pursuit.as_ref().unwrap();
                self.entities[i].status.captain_effects.arm().pursuit =
                    Some(rules.arm(r, self.tick, cp));
                self.entities[i].next_attack = self.tick;
            }
            Skill::Overwhelm => {
                let rules = self.abilities.overwhelm.as_ref().unwrap();
                self.entities[i].status.captain_effects.arm().overwhelm =
                    Some(rules.charge(r, self.tick, cp));
            }
            Skill::ChillWinds => {
                let b = self
                    .abilities
                    .chill_winds
                    .as_ref()
                    .expect("validated Chill Winds");
                let radius = b.radius[r];
                let damage = b.damage.at(r, cp, 0.);
                let end = self.tick + (b.root_seconds[r] * TICK_HZ as f32).round() as u64;
                events.push(Event::ChillWinds {
                    source: src,
                    position: origin,
                    heading: self.entities[i].status.motion_heading,
                });
                let pierce = self.entities[i].item_stats.shield_pierce;
                for j in 0..self.entities.len() {
                    let target = &mut self.entities[j];
                    if !matches!(target.kind, Kind::Hero | Kind::Minion)
                        || target.team == team
                        || target.hp <= 0.
                        || target.position.dist(origin) > radius
                        || !self.state.layout.clear(origin, target.position)
                    {
                        continue;
                    }
                    let rooted = !target.status.blocks_movement_impairment(self.state.tick);
                    if rooted {
                        target.status.root_until = target.status.root_until.max(end);
                    }
                    let raw = if target.kind == Kind::Minion {
                        damage * 0.5
                    } else {
                        damage
                    };
                    let applied = shop::mitigated(raw, target.stats.shield, pierce);
                    self.hurt_with_effect(
                        src,
                        team,
                        j,
                        applied,
                        (DamageSchool::Crystal, ImpactEffect::ChillWinds),
                        events,
                    );
                    if rooted
                        && matches!(self.layout, Layout::Aral { .. })
                        && end - self.tick == TICK_HZ
                        && self.entities[j].hp > 0.
                    {
                        let instance = self.next_id;
                        self.next_id += 1;
                        events.push(Event::ChillWindsRoot {
                            source: src,
                            target: self.entities[j].id,
                            instance,
                        });
                    }
                }
            }
            Skill::WinterSpire => {
                let state = &mut *self.state;
                let a = state
                    .abilities
                    .winter_spire
                    .as_ref()
                    .expect("validated learned Winter Spire");
                let first = state.tick
                    + ((a.first_delay[r] * TICK_HZ as f32).round() as u64)
                        .saturating_sub(rule.windup);
                let second = first + (a.shatter_delay[r] * TICK_HZ as f32).round() as u64;
                for (due, damage) in [
                    (first, a.first.at(r, cp, 0.)),
                    (second, a.shatter.at(r, cp, 0.)),
                ] {
                    self.entities.spawn_spell(SpellEffect {
                        due,
                        src,
                        team,
                        area: Area::WinterSpire(point, a.radius[r]),
                        damage,
                        heal: 0.,
                        channel: 0,
                    });
                }
                if matches!(self.layout, Layout::Aral { .. }) {
                    let id = self.next_id;
                    self.next_id += 1;
                    events.push(Event::WinterSpireProjectile {
                        source: src,
                        id,
                        point,
                    });
                }
            }
            Skill::FishFood | Skill::Splashdown | Skill::Waterwall => {
                self.cast_water(i, c.slot, c.target, r, events);
            }
            Skill::Chakram => {
                let (damage, refund) = if let Some(ranks) = &self.abilities.idris_b {
                    // The tooltip specifies CBASE, CR and BWR (bonus weapon ratio).
                    let items = &self.entities[i].item_stats;
                    (
                        ranks.damage.at(r, items.crystal, items.bonus_weapon),
                        Some(ranks.stamina_from_hero[r]),
                    )
                } else {
                    (weapon * self.abilities.chakram_weapon_ratio, None)
                };
                let mut shot = chakram::Chakram::new(src, team, origin, point, damage, refund);
                if matches!(self.layout, Layout::Aral { .. }) {
                    let id = self.next_id;
                    // The two self-applied cast effects reserve consecutive world instances.
                    self.next_id += 3;
                    events.push(Event::ChakramApplied { source: src, id });
                    shot.arm_native(id + 2, self.tick + 6);
                }
                self.entities.spawn_spell(shot);
            }
            Skill::Achilles => {
                let j = aimed.unwrap();
                let (raw, duration, factor) = match &self.abilities.ringo_a {
                    Some(a) => (
                        a.damage.at(r, self.entities[i].item_stats.crystal, 0.),
                        a.duration[r],
                        1. - a.slow[r],
                    ),
                    None => (65. + r as f32 * 30., 1.5, 0.7),
                };
                if !self.entities[j]
                    .status
                    .blocks_movement_impairment(self.state.tick)
                {
                    self.entities[j].status.slow_until =
                        self.tick + (duration * TICK_HZ as f32).ceil() as u64;
                    self.entities[j].status.slow_factor = factor;
                }
                let dmg = shop::mitigated(
                    raw * self.entities[i].long_shot.map_or(1., |t| t.damage),
                    self.entities[j].stats.shield,
                    self.entities[i].item_stats.shield_pierce,
                );
                self.hurt(src, team, j, dmg, DamageSchool::Crystal, events);
            }
            Skill::Twirl => {
                let e = &mut self.entities[i];
                let duration = self
                    .state
                    .abilities
                    .ringo_b
                    .as_ref()
                    .map_or(60, |b| (b.duration[r] * TICK_HZ as f32).ceil() as u64);
                e.status.haste_until = self.state.tick + duration;
                e.status.twirl_rank = self.state.abilities.ringo_b.as_ref().map(|_| r);
                e.next_attack = self.state.tick;
            }
            Skill::Hellfire => {
                let state = &mut *self.state;
                let j = aimed.unwrap();
                let travel =
                    (origin.dist(self.entities[j].position) * TICK_HZ as f32 / 12.).ceil() as u64;
                if let Some(c) = &state.abilities.ringo_c {
                    let cp = self.entities[i].item_stats.crystal;
                    let impact = state.tick + travel;
                    let base = SpellEffect {
                        due: impact,
                        src,
                        team,
                        area: Area::Hellfire(id, c.radius[r], true),
                        damage: c.impact.at(r, cp, 0.),
                        heal: 0.,
                        channel: 0,
                    };
                    self.entities.spawn_spell(base);
                    for second in 1..=c.duration[r] as u64 {
                        self.entities.spawn_spell(SpellEffect {
                            due: impact + second * TICK_HZ,
                            area: Area::Hellfire(id, c.burn_radius[r], false),
                            damage: c.burn.at(r, cp, 0.),
                            ..base
                        });
                    }
                    return;
                }
                let damage = 140. + r as f32 * 45.;
                self.entities.spawn_spell(SpellEffect::impact(
                    self.tick + travel,
                    src,
                    team,
                    id,
                    damage,
                    DamageSchool::True,
                ));
                for n in 1..=3 {
                    self.entities.spawn_spell(SpellEffect::impact(
                        self.tick + travel + n * TICK_HZ,
                        src,
                        team,
                        id,
                        damage * 0.16,
                        DamageSchool::True,
                    ));
                }
            }
            Skill::Gift if self.abilities.fire.is_some() => {
                self.gift_of_fire(i, aimed.unwrap(), r, events)
            }
            Skill::Gift => {
                let j = aimed.unwrap();
                self.heal(
                    src,
                    j,
                    self.abilities
                        .other
                        .as_ref()
                        .map_or(45. + 25. * r as f32, |o| o.gift.at(r, cp, 0.))
                        + bonus_hp * self.abilities.gift_hp,
                    events,
                );
            }
            Skill::Agent => {
                if let Some(f) = &self.state.abilities.fire {
                    self.entities[i].next_attack = self.state.tick;
                    let s = &mut self.entities[aimed.unwrap()].status;
                    s.agent_source = src;
                    s.agent_burn_bonus = f.agent.at(r, cp, 0.);
                }
                let t = &mut self.entities[aimed.unwrap()];
                t.status.empowered = self.state.abilities.empower[r] as u8;
                if let Some(o) = &self.state.abilities.other {
                    t.status.empower_damage = o.agent.at(r, cp, 0.)
                        + if t.id == src {
                            o.self_agent.at(r, cp, 0.)
                        } else {
                            0.
                        };
                    t.status.agent_crystal_until = Some(
                        self.state.tick + (o.agent_duration[r] * TICK_HZ as f32).round() as u64,
                    );
                } else {
                    t.status.empower_damage = 25. + r as f32 * 10.;
                }
            }
            Skill::Sigil if self.abilities.sigil.is_some() => self.start_sigil(i, r, point),
            Skill::Sigil => {
                let state = &mut *self.state;
                let bonus = bonus_hp;
                let heal = state.abilities.sigil_heal[r] + bonus * state.abilities.sigil_hp;
                for sec in 0..3 {
                    self.entities.spawn_spell(SpellEffect {
                        due: state.tick + 1 + sec * TICK_HZ,
                        src,
                        team,
                        area: Area::Circle(
                            point,
                            state
                                .abilities
                                .other
                                .as_ref()
                                .map_or(2.5, |o| o.sigil_radius[r]),
                        ),
                        damage: state
                            .abilities
                            .other
                            .as_ref()
                            .map_or(40. + 15. * r as f32, |o| o.sigil_dps.at(r, cp, 0.)),
                        heal,
                        channel: 0,
                    });
                }
            }
            Skill::Suppress | Skill::Strike => {
                let token = self.tick;
                self.entities[i].status.channel = token;
                let mut area = Area::Line(origin, point, 1.4);
                let (hits, pulse, damage) = if s == Skill::Strike {
                    (
                        self.abilities.strike_hits,
                        5.,
                        self.abilities
                            .other
                            .as_ref()
                            .map_or(self.abilities.strike_dmg, |o| o.shin.at(r, cp, 0.)),
                    )
                } else if let Some(o) = &self.abilities.other {
                    let count = o.saw_ticks[r];
                    area = Area::Suppression {
                        from: origin,
                        to: point,
                        slow: 1. - o.saw_slow[r],
                        ticks: (o.saw_slow_duration[r] * TICK_HZ as f32).round() as u64,
                        minion_multiplier: 1. - o.saw_minion_reduction[r],
                    };
                    (
                        count as u8,
                        o.saw_duration[r] * TICK_HZ as f32 / count,
                        o.saw_dps.at(r, cp, 0.) * o.saw_duration[r] / count,
                    )
                } else {
                    (4, 10., self.abilities.suppress_dps[r] * 0.5)
                };
                self.entities[i].status.channel_until =
                    self.tick + 1 + (f32::from(hits - 1) * pulse).round() as u64;
                let state = &mut *self.state;
                for n in 0..hits {
                    self.entities.spawn_spell(SpellEffect {
                        due: state.tick + 1 + (f32::from(n) * pulse).round() as u64,
                        src,
                        team,
                        area,
                        damage,
                        heal: 0.,
                        channel: token,
                    });
                }
            }
            Skill::Wind if self.abilities.twister.is_some() => self.begin_twister(i, r, point),
            Skill::Wind => {
                self.entities.spawn_spell(SpellEffect {
                    due: self.tick + 1,
                    src,
                    team,
                    area: Area::Circle(point, 2.),
                    damage: self
                        .state
                        .abilities
                        .other
                        .as_ref()
                        .map_or(self.state.abilities.wind_dmg[r], |o| o.karas.at(r, cp, 0.)),
                    heal: 0.,
                    channel: 0,
                });
            }
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize)]
enum Area {
    BasicTarget(u32, DamageSchool),
    Target(u32, DamageSchool),
    NpcWindup(u32),
    Hellfire(u32, f32, bool),
    WinterSpire(Position, f32),
    Circle(Position, f32),
    Line(Position, Position, f32),
    Suppression {
        from: Position,
        to: Position,
        slow: f32,
        ticks: u64,
        minion_multiplier: f32,
    },
}
#[derive(bevy_ecs::prelude::Component, Clone, Copy, Debug, Serialize)]
pub(super) struct SpellEffect {
    due: u64,
    src: u32,
    team: u8,
    area: Area,
    damage: f32,
    heal: f32,
    channel: u64,
}
impl SpellEffect {
    pub(super) fn npc_windup(due: u64, src: u32, team: u8, target: u32, damage: f32) -> Self {
        Self {
            due,
            src,
            team,
            area: Area::NpcWindup(target),
            damage,
            heal: 0.,
            channel: 0,
        }
    }

    pub fn basic_impact(
        due: u64,
        src: u32,
        team: u8,
        id: u32,
        damage: f32,
        school: DamageSchool,
    ) -> Self {
        let mut result = Self::impact(due, src, team, id, damage, school);
        result.area = Area::BasicTarget(id, school);
        result
    }

    pub fn impact(
        due: u64,
        src: u32,
        team: u8,
        id: u32,
        damage: f32,
        school: DamageSchool,
    ) -> Self {
        Self {
            due,
            src,
            team,
            area: Area::Target(id, school),
            damage,
            heal: 0.,
            channel: 0,
        }
    }
}
/// Median launch-to-impact delays by two-unit separation band, quantized to
/// 20 Hz. The sparse outer bands reuse the last populated band.
fn npc_flight_ticks(template: u16, victim: u16, distance: f32) -> u64 {
    if template == EntityTemplate::OuterTurret5v5.as_raw() {
        return match EntityTemplate::from_raw(victim) {
            Some(EntityTemplate::MinionRange5v5 | EntityTemplate::MinionMelee5v5) => 7,
            Some(EntityTemplate::MinionSiege5v5) => 5,
            _ => unreachable!("only measured Solo minion targets schedule a windup"),
        };
    }
    let bins = match EntityTemplate::from_raw(template) {
        Some(EntityTemplate::RangedMinion) => [2, 6, 12, 14, 14],
        Some(EntityTemplate::LeadMinion) => [2, 6, 10, 10, 10],
        Some(EntityTemplate::Turret | EntityTemplate::VainTurret) => [4, 4, 6, 10, 14],
        _ => unreachable!("only measured ARAL templates schedule a windup"),
    };
    bins[((distance / 2.) as usize).min(bins.len() - 1)]
}

pub(super) fn line_distance(p: Position, a: Position, b: Position) -> f32 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let denom = dx * dx + dy * dy;
    if denom == 0. {
        return p.dist(a);
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / denom).clamp(0., 1.);
    p.dist(Position {
        x: a.x + dx * t,
        y: a.y + dy * t,
    })
}
impl Room<'_> {
    pub(super) fn predicted_hp(&self, e: &Entity, at: u64) -> f32 {
        let incoming: f32 = self
            .entities
            .spells::<SpellEffect>()
            .filter(|v| {
                v.due <= at
                    && matches!(v.area,Area::Target(id, _) | Area::BasicTarget(id,_) if id==e.id)
            })
            .map(|v| v.damage)
            .sum();
        (e.hp - incoming).max(0.)
    }
    pub(super) fn npc_launch_delay(&self, attacker: &Entity, victim: &Entity) -> Option<u64> {
        if matches!(self.layout, Layout::SoloMid { .. }) {
            return (attacker.kind == Kind::Tower
                && attacker.type_id == EntityTemplate::OuterTurret5v5.as_raw()
                && matches!(attacker.id, 510 | 512 | 514)
                && victim.kind == Kind::Minion
                && matches!(
                    EntityTemplate::from_raw(victim.type_id),
                    Some(
                        EntityTemplate::MinionRange5v5
                            | EntityTemplate::MinionMelee5v5
                            | EntityTemplate::MinionSiege5v5
                    )
                ))
            .then_some(2);
        }
        if !matches!(self.layout, Layout::Aral { .. }) {
            return None;
        }
        match (attacker.kind, EntityTemplate::from_raw(attacker.type_id)) {
            (Kind::Tower, Some(EntityTemplate::Turret | EntityTemplate::VainTurret)) => Some(6),
            (Kind::Minion, Some(EntityTemplate::RangedMinion | EntityTemplate::LeadMinion)) => {
                Some(10)
            }
            _ => None,
        }
    }
}

impl crate::persistent_spells::TimedSpell for SpellEffect {
    fn expired_before_step(&self, _: u64) -> bool {
        false
    }
    fn begin_impact(&mut self) -> Self {
        *self
    }
    fn advance(&mut self, world: &mut Room<'_>, events: &mut Vec<Event>) -> bool {
        if world.winner.is_some() {
            return false;
        }
        if self.due > world.tick {
            return true;
        }
        if self.channel > 0
            && !world.index(self.src).is_some_and(|i| {
                world.entities[i].status.channel == self.channel && world.entities[i].hp > 0.
            })
        {
            return false;
        }
        if let Area::NpcWindup(target) = self.area {
            let (Some(a), Some(b)) = (world.index(self.src), world.index(target)) else {
                return false;
            };
            let (source, victim) = (&world.entities[a], &world.entities[b]);
            if source.hp <= 0. || victim.hp <= 0. {
                return false;
            }
            let distance = source.position.dist(victim.position);
            let ticks = npc_flight_ticks(source.type_id, victim.type_id, distance);
            let id = world.next_id;
            world.next_id += 1;
            self.area = Area::Target(target, DamageSchool::Weapon);
            self.due = world.tick + ticks;
            events.push(Event::NpcProjectile {
                id,
                source: self.src,
                target,
            });
            return true;
        }
        if let Area::BasicTarget(id, school) = self.area {
            if let Some(i) = world.index(id) {
                world.basic_hit(self.src, self.team, i, self.damage, school, events);
            }
            return false;
        }
        if let Area::Target(id, school) = self.area {
            if let Some(i) = world.index(id) {
                world.hurt(self.src, self.team, i, self.damage, school, events)
            }
            return false;
        }
        let mut shield_ignoring = false;
        if let Area::Hellfire(id, radius, impact) = self.area {
            let Some(target) = world
                .index(id)
                .map(|i| &world.entities[i])
                .filter(|t| t.hp > 0.)
            else {
                return false;
            };
            self.area = Area::Circle(target.position, radius);
            shield_ignoring = impact;
        }
        let pierce = world
            .index(self.src)
            .map_or(0., |i| world.entities[i].item_stats.shield_pierce);
        for i in 0..world.entities.len() {
            let e = &world.entities[i];
            if e.hp <= 0. || !matches!(e.kind, Kind::Hero | Kind::Minion) {
                continue;
            }
            let hit = match self.area {
                Area::Circle(p, r) | Area::WinterSpire(p, r) => p.dist(e.position) <= r,
                Area::Suppression { from, to, .. } => line_distance(e.position, from, to) <= 1.4,
                Area::Line(a, b, r) => line_distance(e.position, a, b) <= r,
                Area::BasicTarget(..)
                | Area::Target(..)
                | Area::NpcWindup(_)
                | Area::Hellfire(..) => unreachable!(),
            };
            if !hit {
                continue;
            }
            if e.team == self.team {
                if self.heal > 0. {
                    world.heal(self.src, i, self.heal, events)
                }
            } else {
                let mut raw = self.damage;
                if matches!(self.area, Area::WinterSpire(..)) && e.kind == Kind::Minion {
                    raw *= 0.5;
                }
                if let Area::Suppression {
                    minion_multiplier, ..
                } = self.area
                    && e.kind == Kind::Minion
                {
                    raw *= minion_multiplier;
                }
                let dmg = if shield_ignoring {
                    raw
                } else {
                    shop::mitigated(raw, e.stats.shield, pierce)
                };
                if let Area::Suppression { slow, ticks, .. } = self.area
                    && !world.entities[i]
                        .status
                        .blocks_movement_impairment(world.state.tick)
                {
                    world.entities[i].status.slow_factor = slow;
                    world.entities[i].status.slow_until = world.tick + ticks;
                }
                world.hurt(self.src, self.team, i, dmg, DamageSchool::Crystal, events)
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn duel() -> World {
        let mut w = World::new(1, 10);
        w.entities[0].position = Position { x: 26., y: 0. };
        w.entities[1].position = Position { x: 29., y: 0. };
        w
    }
    #[test]
    fn ringo_rank_controls_release_debit_range_and_ready_tick() {
        let mut w = duel();
        std::sync::Arc::make_mut(&mut w.abilities).achilles = CastRanks {
            range: [8.; 5],
            energy: [40., 45., 50., 55., 60.],
            seconds: [9., 8.5, 8., 7.5, 7.],
        };
        let actor = w.entities[0].id;
        let target = w.entities[1].id;
        w.entities[0].level = 3;
        w.entities[0].skill_points = 2;
        w.command(actor, Command::Learn(AbilitySlot::A)).unwrap();
        w.command(actor, Command::Learn(AbilitySlot::A)).unwrap();
        let cast = Command::Cast {
            slot: AbilitySlot::A,
            target: CastTarget::Entity(target),
        };
        w.entities[0].energy = 44.;
        assert_eq!(w.command(actor, cast), Err("ability not ready"));
        w.entities[0].energy = 90.;
        w.entities[1].position.x = 34.01;
        assert_eq!(w.command(actor, cast), Err("cast out of range"));
        w.entities[1].position.x = 34.;
        w.command(actor, cast).unwrap();
        w.tick = 4;
        w.advance_cast(0, &mut Vec::new());
        assert_eq!(w.entities[0].energy, 45.);
        let deadline = 4 + 170;
        assert_eq!(w.entities[0].cooldown[0], deadline);
        w.tick = deadline - 1;
        assert_eq!(w.command(actor, cast), Err("ability not ready"));
        w.tick = deadline;
        w.command(actor, cast).unwrap();
    }

    #[test]
    #[ignore = "requires the original delivery and optional 4.13.4 server data"]
    fn delivered_ringo_tables_reach_every_learnable_rank() {
        use std::{env, path::PathBuf};
        let base = PathBuf::from(env::var_os("HALCYON_DELIVERY").expect("HALCYON_DELIVERY"));
        let extra = PathBuf::from(env::var_os("HALCYON_DATA_EXTRA").expect("HALCYON_DATA_EXTRA"));
        let data = GameData::load_with_extra(base, Some(&extra)).unwrap();
        let w = World::from_data(1, 10, &data).unwrap();
        for (skill, costs, seconds, range) in [
            (
                Skill::Achilles,
                &[40., 45., 50., 55., 60.][..],
                &[9., 8.5, 8., 7.5, 7.][..],
                8.,
            ),
            (
                Skill::Twirl,
                &[40., 45., 50., 55., 60.][..],
                &[8.; 5][..],
                0.,
            ),
            (
                Skill::Hellfire,
                &[100., 115., 130.][..],
                &[90., 80., 70.][..],
                13.,
            ),
        ] {
            for (rank, (&cost, &seconds)) in costs.iter().zip(seconds).enumerate() {
                let rule = w.rules(skill, rank);
                assert_eq!(
                    (rule.range, rule.cost, rule.cooldown),
                    (range, cost, (seconds * TICK_HZ as f32) as u64)
                );
            }
        }
    }

    #[test]
    fn tracked_npc_shot_keeps_its_due_time_and_never_reuses_an_actor_id() {
        let mut w = World::new(7, 6);
        w.use_six_human_roster().unwrap();
        w.use_aral_lane().unwrap();
        let tower = w
            .entities
            .iter()
            .position(|e| w.aral_structure_slot(e.id) == Some(7))
            .unwrap();
        let victim = w
            .entities
            .iter()
            .position(|e| e.kind == Kind::Hero && e.team != w.entities[tower].team)
            .unwrap();
        let source = w.entities[tower].id;
        let target = w.entities[victim].id;
        let team = w.entities[tower].team;
        let origin = w.entities[tower].position;
        w.entities[victim].position = Position {
            x: origin.x + 7.,
            y: origin.y,
        };
        let first_id = w.next_id;
        let mut events = Vec::new();
        w.entities
            .spawn_spell(SpellEffect::npc_windup(6, source, team, target, 42.));
        for tick in 1..=6 {
            w.tick = tick;
            w.advance_spell_family::<SpellEffect>(&mut events);
            assert_eq!(events.len(), usize::from(tick == 6));
        }
        assert_eq!(
            events,
            [Event::NpcProjectile {
                id: first_id,
                source,
                target
            }]
        );
        assert!(w.index(first_id).is_none());
        assert_eq!(w.next_id, first_id + 1);
        w.entities[tower].hp = 0.;
        let hp = w.entities[victim].hp;
        for tick in 7..=16 {
            w.tick = tick;
            w.advance_spell_family::<SpellEffect>(&mut events);
            assert_eq!(
                w.entities[victim].hp,
                if tick == 16 { hp - 42. } else { hp }
            );
        }
        assert!(
            matches!(events.last(),Some(Event::Damage{source:a,target:b,..}) if *a==source && *b==target)
        );
        w.entities
            .spawn_spell(SpellEffect::npc_windup(17, source, team, target, 42.));
        w.tick = 17;
        w.advance_spell_family::<SpellEffect>(&mut events);
        assert!(w.entities.spells::<SpellEffect>().next().is_none());
        assert_eq!(w.next_id, first_id + 1);

        w.entities[tower].hp = 10.;
        w.entities
            .spawn_spell(SpellEffect::npc_windup(18, source, team, target, 42.));
        w.tick = 18;
        w.advance_spell_family::<SpellEffect>(&mut events);
        w.entities[victim].hp = 0.;
        let before = events.len();
        w.tick = 28;
        w.advance_spell_family::<SpellEffect>(&mut events);
        assert_eq!(events.len(), before);
        assert!(w.entities.spells::<SpellEffect>().next().is_none());
    }

    #[test]
    fn solo_outer_tower_launches_then_impacts_only_the_measured_minion_profiles() {
        for (stat, flight) in [(5, 7), (4, 7), (6, 5)] {
            let mut w = World::new(12, 10);
            w.use_solo_mid_lane().unwrap();
            let tower = w.index(510).unwrap();
            let p = w.entities[tower].position;
            let target = w.spawn(
                1,
                Kind::Minion,
                stat,
                Position {
                    x: p.x + 3.,
                    y: p.y,
                },
                false,
            );
            let victim = w.index(target).unwrap();
            assert_eq!(
                w.npc_launch_delay(&w.entities[tower], &w.entities[victim]),
                Some(2)
            );
            assert_eq!(w.npc_launch_delay(&w.entities[tower], &w.entities[1]), None);
            w.entities[tower].order = Command::Attack(target);
            w.entities[tower].winding = Some((1, target));
            let events = w.step();
            assert!(
                events
                    .iter()
                    .any(|e| matches!(e,Event::Attack{source:510,target:t} if *t==target))
            );
            for n in 1..=flight + 2 {
                let events = w.step();
                assert_eq!(
                    events.iter().any(
                        |e| matches!(e,Event::NpcProjectile{source:510,target:t,..} if *t==target)
                    ),
                    n == 2
                );
                assert_eq!(
                    events
                        .iter()
                        .any(|e| matches!(e,Event::Damage{source:510,target:t,..} if *t==target)),
                    n == flight + 2
                );
                if n == 2 {
                    w.entities[tower].hp = 0.;
                }
            }
        }
    }

    #[test]
    fn only_original_aral_ranged_npcs_use_the_measured_delays() {
        let mut w = World::new(3, 6);
        let tower = w.entities.iter().find(|e| e.kind == Kind::Tower).unwrap();
        assert_eq!(w.npc_launch_delay(tower, &w.entities[0]), None);
        w.use_six_human_roster().unwrap();
        w.use_aral_lane().unwrap();
        for _ in 0..18 * TICK_HZ {
            w.step();
        }
        for e in &w.entities {
            let delay = match e.type_id {
                300 | 301 => Some(6),
                287 | 288 => Some(10),
                _ => None,
            };
            assert_eq!(w.npc_launch_delay(e, &w.entities[0]), delay);
        }
        assert_eq!(npc_flight_ticks(287, 243, 5.), 12);
        assert_eq!(npc_flight_ticks(288, 243, 5.), 10);
        assert_eq!(npc_flight_ticks(300, 243, 5.), 6);
        assert_eq!(npc_flight_ticks(301, 243, 9.), 14);
    }

    #[test]
    fn windup_interrupt_and_release_charge_once() {
        let mut w = duel();
        let slot = AbilitySlot::A;
        let cast = Command::Cast {
            slot,
            target: CastTarget::Entity(2),
        };
        assert!(w.command(1, cast).is_err());
        w.command(1, Command::Learn(slot)).unwrap();
        w.command(1, cast).unwrap();
        w.step();
        w.command(1, Command::Stop).unwrap();
        for _ in 0..5 {
            w.step();
        }
        assert_eq!(w.entities[0].energy, w.entities[0].max_energy);
        assert_eq!(w.entities[1].hp, 703.);
        w.command(1, cast).unwrap();
        let mut fired = 0;
        for _ in 0..5 {
            fired += w
                .step()
                .iter()
                .filter(|e| matches!(e, Event::Cast { source: 1, .. }))
                .count();
        }
        assert_eq!(fired, 1);
        assert!(w.entities[1].hp < 703.);
        assert!(w.entities[0].energy < 145.);
        assert!(w.command(1, cast).is_err());
    }
    #[test]
    fn native_lorelai_cooldown_uses_release_tick_and_equipped_reduction() {
        let mut w = World::new(0, 10);
        w.entities[0].type_id = 409;
        w.entities[0].gold = 250;
        w.purchase(1, BasicItem::Hourglass).unwrap();
        let slot = AbilitySlot::B;
        w.command(1, Command::Learn(slot)).unwrap();
        let cast = Command::Cast {
            slot,
            target: CastTarget::Point(w.entities[0].position),
        };
        w.command(1, cast).unwrap();
        w.command(1, Command::Stop).unwrap();
        w.step();
        assert_eq!(w.entities[0].cooldown[slot.idx()], 0);
        w.command(1, cast).unwrap();
        let mut released = false;
        for _ in 0..3 {
            if w.step()
                .iter()
                .any(|e| matches!(e, Event::Cast { source: 1, .. }))
            {
                released = true;
                break;
            }
        }
        assert!(released);
        let deadline = w.entities[0].cooldown[slot.idx()];
        assert_eq!(deadline, w.tick + 149); // ceil(8 seconds * 20 Hz / 1.075).
        w.tick = deadline - 1;
        assert!(w.command(1, cast).is_err());
        w.tick = deadline;
        assert!(w.command(1, cast).is_ok());
    }

    #[test]
    fn targeted_spell_rechecks_visibility_and_range_on_release() {
        let mut w = duel();
        w.command(1, Command::Learn(AbilitySlot::A)).unwrap();
        w.command(
            1,
            Command::Cast {
                slot: AbilitySlot::A,
                target: CastTarget::Entity(2),
            },
        )
        .unwrap();
        w.entities[1].position = base(1);
        for _ in 0..8 {
            w.step();
        }
        assert_eq!(w.entities[0].energy, w.entities[0].max_energy);
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
    }
    #[test]
    fn saw_channel_stops_before_next_damage_pulse() {
        let mut w = duel();
        w.entities[0].type_id = 250;
        w.command(1, Command::Learn(AbilitySlot::B)).unwrap();
        w.command(
            1,
            Command::Cast {
                slot: AbilitySlot::B,
                target: CastTarget::Point(Position { x: 33., y: 0. }),
            },
        )
        .unwrap();
        for _ in 0..8 {
            w.step();
        }
        let hp = w.entities[1].hp;
        assert!((hp - (703. - 70. / 1.2)).abs() < 0.01);
        w.command(1, Command::MoveTo(Position { x: 24., y: 0. }))
            .unwrap();
        for _ in 0..40 {
            w.step();
        }
        assert_eq!(w.entities[1].hp, hp);
    }
    #[test]
    fn lyra_sigil_applies_heal_and_shielded_damage_only_inside_zone() {
        let mut w = duel();
        w.entities[0].type_id = 269;
        w.entities[2].position = Position { x: 27.5, y: 0. };
        w.entities[2].hp = 100.;
        w.command(1, Command::Learn(AbilitySlot::A)).unwrap();
        w.command(
            1,
            Command::Cast {
                slot: AbilitySlot::A,
                target: CastTarget::Point(Position { x: 28., y: 0. }),
            },
        )
        .unwrap();
        for _ in 0..8 {
            w.step();
        }
        assert_eq!(w.entities[2].hp, 125.);
        let enemy = w.entities[1].hp;
        w.entities[1].position = Position { x: 33., y: 0. };
        for _ in 0..40 {
            w.step();
        }
        assert_eq!(w.entities[2].hp, 175.);
        assert_eq!(w.entities[1].hp, enemy);
    }
    #[test]
    fn attack_projectile_survives_attacker_stop_and_is_counted_for_last_hits() {
        let mut w = duel();
        let id = w.spawn(1, Kind::Minion, 4, Position { x: 30., y: 0. }, false);
        w.entities.last_mut().unwrap().hp = 10.;
        w.command(1, Command::Attack(id)).unwrap();
        for _ in 0..4 {
            w.step();
        }
        let target = w.index(id).unwrap();
        assert_eq!(w.entities[target].hp, 10.);
        assert_eq!(w.predicted_hp(&w.entities[target], w.tick + 10), 0.);
        w.command(1, Command::Stop).unwrap();
        for _ in 0..8 {
            w.step();
        }
        assert!(w.index(id).is_none());
        assert_eq!(w.entities[0].gold, 35);
    }
}
