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
    /// Case-sensitive operand found in this ability's own English tooltip.
    #[serde(skip_deserializing, skip_serializing_if = "Option::is_none")]
    pub tooltip_operand: Option<String>,
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

/// Canonical rarity tier for hero-bound talents in `talents_full.json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TalentRarity {
    Rare,
    Epic,
    Legendary,
}

impl TalentRarity {
    pub const ALL: [Self; 3] = [Self::Rare, Self::Epic, Self::Legendary];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rare => "rare",
            Self::Epic => "epic",
            Self::Legendary => "legendary",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "rare" => Some(Self::Rare),
            "epic" => Some(Self::Epic),
            "legendary" => Some(Self::Legendary),
            _ => None,
        }
    }

    pub const fn index(self) -> usize {
        self as usize
    }
}

impl Talent {
    pub fn rarity_kind(&self) -> Option<TalentRarity> {
        TalentRarity::parse(self.rarity.as_deref()?)
    }
}

#[derive(Debug, Default)]
pub struct CombatContent {
    heroes: BTreeMap<String, HeroAbilities>,
    units: BTreeMap<String, FullUnit>,
    talents: BTreeMap<String, Talent>,
    talents_by_official_id: BTreeMap<String, String>,
    talents_by_hero: BTreeMap<String, [Option<String>; 3]>,
}

