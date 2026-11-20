use anyhow::{Context, Result};
use halcyon_data::GameData;
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Report<'a> {
    rules_digest: String,
    content_digest: &'a str,
    draft_heroes: usize,
    draft_skins: usize,
    supplemental: Option<Supplement<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Supplement<'a> {
    reference_digest: &'a str,
    heroes: usize,
    abilities: usize,
    talents: usize,
    units: usize,
    items: usize,
    stores: usize,
    assets: usize,
    indexed_languages: usize,
    installed_dictionaries: usize,
    observed_opcodes: usize,
    spawn_snapshots: usize,
    levels: usize,
    camps_and_modes: usize,
    meshes: usize,
    tiles: usize,
}

pub(super) fn inspect(data: &GameData) -> Result<Report<'_>> {
    let rules = halcyon_sim::WorldRules::load(data).map_err(anyhow::Error::msg)?;
    let mut solo = rules.world(0, 1);
    for hero in &data.hero_names {
        solo.select_hero(0, hero, data)
            .map_err(anyhow::Error::msg)
            .with_context(|| format!("selecting advertised hero {hero}"))?;
    }
    solo.use_solo_mid_lane()
        .and_then(|()| solo.install_map_data(data))
        .map_err(anyhow::Error::msg)
        .context("preparing native Solo scene")?;
    let mut aral = rules.world(0, 6);
    aral.use_six_human_roster()
        .and_then(|()| aral.use_aral_lane())
        .and_then(|()| aral.install_map_data(data))
        .map_err(anyhow::Error::msg)
        .context("preparing native ARAL scene")?;
    let supplemental = if let Some(reference) = data.references() {
        let combat = data.extra.combat_content();
        let items = data
            .item_catalog()
            .context("supplement missing item catalog")?;
        let maps = data.native_maps();
        let mut tiles = 0;
        let mut meshes = 0;
        if let Some(maps) = maps {
            for path in maps.mesh_paths() {
                let mesh = maps
                    .mesh(path)
                    .with_context(|| format!("parsing {path}"))?
                    .context("indexed mesh missing")?;
                meshes += 1;
                tiles += mesh.tile_count();
            }
        }
        Some(Supplement {
            reference_digest: &reference.digest,
            heroes: combat.heroes().len(),
            abilities: combat.heroes().map(|hero| hero.abilities.len()).sum(),
            talents: combat.talents().len(),
            units: combat.units().len(),
            items: items.items().len(),
            stores: items.stores().len(),
            assets: reference.assets.len(),
            indexed_languages: reference.languages.len(),
            installed_dictionaries: reference
                .languages
                .keys()
                .filter(|lang| reference.dictionary_size(lang).is_some())
                .count(),
            observed_opcodes: reference.observations.len(),
            spawn_snapshots: reference.replay_entities.len(),
            levels: maps.map_or(0, |m| m.levels.len()),
            camps_and_modes: maps.map_or(0, |m| m.modes_and_camps.len()),
            meshes,
            tiles,
        })
    } else {
        None
    };
    Ok(Report {
        rules_digest: halcyon_sim::rules_digest(Some(data)),
        content_digest: &data.content_digest,
        draft_heroes: data.hero_names.len(),
        draft_skins: data.skin_names.len(),
        supplemental,
    })
}
