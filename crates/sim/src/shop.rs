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
    EclipsePrism,
    Sorrowblade,
    Shatterglass,
    CoatOfPlates,
    MetalJacket,
    VoidBattery,
    Clockwork,
    SerpentMask,
    ReflexBlock,
    Frostburn,
    FountainOfRenewal,
    Crucible,
    JourneyBoots,
    HalcyonChargers,
    Aftershock,
    WarTreads,
    AtlasPauldron,
    BookOfEulogies,
    BarbedNeedle,
    KineticShield,
    Lifespring,
    BreakingPoint,
    EveOfHarvest,
    HeavyPrism,
    Contraption,
    Flaregun,
    Stormcrown,
    PoisonedShiv,
    Spellfire,
    DragonsEye,
    Spellsword,
    SlumberingHusk,
    Warmail,
    SeraphicShell,
    Dragonheart,
    PiercingSpear,
    PiercingShard,
    Bonesaw,
    BrokenMyth,
    Pulseweave,
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

const OFFERS: [Definition; 56] = [
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
        &[(2, 100.), (3, 1.5)],
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
        &[BlazingSalvo, HeavyPrism],
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
    def(
        EclipsePrism,
        462,
        "Item_EclipsePrism",
        350,
        &[CrystalBit],
        &[(5, 30.0)],
    ),
    def(
        Sorrowblade,
        464,
        "Item_Sorrowblade",
        1300,
        &[SixSins, HeavySteel],
        &[(4, 120.0)],
    ),
    def(
        Shatterglass,
        465,
        "Item_Shatterglass",
        1300,
        &[EclipsePrism, HeavyPrism],
        &[(5, 130.0)],
    ),
    def(
        CoatOfPlates,
        470,
        "Item_CoatOfPlates",
        450,
        &[LightArmor],
        &[(7, 55.0)],
    ),
    def(
        MetalJacket,
        471,
        "Item_MetalJacket",
        1150,
        &[CoatOfPlates],
        &[(7, 95.0)],
    ),
    def(
        VoidBattery,
        474,
        "Item_VoidBattery",
        400,
        &[EnergyBattery],
        &[(2, 250.0), (3, 3.0)],
    ),
    def(
        Clockwork,
        476,
        "Item_Clockwork",
        900,
        &[Chronograph, VoidBattery],
        &[(5, 30.0), (2, 400.0), (3, 5.0), (25, 0.2)],
    ),
    def(
        SerpentMask,
        479,
        "Item_SerpentMask",
        850,
        &[HeavySteel, BarbedNeedle],
        &[(4, 70.0)],
    ),
    def(
        ReflexBlock,
        485,
        "Item_ReflexBlock",
        400,
        &[Oakheart],
        &[(0, 150.0)],
    ),
    def(
        Frostburn,
        486,
        "Item_Frostburn",
        1000,
        &[HeavyPrism, EclipsePrism],
        &[(5, 90.0)],
    ),
    def(
        FountainOfRenewal,
        487,
        "Item_FountainOfRenewal",
        500,
        &[Warmail, Lifespring],
        &[(0, 400.0), (7, 40.0), (8, 40.0)],
    ),
    def(
        Crucible,
        488,
        "Item_Crucible",
        650,
        &[ReflexBlock, Dragonheart],
        &[(0, 550.0)],
    ),
    def(
        JourneyBoots,
        489,
        "Item_JourneyBoots",
        750,
        &[TravelBoots],
        &[(0, 150.0)],
    ),
    def(
        HalcyonChargers,
        490,
        "Item_HalcyonChargers",
        50,
        &[TravelBoots, VoidBattery],
        &[(0, 150.0), (25, 0.1), (2, 250.0), (3, 3.5)],
    ),
    def(
        Aftershock,
        492,
        "Item_Aftershock",
        1150,
        &[Chronograph, EclipsePrism],
        &[(5, 30.0), (3, 1.0), (25, 0.15)],
    ),
    def(
        WarTreads,
        497,
        "Item_WarTreads",
        300,
        &[Dragonheart, TravelBoots],
        &[(0, 400.0)],
    ),
    def(
        AtlasPauldron,
        498,
        "Item_AtlasPauldron",
        950,
        &[CoatOfPlates],
        &[(7, 65.0)],
    ),
    def(
        BookOfEulogies,
        499,
        "Item_BookOfEulogies",
        300,
        &[],
        &[(4, 5.0)],
    ),
    def(
        BarbedNeedle,
        500,
        "Item_BarbedNeedle",
        500,
        &[BookOfEulogies],
        &[(4, 10.0)],
    ),
    def(
        KineticShield,
        502,
        "Item_KineticShield",
        450,
        &[LightShield],
        &[(8, 55.0)],
    ),
    def(
        Lifespring,
        504,
        "Item_Lifespring",
        500,
        &[Oakheart],
        &[(0, 200.0)],
    ),
    def(
        BreakingPoint,
        507,
        "Item_BreakingPoint",
        850,
        &[HeavySteel, BlazingSalvo],
        &[(4, 50.0), (15, 0.2)],
    ),
    def(
        EveOfHarvest,
        511,
        "Item_EveOfHarvest",
        850,
        &[VoidBattery, HeavyPrism],
        &[(5, 60.0), (2, 300.0), (3, 4.0)],
    ),
    def(
        HeavyPrism,
        512,
        "Item_HeavyPrism",
        750,
        &[CrystalBit],
        &[(5, 45.0)],
    ),
    def(
        Contraption,
        516,
        "Item_Contraption",
        700,
        &[Chronograph, Flaregun],
        &[(0, 450.0), (3, 2.0), (25, 0.2)],
    ),
    def(
        Flaregun,
        518,
        "Item_Flaregun",
        300,
        &[Oakheart],
        &[(0, 300.0)],
    ),
    def(
        Stormcrown,
        519,
        "Item_Stormcrown",
        1000,
        &[StormguardBanner, Chronograph],
        &[(0, 300.0), (3, 2.0), (25, 0.2)],
    ),
    def(
        PoisonedShiv,
        520,
        "Item_PoisonedShiv",
        1250,
        &[BlazingSalvo, BarbedNeedle],
        &[(4, 35.0), (15, 0.35)],
    ),
    def(
        Spellfire,
        522,
        "Item_Spellfire",
        1000,
        &[HeavyPrism, EclipsePrism],
        &[(5, 80.0)],
    ),
    def(
        DragonsEye,
        523,
        "Item_DragonsEye",
        1300,
        &[EclipsePrism, HeavyPrism],
        &[(5, 75.0)],
    ),
    def(
        Spellsword,
        524,
        "Item_Spellsword",
        200,
        &[HeavySteel, SixSins, Chronograph],
        &[(4, 85.0), (25, 0.2), (3, 2.0)],
    ),
    def(
        SlumberingHusk,
        525,
        "Item_SlumberingHusk",
        600,
        &[KineticShield, CoatOfPlates],
        &[(7, 55.0), (8, 55.0)],
    ),
    def(
        Warmail,
        538,
        "Item_Warmail",
        200,
        &[LightArmor, LightShield],
        &[(7, 30.0), (8, 30.0)],
    ),
    def(
        SeraphicShell,
        539,
        "Item_SeraphicShell",
        450,
        &[KineticShield, ReflexBlock],
        &[(8, 95.0), (0, 300.0)],
    ),
    def(
        Dragonheart,
        468,
        "Item_Dragonheart",
        350,
        &[Oakheart],
        &[(0, 350.0)],
    ),
    def(
        PiercingSpear,
        506,
        "Item_PiercingSpear",
        600,
        &[WeaponBlade],
        &[(4, 15.), (9, 0.1)],
    ),
    def(
        PiercingShard,
        510,
        "Item_PiercingShard",
        600,
        &[CrystalBit],
        &[(5, 20.), (11, 0.1)],
    ),
    def(
        Bonesaw,
        482,
        "Item_Bonesaw",
        1300,
        &[BlazingSalvo, PiercingSpear],
        &[(4, 30.), (15, 0.3), (9, 0.1), (9, 0.1)],
    ),
    def(
        BrokenMyth,
        496,
        "Item_BrokenMyth",
        1350,
        &[PiercingShard, EclipsePrism],
        &[(5, 50.), (11, 0.1), (11, 0.35)],
    ),
    def(
        Pulseweave,
        533,
        "Item_Pulseweave",
        550,
        &[Dragonheart, Lifespring],
        &[(0, 600.), (18, 0.08)],
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
    offers: Vec<Offer>,
}
impl Default for Shop {
    fn default() -> Self {
        Self {
            offers: OFFERS
                .iter()
                .map(|d| Offer {
                    combine: d.combine,
                    bonuses: d.bonuses.to_vec(),
                })
                .collect(),
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
                } else if (d.kind as usize) < 7 && d.bonuses[0].0 == *stat {
                    return Err(format!("missing bonus {}", d.name));
                }
            }
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
    /// A valid shop order can be declined for insufficient gold or inventory space.
    /// A decline leaves both the inventory and the next item instance untouched.
    pub fn try_purchase(
        &mut self,
        actor: u32,
        item: BasicItem,
    ) -> Result<Option<Purchase>, &'static str> {
        let i = self.index(actor).ok_or("unknown actor")?;
        self.try_buy_at(i, item)
    }
    pub(super) fn buy(&mut self, i: usize, item: BasicItem) -> Result<Purchase, &'static str> {
        self.try_buy_at(i, item)?.ok_or("cannot purchase")
    }
    fn try_buy_at(&mut self, i: usize, item: BasicItem) -> Result<Option<Purchase>, &'static str> {
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
            return Ok(None);
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
        h.item_stats.armor_pierce += delta[9];
        h.item_stats.shield_pierce += delta[11];
        h.item_stats.move_speed_amp += delta[18];
        h.item_stats.attack_speed += delta[15];
        h.item_stats.cooldown += delta[25];
        h.gold -= price as u32;
        h.inventory.push(granted);
        Ok(Some(Purchase {
            actor: h.id,
            paid: price as u32,
            granted,
            consumed,
        }))
    }
}

