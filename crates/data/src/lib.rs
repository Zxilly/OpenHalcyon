//! Read the separately supplied 4.13.4 delivery. Unknown fields retain their original names and values.
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

/// Only retain public server-to-client catalog names from the optional reference trace.
fn read_catalog(dir: &Path, hash: &mut Sha256) -> io::Result<(Vec<String>, Vec<String>)> {
    use std::io::{BufRead, BufReader};
    let trace = dir.join("../match_decoded.jsonl");
    let file = match fs::File::open(trace) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok((vec![], vec![])),
        Err(e) => return Err(e),
    };
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid 0x0453 catalog name");
    let mut heroes = Vec::new();
    let mut skins = Vec::new();
    for line in BufReader::new(file).lines() {
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
            .chunks_exact(2)
            .map(|pair| {
                u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).map_err(|_| invalid())
            })
            .collect::<io::Result<Vec<_>>>()?;
        let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
        let name = std::str::from_utf8(&bytes[..end]).map_err(|_| invalid())?;
        if name.is_empty() || !name.bytes().all(|c| c.is_ascii_graphic()) {
            return Err(invalid());
        }
        hash.update([0x04, 0x53, end as u8]);
        hash.update(name.as_bytes());
        if let Some(hero) = name.strip_prefix('*').and_then(|s| s.strip_suffix('*')) {
            heroes.push(hero.to_owned())
        } else {
            skins.push(name.to_owned())
        }
    }
    Ok((heroes, skins))
}
