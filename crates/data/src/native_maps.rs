use crate::{NavMesh, read};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs, io,
    path::Path,
    sync::{Arc, OnceLock},
};

#[derive(Debug)]
pub struct NativeMaps {
    meshes: BTreeMap<String, MeshAsset>,
    pub levels: BTreeMap<String, LevelLayout>,
    pub modes_and_camps: BTreeMap<String, CampOrMode>,
}

#[derive(Debug)]
struct MeshAsset {
    info: MeshInfo,
    bytes: Box<[u8]>,
    parsed: OnceLock<Result<Arc<NavMesh>, crate::NavMeshError>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MeshInfo {
    pub file: String,
    pub source_path: String,
    pub obb_md5: String,
    pub size_bytes: usize,
    pub detour_header: MeshHeader,
    #[serde(default, skip_deserializing)]
    pub sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MeshHeader {
    pub tile_count: usize,
    pub origin_xyz: [f32; 3],
    pub tile_width: f32,
    pub tile_height: f32,
    pub max_tiles: usize,
    pub max_polys: usize,
    pub tile0_magic: String,
    pub tile0_version: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LevelLayout {
    pub level_id: String,
    pub navmesh: String,
    pub vismesh: Option<String>,
    pub spawn_points: Vec<Placement>,
    pub spawn_stages: Vec<Area>,
    pub turrets_and_structures: Vec<Placement>,
    pub static_entities: Vec<Placement>,
    pub brushes: Vec<Area>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Placement {
    pub name: String,
    pub team: Option<u8>,
    pub pos: [f32; 3],
    pub rot: [f32; 3],
    pub scale: [f32; 3],
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Area {
    pub name: String,
    pub team: Option<u8>,
    pub buff_recharge: Option<String>,
    pub buff_murder: Option<String>,
    pub polygon_xz: Vec<[f32; 2]>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CampOrMode {
    pub id: String,
    pub root_label: Option<String>,
    pub len: usize,
    pub local_spawns: Vec<CampSpawn>,
    pub entity_refs: Vec<String>,
    pub parameters: BTreeMap<String, ExtractedParameter>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CampSpawn {
    pub name: String,
    pub local_pos: [f32; 3],
}

#[derive(Debug, Serialize)]
pub struct PlacedCamp<'a> {
    pub placement: &'a Placement,
    pub definition: &'a CampOrMode,
}

impl PlacedCamp<'_> {
    /// World-space position for extracted camp offsets with axis-aligned
    /// transforms. The offset labels are retained verbatim.
    pub fn spawn_position(&self, spawn: &CampSpawn) -> Option<[f32; 3]> {
        let [0., yaw, 0.] = self.placement.rot else {
            return None;
        };
        let sign = match yaw {
            0. => 1.,
            180. | -180. => -1.,
            _ => return None,
        };
        let p = self.placement.pos;
        let scale = self.placement.scale;
        let [x, y, z] = spawn.local_pos;
        Some([
            p[0] + x * scale[0] * sign,
            p[1] + y * scale[1],
            p[2] + z * scale[2] * sign,
        ])
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExtractedParameter {
    #[serde(rename = "type")]
    pub kind: String,
    pub value: serde_json::Value,
}

impl NativeMaps {
    pub(crate) fn load(dir: &Path, hash: &mut Sha256) -> io::Result<Self> {
        let mut meshes = BTreeMap::new();
        for mut asset in read::<Vec<MeshInfo>>(dir, "maps/navmesh_index.json", hash)? {
            if asset.obb_md5.len() != 32 || !asset.obb_md5.bytes().all(|c| c.is_ascii_hexdigit()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid mesh OBB key",
                ));
            }
            let name = Path::new(&asset.file);
            if name.file_name() != Some(name.as_os_str())
                || name.extension().is_none_or(|e| e != "navmesh")
                || !asset
                    .file
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid mesh asset filename",
                ));
            }
            let relative = format!("maps/navmesh/{}", asset.file);
            let bytes = fs::read(dir.join(&relative))?.into_boxed_slice();
            if bytes.len() != asset.size_bytes {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("mesh size mismatch for {relative}"),
                ));
            }
            hash.update(relative.as_bytes());
            hash.update((bytes.len() as u64).to_be_bytes());
            hash.update(&bytes);
            NavMesh::check_index(&bytes, &asset.detour_header).map_err(|error| {
                io::Error::new(io::ErrorKind::InvalidData, format!("{relative}: {error}"))
            })?;
            asset.sha256 = format!("{:x}", Sha256::digest(&bytes));
            let key = asset.source_path.clone();
            let mesh = MeshAsset {
                info: asset,
                bytes,
                parsed: OnceLock::new(),
            };
            if meshes.insert(key, mesh).is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "duplicate mesh source path",
                ));
            }
        }
        let levels = unique(
            read::<Vec<LevelLayout>>(dir, "maps/level_layouts.json", hash)?,
            |l| &l.level_id,
        )?;
        for level in levels.values() {
            if !meshes.contains_key(&level.navmesh)
                || level
                    .vismesh
                    .as_ref()
                    .is_some_and(|key| !meshes.contains_key(key))
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unindexed mesh in level {}", level.level_id),
                ));
            }
        }
        let modes_and_camps = unique(
            read::<Vec<CampOrMode>>(dir, "maps/camps_and_modes.json", hash)?,
            |m| &m.id,
        )?;
        Ok(Self {
            meshes,
            levels,
            modes_and_camps,
        })
    }

    /// All indexed variants, including walk and visibility assets.
    pub fn mesh_paths(&self) -> impl Iterator<Item = &str> {
        self.meshes.keys().map(String::as_str)
    }

    pub fn mesh_catalog(&self) -> impl Iterator<Item = &MeshInfo> {
        self.meshes.values().map(|asset| &asset.info)
    }

    /// Parse on first query. The bytes are frozen when game data is loaded.
    pub fn mesh(&self, source_path: &str) -> io::Result<Option<Arc<NavMesh>>> {
        self.meshes
            .get(source_path)
            .map(|asset| {
                asset
                    .parsed
                    .get_or_init(|| NavMesh::parse(&asset.bytes).map(Arc::new))
                    .as_ref()
                    .cloned()
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
            })
            .transpose()
    }

    /// Returns the shared cached `polyanya::Mesh` for `source_path`, building it lazily on first query.
    pub fn polyanya_mesh(&self, source_path: &str) -> io::Result<Option<Arc<polyanya::Mesh>>> {
        let Some(mesh) = self.mesh(source_path)? else {
            return Ok(None);
        };
        mesh.polyanya()
            .cloned()
            .map(Some)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
    }

    pub fn solo(&self) -> Option<&LevelLayout> {
        self.levels.get("SovereignsRiseLevel_Base")
    }

    pub fn aral(&self) -> Option<&LevelLayout> {
        self.levels.get("TestingGrounds_ARAL")
    }

    pub fn placed_camps<'a>(
        &'a self,
        level: &'a LevelLayout,
    ) -> impl Iterator<Item = PlacedCamp<'a>> {
        level.static_entities.iter().filter_map(|placement| {
            let key = &placement.name;
            if !["NeutralCamp", "BuffCamp", "BossCamp"]
                .iter()
                .any(|prefix| key.starts_with(prefix))
            {
                return None;
            }
            let definition = self.modes_and_camps.get(key).or_else(|| {
                let (base, side) = key.split_once('_')?;
                matches!(side, "TopLeft" | "TopRight" | "BotLeft" | "BotRight")
                    .then(|| self.modes_and_camps.get(base))
                    .flatten()
            })?;
            (!definition.local_spawns.is_empty()).then_some(PlacedCamp {
                placement,
                definition,
            })
        })
    }
}

