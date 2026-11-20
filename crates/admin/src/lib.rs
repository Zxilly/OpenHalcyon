mod sessions;

use std::sync::Arc;

use axum::{
    Extension, Json, Router,
    extract::{
        Path, Query, Request, State,
        rejection::{JsonRejection, PathRejection, QueryRejection},
    },
    http::{HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, put},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use halcyon_game::{Rooms, TerminateError};
use halcyon_platform::{AppState, PlatformError, PlayerActivity};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sha2::Sha256;
use uuid::Uuid;

use sessions::{Session, Sessions};

const PREFIX: &str = "/api/admin/v1";
pub type PoolStats = Arc<dyn Fn() -> Vec<Value> + Send + Sync>;

pub struct AdminConfig {
    pub origin: String,
    pub username: String,
    pub password_hash: String,
    pub instance_id: Uuid,
    pub started_at: DateTime<Utc>,
    pub client_build: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum AdminConfigError {
    #[error("admin origin must be a canonical HTTPS origin")]
    Origin,
    #[error("invalid admin username")]
    Username,
    #[error("admin password hash must be an Argon2id PHC string")]
    PasswordHash,
}

#[derive(Clone)]
struct Admin {
    config: Arc<AdminConfig>,
    platform: AppState,
    rooms: Rooms,
    pools: PoolStats,
    sessions: Arc<Sessions>,
    cursor_key: [u8; 32],
}

pub fn router(
    config: AdminConfig,
    platform: AppState,
    rooms: Rooms,
    pools: PoolStats,
) -> std::result::Result<Router, AdminConfigError> {
    validate_origin(&config.origin)?;
    sessions::validate_credentials(&config)?;
    let mut cursor_key = [0; 32];
    cursor_key[..16].copy_from_slice(Uuid::new_v4().as_bytes());
    cursor_key[16..].copy_from_slice(Uuid::new_v4().as_bytes());
    let state = Admin {
        config: Arc::new(config),
        platform,
        rooms,
        pools,
        sessions: Arc::new(Sessions::default()),
        cursor_key,
    };
    let api = Router::new()
        .route(
            "/session",
            get(sessions::current)
                .post(sessions::login)
                .delete(sessions::logout),
        )
        .route("/overview", get(overview))
        .route("/federation/pools", get(federation))
        .route("/players", get(players))
        .route("/players/{id}", get(player))
        .route("/matches", get(matches))
        .route("/matches/{id}", get(match_detail))
        .route("/matches/{id}/events", get(match_events))
        .route("/matches/{id}/termination", put(terminate))
        .fallback(|| async { Problem::not_found() });
    Ok(Router::new()
        .nest(PREFIX, api)
        .route("/admin", get(dashboard))
        .route(
            "/admin/assets/control.js",
            get(|| async {
                asset(
                    "application/javascript; charset=utf-8",
                    include_str!("control.js"),
                )
            }),
        )
        .route(
            "/admin/assets/control.css",
            get(|| async { asset("text/css; charset=utf-8", include_str!("control.css")) }),
        )
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state))
}

fn validate_origin(origin: &str) -> std::result::Result<(), AdminConfigError> {
    let url: axum::http::Uri = origin.parse().map_err(|_| AdminConfigError::Origin)?;
    let authority = url.authority().ok_or(AdminConfigError::Origin)?;
    let canonical = format!("https://{authority}");
    if origin != canonical || authority.as_str().contains('@') {
        return Err(AdminConfigError::Origin);
    }
    Ok(())
}

async fn guard(State(state): State<Admin>, mut req: Request, next: Next) -> Response {
    let is_api = req.uri().path().starts_with(PREFIX);
    let method = req.method();
    let writes = !matches!(method.as_str(), "GET" | "HEAD" | "OPTIONS");
    let unprotected = (req.uri().path() == "/api/admin/v1/session"
        || req.uri().path() == "/api/admin/v1/session/")
        && matches!(method.as_str(), "POST" | "DELETE");
    let mut response = if is_api && writes && !matching_origin(&req, &state.config.origin) {
        Problem::new(StatusCode::FORBIDDEN, "origin_rejected", "Origin rejected").into_response()
    } else if is_api && !unprotected {
        match state.sessions.lookup(req.headers()) {
            Some(session) => {
                req.extensions_mut().insert(session);
                next.run(req).await
            }
            None => Problem::unauthorized().into_response(),
        }
    } else {
        next.run(req).await
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

fn matching_origin(req: &Request, configured: &str) -> bool {
    let mut values = req.headers().get_all(header::ORIGIN).iter();
    values
        .next()
        .is_some_and(|value| value.as_bytes() == configured.as_bytes())
        && values.next().is_none()
}

async fn dashboard() -> Response {
    let mut response = Html(include_str!("dashboard.html")).into_response();
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'"),
    );
    response
}

fn asset(content_type: &'static str, content: &'static str) -> Response {
    ([(header::CONTENT_TYPE, content_type)], content).into_response()
}

struct Problem {
    status: StatusCode,
    code: &'static str,
    title: &'static str,
    retry: Option<u64>,
}

impl Problem {
    fn new(status: StatusCode, code: &'static str, title: &'static str) -> Self {
        Self {
            status,
            code,
            title,
            retry: None,
        }
    }

    fn invalid() -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Invalid request",
        )
    }

    fn unauthorized() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "Invalid credentials or session",
        )
    }

    fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", "Resource not found")
    }

    fn internal() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Internal error",
        )
    }
}

