//! Community response schemas shared by the SQLite projections and CE adapter.
use crate::{GroupKind, Role};
use rusqlite::{Row, types::Type};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::BTreeMap;

pub(super) fn enum_column<T: DeserializeOwned>(row: &Row<'_>, index: usize) -> rusqlite::Result<T> {
    let value = row.get_ref(index)?.as_str()?;
    T::deserialize(serde::de::value::StrDeserializer::<serde::de::value::Error>::new(value))
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(index, Type::Text, Box::new(error))
        })
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum QuestStatus {
    Active,
    Claimed,
    Skipped,
}

#[derive(Serialize)]
pub(super) struct Items<T> {
    pub items: Vec<T>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Contact {
    pub player_id: String,
    pub handle: String,
    pub favorite: bool,
    pub online: bool,
    pub status: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct FriendRequest {
    pub player_id: String,
    pub handle: String,
    pub created: i64,
    pub incoming: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Friends {
    pub friends: Vec<Contact>,
    pub requests: Vec<FriendRequest>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct InventoryItem {
    pub id: String,
    pub name: String,
    pub slot: String,
    pub count: u32,
    pub equipped: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Inventory {
    pub items: Vec<InventoryItem>,
    pub equipped: BTreeMap<String, String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Quest {
    pub id: String,
    pub category: String,
    pub title: String,
    pub goal: u32,
    pub skippable: bool,
    pub reward_item: Option<String>,
    pub reward_count: Option<u32>,
    pub progress: u32,
    pub state: QuestStatus,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GroupMember {
    pub player_id: String,
    pub handle: String,
    pub role: Role,
    pub side: u8,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GroupView {
    pub id: String,
    pub kind: GroupKind,
    pub name: String,
    pub tag: String,
    pub motto: String,
    pub mode: String,
    pub invite_only: bool,
    pub lobby_open: bool,
    pub join_code: Option<String>,
    pub created: i64,
    pub max_members: u8,
    pub member_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub members: Option<Vec<GroupMember>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GuildListing {
    pub id: String,
    pub name: String,
    pub tag: String,
    pub motto: String,
    pub member_count: u8,
    pub max_members: u8,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Invitation {
    pub group_id: String,
    pub name: String,
    pub inviter_id: String,
    pub inviter: String,
    pub created: i64,
}
