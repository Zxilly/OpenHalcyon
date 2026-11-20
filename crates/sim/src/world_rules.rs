use super::*;
use std::sync::{Arc, LazyLock};

/// Compiled once per installation. Rooms share numeric tables and keep their own store selection.
pub struct WorldRules {
    pub(super) stats: [Stats; 8],
    pub(super) aral_units: aral::Units,
    pub(super) solo_inner_towers: [Stats; 2],
    pub(super) solo_lane_rewards: Option<halcyon_data::SoloLaneRewards>,
    pub(super) first_wave: u64,
    pub(super) wave_interval: u64,
    pub(super) abilities: Arc<AbilityBook>,
    pub(super) ability_intents: Arc<ai::utility::HeroIntentCatalog>,
    pub(super) shop: shop::Shop,
}
pub(super) static BUILTIN: LazyLock<WorldRules> = LazyLock::new(|| {
    // Confirmed delivery base numbers, usable without distributing extracted assets.
    let stats = [
        Stats::new(EntityTemplate::Ringo, 703., 74., 25., 6.2, 3.6),
        Stats::new(EntityTemplate::OuterTurret5v5, 3000., 190., 100., 9., 0.),
        Stats::new(EntityTemplate::VainCrystalHome5v5, 20000., 0., 0., 22., 0.),
        Stats::new(EntityTemplate::VainCrystalAway5v5, 20000., 0., 0., 22., 0.),
        Stats::new(EntityTemplate::MinionMelee5v5, 575., 13., 0., 1.3, 3.5),
        Stats::new(EntityTemplate::MinionRange5v5, 475., 15., 0., 4.5, 3.5),
        Stats::new(EntityTemplate::MinionSiege5v5, 880., 15., 0., 4., 3.5),
        Stats::new(EntityTemplate::MinionCaptain5v5, 950., 15., 0., 5., 3.5),
    ];

    WorldRules {
        stats,
        aral_units: Default::default(),
        solo_inner_towers: layout::solo_inner_towers(),
        solo_lane_rewards: None,
        first_wave: 900,
        wave_interval: 600,
        abilities: Default::default(),
        ability_intents: Default::default(),
        shop: Default::default(),
    }
});
impl WorldRules {
    pub fn load(data: &GameData) -> Result<Self, String> {
        let units = NAMES.map(|name| {
            let unit = data
                .unit(name)
                .ok_or_else(|| format!("missing unit {name}"))?;
            let id = data
                .type_id(name)
                .ok_or_else(|| format!("missing manifest entry {name}"))?;
            Ok::<_, String>(Stats::load(id, unit, data))
        });
        let stats: Vec<_> = units.into_iter().collect::<Result<_, _>>()?;
        let timing = |key: &str| {
            data.official_rules["five_v_five"]["waves"][key]
                .as_u64()
                .and_then(|v| v.checked_mul(TICK_HZ))
                .filter(|n| *n > 0)
                .ok_or_else(|| format!("invalid wave timing {key}"))
        };

        let mut aral_units = aral::Units::default();
        aral_units.load(data);
        let mut solo_inner_towers = layout::solo_inner_towers();
        for (stats, name) in solo_inner_towers
            .iter_mut()
            .zip(["Turret5v5", "ArmoryTurret5v5"])
        {
            if let Some(unit) = data.unit(name) {
                *stats = Stats::load(stats.id, unit, data);
            }
        }
        Ok(Self {
            stats: stats.try_into().expect("eight unit types"),
            first_wave: timing("first_spawn_s")?,
            wave_interval: timing("interval_s")?,
            aral_units,
            solo_inner_towers,
            solo_lane_rewards: data.solo_lane_rewards(),
            abilities: Arc::new(AbilityBook::from_data(data)?),
            ability_intents: Arc::new(ai::utility::HeroIntentCatalog::from_data(data)),
            shop: shop::Shop::load(data)?,
        })
    }
    pub fn world(&self, seed: u64, human_seats: u8) -> World {
        World::build(seed, human_seats, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rooms_share_rules_and_keep_independent_shop_selections() {
        let mut first = BUILTIN.world(1, 6);
        let second = BUILTIN.world(1, 6);
        assert!(Arc::ptr_eq(&first.abilities, &second.abilities));
        assert!(first.shop.shares_offers_with(&second.shop));
        assert_eq!(first.state_digest(), second.state_digest());
        first.use_item_store(&[]);
        assert_ne!(first.state_digest(), second.state_digest());
        assert!(first.shop.shares_offers_with(&second.shop));
    }
}