impl From<PlatformError> for Problem {
    fn from(error: PlatformError) -> Self {
        tracing::error!(%error, "admin platform query failed");
        Self::internal()
    }
}

impl IntoResponse for Problem {
    fn into_response(self) -> Response {
        let request_id = Uuid::new_v4();
        let mut response = (self.status, Json(json!({
            "title":self.title, "status":self.status.as_u16(), "code":self.code, "requestId":request_id,
        }))).into_response();
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
        if let Some(retry) = self.retry {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from(retry));
        }
        response
    }
}

type Result<T> = std::result::Result<T, Problem>;

async fn overview(State(state): State<Admin>) -> Result<Json<Value>> {
    let counts = state.platform.counts().await?;
    let (active_rooms, human_seats, bot_seats, reserved_seats) = state.rooms.hosting_counts();
    let capacity = state.rooms.capacity();
    let resources = state.rooms.resource_metrics();
    let admission = if capacity == 0 {
        "disabled"
    } else if state.rooms.available_rooms() == 0 {
        "full"
    } else {
        "accepting"
    };
    Ok(Json(json!({
        "sampledAt":Utc::now(), "startedAt":state.config.started_at,
        "serverVersion":env!("CARGO_PKG_VERSION"), "clientBuild":state.config.client_build,
        "onlinePlayers":counts.online_players, "queuedPlayers":counts.queued_players,
        "activeMatches":active_rooms,
        "hosting": {
            "instanceId": state.config.instance_id,
            "admission": admission, "humanSeats": human_seats, "botSeats": bot_seats,
            "reservedSeats": reserved_seats, "tickP99Ms": resources.tick_p99_ms, "cpuPercent": resources.process_cpu_percent,
            "rssBytes": resources.rss_bytes, "outboundBytesPerSecond": resources.egress_bytes_per_second,
        },
    })))
}

async fn federation(State(state): State<Admin>) -> Json<Value> {
    Json(json!({ "items": (state.pools)() }))
}

#[derive(Deserialize)]
struct PlayerQuery {
    cursor: Option<String>,
    limit: Option<usize>,
    online: Option<bool>,
}

#[derive(Serialize, Deserialize)]
struct PlayerCursor {
    online: Option<bool>,
    after: Uuid,
}

fn page_limit(limit: Option<usize>) -> Result<usize> {
    let limit = limit.unwrap_or(50);
    (1..=100)
        .contains(&limit)
        .then_some(limit)
        .ok_or_else(Problem::invalid)
}

