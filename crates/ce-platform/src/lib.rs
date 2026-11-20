//! Compatibility layer for the Android Community Edition platform protocol.
mod wire;
pub use halcyon_protocol::ce::{LobbyState, RpcMethod};
use wire::{Empty, PlayerHandle, Seconds, StateSnapshot, Success};
mod activity;
mod auth;
mod community;
mod content;
mod match_stats;
mod meta;
use meta::Preference;
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
    AppState, BotDifficulty, Guest, LobbyPhase, PlatformError, PlayerProfile, PublicMatches,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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
            talents: talents::Catalog::parse(load(
                "official_getTalentsData.json",
                serde_json::to_value(Empty {})?,
            )?)?,
            skins: load("official_getSkinManifest.json", Value::from("{}"))?,
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
            serde_json::to_value(GuestBootstrap {
                player_uuid: token,
                start_session_url: format!("https://{}", self.config.authority),
            })?,
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
) -> Json<RpcResponse> {
    let result = match serde_json::from_slice::<Request>(&body) {
        Ok(request) if request.method == method => {
            dispatch(&s, RpcMethod::from_wire(&method), &request.params).await
        }
        _ => Err(Error::Invalid),
    };
    tracing::debug!(method, success = result.is_ok(), "CE RPC");
    let value = match result {
        Ok((token, rv)) => RpcResponse {
            code: 0,
            session_token: token,
            return_value: rv,
            reason: None,
        },
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
            RpcResponse {
                code,
                session_token: String::new(),
                return_value: Value::Null,
                reason: Some(reason),
            }
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

async fn dispatch(s: &Platform, m: RpcMethod, params: &[Value]) -> Result<(String, Value), Error> {
    if m == RpcMethod::CreateGuestPlayer {
        if params.len() != 13 || arg(params, 0)? != "kindred" || arg(params, 8)? != "147219" {
            return Err(Error::Invalid);
        }
        return s.guest_response(s.app.create_guest("Guest".into()).await?, None);
    }
    if m == RpcMethod::GetPlayerFromPlatform {
        let (name, password) = local_credentials(params)?;
        return s.guest_response(s.app.login_account(name, password).await?, None);
    }
    if m == RpcMethod::Authenticate && params.first().is_some_and(Value::is_object) {
        let (name, password) = local_credentials(params)?;
        let guest = s.app.login_account(name, password).await?;
        let profile = s.app.profile(&guest.session_token).await?;
        s.community
            .observe_player(profile.player_id, profile.name.clone(), true)
            .await?;
        let token = s
            .tokens
            .sign(profile.player_id, &guest.session_token, Scope::Session)?;
        let mut snapshot = s.start_session(&guest.session_token, &profile).await?;
        s.stamp_state_version(profile.player_id, &mut snapshot)?;
        return Ok((token, snapshot));
    }
    let token = arg(params, 0)?;
    if m == RpcMethod::GetPlayerForGuestAccount {
        let claims = s.tokens.verify(token, Scope::Guest)?;
        let guest = s.app.resume_guest(claims.credential).await?;
        return s.guest_response(guest, Some(token));
    }
    let session = match s.tokens.verify(token, Scope::Session) {
        Ok(claims) => claims.credential,
        Err(_) if m == RpcMethod::StartSessionForPlayer || m == RpcMethod::Authenticate => {
            let resume = s.tokens.verify(token, Scope::Guest)?;
            s.app.resume_guest(resume.credential).await?.session_token
        }
        Err(err) => return Err(err),
    };
    let profile = s.app.profile(&session).await?;
    let id = profile.player_id;
    let mut rv = match m {
        RpcMethod::LinkAccount | RpcMethod::CreateAccountForPlayer => {
            let (name, password) = local_credentials(&params[1..])?;
            s.app.link_account(&session, name, password).await?;
            serde_json::to_value(AccountLinked {
                success: true,
                username: name,
                player_uuid: id,
            })?
        }
        RpcMethod::IsGovernmentIdVerified
        | RpcMethod::VerifyGovernmentId
        | RpcMethod::StorePrepareIAP
        | RpcMethod::StoreProcessIAP => {
            provider::dispatch(s.provider.as_ref(), &s.meta, id, m, &params[1..]).await?
        }
        RpcMethod::Authenticate | RpcMethod::StartSessionForPlayer => {
            s.community
                .observe_player(id, profile.name.clone(), true)
                .await?;
            s.meta
                .set(id, Preference::LastLogin, Value::from(now() as f64))
                .await?;
            let mut rv = s.start_session(&session, &profile).await?;
            rv["playerConfig"] = s.meta.prefs(id, Preference::PlatformConfig).await?;
            rv
        }
        RpcMethod::GetPlayerInfo => {
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
                        serde_json::to_value(PlayerHandle {
                            player_uuid: requested,
                            handle: other.profile.name,
                        })?,
                    ));
                }
            }
            player_info(&profile)
        }
        RpcMethod::GetSkinManifest => s.skins.clone(),
        RpcMethod::GetTalentsData => s.talents_for(id).await?,
        RpcMethod::QueryPendingMatch => s.pending_match(&session, &profile).await?,
        RpcMethod::Update => {
            if let Some(lobby) = &profile.lobby {
                if lobby.mode == "5v5_bots_solo" && lobby.phase_kind() == Some(LobbyPhase::Offered)
                {
                    let mut snapshot = s.projected_state(&session, &profile).await?;
                    s.accept(&session, lobby.match_id.ok_or(Error::Internal)?)
                        .await?;
                    // The local bot host is already allocated; CE polls again at expiry.
                    snapshot.ttl = Seconds::Whole(1);
                    snapshot.expiry = Seconds::Fraction(expiry_in(1));
                    serde_json::to_value(snapshot)?
                } else {
                    serde_json::to_value(s.projected_state(&session, &profile).await?)?
                }
            } else {
                serde_json::to_value(StateSnapshot::from_profile(&profile))?
            }
        }
        RpcMethod::JoinLobby => {
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
            let mut joined = Joined {
                success: true,
                ttl: 100,
                state: None,
                num_players: None,
            };
            if matches!(mode, "casual_aral" | "ranked") {
                let current = StateSnapshot::from_profile(&s.app.profile(&session).await?);
                if current.state == LobbyState::PendingAuto {
                    joined.state = Some(current.state);
                    joined.num_players = Some(s.meta.queued_party_size(id).await?);
                }
            }
            serde_json::to_value(joined)?
        }
        RpcMethod::AcceptMatch => {
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
                Value::Bool(true)
            } else {
                s.start_session(&session, &s.app.profile(&session).await?)
                    .await?
            }
        }
        RpcMethod::SpectateFriend => spectator::begin(s, &session, params).await?,
        RpcMethod::SpectatorExitMatch => spectator::exit(s, &session).await?,
        // The Android startup sequence calls exitLobby even after resuming a
        // live seat. Leaving the pre-match lobby must preserve that seat.
        RpcMethod::ExitLobby
            if profile
                .lobby
                .as_ref()
                .is_some_and(|l| l.phase_kind() == Some(LobbyPhase::Playing)) =>
        {
            serde_json::to_value(s.projected_state(&session, &profile).await?)?
        }
        RpcMethod::RejectMatch if params.len() > 1 => {
            let room = params[1].as_str().ok_or(Error::Invalid)?;
            if room.is_empty() {
                Value::Bool(false)
            } else {
                let room = Uuid::parse_str(room).map_err(|_| Error::Invalid)?;
                Value::Bool(if let Some(peers) = &s.peers {
                    peers.reject_offer(&session, room).await?
                } else {
                    s.app.reject_local_offer(&session, room).await?
                })
            }
        }
        RpcMethod::ExitLobby | RpcMethod::RejectMatch | RpcMethod::NotifyExitPostMatch => {
            if spectator::is_spectating(&profile) || profile.lobby.is_none() {
                spectator::leave(s, &session).await?;
            } else if profile.lobby.is_some() {
                s.cancel_queue(&session).await?;
            }
            if m == RpcMethod::NotifyExitPostMatch {
                serde_json::to_value(Success { success: true })?
            } else {
                serde_json::to_value(StateSnapshot::from_profile(&s.app.profile(&session).await?))?
            }
        }
        RpcMethod::RenamePlayerHandle | RpcMethod::SetPlayerHandle => {
            let native = m == RpcMethod::RenamePlayerHandle
                && params.len() == 3
                && params[1].as_str() == Some("");
            let name = arg(params, if native { 2 } else { 1 })?;
            s.app.rename(&session, name.to_owned()).await?;
            let updated = s.app.profile(&session).await?;
            s.community
                .observe_player(id, updated.name.clone(), true)
                .await?;
            if native {
                serde_json::to_value(RenamedPlayer {
                    handle: updated.name,
                    status: 0,
                    success: true,
                })?
            } else {
                player_info(&updated)
            }
        }
        RpcMethod::UpdatePlatformPlayerConfig => {
            let field = params.get(1).ok_or(Error::Invalid)?;
            let v = if let Some(raw) = field.as_str() {
                serde_json::from_str(raw).map_err(|_| Error::Invalid)?
            } else {
                field.clone()
            };
            if !v.is_object() {
                return Err(Error::Invalid);
            }
            s.meta.set(id, Preference::PlatformConfig, v).await?;
            serde_json::to_value(Empty {})?
        }
        RpcMethod::SetTutorialState => {
            let mut state = profile::tutorial_state(params.get(1).ok_or(Error::Invalid)?)
                .ok_or(Error::Invalid)?;
            if let Some(mode) = params.get(2) {
                state.client_mode = mode.as_i64().ok_or(Error::Invalid)?;
            }
            s.meta
                .set(id, Preference::Tutorial, serde_json::to_value(state)?)
                .await?;
            player_info(&profile)
        }
        RpcMethod::RecordMatchExperienceMetrics => {
            let room = Uuid::parse_str(arg(params, 1)?).map_err(|_| Error::Invalid)?;
            s.meta.metrics(id, room, params[2..].to_vec()).await?;
            serde_json::to_value(Empty {})?
        }
        RpcMethod::NotifyPlayerAction => {
            s.meta
                .set(
                    id,
                    Preference::RecentAction,
                    Value::Array(params[1..].to_vec()),
                )
                .await?;
            if params.len() == 3 && params[1..].iter().all(Value::is_string) {
                serde_json::to_value(Empty {})?
            } else {
                player_info(&profile)
            }
        }
        m if activity::handles(m) => s.activity_rpc(&profile, m, params).await?,
        m if content::Content::handles(m) => s.content_rpc(&profile, m, params).await?,
        RpcMethod::GetLeaderboardData => s.meta.ranking().await?,
        RpcMethod::EndSession => {
            let mut snapshot = s.projected_state(&session, &profile).await?;
            spectator::leave(s, &session).await?;
            s.app.end_session(&session).await?;
            s.community
                .observe_player(id, profile.name.clone(), false)
                .await?;
            if snapshot.state == LobbyState::Menus {
                snapshot.ttl = Seconds::Fraction(5.0);
            }
            let mut snapshot = serde_json::to_value(snapshot)?;
            s.stamp_state_version(id, &mut snapshot)?;
            return Ok((String::new(), snapshot));
        }
        _ => {
            let patch =
                community::dispatch(&s.community, &profile, m, params, Some(&s.app)).await?;
            if matches!(
                m,
                RpcMethod::PartyLobbyEnter
                    | RpcMethod::PartyLobbyExit
                    | RpcMethod::QuintPartyLobbyEnter
                    | RpcMethod::GetInventoryGroups
                    | RpcMethod::GetEquipmentSlotInfo
                    | RpcMethod::SetPresenceInvisibility
            ) {
                let mut rv = s.start_session(&session, &profile).await?;
                if let Some(fields) = patch.as_object() {
                    rv.as_object_mut()
                        .expect("session envelope")
                        .extend(fields.clone())
                }
                rv
            } else if m == RpcMethod::FriendRequestIssueByHandle
                && params.len() == 2
                && params[1].is_string()
            {
                serde_json::to_value(Empty {})?
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
        s.stamp_state_version(id, &mut rv)?;
    }
    Ok((s.tokens.sign(id, &session, Scope::Session)?, rv))
}

