pub(crate) mod error;
pub(crate) mod profile;
pub(crate) mod queries;
pub(crate) mod state;

pub use error::{PlatformConfig, PlatformError, Result};
pub use profile::{Guest, Lobby, PlatformCounts, PlayerActivity, PlayerPage, PlayerProfile};
pub use queries::{Event, EventBatch};
pub use state::{AppState, MatchEventObserver};
pub(crate) use state::{
    abort, announce, credential, digest, display_name, insert_event, parse_id, player,
};