impl Admin {
    fn encode_cursor(&self, resource: &str, value: &impl Serialize) -> Result<String> {
        let bytes = serde_json::to_vec(value).map_err(|_| Problem::internal())?;
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.cursor_key).expect("valid key length");
        mac.update(resource.as_bytes());
        mac.update(&bytes);
        Ok(format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(bytes),
            URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
        ))
    }

    fn decode_cursor<T: DeserializeOwned>(&self, resource: &str, cursor: &str) -> Result<T> {
        if cursor.len() > 512 {
            return Err(Problem::invalid());
        }
        let (payload, signature) = cursor.split_once('.').ok_or_else(Problem::invalid)?;
        let bytes = URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| Problem::invalid())?;
        let signature = URL_SAFE_NO_PAD
            .decode(signature)
            .map_err(|_| Problem::invalid())?;
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.cursor_key).expect("valid key length");
        mac.update(resource.as_bytes());
        mac.update(&bytes);
        mac.verify_slice(&signature)
            .map_err(|_| Problem::invalid())?;
        serde_json::from_slice(&bytes).map_err(|_| Problem::invalid())
    }

    fn player_view(&self, activity: PlayerActivity) -> Value {
        let profile = activity.profile;
        let lobby = profile.lobby.as_ref();
        let match_id = lobby.and_then(|lobby| lobby.match_id);
        let platform_state = if activity.online {
            match lobby.map(|lobby| lobby.phase.as_str()) {
                Some("pending" | "queued") => "pending_auto",
                Some("offered" | "reserving") => "match_pending",
                Some("playing") => "playing",
                Some("finished") => "post_match",
                _ => "menus",
            }
        } else {
            "offline"
        };
        let local_host = match_id
            .filter(|id| self.rooms.detail(*id).is_some())
            .map(|_| self.config.instance_id);
        json!({
            "id":profile.player_id, "displayName":profile.name, "online":activity.online,
            "platformState":platform_state, "currentMatchId":match_id,
            "currentMatchHostId":local_host, "lastSeenAt":DateTime::from_timestamp(activity.last_seen, 0),
        })
    }
}

async fn players(
    State(state): State<Admin>,
    query: std::result::Result<Query<PlayerQuery>, QueryRejection>,
) -> Result<Json<Value>> {
    let Query(query) = query.map_err(|_| Problem::invalid())?;
    let limit = page_limit(query.limit)?;
    let after = query
        .cursor
        .as_deref()
        .map(|cursor| state.decode_cursor::<PlayerCursor>("players", cursor))
        .transpose()?
        .map(|cursor| {
            (cursor.online == query.online)
                .then_some(cursor.after)
                .ok_or_else(Problem::invalid)
        })
        .transpose()?;
    let mut rows = state
        .platform
        .player_activity_page(after, query.online, limit + 1)
        .await?;
    let next = if rows.len() > limit {
        rows.truncate(limit);
        let after = rows.last().expect("positive page limit").profile.player_id;
        Some(state.encode_cursor(
            "players",
            &PlayerCursor {
                online: query.online,
                after,
            },
        )?)
    } else {
        None
    };
    let items: Vec<_> = rows.into_iter().map(|row| state.player_view(row)).collect();
    Ok(Json(json!({ "items":items, "nextCursor":next })))
}

async fn player(
    State(state): State<Admin>,
    id: std::result::Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>> {
    let Path(id) = id.map_err(|_| Problem::invalid())?;
    let activity = state
        .platform
        .player_activity(id)
        .await?
        .ok_or_else(Problem::not_found)?;
    Ok(Json(state.player_view(activity)))
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum MatchPhase {
    Allocated,
    HeroSelect,
    Loading,
    Running,
    Ending,
    Ended,
}

impl MatchPhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Allocated => "allocated",
            Self::HeroSelect => "hero_select",
            Self::Loading => "loading",
            Self::Running => "running",
            Self::Ending => "ending",
            Self::Ended => "ended",
        }
    }
}

