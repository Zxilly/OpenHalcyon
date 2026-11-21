pub use halcyon_data::{
    ARAL_TALENT_HEROES, ARAL_TALENT_RULES_PREFIX, BotDifficulty, BotDifficultyExt, KnownQueue,
    LEGACY_ARAL_QUEUE, LEGACY_ARAL_SEATS, TalentLevels,
};
pub use halcyon_types::{
    HostError, HostResult, HostedMatch, HostedSpectator, MatchHost, NativeOffer, NativeSeat,
    Reservation, SpectatorExit, player_actions,
};

mod events;
mod http;
mod match_lifecycle;
mod service;

use events::event_body::EventBody;
pub use events::event_body::{AbortReason, EventKind, LobbyPhase};
pub use http::{Chat, CreateGuest, PublicMatches, Queue, ResumeGuest, router, router_with_matches};
pub use match_lifecycle::{
    FederatedIntent, FederatedOfferBinding, PooledSpectator, plan_teams, plan_teams_for_mode,
};
pub use service::{
    AppState, Event, EventBatch, Guest, Lobby, MatchEventObserver, PlatformConfig, PlatformCounts,
    PlatformError, PlayerActivity, PlayerPage, PlayerProfile, Result,
};
