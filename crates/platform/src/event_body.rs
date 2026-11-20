use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AbortReason {
    HostUnavailable,
    HostStopped,
    HostRestarted,
    OfferExpired,
    PlayerLeft,
    FederationClosed,
}

#[derive(Serialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub(crate) enum EventBody<'a> {
    Queued {
        mode: &'a str,
    },
    QueueLeft,
    MatchOffered {
        match_id: Uuid,
        mode: &'a str,
    },
    MatchPlaying {
        match_id: Uuid,
        address: &'a str,
    },
    MatchFinished {
        match_id: Uuid,
        result: Option<&'a Value>,
    },
    MatchAborted {
        match_id: Uuid,
        reason: AbortReason,
    },
    PostMatchLeft {
        match_id: Option<Uuid>,
    },
    Chat {
        match_id: Uuid,
        player_id: Uuid,
        text: &'a str,
    },
    SpectatorEntered {
        match_id: Uuid,
    },
    SpectatorLeft {
        match_id: Uuid,
    },
    FriendsChanged,
}

impl EventBody<'_> {
    pub(crate) fn encode(&self) -> crate::Result<String> {
        serde_json::to_string(self).map_err(|_| crate::PlatformError::WorkerUnavailable)
    }
}

/// Local and replicated phases stored in a platform lobby.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LobbyPhase {
    Pending,
    Queued,
    Offered,
    Reserving,
    Playing,
    Spectating,
    Finished,
    PostMatch,
    Closed,
    Aborted,
}

impl LobbyPhase {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "pending" => Some(Self::Pending),
            "queued" => Some(Self::Queued),
            "offered" => Some(Self::Offered),
            "reserving" => Some(Self::Reserving),
            "playing" => Some(Self::Playing),
            "spectating" => Some(Self::Spectating),
            "finished" => Some(Self::Finished),
            "post_match" => Some(Self::PostMatch),
            "closed" => Some(Self::Closed),
            "aborted" => Some(Self::Aborted),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Queued => "queued",
            Self::Offered => "offered",
            Self::Reserving => "reserving",
            Self::Playing => "playing",
            Self::Spectating => "spectating",
            Self::Finished => "finished",
            Self::PostMatch => "post_match",
            Self::Closed => "closed",
            Self::Aborted => "aborted",
        }
    }
}

/// Standard platform event tags. Extensions can retain their own event bodies.
#[derive(Debug, Copy, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Queued,
    QueueLeft,
    MatchOffered,
    MatchPlaying,
    MatchFinished,
    MatchAborted,
    PostMatchLeft,
    Chat,
    SpectatorEntered,
    SpectatorLeft,
    FriendsChanged,
}

impl crate::Event {
    pub fn kind(&self) -> Option<EventKind> {
        use serde::Deserialize;
        EventKind::deserialize(self.body.get("type")?).ok()
    }
}