impl Platform {
    /// Link consecutive RPC snapshots for this local player. The first
    /// predecessor is a fresh local version when no prior response is known.
    fn stamp_state_version(&self, id: Uuid, state: &mut Value) -> Result<(), Error> {
        let mut versions = self.versions.lock().map_err(|_| Error::Internal)?;
        let current = now();
        if versions.len() >= 8192 {
            versions.retain(|_, (_, expiry)| *expiry > current);
        }
        let version = Uuid::new_v4();
        let previous = versions
            .insert(id, (version, current + 3600))
            .map(|p| p.0)
            .unwrap_or_else(Uuid::new_v4);
        state["version"] = serde_json::to_value(version)?;
        state["lastVersion"] = serde_json::to_value(previous)?;
        Ok(())
    }

    async fn start_session(&self, session: &str, p: &PlayerProfile) -> Result<Value, Error> {
        let state = self.projected_state(session, p).await?;
        let grant = self.grants.issue(session)?;
        let origin = &self.config.notify_authority;
        let json_chat = format!("ws://{origin}/chat/{grant}");
        let xmpp = self.xmpp.fields(session, p)?;
        let in_game_chat = xmpp.is_some();
        let chat = xmpp.unwrap_or_else(|| xmpp::ChatFields {
            chat_url: json_chat.clone(),
            chat_pass: grant.to_string(),
            chat_user: p.player_id,
        });
        Ok(serde_json::to_value(SessionBootstrap {
            state,
            player_uuid: p.player_id,
            player_info: player_info(p),
            handle: &p.name,
            platform_url: format!("https://{}", self.config.authority),
            region: "na",
            country: "US",
            platform_time: now(),
            enable_state_updates: self.native_state_updates,
            missed_state_update_messages_threshold: -1,
            notify_url: format!("ws://{origin}/ws/{grant}"),
            notify_fallback_url: format!("http://{origin}/lp/{grant}"),
            chat,
            json_chat_url: in_game_chat.then_some(json_chat),
            ping_post_to_analytics: false,
            ping_host_port_info: self.sites.announcement(),
            unity_ads_game_id: "",
            ironsource_app_id: None,
            experiment_details: [],
            ask_registration_consent: false,
            bucket_incrementer: 3000,
            failover_threshold: 4000,
            support_urls: SupportUrls {
                static_content_url: "",
            },
            constants: SessionConstants {
                features_enabled: in_game_chat.then_some(FeatureFlags { in_game_chat: true }),
            },
            player_config: None,
            pending_team_invites: 0,
            pending_guild_invites: 0,
            pending_party_invites: 0,
            pending_messages: 0,
        })?)
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Joined {
    success: bool,
    ttl: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<LobbyState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_players: Option<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GuestBootstrap {
    #[serde(rename = "playerUUID")]
    player_uuid: String,
    start_session_url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RpcResponse {
    code: i32,
    session_token: String,
    return_value: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
}

#[derive(Serialize)]
struct AccountLinked<'a> {
    success: bool,
    username: &'a str,
    #[serde(rename = "playerUUID")]
    player_uuid: Uuid,
}

#[derive(Serialize)]
struct RenamedPlayer {
    handle: String,
    status: u8,
    success: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionBootstrap<'a> {
    #[serde(flatten)]
    state: StateSnapshot,
    #[serde(rename = "playerUUID")]
    player_uuid: Uuid,
    player_info: Value,
    handle: &'a str,
    platform_url: String,
    region: &'static str,
    country: &'static str,
    platform_time: u64,
    enable_state_updates: bool,
    missed_state_update_messages_threshold: i8,
    notify_url: String,
    notify_fallback_url: String,
    #[serde(flatten)]
    chat: xmpp::ChatFields,
    #[serde(skip_serializing_if = "Option::is_none")]
    json_chat_url: Option<String>,
    ping_post_to_analytics: bool,
    ping_host_port_info: Vec<ping::SiteAnnouncement<'a>>,
    unity_ads_game_id: &'static str,
    ironsource_app_id: Option<String>,
    experiment_details: [(); 0],
    ask_registration_consent: bool,
    bucket_incrementer: u32,
    failover_threshold: u32,
    support_urls: SupportUrls,
    constants: SessionConstants,
    player_config: Option<Value>,
    pending_team_invites: u32,
    pending_guild_invites: u32,
    pending_party_invites: u32,
    pending_messages: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SupportUrls {
    static_content_url: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionConstants {
    #[serde(skip_serializing_if = "Option::is_none")]
    features_enabled: Option<FeatureFlags>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FeatureFlags {
    in_game_chat: bool,
}
