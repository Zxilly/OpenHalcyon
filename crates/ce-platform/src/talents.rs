use super::{Error, Platform};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// Local inventory balances for one authenticated player's talent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct TalentProgress {
    pub level: u32,
    pub fragments: u32,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Binding {
    level_item: String,
    fragment_item: String,
}

pub(crate) struct Catalog {
    definitions: BTreeMap<String, Definition>,
    bindings: BTreeMap<String, Binding>,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum LevelRequirements {
    Costs(BTreeMap<String, u64>),
    AtCap(CappedMarker),
}

#[derive(Serialize, Deserialize)]
enum CappedMarker {
    #[serde(rename = "")]
    Empty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TalentTier {
    Rare,
    Epic,
    Legendary,
}

impl TalentTier {
    fn parse(rarity: &str) -> Option<Self> {
        match rarity {
            "rare" => Some(Self::Rare),
            "epic" => Some(Self::Epic),
            "legendary" => Some(Self::Legendary),
            _ => None,
        }
    }

    fn from_official_id(id: &str) -> Option<Self> {
        id.rsplit_once('_')
            .and_then(|(_, suffix)| Self::parse(suffix))
    }

    const fn default_level(self) -> u32 {
        match self {
            Self::Rare | Self::Epic => 10,
            Self::Legendary => 5,
        }
    }

    fn default_requirements(self) -> (bool, LevelRequirements) {
        match self {
            Self::Rare => (
                false,
                LevelRequirements::Costs(BTreeMap::from([
                    ("[fragments]".to_owned(), 90),
                    ("silver".to_owned(), 1300),
                ])),
            ),
            Self::Epic | Self::Legendary => (true, LevelRequirements::AtCap(CappedMarker::Empty)),
        }
    }
}

struct Definition {
    public: PublicTalent,
    tier: Option<TalentTier>,
    level_row: Option<(u32, bool, LevelRequirements)>,
}

#[derive(Serialize)]
struct PublicTalent {
    id: String,
    hero: String,
    def: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectedTalent<'a> {
    #[serde(flatten)]
    public: &'a PublicTalent,
    #[serde(flatten)]
    progress: TalentProgress,
    max_level: bool,
    level_requirements: &'a LevelRequirements,
}

impl Definition {
    fn unlocked_progress(&self, raw: TalentProgress, unlock_all: bool) -> TalentProgress {
        if !unlock_all {
            return raw;
        }
        let Some(tier) = self.tier else {
            return raw;
        };
        TalentProgress {
            level: raw.level.max(tier.default_level()),
            fragments: raw.fragments.max(1),
        }
    }

