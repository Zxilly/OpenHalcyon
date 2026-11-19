use halcyon_platform::PlayerProfile;
use serde_json::Value;
use uuid::Uuid;

use crate::{Error, Platform, player_info};

pub(crate) async fn begin(
    platform: &Platform,
    session: &str,
    params: &[Value],
) -> Result<Value, Error> {
    let target = params.get(1).ok_or(Error::Invalid)?;
    let object;
    let target = if let Some(raw) = target.as_str().filter(|s| s.trim_start().starts_with('{')) {
        object = serde_json::from_str::<Value>(raw).map_err(|_| Error::Invalid)?;
        &object
    } else {
        target
    };
    let friend = if let Some(handle) = target
        .as_object()
        .and_then(|fields| fields.get("handle").or_else(|| fields.get("playerHandle")))
        .and_then(Value::as_str)
    {
        platform.community.resolve_handle(handle.to_owned()).await?
    } else {
        let id = target
            .as_str()
            .or_else(|| {
                ["friendUUID", "playerUUID", "friendId", "playerId"]
                    .iter()
                    .find_map(|key| target.get(*key).and_then(Value::as_str))
            })
            .ok_or(Error::Invalid)?;
        Uuid::parse_str(id).map_err(|_| Error::Invalid)?
    };
    platform.app.start_spectating(session, friend).await?;
    platform.start_session(session, &platform.app.profile(session).await?)
}

pub(crate) async fn exit(platform: &Platform, session: &str) -> Result<Value, Error> {
    platform.app.leave_spectator(session).await?;
    Ok(player_info(&platform.app.profile(session).await?))
}

pub(crate) fn is_spectating(profile: &PlayerProfile) -> bool {
    profile
        .lobby
        .as_ref()
        .is_some_and(|lobby| lobby.phase == "spectating")
}
