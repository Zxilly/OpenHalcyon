mod economy;
mod exchange;
mod groups;
mod orm;
pub mod queue;
mod schema;
mod social;
pub mod views;

pub use economy::{ItemDefinition, QuestDefinition};
pub use exchange::Recipe;
pub use groups::{
    GroupKind, GroupSettings, Role,
    store::{PartySnapshot, party_invited, party_snapshot},
};
pub use social::may_spectate;

use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use diesel::SqliteConnection;
use diesel_migrations::MigrationHarness;
use rusqlite::{Connection, Transaction};
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
pub struct Community(Arc<Mutex<Connections>>);

struct Connections {
    legacy: Connection,
    orm: SqliteConnection,
}

impl Community {
    /// Can share a SQLite path with the platform. Only community tables are used.
    pub fn open(path: &Path) -> Result<Self> {
        let address = halcyon_storage::DatabaseAddress::new(path);
        let db = Connection::open(address.path())?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let mut orm = address.connect().map_err(orm::connection_error)?;
        orm.immediate_transaction(|db| {
            db.run_pending_migrations(orm::MIGRATIONS)
                .map_err(orm::connection_error)?;
            Ok::<_, CommunityError>(())
        })?;
        Ok(Self(Arc::new(Mutex::new(Connections { legacy: db, orm }))))
    }

    async fn run<T, F>(&self, work: F) -> Result<T>
    where
        F: FnOnce(&Transaction<'_>) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let db = self.0.clone();
        tokio::task::spawn_blocking(move || {
            let mut db = db.lock().map_err(|_| CommunityError::WorkerUnavailable)?;
            let tx = db
                .legacy
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let value = work(&tx)?;
            tx.commit()?;
            Ok(value)
        })
        .await
        .map_err(|_| CommunityError::WorkerUnavailable)?
    }

    /// Run a trusted integration operation inside a single Diesel transaction.
    pub async fn orm_transaction<T, F>(&self, work: F) -> Result<T>
    where
        F: FnOnce(&mut SqliteConnection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let db = self.0.clone();
        tokio::task::spawn_blocking(move || {
            let mut db = db.lock().map_err(|_| CommunityError::WorkerUnavailable)?;
            db.orm.immediate_transaction(work)
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
        self.orm_transaction(move |db| Self::execute_in_transaction(db, actor, action))
            .await
    }

    /// Apply an authenticated action alongside integration writes using the same transaction.
    pub fn execute_in_transaction(
        db: &mut SqliteConnection,
        actor: Uuid,
        action: Action,
    ) -> Result<Value> {
        let actor = actor.to_string();
        orm::ensure_player(db, &actor)?;
        match action {
            Action::Friends
            | Action::RequestFriend { .. }
            | Action::ResolveFriend { .. }
            | Action::RemoveFriend { .. }
            | Action::FavoriteFriend { .. }
            | Action::SetInvisible { .. }
            | Action::SetReceivePresence { .. }
            | Action::SetPresence { .. } => social::execute(db, &actor, action),
            Action::Inventory
            | Action::Equip { .. }
            | Action::SetLoadout { .. }
            | Action::Quests { .. }
            | Action::ClaimQuest { .. }
            | Action::SkipQuest { .. } => economy::store::execute(db, &actor, action),
            _ => groups::store::execute(db, &actor, action),
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
