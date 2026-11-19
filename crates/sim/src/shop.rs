use super::*;
/// Simple base-shop purchases whose single bonus and T1 cost are present in the delivery.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BasicItem {
    WeaponBlade,
    LightArmor,
    LightShield,
    Oakheart,
    EnergyBattery,
}
impl BasicItem {
    const VALUES: [Self; 5] = [
        Self::WeaponBlade,
        Self::LightArmor,
        Self::LightShield,
        Self::Oakheart,
        Self::EnergyBattery,
    ];
    fn descriptor(self) -> (&'static str, u64) {
        match self {
            Self::WeaponBlade => ("Item_WeaponBlade", 4),
            Self::LightArmor => ("Item_LightArmor", 7),
            Self::LightShield => ("Item_LightShield", 8),
            Self::Oakheart => ("Item_Oakheart", 0),
            Self::EnergyBattery => ("Item_EnergyBattery", 2),
        }
    }
}
#[derive(Clone, Copy, Serialize)]
struct Offer {
    price: u32,
    bonus: f32,
}
#[derive(Serialize)]
pub(super) struct Shop([Offer; 5]);
impl Default for Shop {
    fn default() -> Self {
        Self([10., 25., 25., 150., 100.].map(|bonus| Offer { price: 300, bonus }))
    }
}
impl Shop {
    pub fn load(data: &GameData) -> Result<Self, String> {
        let mut shop = Self::default();
        for item in BasicItem::VALUES {
            let (id, stat) = item.descriptor();
            let row = data
                .items
                .iter()
                .find(|v| v.id == id)
                .ok_or_else(|| format!("missing item {id}"))?;
            let price = row
                .cost_a
                .filter(|n| *n > 0)
                .ok_or_else(|| format!("invalid {id} cost_a"))?;
            let bonus = row
                .other
                .get("bonuses")
                .and_then(|v| v.as_array())
                .and_then(|v| v.iter().find(|b| b["stat_id"].as_u64() == Some(stat)))
                .and_then(|v| v["value"].as_f64())
                .filter(|v| *v > 0.)
                .ok_or_else(|| format!("missing item bonus {id}"))? as f32;
            shop.0[item as usize] = Offer { price, bonus };
        }
        Ok(shop)
    }
}
impl World {
    pub(super) fn buy(&mut self, i: usize, item: BasicItem) -> Result<(), &'static str> {
        let offer = self.shop.0[item as usize];
        let h = &mut self.entities[i];
        if h.kind != Kind::Hero || h.position.dist(base(h.team)) > 3. {
            return Err("outside base shop");
        }
        if h.inventory.len() >= 6 || h.gold < offer.price {
            return Err("cannot purchase");
        }
        match item {
            BasicItem::WeaponBlade => h.stats.weapon += offer.bonus,
            BasicItem::LightArmor => h.stats.armor += offer.bonus,
            BasicItem::LightShield => h.stats.shield += offer.bonus,
            BasicItem::Oakheart => {
                h.max_hp += offer.bonus;
                h.hp += offer.bonus;
                h.bonus_hp += offer.bonus
            }
            BasicItem::EnergyBattery => {
                h.max_energy += offer.bonus;
                h.energy += offer.bonus
            }
        }
        h.gold -= offer.price;
        h.inventory.push(item);
        Ok(())
    }
}
