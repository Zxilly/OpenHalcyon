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
    CrystalBit,
    SwiftShooter,
    BlazingSalvo,
    Hourglass,
    Chronograph,
    SprintBoots,
    TravelBoots,
    AlternatingCurrent,
    StormguardBanner,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldItem {
    pub kind: BasicItem,
    pub instance: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Purchase {
    pub actor: u32,
    pub paid: u32,
    pub granted: HeldItem,
    pub consumed: Vec<HeldItem>,
}

use BasicItem::*;

struct Definition {
    kind: BasicItem,
    template: u32,
    name: &'static str,
    combine: u32,
    components: &'static [BasicItem],
    bonuses: &'static [(u8, f32)],
}

const fn def(
    kind: BasicItem,
    template: u32,
    name: &'static str,
    combine: u32,
    components: &'static [BasicItem],
    bonuses: &'static [(u8, f32)],
) -> Definition {
    Definition {
        kind,
        template,
        name,
        combine,
        components,
        bonuses,
    }
}

// Alternating Current's Heavy Prism branch contains Crystal Bit. This shop
// supports buying the observed leaf and charges the missing Prism combine fee.
const OFFERS: [Definition; 16] = [
    def(WeaponBlade, 458, "Item_WeaponBlade", 300, &[], &[(4, 10.)]),
    def(LightArmor, 469, "Item_LightArmor", 300, &[], &[(7, 25.)]),
    def(LightShield, 501, "Item_LightShield", 300, &[], &[(8, 25.)]),
    def(Oakheart, 467, "Item_Oakheart", 300, &[], &[(0, 150.)]),
    def(
        EnergyBattery,
        472,
        "Item_EnergyBattery",
        300,
        &[],
        &[(2, 100.)],
    ),
    def(
        HeavySteel,
        505,
        "Item_HeavySteel",
        850,
        &[WeaponBlade],
        &[(4, 45.)],
    ),
    def(
        SixSins,
        461,
        "Item_SixSins",
        350,
        &[WeaponBlade],
        &[(4, 25.)],
    ),
    def(CrystalBit, 459, "Item_CrystalBit", 300, &[], &[(5, 15.)]),
    def(
        SwiftShooter,
        460,
        "Item_SwiftShooter",
        300,
        &[],
        &[(15, 0.1)],
    ),
    def(
        BlazingSalvo,
        463,
        "Item_BlazingSalvo",
        400,
        &[SwiftShooter],
        &[(15, 0.2)],
    ),
    def(
        Hourglass,
        473,
        "Item_Hourglass",
        250,
        &[],
        &[(25, 0.075), (3, 0.25)],
    ),
    def(
        Chronograph,
        475,
        "Item_Chronograph",
        550,
        &[Hourglass],
        &[(25, 0.15), (3, 0.75)],
    ),
    def(SprintBoots, 477, "Item_SprintBoots", 300, &[], &[]),
    def(
        TravelBoots,
        478,
        "Item_TravelBoots",
        350,
        &[SprintBoots],
        &[(0, 100.)],
    ),
    def(
        AlternatingCurrent,
        509,
        "Item_AlternatingCurrent",
        1050,
        &[BlazingSalvo, CrystalBit],
        &[(5, 45.), (15, 0.4)],
    ),
    def(
        StormguardBanner,
        513,
        "Item_StormguardBanner",
        300,
        &[Oakheart],
        &[(0, 150.)],
    ),
];

impl BasicItem {
    fn definition(self) -> &'static Definition {
        &OFFERS[self as usize]
    }
    pub fn template(self) -> u32 {
        self.definition().template
    }
    pub fn from_template(id: u32) -> Option<Self> {
        OFFERS.iter().find(|d| d.template == id).map(|d| d.kind)
    }
}

