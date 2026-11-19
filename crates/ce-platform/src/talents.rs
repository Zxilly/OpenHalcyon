use super::{Error, Platform};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// Local inventory balances for one authenticated player's talent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct TalentProgress {
    pub level: u32,
    pub fragments: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Binding {
    level_item: String,
    fragment_item: String,
}

pub(crate) struct Catalog {
    definitions: BTreeMap<String, Definition>,
    bindings: BTreeMap<String, Binding>,
}

struct Definition {
    public: Value,
    level_row: Option<(u32, bool, Value)>,
}

impl Definition {
    fn project(&self, progress: TalentProgress) -> Value {
        let mut row = self.public.clone();
        row["level"] = json!(progress.level);
        row["fragments"] = json!(progress.fragments);
        let known = self
            .level_row
            .as_ref()
            .filter(|(level, _, _)| *level == progress.level);
        row["maxLevel"] = json!(known.is_some_and(|(_, capped, _)| *capped));
        row["levelRequirements"] = known
            .map(|(_, _, requirements)| requirements.clone())
            .unwrap_or_else(|| json!({}));
        row
    }
}

impl Catalog {
    pub fn parse(value: Value) -> Result<Self, Error> {
        let invalid = || Error::Config("invalid public talent template");
        let mut definitions = BTreeMap::new();
        for (id, row) in value.as_object().ok_or_else(invalid)? {
            if id.is_empty()
                || row.get("id").and_then(Value::as_str) != Some(id)
                || ["def", "hero"]
                    .iter()
                    .any(|key| row[*key].as_str().is_none_or(str::is_empty))
                || !row["maxLevel"].is_boolean()
                || !row["levelRequirements"]
                    .as_object()
                    .is_some_and(|fields| fields.values().all(Value::is_u64))
                || ["level", "fragments"]
                    .iter()
                    .any(|key| row.get(*key).is_some_and(|v| !v.is_u64()))
            {
                return Err(invalid());
            }
            // An operator can supply a captured response. Admit only documented public
            // fields; envelope credentials, individual balances and unknown data stay out.
            let public = ["id", "def", "hero"]
                .into_iter()
                .map(|key| (key.to_owned(), row[key].clone()))
                .collect();
            let level_row = row
                .get("level")
                .map(|v| {
                    let level =
                        u32::try_from(v.as_u64().ok_or_else(invalid)?).map_err(|_| invalid())?;
                    Ok::<_, Error>((
                        level,
                        row["maxLevel"].as_bool().ok_or_else(invalid)?,
                        row["levelRequirements"].clone(),
                    ))
                })
                .transpose()?;
            definitions.insert(
                id.clone(),
                Definition {
                    public: Value::Object(public),
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

    fn balances(&self, id: &str, inventory: &BTreeMap<String, u32>) -> TalentProgress {
        self.bindings
            .get(id)
            .map_or_else(TalentProgress::default, |b| TalentProgress {
                level: inventory.get(&b.level_item).copied().unwrap_or(0),
                fragments: inventory.get(&b.fragment_item).copied().unwrap_or(0),
            })
    }
}

impl Platform {
    async fn talent_inventory(&self, player: Uuid) -> Result<BTreeMap<String, u32>, Error> {
        if self.talents.bindings.is_empty() {
            return Ok(BTreeMap::new());
        }
        self.meta
            .transaction(move |tx| {
                let mut q =
                    tx.prepare("SELECT item,count FROM community_inventory WHERE player=?")?;
                Ok(
                    q.query_map([player.to_string()], |r| Ok((r.get(0)?, r.get(1)?)))?
                        .collect::<rusqlite::Result<_>>()?,
                )
            })
            .await
    }

    /// Look up a known talent for a trusted caller holding the authoritative player's ID.
    pub async fn talent_progress(
        &self,
        player: Uuid,
        talent_id: &str,
    ) -> Result<Option<TalentProgress>, Error> {
        if !self.talents.definitions.contains_key(talent_id) {
            return Ok(None);
        }
        Ok(Some(self.talents.balances(
            talent_id,
            &self.talent_inventory(player).await?,
        )))
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
                    definition.project(self.talents.balances(id, &inventory)),
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
    fn observed_cap_and_cost_belong_only_to_the_observed_level() {
        for (capped, cost) in [
            (true, json!({})),
            (false, json!({"[fragments]":90,"silver":1300})),
        ] {
            let catalog=Catalog::parse(json!({"example":{"id":"example","hero":"*Hero*","def":"*Definition*","level":10,"fragments":12,"maxLevel":capped,"levelRequirements":cost}})).unwrap();
            let talent = &catalog.definitions["example"];
            let fresh = talent.project(TalentProgress::default());
            assert_eq!(
                (
                    fresh["maxLevel"].clone(),
                    fresh["levelRequirements"].clone()
                ),
                (json!(false), json!({}))
            );
            let at_ten = talent.project(TalentProgress {
                level: 10,
                fragments: 2,
            });
            assert_eq!(at_ten["maxLevel"], capped);
            assert_eq!(at_ten["levelRequirements"], cost);
            assert_eq!(at_ten["fragments"], 2);
        }
    }
}
