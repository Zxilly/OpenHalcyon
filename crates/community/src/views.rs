//! Community response schemas shared by the SQLite projections and CE adapter.
use crate::{GroupKind, Role};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuestStatus {
    Active,
    Claimed,
    Skipped,
}

impl QuestStatus {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Claimed => "claimed",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Items<T> {
    pub items: Vec<T>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Contact {
    pub player_id: String,
    pub handle: String,
    pub favorite: bool,
    pub online: bool,
    pub status: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FriendRequest {
    pub player_id: String,
    pub handle: String,
    pub created: i64,
    pub incoming: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Friends {
    pub friends: Vec<Contact>,
    pub requests: Vec<FriendRequest>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryItem {
    pub id: String,
    pub name: String,
    pub slot: String,
    pub count: u32,
    pub equipped: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Inventory {
    pub items: Vec<InventoryItem>,
    pub equipped: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Quest {
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

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupMember {
    pub player_id: String,
    pub handle: String,
    pub role: Role,
    pub side: u8,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupView {
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

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildListing {
    pub id: String,
    pub name: String,
    pub tag: String,
    pub motto: String,
    pub member_count: u8,
    pub max_members: u8,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Invitation {
    pub group_id: String,
    pub name: String,
    pub inviter_id: String,
    pub inviter: String,
    pub created: i64,
}