#[derive(Serialize)]
struct Offer {
    combine: u32,
    bonuses: Vec<(u8, f32)>,
}
#[derive(Serialize)]
pub(super) struct Shop {
    offers: [Offer; 16],
    prism_combine: u32,
}
impl Default for Shop {
    fn default() -> Self {
        Self {
            offers: OFFERS.each_ref().map(|d| Offer {
                combine: d.combine,
                bonuses: d.bonuses.to_vec(),
            }),
            prism_combine: 750,
        }
    }
}
impl Shop {
    pub fn load(data: &GameData) -> Result<Self, String> {
        let mut shop = Self::default();
        for d in &OFFERS {
            let offer = &mut shop.offers[d.kind as usize];
            let Some(row) = data.items.iter().find(|row| row.id == d.name) else {
                if (d.kind as usize) < 5 {
                    return Err(format!("missing item {}", d.name));
                }
                offer.combine = 0;
                continue;
            };
            offer.combine = row
                .cost_a
                .filter(|n| *n > 0)
                .ok_or_else(|| format!("invalid cost {}", d.name))?;
            for (stat, value) in &mut offer.bonuses {
                if let Some(b) = row
                    .other
                    .get("bonuses")
                    .and_then(|v| v.as_array())
                    .and_then(|rows| {
                        rows.iter()
                            .find(|b| b["stat_id"].as_u64() == Some(u64::from(*stat)))
                    })
                {
                    *value = b["value"]
                        .as_f64()
                        .filter(|v| v.is_finite() && *v > 0.)
                        .ok_or_else(|| format!("invalid bonus {}", d.name))?
                        as f32;
                } else if (d.kind as usize) < 7 {
                    return Err(format!("missing bonus {}", d.name));
                }
            }
        }
        if let Some(prism) = data.items.iter().find(|row| row.id == "Item_HeavyPrism") {
            shop.prism_combine = prism
                .cost_a
                .filter(|v| *v > 0)
                .ok_or("invalid Heavy Prism combine cost")?;
        }
        Ok(shop)
    }

    fn cost(&self, item: BasicItem) -> u64 {
        let loaded = self.offers[item as usize].combine;
        // A partial operator catalog may omit an intermediate. Its verified
        // default recipe value still contributes when the final item is sold.
        let combine = if loaded == 0 {
            item.definition().combine
        } else {
            loaded
        };
        u64::from(combine)
            + if item == AlternatingCurrent {
                u64::from(self.prism_combine)
            } else {
                0
            }
            + item
                .definition()
                .components
                .iter()
                .map(|&part| self.cost(part))
                .sum::<u64>()
    }

    fn consume_branch(kind: BasicItem, held: &[HeldItem], consumed: &mut Vec<HeldItem>) {
        if let Some(&owned) = held
            .iter()
            .find(|h| h.kind == kind && !consumed.contains(h))
        {
            consumed.push(owned);
        } else {
            for &part in kind.definition().components {
                Self::consume_branch(part, held, consumed);
            }
        }
    }
}

