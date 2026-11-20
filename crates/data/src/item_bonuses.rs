//! Verified item-stat channels omitted by the original flat-only delivery.
use crate::items::CffItemStat;
use serde::Deserialize;

use std::{collections::BTreeMap, io};

// The text and numeric ID are separate fields in the delivery. Checking both
// prevents a new or mislabeled stat from entering the accumulator.
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum StatChannel {
    EnergyRegen,
    AttackSpeedDisplay,
    CooldownSpeed,
    CritChance,
    CritDamage,
    #[serde(other)]
    Unknown,
}

#[repr(u8)]
enum ScalarField {
    Amplifier = 2,
    Percent = 3,
}

impl StatChannel {
    fn column(self) -> Option<CffItemStat> {
        Some(match self {
            Self::EnergyRegen => CffItemStat::EnergyRegen,
            Self::AttackSpeedDisplay => CffItemStat::AttackSpeedDisplay,
            Self::CooldownSpeed => CffItemStat::CooldownSpeed,
            Self::CritChance => CffItemStat::CritChance,
            Self::CritDamage => CffItemStat::CritDamage,
            Self::Unknown => return None,
        })
    }
    fn scalar(self) -> Option<ScalarField> {
        match self {
            Self::EnergyRegen => Some(ScalarField::Percent),
            Self::AttackSpeedDisplay
            | Self::CooldownSpeed
            | Self::CritChance
            | Self::CritDamage => Some(ScalarField::Amplifier),
            Self::Unknown => None,
        }
    }
}

fn supported(bonus: &crate::items::ItemBonus) -> bool {
    const BASE_LAYER: u8 = 0;
    if bonus.layer != BASE_LAYER {
        return false;
    }
    let Some(name) = bonus.stat.as_deref() else {
        return false;
    };
    let text = serde::de::value::StrDeserializer::<serde::de::value::Error>::new(name);
    let Ok(channel) = StatChannel::deserialize(text) else {
        return false;
    };
    channel
        .column()
        .is_some_and(|column| column as u8 == bonus.stat_id)
        && channel.scalar().is_some_and(|f| f as u8 == bonus.field)
}

#[derive(Default, Debug)]
pub(crate) struct ItemBonuses(BTreeMap<String, BTreeMap<u8, f32>>);

impl ItemBonuses {
    pub(crate) fn from_catalog(catalog: &crate::items::ItemCatalog) -> io::Result<Self> {
        let mut by_item = BTreeMap::new();
        for item in catalog.items().values() {
            let mut stats = BTreeMap::new();
            for bonus in item.bonuses.iter().filter(|b| supported(b)) {
                if !bonus.value.is_finite() || bonus.value <= 0. {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid supplemental item bonus",
                    ));
                }
                if stats.insert(bonus.stat_id, bonus.value).is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "duplicate supplemental item stat",
                    ));
                }
            }
            if !stats.is_empty() {
                by_item.insert(item.id.clone(), stats);
            }
        }
        Ok(Self(by_item))
    }
    pub(crate) fn get(&self, item: &str, stat: u8) -> Option<f32> {
        self.0.get(item)?.get(&stat).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_channels_require_their_named_stat_layer_and_field() {
        let rows: Vec<crate::items::ItemBonus> = serde_json::from_str(
            r#"[
          {"stat_id":25,"stat":"COOLDOWN_SPEED","field":2,"field_name":"synthetic","layer":0,"value":0.15},
          {"stat_id":15,"stat":"ATTACK_SPEED_DISPLAY","field":1,"field_name":"synthetic","layer":0,"value":0.10},
          {"stat_id":3,"stat":"ENERGY_REGEN","field":3,"field_name":"synthetic","layer":1,"value":0.25},
          {"stat_id":30,"stat":"UNRESOLVED","field":2,"field_name":"synthetic","layer":0,"value":0.20},
          {"stat_id":31,"stat":"CRIT_DAMAGE","field":253,"field_name":"synthetic","layer":0,"value":0.20},
          {"stat_id":255,"stat":"CRIT_CHANCE","field":2,"field_name":"synthetic","layer":0,"value":0.20},
          {"stat_id":32,"stat":null,"field":2,"field_name":"synthetic","layer":0,"value":0.20}
        ]"#,
        )
        .unwrap();
        assert_eq!(
            rows.iter()
                .filter(|b| supported(b))
                .map(|b| b.stat_id)
                .collect::<Vec<_>>(),
            [25]
        );
    }
}