impl CombatContent {
    pub(super) fn resolve_tooltip_operands(&mut self, references: &crate::reference::References) {
        for ability in self
            .heroes
            .values_mut()
            .flat_map(|hero| &mut hero.abilities)
        {
            if ability.damage_variables.is_empty() {
                continue;
            }
            let dictionary = |key: Option<&str>| references.translate("en", key?).map(|t| t.value);
            let sources = [
                ability.short_desc_en.as_deref(),
                dictionary(ability.short_desc_key.as_deref()),
                dictionary(ability.desc_key.as_deref()),
            ];
            let mut operands = BTreeMap::new();
            for text in sources.into_iter().flatten() {
                for bracket in text.split('[').skip(1) {
                    let Some((token, _)) = bracket.split_once(']') else {
                        continue;
                    };
                    let operand = token.split('|').next().unwrap_or_default();
                    if operand.is_empty() {
                        continue;
                    }
                    operands
                        .entry(operand_hash(operand))
                        .and_modify(|name| {
                            if *name != Some(operand) {
                                *name = None;
                            }
                        })
                        .or_insert(Some(operand));
                }
            }
            for block in &mut ability.damage_variables {
                block.tooltip_operand = operands
                    .get(&block.header_u32)
                    .copied()
                    .flatten()
                    .map(str::to_owned);
            }
        }
    }
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
        let talents = indexed(
            crate::read(dir, "talents_full.json", hash)?,
            |t: &Talent| &t.id,
        )?;
        let mut talents_by_official_id = BTreeMap::new();
        let mut talents_by_hero: BTreeMap<String, [Option<String>; 3]> = BTreeMap::new();
        for talent in talents.values() {
            if let Some(official_id) = talent.official_id.as_deref()
                && (official_id.is_empty()
                    || talents_by_official_id
                        .insert(official_id.to_owned(), talent.id.clone())
                        .is_some())
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("empty or duplicate talent official_id {official_id}"),
                ));
            }
            match (talent.hero.as_deref(), talent.rarity.as_deref()) {
                (Some(hero), Some(rarity)) => {
                    let rarity = TalentRarity::parse(rarity).ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("unknown talent rarity {rarity} on {}", talent.id),
                        )
                    })?;
                    let slot =
                        &mut talents_by_hero.entry(hero.to_owned()).or_default()[rarity.index()];
                    if slot.is_some() {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("duplicate {rarity:?} talent for hero {hero}"),
                        ));
                    }
                    *slot = Some(talent.id.clone());
                }
                (None, None) => {}
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("inconsistent hero/rarity metadata on talent {}", talent.id),
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
            talents,
            talents_by_official_id,
            talents_by_hero,
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
    pub fn hero(&self, hero: &str) -> Option<&HeroAbilities> {
        self.heroes.get(hero)
    }
    pub fn hero_for(&self, template: crate::EntityTemplate) -> Option<&HeroAbilities> {
        self.heroes.get(template.hero_id()?)
    }
    pub fn ability(&self, hero: &str, id: &str) -> Option<&Ability> {
        self.heroes
            .get(hero)?
            .abilities
            .iter()
            .find(|a| a.ability_id == id)
    }
    pub fn ability_for_slot(
        &self,
        template: crate::EntityTemplate,
        slot: crate::AbilitySlot,
    ) -> Option<&Ability> {
        let slots = crate::lookup_hero_ability_slots(template)?;
        self.ability(slots.hero_id, slots.ability_id(slot))
    }
    pub fn ability_for_form(
        &self,
        template: crate::EntityTemplate,
        slot: crate::AbilitySlot,
        form: crate::AbilityForm,
    ) -> Option<&Ability> {
        let slots = crate::lookup_hero_ability_slots(template)?;
        self.ability(slots.hero_id, slots.ability_id_for_form(slot, form)?)
    }
    pub fn talent(&self, id: &str) -> Option<&Talent> {
        self.talents.get(id)
    }
    pub fn talent_by_official_id(&self, official_id: &str) -> Option<&Talent> {
        let id = self.talents_by_official_id.get(official_id)?;
        self.talents.get(id)
    }
    pub fn hero_talent(&self, hero: &str, rarity: TalentRarity) -> Option<&Talent> {
        let id = self.talents_by_hero.get(hero)?[rarity.index()].as_deref()?;
        self.talents.get(id)
    }
    pub fn hero_talent_for(
        &self,
        template: crate::EntityTemplate,
        rarity: TalentRarity,
    ) -> Option<&Talent> {
        self.hero_talent(template.hero_id()?, rarity)
    }
    pub fn hero_talents(&self, hero: &str) -> [Option<&Talent>; 3] {
        let Some(ids) = self.talents_by_hero.get(hero) else {
            return [None, None, None];
        };
        std::array::from_fn(|i| ids[i].as_deref().and_then(|id| self.talents.get(id)))
    }
    pub fn hero_talents_for(&self, template: crate::EntityTemplate) -> [Option<&Talent>; 3] {
        template
            .hero_id()
            .map_or([None, None, None], |hero| self.hero_talents(hero))
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
    pub fn combat_content(&self) -> &CombatContent {
        self.extra.combat_content()
    }
    pub fn hero_abilities(&self, name: &str) -> Option<&HeroAbilities> {
        self.extra.combat_content().hero(name)
    }
    pub fn hero_abilities_for(&self, template: crate::EntityTemplate) -> Option<&HeroAbilities> {
        self.extra.combat_content().hero_for(template)
    }
    pub fn ability(&self, hero: &str, id: &str) -> Option<&Ability> {
        self.extra.combat_content().ability(hero, id)
    }
    pub fn ability_for_slot(
        &self,
        template: crate::EntityTemplate,
        slot: crate::AbilitySlot,
    ) -> Option<&Ability> {
        self.extra.combat_content().ability_for_slot(template, slot)
    }
    pub fn ability_for_form(
        &self,
        template: crate::EntityTemplate,
        slot: crate::AbilitySlot,
        form: crate::AbilityForm,
    ) -> Option<&Ability> {
        self.extra
            .combat_content()
            .ability_for_form(template, slot, form)
    }
    pub fn ability_ranks_for_slot(
        &self,
        template: crate::EntityTemplate,
        slot: crate::AbilitySlot,
        variable: &str,
    ) -> Option<&[f32]> {
        self.ability_for_slot(template, slot)?
            .variable(variable)?
            .ranks
            .as_deref()
    }
    pub fn full_unit(&self, name: &str) -> Option<&FullUnit> {
        self.extra.combat_content().units.get(name)
    }
    pub fn talent_definition(&self, id: &str) -> Option<&Talent> {
        self.extra.combat_content().talent(id)
    }
    pub fn talent_by_official_id(&self, official_id: &str) -> Option<&Talent> {
        self.extra
            .combat_content()
            .talent_by_official_id(official_id)
    }
    pub fn hero_talent(&self, hero: &str, rarity: TalentRarity) -> Option<&Talent> {
        self.extra.combat_content().hero_talent(hero, rarity)
    }
    pub fn hero_talent_for(
        &self,
        template: crate::EntityTemplate,
        rarity: TalentRarity,
    ) -> Option<&Talent> {
        self.extra
            .combat_content()
            .hero_talent_for(template, rarity)
    }
    pub fn hero_talents(&self, hero: &str) -> [Option<&Talent>; 3] {
        self.extra.combat_content().hero_talents(hero)
    }
    pub fn hero_talents_for(&self, template: crate::EntityTemplate) -> [Option<&Talent>; 3] {
        self.extra.combat_content().hero_talents_for(template)
    }
    pub fn talents(&self) -> impl ExactSizeIterator<Item = &Talent> {
        self.extra.combat_content().talents()
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
        let header = operand_hash(operand);
        let mut found = self
            .damage_variables
            .iter()
            .filter(|v| v.header_u32 == header);
        let first = found.next()?;
        found.next().is_none().then_some(first)
    }
}