impl World {
    pub(super) fn acquisition_value(&self, item: BasicItem) -> u64 {
        self.shop.cost(item)
    }
    pub fn item_bonuses(&self, item: BasicItem) -> impl Iterator<Item = (u8, f32)> + '_ {
        self.shop.offers[item as usize].bonuses.iter().copied()
    }
    pub fn purchase(&mut self, actor: u32, item: BasicItem) -> Result<Purchase, &'static str> {
        let i = self.index(actor).ok_or("unknown actor")?;
        self.buy(i, item)
    }
    pub(super) fn buy(&mut self, i: usize, item: BasicItem) -> Result<Purchase, &'static str> {
        let h = &self.entities[i];
        if self.winner.is_some() || h.hp <= 0. {
            return Err("shop closed");
        }
        if h.kind != Kind::Hero || !self.layout.at_base(h, true) {
            return Err("outside base shop");
        }
        if self.shop.offers[item as usize].combine == 0 {
            return Err("item unavailable");
        }
        let mut consumed = Vec::new();
        for &part in item.definition().components {
            Shop::consume_branch(part, &h.inventory, &mut consumed);
        }
        let price =
            self.shop.cost(item) - consumed.iter().map(|h| self.shop.cost(h.kind)).sum::<u64>();
        if h.inventory.len() - consumed.len() >= 6 || u64::from(h.gold) < price {
            return Err("cannot purchase");
        }
        let granted = HeldItem {
            kind: item,
            instance: self.next_item,
        };
        self.next_item = self.next_item.checked_add(1).ok_or("item instance limit")?;
        let h = &mut self.entities[i];
        h.inventory.retain(|owned| !consumed.contains(owned));
        // Apply the net modifier once. Upgrading an HP component must preserve
        // missing health when the completed item grants the same HP.
        let mut delta = [0.; 26];
        for used in &consumed {
            for &(stat, value) in &self.shop.offers[used.kind as usize].bonuses {
                delta[stat as usize] -= value;
            }
        }
        for &(stat, value) in &self.shop.offers[item as usize].bonuses {
            delta[stat as usize] += value;
        }
        h.bonus_hp += delta[0];
        h.max_hp += delta[0];
        h.hp = (h.hp + delta[0]).clamp(0., h.max_hp);
        h.max_energy += delta[2];
        h.energy = (h.energy + delta[2]).clamp(0., h.max_energy);
        h.item_stats.energy_regen += delta[3];
        h.stats.weapon += delta[4];
        h.item_stats.crystal += delta[5];
        h.stats.armor += delta[7];
        h.stats.shield += delta[8];
        h.item_stats.attack_speed += delta[15];
        h.item_stats.cooldown += delta[25];
        h.gold -= price as u32;
        h.inventory.push(granted);
        Ok(Purchase {
            actor: h.id,
            paid: price as u32,
            granted,
            consumed,
        })
    }
}

#[derive(Clone, Copy, Default, Debug, Serialize)]
pub struct ItemStats {
    pub crystal: f32,
    pub attack_speed: f32,
    pub energy_regen: f32,
    pub cooldown: f32,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bought_crystal_power_contributes_the_published_karas_basic_ratio() {
        let mut w = World::new(3, 10);
        w.entities[0].position = base(0);
        w.entities[0].type_id = 926;
        w.entities[0].gold = 300;
        w.purchase(1, CrystalBit).unwrap();
        w.entities[1].position = Position { x: 4., y: 0. };
        let expected = 15. * 0.6 * 100. / (100. + w.entities[1].stats.shield);
        w.command(1, Command::Attack(2)).unwrap();
        let applied=(0..15).flat_map(|_|w.step()).any(|event| matches!(event,Event::Damage{source:1,target:2,amount,..} if (amount-expected).abs()<0.001));
        assert!(applied);
    }

    #[test]
    fn archived_aral_build_consumes_exact_components_and_preserves_balance_on_failure() {
        let mut world = World::new(4, 6);
        world.use_six_human_roster().unwrap();
        world.use_aral_lane().unwrap();
        assert_eq!(
            world.player_score(1).unwrap().gold_income.starting_balance,
            2000
        );
        let bit = world.purchase(1, CrystalBit).unwrap();
        let swift = world.purchase(1, SwiftShooter).unwrap();
        let boots = world.purchase(1, SprintBoots).unwrap();
        let salvo = world.purchase(1, BlazingSalvo).unwrap();
        assert_eq!(
            [bit.paid, swift.paid, boots.paid, salvo.paid],
            [300, 300, 300, 400]
        );
        assert_eq!(salvo.consumed, [swift.granted]);
        assert_eq!(world.entities[0].gold, 700);
        let inventory = world.entities[0].inventory.clone();
        let next = world.next_item;
        assert!(world.purchase(1, AlternatingCurrent).is_err());
        assert_eq!(world.entities[0].inventory, inventory);
        assert_eq!(world.next_item, next);
        assert_eq!(world.entities[0].gold, 700);
        world.entities[1].hp = 0.;
        world.entities[1].respawn = u64::MAX;
        let before = world.entities[1].gold;
        while world.entities[0].gold < 2150 {
            world.step();
        }
        assert_eq!(world.entities[0].gold, 2155);
        assert_eq!(world.entities[1].gold - before, 1455);
        assert_eq!(
            world.player_score(1).unwrap().gold_income.passive_awards,
            1455
        );
        let current = world.purchase(1, AlternatingCurrent).unwrap();
        assert_eq!(current.paid, 1800);
        assert_eq!(current.consumed, [salvo.granted, bit.granted]);
        assert_eq!(world.entities[0].item_stats.crystal, 45.);
        assert_eq!(world.entities[0].item_stats.attack_speed, 0.4);
        assert_eq!(world.acquisition_value(AlternatingCurrent), 2800);
        let hp = world.entities[0].max_hp;
        let travel = world.purchase(1, TravelBoots).unwrap();
        assert_eq!(travel.paid, 350);
        assert_eq!(travel.consumed, [boots.granted]);
        assert_eq!(world.entities[0].max_hp, hp + 100.);
        assert_eq!(
            world.entities[0].inventory,
            [current.granted, travel.granted]
        );
        assert_eq!(world.entities[0].gold, 5);
    }

