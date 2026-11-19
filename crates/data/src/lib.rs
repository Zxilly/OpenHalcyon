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
        Ok(Self {
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