    fn project(&self, progress: TalentProgress, unlock_all: bool) -> Value {
        let progress = self.unlocked_progress(progress, unlock_all);
        let known = self
            .level_row
            .as_ref()
            .filter(|(level, _, _)| *level == progress.level);
        let empty = LevelRequirements::Costs(BTreeMap::new());
        let tier_default = (unlock_all && known.is_none())
            .then_some(self.tier)
            .flatten()
            .filter(|tier| progress.level == tier.default_level())
            .map(|tier| tier.default_requirements());
        let (max_level, level_requirements) = if let Some((_, capped, requirements)) = known {
            (*capped, requirements)
        } else if let Some((capped, requirements)) = &tier_default {
            (*capped, requirements)
        } else {
            (false, &empty)
        };
        serde_json::to_value(ProjectedTalent {
            public: &self.public,
            progress,
            max_level,
            level_requirements,
        })
        .expect("public talent projection")
    }
}

impl Catalog {
    pub(super) fn add_supplement(&mut self, data: &halcyon_data::GameData) {
        for talent in data.extra.combat_content().talents() {
            let (Some(id), Some(hero)) = (&talent.official_id, &talent.hero) else {
                continue;
            };
            let tier = talent
                .rarity
                .as_deref()
                .and_then(TalentTier::parse)
                .or_else(|| TalentTier::from_official_id(id));
            self.definitions
                .entry(id.clone())
                .and_modify(|existing| {
                    if existing.tier.is_none() {
                        existing.tier = tier;
                    }
                })
                .or_insert_with(|| Definition {
                    public: PublicTalent {
                        id: id.clone(),
                        def: format!("*{}*", talent.id),
                        hero: format!("*{hero}*"),
                    },
                    tier,
                    // The pack's cost sample does not identify an account level.
                    level_row: None,
                });
        }
    }
    pub fn parse(value: Value) -> Result<Self, Error> {
        let invalid = || Error::Config("invalid public talent template");
        let mut definitions = BTreeMap::new();
        for (id, row) in value.as_object().ok_or_else(invalid)? {
            if id.is_empty()
                || row.get("id").and_then(Value::as_str) != Some(id)
                || ["def", "hero"]
                    .iter()
                    .any(|key| row[*key].as_str().is_none_or(str::is_empty))
                || ["level", "fragments"]
                    .iter()
                    .any(|key| row.get(*key).is_some_and(|v| !v.is_u64()))
            {
                return Err(invalid());
            }
            let capped = row["maxLevel"].as_bool().ok_or_else(invalid)?;
            let requirements =
                LevelRequirements::deserialize(&row["levelRequirements"]).map_err(|_| invalid())?;
            if !capped && matches!(requirements, LevelRequirements::AtCap(_)) {
                return Err(invalid());
            }
            // An operator can supply a captured response. Admit only documented public
            // fields; envelope credentials, individual balances and unknown data stay out.
            let public = PublicTalent {
                id: id.clone(),
                def: row["def"].as_str().ok_or_else(invalid)?.to_owned(),
                hero: row["hero"].as_str().ok_or_else(invalid)?.to_owned(),
            };
            let level_row = row
                .get("level")
                .map(|v| {
                    let level =
                        u32::try_from(v.as_u64().ok_or_else(invalid)?).map_err(|_| invalid())?;
                    Ok::<_, Error>((level, capped, requirements))
                })
                .transpose()?;
            let tier = TalentTier::from_official_id(id);
            definitions.insert(
                id.clone(),
                Definition {
                    public,
                    tier,
                    level_row,
                },
            );
        }
        Ok(Self {
            definitions,
            bindings: BTreeMap::new(),
        })
    }

    pub fn configure(
        &mut self,
        bindings: BTreeMap<String, Binding>,
        items: &BTreeSet<&String>,
    ) -> Result<(), Error> {
        let mut used = BTreeSet::new();
        for (id, b) in &bindings {
            if !self.definitions.contains_key(id)
                || [&b.level_item, &b.fragment_item]
                    .into_iter()
                    .any(|key| !items.contains(key) || !used.insert(key))
            {
                return Err(Error::Config(
                    "unknown talent or invalid talent inventory binding",
                ));
            }
        }
        self.bindings = bindings;
        Ok(())
    }

    /// Only the explicit complete published triplets for the verified templates qualify.
    pub(crate) fn install_match_authority(
        &self,
        app: &halcyon_platform::AppState,
    ) -> Result<(), Error> {
        let mut triplets = BTreeMap::new();
        for entry in halcyon_platform::ARAL_TALENT_HEROES {
            let rows: Vec<_> = self
                .definitions
                .iter()
                .filter(|(_, row)| row.public.hero == entry.hero)
                .collect();
            if rows.len() != 3 {
                continue;
            }
            let items: Option<Vec<_>> = ["rare", "epic", "legendary"]
                .into_iter()
                .zip(entry.definitions_by_rarity)
                .map(|(tier, name)| {
                    let id = format!("{}_{tier}", entry.inventory_family);
                    let def = format!("*Talent_{}_{name}*", entry.asset_family);
                    rows.iter()
                        .any(|(key, row)| key.as_str() == id && row.public.def == *def)
                        .then_some(())?;
                    self.bindings.get(&id).map(|b| b.level_item.clone())
                })
                .collect();
            if let Some(items) = items {
                triplets.insert(entry.template, items);
            }
        }
        if !triplets.is_empty() {
            app.set_talent_levels_resolver(move |tx, player| {
                let id = player.to_string();
                let mut lookup = tx.prepare_cached(
                    "SELECT count FROM community_inventory WHERE player=? AND item=?",
                )?;
                let mut levels = halcyon_platform::TalentLevels::new();
                use rusqlite::OptionalExtension;
                for (&template, items) in &triplets {
                    let counts = items.iter().map(|item| {
                        lookup
                            .query_row([&id, item], |r| r.get::<_, u32>(0))
                            .optional()
                    });
                    let counts = counts.collect::<rusqlite::Result<Vec<_>>>()?;
                    if let Some(level) = equal_wire_level(&counts) {
                        levels.insert(template, level);
                    }
                }
                Ok(levels)
            })?;
        }
        Ok(())
    }

