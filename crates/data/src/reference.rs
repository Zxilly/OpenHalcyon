//! The operator's CE dictionaries, asset index, protocol observations and session settings.
use halcyon_types::skin_hash;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io, path::Path};

#[derive(Debug, Deserialize, Serialize)]
pub struct AssetEntry {
    pub obb_relpath: String,
    pub md5: String,
    pub names: Vec<String>,
    pub v7_size: u32,
    pub ptch_count: u32,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Language {
    pub lang: String,
    pub obb_relpath: String,
    pub md5: String,
    pub char_count: u32,
    pub entry_count: u32,
    pub sample_title: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub enum TrafficDirection {
    #[serde(rename = "C->S")]
    ClientToServer,
    #[serde(rename = "S->C")]
    ServerToClient,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ReadSideTag {
    pub size: Option<u32>,
    pub reader: Option<String>,
    pub notes: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct OpcodeObservation {
    pub opcode_dec: u16,
    pub opcode_hex: String,
    pub vgr_name: Option<String>,
    pub directions: Vec<TrafficDirection>,
    pub count_aral_3matches: u64,
    pub count_vgr_5v5: u64,
    pub observed_sizes: BTreeMap<usize, u64>,
    pub read_side_tag: Option<ReadSideTag>,
    // Samples come from an operator's recordings and may contain player information.
    #[serde(skip_serializing)]
    pub sample_hex: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ReplayEntity {
    pub eid: u32,
    pub entity_type_id: u16,
    pub entity_name: String,
    pub skin_hash_hex: String,
    pub skin_name: String,
    pub team: u8,
    pub payload_len: usize,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct StatRegistry {
    pub runtime_stat_ids: BTreeMap<u16, String>,
    pub runtime_stat_fields: BTreeMap<u16, String>,
    pub runtime_stat_synthesis_formula: String,
    pub ability_variable_formula: String,
    pub ability_variable_unit_types: BTreeMap<u16, String>,
    pub stat_label_enum_0_to_200: BTreeMap<u16, String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientFeatureFlags {
    pub leaderboards: bool,
    pub in_game_chat: bool,
    pub leaderboards_always_query_offline_friends: bool,
    pub live_events: bool,
}

/// Experiment names and settings are supplied by this particular content pack.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientConstants {
    pub features_enabled: ClientFeatureFlags,
    #[serde(flatten)]
    pub experiments: BTreeMap<String, BTreeMap<String, serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedPingSite {
    pub host: String,
    pub port: u16,
    pub site: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMeta {
    pub missed_state_update_messages_threshold: i32,
    pub failover_threshold: u32,
    pub enable_state_updates: bool,
    pub ping_host_port_info: Vec<RecordedPingSite>,
}

#[derive(Debug, Deserialize)]
pub struct ServerConstants {
    pub stat_registry: StatRegistry,
    pub official_session_constants: ClientConstants,
    pub official_session_meta: SessionMeta,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Translation<'a> {
    pub language: &'a str,
    pub value: &'a str,
}

#[derive(Debug)]
pub struct References {
    pub constants: ServerConstants,
    pub assets: BTreeMap<String, AssetEntry>,
    pub languages: BTreeMap<String, Language>,
    dictionaries: BTreeMap<String, BTreeMap<String, String>>,
    pub observations: BTreeMap<u16, OpcodeObservation>,
    pub replay_entities: BTreeMap<u32, ReplayEntity>,
    pub digest: String,
}

fn invalid(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}

impl References {
    pub(crate) fn load(dir: &Path) -> io::Result<Self> {
        let mut hash = Sha256::new();
        let constants = crate::read(dir, "server_constants.json", &mut hash)?;
        let mut assets = BTreeMap::new();
        for row in crate::read::<Vec<AssetEntry>>(dir, "cff0_complete_index.json", &mut hash)? {
            if row.md5.len() != 32
                || !row.md5.bytes().all(|b| b.is_ascii_hexdigit())
                || assets.insert(row.md5.to_ascii_uppercase(), row).is_some()
            {
                return Err(invalid("invalid or duplicate CFF0 asset hash"));
            }
        }
        let mut languages = BTreeMap::new();
        for language in
            crate::read::<Vec<Language>>(dir, "localization/languages_index.json", &mut hash)?
        {
            if languages.insert(language.lang.clone(), language).is_some() {
                return Err(invalid("duplicate language index entry"));
            }
        }
        let mut dictionaries: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        for (lang, file) in [
            ("en", "localization/en.json"),
            ("zh_CN", "localization/zh_CN.json"),
        ] {
            let index = languages
                .get(lang)
                .ok_or_else(|| invalid("dictionary missing from language index"))?;
            let entries: BTreeMap<String, String> = crate::read(dir, file, &mut hash)?;
            if usize::try_from(index.entry_count).ok() != Some(entries.len()) {
                return Err(invalid(
                    "dictionary entry count differs from language index",
                ));
            }
            dictionaries.insert(lang.into(), entries);
        }
        let mut observations = BTreeMap::new();
        for row in crate::read::<Vec<OpcodeObservation>>(
            dir,
            "protocol_empirical/opcodes_observed.json",
            &mut hash,
        )? {
            let raw = row
                .opcode_hex
                .strip_prefix("0x")
                .and_then(|hex| u16::from_str_radix(hex, 16).ok());
            let total = row
                .count_aral_3matches
                .checked_add(row.count_vgr_5v5)
                .ok_or_else(|| invalid("empirical opcode count overflow"))?;
            if raw != Some(row.opcode_dec)
                || !row.sample_hex.len().is_multiple_of(2)
                || !row.sample_hex.bytes().all(|b| b.is_ascii_hexdigit())
                || row
                    .observed_sizes
                    .values()
                    .try_fold(0u64, |a, b| a.checked_add(*b))
                    != Some(total)
                || observations.insert(row.opcode_dec, row).is_some()
            {
                return Err(invalid("inconsistent empirical opcode entry"));
            }
        }
        let mut replay_entities = BTreeMap::new();
        for actor in crate::read::<Vec<ReplayEntity>>(
            dir,
            "protocol_empirical/replay_entities_5v5.json",
            &mut hash,
        )? {
            let hash = actor
                .skin_hash_hex
                .strip_prefix("0x")
                .and_then(|h| u32::from_str_radix(h, 16).ok());
            if hash != Some(skin_hash(&actor.skin_name))
                || replay_entities.insert(actor.eid, actor).is_some()
            {
                return Err(invalid("inconsistent replay entity identity or skin hash"));
            }
        }
        Ok(Self {
            constants,
            assets,
            dictionaries,
            languages,
            observations,
            replay_entities,
            digest: format!("{:x}", hash.finalize()),
        })
    }

    pub fn translate(&self, language: &str, key: &str) -> Option<Translation<'_>> {
        let normalized = if language.contains('-') {
            std::borrow::Cow::Owned(language.replace('-', "_"))
        } else {
            std::borrow::Cow::Borrowed(language)
        };
        let candidates = [normalized.as_ref(), normalized.split('_').next()?, "en"];
        candidates.into_iter().find_map(|locale| {
            let (language, dictionary) = self.dictionaries.get_key_value(locale)?;
            Some(Translation {
                language,
                value: dictionary.get(key)?,
            })
        })
    }

    pub fn dictionary_size(&self, language: &str) -> Option<usize> {
        self.dictionaries.get(language).map(BTreeMap::len)
    }

    pub fn message_keys(&self, language: &str) -> impl Iterator<Item = &str> {
        let english = &self.dictionaries["en"];
        english
            .keys()
            .chain(
                matches!(language, "zh_CN" | "zh-CN")
                    .then(|| &self.dictionaries["zh_CN"])
                    .into_iter()
                    .flat_map(|d| d.keys())
                    .filter(|k| !english.contains_key(*k)),
            )
            .map(String::as_str)
    }

    pub fn observed_known_opcodes(&self, mut is_known: impl FnMut(u16) -> bool) -> usize {
        self.observations
            .keys()
            .filter(|&&raw| is_known(raw))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires the operator's 147219 supplement"]
    fn installed_reference_pack_agrees_with_recordings_and_dictionary() {
        let path = std::env::var_os("HALCYON_DATA_EXTRA").expect("set HALCYON_DATA_EXTRA");
        let r = References::load(Path::new(&path)).unwrap();
        assert_eq!((r.assets.len(), r.languages.len()), (942, 14));
        assert_eq!(r.dictionary_size("en"), Some(7329));
        assert_eq!(r.dictionary_size("zh_CN"), Some(7328));
        assert_eq!(r.message_keys("zh-CN").count(), 7329);
        let key = "RETRIEVE_CREDENTIALS_DESCRIPTION";
        assert_eq!(r.translate("zh-CN", key).unwrap().language, "en");
        assert!(r.translate("zh_CN", "missing key").is_none());
        assert_eq!(r.observations.len(), 105);
        assert_eq!(
            r.observations
                .values()
                .map(|r| r.count_aral_3matches)
                .sum::<u64>(),
            176197
        );
        assert_eq!(
            r.observations
                .values()
                .map(|r| r.count_vgr_5v5)
                .sum::<u64>(),
            43160
        );
        assert_eq!(r.replay_entities.len(), 10);
    }
}