    #[test]
    fn direct_and_partial_recipes_apply_measured_modifiers_and_shop_constraints() {
        let mut w = World::new(4, 1);
        w.entities[0].position = base(0);
        w.entities[0].gold = 20000;
        let banner = w.purchase(1, StormguardBanner).unwrap();
        assert_eq!(banner.paid, 600);
        assert_eq!(w.entities[0].bonus_hp, 150.);
        let chrono = w.purchase(1, Chronograph).unwrap();
        assert_eq!(chrono.paid, 800);
        assert_eq!(w.entities[0].item_stats.cooldown, 0.15);
        assert_eq!(w.entities[0].item_stats.energy_regen, 0.75);
        let glass = w.purchase(1, Hourglass).unwrap();
        let upgraded = w.purchase(1, Chronograph).unwrap();
        assert_eq!(upgraded.paid, 550);
        assert_eq!(upgraded.consumed, [glass.granted]);
        assert_eq!(w.entities[0].item_stats.cooldown, 0.3);
        assert_eq!(w.entities[0].item_stats.energy_regen, 1.5);
        let swift = w.purchase(1, SwiftShooter).unwrap();
        let bit = w.purchase(1, CrystalBit).unwrap();
        w.purchase(1, WeaponBlade).unwrap();
        assert!(w.purchase(1, SprintBoots).is_err());
        let current = w.purchase(1, AlternatingCurrent).unwrap();
        assert_eq!(current.paid, 2200);
        assert_eq!(current.consumed, [swift.granted, bit.granted]);
        assert_eq!(w.entities[0].inventory.len(), 5);
        w.entities[0].position = base(1);
        let before = w.entities[0].gold;
        assert!(w.purchase(1, SprintBoots).is_err());
        assert_eq!(w.entities[0].gold, before);
    }

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
        assert_eq!(upgrade.consumed, [first.granted]);
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
        w.hurt(1, 0, idx, 1., DamageSchool::Weapon, &mut events);
        assert!(!events.iter().any(|e| matches!(e, Event::GoldAward { .. })));
        w.hurt(1, 0, idx, 100000., DamageSchool::Weapon, &mut events);
        let count = events.len();
        w.hurt(1, 0, idx, 100000., DamageSchool::Weapon, &mut events);
        assert_eq!(count, events.len());
        for (e, old) in w.entities[..10].iter().zip(before) {
            let delta = e.gold - old;
            let income = w.player_score(e.id).unwrap().gold_income;
            assert_eq!(income.starting_balance, old);
            assert_eq!(income.passive_awards, 0);
            assert_eq!(
                income.last_hit_bounties,
                if e.id == 1 { u64::from(delta) } else { 0 }
            );
            assert_eq!(
                income.lane_proximity_awards,
                if e.id != 1 { u64::from(delta) } else { 0 }
            );
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