#[derive(Deserialize)]
struct MatchQuery {
    cursor: Option<String>,
    limit: Option<usize>,
    phase: Option<MatchPhase>,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
struct MatchOrder {
    created: DateTime<Utc>,
    id: Uuid,
}

#[derive(Serialize, Deserialize)]
struct MatchCursor {
    phase: Option<MatchPhase>,
    order: MatchOrder,
}

fn match_order(room: &Value) -> Result<MatchOrder> {
    Ok(MatchOrder {
        created: serde_json::from_value(room["createdAt"].clone())
            .map_err(|_| Problem::internal())?,
        id: serde_json::from_value(room["id"].clone()).map_err(|_| Problem::internal())?,
    })
}

async fn matches(
    State(state): State<Admin>,
    query: std::result::Result<Query<MatchQuery>, QueryRejection>,
) -> Result<Json<Value>> {
    let Query(query) = query.map_err(|_| Problem::invalid())?;
    let limit = page_limit(query.limit)?;
    let after = query
        .cursor
        .as_deref()
        .map(|cursor| state.decode_cursor::<MatchCursor>("matches", cursor))
        .transpose()?
        .map(|cursor| {
            (cursor.phase == query.phase)
                .then_some(cursor.order)
                .ok_or_else(Problem::invalid)
        })
        .transpose()?;
    let mut keyed = state
        .rooms
        .list()
        .into_iter()
        .filter(|room| {
            query
                .phase
                .is_none_or(|phase| room["phase"] == phase.as_str())
        })
        .map(|room| Ok((match_order(&room)?, room)))
        .collect::<Result<Vec<_>>>()?;
    keyed.retain(|(key, _)| after.as_ref().is_none_or(|cursor| key < cursor));
    keyed.sort_unstable_by(|(left, _), (right, _)| right.cmp(left));
    let has_more = keyed.len() > limit;
    keyed.truncate(limit);
    let next = if has_more {
        let order = match_order(&keyed.last().expect("positive page limit").1)?;
        Some(state.encode_cursor(
            "matches",
            &MatchCursor {
                phase: query.phase,
                order,
            },
        )?)
    } else {
        None
    };
    let items: Vec<_> = keyed.into_iter().map(|(_, value)| value).collect();
    Ok(Json(json!({ "items":items, "nextCursor":next })))
}

async fn match_detail(
    State(state): State<Admin>,
    id: std::result::Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>> {
    let Path(id) = id.map_err(|_| Problem::invalid())?;
    let mut room = state.rooms.detail(id).ok_or_else(Problem::not_found)?;
    if let Some(participants) = room["participants"].as_array_mut() {
        for participant in participants {
            if let Ok(id) = serde_json::from_value(participant["playerId"].clone())
                && let Some(activity) = state.platform.player_activity(id).await?
            {
                participant["displayName"] = json!(activity.profile.name);
                if participant["homeInstanceId"].is_null() {
                    participant["homeInstanceId"] = json!(state.config.instance_id);
                }
            }
        }
    }
    Ok(Json(room))
}

#[derive(Deserialize)]
struct EventQuery {
    #[serde(default)]
    after: u64,
    limit: Option<usize>,
}

async fn match_events(
    State(state): State<Admin>,
    id: std::result::Result<Path<Uuid>, PathRejection>,
    query: std::result::Result<Query<EventQuery>, QueryRejection>,
) -> Result<Json<Value>> {
    let Path(id) = id.map_err(|_| Problem::invalid())?;
    let Query(query) = query.map_err(|_| Problem::invalid())?;
    let limit = page_limit(query.limit)?;
    state
        .rooms
        .events(id, query.after, limit)
        .map(Json)
        .ok_or_else(Problem::not_found)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TerminationRequest {
    reason: String,
}

async fn terminate(
    State(state): State<Admin>,
    Extension(session): Extension<Session>,
    id: std::result::Result<Path<Uuid>, PathRejection>,
    input: std::result::Result<Json<TerminationRequest>, JsonRejection>,
) -> Result<impl IntoResponse> {
    let Path(id) = id.map_err(|_| Problem::invalid())?;
    let Json(input) = input.map_err(|_| Problem::invalid())?;
    let (finished, record) = state
        .rooms
        .terminate(id, &session.username, &input.reason)
        .map_err(|error| match error {
            TerminateError::Invalid => Problem::invalid(),
            TerminateError::Missing => Problem::not_found(),
            TerminateError::Conflict => Problem::new(
                StatusCode::CONFLICT,
                "termination_conflict",
                "Termination conflicts with room state",
            ),
        })?;
    Ok((
        if finished {
            StatusCode::OK
        } else {
            StatusCode::ACCEPTED
        },
        Json(record),
    ))
}