fn operand_hash(label: &str) -> u32 {
    label.bytes().fold(0x811c9dc5_u32, |h, byte| {
        (h ^ u32::from(byte)).wrapping_mul(0x01000193)
    })
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
    fn indexed_talent_and_slot_lookups_work_without_supplement() {
        let mut content = CombatContent::default();
        content.heroes.insert(
            "Hero009".into(),
            HeroAbilities {
                hero_id: "Hero009".into(),
                abilities: vec![
                    Ability {
                        ability_id: "Ability__Krul__A".into(),
                        variables: vec![AbilityVariable {
                            name: "Cooldown".into(),
                            ranks: Some(vec![9., 8.5, 8., 7.5, 7.]),
                            ..Default::default()
                        }],
                        ..Default::default()
                    },
                    Ability {
                        ability_id: "Ability__Krul__B".into(),
                        ..Default::default()
                    },
                    Ability {
                        ability_id: "Ability__Krul__C".into(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
        );
        content.talents.insert(
            "Talent_Krul_DeadMansBarrier".into(),
            Talent {
                id: "Talent_Krul_DeadMansBarrier".into(),
                official_id: Some("krul_rare".into()),
                hero: Some("Hero009".into()),
                rarity: Some("rare".into()),
                ..Default::default()
            },
        );
        content
            .talents_by_official_id
            .insert("krul_rare".into(), "Talent_Krul_DeadMansBarrier".into());
        content.talents_by_hero.insert(
            "Hero009".into(),
            [Some("Talent_Krul_DeadMansBarrier".into()), None, None],
        );

        assert_eq!(
            content
                .ability_for_slot(crate::EntityTemplate::Hero009, crate::AbilitySlot::A)
                .unwrap()
                .ability_id,
            "Ability__Krul__A"
        );
        assert_eq!(
            content
                .ability_for_slot(crate::EntityTemplate::Hero009, crate::AbilitySlot::A)
                .unwrap()
                .variable("Cooldown")
                .unwrap()
                .ranks
                .as_deref(),
            Some([9., 8.5, 8., 7.5, 7.].as_slice())
        );
        assert_eq!(
            content
                .talent_by_official_id("krul_rare")
                .unwrap()
                .rarity_kind(),
            Some(TalentRarity::Rare)
        );
        assert_eq!(
            content
                .hero_talent_for(crate::EntityTemplate::Hero009, TalentRarity::Rare)
                .unwrap()
                .id,
            "Talent_Krul_DeadMansBarrier"
        );
        assert!(
            content
                .hero_talent_for(crate::EntityTemplate::Hero009, TalentRarity::Epic)
                .is_none()
        );
        assert_eq!(
            TalentRarity::parse("legendary"),
            Some(TalentRarity::Legendary)
        );
        assert_eq!(TalentRarity::parse("mythic"), None);
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
                .flat_map(|a| &a.damage_variables)
                .filter(|block| block.tooltip_operand.is_some())
                .count(),
            154
        );
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
        assert_eq!(
            data.talent_by_official_id("ringo_rare").unwrap().id,
            "Talent_Ringo_LongShot"
        );
        assert_eq!(
            data.hero_talent_for(crate::EntityTemplate::Ringo, TalentRarity::Rare)
                .unwrap()
                .id,
            "Talent_Ringo_LongShot"
        );
        assert_eq!(
            data.hero_talents_for(crate::EntityTemplate::Idris)
                .into_iter()
                .flatten()
                .count(),
            3
        );
        assert_eq!(data.talents().len(), 189);
        assert_eq!(
            data.ability_for_slot(crate::EntityTemplate::Catherine, crate::AbilitySlot::A)
                .unwrap()
                .ability_id,
            "Ability__Catherine__A"
        );
        assert_eq!(
            data.ability_for_form(
                crate::EntityTemplate::Malene,
                crate::AbilitySlot::B,
                crate::AbilityForm::Alternate
            )
            .unwrap()
            .ability_id,
            "Ability__Malene__B2"
        );
        assert!(
            data.ability_ranks_for_slot(
                crate::EntityTemplate::Idris,
                crate::AbilitySlot::B,
                "Cooldown"
            )
            .is_some()
        );
    }
}
