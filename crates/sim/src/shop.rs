#[path = "attack_items.rs"]
pub(super) mod attack_items;
#[path = "infusions.rs"]
pub(super) mod infusions;
#[path = "item_modifiers.rs"]
mod item_modifiers;
use super::*;
use halcyon_data::items::{ItemActiveParameter, ItemTuning};
use item_modifiers::EffectDelta;
pub use item_modifiers::NativeItemStat;
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
    MinionsFoot,
    LuckyStrike,
    TyrantsMonocle,
    Aegis,
    CapacitorPlate,
    CrystalInfusion,
    DragonbloodContract,
    Flare,
    FlareLoader,
    ProtectorContract,
    RooksDecree,
    ScoutPak,
    ScoutTrap,
    ScoutTuff,
    Shiversteel,
    SuperScout2000,
    TeleportBoots,
    TensionBow,
    TornadoTrigger,
    WeaponInfusion,
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
    bonuses: &'static [(NativeItemStat, f32)],
}

const fn def(
    kind: BasicItem,
    template: u32,
    name: &'static str,
    combine: u32,
    components: &'static [BasicItem],
    bonuses: &'static [(NativeItemStat, f32)],
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

use NativeItemStat::*;
const OFFERS: [Definition; 76] = [
    def(
        WeaponBlade,
        458,
        "Item_WeaponBlade",
        300,
        &[],
        &[(WeaponPower, 10.)],
    ),
    def(
        LightArmor,
        469,
        "Item_LightArmor",
        300,
        &[],
        &[(Armor, 25.)],
    ),
    def(
        LightShield,
        501,
        "Item_LightShield",
        300,
        &[],
        &[(Shield, 25.)],
    ),
    def(Oakheart, 467, "Item_Oakheart", 300, &[], &[(Health, 150.)]),
    def(
        EnergyBattery,
        472,
        "Item_EnergyBattery",
        300,
        &[],
        &[(Energy, 100.), (EnergyRegen, 1.5)],
    ),
    def(
        HeavySteel,
        505,
        "Item_HeavySteel",
        850,
        &[WeaponBlade],
        &[(WeaponPower, 45.)],
    ),
    def(
        SixSins,
        461,
        "Item_SixSins",
        350,
        &[WeaponBlade],
        &[(WeaponPower, 25.)],
    ),
    def(
        CrystalBit,
        459,
        "Item_CrystalBit",
        300,
        &[],
        &[(CrystalPower, 15.)],
    ),
    def(
        SwiftShooter,
        460,
        "Item_SwiftShooter",
        300,
        &[],
        &[(AttackSpeed, 0.1)],
    ),
    def(
        BlazingSalvo,
        463,
        "Item_BlazingSalvo",
        400,
        &[SwiftShooter],
        &[(AttackSpeed, 0.2)],
    ),
    def(
        Hourglass,
        473,
        "Item_Hourglass",
        250,
        &[],
        &[(CooldownSpeed, 0.075), (EnergyRegen, 0.25)],
    ),
    def(
        Chronograph,
        475,
        "Item_Chronograph",
        550,
        &[Hourglass],
        &[(CooldownSpeed, 0.15), (EnergyRegen, 0.75)],
    ),
    def(SprintBoots, 477, "Item_SprintBoots", 300, &[], &[]),
    def(
        TravelBoots,
        478,
        "Item_TravelBoots",
        350,
        &[SprintBoots],
        &[(Health, 100.)],
    ),
    def(
        AlternatingCurrent,
        509,
        "Item_AlternatingCurrent",
        1050,
        &[BlazingSalvo, HeavyPrism],
        &[(CrystalPower, 45.), (AttackSpeed, 0.4)],
    ),
    def(
        StormguardBanner,
        513,
        "Item_StormguardBanner",
        300,
        &[Oakheart],
        &[(Health, 150.)],
    ),
    def(
        EclipsePrism,
        462,
        "Item_EclipsePrism",
        350,
        &[CrystalBit],
        &[(CrystalPower, 30.0)],
    ),
    def(
        Sorrowblade,
        464,
        "Item_Sorrowblade",
        1300,
        &[SixSins, HeavySteel],
        &[(WeaponPower, 120.0)],
    ),
    def(
        Shatterglass,
        465,
        "Item_Shatterglass",
        1300,
        &[EclipsePrism, HeavyPrism],
        &[(CrystalPower, 130.0)],
    ),
    def(
        CoatOfPlates,
        470,
        "Item_CoatOfPlates",
        450,
        &[LightArmor],
        &[(Armor, 55.0)],
    ),
    def(
        MetalJacket,
        471,
        "Item_MetalJacket",
        1150,
        &[CoatOfPlates],
        &[(Armor, 95.0)],
    ),
    def(
        VoidBattery,
        474,
        "Item_VoidBattery",
        400,
        &[EnergyBattery],
        &[(Energy, 250.0), (EnergyRegen, 3.0)],
    ),
    def(
        Clockwork,
        476,
        "Item_Clockwork",
        900,
        &[Chronograph, VoidBattery],
        &[
            (CrystalPower, 30.0),
            (Energy, 400.0),
            (EnergyRegen, 5.0),
            (CooldownSpeed, 0.2),
        ],
    ),
    def(
        SerpentMask,
        479,
        "Item_SerpentMask",
        850,
        &[HeavySteel, BarbedNeedle],
        &[(WeaponPower, 70.0)],
    ),
    def(
        ReflexBlock,
        485,
        "Item_ReflexBlock",
        400,
        &[Oakheart],
        &[(Health, 150.0)],
    ),
    def(
        Frostburn,
        486,
        "Item_Frostburn",
        1000,
        &[HeavyPrism, EclipsePrism],
        &[(CrystalPower, 90.0)],
    ),
    def(
        FountainOfRenewal,
        487,
        "Item_FountainOfRenewal",
        500,
        &[Warmail, Lifespring],
        &[(Health, 400.0), (Armor, 40.0), (Shield, 40.0)],
    ),
    def(
        Crucible,
        488,
        "Item_Crucible",
        650,
        &[ReflexBlock, Dragonheart],
        &[(Health, 550.0)],
    ),
    def(
        JourneyBoots,
        489,
        "Item_JourneyBoots",
        750,
        &[TravelBoots],
        &[(Health, 150.0)],
    ),
    def(
        HalcyonChargers,
        490,
        "Item_HalcyonChargers",
        50,
        &[TravelBoots, VoidBattery],
        &[
            (Health, 150.0),
            (CooldownSpeed, 0.1),
            (Energy, 250.0),
            (EnergyRegen, 3.5),
        ],
    ),
    def(
        Aftershock,
        492,
        "Item_Aftershock",
        1150,
        &[Chronograph, EclipsePrism],
        &[
            (CrystalPower, 30.0),
            (EnergyRegen, 1.0),
            (CooldownSpeed, 0.15),
        ],
    ),
    def(
        WarTreads,
        497,
        "Item_WarTreads",
        300,
        &[Dragonheart, TravelBoots],
        &[(Health, 400.0)],
    ),
    def(
        AtlasPauldron,
        498,
        "Item_AtlasPauldron",
        950,
        &[CoatOfPlates],
        &[(Armor, 65.0)],
    ),
    def(
        BookOfEulogies,
        499,
        "Item_BookOfEulogies",
        300,
        &[],
        &[(WeaponPower, 5.0)],
    ),
    def(
        BarbedNeedle,
        500,
        "Item_BarbedNeedle",
        500,
        &[BookOfEulogies],
        &[(WeaponPower, 10.0)],
    ),
    def(
        KineticShield,
        502,
        "Item_KineticShield",
        450,
        &[LightShield],
        &[(Shield, 55.0)],
    ),
    def(
        Lifespring,
        504,
        "Item_Lifespring",
        500,
        &[Oakheart],
        &[(Health, 200.0)],
    ),
    def(
        BreakingPoint,
        507,
        "Item_BreakingPoint",
        850,
        &[HeavySteel, BlazingSalvo],
        &[(WeaponPower, 50.0), (AttackSpeed, 0.2)],
    ),
    def(
        EveOfHarvest,
        511,
        "Item_EveOfHarvest",
        850,
        &[VoidBattery, HeavyPrism],
        &[(CrystalPower, 60.0), (Energy, 300.0), (EnergyRegen, 4.0)],
    ),
    def(
        HeavyPrism,
        512,
        "Item_HeavyPrism",
        750,
        &[CrystalBit],
        &[(CrystalPower, 45.0)],
    ),
    def(
        Contraption,
        516,
        "Item_Contraption",
        700,
        &[Chronograph, Flaregun],
        &[(Health, 450.0), (EnergyRegen, 2.0), (CooldownSpeed, 0.2)],
    ),
    def(
        Flaregun,
        518,
        "Item_Flaregun",
        300,
        &[Oakheart],
        &[(Health, 300.0)],
    ),
    def(
        Stormcrown,
        519,
        "Item_Stormcrown",
        1000,
        &[StormguardBanner, Chronograph],
        &[(Health, 300.0), (EnergyRegen, 2.0), (CooldownSpeed, 0.2)],
    ),
    def(
        PoisonedShiv,
        520,
        "Item_PoisonedShiv",
        1250,
        &[BlazingSalvo, BarbedNeedle],
        &[(WeaponPower, 35.0), (AttackSpeed, 0.35)],
    ),
    def(
        Spellfire,
        522,
        "Item_Spellfire",
        1000,
        &[HeavyPrism, EclipsePrism],
        &[(CrystalPower, 80.0)],
    ),
    def(
        DragonsEye,
        523,
        "Item_DragonsEye",
        1300,
        &[EclipsePrism, HeavyPrism],
        &[(CrystalPower, 75.0)],
    ),
    def(
        Spellsword,
        524,
        "Item_Spellsword",
        200,
        &[HeavySteel, SixSins, Chronograph],
        &[
            (WeaponPower, 85.0),
            (CooldownSpeed, 0.2),
            (EnergyRegen, 2.0),
        ],
    ),
    def(
        SlumberingHusk,
        525,
        "Item_SlumberingHusk",
        600,
        &[KineticShield, CoatOfPlates],
        &[(Armor, 55.0), (Shield, 55.0)],
    ),
    def(
        Warmail,
        538,
        "Item_Warmail",
        200,
        &[LightArmor, LightShield],
        &[(Armor, 30.0), (Shield, 30.0)],
    ),
    def(
        SeraphicShell,
        539,
        "Item_SeraphicShell",
        450,
        &[KineticShield, ReflexBlock],
        &[(Shield, 95.0), (Health, 300.0)],
    ),
    def(
        Dragonheart,
        468,
        "Item_Dragonheart",
        350,
        &[Oakheart],
        &[(Health, 350.0)],
    ),
    def(
        PiercingSpear,
        506,
        "Item_PiercingSpear",
        600,
        &[WeaponBlade],
        &[(WeaponPower, 15.), (ArmorPierce, 0.1)],
    ),
    def(
        PiercingShard,
        510,
        "Item_PiercingShard",
        600,
        &[CrystalBit],
        &[(CrystalPower, 20.), (ShieldPierce, 0.1)],
    ),
    def(
        Bonesaw,
        482,
        "Item_Bonesaw",
        1300,
        &[BlazingSalvo, PiercingSpear],
        &[
            (WeaponPower, 30.),
            (AttackSpeed, 0.3),
            (ArmorPierce, 0.1),
            (ArmorPierce, 0.1),
        ],
    ),
    def(
        BrokenMyth,
        496,
        "Item_BrokenMyth",
        1350,
        &[PiercingShard, EclipsePrism],
        &[
            (CrystalPower, 50.),
            (ShieldPierce, 0.1),
            (ShieldPierce, 0.35),
        ],
    ),
    def(
        Pulseweave,
        533,
        "Item_Pulseweave",
        550,
        &[Dragonheart, Lifespring],
        &[(Health, 600.), (MoveSpeedAmplifier, 0.08)],
    ),
    def(
        MinionsFoot,
        517,
        "Item_MinionsFoot",
        300,
        &[],
        &[(CritChance, 0.10), (CritDamage, 0.05)],
    ),
    def(
        LuckyStrike,
        508,
        "Item_LuckyStrike",
        600,
        &[MinionsFoot],
        &[(CritChance, 0.20), (CritDamage, 0.05)],
    ),
    def(
        TyrantsMonocle,
        491,
        "Item_TyrantsMonocle",
        1350,
        &[SixSins, LuckyStrike],
        &[(WeaponPower, 60.), (CritChance, 0.35), (CritDamage, 0.15)],
    ),
    def(
        Aegis,
        503,
        "Item_Aegis",
        900,
        &[Warmail, ReflexBlock],
        &[(Health, 200.0), (Armor, 45.0), (Shield, 45.0)],
    ),
    def(
        CapacitorPlate,
        534,
        "Item_CapacitorPlate",
        550,
        &[Dragonheart, Warmail],
        &[
            (Health, 450.0),
            (Armor, 30.0),
            (Shield, 30.0),
            (CooldownSpeed, 0.15),
        ],
    ),
    def(CrystalInfusion, 494, "Item_CrystalInfusion", 500, &[], &[]),
    def(
        DragonbloodContract,
        536,
        "Item_DragonbloodContract",
        300,
        &[Oakheart],
        &[(Health, 250.0)],
    ),
    def(Flare, 481, "Item_Flare", 25, &[], &[]),
    def(
        FlareLoader,
        532,
        "Item_FlareLoader",
        300,
        &[],
        &[(Health, 150.0)],
    ),
    def(
        ProtectorContract,
        535,
        "Item_ProtectorContract",
        300,
        &[Oakheart],
        &[(Health, 300.0)],
    ),
    def(
        RooksDecree,
        531,
        "Item_RooksDecree",
        600,
        &[ProtectorContract, Warmail],
        &[
            (Health, 550.0),
            (Armor, 30.0),
            (Shield, 30.0),
            (CooldownSpeed, 0.05),
        ],
    ),
    def(
        ScoutPak,
        529,
        "Item_ScoutPak",
        250,
        &[Hourglass],
        &[(CooldownSpeed, 0.1), (EnergyRegen, 2.0)],
    ),
    def(ScoutTrap, 495, "Item_ScoutTrap", 50, &[], &[]),
    def(
        ScoutTuff,
        528,
        "Item_ScoutTuff",
        200,
        &[FlareLoader],
        &[(Health, 250.0)],
    ),
    def(
        Shiversteel,
        484,
        "Item_Shiversteel",
        1100,
        &[DragonbloodContract, BlazingSalvo],
        &[(Health, 600.0), (AttackSpeed, 0.2)],
    ),
    def(
        SuperScout2000,
        527,
        "Item_SuperScout2000",
        1000,
        &[ScoutTuff, ScoutPak],
        &[(Health, 450.0), (EnergyRegen, 4.5), (CooldownSpeed, 0.2)],
    ),
    def(
        TeleportBoots,
        530,
        "Item_TeleportBoots",
        750,
        &[TravelBoots],
        &[(Health, 150.0), (Shield, 15.0), (Armor, 15.0)],
    ),
    def(
        TensionBow,
        480,
        "Item_TensionBow",
        1350,
        &[SixSins, PiercingSpear],
        &[(WeaponPower, 40.0)],
    ),
    def(
        TornadoTrigger,
        466,
        "Item_TornadoTrigger",
        1200,
        &[BlazingSalvo, LuckyStrike],
        &[(AttackSpeed, 0.4), (CritChance, 0.35), (CritDamage, 0.05)],
    ),
    def(WeaponInfusion, 493, "Item_WeaponInfusion", 500, &[], &[]),
];

impl BasicItem {
    // The legacy/custom store has positive ARAL transaction evidence for these five additions.
    fn catalog_store_allows(
        self,
        settings: Option<halcyon_data::items::StoreSettings>,
    ) -> Option<bool> {
        use halcyon_data::items::StoreSettings::*;
        let allow = match self {
            Flare | ScoutTrap => matches!(settings, None | Some(HalcyonFold)),
            CrystalInfusion | WeaponInfusion => !matches!(settings, Some(BlitzRevamp)),
            FlareLoader | ScoutPak | ScoutTuff | SuperScout2000 | TeleportBoots => {
                matches!(settings, Some(Five | Practice))
            }
            CapacitorPlate => true,
            Aegis | DragonbloodContract | ProtectorContract | RooksDecree | Shiversteel
            | TensionBow | TornadoTrigger => settings.is_some(),
            _ => return None,
        };
        Some(allow)
    }
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

#[derive(Clone, Serialize)]
struct Offer {
    boot_move: f32,
    components: Vec<BasicItem>,
    refund: Option<u32>,
    standard_5v5: bool,
    combine: u32,
    bonuses: Vec<(NativeItemStat, f32)>,
}
#[derive(Clone, Serialize)]
pub(super) struct Shop {
    pub(super) vision: vision_devices::Rules,
    attacks: attack_items::Rules,
    infusions: [Option<infusions::Rule>; 2],
    flask: Option<FlaskRule>,
    sprint: Option<SprintRule>,
    offers: std::sync::Arc<[Offer]>,
    permitted: Vec<bool>,
}
impl Default for Shop {
    fn default() -> Self {
        Self {
            vision: Default::default(),
            attacks: attack_items::Rules::default(),
            infusions: infusions::Rule::DEFAULTS,
            flask: Some(FlaskRule::ARAL_STARTER),
            sprint: None,
            permitted: OFFERS
                .iter()
                .map(|d| d.kind.catalog_store_allows(None).unwrap_or(true))
                .collect(),
            offers: OFFERS
                .iter()
                .map(|d| Offer {
                    boot_move: 0.,
                    components: d.components.to_vec(),
                    refund: match d.kind {
                        Hourglass => Some(125),
                        BlazingSalvo => Some(350),
                        Stormcrown => Some(1200),
                        Aegis => Some(1200),
                        CapacitorPlate => Some(1000),
                        CrystalInfusion => Some(250),
                        DragonbloodContract => Some(300),
                        Flare => Some(15),
                        FlareLoader => Some(150),
                        ProtectorContract => Some(300),
                        RooksDecree => Some(1000),
                        ScoutPak => Some(250),
                        ScoutTrap => Some(25),
                        ScoutTuff => Some(250),
                        Shiversteel => Some(1200),
                        SuperScout2000 => Some(1000),
                        TeleportBoots => Some(700),
                        TensionBow => Some(1450),
                        TornadoTrigger => Some(1400),
                        WeaponInfusion => Some(250),
                        _ => None,
                    },
                    standard_5v5: !matches!(d.kind, Flare | ScoutTrap),
                    combine: d.combine,
                    bonuses: d.bonuses.to_vec(),
                })
                .collect(),
        }
    }
}
#[derive(Clone, Copy, Serialize)]
struct FlaskRule {
    duration: u64,
    cooldown: u64,
    barrier: f32,
    heal: f32,
    energy_fraction: f32,
}
impl FlaskRule {
    const ARAL_STARTER: Self = Self {
        duration: 5 * TICK_HZ,
        cooldown: 120 * TICK_HZ,
        barrier: 115.,
        heal: 200.,
        energy_fraction: 0.3,
    };
    fn load(item: &halcyon_data::items::FullItem) -> Option<Self> {
        let positive = |key| item.passive_value(key).filter(|&v| v > 0.);
        let energy_fraction = positive(ItemTuning::EnergyPercent.key()).filter(|&v| v <= 1.)?;
        Some(Self {
            duration: seconds_to_ticks(positive(ItemTuning::Duration.key())?)?,
            cooldown: seconds_to_ticks(
                item.fixed_active_seconds(ItemActiveParameter::Cooldown.key())?,
            )?,
            barrier: positive(ItemTuning::Barrier.key())?,
            heal: positive(ItemTuning::HealAmount.key())?,
            energy_fraction,
        })
    }
}
fn seconds_to_ticks(seconds: f32) -> Option<u64> {
    (seconds > 0. && seconds <= 86400.).then(|| (seconds * TICK_HZ as f32).ceil() as u64)
}
#[derive(Serialize, Clone, Copy)]
struct SprintRule {
    speed: f32,
    duration: u64,
    cooldown: u64,
}
impl SprintRule {
    fn load(item: &halcyon_data::items::FullItem) -> Option<Self> {
        Some(Self {
            speed: item
                .passive_value(ItemTuning::Sprint.key())
                .filter(|&v| v > 0.)?,
            duration: seconds_to_ticks(item.passive_value(ItemTuning::SprintDuration.key())?)?,
            cooldown: seconds_to_ticks(
                item.fixed_active_seconds(ItemActiveParameter::Cooldown.key())?,
            )?,
        })
    }
}
impl Shop {
    #[cfg(test)]
    pub(super) fn shares_offers_with(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.offers, &other.offers)
    }

    pub(super) fn bot_item(
        &self,
        inventory_size: usize,
        gold: u32,
        solo: bool,
    ) -> Option<BasicItem> {
        let preferred = match inventory_size {
            1 if self.sprint.is_some() => SprintBoots,
            2 => LightArmor,
            _ => WeaponBlade,
        };
        [preferred, WeaponBlade, LightArmor, LightShield, Oakheart]
            .into_iter()
            .find(|&item| {
                let offer = &self.offers[item as usize];
                self.permitted[item as usize]
                    && (!solo || offer.standard_5v5)
                    && offer.combine > 0
                    && offer.combine <= gold
            })
    }

    fn boot_speed(&self, inventory: &[HeldItem]) -> f32 {
        inventory
            .iter()
            .map(|i| self.offers[i.kind as usize].boot_move)
            .fold(0., f32::max)
    }
    pub fn load(data: &GameData) -> Result<Self, String> {
        let mut shop = Self {
            vision: vision_devices::Rules::load(data.item_catalog())?,
            attacks: attack_items::Rules::load(data.item_catalog()),
            infusions: data
                .item_catalog()
                .map_or(infusions::Rule::DEFAULTS, |catalog| {
                    [WeaponInfusion, CrystalInfusion].map(|kind| {
                        infusions::Rule::load(catalog.item(kind.definition().name)?, kind)
                    })
                }),
            flask: data
                .item_catalog()
                .and_then(|c| c.item("Item_HealingFlask"))
                .and_then(FlaskRule::load)
                .or_else(|| {
                    if data.item_catalog().is_none() {
                        Some(FlaskRule::ARAL_STARTER)
                    } else {
                        None
                    }
                }),
            ..Self::default()
        };
        if let Some(names) = data.standard_5v5_items() {
            shop.set_standard_5v5(names);
        }
        for d in &OFFERS {
            let offer = &mut std::sync::Arc::make_mut(&mut shop.offers)[d.kind as usize];
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
            if let Some(catalog) = data.item_catalog() {
                let Some(full) = catalog.item(d.name) else {
                    offer.combine = 0;
                    continue;
                };
                if matches!(
                    d.kind,
                    SprintBoots
                        | TravelBoots
                        | JourneyBoots
                        | HalcyonChargers
                        | WarTreads
                        | TeleportBoots
                ) {
                    offer.boot_move = full
                        .passive_value(ItemTuning::Move.key())
                        .filter(|&v| v > 0.)
                        .unwrap_or_default();
                }
                if d.kind == SprintBoots {
                    shop.sprint = SprintRule::load(full);
                }
                offer.combine = full.cost_a.unwrap_or_default();
                offer.refund = full.cost_b;
                let mut recipe = Vec::with_capacity(full.components.len());
                if full.components.len() > 8 || !(1..=3).contains(&full.tier) {
                    return Err(format!("invalid recipe shape for {}", d.name));
                }
                for name in &full.components {
                    let Some(component) = OFFERS.iter().find(|c| c.name == name) else {
                        offer.combine = 0;
                        break;
                    };
                    if catalog
                        .item(name)
                        .is_none_or(|row| row.tier >= full.tier || row.tier == 0)
                    {
                        return Err(format!("invalid recipe tier for {}", d.name));
                    }
                    recipe.push(component.kind);
                }
                if offer.combine != 0 {
                    offer.components = recipe;
                }
            }
            let mut split_component = false;
            for (stat, value) in &mut offer.bonuses {
                let Some(column) = stat.catalog_column() else {
                    if let Some(catalog) = data.item_catalog()
                        && let Some(v) = item_modifiers::passive_operand(
                            catalog,
                            d.kind,
                            *stat,
                            split_component,
                        )?
                    {
                        *value = v;
                    }
                    split_component = true;
                    continue;
                };
                if let Some(full) =
                    data.supplemental_item_bonus(d.name, column as u8)
                        .or_else(|| {
                            data.item_catalog()?
                                .item(d.name)?
                                .flat_equipment_stat(column)
                        })
                {
                    *value = full;
                    continue;
                }
                if let Some(b) = row
                    .other
                    .get("bonuses")
                    .and_then(|v| v.as_array())
                    .and_then(|rows| {
                        rows.iter()
                            .find(|b| b["stat_id"].as_u64() == Some(u64::from(column as u8)))
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
        // Validate the merged operator and fallback graph before recursive pricing.
        fn valid_recipe(shop: &Shop, item: BasicItem, remaining: u8) -> bool {
            let parts = &shop.offers[item as usize].components;
            parts.is_empty()
                || (remaining > 0
                    && parts
                        .iter()
                        .all(|&part| valid_recipe(shop, part, remaining - 1)))
        }
        if OFFERS.iter().any(|d| !valid_recipe(&shop, d.kind, 2)) {
            return Err("item recipes exceed three tiers".into());
        }
        Ok(shop)
    }

    fn set_standard_5v5(&mut self, names: &std::collections::BTreeSet<String>) {
        for (offer, definition) in std::sync::Arc::make_mut(&mut self.offers)
            .iter_mut()
            .zip(&OFFERS)
        {
            offer.standard_5v5 = names.contains(definition.name);
        }
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
            + self.offers[item as usize]
                .components
                .iter()
                .map(|&part| self.cost(part))
                .sum::<u64>()
    }

    fn consume_branch(&self, kind: BasicItem, held: &[HeldItem], consumed: &mut Vec<HeldItem>) {
        if let Some(&owned) = held
            .iter()
            .find(|h| h.kind == kind && !consumed.contains(h))
        {
            consumed.push(owned);
        } else {
            for &part in &self.offers[kind as usize].components {
                self.consume_branch(part, held, consumed);
            }
        }
    }
}

impl World {
    pub(super) fn acquisition_value(&self, item: BasicItem) -> u64 {
        self.shop.cost(item)
    }
    pub fn item_modifiers(
        &self,
        item: BasicItem,
    ) -> impl Iterator<Item = (NativeItemStat, f32)> + '_ {
        self.shop.offers[item as usize].bonuses.iter().copied()
    }
    pub fn item_bonuses(&self, item: BasicItem) -> impl Iterator<Item = (u8, f32)> + '_ {
        self.item_modifiers(item)
            .map(|(stat, value)| (stat as u8, value))
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
        if h.kind != Kind::Hero || !self.at_item_shop(h) {
            return Err("outside base shop");
        }
        let offer = &self.shop.offers[item as usize];
        if !self.shop.permitted[item as usize]
            || offer.combine == 0
            || (matches!(self.layout, Layout::SoloMid { .. }) && !offer.standard_5v5)
        {
            return Err("item unavailable");
        }
        let mut consumed = Vec::new();
        for &part in &self.shop.offers[item as usize].components {
            self.shop.consume_branch(part, &h.inventory, &mut consumed);
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
        let aral_inventory = if let Some(slots) = &h.aral_inventory {
            let Some(next) = slots.with_purchase(&consumed, granted) else {
                return Ok(None);
            };
            Some(next)
        } else {
            None
        };
        self.next_item = self.next_item.checked_add(1).ok_or("item instance limit")?;
        let h = &mut self.entities[i];
        h.inventory.retain(|owned| !consumed.contains(owned));
        if let Some(prepared) = aral_inventory {
            *h.aral_inventory
                .as_deref_mut()
                .expect("prepared ARAL inventory") = prepared;
        }
        // Apply the net modifier once. Upgrading an HP component must preserve
        // missing health when the completed item grants the same HP.
        let mut delta = EffectDelta::default();
        for used in &consumed {
            for &(stat, value) in &self.shop.offers[used.kind as usize].bonuses {
                delta.add(stat, -value);
            }
        }
        for &(stat, value) in &self.shop.offers[item as usize].bonuses {
            delta.add(stat, value);
        }
        apply_item_delta(h, delta);
        h.gold -= price as u32;
        h.inventory.push(granted);
        attack_items::refresh(h, &self.shop);
        h.item_stats.boot_speed = self.shop.boot_speed(&h.inventory);
        Ok(Some(Purchase {
            actor: h.id,
            paid: price as u32,
            granted,
            consumed,
        }))
    }
}

fn apply_item_delta(h: &mut Entity, delta: EffectDelta) {
    h.bonus_hp += delta.health;
    h.max_hp += delta.health;
    h.hp = (h.hp + delta.health).clamp(0., h.max_hp);
    h.max_energy += delta.energy;
    h.energy = (h.energy + delta.energy).clamp(0., h.max_energy);
    h.item_stats.energy_regen += delta.energy_regen;
    h.item_stats.bonus_weapon += delta.weapon_power;
    h.stats.weapon += delta.weapon_power;
    h.item_stats.crystal += delta.crystal_power;
    h.stats.armor += delta.armor;
    h.stats.shield += delta.shield;
    h.item_stats.shield_pierce += delta.shield_pierce;
    h.item_stats.move_speed_amp += delta.move_speed_amplifier;
    h.item_stats.attack_speed += delta.attack_speed;
    h.item_stats.cooldown += delta.cooldown_speed;
    h.item_stats.crit_chance += delta.crit_chance;
    h.item_stats.crit_damage += delta.crit_damage;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemSale {
    pub actor: u32,
    pub sold: HeldItem,
    pub refund: u32,
    pub balance: u32,
}
impl World {
    pub(super) fn flask_available(&self, index: usize) -> bool {
        self.shop.flask.is_some() && self.tick >= self.entities[index].item_stats.flask_ready
    }
    pub(super) fn flask_instance(&self, index: usize) -> Option<u32> {
        self.entities[index]
            .aral_inventory
            .as_ref()?
            .slots()
            .iter()
            .flatten()
            .find(|item| item.template == AralStarter::HealingFlask.grant().template)
            .map(|item| item.instance)
    }
    /// Activate the ARAL starter instance in this actor's own native slots.
    pub fn activate_healing_flask(
        &mut self,
        actor: u32,
        instance: u32,
    ) -> Result<(), &'static str> {
        let rule = self.shop.flask.ok_or("flask definition unavailable")?;
        let index = self.index(actor).ok_or("unknown actor")?;
        if self.flask_instance(index) != Some(instance) {
            return Err("flask not held");
        }
        let h = &mut self.entities[index];
        if self.winner.is_some()
            || h.kind != Kind::Hero
            || h.hp <= 0.
            || self.tick < h.item_stats.flask_ready
        {
            return Err("cannot activate flask");
        }
        h.item_stats.flask_ready = self.tick.saturating_add(rule.cooldown);
        h.status.flask_until = self.tick.saturating_add(rule.duration);
        h.status.flask_barrier = rule.barrier;
        h.status.flask_heal_per_tick = rule.heal / rule.duration as f32;
        h.status.flask_energy_per_tick = h.max_energy * rule.energy_fraction / rule.duration as f32;
        self.pending_item_events
            .push(Event::FlaskActivated { actor, instance });
        Ok(())
    }
    pub fn activate_sprint_boots(&mut self, actor: u32, instance: u32) -> Result<(), &'static str> {
        let rule = self.shop.sprint.ok_or("sprint definition unavailable")?;
        let index = self.index(actor).ok_or("unknown actor")?;
        let h = &mut self.entities[index];
        if self.winner.is_some()
            || h.kind != Kind::Hero
            || h.hp <= 0.
            || self.tick < h.item_stats.sprint_ready
            || !h
                .inventory
                .iter()
                .any(|i| i.instance == instance && i.kind == SprintBoots)
        {
            return Err("cannot activate sprint");
        }
        h.item_stats.sprint_speed = rule.speed;
        h.item_stats.sprint_until = self.tick.saturating_add(rule.duration);
        h.item_stats.sprint_ready = self.tick.saturating_add(rule.cooldown);
        Ok(())
    }

    pub fn use_catalog_store(&mut self, settings: halcyon_data::items::StoreSettings) {
        for (permitted, definition) in self.shop.permitted.iter_mut().zip(&OFFERS) {
            if let Some(allowed) = definition.kind.catalog_store_allows(Some(settings)) {
                *permitted = allowed;
            }
        }
    }
    pub fn use_item_store(&mut self, entries: &[String]) {
        for (permitted, definition) in self.shop.permitted.iter_mut().zip(&OFFERS) {
            *permitted = entries.iter().any(|entry| entry == definition.name);
        }
    }

    /// Sell a held instance at base. Captures establish the completed sale.
    pub fn sell_item(&mut self, actor: u32, instance: u32) -> Result<ItemSale, &'static str> {
        let index = self.index(actor).ok_or("unknown actor")?;
        let h = &self.entities[index];
        if self.winner.is_some() || h.hp <= 0. || h.kind != Kind::Hero || !self.at_item_shop(h) {
            return Err("outside base shop");
        }
        let offset = h
            .inventory
            .iter()
            .position(|i| i.instance == instance)
            .ok_or("item not held")?;
        let sold = h.inventory[offset];
        let offer = &self.shop.offers[sold.kind as usize];
        let refund = offer.refund.ok_or("sale price unavailable")?;
        let balance = h.gold.checked_add(refund).ok_or("gold limit")?;
        let mut delta = EffectDelta::default();
        for &(stat, value) in &offer.bonuses {
            delta.add(stat, -value);
        }
        let h = &mut self.entities[index];
        if let Some(slots) = &mut h.aral_inventory {
            slots.remove_held(sold)?;
        }
        h.inventory.remove(offset);
        attack_items::refresh(h, &self.shop);
        h.item_stats.boot_speed = self.shop.boot_speed(&h.inventory);
        h.gold = balance;
        apply_item_delta(h, delta);
        let sale = ItemSale {
            actor,
            sold,
            refund,
            balance,
        };
        self.pending_item_events.push(Event::ItemSold(sale));
        Ok(sale)
    }
}
#[derive(Clone, Copy, Default, Debug, Serialize)]
pub struct ItemStats {
    attack_passives: attack_items::Equipped,
    pub(super) bonus_weapon: f32,
    infusion: Option<infusions::Active>,
    flask_ready: u64,
    pub(super) boot_speed: f32,
    pub(super) sprint_speed: f32,
    pub(super) sprint_until: u64,
    pub(super) sprint_ready: u64,
    pub crit_chance: f32,
    pub crit_damage: f32,
    pub armor_pierce: f32,
    pub shield_pierce: f32,
    pub move_speed_amp: f32,
    pub crystal: f32,
    pub attack_speed: f32,
    pub energy_regen: f32,
    pub cooldown: f32,
}
/// One SplitMix64 draw per eligible released weapon attack. The top 24 bits
/// map uniformly onto the f32 grid in [0, 1). This is the host's RNG policy.
pub(super) fn weapon_multiplier(state: &mut u64, stats: ItemStats) -> f32 {
    if stats.crit_chance <= 0. {
        return 1.;
    }
    *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut bits = *state;
    bits = (bits ^ (bits >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    bits = (bits ^ (bits >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    let roll = ((bits ^ (bits >> 31)) >> 40) as f32 / 16_777_216.;
    if roll < stats.crit_chance.min(1.) {
        1.5 + stats.crit_damage
    } else {
        1.
    }
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
    fn injured_bot_activates_its_own_aral_starter_once() {
        let mut w = World::new(17, 6);
        w.use_six_human_roster().unwrap();
        w.use_aral_lane().unwrap();
        w.entities.truncate(1);
        w.entities[0].bot = true;
        w.entities[0].hp = 0.1 * w.entities[0].max_hp;
        let events = (0..100).flat_map(|_| w.step()).collect::<Vec<_>>();
        assert_eq!(
            events
                .iter()
                .filter(|ev| matches!(
                    ev,
                    Event::FlaskActivated {
                        actor: 1,
                        instance: 2000
                    }
                ))
                .count(),
            1
        );
        let activated = events
            .iter()
            .position(|event| matches!(event, Event::FlaskActivated { .. }))
            .unwrap();
        let healed = events
            .iter()
            .position(|event| {
                matches!(
                    event,
                    Event::Heal {
                        source: 1,
                        target: 1,
                        ..
                    }
                )
            })
            .unwrap();
        assert!(activated < healed);
    }
    #[test]
    fn flask_uses_current_actor_slot_absorbs_actual_damage_and_recovers_for_100_ticks() {
        let mut w = World::new(9, 6);
        w.use_six_human_roster().unwrap();
        w.use_aral_lane().unwrap();
        w.shop.flask = Some(FlaskRule {
            duration: 100,
            cooldown: 2400,
            barrier: 115.,
            heal: 200.,
            energy_fraction: 0.3,
        });
        w.entities.truncate(1);
        let e = &mut w.entities[0];
        e.position = Position { x: 60., y: 20. };
        e.max_hp = 1000.;
        e.hp = 50.;
        e.energy = 0.;
        assert!(
            w.command(1, Command::ActivateHealingFlask { instance: 2001 })
                .is_err()
        );
        w.command(1, Command::ActivateHealingFlask { instance: 2000 })
            .unwrap();
        assert!(
            w.command(1, Command::ActivateHealingFlask { instance: 2000 })
                .is_err()
        );
        let mut events = Vec::new();
        w.hurt(2, 1, 0, 125., DamageSchool::Weapon, &mut events);
        assert_eq!(w.entities[0].hp, 40.);
        let restored: f32 = (0..101)
            .flat_map(|_| w.step())
            .filter_map(|event| match event {
                Event::Heal {
                    source: 1,
                    target: 1,
                    amount,
                    ..
                } => Some(amount),
                _ => None,
            })
            .sum();
        assert_eq!(restored, 200.);
        assert!(w.entities[0].energy >= 0.3 * w.entities[0].max_energy);
        assert!(w.entities[0].item_stats.flask_ready > w.tick);
    }

    #[test]
    fn sprint_checks_held_instance_and_uses_server_cooldown_across_resale() {
        let mut w = World::new(5, 1);
        w.entities[0].gold = 2000;
        w.shop.sprint = Some(SprintRule {
            speed: 2.,
            duration: 40,
            cooldown: 3000,
        });
        std::sync::Arc::make_mut(&mut w.shop.offers)[SprintBoots as usize].boot_move = 0.3;
        std::sync::Arc::make_mut(&mut w.shop.offers)[SprintBoots as usize].refund = Some(150);
        let first = w.purchase(1, SprintBoots).unwrap().granted;
        let second = w.purchase(1, SprintBoots).unwrap().granted;
        assert_eq!(w.entities[0].item_stats.boot_speed, 0.3);
        assert!(w.activate_sprint_boots(2, first.instance).is_err());
        w.command(
            1,
            Command::ActivateSprintBoots {
                instance: first.instance,
            },
        )
        .unwrap();
        assert_eq!(w.entities[0].item_stats.sprint_until, 40);
        w.command(
            1,
            Command::Sell {
                instance: first.instance,
            },
        )
        .unwrap();
        assert_eq!(w.entities[0].item_stats.boot_speed, 0.3);
        assert!(w.activate_sprint_boots(1, second.instance).is_err());
        w.tick = 3000;
        assert!(w.activate_sprint_boots(1, first.instance).is_err());
        w.activate_sprint_boots(1, second.instance).unwrap();
        w.sell_item(1, second.instance).unwrap();
        assert_eq!(w.entities[0].item_stats.boot_speed, 0.);
    }

    #[test]
    fn sales_require_ownership_and_remove_the_instance_and_its_stats_once() {
        let mut w = World::new(5, 6);
        w.use_six_human_roster().unwrap();
        w.use_aral_lane().unwrap();
        std::sync::Arc::make_mut(&mut w.shop.offers)[Hourglass as usize].refund = Some(125);
        let receipt = w.purchase(1, Hourglass).unwrap();
        let held = receipt.granted;
        assert!(w.sell_item(2, held.instance).is_err());
        let balance = w.entities[0].gold;
        let sale = w.sell_item(1, held.instance).unwrap();
        assert_eq!(
            (sale.balance, sale.sold, sale.refund),
            (balance + 125, held, 125)
        );
        assert_eq!(w.entities[0].item_stats.cooldown, 0.);
        assert!(
            w.entities[0]
                .aral_inventory
                .as_ref()
                .unwrap()
                .slots()
                .iter()
                .flatten()
                .all(|s| s.instance != held.instance)
        );
        assert!(w.sell_item(1, held.instance).is_err());
        assert_eq!(w.entities[0].gold, balance + 125);
    }

    #[test]
    #[ignore = "requires the operator's delivery and supplement directories"]
    fn original_supplement_supplies_verified_nonflat_item_bonuses() {
        let delivery = std::env::var_os("HALCYON_DELIVERY").expect("HALCYON_DELIVERY");
        let extra = std::env::var_os("HALCYON_DATA_EXTRA").expect("HALCYON_DATA_EXTRA");
        let mut data =
            GameData::load_with_extra(delivery, Some(std::path::Path::new(&extra))).unwrap();
        // A legacy operator extension may supply the CFF0 attack-range column.
        // Neither the wire selector 18 nor Pulseweave's movement may read it.
        let pulse_row = data
            .items
            .iter_mut()
            .find(|r| r.id == Pulseweave.definition().name)
            .unwrap();
        pulse_row
            .other
            .get_mut("bonuses")
            .unwrap()
            .as_array_mut()
            .unwrap()
            .push(serde_json::from_str(r#"{"stat_id":18,"value":123.0}"#).unwrap());
        let shop = Shop::load(&data).unwrap();
        assert_eq!(shop.attacks, attack_items::Rules::default());
        assert_eq!(shop.vision, vision_devices::Rules::default());
        let catalog = data.item_catalog().unwrap();
        assert_eq!((catalog.items().len(), catalog.stores().len()), (93, 5));
        assert_eq!(
            catalog
                .item("Item_SprintBoots")
                .unwrap()
                .fixed_active_seconds(ItemActiveParameter::Cooldown.key()),
            Some(150.)
        );
        for (item, modifiers) in [
            (Pulseweave, vec![(Health, 600.), (MoveSpeedAmplifier, 0.08)]),
            (PiercingSpear, vec![(WeaponPower, 15.), (ArmorPierce, 0.1)]),
            (
                PiercingShard,
                vec![(CrystalPower, 20.), (ShieldPierce, 0.1)],
            ),
            (
                Bonesaw,
                vec![
                    (WeaponPower, 30.),
                    (AttackSpeed, 0.3),
                    (ArmorPierce, 0.1),
                    (ArmorPierce, 0.1),
                ],
            ),
            (
                BrokenMyth,
                vec![
                    (CrystalPower, 50.),
                    (ShieldPierce, 0.1),
                    (ShieldPierce, 0.35),
                ],
            ),
        ] {
            assert_eq!(shop.offers[item as usize].bonuses, modifiers);
        }
        assert_eq!(shop.sprint.unwrap().duration, 40);
        assert_eq!(shop.offers[Hourglass as usize].refund, Some(125));
        for (kind, refund) in [(BlazingSalvo, 350), (Stormcrown, 1200)] {
            assert_eq!(shop.offers[kind as usize].refund, Some(refund));
        }
        use halcyon_data::items::ItemStore;
        assert_eq!(OFFERS.len(), 76);
        for item in &OFFERS {
            let row = catalog.item(item.name).unwrap();
            assert!(row.cost_a.is_some_and(|cost| cost > 0));
            assert!(!row.buyable_in_stores.is_empty());
            for (settings, store) in [
                (StoreSettings::Five, ItemStore::Standard5v5),
                (StoreSettings::HalcyonFold, ItemStore::Standard3v3),
                (StoreSettings::BlitzRevamp, ItemStore::Blitz3v3),
            ] {
                if let Some(allowed) = item.kind.catalog_store_allows(Some(settings)) {
                    assert_eq!(
                        allowed,
                        row.buyable_in_stores.iter().any(|s| s == store.key()),
                        "{}",
                        item.name
                    );
                    assert_eq!(shop.offers[item.kind as usize].refund, row.cost_b);
                }
            }
        }
        assert_eq!(catalog.item("Item_ScoutTrap").unwrap().cost_b, Some(25));
        use halcyon_data::items::StoreSettings;
        for settings in [
            StoreSettings::Five,
            StoreSettings::HalcyonFold,
            StoreSettings::BlitzRevamp,
        ] {
            assert!(
                data.store_for_settings(settings.key())
                    .is_some_and(|items| !items.is_empty())
            );
        }

        for (kind, stat, value) in [
            (SwiftShooter, 15, 0.1),
            (Hourglass, 25, 0.075),
            (EnergyBattery, 3, 1.5),
            (MinionsFoot, 30, 0.1),
            (MinionsFoot, 31, 0.05),
        ] {
            assert_eq!(
                data.supplemental_item_bonus(kind.definition().name, stat),
                Some(value)
            );
            assert!(
                shop.offers[kind as usize]
                    .bonuses
                    .iter()
                    .any(|&(id, v)| id as u8 == stat && v == value)
            );
        }
    }

    #[test]
    fn upgraded_components_remove_their_loaded_attack_speed_exactly_once() {
        let mut world = World::new(37, 1);
        let actor = world.entities[0].id;
        world.entities[0].gold = 10_000;
        std::sync::Arc::make_mut(&mut world.shop.offers)[SwiftShooter as usize].bonuses =
            vec![(AttackSpeed, 0.125)];
        std::sync::Arc::make_mut(&mut world.shop.offers)[BlazingSalvo as usize].bonuses =
            vec![(AttackSpeed, 0.375)];
        let before = world.entities[0].item_stats.attack_speed;
        let component = world.purchase(actor, SwiftShooter).unwrap();
        assert_eq!(world.entities[0].item_stats.attack_speed, before + 0.125);
        let upgrade = world.purchase(actor, BlazingSalvo).unwrap();
        assert_eq!(upgrade.consumed, [component.granted]);
        assert_eq!(world.entities[0].item_stats.attack_speed, before + 0.375);
        assert_eq!(
            world.item_bonuses(BlazingSalvo).collect::<Vec<_>>(),
            [(15, 0.375)]
        );
    }

    #[test]
    fn standard_store_filters_supported_solo_purchases_before_consuming_anything() {
        let names = [
            WeaponBlade.definition().name.to_owned(),
            "Item_Cheater".to_owned(),
        ]
        .into();
        let mut world = World::new(1, 1);
        world.use_solo_mid_lane().unwrap();
        world.shop.set_standard_5v5(&names);
        world.entities[0].gold = 20_000;
        let id = world.entities[0].id;
        let blade = world.purchase(id, WeaponBlade).unwrap();
        let balance = world.entities[0].gold;
        assert_eq!(world.try_purchase(id, HeavySteel), Err("item unavailable"));
        assert_eq!(world.entities[0].inventory, [blade.granted]);
        assert_eq!(world.entities[0].gold, balance);
        world.shop.set_standard_5v5(&Default::default());
        assert_eq!(world.try_purchase(id, WeaponBlade), Err("item unavailable"));
        for aral in [false, true] {
            let mut other = World::new(2, if aral { 6 } else { 1 });
            if aral {
                other.use_six_human_roster().unwrap();
                other.use_aral_lane().unwrap();
            } else {
                other.use_solo_mid_lane().unwrap();
            }
            if aral {
                other.shop.set_standard_5v5(&Default::default());
            }
            let actor = other.entities[0].id;
            assert!(other.purchase(actor, WeaponBlade).is_ok());
        }
    }

    #[test]
    fn expanded_catalog_trades_owned_components_and_enforces_store_membership() {
        use halcyon_data::items::StoreSettings;
        let mut world = World::new(1, 1);
        world.entities[0].gold = 10_000;
        world.use_catalog_store(StoreSettings::Five);
        assert_eq!(world.purchase(1, ScoutTrap), Err("item unavailable"));
        let scout = world.purchase(1, FlareLoader).unwrap();
        let tuff = world.purchase(1, ScoutTuff).unwrap();
        assert_eq!((scout.paid, tuff.paid), (300, 200));
        assert_eq!(tuff.consumed, vec![scout.granted]);
        let pak = world.purchase(1, ScoutPak).unwrap();
        let full = world.purchase(1, SuperScout2000).unwrap();
        assert_eq!((pak.paid, full.paid), (500, 1000));
        assert_eq!(full.consumed, vec![tuff.granted, pak.granted]);
        assert!(world.sell_item(1, tuff.granted.instance).is_err());
        assert_eq!(
            world.sell_item(1, full.granted.instance).unwrap().refund,
            1000
        );
        assert!(world.sell_item(1, full.granted.instance).is_err());
        world.use_catalog_store(StoreSettings::HalcyonFold);
        assert_eq!(world.purchase(1, FlareLoader), Err("item unavailable"));
        let trap = world.purchase(1, ScoutTrap).unwrap();
        assert_eq!(
            world.sell_item(1, trap.granted.instance).unwrap().refund,
            25
        );
        assert_eq!(BasicItem::from_template(549), None);
    }

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
    fn crit_recipes_credit_components_and_replace_their_modifiers() {
        let mut w = World::new(7, 10);
        w.entities[0].gold = 20_000;
        let foot = w.purchase(1, MinionsFoot).unwrap();
        let lucky = w.purchase(1, LuckyStrike).unwrap();
        assert_eq!((foot.paid, lucky.paid), (300, 600));
        assert_eq!(lucky.consumed, [foot.granted]);
        let sins = w.purchase(1, SixSins).unwrap();
        let monocle = w.purchase(1, TyrantsMonocle).unwrap();
        assert_eq!(monocle.consumed, [sins.granted, lucky.granted]);
        assert_eq!(monocle.paid, 1350);
        assert_eq!(w.entities[0].item_stats.crit_chance, 0.35);
        assert_eq!(w.entities[0].item_stats.crit_damage, 0.15);
        assert_eq!(w.purchase(1, LuckyStrike).unwrap().paid, 900);
        assert_eq!(w.purchase(1, TyrantsMonocle).unwrap().paid, 2000);
        assert_eq!(w.purchase(1, TyrantsMonocle).unwrap().paid, 2900);
        let stats = w.entities[0].item_stats;
        assert!(stats.crit_chance > 1.);
        for mut seed in 0..64 {
            assert!((weapon_multiplier(&mut seed, stats) - 1.95).abs() < 1e-5);
        }
    }

    #[test]
    fn critical_roll_is_seeded_and_only_drawn_on_a_released_hero_weapon_attack() {
        let run = |seed: u64, retire: bool| {
            let mut w = World::new(seed, 10);
            w.entities[0].gold = 2900;
            w.purchase(1, TyrantsMonocle).unwrap();
            let pos = Position { x: 0., y: 0. };
            w.entities[0].position = pos;
            w.entities[1].position = Position { x: 4., y: 0. };
            w.entities[0].stats.range = 6.;
            w.entities[1].stats.armor = 0.;
            w.command(1, Command::Attack(2)).unwrap();
            w.step();
            w.command(1, Command::Stop).unwrap();
            for _ in 0..5 {
                w.step();
            }
            assert_eq!(w.crit_rng, seed);
            w.command(1, Command::Attack(2)).unwrap();
            let raw = w.entities[0].stats.weapon;
            let mut expected_state = seed;
            let expected = raw * weapon_multiplier(&mut expected_state, w.entities[0].item_stats);
            while !w.step().iter().any(|v| {
                matches!(
                    v,
                    Event::Attack {
                        source: 1,
                        target: 2
                    }
                )
            }) {
                assert_eq!(w.crit_rng, seed);
                assert!(w.tick < 20);
            }
            assert_eq!(w.crit_rng, expected_state);
            w.command(1, Command::Stop).unwrap();
            if retire {
                w.entities[1].hp = 0.;
                w.entities[1].respawn = u64::MAX;
            }
            let hits: Vec<_> = (0..10)
                .flat_map(|_| w.step())
                .filter_map(|v| match v {
                    Event::Damage {
                        source: 1,
                        target: 2,
                        amount,
                        ..
                    } => Some(amount),
                    _ => None,
                })
                .collect();
            if retire {
                assert!(hits.is_empty());
            } else {
                assert_eq!(hits.len(), 1);
                assert!((hits[0] - expected).abs() < 0.001);
            }
            assert_eq!(w.crit_rng, expected_state);
            expected > raw
        };
        assert_eq!(run(3, false), run(3, true));
        assert_ne!(run(3, false), run(0, false));
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
        for idx in 0..10 {
            let e = &mut world.entities[idx];
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
        let before: Vec<_> = w.entities.iter().take(10).map(|e| e.gold).collect();
        let mut events = vec![];
        w.hurt(1, 0, idx, 1., DamageSchool::Weapon, &mut events);
        assert!(!events.iter().any(|e| matches!(e, Event::GoldAward { .. })));
        w.hurt(1, 0, idx, 100000., DamageSchool::Weapon, &mut events);
        let count = events.len();
        w.hurt(1, 0, idx, 100000., DamageSchool::Weapon, &mut events);
        assert_eq!(count, events.len());
        for (e, old) in w.entities.iter().take(10).zip(before) {
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
