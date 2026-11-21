use crate::GameData;
use halcyon_protocol::skin_hash;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    io,
    path::Path,
};

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Catalog {
    pub heroes: Vec<Hero>,
    pub official_skins: Vec<OfficialSkin>,
    pub official_themes: Vec<Theme>,
    pub cff0_skin_manifest: Vec<Skin>,
    pub lobby_manifests: BTreeMap<String, Vec<String>>,
    pub skin_hash_algorithm: String,
}
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Hero {
    pub hero_id: String,
    pub entity_type_id: Option<u16>,
    pub name_en: String,
    pub name_zh: Option<String>,
    pub role_en: Option<String>,
    pub tagline_en: Option<String>,
    pub default_skin: String,
    pub default_skin_hash_u32: u32,
    pub default_skin_hash_hex: String,
    pub skins: Vec<Skin>,
}
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Skin {
    #[serde(rename = "skinKey")]
    pub name: String,
    pub skin_hash_u32: u32,
    pub skin_hash_hex: String,
    pub tier: Option<u16>,
    #[serde(rename = "themeKey")]
    pub theme_key: Option<String>,
}
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Theme {
    pub theme_key: String,
    pub hero_key: String,
}
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct CardCost {
    #[serde(rename = "cardID")]
    pub card_id: String,
    pub amount: u32,
}
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficialSkin {
    #[serde(rename = "heroKey")]
    pub owner: String,
    pub skin_key: String,
    #[serde(rename = "skin_hash_u32", skip_serializing)]
    pub skin_hash_u32: u32,
    #[serde(rename = "skin_hash_hex", skip_serializing)]
    pub skin_hash_hex: String,
    pub image_name: String,
    pub tier: u16,
    pub priority: i32,
    pub theme_key: Option<String>,
    pub obtainable: bool,
    pub available_on_epoch: i64,
    pub advertise_coming_soon: bool,
    pub prereq: String,
    pub can_prorate: bool,
    pub single_card: bool,
    pub card_costs: Vec<CardCost>,
    pub allow_weave: bool,
    pub season_chest_unlock: bool,
    pub visible: bool,
    pub limited_edition_end_epoch: i64,
}

/// Default skin key in `KindredSkinManifest.def` for a hero identifier.
/// Four legacy internal hero identifiers map to their public skin prefix.
pub fn default_skin_name(hero: &str) -> Cow<'static, str> {
    match hero {
        "Hero009" => Cow::Borrowed("Krul_DefaultSkin"),
        "Hero010" => Cow::Borrowed("Skaarf_DefaultSkin"),
        "Hero016" => Cow::Borrowed("Rona_DefaultSkin"),
        "Sayoc" => Cow::Borrowed("Taka_DefaultSkin"),
        _ => Cow::Owned(format!("{hero}_DefaultSkin")),
    }
}

/// Authoritative client `lookup2` hash for a hero's default skin.
pub fn default_skin_hash(hero: &str) -> u32 {
    skin_hash(&default_skin_name(hero))
}

#[derive(Debug)]
struct Choices {
    default: u32,
    advertised: BTreeSet<u32>,
}

