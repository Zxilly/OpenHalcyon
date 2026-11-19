//! Compatibility layer for the Android Community Edition platform protocol.
mod activity;
mod auth;
mod community;
mod content;
mod match_stats;
mod meta;
mod native_offer;
mod notify;
mod party_notify;
mod ping;
pub use ping::PingSite;
mod profile;
use profile::player_info;
mod provider;
pub use provider::{ProviderConfig, ProviderProduct};
mod public_api;
mod spectator;
mod talents;
pub use talents::TalentProgress;
mod xmpp;
use auth::{Scope, Tokens};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    routing::{get, post},
};
use halcyon_platform::{
    AppState, BotDifficulty, Guest, PlatformError, PlayerProfile, PublicMatches,
};
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
    #[error("{0}")]
    Unavailable(&'static str),
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
    native_state_updates: bool,
    xmpp: xmpp::Chat,
    sites: ping::Sites,
    versions: Mutex<HashMap<Uuid, (Uuid, u64)>>,
    meta: meta::Meta,
    provider: Option<provider::Verifier>,
    community: halcyon_community::Community,
    content: content::Content,
    peers: Option<halcyon_peer_play::PeerPlay>,
    talents: talents::Catalog,
    skins: Value,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn expiry_in(ttl: u64) -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
        + ttl as f64
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
            native_state_updates: false,
            xmpp: Default::default(),
            sites: Default::default(),
            versions: Default::default(),
            community: halcyon_community::Community::open(database)?,
            meta: meta::Meta::open(database)?,
            provider: None,
            content: Default::default(),
            peers: None,
            talents: talents::Catalog::parse(load("official_getTalentsData.json", json!({}))?)?,
            skins: load("official_getSkinManifest.json", json!("{}"))?,
        }))
    }

    /// Opt in to the native client's state subscription.
    pub fn configure_native_state_updates(
        platform: &mut Arc<Self>,
        enabled: bool,
    ) -> Result<(), Error> {
        Arc::get_mut(platform)
            .ok_or(Error::Config(
                "configure state updates before sharing the platform",
            ))?
            .native_state_updates = enabled;
        Ok(())
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
        self.accept_offer(session, id).await?;
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
                Error::Unavailable(reason) => (-6, *reason),
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

fn local_credentials(params: &[Value]) -> Result<(&str, String), Error> {
    if let Some(obj) = params.first().and_then(Value::as_object) {
        let name = obj
            .get("username")
            .and_then(Value::as_str)
            .ok_or(Error::Invalid)?;
        let password = obj
            .get("password")
            .and_then(Value::as_str)
            .ok_or(Error::Invalid)?;
        Ok((name, password.to_owned()))
    } else {
        Ok((arg(params, 0)?, arg(params, 1)?.to_owned()))
    }
}

async fn dispatch(s: &Platform, m: &str, params: &[Value]) -> Result<(String, Value), Error> {
    if m == "createGuestPlayer" {
        if params.len() != 13 || arg(params, 0)? != "kindred" || arg(params, 8)? != "147219" {
            return Err(Error::Invalid);
        }
        return s.guest_response(s.app.create_guest("Guest".into()).await?, None);
    }
    if m == "getPlayerFromPlatform" {
        let (name, password) = local_credentials(params)?;
        return s.guest_response(s.app.login_account(name, password).await?, None);
    }
    if m == "authenticate" && params.first().is_some_and(Value::is_object) {
        let (name, password) = local_credentials(params)?;
        let guest = s.app.login_account(name, password).await?;
        let profile = s.app.profile(&guest.session_token).await?;
        s.community
            .observe_player(profile.player_id, profile.name.clone(), true)
            .await?;
        let token = s
            .tokens
            .sign(profile.player_id, &guest.session_token, Scope::Session)?;
        return Ok((
            token,
            s.start_session(&guest.session_token, &profile).await?,
        ));
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
        "link_account" | "createAccountForPlayer" => {
            let (name, password) = local_credentials(&params[1..])?;
            s.app.link_account(&session, name, password).await?;
            json!({"success":true,"username":name,"playerUUID":id})
        }
        "isGovernmentIdVerified" | "verifyGovernmentId" | "storePrepareIAP" | "storeProcessIAP" => {
            provider::dispatch(s.provider.as_ref(), &s.meta, id, m, &params[1..]).await?
        }
        "authenticate" | "startSessionForPlayer" => {
            s.community
                .observe_player(id, profile.name.clone(), true)
                .await?;
            s.meta.set(id, "last_login", json!(now() as f64)).await?;
            let mut rv = s.start_session(&session, &profile).await?;
            rv["playerConfig"] = s.meta.prefs(id, "platform_config").await?;
            rv
        }
        "getPlayerInfo" => {
            if let Some(target) = params.get(1) {
                let requested = Uuid::parse_str(target.as_str().ok_or(Error::Invalid)?)
                    .map_err(|_| Error::Invalid)?;
                if requested != id {
                    let other = s
                        .app
                        .player_activity(requested)
                        .await?
                        .ok_or(halcyon_community::CommunityError::Missing("unknown player"))?;
                    return Ok((
                        s.tokens.sign(id, &session, Scope::Session)?,
                        json!({"playerUUID":requested,"handle":other.profile.name}),
                    ));
                }
            }
            player_info(&profile)
        }
        "getSkinManifest" => s.skins.clone(),
        "getTalentsData" => s.talents_for(id).await?,
        "queryPendingMatch" => s.pending_match(&session, &profile).await?,
        "update" => {
            if let Some(lobby) = &profile.lobby {
                if lobby.mode == "5v5_bots_solo" && lobby.phase == "offered" {
                    let mut snapshot = s.projected_state(&session, &profile).await?;
                    s.accept(&session, lobby.match_id.ok_or(Error::Internal)?)
                        .await?;
                    // The local bot host is already allocated; CE polls again at expiry.
                    snapshot["ttl"] = json!(1);
                    snapshot["expiry"] = json!(expiry_in(1));
                    snapshot
                } else {
                    s.projected_state(&session, &profile).await?
                }
            } else {
                state(&profile)
            }
        }
        "joinLobby" => {
            let lobby = params.get(1).ok_or(Error::Invalid)?;
            let parsed;
            let obj = if let Some(raw) = lobby.as_str() {
                parsed = serde_json::from_str::<Value>(raw).map_err(|_| Error::Invalid)?;
                &parsed
            } else {
                lobby
            };
            let mode = obj
                .get("lobby")
                .and_then(Value::as_str)
                .ok_or(Error::Invalid)?;
            let difficulty = match obj.get("difficulty") {
                Some(value) if !value.is_null() && BotDifficulty::supports_queue(mode) => Some(
                    value
                        .as_str()
                        .and_then(BotDifficulty::from_wire)
                        .ok_or(Error::Invalid)?,
                ),
                _ => None,
            };
            s.enqueue_with_estimates(
                &session,
                mode.to_owned(),
                difficulty,
                s.sites.report(&obj["sites"]),
            )
            .await?;
            let mut joined = json!({"success":true,"ttl":100});
            if matches!(mode, "casual_aral" | "ranked") {
                let current = state(&s.app.profile(&session).await?);
                if current["state"] == "pending_auto" {
                    joined["state"] = current["state"].clone();
                    joined["numPlayers"] = json!(s.meta.queued_party_size(id).await?);
                }
            }
            joined
        }
        "acceptMatch" => {
            let requested = params
                .get(1)
                .map(|v| v.as_str().ok_or(Error::Invalid))
                .transpose()?;
            let room = match requested {
                Some(raw) if !raw.is_empty() => Uuid::parse_str(raw).map_err(|_| Error::Invalid)?,
                _ => profile
                    .lobby
                    .as_ref()
                    .and_then(|l| l.match_id)
                    .ok_or(Error::Invalid)?,
            };
            s.accept(&session, room).await?;
            if requested.is_some() {
                json!(true)
            } else {
                s.start_session(&session, &s.app.profile(&session).await?)
                    .await?
            }
        }
        "spectateFriend" => spectator::begin(s, &session, params).await?,
        "spectatorExitMatch" => spectator::exit(s, &session).await?,
        // The Android startup sequence calls exitLobby even after resuming a
        // live seat. Leaving the pre-match lobby must preserve that seat.
        "exitLobby" if profile.lobby.as_ref().is_some_and(|l| l.phase == "playing") => {
            s.projected_state(&session, &profile).await?
        }
        "rejectMatch" if params.len() > 1 => {
            let room = params[1].as_str().ok_or(Error::Invalid)?;
            if room.is_empty() {
                json!(false)
            } else {
                let room = Uuid::parse_str(room).map_err(|_| Error::Invalid)?;
                json!(if let Some(peers) = &s.peers {
                    peers.reject_offer(&session, room).await?
                } else {
                    s.app.reject_local_offer(&session, room).await?
                })
            }
        }
        "exitLobby" | "rejectMatch" | "notifyExitPostMatch" => {
            if spectator::is_spectating(&profile) || profile.lobby.is_none() {
                spectator::leave(s, &session).await?;
            } else if profile.lobby.is_some() {
                s.cancel_queue(&session).await?;
            }
            if m == "notifyExitPostMatch" {
                json!({"success":true})
            } else {
                state(&s.app.profile(&session).await?)
            }
        }
        "renamePlayerHandle" | "setPlayerHandle" => {
            let native =
                m == "renamePlayerHandle" && params.len() == 3 && params[1].as_str() == Some("");
            let name = arg(params, if native { 2 } else { 1 })?;
            s.app.rename(&session, name.to_owned()).await?;
            let updated = s.app.profile(&session).await?;
            s.community
                .observe_player(id, updated.name.clone(), true)
                .await?;
            if native {
                json!({"handle":updated.name,"status":0,"success":true})
            } else {
                player_info(&updated)
            }
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
            let mut state = profile::tutorial_state(params.get(1).ok_or(Error::Invalid)?)
                .ok_or(Error::Invalid)?;
            if let Some(mode) = params.get(2) {
                state["clientMode"] = json!(mode.as_i64().ok_or(Error::Invalid)?);
            }
            s.meta.set(id, "tutorial", state).await?;
            player_info(&profile)
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
            if params.len() == 3 && params[1..].iter().all(Value::is_string) {
                json!({})
            } else {
                player_info(&profile)
            }
        }
        m if activity::handles(m) => s.activity_rpc(&profile, m, params).await?,
        m if content::Content::handles(m) => s.content_rpc(&profile, m, params).await?,
        "getLeaderboardData" => s.meta.ranking().await?,
        "endSession" => {
            let snapshot = s.projected_state(&session, &profile).await?;
            spectator::leave(s, &session).await?;
            s.app.end_session(&session).await?;
            s.community
                .observe_player(id, profile.name.clone(), false)
                .await?;
            return Ok((String::new(), snapshot));
        }
        _ => {
            let patch =
                community::dispatch(&s.community, &profile, m, params, Some(&s.app)).await?;
            if matches!(
                m,
                "partyLobbyEnter"
                    | "partyLobbyExit"
                    | "quintPartyLobbyEnter"
                    | "getInventoryGroups"
                    | "getEquipmentSlotInfo"
                    | "setPresenceInvisibility"
            ) {
                let mut rv = s.start_session(&session, &profile).await?;
                if let Some(fields) = patch.as_object() {
                    rv.as_object_mut()
                        .expect("session envelope")
                        .extend(fields.clone())
                }
                rv
            } else if m == "friendRequestIssueByHandle"
                && params.len() == 2
                && params[1].is_string()
            {
                json!({})
            } else {
                patch
            }
        }
    };
    if rv.get("playerInfo").is_some() || rv.get("completed").is_some() {
        let fields = s.meta.player_fields(id).await?;
        if rv.get("playerInfo").is_some() {
            for name in [
                "pendingMessages",
                "pendingPartyInvites",
                "pendingGuildInvites",
                "pendingTeamInvites",
            ] {
                if let Some(flag) = fields.get(name) {
                    rv[name] = flag.clone();
                }
            }
            rv["playerInfo"]
                .as_object_mut()
                .ok_or(Error::Internal)?
                .extend(fields.as_object().unwrap().clone());
        } else {
            rv.as_object_mut()
                .ok_or(Error::Internal)?
                .extend(fields.as_object().unwrap().clone());
        }
        let info = if rv.get("playerInfo").is_some() {
            &mut rv["playerInfo"]
        } else {
            &mut rv
        };
        if info.get("targeting").is_some() {
            info["targeting"]["currency"] = fields["currency"].clone();
        }
    }
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

fn state(p: &PlayerProfile) -> Value {
    let l = p.lobby.as_ref();
    let (name, ttl) = match l.map(|l| l.phase.as_str()) {
        Some("queued" | "pending") => ("pending_auto", 10),
        Some("offered" | "reserving") => ("match_pending", 59),
        Some("playing") => ("playing", 100),
        Some("spectating") => ("spectating", 100),
        Some("finished" | "post_match") => ("post_match", 100),
        _ => ("menus", 100),
    };
    let mut v = json!({"state":name,"ttl":ttl,"expiry":expiry_in(ttl),"version":Uuid::new_v4(),
        "lastVersion":Uuid::nil(),"numPlayers":1,"notify":matches!(name,"pending_auto"|"match_pending")});
    if let Some(l) = l
        && matches!(name, "playing" | "spectating")
    {
        v["matchId"] = json!(l.match_id);
        if let Some(credential) = l.spectator_id {
            v["spectatorId"] = json!(credential);
        }
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
    async fn start_session(&self, session: &str, p: &PlayerProfile) -> Result<Value, Error> {
        let mut rv = self.projected_state(session, p).await?;
        let grant = self.grants.issue(session)?;
        let http = format!("http://{}/lp/{grant}", self.config.notify_authority);
        let ws = format!("ws://{}/ws/{grant}", self.config.notify_authority);
        let extras = json!({"playerUUID":p.player_id,"playerInfo":player_info(p),"handle":p.name,
            "platformUrl":format!("https://{}",self.config.authority),"region":"na","country":"US",
            "platformTime":now(),"enableStateUpdates":self.native_state_updates,"missedStateUpdateMessagesThreshold":-1,
            "notifyUrl":ws,"notifyFallbackUrl":http,"chatUrl":format!("ws://{}/chat/{grant}",self.config.notify_authority),"chatPass":grant,
            "chatUser":p.player_id,"pingPostToAnalytics":false,"pingHostPortInfo":self.sites.announcement(),
            "unityAdsGameId":"","ironsourceAppId":null,"experimentDetails":[],
            "askRegistrationConsent":false,"bucketIncrementer":3000,"failoverThreshold":4000,
            "supportUrls":{"staticContentUrl":""},"constants":{},"playerConfig":null,"pendingTeamInvites":0,
            "pendingGuildInvites":0,"pendingPartyInvites":0,"pendingMessages":0});
        rv.as_object_mut()
            .unwrap()
            .extend(extras.as_object().unwrap().clone());
        if let Some(fields) = self.xmpp.fields(session, p)? {
            rv["jsonChatUrl"] = rv["chatUrl"].clone();
            rv["constants"]["featuresEnabled"]["inGameChat"] = json!(true);
            rv.as_object_mut()
                .expect("session response is an object")
                .extend(
                    fields
                        .as_object()
                        .expect("chat fields are an object")
                        .clone(),
                );
        }
        Ok(rv)
    }
}
