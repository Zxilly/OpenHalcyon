//! Compatibility layer for the Android Community Edition platform protocol.
mod auth;
mod community;
mod meta;
mod notify;
use auth::{Scope, Tokens};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    routing::{get, post},
};
use halcyon_platform::{AppState, Guest, PlatformError, PlayerProfile};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::Path as FsPath,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid arguments")]
    Invalid,
    #[error("invalid credentials")]
    Unauthorized,
    #[error("unsupported method")]
    Unsupported,
    #[error(transparent)]
    Community(#[from] halcyon_community::CommunityError),
    #[error("internal platform error")]
    Internal,
    #[error("metadata storage error")]
    Storage(#[from] rusqlite::Error),
    #[error("{0}")]
    Config(&'static str),
    #[error(transparent)]
    Platform(#[from] PlatformError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub struct Config {
    /// Public host:port returned by bootstrap.
    pub authority: String,
    /// Public plaintext notify HTTP and WS authority.
    pub notify_authority: String,
}

pub struct Platform {
    app: AppState,
    config: Config,
    tokens: Tokens,
    grants: notify::Grants,
    versions: Mutex<HashMap<Uuid, (Uuid, u64)>>,
    meta: meta::Meta,
    community: halcyon_community::Community,
    peers: Option<halcyon_peer_play::PeerPlay>,
    talents: Value,
    skins: Value,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl Platform {
    pub fn new(
        app: AppState,
        config: Config,
        secret: &[u8],
        database: &FsPath,
        manifests: Option<&FsPath>,
    ) -> Result<Arc<Self>, Error> {
        for authority in [&config.authority, &config.notify_authority] {
            authority
                .parse::<axum::http::uri::Authority>()
                .ok()
                .filter(|a| a.port_u16().is_some() && !a.as_str().contains('@'))
                .ok_or(Error::Config("expected public host:port"))?;
        }
        let load = |name: &str, default: Value| -> Result<Value, Error> {
            let Some(dir) = manifests else {
                return Ok(default);
            };
            // Operator-supplied archive envelopes may contain account credentials.
            // Retain only the public manifest payload.
            let value: Value = serde_json::from_reader(std::fs::File::open(dir.join(name))?)?;
            value
                .get("returnValue")
                .cloned()
                .ok_or(Error::Config("manifest is missing returnValue"))
        };
        Ok(Arc::new(Self {
            app,
            config,
            tokens: Tokens::new(secret)?,
            grants: Default::default(),
            versions: Default::default(),
            meta: meta::Meta::open(database)?,
            community: halcyon_community::Community::open(database)?,
            peers: None,
            talents: load("official_getTalentsData.json", json!({}))?,
            skins: load("official_getSkinManifest.json", json!("{}"))?,
        }))
    }

    pub fn with_peers(
        app: AppState,
        config: Config,
        secret: &[u8],
        database: &FsPath,
        manifests: Option<&FsPath>,
        peers: halcyon_peer_play::PeerPlay,
    ) -> Result<Arc<Self>, Error> {
        let mut platform = Self::new(app, config, secret, database, manifests)?;
        Arc::get_mut(&mut platform)
            .expect("new platform has one owner")
            .peers = Some(peers);
        Ok(platform)
    }
    async fn accept(&self, session: &str, id: Uuid) -> Result<(), Error> {
        match &self.peers {
            Some(peers) => peers.accept(session, id).await?,
            None => self.app.accept(session, id).await?,
        }
        Ok(())
    }
    pub fn bootstrap(self: &Arc<Self>) -> Router {
        Router::new()
            .route(
                "/kindred/live/147219-status-redirect",
                get(
                    |State(s): State<Arc<Self>>| async move { format!("{}\n", s.config.authority) },
                ),
            )
            .with_state(self.clone())
    }
    pub fn rpc(self: &Arc<Self>) -> Router {
        Router::new()
            .route("/JSONRpc/{method}", post(rpc))
            .layer(DefaultBodyLimit::max(256 * 1024))
            .with_state(self.clone())
    }
    pub fn notifications(self: &Arc<Self>) -> Router {
        notify::router(self.clone())
    }
    fn guest_response(
        &self,
        guest: Guest,
        recovery: Option<&str>,
    ) -> Result<(String, Value), Error> {
        let token = match guest.recovery_token.as_deref() {
            Some(key) => self.tokens.sign(guest.player_id, key, Scope::Guest)?,
            None => recovery.ok_or(Error::Unauthorized)?.to_string(),
        };
        Ok((
            String::new(),
            json!({"playerUUID":token,"startSessionUrl":format!("https://{}", self.config.authority)}),
        ))
    }
}

#[derive(Deserialize)]
struct Request {
    method: String,
    params: Vec<Value>,
}

async fn rpc(
    State(s): State<Arc<Platform>>,
    Path(method): Path<String>,
    body: Bytes,
) -> Json<Value> {
    let result = match serde_json::from_slice::<Request>(&body) {
        Ok(request) if request.method == method => dispatch(&s, &method, &request.params).await,
        _ => Err(Error::Invalid),
    };
    tracing::debug!(method, success = result.is_ok(), "CE RPC");
    let value = match result {
        Ok((token, rv)) => json!({"code":0,"sessionToken":token,"returnValue":rv}),
        Err(err) => {
            let (code, reason) = match &err {
                Error::Unauthorized | Error::Platform(PlatformError::Unauthorized) => {
                    (-1, "invalid credentials")
                }
                Error::Unsupported => (-32601, "method not implemented"),
                Error::Invalid
                | Error::Platform(PlatformError::Invalid(_))
                | Error::Community(halcyon_community::CommunityError::Invalid(_)) => {
                    (-32602, "invalid parameters")
                }
                Error::Platform(PlatformError::Conflict(_))
                | Error::Community(halcyon_community::CommunityError::Conflict(_)) => {
                    (-2, "state conflict")
                }
                Error::Community(halcyon_community::CommunityError::Missing(_)) => {
                    (-4, "resource not found")
                }
                Error::Community(halcyon_community::CommunityError::Forbidden) => {
                    (-5, "operation forbidden")
                }
                _ => (-3, "service unavailable"),
            };
            tracing::debug!(method,%code,reason,"CE request failed");
            json!({"code":code,"sessionToken":"","returnValue":null,"reason":reason})
        }
    };
    Json(value)
}

fn arg(params: &[Value], i: usize) -> Result<&str, Error> {
    params.get(i).and_then(Value::as_str).ok_or(Error::Invalid)
}

async fn dispatch(s: &Platform, m: &str, params: &[Value]) -> Result<(String, Value), Error> {
    if m == "createGuestPlayer" {
        if params.len() != 13 || arg(params, 0)? != "kindred" || arg(params, 8)? != "147219" {
            return Err(Error::Invalid);
        }
        return s.guest_response(s.app.create_guest("Guest".into()).await?, None);
    }
    let token = arg(params, 0)?;
    if m == "getPlayerForGuestAccount" {
        let claims = s.tokens.verify(token, Scope::Guest)?;
        let guest = s.app.resume_guest(claims.credential).await?;
        return s.guest_response(guest, Some(token));
    }
    let session = match s.tokens.verify(token, Scope::Session) {
        Ok(claims) => claims.credential,
        Err(_) if m == "startSessionForPlayer" || m == "authenticate" => {
            let resume = s.tokens.verify(token, Scope::Guest)?;
            s.app.resume_guest(resume.credential).await?.session_token
        }
        Err(err) => return Err(err),
    };
    let profile = s.app.profile(&session).await?;
    let id = profile.player_id;
    let mut rv = match m {
        "authenticate" | "startSessionForPlayer" => {
            s.community
                .observe_player(id, profile.name.clone(), true)
                .await?;
            let mut rv = s.start_session(&session, &profile)?;
            rv["playerConfig"] = s.meta.prefs(id, "platform_config").await?;
            rv
        }
        "getPlayerInfo" => {
            let mut info = player_info(&profile);
            if let Some(v) = s.meta.prefs(id, "tutorial").await?.as_i64() {
                info["tutorialState"] = json!(v);
            }
            info
        }
        "getSkinManifest" => s.skins.clone(),
        "getTalentsData" => s.talents.clone(),
        "update" | "queryPendingMatch" => {
            if let Some(lobby) = &profile.lobby {
                if lobby.mode == "5v5_bots_solo" && lobby.phase == "offered" {
                    let mut snapshot = state(&profile);
                    s.accept(&session, lobby.match_id.ok_or(Error::Internal)?)
                        .await?;
                    // The local bot host is already allocated; CE polls again at expiry.
                    snapshot["ttl"] = json!(1);
                    snapshot["expiry"] = json!(now() + 1);
                    snapshot
                } else {
                    state(&profile)
                }
            } else {
                state(&profile)
            }
        }
        "joinLobby" => {
            let lobby = params.get(1).ok_or(Error::Invalid)?;
            let parsed;
            let obj = if let Some(raw) = lobby.as_str() {
                parsed = serde_json::from_str::<Value>(raw)?;
                &parsed
            } else {
                lobby
            };
            let mode = obj
                .get("lobby")
                .and_then(Value::as_str)
                .ok_or(Error::Invalid)?;
            let seats = if mode == "5v5_bots_solo" {
                1
            } else {
                s.app.seats_per_match()
            };
            if let Some(peers) = &s.peers {
                peers
                    .join(
                        &session,
                        mode.to_owned(),
                        u8::try_from(seats).map_err(|_| Error::Invalid)?,
                    )
                    .await?;
            } else {
                s.app
                    .join_with_seats(&session, mode.to_owned(), seats)
                    .await?;
            }
            json!({"success":true,"ttl":100})
        }
        "acceptMatch" => {
            let room = profile
                .lobby
                .as_ref()
                .and_then(|l| l.match_id)
                .ok_or(Error::Invalid)?;
            s.accept(&session, room).await?;
            s.start_session(&session, &s.app.profile(&session).await?)?
        }
        "exitLobby" | "rejectMatch" | "notifyExitPostMatch" => {
            if profile.lobby.is_some() {
                match &s.peers {
                    Some(peers) => peers.leave(&session).await?,
                    None => s.app.leave(&session).await?,
                }
            }
            if m == "notifyExitPostMatch" {
                json!({"success":true})
            } else {
                state(&s.app.profile(&session).await?)
            }
        }
        "renamePlayerHandle" | "setPlayerHandle" => {
            s.app.rename(&session, arg(params, 1)?.to_owned()).await?;
            let updated = s.app.profile(&session).await?;
            s.community
                .observe_player(id, updated.name.clone(), true)
                .await?;
            player_info(&updated)
        }
        "updatePlatformPlayerConfig" => {
            let field = params.get(1).ok_or(Error::Invalid)?;
            let v = if let Some(raw) = field.as_str() {
                serde_json::from_str(raw).map_err(|_| Error::Invalid)?
            } else {
                field.clone()
            };
            if !v.is_object() {
                return Err(Error::Invalid);
            }
            s.meta.set(id, "platform_config", v).await?;
            json!({})
        }
        "setTutorialState" => {
            s.meta
                .set(id, "tutorial", params.get(1).ok_or(Error::Invalid)?.clone())
                .await?;
            let mut info = player_info(&profile);
            info["tutorialState"] = params[1].clone();
            info
        }
        "recordMatchExperienceMetrics" => {
            let room = Uuid::parse_str(arg(params, 1)?).map_err(|_| Error::Invalid)?;
            s.meta.metrics(id, room, params[2..].to_vec()).await?;
            json!({})
        }
        "notifyPlayerAction" => {
            s.meta
                .set(id, "recent_action", Value::Array(params[1..].to_vec()))
                .await?;
            player_info(&profile)
        }
        "getLeaderboardData" => s.meta.ranking().await?,
        "getLiveEventData" => json!({"events":[]}),
        "endSession" => {
            s.app.end_session(&session).await?;
            s.community
                .observe_player(id, profile.name.clone(), false)
                .await?;
            return Ok((
                String::new(),
                state(&PlayerProfile {
                    lobby: None,
                    ..profile
                }),
            ));
        }
        _ => {
            let patch = community::dispatch(&s.community, &profile, m, params).await?;
            if matches!(
                m,
                "partyLobbyEnter"
                    | "partyLobbyExit"
                    | "quintPartyLobbyEnter"
                    | "getInventoryGroups"
                    | "getEquipmentSlotInfo"
                    | "setPresenceInvisibility"
            ) {
                let mut rv = s.start_session(&session, &profile)?;
                if let Some(fields) = patch.as_object() {
                    rv.as_object_mut()
                        .expect("session envelope")
                        .extend(fields.clone())
                }
                rv
            } else {
                patch
            }
        }
    };
    if rv.get("version").is_some() {
        let mut versions = s.versions.lock().map_err(|_| Error::Internal)?;
        let current = now();
        if versions.len() >= 8192 {
            versions.retain(|_, (_, expiry)| *expiry > current);
        }
        let version = Uuid::new_v4();
        let previous = versions
            .insert(id, (version, current + 3600))
            .map(|p| p.0)
            .unwrap_or_default();
        rv["version"] = json!(version);
        rv["lastVersion"] = json!(previous);
    }
    Ok((s.tokens.sign(id, &session, Scope::Session)?, rv))
}

fn player_info(p: &PlayerProfile) -> Value {
    json!({"handle":p.name,"playerHandle":p.name,"level":1,"xp":0,
        "karmaLevel":0,"skillTier":0,"skillTierBronzeLine":0,"skillTierSilverLine":0,"skillTierGoldLine":0,"mmrBucket":0,
        "wins":0,"completed":0,"completed_non_tutorial":0,"canUseAllHeroes":true,
        "freeunlocks":[],"playerChests":[],"trophyCase":[],"pendingPartyInvites":false,"pendingMessages":false,
        "currency":{"gold":0,"silver":0,"essence":0,"opal":0,"epic_key":0,"seasonal_key":0},
        "entitlement_5v5_casual":true,"entitlement_coopbots":true,"tutorialState":0})
}

fn state(p: &PlayerProfile) -> Value {
    let l = p.lobby.as_ref();
    let (name, ttl) = match l.map(|l| l.phase.as_str()) {
        Some("queued" | "pending") => ("pending_auto", 10),
        Some("offered") => ("match_pending", 59),
        Some("playing") => ("playing", 100),
        Some("finished" | "post_match") => ("post_match", 100),
        _ => ("menus", 100),
    };
    let mut v = json!({"state":name,"ttl":ttl,"expiry":now()+ttl,"version":Uuid::new_v4(),
        "lastVersion":Uuid::nil(),"numPlayers":1,"notify":matches!(name,"pending_auto"|"match_pending")});
    if let Some(l) = l
        && name == "playing"
    {
        v["matchId"] = json!(l.match_id);
        if let Some((host, port)) = l.address.as_deref().and_then(|a| a.rsplit_once(':')) {
            v["host"] = json!(host);
            v["port"] = json!(port.parse::<u16>().ok());
        }
        v["site"] = json!("");
        v["timestamp"] = json!(chrono::Utc::now().format("%m/%d/%y %H:%M:%S").to_string());
    }
    v
}

impl Platform {
    fn start_session(&self, session: &str, p: &PlayerProfile) -> Result<Value, Error> {
        let mut rv = state(p);
        let grant = self.grants.issue(session)?;
        let http = format!("http://{}/lp/{grant}", self.config.notify_authority);
        let ws = format!("ws://{}/ws/{grant}", self.config.notify_authority);
        let extras = json!({"playerUUID":p.player_id,"playerInfo":player_info(p),"handle":p.name,
            "platformUrl":format!("https://{}",self.config.authority),"region":"na","country":"US",
            "platformTime":now(),"enableStateUpdates":false,"missedStateUpdateMessagesThreshold":-1,
            "notifyUrl":ws,"notifyFallbackUrl":http,"chatUrl":format!("ws://{}/chat/{grant}",self.config.notify_authority),"chatPass":grant,
            "pingHostPortInfo":[],"constants":{},"playerConfig":null,"pendingTeamInvites":false,
            "pendingGuildInvites":false,"pendingPartyInvites":false,"pendingMessages":false});
        rv.as_object_mut()
            .unwrap()
            .extend(extras.as_object().unwrap().clone());
        Ok(rv)
    }
}
