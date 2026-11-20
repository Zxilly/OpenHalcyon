pub(crate) mod store;

use diesel::SqliteConnection;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Community, CommunityError, Result, validated};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemDefinition {
    pub id: String,
    pub slot: String,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestDefinition {
    pub id: String,
    pub category: String,
    pub title: String,
    pub goal: u32,
    pub skippable: bool,
    pub reward: Option<(String, u32)>,
}

pub(crate) fn key(value: String) -> Result<String> {
    let value = validated(value, 80, true)?;
    if !value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b':'))
    {
        return Err(CommunityError::Invalid("invalid item, slot, or quest id"));
    }
    Ok(value)
}

impl Community {
    /// Read the player's canonical inventory balances for a trusted content projection.
    pub async fn inventory_balances(
        &self,
        player: Uuid,
    ) -> Result<std::collections::BTreeMap<String, u32>> {
        self.with_orm(move |db| store::inventory_balances(db, &player.to_string()))
            .await
    }

    /// Define equipable content from trusted configuration.
    pub async fn define_item(&self, definition: ItemDefinition) -> Result<()> {
        let id = key(definition.id)?;
        let slot = key(definition.slot)?;
        let name = validated(definition.name, 64, true)?;
        self.orm_transaction(move |db| store::define_item(db, &id, &slot, &name))
            .await
    }

    pub async fn define_quest(&self, definition: QuestDefinition) -> Result<()> {
        let id = key(definition.id)?;
        let category = key(definition.category)?;
        let title = validated(definition.title, 128, true)?;
        let goal = definition.goal;
        let skippable = definition.skippable;
        if goal == 0
            || definition
                .reward
                .as_ref()
                .is_some_and(|(_, count)| *count == 0)
        {
            return Err(CommunityError::Invalid(
                "quest goal and reward count must be positive",
            ));
        }
        let (item, count) = match definition.reward {
            Some((item, count)) => (Some(key(item)?), Some(count)),
            None => (None, None),
        };
        self.orm_transaction(move |db| {
            store::define_quest(
                db,
                &store::QuestRow {
                    id,
                    category,
                    title,
                    goal: i64::from(goal),
                    skippable,
                    reward_item: item,
                    reward_count: count.map(i64::from),
                },
            )
        })
        .await
    }

    /// Grant an earned item exactly once per trusted grant id.
    pub async fn award_item(
        &self,
        grant: Uuid,
        player: Uuid,
        item: String,
        count: u32,
    ) -> Result<()> {
        self.orm_transaction(move |db| store::award_item(db, grant, player, &item, count))
            .await
    }

    /// Apply the grant inside a caller-owned Diesel transaction with its source receipt.
    pub fn award_item_in_orm_transaction(
        db: &mut SqliteConnection,
        grant: Uuid,
        player: Uuid,
        item: &str,
        count: u32,
    ) -> Result<()> {
        store::award_item(db, grant, player, item, count)
    }

    /// Apply verified progress exactly once per trusted event id.
    pub async fn record_progress(
        &self,
        event: Uuid,
        player: Uuid,
        quest: String,
        amount: u32,
    ) -> Result<()> {
        let quest = key(quest)?;
        if amount == 0 {
            return Err(CommunityError::Invalid("progress must be positive"));
        }
        self.orm_transaction(move |db| store::record_progress(db, event, player, &quest, amount))
            .await
    }
}
