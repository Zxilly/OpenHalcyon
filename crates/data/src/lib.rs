//! Read the separately supplied 4.13.4 delivery. Unknown fields retain their original names and values.
mod ability_parameter;
pub use ability_parameter::{AbilityParameter, DamageOperand, ModeledTalent, TalentParameter};
mod ability_slots;
pub use ability_slots::{
    AbilityEntryKind, AbilityForm, AbilitySetEntry, AbilitySlot, HeroAbilitySlots,
    all_hero_ability_slots, lookup_hero_ability_slots, lookup_hero_ability_slots_by_id,
    lookup_hero_ability_slots_by_raw,
};
pub mod combat_content;
pub use combat_content::TalentRarity;
mod difficulty;
mod entity_template;
mod extra;
mod item_bonuses;
pub mod items;
mod native_maps;
mod navmesh;
pub mod navmesh_polyanya;
pub use polyanya;
pub mod reference;
pub use native_maps::{
    Area, CampOrMode, CampSpawn, ExtractedParameter, LevelLayout, MeshHeader, MeshInfo, NativeMaps,
    PlacedCamp, Placement,
};
pub use navmesh::{NavMesh, NavMeshError, RouteSearch};
pub mod skins;
mod unit_rewards;
pub use difficulty::BotDifficulty;
pub use entity_template::EntityTemplate;
pub use extra::ExtraData;
pub use halcyon_protocol::ce::{KnownQueue, NativeMode};
pub use unit_rewards::SoloLaneRewards;

/// Matchmaking ID observed in the iOS-era ARAL requests.
pub const LEGACY_ARAL_QUEUE: &str = KnownQueue::CasualAral.as_str();
pub const THREE_PER_SIDE_SEATS: usize = 6;
pub const LEGACY_ARAL_SEATS: usize = THREE_PER_SIDE_SEATS;

/// Numeric ARAL entitlement per hero template. This never identifies a named talent.
pub type TalentLevels = BTreeMap<u16, std::num::NonZeroU8>;
pub const ARAL_TALENT_RULES_PREFIX: &str = "aral-talent-levels:";

