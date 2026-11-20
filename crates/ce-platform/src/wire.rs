use crate::{LobbyPhase, LobbyState, PlayerProfile, expiry_in};
use serde::Serialize;
use uuid::Uuid;

#[derive(Default, Serialize)]
pub(crate) struct Empty {}

#[derive(Serialize)]
pub(crate) struct Success {
    pub success: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlayerHandle {
    #[serde(rename = "playerUUID")]
    pub player_uuid: Uuid,
    pub handle: String,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PartyNotice {
    #[serde(rename = "quintPartyStateUpdate")]
    StateUpdate,
    #[serde(rename = "quintPartyJoinRequest")]
    JoinRequest,
    #[serde(rename = "quintPartyPlayerLeft")]
    PlayerLeft,
    #[serde(rename = "quintPartyMemberKick")]
    MemberKick,
}

impl PartyNotice {
    pub fn from_wire(text: &str) -> Option<Self> {
        Some(match text {
            "quintPartyStateUpdate" => Self::StateUpdate,
            "quintPartyJoinRequest" => Self::JoinRequest,
            "quintPartyPlayerLeft" => Self::PlayerLeft,
            "quintPartyMemberKick" => Self::MemberKick,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(untagged)]
pub(crate) enum Seconds {
    Whole(i64),
    Fraction(f64),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StateSnapshot {
    pub state: LobbyState,
    pub ttl: Seconds,
    pub expiry: Seconds,
    pub version: Uuid,
    pub last_version: Uuid,
    pub num_players: usize,
    pub notify: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_id: Option<Option<Uuid>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spectator_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<Option<u16>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_queued_entries: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prev_state: Option<LobbyState>,
}

impl StateSnapshot {
    pub fn from_profile(profile: &PlayerProfile) -> Self {
        let lobby = profile.lobby.as_ref();
        let (state, ttl) = match lobby.and_then(|l| l.phase_kind()) {
            Some(LobbyPhase::Queued | LobbyPhase::Pending) => (LobbyState::PendingAuto, 10),
            Some(LobbyPhase::Offered | LobbyPhase::Reserving) => (LobbyState::MatchPending, 59),
            Some(LobbyPhase::Playing) => (LobbyState::Playing, 100),
            Some(LobbyPhase::Spectating) => (LobbyState::Spectating, 100),
            Some(LobbyPhase::Finished | LobbyPhase::PostMatch) => (LobbyState::PostMatch, 100),
            _ => (LobbyState::Menus, 100),
        };
        let expiry = match (state, lobby.and_then(|l| l.queued_at_ms)) {
            (LobbyState::Menus, _) => expiry_in(600),
            (LobbyState::PendingAuto, Some(joined)) => joined as f64 / 1000.0 + 3600.0,
            _ => expiry_in(ttl),
        };
        let game = lobby.filter(|_| matches!(state, LobbyState::Playing | LobbyState::Spectating));
        let endpoint = game
            .and_then(|l| l.address.as_deref())
            .and_then(|a| a.rsplit_once(':'));
        Self {
            state,
            ttl: Seconds::Whole(ttl as i64),
            expiry: Seconds::Fraction(expiry),
            version: Uuid::new_v4(),
            last_version: Uuid::nil(),
            num_players: 1,
            notify: matches!(state, LobbyState::PendingAuto | LobbyState::MatchPending),
            match_id: game.map(|l| l.match_id),
            spectator_id: game.and_then(|l| l.spectator_id),
            host: endpoint.map(|(host, _)| host.to_owned()),
            port: endpoint.map(|(_, port)| port.parse().ok()),
            site: game.map(|_| ""),
            timestamp: game.map(|_| timestamp()),
            num_queued_entries: None,
            prev_state: None,
        }
    }
}

pub(crate) fn timestamp() -> String {
    chrono::Utc::now().format("%m/%d/%y %H:%M:%S").to_string()
}
