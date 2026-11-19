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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldItem {
    pub kind: BasicItem,
    pub instance: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Purchase {
    pub actor: u32,
    pub paid: u32,
    pub granted: HeldItem,
    pub consumed: Option<HeldItem>,
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
    pub fn template(self) -> u32 {
        [458, 469, 501, 467, 472, 505, 461][self as usize]
    }
    pub fn from_template(id: u32) -> Option<Self> {
        Self::VALUES.into_iter().find(|item| item.template() == id)
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
            let Some(row) = data.items.iter().find(|v| v.id == id) else {
                if matches!(item, BasicItem::HeavySteel | BasicItem::SixSins) {
                    shop.0[item as usize] = Offer {
                        price: 0,
                        bonus: 0.,
                    };
                    continue;
                }
                return Err(format!("missing item {id}"));
            };
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
    pub fn purchase(&mut self, actor: u32, item: BasicItem) -> Result<Purchase, &'static str> {
        let i = self.index(actor).ok_or("unknown actor")?;
        self.buy(i, item)
    }
    pub(super) fn buy(&mut self, i: usize, item: BasicItem) -> Result<Purchase, &'static str> {
        if self.winner.is_some() || self.entities[i].hp <= 0. {
            return Err("shop closed");
        }
        let offer = self.shop.0[item as usize];
        if offer.price == 0 {
            return Err("item unavailable");
        }
        let h = &mut self.entities[i];
        if h.kind != Kind::Hero || h.position.dist(base(h.team)) > 3. {
            return Err("outside base shop");
        }
        let blade = self.shop.0[BasicItem::WeaponBlade as usize];
        let upgrade = matches!(item, BasicItem::HeavySteel | BasicItem::SixSins);
        let component = if upgrade {
            h.inventory
                .iter()
                .position(|i| i.kind == BasicItem::WeaponBlade)
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
        let granted = HeldItem {
            kind: item,
            instance: self.next_item,
        };
        self.next_item = self.next_item.checked_add(1).ok_or("item instance limit")?;
        let consumed = component.map(|index| {
            h.stats.weapon -= blade.bonus;
            h.inventory.remove(index)
        });
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
        h.inventory.push(granted);
        Ok(Purchase {
            actor: h.id,
            paid: price,
            granted,
            consumed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bot_receipt_and_exact_component_instances_survive_duplicate_templates() {
        let mut world = World::new(4, 1);
        for e in &mut world.entities[..10] {
            e.position = base(e.team);
            e.gold = 3000;
        }
        let blade = BasicItem::WeaponBlade;
        let first = world.purchase(1, blade).unwrap();
        let second = world.purchase(1, blade).unwrap();
        let upgrade = world.purchase(1, BasicItem::HeavySteel).unwrap();
        assert_eq!(upgrade.consumed, Some(first.granted));
        assert_eq!(upgrade.paid, 850);
        assert_eq!(
            world.entities[0].inventory,
            [second.granted, upgrade.granted]
        );
        let receipt = (0..10)
            .flat_map(|_| world.step())
            .find_map(|event| match event {
                Event::Purchase(receipt) => Some(receipt),
                _ => None,
            })
            .unwrap();
        let actor = &world.entities[world.index(receipt.actor).unwrap()];
        assert!(actor.bot && actor.inventory.contains(&receipt.granted));
        assert!(receipt.granted.instance > upgrade.granted.instance);
        assert_eq!(receipt.paid, 300);
    }

    #[test]
    fn lethal_awards_match_actual_balance_changes_once() {
        let mut w = World::new(2, 1);
        let pos = base(0);
        let target = w.spawn(1, Kind::Minion, 0, pos, false);
        let idx = w.index(target).unwrap();
        w.entities[0].position = pos;
        w.entities[2].position = pos;
        let before: Vec<_> = w.entities[..10].iter().map(|e| e.gold).collect();
        let mut events = vec![];
        w.hurt(1, 0, idx, 1., &mut events);
        assert!(!events.iter().any(|e| matches!(e, Event::GoldAward { .. })));
        w.hurt(1, 0, idx, 100000., &mut events);
        let count = events.len();
        w.hurt(1, 0, idx, 100000., &mut events);
        assert_eq!(count, events.len());
        for (e, old) in w.entities[..10].iter().zip(before) {
            let delta = e.gold - old;
            let awards: Vec<_> = events
                .iter()
                .filter_map(|event| match event {
                    Event::GoldAward { id, amount } if *id == e.id => Some(*amount),
                    _ => None,
                })
                .collect();
            if delta > 0 {
                assert_eq!(awards, [delta]);
            } else {
                assert!(awards.is_empty());
            }
        }
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::GoldAward { id: 3, .. }))
        );
    }
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
            h.inventory = (0..5)
                .map(|instance| HeldItem {
                    kind: BasicItem::LightArmor,
                    instance,
                })
                .collect();
            h.stats.weapon = start;
            h.gold = 300;
            w.command(1, Command::Buy(BasicItem::WeaponBlade)).unwrap();
            w.entities[0].gold = upgrade - 1;
            assert!(w.command(1, Command::Buy(item)).is_err());
            assert_eq!(
                w.entities[0].inventory.last().map(|held| held.kind),
                Some(BasicItem::WeaponBlade)
            );
            w.entities[0].gold = upgrade;
            w.command(1, Command::Buy(item)).unwrap();
            assert_eq!(w.entities[0].inventory.len(), 6);
            assert_eq!(w.entities[0].inventory.last().map(|v| v.kind), Some(item));
            assert_eq!(w.entities[0].gold, 0);
            assert_eq!(w.entities[0].stats.weapon, start + bonus);
        }
    }
}
