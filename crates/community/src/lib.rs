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

use diesel::{Connection, SqliteConnection};
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
    Storage(#[from] halcyon_storage::Error),
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
pub struct Community(Arc<Mutex<SqliteConnection>>);

impl Community {
    /// Can share a SQLite path with the platform. Only community tables are used.
    pub fn open(path: &Path) -> Result<Self> {
        let address = halcyon_storage::DatabaseAddress::new(path);
        let mut orm = address.connect().map_err(orm::connection_error)?;
        halcyon_storage::ensure_schema(&mut orm)?;
        Ok(Self(Arc::new(Mutex::new(orm))))
    }

    /// Run a synchronous trusted integration operation inside a single Diesel transaction.
    pub fn orm_transaction_sync<T, F>(&self, work: F) -> Result<T>
    where
        F: FnOnce(&mut SqliteConnection) -> Result<T>,
    {
        let mut db = self
            .0
            .lock()
            .map_err(|_| CommunityError::WorkerUnavailable)?;
        db.immediate_transaction(work)
    }

    /// Run a trusted integration operation inside a single Diesel transaction.
    pub async fn orm_transaction<T, F>(&self, work: F) -> Result<T>
    where
        F: FnOnce(&mut SqliteConnection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        self.orm_try_transaction(work).await
    }

    /// Run a trusted integration write with caller-defined error mapping inside an immediate Diesel transaction.
    pub async fn orm_try_transaction<T, E, F>(&self, work: F) -> std::result::Result<T, E>
    where
        F: FnOnce(&mut SqliteConnection) -> std::result::Result<T, E> + Send + 'static,
        T: Send + 'static,
        E: From<CommunityError> + From<diesel::result::Error> + Send + 'static,
    {
        self.with_orm_result(move |db| db.immediate_transaction(work))
            .await
    }

    /// Run a trusted integration read inside a single deferred Diesel transaction.
    pub async fn orm_read<T, E, F>(&self, work: F) -> std::result::Result<T, E>
    where
        F: FnOnce(&mut SqliteConnection) -> std::result::Result<T, E> + Send + 'static,
        T: Send + 'static,
        E: From<CommunityError> + From<diesel::result::Error> + Send + 'static,
    {
        self.with_orm_result(move |db| db.transaction(work)).await
    }

    async fn with_orm<T, F>(&self, work: F) -> Result<T>
    where
        F: FnOnce(&mut SqliteConnection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        self.with_orm_result(work).await
    }

    async fn with_orm_result<T, E, F>(&self, work: F) -> std::result::Result<T, E>
    where
        F: FnOnce(&mut SqliteConnection) -> std::result::Result<T, E> + Send + 'static,
        T: Send + 'static,
        E: From<CommunityError> + Send + 'static,
    {
        let db = self.0.clone();
        tokio::task::spawn_blocking(move || {
            let mut db = db.lock().map_err(|_| CommunityError::WorkerUnavailable)?;
            work(&mut db)
        })
        .await
        .map_err(|_| E::from(CommunityError::WorkerUnavailable))?
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
