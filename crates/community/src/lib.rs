mod economy;
mod exchange;
mod groups;
pub mod queue;
mod social;
pub mod views;

pub use economy::{ItemDefinition, QuestDefinition};
pub use exchange::Recipe;
pub use groups::{GroupKind, GroupSettings, Role};
pub use social::may_spectate;

use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use rusqlite::{Connection, OptionalExtension, Transaction};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum CommunityError {
    #[error("{0}")]
    Invalid(&'static str),
    #[error("{0}")]
    Conflict(&'static str),
    #[error("{0}")]
    Missing(&'static str),
    #[error("operation is not permitted")]
    Forbidden,
    #[error("community storage unavailable: {0}")]
    Storage(#[from] rusqlite::Error),
    #[error("could not serialize community data: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("community worker unavailable")]
    WorkerUnavailable,
}

pub type Result<T> = std::result::Result<T, CommunityError>;

pub enum Action {
    Friends,
    RequestFriend {
        target: Uuid,
    },
    ResolveFriend {
        requester: Uuid,
        accept: bool,
    },
    RemoveFriend {
        target: Uuid,
    },
    FavoriteFriend {
        target: Uuid,
        favorite: bool,
    },
    SetInvisible {
        invisible: bool,
    },
    SetReceivePresence {
        receive: bool,
    },
    SetPresence {
        status: String,
    },
    CreateGroup {
        kind: GroupKind,
        name: String,
        settings: GroupSettings,
    },
    Group {
        kind: GroupKind,
    },
    GroupById {
        group: Uuid,
    },
    GroupInvites {
        kind: GroupKind,
    },
    SearchGuilds {
        term: String,
        limit: usize,
    },
    BrowseOpenGuilds {
        page: u32,
    },
    Invite {
        kind: GroupKind,
        target: Uuid,
    },
    ResolveInvite {
        group: Uuid,
        accept: bool,
    },
    JoinOpenGuild {
        group: Uuid,
    },
    ChangeGuildMembership {
        group: Uuid,
        join: bool,
    },
    JoinPartyByCode {
        code: String,
    },
    LeaveGroup {
        kind: GroupKind,
    },
    DisbandGroup {
        kind: GroupKind,
    },
    Kick {
        kind: GroupKind,
        target: Uuid,
    },
    SetRole {
        kind: GroupKind,
        target: Uuid,
        role: Role,
    },
    TransferOwnership {
        kind: GroupKind,
        target: Uuid,
    },
    UpdateGroup {
        kind: GroupKind,
        settings: GroupSettings,
    },
    MovePartyMember {
        target: Uuid,
        side: u8,
    },
    BalanceParty,
    SetPartyLobby {
        open: bool,
    },
    UpdatePartyState {
        mode: Option<String>,
        open: Option<bool>,
    },
    Inventory,
    Equip {
        slot: String,
        item: Option<String>,
    },
    SetLoadout {
        slots: std::collections::BTreeMap<String, Option<String>>,
    },
    Quests {
        category: Option<String>,
    },
    ClaimQuest {
        quest: String,
    },
    SkipQuest {
        quest: String,
    },
}

#[derive(Clone)]
pub struct Community(Arc<Mutex<Connection>>);

impl Community {
    /// Can share a SQLite path with the platform. Only community tables are used.
    pub fn open(path: &Path) -> Result<Self> {
        let db = Connection::open(path)?;
        db.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
             CREATE TABLE IF NOT EXISTS community_players (
                id TEXT PRIMARY KEY, handle TEXT NOT NULL, online INTEGER NOT NULL,
                invisible INTEGER NOT NULL DEFAULT 0, receive_presence INTEGER NOT NULL DEFAULT 1,
                status TEXT NOT NULL DEFAULT '', last_seen INTEGER NOT NULL DEFAULT (unixepoch())
             );
             CREATE INDEX IF NOT EXISTS community_handles ON community_players(handle COLLATE NOCASE);
             CREATE TABLE IF NOT EXISTS community_friend_requests (
                requester TEXT NOT NULL REFERENCES community_players(id),
                recipient TEXT NOT NULL REFERENCES community_players(id),
                created INTEGER NOT NULL DEFAULT (unixepoch()),
                PRIMARY KEY(requester, recipient)
             );
             CREATE TABLE IF NOT EXISTS community_friends (
                player TEXT NOT NULL REFERENCES community_players(id),
                friend TEXT NOT NULL REFERENCES community_players(id),
                favorite INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(player, friend)
             );
             CREATE TABLE IF NOT EXISTS community_groups (
                id TEXT PRIMARY KEY, kind TEXT NOT NULL, name TEXT NOT NULL,
                tag TEXT NOT NULL DEFAULT '', motto TEXT NOT NULL DEFAULT '',
                mode TEXT NOT NULL DEFAULT '', invite_only INTEGER NOT NULL DEFAULT 1,
                lobby_open INTEGER NOT NULL DEFAULT 0, code TEXT UNIQUE,
                created INTEGER NOT NULL DEFAULT (unixepoch())
             );
             CREATE TABLE IF NOT EXISTS community_members (
                group_id TEXT NOT NULL REFERENCES community_groups(id) ON DELETE CASCADE,
                kind TEXT NOT NULL, player TEXT NOT NULL REFERENCES community_players(id),
                role TEXT NOT NULL, side INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY(group_id, player), UNIQUE(kind, player)
             );
             CREATE TABLE IF NOT EXISTS community_invites (
                group_id TEXT NOT NULL REFERENCES community_groups(id) ON DELETE CASCADE,
                player TEXT NOT NULL REFERENCES community_players(id),
                inviter TEXT NOT NULL REFERENCES community_players(id),
                created INTEGER NOT NULL DEFAULT (unixepoch()),
                PRIMARY KEY(group_id, player)
             );
             CREATE TABLE IF NOT EXISTS community_items (
                id TEXT PRIMARY KEY, slot TEXT NOT NULL, name TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS community_inventory (
                player TEXT NOT NULL REFERENCES community_players(id),
                item TEXT NOT NULL REFERENCES community_items(id), count INTEGER NOT NULL CHECK(count > 0),
                PRIMARY KEY(player, item)
             );
             CREATE TABLE IF NOT EXISTS community_equipment (
                player TEXT NOT NULL REFERENCES community_players(id),
                slot TEXT NOT NULL, item TEXT NOT NULL REFERENCES community_items(id),
                PRIMARY KEY(player, slot)
             );
             CREATE TABLE IF NOT EXISTS community_item_grants (
                id TEXT PRIMARY KEY, player TEXT NOT NULL REFERENCES community_players(id),
                item TEXT NOT NULL REFERENCES community_items(id), count INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS community_quests (
                id TEXT PRIMARY KEY, category TEXT NOT NULL, title TEXT NOT NULL,
                goal INTEGER NOT NULL CHECK(goal > 0), skippable INTEGER NOT NULL,
                reward_item TEXT REFERENCES community_items(id),
                reward_count INTEGER
             );
             CREATE TABLE IF NOT EXISTS community_quest_states (
                player TEXT NOT NULL REFERENCES community_players(id),
                quest TEXT NOT NULL REFERENCES community_quests(id), progress INTEGER NOT NULL DEFAULT 0,
                state TEXT NOT NULL DEFAULT 'active', PRIMARY KEY(player, quest)
             );
             CREATE TABLE IF NOT EXISTS community_quest_progress (
                id TEXT PRIMARY KEY, player TEXT NOT NULL REFERENCES community_players(id),
                quest TEXT NOT NULL REFERENCES community_quests(id), amount INTEGER NOT NULL
             );",
        )?;
        queue::initialize(&db)?;
        exchange::initialize(&db)?;
        Ok(Self(Arc::new(Mutex::new(db))))
    }

    async fn run<T, F>(&self, work: F) -> Result<T>
    where
        F: FnOnce(&Transaction<'_>) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let db = self.0.clone();
        tokio::task::spawn_blocking(move || {
            let mut db = db.lock().map_err(|_| CommunityError::WorkerUnavailable)?;
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let value = work(&tx)?;
            tx.commit()?;
            Ok(value)
        })
        .await
        .map_err(|_| CommunityError::WorkerUnavailable)?
    }

    /// Run a trusted integration operation in the community database transaction.
    pub async fn transaction<T, F>(&self, work: F) -> Result<T>
    where
        F: FnOnce(&Transaction<'_>) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        self.run(work).await
    }

    /// Execute actions as an already-authenticated player.
    pub async fn execute(&self, actor: Uuid, action: Action) -> Result<Value> {
        self.run(move |tx| Self::execute_in_transaction(tx, actor, action))
            .await
    }

    /// Apply an authenticated action alongside other writes in the same database transaction.
    pub fn execute_in_transaction(
        tx: &Transaction<'_>,
        actor: Uuid,
        action: Action,
    ) -> Result<Value> {
        let actor = actor.to_string();
        ensure_player(tx, &actor)?;
        match action {
            Action::Friends
            | Action::RequestFriend { .. }
            | Action::ResolveFriend { .. }
            | Action::RemoveFriend { .. }
            | Action::FavoriteFriend { .. }
            | Action::SetInvisible { .. }
            | Action::SetReceivePresence { .. }
            | Action::SetPresence { .. } => social::execute(tx, &actor, action),
            Action::Inventory
            | Action::Equip { .. }
            | Action::SetLoadout { .. }
            | Action::Quests { .. }
            | Action::ClaimQuest { .. }
            | Action::SkipQuest { .. } => economy::execute(tx, &actor, action),
            _ => groups::execute(tx, &actor, action),
        }
    }
}

fn ensure_player(tx: &Transaction<'_>, player: &str) -> Result<()> {
    if !tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM community_players WHERE id=?)",
        [player],
        |row| row.get(0),
    )? {
        return Err(CommunityError::Missing("player does not exist"));
    }
    Ok(())
}

fn validated(value: String, max: usize, required: bool) -> Result<String> {
    let value = value.trim().to_owned();
    if value.chars().count() > max
        || (required && value.is_empty())
        || value.chars().any(char::is_control)
    {
        return Err(CommunityError::Invalid("invalid text"));
    }
    Ok(value)
}

fn optional_row<T>(result: rusqlite::Result<T>, message: &'static str) -> Result<T> {
    result.optional()?.ok_or(CommunityError::Missing(message))
}