#[derive(Clone, Copy, Default, Debug, Serialize)]
pub struct ItemStats {
    pub armor_pierce: f32,
    pub shield_pierce: f32,
    pub move_speed_amp: f32,
    pub crystal: f32,
    pub attack_speed: f32,
    pub energy_regen: f32,
    pub cooldown: f32,
}
/// The delivered damage rules attribute this formula to contemporary guides.
/// Penetration bypasses a fraction of mitigation for this attacker's damage.
pub(super) fn mitigated(raw: f32, defense: f32, pierce: f32) -> f32 {
    raw * (100. + pierce.clamp(0., 1.) * defense) / (100. + defense)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_recorded_supported_recipe_debits_and_consumes_the_correct_instances() {
        for (index, definition) in OFFERS.iter().enumerate() {
            assert_eq!(definition.kind as usize, index);
            assert_eq!(
                BasicItem::from_template(definition.template),
                Some(definition.kind)
            );
        }
        let profiles: &[(u32, u32, &[u32])] = &[
            (458, 300, &[]),
            (459, 300, &[]),
            (460, 300, &[]),
            (461, 350, &[458]),
            (461, 650, &[]),
            (462, 350, &[459]),
            (462, 650, &[]),
            (463, 400, &[460]),
            (463, 700, &[]),
            (464, 1300, &[461, 505]),
            (464, 2150, &[461, 458]),
            (464, 2500, &[458, 458]),
            (464, 2800, &[458]),
            (464, 3100, &[]),
            (465, 1300, &[462, 512]),
            (465, 1650, &[459, 512]),
            (465, 1950, &[512]),
            (465, 2400, &[459, 459]),
            (465, 3000, &[]),
            (467, 300, &[]),
            (469, 300, &[]),
            (470, 750, &[]),
            (471, 1900, &[]),
            (472, 300, &[]),
            (473, 250, &[]),
            (474, 400, &[472]),
            (474, 700, &[]),
            (475, 800, &[]),
            (476, 1600, &[475]),
            (476, 2100, &[472]),
            (477, 300, &[]),
            (478, 350, &[477]),
            (478, 650, &[]),
            (479, 850, &[505, 500]),
            (479, 1350, &[505, 499]),
            (479, 2000, &[500]),
            (479, 2800, &[]),
            (485, 400, &[467]),
            (485, 700, &[]),
            (486, 1350, &[512, 459]),
            (486, 2700, &[]),
            (487, 500, &[538, 504]),
            (487, 700, &[469, 501, 504]),
            (488, 1300, &[485]),
            (489, 1100, &[477]),
            (490, 50, &[478, 474]),
            (490, 800, &[477, 472]),
            (490, 1100, &[472]),
            (490, 1100, &[477]),
            (490, 1400, &[]),
            (492, 2600, &[]),
            (497, 950, &[478]),
            (497, 1300, &[477]),
            (497, 1600, &[]),
            (498, 1700, &[]),
            (499, 300, &[]),
            (500, 500, &[499]),
            (500, 800, &[]),
            (501, 300, &[]),
            (502, 750, &[]),
            (504, 500, &[467]),
            (504, 800, &[]),
            (505, 850, &[458]),
            (505, 1150, &[]),
            (507, 1250, &[505, 460]),
            (507, 2700, &[]),
            (509, 1750, &[512]),
            (509, 1800, &[463, 459]),
            (509, 2800, &[]),
            (511, 1900, &[474]),
            (511, 2300, &[459]),
            (512, 750, &[459]),
            (512, 1050, &[]),
            (513, 600, &[]),
            (516, 1500, &[518]),
            (518, 600, &[]),
            (519, 1000, &[513, 475]),
            (520, 1950, &[500]),
            (522, 1000, &[512, 462]),
            (522, 2100, &[459, 459]),
            (522, 2700, &[]),
            (523, 2350, &[462]),
            (523, 3000, &[]),
            (524, 2800, &[]),
            (525, 1050, &[502, 469]),
            (525, 2100, &[]),
            (538, 500, &[469]),
            (538, 800, &[]),
            (539, 1150, &[502]),
            (539, 1200, &[485]),
            (539, 1600, &[501]),
            (506, 900, &[]),
            (510, 900, &[]),
            (482, 2900, &[]),
            (482, 2200, &[463]),
            (482, 2600, &[460]),
            (496, 2900, &[]),
            (496, 1950, &[459, 462]),
            (533, 2000, &[]),
        ];
        for &(template, debit, parts) in profiles {
            let mut w = World::new(7, 6);
            w.entities[0].position = base(0);
            w.entities[0].gold = 10_000;
            // Buy finished branches first so acquiring one never consumes
            // a separately held leaf from the captured pre-purchase inventory.
            let mut order: Vec<_> = parts
                .iter()
                .enumerate()
                .map(|(i, &key)| (i, BasicItem::from_template(key).unwrap()))
                .collect();
            order.sort_by_key(|&(_, part)| std::cmp::Reverse(w.acquisition_value(part)));
            let mut held: Vec<_> = order
                .into_iter()
                .map(|(i, part)| (i, w.purchase(1, part).unwrap().granted))
                .collect();
            held.sort_by_key(|&(i, _)| i);
            let owned: Vec<_> = held.into_iter().map(|(_, item)| item).collect();
            let item = BasicItem::from_template(template).unwrap();
            let original_inventory = w.entities[0].inventory.clone();
            w.entities[0].gold = debit - 1;
            assert!(w.purchase(1, item).is_err(), "template {template}");
            assert_eq!(w.entities[0].inventory, original_inventory);
            w.entities[0].gold = debit;
            let result = w.purchase(1, item).unwrap();
            assert_eq!(result.paid, debit, "template {template}");
            assert_eq!(result.consumed, owned, "template {template}");
            assert_eq!(w.entities[0].inventory, [result.granted]);
            assert_eq!(w.entities[0].gold, 0);
        }
    }

    #[test]
    fn penetration_bypasses_only_its_damage_school_and_speed_amp_changes_distance() {
        let mut w = World::new(3, 10);
        w.entities[0].position = base(0);
        w.entities[0].gold = 10_000;
        let shard = w.purchase(1, PiercingShard).unwrap();
        let myth = w.purchase(1, BrokenMyth).unwrap();
        assert_eq!(myth.consumed, [shard.granted]);
        assert!((w.entities[0].item_stats.shield_pierce - 0.45).abs() < 0.00001);
        assert_eq!(w.entities[0].item_stats.armor_pierce, 0.);
        let spear = w.purchase(1, PiercingSpear).unwrap();
        let saw = w.purchase(1, Bonesaw).unwrap();
        assert_eq!(saw.consumed, [spear.granted]);
        assert!((w.entities[0].item_stats.armor_pierce - 0.2).abs() < 0.00001);
        assert_eq!(mitigated(100., 100., 0.2), 60.);
        assert_eq!(mitigated(100., 100., 0.45), 72.5);

        w.entities[1].position = Position { x: 4., y: 0. };
        let weapon = w.entities[0].weapon_power();
        let armor = w.entities[1].armor();
        let expected = weapon * (100. + armor * 0.2) / (100. + armor);
        w.command(1, Command::Attack(2)).unwrap();
        assert!((0..15).flat_map(|_| w.step()).any(|event| matches!(event,
            Event::Damage { source: 1, target: 2, amount, .. } if (amount-expected).abs()<0.001)));

        let mut slow = World::new(9, 10);
        let mut fast = World::new(9, 10);
        fast.entities[0].gold = 2000;
        fast.purchase(1, Pulseweave).unwrap();
        let origin = slow.entities[0].position;
        let dest = Position {
            x: origin.x + 5.,
            y: origin.y,
        };
        for world in [&mut slow, &mut fast] {
            world.command(1, Command::MoveTo(dest)).unwrap();
            world.step();
        }
        let ordinary = origin.dist(slow.entities[0].position);
        assert!(ordinary > 0.);
        assert!((origin.dist(fast.entities[0].position) - ordinary * 1.08).abs() < 0.00001);
    }

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
