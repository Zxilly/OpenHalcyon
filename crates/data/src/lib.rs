//! Read the separately supplied 4.13.4 delivery. Unknown fields retain their original names and values.
mod difficulty;
pub use difficulty::BotDifficulty;

/// Matchmaking ID observed in the iOS-era ARAL requests.
pub const LEGACY_ARAL_QUEUE: &str = "casual_aral";
pub const THREE_PER_SIDE_SEATS: usize = 6;
pub const LEGACY_ARAL_SEATS: usize = THREE_PER_SIDE_SEATS;

/// These exact queue IDs advertise three human seats on each team.
pub fn three_per_side(mode: &str) -> bool {
    matches!(mode, LEGACY_ARAL_QUEUE | "ranked")
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
    pub fn load(dir: impl AsRef<Path>) -> io::Result<Self> {
        let dir = dir.as_ref();
        let mut hash = Sha256::new();
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
        let (hero_names, skin_names) = read_catalog(dir, &mut hash)?;
        Ok(Self {
            hero_names,
            skin_names,
            units: read(dir, "units.json", &mut hash)?,
            items: read(dir, "items.json", &mut hash)?,
            manifest: read(dir, "entity_manifest.json", &mut hash)?,
            rules: read(dir, "rules.json", &mut hash)?,
            official_rules: read(dir, "official_rules.json", &mut hash)?,
            abilities: read(dir, "ability_raw.json", &mut hash)?,
            combat_constants: read(dir, "combat_constants.json", &mut hash)?,
            content_digest: format!("{:x}", hash.finalize()),
        })
    }
    pub fn unit(&self, name: &str) -> Option<&Unit> {
        self.units.iter().find(|v| v.name == name)
    }
    pub fn type_id(&self, name: &str) -> Option<u16> {
        self.manifest
            .iter()
            .find(|v| v.name.trim_matches('*') == name)
            .map(|v| v.entity_type_id)
    }
}

fn read_catalog(dir: &Path, hash: &mut Sha256) -> io::Result<(Vec<String>, Vec<String>)> {
    use std::io::BufReader;
    let names: Vec<String> = match fs::File::open(dir.join("catalog.json")) {
        Ok(file) => serde_json::from_reader(BufReader::new(file))?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            match fs::File::open(dir.join("../match_decoded.jsonl")) {
                Ok(file) => catalog_from_trace(BufReader::new(file))?,
                Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
                Err(e) => return Err(e),
            }
        }
        Err(e) => return Err(e),
    };
    let mut heroes = Vec::new();
    let mut skins = Vec::new();
    for name in names {
        validate_name(&name)?;
        hash.update([0x04, 0x53, name.len() as u8]);
        hash.update(name.as_bytes());
        if let Some(hero) = name.strip_prefix('*').and_then(|s| s.strip_suffix('*')) {
            heroes.push(hero.to_owned())
        } else {
            skins.push(name)
        }
    }
    Ok((heroes, skins))
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
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid 0x0453 record");
    let mut names = Vec::new();
    for line in trace.lines() {
        let row: Value = serde_json::from_str(&line?).map_err(io::Error::other)?;
        if row["dir"] != "S2C" || row["opcode"].as_u64() != Some(0x453) {
            continue;
        }
        let hex = row["payload"].as_str().ok_or_else(invalid)?;
        if hex.len() != 76 || !hex.is_ascii() {
            return Err(invalid());
        }
        let bytes = hex
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| {
                u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).map_err(|_| invalid())
            })
            .collect::<io::Result<Vec<_>>>()?;
        let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
        let name = std::str::from_utf8(&bytes[..end]).map_err(|_| invalid())?;
        validate_name(name)?;
        names.push(name.to_owned());
    }
    Ok(names)
}