fn unique<T>(values: Vec<T>, key: impl Fn(&T) -> &String) -> io::Result<BTreeMap<String, T>> {
    let mut index = BTreeMap::new();
    for value in values {
        if index.insert(key(&value).clone(), value).is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "duplicate map metadata ID",
            ));
        }
    }
    Ok(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires the operator's local server-data supplement"]
    fn loads_supplement_and_can_query_every_indexed_variant() {
        let dir = std::path::PathBuf::from(std::env::var_os("HALCYON_DATA_EXTRA").unwrap());
        let mut data = NativeMaps::load(&dir, &mut Sha256::new()).unwrap();
        assert_eq!(data.mesh_paths().count(), 16);
        for path in data.mesh_paths() {
            assert!(data.mesh(path).unwrap().is_some(), "{path}");
        }
        let camps: Vec<_> = data.placed_camps(data.solo().unwrap()).collect();
        assert_eq!(camps.len(), 14);
        assert!(camps.iter().all(|camp| {
            camp.definition
                .local_spawns
                .iter()
                .all(|spawn| camp.spawn_position(spawn).is_some())
        }));
        let asset = data.meshes.values_mut().next().unwrap();
        asset.info.detour_header.origin_xyz[0] += 1.;
        assert!(NavMesh::check_index(&asset.bytes, &asset.info.detour_header).is_err());
    }
}
