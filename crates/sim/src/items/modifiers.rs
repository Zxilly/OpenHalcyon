//! Typed native attribute selectors, catalog sources, and authoritative deltas.
use crate::BasicItem;
use halcyon_data::items::{CffItemStat, ItemCatalog, ItemTuning};
use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(into = "u8")]
#[repr(u8)]
pub enum NativeItemStat {
    Health = 0,
    Energy = 2,
    EnergyRegen = 3,
    WeaponPower = 4,
    CrystalPower = 5,
    Armor = 7,
    Shield = 8,
    ArmorPierce = 9,
    ShieldPierce = 11,
    AttackSpeed = 15,
    MoveSpeedAmplifier = 18,
    CooldownSpeed = 25,
    CritChance = 30,
    CritDamage = 31,
}
impl From<NativeItemStat> for u8 {
    fn from(s: NativeItemStat) -> Self {
        s as u8
    }
}
impl NativeItemStat {
    pub const fn layer(self) -> u8 {
        if matches!(self, Self::MoveSpeedAmplifier) {
            2
        } else {
            1
        }
    }
    pub const fn restore_after_spawn(self) -> bool {
        !matches!(
            self,
            Self::Health | Self::Energy | Self::WeaponPower | Self::Armor | Self::Shield
        )
    }
    pub(crate) const fn catalog_column(self) -> Option<CffItemStat> {
        Some(match self {
            Self::Health => CffItemStat::Health,
            Self::Energy => CffItemStat::Energy,
            Self::EnergyRegen => CffItemStat::EnergyRegen,
            Self::WeaponPower => CffItemStat::WeaponPower,
            Self::CrystalPower => CffItemStat::CrystalPower,
            Self::Armor => CffItemStat::Armor,
            Self::Shield => CffItemStat::Shield,
            Self::AttackSpeed => CffItemStat::AttackSpeedDisplay,
            Self::CooldownSpeed => CffItemStat::CooldownSpeed,
            Self::CritChance => CffItemStat::CritChance,
            Self::CritDamage => CffItemStat::CritDamage,
            Self::ArmorPierce | Self::ShieldPierce | Self::MoveSpeedAmplifier => return None,
        })
    }
}

pub(crate) fn passive_operand(
    catalog: &ItemCatalog,
    item: BasicItem,
    stat: NativeItemStat,
    second: bool,
) -> Result<Option<f32>, String> {
    use BasicItem::*;
    let (key, component) = match (item, stat) {
        (Pulseweave, NativeItemStat::MoveSpeedAmplifier) => (ItemTuning::BonusBaseMoveSpeed, None),
        (PiercingSpear, NativeItemStat::ArmorPierce) => (ItemTuning::ArmorPen, None),
        (PiercingShard, NativeItemStat::ShieldPierce) => (ItemTuning::ShieldPierce, None),
        (Bonesaw, NativeItemStat::ArmorPierce) => (ItemTuning::ArmorPen, Some(PiercingSpear)),
        (BrokenMyth, NativeItemStat::ShieldPierce) => {
            (ItemTuning::ShieldPierce, Some(PiercingShard))
        }
        _ => return Ok(None),
    };
    let value = |kind: BasicItem| {
        catalog
            .item(kind.definition().name)?
            .passive_value(key.key())
            .filter(|v| (0.0..=1.0).contains(v))
    };
    let Some(total) = value(item) else {
        return Ok(None);
    };
    let Some(part) = component else {
        return Ok(Some(total));
    };
    let Some(base) = value(part) else {
        return Ok(None);
    };
    if base > total {
        return Err(format!(
            "{} passive exceeds its completed item's total",
            part.definition().name
        ));
    }
    Ok(Some(if second { total - base } else { base }))
}

#[derive(Default)]
pub(crate) struct EffectDelta {
    pub(crate) health: f32,
    pub(crate) energy: f32,
    pub(crate) energy_regen: f32,
    pub(crate) weapon_power: f32,
    pub(crate) crystal_power: f32,
    pub(crate) armor: f32,
    pub(crate) shield: f32,
    pub(crate) shield_pierce: f32,
    pub(crate) attack_speed: f32,
    pub(crate) move_speed_amplifier: f32,
    pub(crate) cooldown_speed: f32,
    pub(crate) crit_chance: f32,
    pub(crate) crit_damage: f32,
}
impl EffectDelta {
    pub(crate) fn add(&mut self, stat: NativeItemStat, value: f32) {
        match stat {
            NativeItemStat::Health => self.health += value,
            NativeItemStat::Energy => self.energy += value,
            NativeItemStat::EnergyRegen => self.energy_regen += value,
            NativeItemStat::WeaponPower => self.weapon_power += value,
            NativeItemStat::CrystalPower => self.crystal_power += value,
            NativeItemStat::Armor => self.armor += value,
            NativeItemStat::Shield => self.shield += value,
            NativeItemStat::ArmorPierce => {} // Recomputed from the strongest held Armorbreaker.
            NativeItemStat::ShieldPierce => self.shield_pierce += value,
            NativeItemStat::AttackSpeed => self.attack_speed += value,
            NativeItemStat::MoveSpeedAmplifier => self.move_speed_amplifier += value,
            NativeItemStat::CooldownSpeed => self.cooldown_speed += value,
            NativeItemStat::CritChance => self.crit_chance += value,
            NativeItemStat::CritDamage => self.crit_damage += value,
        }
    }
}
