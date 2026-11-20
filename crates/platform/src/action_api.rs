use super::*;
use player_actions::{ActionCall, ActionContext, ActionDecision, ActionReply, ActionRequest};

impl AppState {
    pub async fn playing_player(&self, token: &str, room: Uuid) -> Result<Uuid> {
        let profile = self.profile(token).await?;
        if !profile.lobby.is_some_and(|l| {
            l.match_id == Some(room) && l.phase_kind() == Some(LobbyPhase::Playing)
        }) {
            return Err(PlatformError::Conflict("no playing seat in this match"));
        }
        Ok(profile.player_id)
    }
}
pub(super) async fn inspect(
    Extension(matches): Extension<Arc<dyn PublicMatches>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<ActionContext>> {
    match matches
        .player_action(bearer(&headers)?, id, ActionCall::Inspect)
        .await?
    {
        ActionReply::Context(c) => Ok(Json(c)),
        _ => Err(PlatformError::Conflict("no running seat in this match")),
    }
}
pub(super) async fn execute(
    Extension(matches): Extension<Arc<dyn PublicMatches>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(request): Json<ActionRequest>,
) -> Result<StatusCode> {
    let reply = matches
        .player_action(bearer(&headers)?, id, ActionCall::Execute(request))
        .await?;
    match reply {
        ActionReply::Decision(ActionDecision::Applied) => Ok(StatusCode::NO_CONTENT),
        ActionReply::Decision(ActionDecision::Invalid) => {
            Err(PlatformError::Invalid("invalid or expired action"))
        }
        ActionReply::Decision(ActionDecision::Stale) => {
            Err(PlatformError::Conflict("action sequence already used"))
        }
        ActionReply::Decision(ActionDecision::Rejected) => {
            Err(PlatformError::Conflict("action rejected by world"))
        }
        _ => Err(PlatformError::Conflict("no running seat in this match")),
    }
}
