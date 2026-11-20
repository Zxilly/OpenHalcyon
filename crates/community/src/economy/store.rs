use crate::{
    Action, CommunityError, Result,
    economy::key,
    orm::ensure_player,
    schema::{
        community_equipment as equipment, community_inventory as inventory,
        community_item_grants as grants, community_items as items,
        community_quest_progress as progress, community_quest_states as states,
        community_quests as quests,
    },
    views::{Inventory, InventoryItem, Items, Quest, QuestStatus},
};
use diesel::{prelude::*, upsert::excluded};
use serde_json::Value;
use uuid::Uuid;

pub(crate) fn unsigned(value: i64) -> Result<u32> {
    u32::try_from(value).map_err(|_| CommunityError::WorkerUnavailable)
}

pub(crate) fn define_item(
    db: &mut SqliteConnection,
    id: &str,
    slot: &str,
    name: &str,
) -> Result<()> {
    diesel::insert_into(items::table)
        .values((items::id.eq(id), items::slot.eq(slot), items::name.eq(name)))
        .on_conflict_do_nothing()
        .execute(db)?;
    let same = items::table
        .find(id)
        .select(items::slot.eq(slot).and(items::name.eq(name)))
        .first::<bool>(db)?;
    if !same {
        return Err(CommunityError::Conflict(
            "item id has a different definition",
        ));
    }
    Ok(())
}

#[derive(Insertable, Queryable, Selectable, PartialEq, Eq)]
#[diesel(table_name=quests)]
pub(crate) struct QuestRow {
    pub id: String,
    pub category: String,
    pub title: String,
    pub goal: i64,
    pub skippable: bool,
    pub reward_item: Option<String>,
    pub reward_count: Option<i64>,
}

pub(crate) fn define_quest(db: &mut SqliteConnection, row: &QuestRow) -> Result<()> {
    if let Some(item) = &row.reward_item {
        item_slot(db, item)?;
    }
    diesel::insert_into(quests::table)
        .values(row)
        .on_conflict_do_nothing()
        .execute(db)?;
    let existing = quests::table
        .find(&row.id)
        .select(QuestRow::as_select())
        .first(db)?;
    if row != &existing {
        return Err(CommunityError::Conflict(
            "quest id has a different definition",
        ));
    }
    Ok(())
}

fn item_slot(db: &mut SqliteConnection, id: &str) -> Result<String> {
    items::table
        .find(id)
        .select(items::slot)
        .first(db)
        .optional()?
        .ok_or(CommunityError::Missing("item does not exist"))
}

pub(crate) fn held(db: &mut SqliteConnection, player: &str, item: &str) -> Result<u32> {
    inventory::table
        .find((player, item))
        .select(inventory::count)
        .first::<i64>(db)
        .optional()?
        .map(unsigned)
        .transpose()
        .map(|amount| amount.unwrap_or_default())
}

pub(crate) fn add_item(
    db: &mut SqliteConnection,
    player: &str,
    item: &str,
    count: u32,
) -> Result<()> {
    let new = held(db, player, item)?
        .checked_add(count)
        .ok_or(CommunityError::Conflict(
            "inventory count exceeds the supported range",
        ))?;
    diesel::insert_into(inventory::table)
        .values((
            inventory::player.eq(player),
            inventory::item.eq(item),
            inventory::count.eq(i64::from(new)),
        ))
        .on_conflict((inventory::player, inventory::item))
        .do_update()
        .set(inventory::count.eq(excluded(inventory::count)))
        .execute(db)?;
    Ok(())
}

pub(crate) fn award_item(
    db: &mut SqliteConnection,
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
    ensure_player(db, &player)?;
    item_slot(db, &item)?;
    let inserted = diesel::insert_into(grants::table)
        .values((
            grants::id.eq(&grant),
            grants::player.eq(&player),
            grants::item.eq(&item),
            grants::count.eq(i64::from(count)),
        ))
        .on_conflict_do_nothing()
        .execute(db)?;
    if inserted != 0 {
        add_item(db, &player, &item, count)?;
    } else {
        let same = grants::table
            .find(&grant)
            .select(
                grants::player
                    .eq(&player)
                    .and(grants::item.eq(&item))
                    .and(grants::count.eq(i64::from(count))),
            )
            .first::<bool>(db)?;
        if !same {
            return Err(CommunityError::Conflict("grant id is already in use"));
        }
    }
    Ok(())
}

