//! Verified item-stat channels omitted by the original flat-only delivery.
use serde::Deserialize;
use sha2::Sha256;
use std::{collections::BTreeMap, io, path::Path};

#[derive(Deserialize)]
struct Item {
    id: String,
    bonuses: Vec<Bonus>,
}

// The text and numeric ID are separate fields in the delivery. Checking both
// prevents a new or mislabeled stat from entering the accumulator.
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[repr(u8)]
enum StatChannel {
    EnergyRegen = 3,
    AttackSpeedDisplay = 15,
    CooldownSpeed = 25,
    CritChance = 30,
    CritDamage = 31,
    #[serde(other)]
    Unknown,
}

#[repr(u8)]
enum ScalarField {
    Amplifier = 2,
    Percent = 3,
}

impl StatChannel {
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

#[derive(Deserialize)]
struct Bonus {
    stat_id: u8,
    stat: Option<StatChannel>,
    value: f32,
    layer: u8,
    field: u8,
}

impl Bonus {
    fn supported(&self) -> bool {
        const BASE_LAYER: u8 = 0;
        self.layer == BASE_LAYER
            && self.stat.is_some_and(|channel| {
                channel as u8 == self.stat_id
                    && channel
                        .scalar()
                        .is_some_and(|field| field as u8 == self.field)
            })
    }
}

#[derive(Default, Debug)]
pub(crate) struct ItemBonuses(BTreeMap<String, BTreeMap<u8, f32>>);

impl ItemBonuses {
    pub(crate) fn load(dir: &Path, hash: &mut Sha256) -> io::Result<Self> {
        Self::from_rows(super::read(dir, "items_full.json", hash)?)
    }
    fn from_rows(rows: Vec<Item>) -> io::Result<Self> {
        let mut by_item = BTreeMap::new();
        for item in rows {
            let mut stats = BTreeMap::new();
            for bonus in item.bonuses.into_iter().filter(Bonus::supported) {
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
            if by_item.insert(item.id, stats).is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "duplicate supplemental item ID",
                ));
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
        let rows: Vec<Item> = serde_json::from_str(
            r#"[{"id":"test","bonuses":[
          {"stat_id":25,"stat":"COOLDOWN_SPEED","field":2,"layer":0,"value":0.15},
          {"stat_id":15,"stat":"ATTACK_SPEED_DISPLAY","field":1,"layer":0,"value":0.10},
          {"stat_id":3,"stat":"ENERGY_REGEN","field":3,"layer":1,"value":0.25},
          {"stat_id":30,"stat":"UNRESOLVED","field":2,"layer":0,"value":0.20},
          {"stat_id":31,"stat":"CRIT_DAMAGE","field":253,"layer":0,"value":0.20},
          {"stat_id":255,"stat":"CRIT_CHANCE","field":2,"layer":0,"value":0.20},
          {"stat_id":32,"stat":null,"field":2,"layer":0,"value":0.20}
        ]}]"#,
        )
        .unwrap();
        let values = ItemBonuses::from_rows(rows).unwrap();
        assert_eq!(values.get("test", 25), Some(0.15));
        for id in [15, 3, 30, 31, 32, 255] {
            assert_eq!(values.get("test", id), None);
        }
    }
}
