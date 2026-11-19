use super::*;
/// Base-shop purchases backed by named stats and observed component semantics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BasicItem {
    WeaponBlade,
    LightArmor,
    LightShield,
    Oakheart,
    EnergyBattery,
    HeavySteel,
    SixSins,
}
impl BasicItem {
    const VALUES: [Self; 7] = [
        Self::WeaponBlade,
        Self::LightArmor,
        Self::LightShield,
        Self::Oakheart,
        Self::EnergyBattery,
        Self::HeavySteel,
        Self::SixSins,
    ];
    pub fn from_template(id: u32) -> Option<Self> {
        Some(match id {
            458 => Self::WeaponBlade,
            461 => Self::SixSins,
            467 => Self::Oakheart,
            469 => Self::LightArmor,
            472 => Self::EnergyBattery,
            501 => Self::LightShield,
            505 => Self::HeavySteel,
            _ => return None,
        })
    }
    fn descriptor(self) -> (&'static str, u64) {
        match self {
            Self::HeavySteel => ("Item_HeavySteel", 4),
            Self::SixSins => ("Item_SixSins", 4),
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
pub(super) struct Shop([Offer; 7]);
impl Default for Shop {
    fn default() -> Self {
        Self(
            [
                (300, 10.),
                (300, 25.),
                (300, 25.),
                (300, 150.),
                (300, 100.),
                (850, 45.),
                (350, 25.),
            ]
            .map(|(price, bonus)| Offer { price, bonus }),
        )
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
    pub fn item_bonus(&self, item: BasicItem) -> (u8, f32) {
        (item.descriptor().1 as u8, self.shop.0[item as usize].bonus)
    }
    pub(super) fn buy(&mut self, i: usize, item: BasicItem) -> Result<(), &'static str> {
        let offer = self.shop.0[item as usize];
        let h = &mut self.entities[i];
        if h.kind != Kind::Hero || h.position.dist(base(h.team)) > 3. {
            return Err("outside base shop");
        }
        let blade = self.shop.0[BasicItem::WeaponBlade as usize];
        let upgrade = matches!(item, BasicItem::HeavySteel | BasicItem::SixSins);
        let component = if upgrade {
            h.inventory
                .iter()
                .position(|i| *i == BasicItem::WeaponBlade)
        } else {
            None
        };
        let price = if upgrade && component.is_none() {
            offer
                .price
                .checked_add(blade.price)
                .ok_or("invalid recipe price")?
        } else {
            offer.price
        };
        if (h.inventory.len() >= 6 && component.is_none()) || h.gold < price {
            return Err("cannot purchase");
        }
        if let Some(index) = component {
            h.inventory.remove(index);
            h.stats.weapon -= blade.bonus;
        }
        match item {
            BasicItem::HeavySteel | BasicItem::SixSins | BasicItem::WeaponBlade => {
                h.stats.weapon += offer.bonus
            }
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
        h.gold -= price;
        h.inventory.push(item);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observed_tier_two_upgrades_consume_one_blade_or_charge_full_price() {
        for (item, upgrade, bonus) in [
            (BasicItem::HeavySteel, 850, 45.),
            (BasicItem::SixSins, 350, 25.),
        ] {
            let mut w = World::new(4, 1);
            w.entities[0].position = base(0);
            let start = w.entities[0].stats.weapon;
            w.entities[0].gold = upgrade + 300;
            w.command(1, Command::Buy(item)).unwrap();
            assert_eq!(w.entities[0].gold, 0);
            assert_eq!(w.entities[0].stats.weapon, start + bonus);
            let h = &mut w.entities[0];
            h.inventory = vec![BasicItem::LightArmor; 5];
            h.stats.weapon = start;
            h.gold = 300;
            w.command(1, Command::Buy(BasicItem::WeaponBlade)).unwrap();
            w.entities[0].gold = upgrade - 1;
            assert!(w.command(1, Command::Buy(item)).is_err());
            assert_eq!(
                w.entities[0].inventory.last(),
                Some(&BasicItem::WeaponBlade)
            );
            w.entities[0].gold = upgrade;
            w.command(1, Command::Buy(item)).unwrap();
            assert_eq!(w.entities[0].inventory.len(), 6);
            assert_eq!(w.entities[0].inventory.last(), Some(&item));
            assert_eq!(w.entities[0].gold, 0);
            assert_eq!(w.entities[0].stats.weapon, start + bonus);
        }
    }
}