pub(crate) fn record_progress(
    db: &mut SqliteConnection,
    event: Uuid,
    player: Uuid,
    quest: &str,
    amount: u32,
) -> Result<()> {
    let player = player.to_string();
    let event = event.to_string();
    ensure_player(db, &player)?;
    let goal = quests::table
        .find(quest)
        .select(quests::goal)
        .first::<i64>(db)
        .optional()?
        .ok_or(CommunityError::Missing("quest does not exist"))
        .and_then(unsigned)?;
    let inserted = diesel::insert_into(progress::table)
        .values((
            progress::id.eq(&event),
            progress::player.eq(&player),
            progress::quest.eq(quest),
            progress::amount.eq(i64::from(amount)),
        ))
        .on_conflict_do_nothing()
        .execute(db)?;
    if inserted == 0 {
        let same = progress::table
            .find(&event)
            .select(
                progress::player
                    .eq(&player)
                    .and(progress::quest.eq(quest))
                    .and(progress::amount.eq(i64::from(amount))),
            )
            .first::<bool>(db)?;
        if !same {
            return Err(CommunityError::Conflict(
                "progress event id is already in use",
            ));
        }
        return Ok(());
    }
    let before = state(db, &player, quest)?;
    if before.1 == QuestStatus::Active {
        let updated = i64::from(before.0.saturating_add(amount).min(goal));
        diesel::insert_into(states::table)
            .values((
                states::player.eq(&player),
                states::quest.eq(quest),
                states::progress.eq(updated),
            ))
            .on_conflict((states::player, states::quest))
            .do_update()
            .set(states::progress.eq(excluded(states::progress)))
            .execute(db)?;
    }
    Ok(())
}

fn state(db: &mut SqliteConnection, player: &str, quest: &str) -> Result<(u32, QuestStatus)> {
    let row = states::table
        .find((player, quest))
        .select((states::progress, states::state))
        .first::<(i64, String)>(db)
        .optional()?;
    match row {
        Some((progress, status)) => Ok((unsigned(progress)?, parse_state(&status)?)),
        None => Ok((0, QuestStatus::Active)),
    }
}

fn parse_state(state: &str) -> Result<QuestStatus> {
    serde::Deserialize::deserialize(
        serde::de::value::StrDeserializer::<serde::de::value::Error>::new(state),
    )
    .map_err(|_| CommunityError::WorkerUnavailable)
}

pub(super) fn inventory_balances(
    db: &mut SqliteConnection,
    player: &str,
) -> Result<std::collections::BTreeMap<String, u32>> {
    inventory::table
        .filter(inventory::player.eq(player))
        .select((inventory::item, inventory::count))
        .load::<(String, i64)>(db)?
        .into_iter()
        .map(|(item, count)| Ok((item, unsigned(count)?)))
        .collect()
}