/// Public talent triplets independently cross-checked against delivery entity template IDs.
pub struct AralTalentHero {
    pub template: u16,
    pub hero: &'static str,
    pub inventory_family: &'static str,
    pub asset_family: &'static str,
    pub definitions_by_rarity: [&'static str; 3],
}

pub const ARAL_TALENT_HEROES: [AralTalentHero; 25] = [
    AralTalentHero {
        template: 249,
        hero: "*Glaive*",
        inventory_family: "glaive",
        asset_family: "Glaive",
        definitions_by_rarity: ["ViolentAfterburn", "StunningBloodsong", "TwistedPursuit"],
    },
    AralTalentHero {
        template: 250,
        hero: "*SAW*",
        inventory_family: "saw",
        asset_family: "SAW",
        definitions_by_rarity: ["PiercingBullets", "SpeedySpinUp", "RoadRage"],
    },
    AralTalentHero {
        template: 254,
        hero: "*Hero009*",
        inventory_family: "krul",
        asset_family: "Krul",
        definitions_by_rarity: ["DeadMansBarrier", "HealingSmite", "CursedBlade"],
    },
    AralTalentHero {
        template: 255,
        hero: "*Hero010*",
        inventory_family: "skaarf",
        asset_family: "Skaarf",
        definitions_by_rarity: ["Splitfire", "FlashFire", "FlamingSludge"],
    },
    AralTalentHero {
        template: 257,
        hero: "*Ardan*",
        inventory_family: "ardan",
        asset_family: "Ardan",
        definitions_by_rarity: ["Vanguardian", "HeavyHitter", "KnockoutPunch"],
    },
    AralTalentHero {
        template: 258,
        hero: "*Vox*",
        inventory_family: "vox",
        asset_family: "Vox",
        definitions_by_rarity: ["DeepDash", "SoundBarrier", "EchoChamber"],
    },
    AralTalentHero {
        template: 260,
        hero: "*Hero016*",
        inventory_family: "rona",
        asset_family: "Rona",
        definitions_by_rarity: ["MOARRage", "HeroicBrawler", "FastWhirlwind"],
    },
    AralTalentHero {
        template: 265,
        hero: "*Skye*",
        inventory_family: "skye",
        asset_family: "Skye",
        definitions_by_rarity: ["EnhancedThrusters", "RainingDeath", "PiercingBarrage"],
    },
    AralTalentHero {
        template: 266,
        hero: "*Reim*",
        inventory_family: "reim",
        asset_family: "Reim",
        definitions_by_rarity: ["CreepingCold", "LongWinter", "Revenant"],
    },
    AralTalentHero {
        template: 268,
        hero: "*Alpha*",
        inventory_family: "alpha",
        asset_family: "Alpha",
        definitions_by_rarity: ["CoreOverclock", "PrimeTime", "Acceleration"],
    },
    AralTalentHero {
        template: 269,
        hero: "*Lyra*",
        inventory_family: "lyra",
        asset_family: "Lyra",
        definitions_by_rarity: ["TwinMissiles", "MobileBulwark", "GythianWard"],
    },
    AralTalentHero {
        template: 273,
        hero: "*Idris*",
        inventory_family: "idris",
        asset_family: "Idris",
        definitions_by_rarity: ["PathOfTheSeeker", "ChaChaChakram", "ShimmerHeal"],
    },
    AralTalentHero {
        template: 275,
        hero: "*Lance*",
        inventory_family: "lance",
        asset_family: "Lance",
        definitions_by_rarity: ["ReinforcedWalls", "Rollout", "Striker"],
    },
    AralTalentHero {
        template: 280,
        hero: "*Blackfeather*",
        inventory_family: "blackfeather",
        asset_family: "Blackfeather",
        definitions_by_rarity: ["HummingbirdHeart", "PointGuard", "Elusiveness"],
    },
    AralTalentHero {
        template: 285,
        hero: "*Celeste*",
        inventory_family: "celeste",
        asset_family: "Celeste",
        definitions_by_rarity: ["YoungStar", "CoreLeafClover", "GalaxyCollapse"],
    },
    AralTalentHero {
        template: 395,
        hero: "*Gwen*",
        inventory_family: "gwen",
        asset_family: "Gwen",
        definitions_by_rarity: ["SpeedLoader", "Skedaddling", "ThreeOfAKind"],
    },
    AralTalentHero {
        template: 399,
        hero: "*Baptiste*",
        inventory_family: "baptiste",
        asset_family: "Baptiste",
        definitions_by_rarity: ["BadBadMojo", "TheReaper", "ArmyOfSouls"],
    },
    AralTalentHero {
        template: 413,
        hero: "*Varya*",
        inventory_family: "varya",
        asset_family: "Varya",
        definitions_by_rarity: ["TalentA", "TalentB", "TalentC"],
    },
    AralTalentHero {
        template: 418,
        hero: "*Magnus*",
        inventory_family: "magnus",
        asset_family: "Magnus",
        definitions_by_rarity: ["TalentA", "TalentB", "TalentC"],
    },
    AralTalentHero {
        template: 436,
        hero: "*Ylva*",
        inventory_family: "ylva",
        asset_family: "Ylva",
        definitions_by_rarity: ["TalentA", "TalentB", "TalentC"],
    },
    AralTalentHero {
        template: 439,
        hero: "*Yates*",
        inventory_family: "yates",
        asset_family: "Yates",
        definitions_by_rarity: ["TalentA", "TalentB", "TalentC"],
    },
    AralTalentHero {
        template: 440,
        hero: "*Inara*",
        inventory_family: "inara",
        asset_family: "Inara",
        definitions_by_rarity: ["TalentA", "TalentB", "TalentC"],
    },
    AralTalentHero {
        template: 915,
        hero: "*Caine*",
        inventory_family: "caine",
        asset_family: "Caine",
        definitions_by_rarity: ["TalentA", "TalentB", "TalentC"],
    },
    AralTalentHero {
        template: 925,
        hero: "*Amael*",
        inventory_family: "amael",
        asset_family: "Amael",
        definitions_by_rarity: ["TalentA", "TalentB", "TalentC"],
    },
    AralTalentHero {
        template: 926,
        hero: "*Karas*",
        inventory_family: "karas",
        asset_family: "Karas",
        definitions_by_rarity: ["TalentA", "TalentB", "TalentC"],
    },
];

pub fn valid_talent_levels(mode: &str, seats: usize, grants: &[TalentLevels]) -> bool {
    grants.is_empty()
        || (mode == LEGACY_ARAL_QUEUE
            && seats == grants.len()
            && grants.iter().flatten().all(|(hero, _)| {
                ARAL_TALENT_HEROES
                    .iter()
                    .any(|entry| entry.template == *hero)
            }))
}

/// These exact queue IDs advertise three human seats on each team.
pub fn three_per_side(mode: &str) -> bool {
    matches!(
        KnownQueue::parse(mode),
        Some(KnownQueue::CasualAral | KnownQueue::Ranked)
    )
}

use serde::{Deserialize, de::DeserializeOwned};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, io, path::Path};

