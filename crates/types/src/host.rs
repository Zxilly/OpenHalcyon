//! Match-host reservation, observation, and shared boundary error contracts.
use std::sync::Arc;
use uuid::Uuid;

use crate::{BotDifficulty, TalentLevels, player_actions};

#[derive(Debug, Clone)]
pub struct Reservation {
    pub talent_levels: Vec<TalentLevels>,
    pub match_id: Uuid,
    pub players: Vec<Uuid>,
    /// Parallel human team indices (0 or 1). Empty retains the default room layout.
    pub teams: Vec<u8>,
    pub mode: String,
    pub difficulty: Option<BotDifficulty>,
}

#[derive(Debug, Clone)]
pub struct HostedMatch {
    pub address: String,
}

#[derive(Debug, Clone)]
pub struct HostedSpectator {
    pub address: String,
    pub spectator_id: Uuid,
}

#[derive(Debug)]
pub struct NativeSeat {
    pub player: Uuid,
    pub accepted: bool,
}

#[derive(Debug)]
pub struct NativeOffer {
    pub id: Uuid,
    pub seats: Vec<NativeSeat>,
    pub entries: u32,
    pub expires: Option<i64>,
}

/// Persist a native request to close this exact spectator grant.
pub type SpectatorExit = Arc<dyn Fn(Uuid) -> HostResult<()> + Send + Sync>;

/// Error returned across `MatchHost` and `PeerPlayPort` crate boundaries.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HostError {
    #[error("authentication required")]
    Unauthorized,
    #[error("{0}")]
    Invalid(&'static str),
    #[error("{0}")]
    Conflict(&'static str),
    #[error("match host unavailable")]
    HostUnavailable,
    #[error("storage unavailable: {0}")]
    Database(String),
    #[error("platform worker unavailable")]
    WorkerUnavailable,
}

pub type HostResult<T> = std::result::Result<T, HostError>;

/// Implementations must make repeated reservations for the same match id idempotent.
pub trait MatchHost: Send + Sync + 'static {
    fn reserve(&self, reservation: Reservation) -> HostResult<HostedMatch>;

    fn local_action(
        &self,
        _room: Uuid,
        _player: Uuid,
        _call: player_actions::ActionCall,
    ) -> HostResult<player_actions::ActionReply> {
        Err(HostError::HostUnavailable)
    }

    /// Issue or refresh an isolated read-only credential for this viewer.
    fn spectate(&self, _match_id: Uuid, _viewer: Uuid) -> HostResult<HostedSpectator> {
        Err(HostError::HostUnavailable)
    }

    fn spectate_with_exit(
        &self,
        room: Uuid,
        viewer: Uuid,
        _exit: SpectatorExit,
    ) -> HostResult<HostedSpectator> {
        self.spectate(room, viewer)
    }

    /// Revoke the credential and close its connections. Repeated calls are harmless.
    fn stop_spectating(&self, _match_id: Uuid, _spectator_id: Uuid) -> HostResult<()> {
        Err(HostError::HostUnavailable)
    }
}
