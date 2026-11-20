use crate::views::{Inventory, InventoryItem, Items, Quest, QuestStatus, enum_column};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{Action, Community, CommunityError, Result, ensure_player, optional_row, validated};

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

fn key(value: String) -> Result<String> {
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
    /// Define equipable content from trusted configuration.
    pub async fn define_item(&self, definition: ItemDefinition) -> Result<()> {
        let id = key(definition.id)?;
        let slot = key(definition.slot)?;
        let name = validated(definition.name, 64, true)?;
        self.run(move |tx| {
            tx.execute(
                "INSERT INTO community_items(id,slot,name) VALUES(?,?,?) ON CONFLICT DO NOTHING",
                params![id, slot, name],
            )?;
            let same: bool = tx.query_row(
                "SELECT slot=? AND name=? FROM community_items WHERE id=?",
                params![slot, name, id],
                |row| row.get(0),
            )?;
            if !same {
                return Err(CommunityError::Conflict(
                    "item id has a different definition",
                ));
            }
            Ok(())
        })
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
        self.run(move |tx| {
            if let Some(item) = &item {
                item_slot(tx, item)?;
            }
            tx.execute(
                "INSERT INTO community_quests(id,category,title,goal,skippable,reward_item,reward_count)
                 VALUES(?,?,?,?,?,?,?) ON CONFLICT DO NOTHING",
                params![id, category, title, goal, skippable, item, count],
            )?;
            let same: bool = tx.query_row(
                "SELECT category=? AND title=? AND goal=? AND skippable=? AND reward_item IS ? AND reward_count IS ?
                 FROM community_quests WHERE id=?",
                params![category, title, goal, skippable, item, count, id], |row| row.get(0),
            )?;
            if !same {
                return Err(CommunityError::Conflict("quest id has a different definition"));
            }
            Ok(())
        }).await
    }

    /// Grant an earned item exactly once per trusted grant id.
    pub async fn award_item(
        &self,
        grant: Uuid,
        player: Uuid,
        item: String,
        count: u32,
    ) -> Result<()> {
        self.run(move |tx| Self::award_item_in_transaction(tx, grant, player, &item, count))
            .await
    }

    /// Apply the grant inside a caller-owned transaction with its source receipt.
    pub fn award_item_in_transaction(
        tx: &Transaction<'_>,
        grant: Uuid,
        player: Uuid,
        item: &str,
        count: u32,
    ) -> Result<()> {
        let item = key(item.to_owned())?;
        if count == 0 {
            return Err(CommunityError::Invalid("item count must be positive"));
        }
        let player = player.to_string();
        let grant = grant.to_string();
        ensure_player(tx, &player)?;
        item_slot(tx, &item)?;
        let inserted = tx.execute(
            "INSERT INTO community_item_grants(id,player,item,count) VALUES(?,?,?,?) ON CONFLICT DO NOTHING",
            params![grant, player, item, count],
        )?;
        if inserted == 1 {
            add_item(tx, &player, &item, count)?;
        } else {
            let same: bool = tx.query_row(
                "SELECT player=? AND item=? AND count=? FROM community_item_grants WHERE id=?",
                params![player, item, count, grant],
                |row| row.get(0),
            )?;
            if !same {
                return Err(CommunityError::Conflict("grant id is already in use"));
            }
        }
        Ok(())
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
        self.run(move |tx| {
            let event = event.to_string();
            let player = player.to_string();
            ensure_player(tx, &player)?;
            let goal: u32 = optional_row(tx.query_row(
                "SELECT goal FROM community_quests WHERE id=?", [&quest], |row| row.get(0),
            ), "quest does not exist")?;
            let inserted = tx.execute(
                "INSERT INTO community_quest_progress(id,player,quest,amount) VALUES(?,?,?,?) ON CONFLICT DO NOTHING",
                params![event, player, quest, amount],
            )?;
            if inserted == 0 {
                let same: bool = tx.query_row(
                    "SELECT player=? AND quest=? AND amount=? FROM community_quest_progress WHERE id=?",
                    params![player, quest, amount, event], |row| row.get(0),
                )?;
                if !same {
                    return Err(CommunityError::Conflict("progress event id is already in use"));
                }
            } else {
                tx.execute(
                    "INSERT INTO community_quest_states(player,quest,progress) VALUES(?1,?2,min(?3,?4))
                     ON CONFLICT(player,quest) DO UPDATE SET progress=min(progress + ?3,?4) WHERE state='active'",
                    params![player, quest, amount, goal],
                )?;
            }
            Ok(())
        }).await
    }
}

fn item_slot(tx: &Transaction<'_>, item: &str) -> Result<String> {
    optional_row(
        tx.query_row(
            "SELECT slot FROM community_items WHERE id=?",
            [item],
            |row| row.get(0),
        ),
        "item does not exist",
    )
}

pub(super) fn add_item(tx: &Transaction<'_>, player: &str, item: &str, count: u32) -> Result<()> {
    let updated = tx.execute(
        "INSERT INTO community_inventory(player,item,count) VALUES(?,?,?)
         ON CONFLICT(player,item) DO UPDATE SET count=count+excluded.count WHERE count+excluded.count <= 4294967295",
        params![player, item, count],
    )?;
    if updated == 0 {
        return Err(CommunityError::Conflict(
            "inventory count exceeds the supported range",
        ));
    }
    Ok(())
}