fn inventory_view(db: &mut SqliteConnection, player: &str) -> Result<Value> {
    let rows = inventory::table
        .inner_join(items::table.on(items::id.eq(inventory::item)))
        .left_join(
            equipment::table.on(equipment::player
                .eq(inventory::player)
                .and(equipment::item.eq(inventory::item))
                .and(equipment::slot.eq(items::slot))),
        )
        .filter(inventory::player.eq(player))
        .select((
            inventory::item,
            items::name,
            items::slot,
            inventory::count,
            equipment::item.nullable().is_not_null(),
        ))
        .order((items::slot, inventory::item))
        .load::<(String, String, String, i64, bool)>(db)?;
    let items = rows
        .into_iter()
        .map(|(id, name, slot, count, equipped)| {
            Ok(InventoryItem {
                id,
                name,
                slot,
                count: unsigned(count)?,
                equipped,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let equipped = items
        .iter()
        .filter(|i| i.equipped)
        .map(|i| (i.slot.clone(), i.id.clone()))
        .collect();
    Ok(serde_json::to_value(Inventory { items, equipped })?)
}

fn quest_view(db: &mut SqliteConnection, player: &str, category: Option<String>) -> Result<Value> {
    let category = category.map(key).transpose()?;
    let mut query = quests::table
        .select(QuestRow::as_select())
        .order(quests::id)
        .into_boxed();
    if let Some(category) = category {
        query = query.filter(quests::category.eq(category));
    }
    let states = states::table
        .filter(states::player.eq(player))
        .select((states::quest, states::progress, states::state))
        .load::<(String, i64, String)>(db)?
        .into_iter()
        .map(|(quest, progress, status)| (quest, (progress, status)))
        .collect::<std::collections::BTreeMap<_, _>>();
    let items = query
        .load::<QuestRow>(db)?
        .into_iter()
        .map(|row| {
            let (progress, status) = match states.get(&row.id) {
                Some((progress, state)) => (unsigned(*progress)?, parse_state(state)?),
                None => (0, QuestStatus::Active),
            };
            Ok(Quest {
                id: row.id,
                category: row.category,
                title: row.title,
                goal: unsigned(row.goal)?,
                skippable: row.skippable,
                reward_item: row.reward_item,
                reward_count: row.reward_count.map(unsigned).transpose()?,
                progress,
                state: status,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(serde_json::to_value(Items { items })?)
}

fn equip(
    db: &mut SqliteConnection,
    player: &str,
    slot: String,
    item: Option<String>,
) -> Result<()> {
    let slot = key(slot)?;
    if let Some(item) = item {
        let item = key(item)?;
        if item_slot(db, &item)? != slot {
            return Err(CommunityError::Invalid("item belongs to a different slot"));
        }
        if held(db, player, &item)? == 0 {
            return Err(CommunityError::Forbidden);
        }
        diesel::insert_into(equipment::table)
            .values((
                equipment::player.eq(player),
                equipment::slot.eq(&slot),
                equipment::item.eq(&item),
            ))
            .on_conflict((equipment::player, equipment::slot))
            .do_update()
            .set(equipment::item.eq(excluded(equipment::item)))
            .execute(db)?;
    } else {
        diesel::delete(equipment::table.find((player, &slot))).execute(db)?;
    }
    Ok(())
}

pub(crate) fn execute(db: &mut SqliteConnection, player: &str, action: Action) -> Result<Value> {
    match action {
        Action::Inventory => inventory_view(db, player),
        Action::Equip { slot, item } => {
            equip(db, player, slot, item)?;
            inventory_view(db, player)
        }
        Action::SetLoadout { slots } => {
            if slots.len() > 64 {
                return Err(CommunityError::Invalid("too many equipment slots"));
            }
            for (slot, item) in slots {
                equip(db, player, slot, item)?;
            }
            inventory_view(db, player)
        }
        Action::Quests { category } => quest_view(db, player, category),
        Action::ClaimQuest { quest } => {
            let quest = key(quest)?;
            let row = quests::table
                .find(&quest)
                .select(QuestRow::as_select())
                .first(db)
                .optional()?
                .ok_or(CommunityError::Missing("quest does not exist"))?;
            let (amount, status) = state(db, player, &quest)?;
            if status != QuestStatus::Claimed {
                if status != QuestStatus::Active || amount < unsigned(row.goal)? {
                    return Err(CommunityError::Conflict("quest is not claimable"));
                }
                diesel::update(states::table.find((player, &quest)))
                    .set(states::state.eq(QuestStatus::Claimed.as_str()))
                    .execute(db)?;
                if let Some((item, count)) = row.reward_item.zip(row.reward_count) {
                    add_item(db, player, &item, unsigned(count)?)?;
                }
            }
            quest_view(db, player, None)
        }
        Action::SkipQuest { quest } => {
            let quest = key(quest)?;
            let skippable = quests::table
                .find(&quest)
                .select(quests::skippable)
                .first::<bool>(db)
                .optional()?
                .ok_or(CommunityError::Missing("quest does not exist"))?;
            if !skippable || state(db, player, &quest)?.1 == QuestStatus::Claimed {
                return Err(CommunityError::Conflict("quest cannot be skipped"));
            }
            diesel::insert_into(states::table)
                .values((
                    states::player.eq(player),
                    states::quest.eq(&quest),
                    states::state.eq(QuestStatus::Skipped.as_str()),
                ))
                .on_conflict((states::player, states::quest))
                .do_update()
                .set(states::state.eq(QuestStatus::Skipped.as_str()))
                .execute(db)?;
            quest_view(db, player, None)
        }
        _ => Err(CommunityError::Invalid("invalid inventory action")),
    }
}
