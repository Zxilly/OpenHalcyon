//! Startup indexes for the separately distributed CE data supplement.
use super::read;
use serde::Deserialize;
use sha2::Sha256;
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::Path,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbilityParameter {
    Cooldown,
    EnergyCost,
    Range,
}

impl AbilityParameter {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cooldown => "Cooldown",
            Self::EnergyCost => "Energy Cost",
            Self::Range => "Range",
        }
    }
}

#[derive(Debug, Deserialize)]
struct RankedVariable {
    name: String,
    ranks: Box<[f32]>,
}

#[derive(Deserialize)]
struct Ability {
    ability_id: String,
    variables: Vec<RankedVariable>,
}

#[derive(Deserialize)]
struct Hero {
    hero_id: String,
    abilities: Vec<Ability>,
}

#[derive(Default, Debug)]
pub struct ExtraData {
    pub(crate) solo_lane_rewards: Option<crate::SoloLaneRewards>,
    pub(crate) item_bonuses: crate::item_bonuses::ItemBonuses,
    pub(crate) skins: Option<crate::skins::SkinIndex>,
    heroes: BTreeMap<String, BTreeMap<String, Vec<RankedVariable>>>,
    standard_five: Option<BTreeSet<String>>,
}

impl ExtraData {
    pub(super) fn load(
        dir: &Path,
        hash: &mut Sha256,
        hero_names: &[String],
        skin_names: &[String],
    ) -> io::Result<Self> {
        let mut heroes = BTreeMap::new();
        for hero in read::<Vec<Hero>>(dir, "abilities.json", hash)? {
            let mut abilities = BTreeMap::new();
            for ability in hero.abilities {
                if ability.variables.iter().any(|v| {
                    v.name.is_empty()
                        || v.ranks.is_empty()
                        || v.ranks.iter().any(|r| !r.is_finite())
                }) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("invalid ranks for {} in abilities.json", ability.ability_id),
                    ));
                }
                if abilities
                    .insert(ability.ability_id, ability.variables)
                    .is_some()
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "duplicate ability ID for one hero in abilities.json",
                    ));
                }
            }
            if heroes.insert(hero.hero_id, abilities).is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "duplicate hero ID in abilities.json",
                ));
            }
        }
        #[derive(Deserialize)]
        struct Stores {
            #[serde(rename = "ItemStore_Standard_5v5")]
            standard_five: BTreeSet<String>,
        }
        let stores: Stores = read(dir, "item_stores.json", hash)?;
        Ok(Self {
            solo_lane_rewards: Some(crate::SoloLaneRewards::load(dir, hash)?),
            item_bonuses: crate::item_bonuses::ItemBonuses::load(dir, hash)?,
            skins: Some(crate::skins::SkinIndex::load(
                dir, hash, hero_names, skin_names,
            )?),
            heroes,
            standard_five: Some(stores.standard_five),
        })
    }

    pub(super) fn ranks(&self, hero: &str, ability: &str, name: &str) -> Option<&[f32]> {
        let variables = self.heroes.get(hero)?.get(ability)?;
        let mut matches = variables
            .iter()
            .filter(|v| v.name.eq_ignore_ascii_case(name));
        let first = matches.next()?;
        // Some extracted abilities attach two different variables with the same label.
        matches.next().is_none().then_some(first.ranks.as_ref())
    }

    pub(super) fn standard_five(&self) -> Option<&BTreeSet<String>> {
        self.standard_five.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ambiguous_labels_do_not_select_an_arbitrary_rank_table() {
        let variable = |name: &str, first| RankedVariable {
            name: name.into(),
            ranks: Box::new([first, first + 1.]),
        };
        let data = ExtraData {
            heroes: BTreeMap::from([(
                "FixtureHero".into(),
                BTreeMap::from([(
                    "AbilityA".into(),
                    vec![
                        variable("Range", 12.),
                        variable("Energy Cost", 30.),
                        variable("ENERGY COST", 45.),
                    ],
                )]),
            )]),
            ..Default::default()
        };
        assert_eq!(
            data.ranks("FixtureHero", "AbilityA", "range"),
            Some([12., 13.].as_slice())
        );
        assert!(
            data.ranks("FixtureHero", "AbilityA", "energy cost")
                .is_none()
        );
        assert!(data.ranks("FixtureHero", "AbilityB", "Range").is_none());
    }
}