/// Pre-fix `lookup2` tail bug (bytes `8..10` placed at bits `0..23` instead of
/// `8..31`) present in older `heroes_and_skins.json` supplements for names with
/// `len % 12 >= 9`. Used only to validate legacy supplement files before
/// normalizing every stored hash to the true client `skin_hash`.
fn legacy_tail0_skin_hash(name: &str) -> u32 {
    const GOLDEN_RATIO: u32 = 0x9e37_79b9;
    const SKIN_SEED: u32 = 0x1234_5678;

    fn mix(a: &mut u32, b: &mut u32, c: &mut u32) {
        *a = a.wrapping_sub(*b).wrapping_sub(*c) ^ (*c >> 13);
        *b = b.wrapping_sub(*c).wrapping_sub(*a) ^ (*a << 8);
        *c = c.wrapping_sub(*a).wrapping_sub(*b) ^ (*b >> 13);
        *a = a.wrapping_sub(*b).wrapping_sub(*c) ^ (*c >> 12);
        *b = b.wrapping_sub(*c).wrapping_sub(*a) ^ (*a << 16);
        *c = c.wrapping_sub(*a).wrapping_sub(*b) ^ (*b >> 5);
        *a = a.wrapping_sub(*b).wrapping_sub(*c) ^ (*c >> 3);
        *b = b.wrapping_sub(*c).wrapping_sub(*a) ^ (*a << 10);
        *c = c.wrapping_sub(*a).wrapping_sub(*b) ^ (*b >> 15);
    }

    let (mut a, mut b, mut c) = (GOLDEN_RATIO, GOLDEN_RATIO, SKIN_SEED);
    let (chunks, rem) = name.as_bytes().as_chunks::<12>();
    let add =
        |acc: u32, bytes: &[u8]| acc.wrapping_add(u32::from_le_bytes(bytes.try_into().unwrap()));
    for chunk in chunks {
        a = add(a, &chunk[..4]);
        b = add(b, &chunk[4..8]);
        c = add(c, &chunk[8..]);
        mix(&mut a, &mut b, &mut c);
    }
    let mut tail = [0u8; 12];
    tail[..rem.len()].copy_from_slice(rem);
    a = add(a, &tail[..4]);
    b = add(b, &tail[4..8]);
    c = add(c.wrapping_add(name.len() as u32), &tail[8..]);
    mix(&mut a, &mut b, &mut c);
    c
}

fn validate_and_normalize_hash(name: &str, numeric: &mut u32, hex: &mut String) -> bool {
    let hex_matches = hex
        .strip_prefix("0x")
        .and_then(|value| u32::from_str_radix(value, 16).ok())
        == Some(*numeric);
    if !hex_matches {
        return false;
    }
    let canonical = skin_hash(name);
    let accepted =
        *numeric == canonical || (name.len() % 12 >= 9 && *numeric == legacy_tail0_skin_hash(name));
    if !accepted {
        return false;
    }
    if *numeric != canonical {
        *numeric = canonical;
        *hex = format!("{canonical:#010x}");
    }
    true
}
#[derive(Debug)]
pub(crate) struct SkinIndex(BTreeMap<String, Choices>, Catalog);

impl SkinIndex {
    pub(crate) fn load(
        dir: &Path,
        hash: &mut Sha256,
        draft: &mut Option<crate::DraftCatalog>,
    ) -> io::Result<Self> {
        let catalog: Catalog = crate::read(dir, "heroes_and_skins.json", hash)?;
        if draft.is_none() {
            // A public default skin identifies a selectable hero. Optional skins
            // have a separate advertisement contract in the original draft.
            let defaults: BTreeSet<_> = catalog
                .official_skins
                .iter()
                .filter(|s| s.visible && s.tier == 0)
                .map(|s| s.skin_key.as_str())
                .collect();
            let mut fallback = crate::DraftCatalog::default();
            for hero in &catalog.heroes {
                if hero.entity_type_id.is_some() && defaults.contains(hero.default_skin.as_str()) {
                    let entry = format!("*{}*", hero.hero_id);
                    crate::validate_name(&entry)?;
                    hash.update(
                        halcyon_protocol::Opcode::DraftCatalogEntry
                            .raw()
                            .to_be_bytes(),
                    );
                    hash.update([entry.len() as u8]);
                    hash.update(entry.as_bytes());
                    fallback.heroes.push(hero.hero_id.clone());
                }
            }
            *draft = Some(fallback);
        }
        let draft = draft
            .as_ref()
            .expect("explicit or supplemental draft catalog");
        Self::build(catalog, &draft.heroes, &draft.skins)
    }

