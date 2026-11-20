use super::{Admin, Json, Path, Problem, Query, Result, State};
use axum::extract::rejection::{PathRejection, QueryRejection};
use halcyon_protocol::Opcode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use strum::{EnumIter, IntoEnumIterator};

#[derive(Clone, Copy, Deserialize, Serialize, EnumIter)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Collection {
    OpcodeRegistry,
    Heroes,
    Abilities,
    Units,
    Talents,
    Items,
    Stores,
    Skins,
    Themes,
    SkinAssets,
    LobbyManifests,
    Levels,
    Camps,
    Meshes,
    Languages,
    Messages,
    Assets,
    Opcodes,
    ReplayEntities,
    Stats,
    SessionConstants,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Installed {
    installed: bool,
    rules_content_digest: Option<String>,
    reference_digest: Option<String>,
    collections: Vec<Collection>,
}

pub(super) async fn installed(State(state): State<Admin>) -> Json<Installed> {
    let data = state.rooms.game_data();
    let reference = data.and_then(|data| data.references());
    Json(Installed {
        installed: reference.is_some(),
        rules_content_digest: data.map(|data| data.content_digest.clone()),
        reference_digest: reference.map(|r| r.digest.clone()),
        collections: Collection::iter()
            .filter(|collection| {
                reference.is_some() || matches!(collection, Collection::OpcodeRegistry)
            })
            .collect(),
    })
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct Filter {
    q: String,
    language: String,
    offset: usize,
    limit: usize,
}
impl Default for Filter {
    fn default() -> Self {
        Self {
            q: String::new(),
            language: "en".into(),
            offset: 0,
            limit: 20,
        }
    }
}

#[derive(Serialize)]
struct Entry {
    key: String,
    data: Value,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Page {
    items: Vec<Entry>,
    next_offset: Option<usize>,
}

impl Filter {
    fn page<K: AsRef<str>, T: Serialize>(
        &self,
        rows: impl IntoIterator<Item = (K, T)>,
    ) -> Result<Json<Page>> {
        self.page_matching(rows, |_, _| false)
    }

    fn page_matching<K: AsRef<str>, T: Serialize>(
        &self,
        rows: impl IntoIterator<Item = (K, T)>,
        matches: impl Fn(&T, &str) -> bool,
    ) -> Result<Json<Page>> {
        let needle = self.q.to_ascii_lowercase();
        let mut rows = rows
            .into_iter()
            .filter(|(key, data)| {
                needle.is_empty()
                    || key.as_ref().to_ascii_lowercase().contains(&needle)
                    || matches(data, &needle)
            })
            .skip(self.offset);
        let items = rows
            .by_ref()
            .take(self.limit)
            .map(|(key, data)| {
                Ok(Entry {
                    key: key.as_ref().to_owned(),
                    data: serde_json::to_value(data).map_err(|_| Problem::internal())?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let next_offset = rows
            .next()
            .and_then(|_| self.offset.checked_add(self.limit));
        Ok(Json(Page { items, next_offset }))
    }
}

pub(super) async fn browse(
    State(state): State<Admin>,
    collection: std::result::Result<Path<Collection>, PathRejection>,
    filter: std::result::Result<Query<Filter>, QueryRejection>,
) -> Result<Json<Page>> {
    let Path(collection) = collection.map_err(|_| Problem::invalid())?;
    let Query(f) = filter.map_err(|_| Problem::invalid())?;
    if !(1..=50).contains(&f.limit) || f.q.len() > 128 || f.language.len() > 32 {
        return Err(Problem::invalid());
    }
    if matches!(collection, Collection::OpcodeRegistry) {
        #[derive(Serialize)]
        struct RegisteredOpcode {
            name: Opcode,
            value: u16,
        }
        return f.page_matching(
            Opcode::ALL.iter().map(|&name| {
                let value = name.raw();
                (format!("0x{value:04X}"), RegisteredOpcode { name, value })
            }),
            |row, needle| {
                row.name.as_str().to_ascii_lowercase().contains(needle)
                    || row.value.to_string() == needle
            },
        );
    }
    let data = state.rooms.game_data().ok_or_else(Problem::not_found)?;
    let r = data.references().ok_or_else(Problem::not_found)?;
    let combat = data.extra.combat_content();
    let cosmetics = || data.cosmetics().ok_or_else(Problem::not_found);
    let maps = || data.native_maps().ok_or_else(Problem::not_found);
    let items = || data.item_catalog().ok_or_else(Problem::not_found);
    match collection {
        Collection::OpcodeRegistry => unreachable!("built-in collection handled above"),
        Collection::Heroes => {
            #[derive(Serialize)]
            struct HeroDetail<'a> {
                #[serde(flatten)]
                identity: &'a halcyon_data::skins::Hero,
                combat: Option<&'a halcyon_data::combat_content::HeroAbilities>,
            }
            f.page(cosmetics()?.heroes.iter().map(|hero| {
                (
                    &hero.hero_id,
                    HeroDetail {
                        identity: hero,
                        combat: data.hero_abilities(&hero.hero_id),
                    },
                )
            }))
        }
        Collection::Abilities => {
            #[derive(Serialize)]
            struct AbilityDetail<'a> {
                hero: &'a str,
                #[serde(flatten)]
                definition: &'a halcyon_data::combat_content::Ability,
                description: Option<halcyon_data::reference::Translation<'a>>,
            }
            let entries = combat.heroes().flat_map(|hero| {
                hero.abilities.iter().map(|ability| {
                    (
                        &ability.ability_id,
                        AbilityDetail {
                            hero: &hero.hero_id,
                            definition: ability,
                            description: ability
                                .desc_key
                                .as_deref()
                                .and_then(|key| r.translate(&f.language, key)),
                        },
                    )
                })
            });
            f.page_matching(entries, |detail, q| {
                [
                    detail.definition.name_en.as_deref(),
                    detail.definition.name_zh.as_deref(),
                ]
                .into_iter()
                .flatten()
                .any(|name| name.to_ascii_lowercase().contains(q))
            })
        }
        Collection::Units => f.page(combat.units().map(|u| (&u.name, u))),
        Collection::Talents => f.page(combat.talents().map(|t| (&t.id, t))),
        Collection::Items => f.page(items()?.items()),
        Collection::Stores => f.page(items()?.stores()),
        Collection::Skins => {
            #[derive(Serialize)]
            struct SkinDetail<'a> {
                #[serde(flatten)]
                definition: &'a halcyon_data::skins::OfficialSkin,
                skin_hash_u32: u32,
                skin_hash_hex: &'a str,
            }
            f.page(cosmetics()?.official_skins.iter().map(|s| {
                (
                    &s.skin_key,
                    SkinDetail {
                        definition: s,
                        skin_hash_u32: s.skin_hash_u32,
                        skin_hash_hex: &s.skin_hash_hex,
                    },
                )
            }))
        }
        Collection::Themes => f.page(
            cosmetics()?
                .official_themes
                .iter()
                .map(|t| (&t.theme_key, t)),
        ),
        Collection::SkinAssets => {
            f.page(cosmetics()?.cff0_skin_manifest.iter().map(|s| (&s.name, s)))
        }
        Collection::LobbyManifests => f.page(&cosmetics()?.lobby_manifests),
        Collection::Levels => f.page(&maps()?.levels),
        Collection::Camps => f.page(&maps()?.modes_and_camps),
        Collection::Meshes => f.page(maps()?.mesh_catalog().map(|mesh| (&mesh.source_path, mesh))),
        Collection::Languages => f.page(&r.languages),
        Collection::Messages => {
            let needle = f.q.to_ascii_lowercase();
            let mut keys = r
                .message_keys(&f.language)
                .filter(|key| needle.is_empty() || key.to_ascii_lowercase().contains(&needle))
                .skip(f.offset);
            let items = keys
                .by_ref()
                .take(f.limit)
                .filter_map(|key| {
                    r.translate(&f.language, key).map(|value| {
                        serde_json::to_value(value).map(|data| Entry {
                            key: key.into(),
                            data,
                        })
                    })
                })
                .collect::<std::result::Result<_, _>>()
                .map_err(|_| Problem::internal())?;
            Ok(Json(Page {
                items,
                next_offset: keys.next().and_then(|_| f.offset.checked_add(f.limit)),
            }))
        }
        Collection::Assets => f.page_matching(&r.assets, |asset, q| {
            asset
                .names
                .iter()
                .any(|n| n.to_ascii_lowercase().contains(q))
                || asset.obb_relpath.to_ascii_lowercase().contains(q)
        }),
        Collection::Opcodes => f.page(r.observations.values().map(|o| (&o.opcode_hex, o))),
        Collection::ReplayEntities => f.page(
            r.replay_entities
                .iter()
                .map(|(id, entity)| (id.to_string(), entity)),
        ),
        Collection::Stats => f.page([("stat_registry", &r.constants.stat_registry)]),
        Collection::SessionConstants => {
            #[derive(Serialize)]
            struct SessionSettings<'a> {
                constants: &'a halcyon_data::reference::ClientConstants,
                missed_state_update_messages_threshold: i32,
                failover_threshold: u32,
                enable_state_updates: bool,
                recorded_ping_site_count: usize,
            }
            let meta = &r.constants.official_session_meta;
            f.page([(
                "official_session_constants",
                SessionSettings {
                    constants: &r.constants.official_session_constants,
                    missed_state_update_messages_threshold: meta
                        .missed_state_update_messages_threshold,
                    failover_threshold: meta.failover_threshold,
                    enable_state_updates: meta.enable_state_updates,
                    recorded_ping_site_count: meta.ping_host_port_info.len(),
                },
            )])
        }
    }
}
