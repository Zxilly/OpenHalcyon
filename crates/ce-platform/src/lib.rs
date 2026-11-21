//! Compatibility layer for the Android Community Edition platform protocol.
pub use halcyon_types::ce::{LobbyState, RpcMethod};

mod auth;
mod catalog;
mod match_flow;
mod notify;
mod rpc;
mod social;
mod store;

pub use auth::{ProviderConfig, ProviderProduct};
pub use catalog::TalentProgress;
pub use match_flow::PingSite;

use auth::Tokens;
use axum::Router;
use halcyon_platform::{AppState, PlatformError};
use halcyon_types::{HostError, PeerPlayPort};
use serde_json::Value;
use std::{
    collections::HashMap,
    path::Path as FsPath,
    sync::{Arc, Mutex},
};
use store::meta::Meta;
use thiserror::Error;
use uuid::Uuid;

pub(crate) use rpc::dispatch::{expiry_in, now};

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid arguments")]
    Invalid,
    #[error("invalid credentials")]
    Unauthorized,
    #[error("unsupported method")]
    Unsupported,
    #[error("{0}")]
    Unavailable(&'static str),
    #[error(transparent)]
    Community(#[from] halcyon_community::CommunityError),
    #[error("internal platform error")]
    Internal,
    #[error("metadata storage error")]
    Storage(#[from] halcyon_storage::Error),
    #[error("ORM storage error")]
    Orm(#[from] diesel::result::Error),
    #[error("{0}")]
    Config(&'static str),
    #[error(transparent)]
    Platform(#[from] PlatformError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl From<HostError> for Error {
    fn from(error: HostError) -> Self {
        Self::Platform(error.into())
    }
}

pub struct Config {
    /// Public host:port returned by bootstrap.
    pub authority: String,
    /// Public plaintext notify HTTP and WS authority.
    pub notify_authority: String,
}

pub struct Platform {
    pub(crate) game_data: Option<Arc<halcyon_data::GameData>>,
    pub(crate) app: AppState,
    pub(crate) config: Config,
    pub(crate) tokens: Tokens,
    pub(crate) grants: notify::Grants,
    pub(crate) native_state_updates: bool,
    pub(crate) unlock_all_talents: bool,
    pub(crate) xmpp: social::xmpp::Chat,
    pub(crate) sites: match_flow::ping::Sites,
    pub(crate) versions: Mutex<HashMap<Uuid, (Uuid, u64)>>,
    pub(crate) meta: Meta,
    pub(crate) provider: Option<auth::provider::Verifier>,
    pub(crate) community: halcyon_community::Community,
    pub(crate) content: catalog::content::Content,
    pub(crate) peers: Option<Arc<dyn PeerPlayPort>>,
    pub(crate) talents: catalog::talents::Catalog,
    pub(crate) skins: Value,
    pub(crate) archived_manifests: bool,
}

impl Platform {
    pub fn new(
        app: AppState,
        config: Config,
        secret: &[u8],
        database: &FsPath,
        manifests: Option<&FsPath>,
    ) -> Result<Arc<Self>, Error> {
        rpc::dispatch::create_platform(app, config, secret, database, manifests)
    }

    /// Attach the same immutable pack used by the match host before routing requests.
    pub fn configure_game_data(
        platform: &mut Arc<Self>,
        data: Option<Arc<halcyon_data::GameData>>,
    ) -> Result<(), Error> {
        rpc::dispatch::attach_game_data(platform, data)
    }

    /// Opt in to the native client's state subscription.
    pub fn configure_native_state_updates(
        platform: &mut Arc<Self>,
        enabled: bool,
    ) -> Result<(), Error> {
        Arc::get_mut(platform)
            .ok_or(Error::Config(
                "configure state updates before sharing the platform",
            ))?
            .native_state_updates = enabled;
        Ok(())
    }

    /// Unlock all hero talents with Vainglory CE official guest levels (`10/10/5`).
    pub fn configure_unlock_all_talents(
        platform: &mut Arc<Self>,
        enabled: bool,
    ) -> Result<(), Error> {
        Arc::get_mut(platform)
            .ok_or(Error::Config(
                "configure talent defaults before sharing the platform",
            ))?
            .unlock_all_talents = enabled;
        Ok(())
    }

    pub fn with_peers(
        app: AppState,
        config: Config,
        secret: &[u8],
        database: &FsPath,
        manifests: Option<&FsPath>,
        peers: Arc<dyn PeerPlayPort>,
    ) -> Result<Arc<Self>, Error> {
        rpc::dispatch::with_peers_platform(app, config, secret, database, manifests, peers)
    }

    pub(crate) async fn accept(&self, session: &str, id: Uuid) -> Result<(), Error> {
        halcyon_platform::PublicMatches::accept_offer(self, session, id).await?;
        Ok(())
    }

    pub fn bootstrap(self: &Arc<Self>) -> Router {
        rpc::dispatch::bootstrap_router(self.clone())
    }

    pub fn rpc(self: &Arc<Self>) -> Router {
        rpc::dispatch::rpc_router(self.clone())
    }

    pub fn notifications(self: &Arc<Self>) -> Router {
        rpc::dispatch::notify_router(self.clone())
    }
}