    fn balances(&self, id: &str, inventory: &BTreeMap<String, u32>) -> TalentProgress {
        self.bindings
            .get(id)
            .map_or_else(TalentProgress::default, |b| TalentProgress {
                level: inventory.get(&b.level_item).copied().unwrap_or(0),
                fragments: inventory.get(&b.fragment_item).copied().unwrap_or(0),
            })
    }
}

fn equal_wire_level(counts: &[Option<u32>]) -> Option<std::num::NonZeroU8> {
    let [Some(a), Some(b), Some(c)] = counts else {
        return None;
    };
    if a != b || a != c {
        return None;
    }
    std::num::NonZeroU8::new(u8::try_from(*a).ok()?)
}

impl Platform {
    async fn talent_inventory(&self, player: Uuid) -> Result<BTreeMap<String, u32>, Error> {
        if self.talents.bindings.is_empty() {
            return Ok(BTreeMap::new());
        }
        Ok(self.community.inventory_balances(player).await?)
    }

    /// Look up a known talent for a trusted caller holding the authoritative player's ID.
    pub async fn talent_progress(
        &self,
        player: Uuid,
        talent_id: &str,
    ) -> Result<Option<TalentProgress>, Error> {
        let Some(definition) = self.talents.definitions.get(talent_id) else {
            return Ok(None);
        };
        let progress = self
            .talents
            .balances(talent_id, &self.talent_inventory(player).await?);
        Ok(Some(
            definition.unlocked_progress(progress, self.unlock_all_talents),
        ))
    }

