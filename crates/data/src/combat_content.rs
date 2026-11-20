//! Client-derived combat definitions. Numeric slots with unresolved semantics retain their source names.
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{collections::BTreeMap, io, path::Path};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequirementSample {
    Costs(BTreeMap<String, u32>),
    Text(String),
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct HeroAbilities {
    pub hero_id: String,
    pub name_en: String,
    pub name_zh: String,
    pub abilities: Vec<Ability>,
    pub extra_variables: Vec<AbilityVariable>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Ability {
    pub ability_id: String,
    pub max_rank_guess: u32,
    pub name_key: Option<String>,
    pub name_en: Option<String>,
    pub name_zh: Option<String>,
    pub target_type: Option<String>,
    pub desc_key: Option<String>,
    pub short_desc_key: Option<String>,
    pub short_desc_en: Option<String>,
    pub short_desc_zh: Option<String>,
    pub damage_variables: Vec<DamageVariable>,
    pub variables: Vec<AbilityVariable>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AbilityVariable {
    pub name: String,
    pub base: f32,
    pub per_rank: f32,
    pub overdrive: f32,
    pub cp_ratio: f32,
    pub wp_ratio: f32,
    pub alt_ratio: f32,
    pub bonus_as_ratio: f32,
    pub clamp: f32,
    pub stat_label_id: u32,
    pub stat_label: Option<String>,
    pub unit_type: String,
    pub meta_raw: [u32; 4],
    pub off: u32,
    pub ranks: Option<Vec<f32>>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DamageVariable {
    pub name: Option<String>,
    pub header_u32: u32,
    pub base: f32,
    pub per_rank: f32,
    pub f2: f32,
    pub overdrive: f32,
    pub cp_ratio: f32,
    pub wp_ratio: f32,
    pub f6: f32,
    pub f7: f32,
    pub off: u32,
    pub ranks: Vec<f32>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Talent {
    pub id: String,
    pub official_id: Option<String>,
    pub hero: Option<String>,
    pub rarity: Option<String>,
    pub name_key: String,
    pub name_en: String,
    pub name_zh: String,
    pub desc_key: String,
    pub desc_en: String,
    pub desc_zh: String,
    pub icon: String,
    pub buff: String,
    pub modifiers: Vec<TalentModifier>,
    pub official_level_requirements_sample: Option<RequirementSample>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TalentModifier {
    pub label: String,
    pub base: f32,
    pub per_level: f32,
    pub overdrive: f32,
    pub cp_ratio: f32,
    pub wp_ratio: f32,
    pub clamp: f32,
    pub stat_label_id: u32,
    pub stat_label: Option<String>,
    pub unit_type: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FullUnit {
    pub name: String,
    pub is_hero: bool,
    pub entity_type_id: Option<u16>,
    pub name_en: Option<String>,
    pub name_zh: Option<String>,
    pub hp: f32,
    pub hp_per_level: f32,
    pub hp_regen: f32,
    pub hp_regen_per_level: f32,
    pub energy: f32,
    pub energy_per_level: f32,
    pub energy_regen: f32,
    pub energy_regen_per_level: f32,
    pub weapon: f32,
    pub weapon_per_level: f32,
    pub alt_damage_264: f32,
    pub alt_damage_per_level_268: f32,
    pub armor: f32,
    pub armor_per_level: f32,
    pub shield: f32,
    pub shield_per_level: f32,
    pub dmg_mod_288: f32,
    pub dmg_mod_292: f32,
    pub range: f32,
    pub attack_speed: f32,
    pub attack_speed_per_level: f32,
    pub stat_308: f32,
    pub attack_delay_312: f32,
    pub collision_radius: f32,
    pub vision_range: f32,
    pub move_speed: f32,
    pub xp_bounty: f32,
    pub gold_bounty: f32,
    pub bounty_extra_368: f32,
    pub bounty_extra_376: f32,
    pub bounty_extra_384: f32,
    pub bounty_extra_388: f32,
}

#[derive(Debug, Default)]
pub struct CombatContent {
    heroes: BTreeMap<String, HeroAbilities>,
    units: BTreeMap<String, FullUnit>,
    talents: BTreeMap<String, Talent>,
}

impl CombatContent {
    pub(crate) fn load(dir: &Path, hash: &mut Sha256) -> io::Result<Self> {
        fn indexed<T>(rows: Vec<T>, key: impl Fn(&T) -> &str) -> io::Result<BTreeMap<String, T>> {
            let mut index = BTreeMap::new();
            for row in rows {
                let name = key(&row);
                if name.is_empty() || index.contains_key(name) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("empty or duplicate combat definition key {name}"),
                    ));
                }
                index.insert(name.to_owned(), row);
            }
            Ok(index)
        }
        let heroes = indexed(
            crate::read(dir, "abilities.json", hash)?,
            |h: &HeroAbilities| &h.hero_id,
        )?;
        for hero in heroes.values() {
            let mut ids = std::collections::BTreeSet::new();
            for a in &hero.abilities {
                if a.ability_id.is_empty()
                    || !ids.insert(&a.ability_id)
                    || a.variables.iter().any(|v| {
                        v.name.is_empty()
                            || v.ranks
                                .as_ref()
                                .is_none_or(|r| r.is_empty() || r.iter().any(|n| !n.is_finite()))
                    })
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("invalid ability or rank table {}", a.ability_id),
                    ));
                }
            }
        }
        Ok(Self {
            heroes,
            units: indexed(
                crate::read(dir, "units_full.json", hash)?,
                |u: &FullUnit| &u.name,
            )?,
            talents: indexed(
                crate::read(dir, "talents_full.json", hash)?,
                |t: &Talent| &t.id,
            )?,
        })
    }
    pub fn heroes(&self) -> impl ExactSizeIterator<Item = &HeroAbilities> {
        self.heroes.values()
    }
    pub fn units(&self) -> impl ExactSizeIterator<Item = &FullUnit> + Clone {
        self.units.values()
    }
    pub fn talents(&self) -> impl ExactSizeIterator<Item = &Talent> {
        self.talents.values()
    }
    pub fn ability(&self, hero: &str, id: &str) -> Option<&Ability> {
        self.heroes
            .get(hero)?
            .abilities
            .iter()
            .find(|a| a.ability_id == id)
    }
}
impl Ability {
    pub fn variable(&self, name: &str) -> Option<&AbilityVariable> {
        let mut matches = self
            .variables
            .iter()
            .filter(|v| v.name.eq_ignore_ascii_case(name));
        let first = matches.next()?;
        matches.next().is_none().then_some(first)
    }
}
impl crate::GameData {
    pub fn hero_abilities(&self, name: &str) -> Option<&HeroAbilities> {
        self.extra.combat_content().heroes.get(name)
    }
    pub fn ability(&self, hero: &str, id: &str) -> Option<&Ability> {
        self.extra.combat_content().ability(hero, id)
    }
    pub fn full_unit(&self, name: &str) -> Option<&FullUnit> {
        self.extra.combat_content().units.get(name)
    }
    pub fn talent_definition(&self, id: &str) -> Option<&Talent> {
        self.extra.combat_content().talents.get(id)
    }
}
impl TalentModifier {
    /// A level is an earned talent level. This is independent of a hero's ability overdrive rank.
    /// The two extracted nonzero clamps are lower bounds on decreasing charge-time multipliers.
    pub fn value_at(&self, level: std::num::NonZeroU8, crystal: f32, weapon: f32) -> Option<f32> {
        if !crystal.is_finite() || !weapon.is_finite() || self.overdrive != 0. {
            return None;
        }
        let base = self.base
            + f32::from(level.get() - 1) * self.per_level
            + crystal * self.cp_ratio
            + weapon * self.wp_ratio;
        let value = if self.clamp == 0. {
            base
        } else if self.label == "CHARGE_TIME" && self.per_level < 0. {
            base.max(self.clamp)
        } else {
            return None;
        };
        value.is_finite().then_some(value)
    }
}
impl Talent {
    pub fn modifier(&self, label: &str) -> Option<&TalentModifier> {
        let mut found = self.modifiers.iter().filter(|m| m.label == label);
        let first = found.next()?;
        found.next().is_none().then_some(first)
    }
}

/// Targeting categories attached to shipped definitions, including internal actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetType<'a> {
    Directional,
    Targeted,
    Radius,
    SelfBuff,
    AllyBuff,
    Untargeted,
    CenterOnTarget,
    Unspecified,
    Other(&'a str),
}
impl Ability {
    pub fn targeting(&self) -> Option<TargetType<'_>> {
        Some(match self.target_type.as_deref()? {
            "LABEL_ABILITY_TYPE_DIRECTIONAL" => TargetType::Directional,
            "LABEL_ABILITY_TYPE_TARGETED" => TargetType::Targeted,
            "LABEL_ABILITY_TYPE_RADIUS" => TargetType::Radius,
            "LABEL_ABILITY_TYPE_BUFF_SELF" => TargetType::SelfBuff,
            "LABEL_ABILITY_TYPE_BUFF_ALLY" => TargetType::AllyBuff,
            "LABEL_ABILITY_TYPE_UNTARGETED" => TargetType::Untargeted,
            "LABEL_ABILITY_TYPE_CENTER_ON_TARGET" => TargetType::CenterOnTarget,
            "<none>" => TargetType::Unspecified,
            other => TargetType::Other(other),
        })
    }
}

