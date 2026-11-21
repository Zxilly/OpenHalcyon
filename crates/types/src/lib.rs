//! Leaf wire contracts and port traits shared across workspace crates.

pub mod ce;
pub mod difficulty;
pub mod host;
pub mod peer_play;
pub mod player_actions;
pub mod skin;

/// Wire opcode tag for `0x0453` (`ActionDraftCatalogEntry`) hero and skin catalog entries.
pub const DRAFT_CATALOG_ENTRY_TAG: u16 = 0x0453;

pub use ce::{KnownQueue, LobbyState, NativeMode, RpcMethod};
pub use difficulty::{BotDifficulty, TalentLevels};
pub use host::{
    HostError, HostResult, HostedMatch, HostedSpectator, MatchHost, NativeOffer, NativeSeat,
    Reservation, SpectatorExit,
};
pub use peer_play::{BoxFuture, PeerPlayPort};
pub use skin::skin_hash;