fn inventory(tx: &Transaction<'_>, player: &str) -> Result<Value> {
    let mut query = tx.prepare(
        "SELECT i.item, d.name, d.slot, i.count, e.item IS NOT NULL
         FROM community_inventory i JOIN community_items d ON d.id=i.item
         LEFT JOIN community_equipment e ON e.player=i.player AND e.item=i.item AND e.slot=d.slot
         WHERE i.player=? ORDER BY d.slot,i.item",
    )?;
    let items = query
        .query_map([player], |row| {
            Ok(InventoryItem {
                id: row.get(0)?,
                name: row.get(1)?,
                slot: row.get(2)?,
                count: row.get(3)?,
                equipped: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let equipped = items
        .iter()
        .filter(|item| item.equipped)
        .map(|item| (item.slot.clone(), item.id.clone()))
        .collect();
    Ok(serde_json::to_value(Inventory { items, equipped })?)
}

fn quests(tx: &Transaction<'_>, player: &str, category: Option<String>) -> Result<Value> {
    let category = category.map(key).transpose()?;
    let mut query = tx.prepare(
        "SELECT q.id,q.category,q.title,q.goal,q.skippable,q.reward_item,q.reward_count,
                coalesce(s.progress,0),coalesce(s.state,'active')
         FROM community_quests q LEFT JOIN community_quest_states s ON s.quest=q.id AND s.player=?1
         WHERE ?2 IS NULL OR q.category=?2 ORDER BY q.id",
    )?;
    let items = query
        .query_map(params![player, category], |row| {
            Ok(Quest {
                id: row.get(0)?,
                category: row.get(1)?,
                title: row.get(2)?,
                goal: row.get(3)?,
                skippable: row.get(4)?,
                reward_item: row.get(5)?,
                reward_count: row.get(6)?,
                progress: row.get(7)?,
                state: enum_column(row, 8)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(serde_json::to_value(Items { items })?)
}

struct QuestState {
    goal: u32,
    skippable: bool,
    reward: Option<(String, u32)>,
    progress: u32,
    state: QuestStatus,
}

fn quest_state(tx: &Transaction<'_>, player: &str, quest: &str) -> Result<QuestState> {
    optional_row(tx.query_row(
        "SELECT q.goal,q.skippable,q.reward_item,q.reward_count,coalesce(s.progress,0),coalesce(s.state,'active')
         FROM community_quests q LEFT JOIN community_quest_states s ON s.quest=q.id AND s.player=?1 WHERE q.id=?2",
        params![player, quest], |row| {
            Ok(QuestState {
                goal:row.get(0)?, skippable:row.get(1)?,
                reward:row.get::<_, Option<String>>(2)?.zip(row.get::<_, Option<u32>>(3)?),
                progress:row.get(4)?, state:enum_column(row,5)?,
            })
        },
    ), "quest does not exist")
}

fn equip(tx: &Transaction<'_>, player: &str, slot: String, item: Option<String>) -> Result<()> {
    let slot = key(slot)?;
    if let Some(item) = item {
        let item = key(item)?;
        if item_slot(tx, &item)? != slot {
            return Err(CommunityError::Invalid("item belongs to a different slot"));
        }
        let owned = tx
            .query_row(
                "SELECT count FROM community_inventory WHERE player=? AND item=?",
                params![player, item],
                |row| row.get::<_, u32>(0),
            )
            .optional()?
            .unwrap_or(0);
        if owned == 0 {
            return Err(CommunityError::Forbidden);
        }
        tx.execute(
            "INSERT INTO community_equipment(player,slot,item) VALUES(?,?,?)
             ON CONFLICT(player,slot) DO UPDATE SET item=excluded.item",
            params![player, slot, item],
        )?;
    } else {
        tx.execute(
            "DELETE FROM community_equipment WHERE player=? AND slot=?",
            params![player, slot],
        )?;
    }
    Ok(())
}

pub(crate) fn execute(tx: &Transaction<'_>, player: &str, action: Action) -> Result<Value> {
    match action {
        Action::Inventory => inventory(tx, player),
        Action::Equip { slot, item } => {
            equip(tx, player, slot, item)?;
            inventory(tx, player)
        }
        Action::SetLoadout { slots } => {
            if slots.len() > 64 {
                return Err(CommunityError::Invalid("too many equipment slots"));
            }
            for (slot, item) in slots {
                equip(tx, player, slot, item)?;
            }
            inventory(tx, player)
        }
        Action::Quests { category } => quests(tx, player, category),
        Action::ClaimQuest { quest } => {
            let quest = key(quest)?;
            let state = quest_state(tx, player, &quest)?;
            if state.state != QuestStatus::Claimed {
                if state.state != QuestStatus::Active || state.progress < state.goal {
                    return Err(CommunityError::Conflict("quest is not claimable"));
                }
                tx.execute(
                    "UPDATE community_quest_states SET state='claimed' WHERE player=? AND quest=?",
                    params![player, quest],
                )?;
                if let Some((item, count)) = state.reward {
                    add_item(tx, player, &item, count)?;
                }
            }
            quests(tx, player, None)
        }
        Action::SkipQuest { quest } => {
            let quest = key(quest)?;
            let state = quest_state(tx, player, &quest)?;
            if !state.skippable || state.state == QuestStatus::Claimed {
                return Err(CommunityError::Conflict("quest cannot be skipped"));
            }
            tx.execute(
                "INSERT INTO community_quest_states(player,quest,state) VALUES(?,?,'skipped')
                 ON CONFLICT(player,quest) DO UPDATE SET state='skipped'",
                params![player, quest],
            )?;
            quests(tx, player, None)
        }
        _ => Err(CommunityError::Invalid("invalid inventory action")),
    }
}