#[derive(Debug, Deserialize)]
pub struct Unit {
    pub name: String,
    pub hp: f32,
    pub energy: f32,
    pub weapon: f32,
    pub armor: f32,
    pub shield: f32,
    pub range: f32,
    pub attack_speed: f32,
    pub move_speed: f32,
    pub hp_per_level: f32,
    pub energy_per_level: f32,
    pub weapon_per_level: f32,
    pub armor_per_level: f32,
    pub shield_per_level: f32,
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct Item {
    pub id: String,
    pub name: String,
    pub cost_a: Option<u32>,
    pub cost_b: Option<u32>,
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct ManifestEntry {
    pub name: String,
    pub entity_type_id: u16,
    pub pair_index: u16,
    pub path: String,
}

#[derive(Debug)]
pub struct GameData {
    pub extra: ExtraData,
    pub units: Vec<Unit>,
    pub items: Vec<Item>,
    pub manifest: Vec<ManifestEntry>,
    pub rules: Vec<Value>,
    pub official_rules: Value,
    pub abilities: Vec<Value>,
    pub combat_constants: Value,
    pub content_digest: String,
    pub hero_names: Vec<String>,
    pub skin_names: Vec<String>,
}
impl GameData {
    pub fn references(&self) -> Option<&reference::References> {
        self.extra.references.as_ref()
    }
    pub fn load(dir: impl AsRef<Path>) -> io::Result<Self> {
        Self::load_with_extra(dir, None)
    }

    pub fn load_with_extra(dir: impl AsRef<Path>, extra: Option<&Path>) -> io::Result<Self> {
        let dir = dir.as_ref();
        let mut hash = Sha256::new();
        let mut catalog = read_catalog(dir, &mut hash)?;
        Ok(Self {
            units: read(dir, "units.json", &mut hash)?,
            items: read(dir, "items.json", &mut hash)?,
            manifest: read(dir, "entity_manifest.json", &mut hash)?,
            rules: read(dir, "rules.json", &mut hash)?,
            official_rules: read(dir, "official_rules.json", &mut hash)?,
            abilities: read(dir, "ability_raw.json", &mut hash)?,
            combat_constants: read(dir, "combat_constants.json", &mut hash)?,
            extra: match extra {
                Some(dir) => ExtraData::load(dir, &mut hash, &mut catalog)?,
                None => ExtraData::default(),
            },
            hero_names: catalog
                .as_mut()
                .map(|c| std::mem::take(&mut c.heroes))
                .unwrap_or_default(),
            skin_names: catalog.map(|c| c.skins).unwrap_or_default(),
            content_digest: format!("{:x}", hash.finalize()),
        })
    }
    /// Borrows an unambiguous rank table. Lookups preserve the hero and ability IDs.
    pub fn ability_ranks(&self, hero: &str, ability: &str, variable: &str) -> Option<&[f32]> {
        self.extra.ranks(hero, ability, variable)
    }
    pub fn item_catalog(&self) -> Option<&items::ItemCatalog> {
        self.extra.items.as_ref()
    }
    pub fn standard_5v5_items(&self) -> Option<&std::collections::BTreeSet<String>> {
        self.extra.standard_five()
    }
    pub fn supplemental_item_bonus(&self, item: &str, stat: u8) -> Option<f32> {
        self.extra.item_bonuses.get(item, stat)
    }
    pub fn unit(&self, name: &str) -> Option<&Unit> {
        self.units.iter().find(|v| v.name == name)
    }
    pub fn native_maps(&self) -> Option<&NativeMaps> {
        self.extra.native_maps.as_ref()
    }
    pub fn type_id(&self, name: &str) -> Option<u16> {
        self.manifest
            .iter()
            .find(|v| v.name.trim_matches('*') == name)
            .map(|v| v.entity_type_id)
    }
}

fn read<T: DeserializeOwned>(dir: &Path, name: &str, hash: &mut Sha256) -> io::Result<T> {
    let path = dir.join(name);
    let bytes = fs::read(&path)
        .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", path.display())))?;
    hash.update(name.as_bytes());
    hash.update((bytes.len() as u64).to_be_bytes());
    hash.update(&bytes);
    serde_json::from_slice(&bytes).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{}: {e}", path.display()),
        )
    })
}