    fn build(mut catalog: Catalog, heroes: &[String], skins: &[String]) -> io::Result<Self> {
        let invalid = || {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "inconsistent hero/skin relationship or hash in heroes_and_skins.json",
            )
        };
        let advertised: BTreeSet<_> = skins.iter().map(String::as_str).collect();
        let playable: BTreeSet<_> = heroes.iter().map(String::as_str).collect();
        for record in &mut catalog.official_skins {
            if !validate_and_normalize_hash(
                &record.skin_key,
                &mut record.skin_hash_u32,
                &mut record.skin_hash_hex,
            ) {
                return Err(invalid());
            }
        }
        let mut owners = BTreeMap::new();
        for record in &catalog.official_skins {
            let owner = record
                .owner
                .strip_prefix('*')
                .and_then(|h| h.strip_suffix('*'))
                .ok_or_else(invalid)?;
            if owners.insert(record.skin_key.as_str(), owner).is_some() {
                return Err(invalid());
            }
        }
        let mut attached = BTreeSet::new();
        let mut index = BTreeMap::new();
        for hero in &mut catalog.heroes {
            if !validate_and_normalize_hash(
                &hero.default_skin,
                &mut hero.default_skin_hash_u32,
                &mut hero.default_skin_hash_hex,
            ) {
                return Err(invalid());
            }
            if owners
                .get(hero.default_skin.as_str())
                .is_some_and(|owner| *owner != hero.hero_id)
            {
                return Err(invalid());
            }
            let mut allowed = BTreeSet::new();
            for skin in &mut hero.skins {
                if owners.get(skin.name.as_str()).copied() != Some(hero.hero_id.as_str())
                    || !validate_and_normalize_hash(
                        &skin.name,
                        &mut skin.skin_hash_u32,
                        &mut skin.skin_hash_hex,
                    )
                {
                    return Err(invalid());
                }
                if !attached.insert(skin.name.clone()) {
                    return Err(invalid());
                }
                if advertised.contains(skin.name.as_str()) {
                    allowed.insert(skin.skin_hash_u32);
                }
            }
            if playable.contains(hero.hero_id.as_str()) {
                // Preserve operator-advertised custom names without reassigning official skins.
                let prefix = format!("{}_", hero.hero_id);
                for name in &advertised {
                    if !owners.contains_key(name) && name.starts_with(&prefix) {
                        allowed.insert(skin_hash(name));
                    }
                }
                if index
                    .insert(
                        hero.hero_id.clone(),
                        Choices {
                            default: hero.default_skin_hash_u32,
                            advertised: allowed,
                        },
                    )
                    .is_some()
                {
                    return Err(invalid());
                }
            }
        }
        if attached.len() != owners.len() {
            return Err(invalid());
        }
        for entry in &mut catalog.cff0_skin_manifest {
            if !validate_and_normalize_hash(
                &entry.name,
                &mut entry.skin_hash_u32,
                &mut entry.skin_hash_hex,
            ) {
                return Err(invalid());
            }
        }
        Ok(Self(index, catalog))
    }

    fn select(&self, hero: &str, requested: Option<u32>) -> Option<u32> {
        let choices = self.0.get(hero)?;
        Some(
            requested
                .filter(|value| choices.advertised.contains(value))
                .unwrap_or(choices.default),
        )
    }
}

