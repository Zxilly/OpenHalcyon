//! Verified item-stat channels omitted by the original flat-only delivery.
use serde::Deserialize;
use sha2::Sha256;
use std::{collections::BTreeMap, io, path::Path};

#[derive(Deserialize)]
struct Item {
    id: String,
    bonuses: Vec<Bonus>,
}

#[derive(Deserialize)]
struct Bonus {
    stat_id: u8,
    stat: Option<String>,
    value: f32,
    layer: u8,
    field: u8,
}

impl Bonus {
    // These five layer-zero channels use the same units in the supplement,
    // the original shop modifiers, and the simulation stat accumulator.
    fn supported(&self) -> bool {
        self.layer == 0
            && matches!(
                (self.stat_id, self.field, self.stat.as_deref()),
                (3, 3, Some("ENERGY_REGEN"))
                    | (15, 2, Some("ATTACK_SPEED_DISPLAY"))
                    | (25, 2, Some("COOLDOWN_SPEED"))
                    | (30, 2, Some("CRIT_CHANCE"))
                    | (31, 2, Some("CRIT_DAMAGE"))
            )
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
          {"stat_id":30,"stat":"UNRESOLVED","field":2,"layer":0,"value":0.20}
        ]}]"#,
        )
        .unwrap();
        let values = ItemBonuses::from_rows(rows).unwrap();
        assert_eq!(values.get("test", 25), Some(0.15));
        for id in [15, 3, 30] {
            assert_eq!(values.get("test", id), None);
        }
    }
}