    pub(crate) async fn talents_for(&self, player: Uuid) -> Result<Value, Error> {
        let inventory = self.talent_inventory(player).await?;
        let result: Map<_, _> = self
            .talents
            .definitions
            .iter()
            .map(|(id, definition)| {
                (
                    id.clone(),
                    definition.project(
                        self.talents.balances(id, &inventory),
                        self.unlock_all_talents,
                    ),
                )
            })
            .collect();
        Ok(Value::Object(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_positive_wire_level_requires_three_owned_representable_levels() {
        assert_eq!(equal_wire_level(&[Some(7); 3]).map(|v| v.get()), Some(7));
        for denied in [
            [None, Some(7), Some(7)],
            [Some(7), Some(7), Some(8)],
            [Some(0); 3],
            [Some(256); 3],
        ] {
            assert_eq!(equal_wire_level(&denied), None);
        }
        assert_eq!(equal_wire_level(&[Some(7); 2]), None);
    }

    #[tokio::test]
    async fn local_reservation_and_durable_peer_ticket_use_the_authoritative_inventory() {
        use halcyon_community::{Community, ItemDefinition};
        use halcyon_platform::{
            AppState, HostResult, HostedMatch, MatchHost, PlatformConfig, Reservation,
        };
        use std::sync::{Arc, Mutex};
        #[derive(Default)]
        struct Host(Mutex<Option<Reservation>>);
        impl MatchHost for Host {
            fn reserve(&self, reservation: Reservation) -> HostResult<HostedMatch> {
                *self.0.lock().unwrap() = Some(reservation);
                Ok(HostedMatch {
                    address: "localhost:7557".into(),
                })
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let config = PlatformConfig {
            database_path: dir.path().join("platform.db"),
            public_address: "localhost:8080".into(),
            seats_per_match: 6,
        };
        let host = Arc::new(Host::default());
        let app = AppState::open(config.clone(), host.clone()).unwrap();
        let inventory = Community::open(&config.database_path).unwrap();
        let rows: Map<_, _> = [
            ("Caine", "caine", ["TalentA", "TalentB", "TalentC"]),
            ("Ringo", "ringo", ["LongShot", "BulletSplit", "SpikedBrew"]),
        ]
        .into_iter()
        .flat_map(|(hero, family, definitions)| {
            ["rare", "epic", "legendary"]
                .into_iter()
                .zip(definitions)
                .map(move |(tier, name)| {
                    let id = format!("{family}_{tier}");
                    (
                        id.clone(),
                        talent_fixture(
                            PublicTalent {
                                id,
                                hero: format!("*{hero}*"),
                                def: format!("*Talent_{hero}_{name}*"),
                            },
                            74,
                            false,
                            &LevelRequirements::Costs(BTreeMap::new()),
                        ),
                    )
                })
        })
        .collect();
        let mut catalog = Catalog::parse(Value::Object(rows)).unwrap();
        for id in catalog.definitions.keys() {
            for item in [id.clone(), format!("{id}_fragments")] {
                inventory
                    .define_item(ItemDefinition {
                        name: item.clone(),
                        id: item,
                        slot: "test".into(),
                    })
                    .await
                    .unwrap();
            }
            catalog.bindings.insert(
                id.clone(),
                Binding {
                    level_item: id.clone(),
                    fragment_item: format!("{id}_fragments"),
                },
            );
        }
        catalog.install_match_authority(&app).unwrap();

        let guests = {
            let mut guests = Vec::new();
            for index in 0..7 {
                let name = format!("Guest{index}");
                let guest = app.create_guest(name.clone()).await.unwrap();
                inventory
                    .observe_player(guest.player_id, name, true)
                    .await
                    .unwrap();
                if matches!(index, 0 | 1 | 6) {
                    for (ordinal, id) in catalog.definitions.keys().enumerate() {
                        let level = if index == 1 && ordinal == 0 { 8 } else { 7 };
                        inventory
                            .award_item(Uuid::new_v4(), guest.player_id, id.clone(), level)
                            .await
                            .unwrap();
                    }
                }
                guests.push(guest);
            }
            guests
        };
        let mode = "casual_aral".to_string();
        for guest in &guests[..6] {
            app.join(&guest.session_token, mode.clone()).await.unwrap();
        }
        let match_id = app
            .profile(&guests[0].session_token)
            .await
            .unwrap()
            .lobby
            .unwrap()
            .match_id
            .unwrap();
        for guest in &guests[..6] {
            app.accept(&guest.session_token, match_id).await.unwrap();
        }
        {
            let reserved = host.0.lock().unwrap();
            let reserved = reserved.as_ref().unwrap();
            assert_eq!(reserved.players.len(), reserved.talent_levels.len());
            for (id, grant) in reserved.players.iter().zip(&reserved.talent_levels) {
                assert!(
                    !grant.contains_key(&243),
                    "Ringo's extra asset leaves the triplet ambiguous"
                );
                assert_eq!(
                    grant.get(&915).map(|level| level.get()),
                    (*id == guests[0].player_id).then_some(7)
                );
            }
        }

        let voter = &guests[6];
        let pool = Uuid::new_v4();
        let ticket = app
            .federated_join(&voter.session_token, pool, mode, 6)
            .await
            .unwrap()
            .ticket;
        inventory
            .award_item(Uuid::new_v4(), voter.player_id, "caine_rare".into(), 1)
            .await
            .unwrap();
        drop(app);
        let reopened = AppState::open(config, host).unwrap();
        let frozen = reopened.pooled_talent_levels(pool, ticket).await.unwrap();
        assert_eq!(frozen[0][&915].get(), 7);
        assert!(
            reopened
                .pooled_talent_levels(Uuid::new_v4(), ticket)
                .await
                .unwrap()
                .is_empty()
        );
    }

    fn talent_fixture(
        public: PublicTalent,
        level: u32,
        max_level: bool,
        requirements: &LevelRequirements,
    ) -> Value {
        serde_json::to_value(ProjectedTalent {
            public: &public,
            progress: TalentProgress {
                level,
                fragments: 12,
            },
            max_level,
            level_requirements: requirements,
        })
        .unwrap()
    }

    #[test]
    fn observed_cap_and_cost_belong_only_to_the_observed_level() {
        for (capped, cost) in [
            (true, LevelRequirements::Costs(BTreeMap::new())),
            (true, LevelRequirements::AtCap(CappedMarker::Empty)),
            (
                false,
                LevelRequirements::Costs(BTreeMap::from([
                    ("[fragments]".into(), 90),
                    ("silver".into(), 1300),
                ])),
            ),
        ] {
            let fixture = talent_fixture(
                PublicTalent {
                    id: "example".into(),
                    hero: "*Hero*".into(),
                    def: "*Definition*".into(),
                },
                10,
                capped,
                &cost,
            );
            let catalog = Catalog::parse(
                serde_json::to_value(BTreeMap::from([("example", fixture)])).unwrap(),
            )
            .unwrap();
            let talent = &catalog.definitions["example"];
            let fresh = talent.project(TalentProgress::default(), false);
            assert_eq!(
                (
                    fresh["maxLevel"].clone(),
                    fresh["levelRequirements"].clone()
                ),
                (
                    Value::Bool(false),
                    serde_json::to_value(BTreeMap::<String, u64>::new()).unwrap()
                )
            );
            let at_ten = talent.project(
                TalentProgress {
                    level: 10,
                    fragments: 2,
                },
                false,
            );
            assert_eq!(at_ten["maxLevel"], capped);
            assert_eq!(
                at_ten["levelRequirements"],
                serde_json::to_value(cost).unwrap()
            );
            assert_eq!(at_ten["fragments"], 2);
        }
    }

    #[test]
    fn unlock_all_talents_projects_official_ce_levels_and_requirements() {
        let mut catalog = Catalog::parse(Value::Object(Map::new())).unwrap();
        for (id, tier, def) in [
            ("baron_rare", TalentTier::Rare, "*Talent_Baron_TalentA*"),
            ("baron_epic", TalentTier::Epic, "*Talent_Baron_TalentB*"),
            (
                "baron_legendary",
                TalentTier::Legendary,
                "*Talent_Baron_TalentC*",
            ),
        ] {
            catalog.definitions.insert(
                id.to_owned(),
                Definition {
                    public: PublicTalent {
                        id: id.to_owned(),
                        hero: "*Baron*".to_owned(),
                        def: def.to_owned(),
                    },
                    tier: Some(tier),
                    level_row: None,
                },
            );
        }

        // Default mode keeps unowned talents locked at 0/0.
        assert_eq!(
            catalog.definitions["baron_rare"].project(TalentProgress::default(), false)["level"],
            0
        );

        // Unlocked mode sets Rare=10, Epic=10 (maxLevel), Legendary=5 (maxLevel).
        let rare = catalog.definitions["baron_rare"].project(TalentProgress::default(), true);
        assert_eq!(rare["level"], 10);
        assert_eq!(rare["fragments"], 1);
        assert_eq!(rare["maxLevel"], false);
        assert_eq!(rare["levelRequirements"]["[fragments]"], 90);
        assert_eq!(rare["levelRequirements"]["silver"], 1300);

        let epic = catalog.definitions["baron_epic"].project(TalentProgress::default(), true);
        assert_eq!(epic["level"], 10);
        assert_eq!(epic["fragments"], 1);
        assert_eq!(epic["maxLevel"], true);
        assert_eq!(epic["levelRequirements"], "");

        let leg = catalog.definitions["baron_legendary"].project(TalentProgress::default(), true);
        assert_eq!(leg["level"], 5);
        assert_eq!(leg["fragments"], 1);
        assert_eq!(leg["maxLevel"], true);
        assert_eq!(leg["levelRequirements"], "");
    }
}
