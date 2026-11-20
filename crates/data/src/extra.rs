//! Startup indexes for the separately distributed CE data supplement.
use sha2::Sha256;
use std::{collections::BTreeSet, io, path::Path};

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

#[derive(Default, Debug)]
pub struct ExtraData {
    pub(crate) references: Option<crate::reference::References>,
    pub(crate) native_maps: Option<crate::NativeMaps>,
    pub(crate) solo_lane_rewards: Option<crate::SoloLaneRewards>,
    pub(crate) item_bonuses: crate::item_bonuses::ItemBonuses,
    pub(crate) skins: Option<crate::skins::SkinIndex>,
    combat: crate::combat_content::CombatContent,
    pub(crate) items: Option<crate::items::ItemCatalog>,
}

impl ExtraData {
    pub(super) fn load(
        dir: &Path,
        hash: &mut Sha256,
        catalog: &mut Option<crate::DraftCatalog>,
    ) -> io::Result<Self> {
        let references = crate::reference::References::load(dir)?;
        let mut combat = crate::combat_content::CombatContent::load(dir, hash)?;
        combat.resolve_tooltip_operands(&references);
        let items = crate::items::ItemCatalog::load(dir, hash)?;
        Ok(Self {
            references: Some(references),
            native_maps: dir
                .join("maps/navmesh_index.json")
                .try_exists()?
                .then(|| crate::NativeMaps::load(dir, hash))
                .transpose()?,
            solo_lane_rewards: Some(crate::SoloLaneRewards::from_rows(combat.units())?),
            item_bonuses: crate::item_bonuses::ItemBonuses::from_catalog(&items)?,
            skins: Some(crate::skins::SkinIndex::load(dir, hash, catalog)?),
            combat,
            items: Some(items),
        })
    }

    pub(super) fn ranks(&self, hero: &str, ability: &str, name: &str) -> Option<&[f32]> {
        self.combat
            .ability(hero, ability)?
            .variable(name)?
            .ranks
            .as_deref()
    }
    pub fn combat_content(&self) -> &crate::combat_content::CombatContent {
        &self.combat
    }

    pub(super) fn standard_five(&self) -> Option<&BTreeSet<String>> {
        self.items.as_ref().map(|i| i.standard_five())
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn ambiguous_labels_do_not_select_an_arbitrary_rank_table() {
        use crate::combat_content::{Ability, AbilityVariable};
        let variable = |name: &str, first| AbilityVariable {
            name: name.into(),
            ranks: Some(vec![first, first + 1.]),
            ..Default::default()
        };
        let ability = Ability {
            variables: vec![
                variable("Range", 12.),
                variable("Energy Cost", 30.),
                variable("ENERGY COST", 45.),
            ],
            ..Default::default()
        };
        assert_eq!(
            ability.variable("range").unwrap().ranks.as_deref(),
            Some([12., 13.].as_slice())
        );
        assert!(ability.variable("energy cost").is_none());
    }
}
