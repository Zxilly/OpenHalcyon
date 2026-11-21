use crate::{LobbyState, expiry_in};
use axum::http::uri::Authority;
use halcyon_platform::{LobbyPhase, PlayerProfile};
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

#[derive(Clone, Copy, Serialize, strum::EnumString, strum::IntoStaticStr)]
#[serde(into = "&'static str")]
pub(crate) enum PartyNotice {
    #[strum(serialize = "quintPartyStateUpdate")]
    StateUpdate,
    #[strum(serialize = "quintPartyJoinRequest")]
    JoinRequest,
    #[strum(serialize = "quintPartyPlayerLeft")]
    PlayerLeft,
    #[strum(serialize = "quintPartyMemberKick")]
    MemberKick,
}

impl PartyNotice {
    pub fn as_str(self) -> &'static str {
        self.into()
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
        let endpoint = game.and_then(|l| l.address.as_deref()).and_then(endpoint);
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
            host: endpoint.as_ref().map(|address| address.host().to_owned()),
            port: endpoint.as_ref().map(Authority::port_u16),
            site: game.map(|_| ""),
            timestamp: game.map(|_| timestamp()),
            num_queued_entries: None,
            prev_state: None,
        }
    }
}

fn endpoint(value: &str) -> Option<Authority> {
    let address: Authority = value.parse().ok()?;
    let host = address.host();
    // A missing explicit port omits both fields. An explicit malformed
    // port on a valid host preserves the historical null port field.
    (!host.is_empty()
        && address
            .as_str()
            .strip_prefix(host)
            .is_some_and(|rest| rest.starts_with(':'))
        && !address.as_str().contains('@'))
    .then_some(address)
}

pub(crate) fn timestamp() -> String {
    chrono::Utc::now().format("%m/%d/%y %H:%M:%S").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use halcyon_platform::Lobby;
    use serde_json::Value;

    #[test]
    fn connection_fields_preserve_brackets_and_distinguish_null_from_omission() {
        let id = Uuid::new_v4();
        for (address, expected) in [
            (Some("127.0.0.1:65535"), Some(("127.0.0.1", Some(65535)))),
            (
                Some("[2001:db8::1]:443"),
                Some(("[2001:db8::1]", Some(443))),
            ),
            (Some("room.example:65536"), Some(("room.example", None))),
            (Some("room.example:bad"), Some(("room.example", None))),
            (Some("room.example:"), Some(("room.example", None))),
            (None, None),
            (Some("room.example"), None),
            (Some("[2001:db8::1]"), None),
            (Some("2001:db8::1:443"), None),
            (Some("user@room.example:443"), None),
            (Some("room.example:443/path"), None),
            (Some(":443"), None),
        ] {
            let profile = PlayerProfile {
                player_id: Uuid::new_v4(),
                name: "Guest".into(),
                lobby: Some(Lobby {
                    mode: halcyon_types::ce::KnownQueue::CasualAral.as_str().into(),
                    match_id: Some(id),
                    phase: LobbyPhase::Playing.as_str().into(),
                    address: address.map(str::to_owned),
                    result: None,
                    spectator_id: None,
                    settled_at_ms: None,
                    queued_at_ms: None,
                }),
            };
            let snapshot = serde_json::to_value(StateSnapshot::from_profile(&profile)).unwrap();
            assert_eq!(snapshot["matchId"], id.to_string(), "{address:?}");
            match expected {
                Some((host, port)) => {
                    assert_eq!(snapshot["host"], host, "{address:?}");
                    assert_eq!(
                        snapshot.get("port"),
                        Some(&port.map_or(Value::Null, Value::from)),
                        "{address:?}"
                    );
                }
                None => {
                    assert!(snapshot.get("host").is_none(), "{address:?}");
                    assert!(snapshot.get("port").is_none(), "{address:?}");
                }
            }
        }
    }
}
