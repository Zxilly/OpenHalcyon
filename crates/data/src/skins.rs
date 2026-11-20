use crate::GameData;
use halcyon_protocol::skin_hash;
use serde::Deserialize;
use sha2::Sha256;
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::Path,
};

#[derive(Deserialize)]
struct Catalog {
    heroes: Vec<Hero>,
    official_skins: Vec<OfficialSkin>,
}
#[derive(Deserialize)]
struct Hero {
    hero_id: String,
    default_skin: String,
    default_skin_hash_u32: u32,
    skins: Vec<Skin>,
}
#[derive(Deserialize)]
struct Skin {
    #[serde(rename = "skinKey")]
    name: String,
    skin_hash_u32: u32,
}
#[derive(Deserialize)]
struct OfficialSkin {
    #[serde(rename = "heroKey")]
    owner: String,
    #[serde(flatten)]
    skin: Skin,
}

#[derive(Debug)]
struct Choices {
    default: u32,
    advertised: BTreeSet<u32>,
}
#[derive(Debug)]
pub(crate) struct SkinIndex(BTreeMap<String, Choices>);

impl SkinIndex {
    pub(crate) fn load(
        dir: &Path,
        hash: &mut Sha256,
        heroes: &[String],
        skins: &[String],
    ) -> io::Result<Self> {
        let catalog = crate::read(dir, "heroes_and_skins.json", hash)?;
        Self::build(catalog, heroes, skins)
    }

    fn build(catalog: Catalog, heroes: &[String], skins: &[String]) -> io::Result<Self> {
        let invalid = || {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "inconsistent hero/skin relationship or hash in heroes_and_skins.json",
            )
        };
        let advertised: BTreeSet<_> = skins.iter().map(String::as_str).collect();
        let playable: BTreeSet<_> = heroes.iter().map(String::as_str).collect();
        let mut owners = BTreeMap::new();
        for record in &catalog.official_skins {
            let owner = record
                .owner
                .strip_prefix('*')
                .and_then(|h| h.strip_suffix('*'))
                .ok_or_else(invalid)?;
            if skin_hash(&record.skin.name) != record.skin.skin_hash_u32
                || owners.insert(record.skin.name.as_str(), owner).is_some()
            {
                return Err(invalid());
            }
        }
        let mut attached = BTreeSet::new();
        let mut index = BTreeMap::new();
        for hero in catalog.heroes {
            if skin_hash(&hero.default_skin) != hero.default_skin_hash_u32 {
                return Err(invalid());
            }
            if owners
                .get(hero.default_skin.as_str())
                .is_some_and(|owner| *owner != hero.hero_id)
            {
                return Err(invalid());
            }
            let mut allowed = BTreeSet::new();
            for skin in hero.skins {
                if owners.get(skin.name.as_str()).copied() != Some(hero.hero_id.as_str())
                    || skin_hash(&skin.name) != skin.skin_hash_u32
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
                        hero.hero_id,
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
        Ok(Self(index))
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
        }
    }
    fn catalog() -> Catalog {
        let names = ["Krul_DefaultSkin", "Krul_Skin_Cyber", "Krul_Skin_Pirate"];
        Catalog {
            heroes: vec![Hero {
                hero_id: "Hero009".into(),
                default_skin: names[0].into(),
                default_skin_hash_u32: skin_hash(names[0]),
                skins: names.map(skin).into(),
            }],
            official_skins: names
                .into_iter()
                .map(|s| OfficialSkin {
                    owner: "*Hero009*".into(),
                    skin: skin(s),
                })
                .collect(),
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
    #[ignore = "requires delivery and skin supplement files"]
    fn loaded_catalog_accepts_advertised_legacy_hero_skin() {
        let base = std::env::var_os("HALCYON_DELIVERY").unwrap();
        let extra = std::env::var_os("HALCYON_DATA_EXTRA").unwrap();
        let d = GameData::load_with_extra(base, Some(Path::new(&extra))).unwrap();
        assert_eq!(d.hero_names.len(), 57);
        assert_eq!(d.skin_names.len(), 218);
        for hero in &d.hero_names {
            assert!(d.indexed_skin(hero, None).is_some());
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
        assert_eq!(d.indexed_skin("Hero062", None), None);
    }
}