impl Ability {
    /// Damage block headers use 32-bit FNV-1a of the exact tooltip operand (including case).
    pub fn damage(&self, operand: &str) -> Option<&DamageVariable> {
        let header = operand.bytes().fold(0x811c9dc5_u32, |h, byte| {
            (h ^ u32::from(byte)).wrapping_mul(0x01000193)
        });
        let mut found = self
            .damage_variables
            .iter()
            .filter(|v| v.header_u32 == header);
        let first = found.next()?;
        found.next().is_none().then_some(first)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU8;
    #[test]
    fn linear_modifiers_distinguish_cost_sample_and_clamp_direction() {
        let level = NonZeroU8::new(4).unwrap();
        let mut modifier = TalentModifier {
            base: 1.,
            per_level: 0.5,
            cp_ratio: 0.5,
            wp_ratio: 1.,
            ..Default::default()
        };
        assert_eq!(modifier.value_at(level, 10., 3.), Some(10.5));
        modifier.clamp = 1.;
        assert_eq!(modifier.value_at(level, 10., 3.), None);
        modifier.label = "CHARGE_TIME".into();
        modifier.per_level = -0.9;
        assert_eq!(modifier.value_at(level, 0., 0.), Some(1.));
    }

    #[test]
    #[ignore = "requires the original delivery and supplement"]
    fn shipped_full_combat_definitions_preserve_headers_modifiers_and_rank_tables() {
        let base = std::env::var_os("HALCYON_DELIVERY").unwrap();
        let extra = std::env::var_os("HALCYON_DATA_EXTRA").unwrap();
        let data = crate::GameData::load_with_extra(base, Some(Path::new(&extra))).unwrap();
        let content = data.extra.combat_content();
        assert_eq!(
            (
                content.heroes().len(),
                content.units().len(),
                content.talents().len()
            ),
            (61, 219, 189)
        );
        let skills = content
            .heroes()
            .flat_map(|h| h.abilities.iter())
            .collect::<Vec<_>>();
        assert_eq!(skills.len(), 277);
        assert_eq!(
            skills
                .iter()
                .map(|a| a.damage_variables.len())
                .sum::<usize>(),
            180
        );
        assert_eq!(
            skills.iter().map(|a| a.variables.len()).sum::<usize>(),
            1599
        );
        assert_eq!(
            content
                .heroes()
                .map(|h| h.extra_variables.len())
                .sum::<usize>(),
            19
        );
        assert_eq!(
            data.ability("Ringo", "Ability__Ringo__B")
                .unwrap()
                .targeting(),
            Some(TargetType::SelfBuff)
        );
        assert_eq!(
            data.full_unit("5v5_Minion_Melee").unwrap().alt_damage_264,
            20.
        );
        let shot = data.talent_definition("Talent_Ringo_LongShot").unwrap();
        assert_eq!(
            shot.modifier("BONUS_RANGE")
                .unwrap()
                .value_at(NonZeroU8::new(5).unwrap(), 0., 0.),
            Some(4.)
        );
    }
}
