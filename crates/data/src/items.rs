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

/// CFF0 stat IDs used by equipment. Native 041C attribute selectors use
/// a separate namespace; notably, 18 here means attack range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum CffItemStat {
    Health = 0,
    HealthRegen = 1,
    Energy = 2,
    EnergyRegen = 3,
    WeaponPower = 4,
    CrystalPower = 5,
    Armor = 7,
    Shield = 8,
    ArmorPierce = 9,
    ShieldPierce = 11,
    MoveSpeed = 13,
    AttackSpeed = 14,
    AttackSpeedDisplay = 15,
    AttackRange = 18,
    WeaponVamp = 23,
    CrystalVamp = 24,
    CooldownSpeed = 25,
    CritChance = 30,
    CritDamage = 31,
}

impl CffItemStat {
    /// Layer-zero flat rows from the equipment table. Non-flat channels have
    /// separate field and unit contracts in the supplemental bonus index.
    fn flat_key(self) -> Option<&'static str> {
        Some(match self {
            Self::Health => "HEALTH",
            Self::Energy => "ENERGY",
            Self::WeaponPower => "WEAPON_POWER",
            Self::CrystalPower => "CRYSTAL_POWER",
            Self::Armor => "ARMOR",
            Self::Shield => "SHIELD",
            _ => return None,
        })
    }
}
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
    Lifespan,
    InvisibleDelay,
    TrapArmTime,
    TriggerRadius,
    DetonateRadius,
    DetonateDelay,
    RevealDuration,
    VisionRadius,

    Ratio,
    ProcTime,
    Damage,
    BonusWeaponRatio,
    Vamp,
    OmniVamp,
    HealRanged,
    HealMelee,

    WeaponMin,
    WeaponMax,
    AttackSpeedMin,
    AttackSpeedMax,
    CrystalMin,
    CrystalMax,
    CooldownMin,
    CooldownMax,
    ArmorMin,
    ArmorMax,
    ShieldMin,
    ShieldMax,

    ArmorPen,
    ShieldPierce,
    BonusBaseMoveSpeed,
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
            Self::Lifespan => "LIFESPAN",
            Self::InvisibleDelay => "INVIS_CHARGEUP_TIME",
            Self::TrapArmTime => "CHARGEUP_TIME",
            Self::TriggerRadius => "TRIGGER_RADIUS",
            Self::DetonateRadius => "DETONATE_RADIUS",
            Self::DetonateDelay => "DETONATE_DELAY",
            Self::RevealDuration => "REVEAL_DURATION",
            Self::VisionRadius => "VISION_RADIUS",
            Self::Ratio => "RATIO",
            Self::ProcTime => "TIME",
            Self::Damage => "DAMAGE",
            Self::BonusWeaponRatio => "BONUS_WEAPON_RATIO",
            Self::Vamp => "VAMP",
            Self::OmniVamp => "OMNI_VAMP",
            Self::HealRanged => "HEAL_RANGED",
            Self::HealMelee => "HEAL_MELEE",
            Self::WeaponMin => "WEAPON_MIN",
            Self::WeaponMax => "WEAPON_MAX",
            Self::AttackSpeedMin => "ATTACKSPEED_MIN",
            Self::AttackSpeedMax => "ATTACKSPEED_MAX",
            Self::CrystalMin => "CRYSTAL_MIN",
            Self::CrystalMax => "CRYSTAL_MAX",
            Self::CooldownMin => "CDA_MIN",
            Self::CooldownMax => "CDA_MAX",
            Self::ArmorMin => "ARMOR_MIN",
            Self::ArmorMax => "ARMOR_MAX",
            Self::ShieldMin => "SHIELD_MIN",
            Self::ShieldMax => "SHIELD_MAX",
            Self::ArmorPen => "ARMOR_PEN",
            Self::ShieldPierce => "SHIELD_PIERCE",
            Self::BonusBaseMoveSpeed => "BONUS_BASE_MSPD",
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
    Range,
    Charges,
    Cooldown,
}
impl ItemActiveParameter {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Range => "Range",
            Self::Charges => "Charges",
            Self::Cooldown => "Cooldown",
        }
    }
}
#[derive(Clone, Copy)]
enum ItemParameterUnit {
    Seconds,
    Decimal,
    FlatInt,
}
impl ItemParameterUnit {
    fn key(self) -> &'static str {
        match self {
            Self::Seconds => "seconds",
            Self::Decimal => "decimal",
            Self::FlatInt => "flat_int",
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
    pub fn flat_equipment_stat(&self, column: CffItemStat) -> Option<f32> {
        let key = column.flat_key()?;
        let mut rows = self.bonuses.iter().filter(|b| {
            b.stat_id == column as u8
                && b.stat.as_deref() == Some(key)
                && b.layer == 0
                && b.field == 1
        });
        let row = rows.next()?;
        (rows.next().is_none() && row.value.is_finite() && row.value > 0.).then_some(row.value)
    }
    /// An unambiguous scalar from the passive/tuning list.
    pub fn passive_value(&self, label: &str) -> Option<f32> {
        let mut rows = self.passive_variables.iter().filter(|v| v.label == label);
        let first = rows.next()?;
        (rows.next().is_none() && first.value.is_finite()).then_some(first.value)
    }
    pub fn fixed_active(&self, parameter: ItemActiveParameter) -> Option<f32> {
        let unit = match parameter {
            ItemActiveParameter::Cooldown => ItemParameterUnit::Seconds,
            ItemActiveParameter::Range => ItemParameterUnit::Decimal,
            ItemActiveParameter::Charges => ItemParameterUnit::FlatInt,
        };
        self.fixed_active_value(parameter.key(), unit)
    }
    pub fn fixed_active_seconds(&self, name: &str) -> Option<f32> {
        self.fixed_active_value(name, ItemParameterUnit::Seconds)
    }
    fn fixed_active_value(&self, name: &str, unit: ItemParameterUnit) -> Option<f32> {
        let mut rows = self.active_variables.iter().filter(|row| row.name == name);
        let v = rows.next()?;
        (rows.next().is_none()
            && v.unit_type == unit.key()
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