impl GameData {
    pub fn cosmetics(&self) -> Option<&Catalog> {
        self.extra.skins.as_ref().map(|index| &index.1)
    }
    /// Resolves a selection against the optional attached ownership and advertised draft catalog.
    pub fn indexed_skin(&self, hero: &str, requested: Option<u32>) -> Option<u32> {
        self.extra.skins.as_ref()?.select(hero, requested)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn skin(name: &str) -> Skin {
        Skin {
            name: name.into(),
            skin_hash_u32: skin_hash(name),
            skin_hash_hex: format!("{:#010x}", skin_hash(name)),
            ..Default::default()
        }
    }
    fn catalog() -> Catalog {
        let names = ["Krul_DefaultSkin", "Krul_Skin_Cyber", "Krul_Skin_Pirate"];
        Catalog {
            heroes: vec![Hero {
                hero_id: "Hero009".into(),
                default_skin: names[0].into(),
                default_skin_hash_u32: skin_hash(names[0]),
                default_skin_hash_hex: format!("{:#010x}", skin_hash(names[0])),
                skins: names.map(skin).into(),
                ..Default::default()
            }],
            official_skins: names
                .into_iter()
                .map(|s| OfficialSkin {
                    owner: "*Hero009*".into(),
                    skin_key: s.into(),
                    skin_hash_u32: skin_hash(s),
                    skin_hash_hex: format!("{:#010x}", skin_hash(s)),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }
    #[test]
    fn advertised_alias_and_custom_skin_require_the_selected_hero() {
        let names = ["Krul_Skin_Cyber".into(), "Hero009_Custom".into()];
        let index = SkinIndex::build(catalog(), &["Hero009".into()], &names).unwrap();
        let default = skin_hash("Krul_DefaultSkin");
        assert_eq!(index.select("Hero009", None), Some(default));
        for name in &names {
            assert_eq!(
                index.select("Hero009", Some(skin_hash(name))),
                Some(skin_hash(name))
            );
        }
        assert_eq!(
            index.select("Hero009", Some(skin_hash("Krul_Skin_Pirate"))),
            Some(default)
        );
        assert_eq!(index.select("Krul", None), None);
        assert_eq!(
            SkinIndex::build(catalog(), &[], &names)
                .unwrap()
                .select("Hero009", None),
            None
        );
    }
    #[test]
    fn corrupt_owner_or_hash_rejects_the_supplement() {
        let mut c = catalog();
        c.official_skins[1].owner = "*Ringo*".into();
        assert!(SkinIndex::build(c, &[], &[]).is_err());
        let mut c = catalog();
        c.heroes[0].default_skin_hash_u32 ^= 1;
        assert!(SkinIndex::build(c, &[], &[]).is_err());
    }

    #[test]
    fn normalizes_legacy_tail0_supplement_hashes_to_client_lookup2() {
        let legacy_skin = |name: &str| {
            let h = legacy_tail0_skin_hash(name);
            Skin {
                name: name.into(),
                skin_hash_u32: h,
                skin_hash_hex: format!("{h:#010x}"),
                ..Default::default()
            }
        };
        let legacy_official = |owner: &str, name: &str| {
            let h = legacy_tail0_skin_hash(name);
            OfficialSkin {
                owner: format!("*{owner}*"),
                skin_key: name.into(),
                skin_hash_u32: h,
                skin_hash_hex: format!("{h:#010x}"),
                ..Default::default()
            }
        };
        let mut c = Catalog {
            heroes: [
                ("Catherine", "Catherine_DefaultSkin"),
                ("Silvernail", "Silvernail_DefaultSkin"),
                ("Churnwalker", "Churnwalker_DefaultSkin"),
            ]
            .into_iter()
            .map(|(hero, default_skin)| {
                let h = legacy_tail0_skin_hash(default_skin);
                Hero {
                    hero_id: hero.into(),
                    default_skin: default_skin.into(),
                    default_skin_hash_u32: h,
                    default_skin_hash_hex: format!("{h:#010x}"),
                    skins: vec![legacy_skin(default_skin)],
                    ..Default::default()
                }
            })
            .collect(),
            official_skins: vec![
                legacy_official("Catherine", "Catherine_DefaultSkin"),
                legacy_official("Silvernail", "Silvernail_DefaultSkin"),
                legacy_official("Churnwalker", "Churnwalker_DefaultSkin"),
            ],
            cff0_skin_manifest: vec![legacy_skin("Churnwalker")],
            ..Default::default()
        };
        assert_ne!(
            c.heroes[2].default_skin_hash_u32,
            skin_hash("Churnwalker_DefaultSkin")
        );
        let playable = vec![
            "Catherine".into(),
            "Silvernail".into(),
            "Churnwalker".into(),
        ];
        let index = SkinIndex::build(c, &playable, &[]).unwrap();
        assert_eq!(index.select("Catherine", None), Some(0x62db_dc96));
        assert_eq!(index.select("Silvernail", None), Some(0xf4ec_ac3f));
        assert_eq!(index.select("Churnwalker", None), Some(0xc73e_ab15));
        assert_eq!(index.1.heroes[0].default_skin_hash_u32, 0x62db_dc96);
        assert_eq!(index.1.heroes[0].default_skin_hash_hex, "0x62dbdc96");
        assert_eq!(index.1.heroes[1].default_skin_hash_u32, 0xf4ec_ac3f);
        assert_eq!(index.1.heroes[1].default_skin_hash_hex, "0xf4ecac3f");
        assert_eq!(index.1.heroes[2].default_skin_hash_u32, 0xc73e_ab15);
        assert_eq!(index.1.heroes[2].default_skin_hash_hex, "0xc73eab15");
        assert_eq!(
            index.1.cff0_skin_manifest[0].skin_hash_u32,
            skin_hash("Churnwalker")
        );

        // Corrupting a len%12>=9 hash still fails validation.
        c = Catalog {
            heroes: vec![Hero {
                hero_id: "Churnwalker".into(),
                default_skin: "Churnwalker_DefaultSkin".into(),
                default_skin_hash_u32: 0xc2b9_fc85 ^ 1,
                default_skin_hash_hex: format!("{:#010x}", 0xc2b9_fc85u32 ^ 1),
                skins: vec![legacy_skin("Churnwalker_DefaultSkin")],
                ..Default::default()
            }],
            official_skins: vec![legacy_official("Churnwalker", "Churnwalker_DefaultSkin")],
            ..Default::default()
        };
        assert!(SkinIndex::build(c, &["Churnwalker".into()], &[]).is_err());
    }

    #[test]
    fn default_skin_helper_maps_aliased_and_long_tail_heroes() {
        for (hero, name, expected_hash) in [
            ("Hero009", "Krul_DefaultSkin", 0x9d1a_d5d3),
            ("Hero010", "Skaarf_DefaultSkin", 0x09ea_f99f),
            ("Hero016", "Rona_DefaultSkin", 0x7bd2_186c),
            ("Sayoc", "Taka_DefaultSkin", 0xf7a1_e642),
            ("Amael", "Amael_DefaultSkin", 0x2fd7_245d),
            ("Catherine", "Catherine_DefaultSkin", 0x62db_dc96),
            ("Silvernail", "Silvernail_DefaultSkin", 0xf4ec_ac3f),
            ("Churnwalker", "Churnwalker_DefaultSkin", 0xc73e_ab15),
        ] {
            assert_eq!(default_skin_name(hero), name);
            assert_eq!(default_skin_hash(hero), expected_hash);
        }
    }

    #[test]
    #[ignore = "requires the CE supplement and the independent original draft announcement"]
    fn standalone_supplement_advertises_exactly_the_original_playable_heroes() {
        let delivery = std::path::PathBuf::from(std::env::var_os("HALCYON_DELIVERY").unwrap());
        let extra = std::path::PathBuf::from(std::env::var_os("HALCYON_DATA_EXTRA").unwrap());
        let reference = crate::read_catalog(&delivery, &mut Sha256::new())
            .unwrap()
            .unwrap();
        let mut draft = None;
        let index = SkinIndex::load(&extra, &mut Sha256::new(), &mut draft).unwrap();
        let draft = draft.unwrap();
        assert_eq!(draft.heroes.len(), 57);
        assert_eq!(
            draft.heroes.iter().collect::<BTreeSet<_>>(),
            reference.heroes.iter().collect()
        );
        assert!(draft.skins.is_empty());
        for hero in &reference.heroes {
            assert_eq!(index.select(hero, None), Some(default_skin_hash(hero)));
        }
        let mut explicit = Some(crate::DraftCatalog::default());
        SkinIndex::load(&extra, &mut Sha256::new(), &mut explicit).unwrap();
        assert!(explicit.unwrap().heroes.is_empty());
    }
    #[test]
    #[ignore = "requires delivery and skin supplement files"]
    fn loaded_catalog_accepts_advertised_legacy_hero_skin() {
        let base = std::env::var_os("HALCYON_DELIVERY").unwrap();
        let extra = std::env::var_os("HALCYON_DATA_EXTRA").unwrap();
        let d = GameData::load_with_extra(base, Some(Path::new(&extra))).unwrap();
        assert_eq!(d.hero_names.len(), 57);
        assert_eq!(d.skin_names.len(), 218);
        for hero in &d.hero_names {
            assert_eq!(d.indexed_skin(hero, None), Some(default_skin_hash(hero)));
        }
        for skin in &d.skin_names {
            let hash = skin_hash(skin);
            assert_eq!(
                d.hero_names
                    .iter()
                    .filter(|name| d.indexed_skin(name, Some(hash)) == Some(hash))
                    .count(),
                1
            );
        }
        assert_eq!(
            d.indexed_skin("Hero009", None),
            Some(skin_hash("Krul_DefaultSkin"))
        );
        assert_eq!(d.indexed_skin("Catherine", None), Some(0x62db_dc96));
        assert_eq!(d.indexed_skin("Silvernail", None), Some(0xf4ec_ac3f));
        assert_eq!(d.indexed_skin("Churnwalker", None), Some(0xc73e_ab15));
        assert_eq!(d.indexed_skin("Hero062", None), None);
    }
}
