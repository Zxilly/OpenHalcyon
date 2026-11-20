//! Compatibility layer for the Android Community Edition platform protocol.
mod wire;
use halcyon_protocol::ce::KnownQueue;
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
use activity::PendingCounts;
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
    game_data: Option<Arc<halcyon_data::GameData>>,
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
    archived_manifests: bool,
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
            game_data: None,
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
            archived_manifests: manifests.is_some(),
        }))
    }

    /// Attach the same immutable pack used by the match host before routing requests.
    pub fn configure_game_data(
        platform: &mut Arc<Self>,
        data: Option<Arc<halcyon_data::GameData>>,
    ) -> Result<(), Error> {
        let this = Arc::get_mut(platform).ok_or(Error::Config(
            "configure content before sharing the platform",
        ))?;
        if let Some(data) = data.as_deref() {
            this.talents.add_supplement(data);
            if !this.archived_manifests
                && let Some(catalog) = data.cosmetics()
            {
                #[derive(Serialize)]
                struct SkinManifest<'a> {
                    skins: &'a [halcyon_data::skins::OfficialSkin],
                    themes: &'a [halcyon_data::skins::Theme],
                }
                this.skins = Value::String(serde_json::to_string(&SkinManifest {
                    skins: &catalog.official_skins,
                    themes: &catalog.official_themes,
                })?);
            }
        }
        this.game_data = data;
        Ok(())
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
        let bootstrap = s
            .session_bootstrap(
                &guest.session_token,
                &profile,
                SessionProjection::AccountLogin,
                None,
            )
            .await?;
        return Ok((token, bootstrap));
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
    let rv = match m {
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
            s.session_bootstrap(&session, &profile, SessionProjection::Resume, None)
                .await?
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
            s.project_player_info(&profile).await?
        }
        RpcMethod::GetSkinManifest => s.skins.clone(),
        RpcMethod::GetTalentsData => s.talents_for(id).await?,
        RpcMethod::QueryPendingMatch => s.pending_match(&session, &profile).await?,
        RpcMethod::Update => {
            if let Some(lobby) = &profile.lobby {
                if KnownQueue::parse(&lobby.mode) == Some(KnownQueue::SoloBots)
                    && lobby.phase_kind() == Some(LobbyPhase::Offered)
                {
                    let mut snapshot = s.projected_state(&session, &profile).await?;
                    s.accept(&session, lobby.match_id.ok_or(Error::Internal)?)
                        .await?;
                    // The local bot host is already allocated; CE polls again at expiry.
                    snapshot.ttl = Seconds::Whole(1);
                    snapshot.expiry = Seconds::Fraction(expiry_in(1));
                    s.state_response(id, snapshot)?
                } else {
                    s.state_response(id, s.projected_state(&session, &profile).await?)?
                }
            } else {
                s.state_response(id, StateSnapshot::from_profile(&profile))?
            }
        }
        RpcMethod::JoinLobby => {
            let lobby = params.get(1).ok_or(Error::Invalid)?;
            let parsed;
            let payload = if let Some(raw) = lobby.as_str() {
                parsed = serde_json::from_str::<Value>(raw).map_err(|_| Error::Invalid)?;
                &parsed
            } else {
                lobby
            };
            let request = JoinLobbyRequest::deserialize(payload).map_err(|_| Error::Invalid)?;
            let mode = request.lobby.as_str();
            let difficulty = match request.difficulty {
                Some(value) if BotDifficulty::supports_queue(mode) => Some(
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
                s.sites
                    .report(request.sites.as_ref().unwrap_or(&Value::Null)),
            )
            .await?;
            let mut joined = Joined {
                success: true,
                ttl: 100,
                state: None,
                num_players: None,
            };
            if matches!(
                KnownQueue::parse(mode),
                Some(KnownQueue::CasualAral | KnownQueue::Ranked)
            ) {
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
            s.state_response(id, s.projected_state(&session, &profile).await?)?
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
        RpcMethod::NotifyExitPostMatch => {
            s.app.acknowledge_post_match(&session).await?;
            serde_json::to_value(Success { success: true })?
        }
        RpcMethod::ExitLobby | RpcMethod::RejectMatch => {
            if spectator::is_spectating(&profile) || profile.lobby.is_none() {
                spectator::leave(s, &session).await?;
            } else if profile.lobby.is_some() {
                s.cancel_queue(&session).await?;
            }
            s.state_response(
                id,
                StateSnapshot::from_profile(&s.app.profile(&session).await?),
            )?
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
                s.project_player_info(&updated).await?
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
            s.project_player_info(&profile).await?
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
                s.project_player_info(&profile).await?
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
            return Ok((String::new(), s.state_response(id, snapshot)?));
        }
        _ => {
            let reply =
                community::dispatch(&s.community, &profile, m, params, Some(&s.app)).await?;
            let (patch, is_profile) = match reply {
                community::Reply::Standalone(data) => (data, false),
                community::Reply::PlayerInfo(data) => (data, true),
            };
            if matches!(
                m,
                RpcMethod::PartyLobbyEnter
                    | RpcMethod::PartyLobbyExit
                    | RpcMethod::QuintPartyLobbyEnter
                    | RpcMethod::GetInventoryGroups
                    | RpcMethod::GetEquipmentSlotInfo
                    | RpcMethod::SetPresenceInvisibility
            ) {
                s.session_bootstrap(
                    &session,
                    &profile,
                    SessionProjection::Current,
                    patch.as_object().map(|_| &patch),
                )
                .await?
            } else if m == RpcMethod::FriendRequestIssueByHandle
                && params.len() == 2
                && params[1].is_string()
            {
                serde_json::to_value(Empty {})?
            } else if is_profile {
                let mut info = s.project_player_info(&profile).await?;
                info.as_object_mut()
                    .ok_or(Error::Internal)?
                    .extend(patch.as_object().ok_or(Error::Internal)?.clone());
                info
            } else {
                patch
            }
        }
    };
    Ok((s.tokens.sign(id, &session, Scope::Session)?, rv))
}

impl Platform {
    /// Link consecutive RPC snapshots for this local player. The first
    /// predecessor is a fresh local version when no prior response is known.
    fn stamp_state_version(&self, id: Uuid, state: &mut StateSnapshot) -> Result<(), Error> {
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
        state.version = version;
        state.last_version = previous;
        Ok(())
    }

    fn state_response(&self, id: Uuid, mut state: StateSnapshot) -> Result<Value, Error> {
        self.stamp_state_version(id, &mut state)?;
        Ok(serde_json::to_value(state)?)
    }

    pub(crate) async fn project_player_info(
        &self,
        profile: &PlayerProfile,
    ) -> Result<Value, Error> {
        let fields = self.meta.player_fields(profile.player_id).await?;
        Ok(profile::player_info(profile, fields))
    }

    async fn start_session(&self, session: &str, p: &PlayerProfile) -> Result<Value, Error> {
        self.session_bootstrap(session, p, SessionProjection::Current, None)
            .await
    }

    async fn session_bootstrap(
        &self,
        session: &str,
        p: &PlayerProfile,
        projection: SessionProjection,
        extras: Option<&Value>,
    ) -> Result<Value, Error> {
        let mut state = self.projected_state(session, p).await?;
        self.stamp_state_version(p.player_id, &mut state)?;
        let fields = if projection == SessionProjection::AccountLogin {
            activity::PlayerActivityFields::default()
        } else {
            self.meta.player_fields(p.player_id).await?
        };
        let pending = fields.pending.with_defaults();
        let player_config = if projection == SessionProjection::Resume {
            Some(
                self.meta
                    .prefs(p.player_id, Preference::PlatformConfig)
                    .await?,
            )
        } else {
            None
        };
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
        let reference = self
            .game_data
            .as_deref()
            .and_then(halcyon_data::GameData::references);
        let constants = reference.map(|r| &r.constants);
        let flags = constants.map(|c| &c.official_session_constants.features_enabled);
        Ok(serde_json::to_value(SessionBootstrap {
            state,
            player_uuid: p.player_id,
            player_info: profile::player_info(p, fields),
            handle: &p.name,
            platform_url: format!("https://{}", self.config.authority),
            region: "na",
            country: "US",
            platform_time: now(),
            enable_state_updates: self.native_state_updates,
            missed_state_update_messages_threshold: constants.map_or(-1, |c| {
                c.official_session_meta
                    .missed_state_update_messages_threshold
            }),
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
            failover_threshold: constants
                .map_or(4000, |c| c.official_session_meta.failover_threshold),
            support_urls: SupportUrls {
                static_content_url: "",
            },
            constants: SessionConstants {
                features_enabled: (in_game_chat || flags.is_some()).then_some(FeatureFlags {
                    in_game_chat,
                    leaderboards: flags.map(|f| f.leaderboards),
                    leaderboards_always_query_offline_friends: flags
                        .map(|f| f.leaderboards_always_query_offline_friends),
                    live_events: flags.map(|f| f.live_events),
                }),
                experiments: constants.map(|c| &c.official_session_constants.experiments),
            },
            player_config,
            extras,
            pending,
        })?)
    }
}

#[derive(Deserialize)]
struct JoinLobbyRequest {
    lobby: String,
    difficulty: Option<Value>,
    sites: Option<Value>,
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
    missed_state_update_messages_threshold: i32,
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
    constants: SessionConstants<'a>,
    player_config: Option<Value>,
    #[serde(flatten)]
    extras: Option<&'a Value>,
    #[serde(flatten)]
    pending: PendingCounts,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SupportUrls {
    static_content_url: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionConstants<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    features_enabled: Option<FeatureFlags>,
    #[serde(flatten)]
    experiments:
        Option<&'a std::collections::BTreeMap<String, std::collections::BTreeMap<String, Value>>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FeatureFlags {
    in_game_chat: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    leaderboards: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    leaderboards_always_query_offline_friends: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    live_events: Option<bool>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SessionProjection {
    AccountLogin,
    Resume,
    Current,
}
