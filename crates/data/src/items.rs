//! Operator equipment definitions and ordered native store catalogs.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha256;
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::Path,
};

use halcyon_protocol::ce::NativeMode;

/// Gameplay settings records with direct store references used by native queues.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub enum StoreSettings {
    #[serde(rename = "Standard_HF")]
    HalcyonFold,
    #[serde(rename = "Standard_5v5")]
    Five,
    #[serde(rename = "Practice_5v5")]
    Practice,
    #[serde(rename = "Blitz_3v3_Revamp")]
    BlitzRevamp,
}
impl StoreSettings {
    pub const fn key(self) -> &'static str {
        match self {
            Self::HalcyonFold => "Standard_HF",
            Self::Five => "Standard_5v5",
            Self::Practice => "Practice_5v5",
            Self::BlitzRevamp => "Blitz_3v3_Revamp",
        }
    }
    pub const fn for_mode(mode: NativeMode) -> Option<Self> {
        match mode {
            NativeMode::Aral => None,
            NativeMode::RankedFold => Some(Self::HalcyonFold),
            NativeMode::Practice => Some(Self::Practice),
            NativeMode::CasualFive
            | NativeMode::RankedFive
            | NativeMode::SoloBots
            | NativeMode::CoopBots
            | NativeMode::Private
            | NativeMode::PrivateDraft
            | NativeMode::PrivateBots => Some(Self::Five),
        }
    }
}

#[derive(Clone, Copy)]
pub enum ItemTuning {
    Move,
    Sprint,
    SprintDuration,
    Duration,
    Barrier,
    HealAmount,
    EnergyPercent,
}
impl ItemTuning {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Move => "MOVE",
            Self::Sprint => "SPRINT",
            Self::SprintDuration => "SPRINT_DURATION",
            Self::Duration => "DURATION",
            Self::Barrier => "BARRIER",
            Self::HealAmount => "HEAL_AMOUNT",
            Self::EnergyPercent => "ENERGY_PERCENT",
        }
    }
}

#[derive(Clone, Copy)]
pub enum ItemActiveParameter {
    Cooldown,
}
impl ItemActiveParameter {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Cooldown => "Cooldown",
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ItemStore {
    Blitz3v3,
    Standard3v3,
    Standard5v5,
    Tutorial2,
    Tutorial3,
}
impl ItemStore {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Blitz3v3 => "ItemStore_Blitz_3v3_Revamp",
            Self::Standard3v3 => "ItemStore_Standard_3v3",
            Self::Standard5v5 => "ItemStore_Standard_5v5",
            Self::Tutorial2 => "ItemStore_Tutorial2",
            Self::Tutorial3 => "ItemStore_Tutorial3",
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FullItem {
    pub id: String,
    pub name_en: String,
    pub name_zh: String,
    pub name_key: String,
    pub desc_en: String,
    pub desc_zh: String,
    pub icon: String,
    pub short_desc_en: Option<String>,
    pub tier: u8,
    pub cost_a: Option<u32>,
    pub cost_b: Option<u32>,
    pub bonuses: Vec<ItemBonus>,
    pub active_variables: Vec<ItemActiveVariable>,
    pub passive_variables: Vec<ItemPassiveVariable>,
    pub components: Vec<String>,
    pub buyable_in_stores: Vec<String>,
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct ItemBonus {
    pub stat_id: u8,
    pub layer: u8,
    pub field: u8,
    pub stat: Option<String>,
    pub value: f32,
    pub field_name: String,
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct ItemActiveVariable {
    pub name: String,
    pub unit_type: String,
    pub stat_label: Option<String>,
    pub base: f32,
    pub per_rank: f32,
    pub overdrive: f32,
    pub cp_ratio: f32,
    pub wp_ratio: f32,
    pub alt_ratio: f32,
    pub bonus_as_ratio: f32,
    pub clamp: f32,
    pub stat_label_id: u32,
    pub off: u32,
    pub meta_raw: [u32; 4],
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct ItemPassiveVariable {
    pub label: String,
    pub value: f32,
    pub extra_floats: Vec<f32>,
    pub off: u32,
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

#[derive(Debug)]
pub struct ItemCatalog {
    items: BTreeMap<String, FullItem>,
    stores: BTreeMap<String, Vec<String>>,
    standard_five: BTreeSet<String>,
}
impl ItemCatalog {
    pub(crate) fn load(dir: &Path, hash: &mut Sha256) -> io::Result<Self> {
        let rows: Vec<FullItem> = super::read(dir, "items_full.json", hash)?;
        let stores: BTreeMap<String, Vec<String>> = super::read(dir, "item_stores.json", hash)?;
        let mut items = BTreeMap::new();
        for row in rows {
            if items.insert(row.id.clone(), row).is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "duplicate item ID",
                ));
            }
        }
        let standard_five = stores
            .get(ItemStore::Standard5v5.key())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "missing standard 5v5 store")
            })?
            .iter()
            .cloned()
            .collect();
        Ok(Self {
            items,
            stores,
            standard_five,
        })
    }
    pub fn items(&self) -> &BTreeMap<String, FullItem> {
        &self.items
    }
    pub fn item(&self, id: &str) -> Option<&FullItem> {
        self.items.get(id)
    }
    pub fn stores(&self) -> &BTreeMap<String, Vec<String>> {
        &self.stores
    }
    pub fn store(&self, store: ItemStore) -> Option<&[String]> {
        self.stores.get(store.key()).map(Vec::as_slice)
    }
    pub(crate) fn standard_five(&self) -> &BTreeSet<String> {
        &self.standard_five
    }
}

impl super::GameData {
    /// Resolve the store reference in an original gameplay settings record.
    /// Ambiguous references yield no selection.
    pub fn store_for_settings(&self, settings: &str) -> Option<&[String]> {
        let catalog = self.item_catalog()?;
        let settings = self.native_maps()?.modes_and_camps.get(settings)?;
        let mut matches = settings
            .entity_refs
            .iter()
            .filter_map(|name| catalog.stores.get(name));
        let first = matches.next()?;
        matches.next().is_none().then_some(first.as_slice())
    }
}
impl FullItem {
    /// An unambiguous scalar from the passive/tuning list.
    pub fn passive_value(&self, label: &str) -> Option<f32> {
        let mut rows = self.passive_variables.iter().filter(|v| v.label == label);
        let first = rows.next()?;
        (rows.next().is_none() && first.value.is_finite()).then_some(first.value)
    }
    pub fn fixed_active_seconds(&self, name: &str) -> Option<f32> {
        let mut rows = self.active_variables.iter().filter(|row| row.name == name);
        let v = rows.next()?;
        (rows.next().is_none()
            && v.unit_type == "seconds"
            && [
                v.per_rank,
                v.overdrive,
                v.cp_ratio,
                v.wp_ratio,
                v.alt_ratio,
                v.bonus_as_ratio,
                v.clamp,
            ]
            .iter()
            .all(|x| *x == 0.)
            && v.base.is_finite()
            && v.base > 0.)
            .then_some(v.base)
    }
}