#[derive(Default)]
struct DraftCatalog {
    heroes: Vec<String>,
    skins: Vec<String>,
}

fn read_catalog(dir: &Path, hash: &mut Sha256) -> io::Result<Option<DraftCatalog>> {
    use std::io::BufReader;
    let names: Vec<String> = match fs::File::open(dir.join("catalog.json")) {
        Ok(file) => serde_json::from_reader(BufReader::new(file))?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            match fs::File::open(dir.join("../match_decoded.jsonl")) {
                Ok(file) => catalog_from_trace(BufReader::new(file))?,
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(e),
            }
        }
        Err(e) => return Err(e),
    };
    let mut catalog = DraftCatalog::default();
    for name in names {
        validate_name(&name)?;
        hash.update([0x04, 0x53, name.len() as u8]);
        hash.update(name.as_bytes());
        if let Some(hero) = name.strip_prefix('*').and_then(|s| s.strip_suffix('*')) {
            catalog.heroes.push(hero.to_owned())
        } else {
            catalog.skins.push(name)
        }
    }
    Ok(Some(catalog))
}

fn validate_name(name: &str) -> io::Result<()> {
    if name.is_empty() || name.len() > 32 || !name.bytes().all(|c| c.is_ascii_graphic()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid 0x0453 catalog name",
        ));
    }
    Ok(())
}

/// Extract the names advertised in server-to-client 0x0453 records, in order.
pub fn catalog_from_trace(trace: impl io::BufRead) -> io::Result<Vec<String>> {
    #[derive(Deserialize)]
    enum Direction {
        #[serde(rename = "S2C")]
        ServerToClient,
        #[serde(other)]
        Other,
    }
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid 0x0453 record");
    let mut names = Vec::new();
    for line in trace.lines() {
        let row: Value = serde_json::from_str(&line?).map_err(io::Error::other)?;
        if !matches!(
            Direction::deserialize(&row["dir"]),
            Ok(Direction::ServerToClient)
        ) || row["opcode"].as_u64()
            != Some(u64::from(halcyon_protocol::Opcode::DraftCatalogEntry.raw()))
        {
            continue;
        }
        let hex = row["payload"].as_str().ok_or_else(invalid)?;
        let mut bytes = [0; 38];
        if hex.len() != bytes.len() * 2 {
            return Err(invalid());
        }
        data_encoding::HEXLOWER_PERMISSIVE
            .decode_mut(hex.as_bytes(), &mut bytes)
            .map_err(|_| invalid())?;
        let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
        let name = std::str::from_utf8(&bytes[..end]).map_err(|_| invalid())?;
        validate_name(name)?;
        names.push(name.to_owned());
    }
    Ok(names)
}
