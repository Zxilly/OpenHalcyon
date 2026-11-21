use std::{sync::Arc, time::Duration};

use axum::{
    Extension, Json, Router,
    extract::{
        Path, Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::{get, post},
};
use halcyon_data::BotDifficulty;
use halcyon_types::player_actions;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    PlatformError, PlayerProfile, Result,
    events::action_api,
    service::{AppState, EventBatch, Guest, digest},
};

pub fn router(state: AppState) -> Router {
    router_with_matches(state.clone(), Arc::new(state))
}

/// Match operations selected by the home service. The public API authenticates
/// with the same opaque bearer token regardless of the configured authority.
#[async_trait::async_trait]
pub trait PublicMatches: Send + Sync {
    async fn player_action(
        &self,
        _token: &str,
        _room: Uuid,
        _call: player_actions::ActionCall,
    ) -> Result<player_actions::ActionReply> {
        Err(PlatformError::HostUnavailable)
    }

    async fn enqueue(
        &self,
        token: &str,
        mode: String,
        difficulty: Option<BotDifficulty>,
    ) -> Result<()>;
    async fn cancel_queue(&self, token: &str) -> Result<()>;
    async fn accept_offer(&self, token: &str, id: Uuid) -> Result<()>;
    async fn match_chat(&self, token: &str, id: Uuid, text: String) -> Result<()>;
}

pub fn router_with_matches(state: AppState, matches: Arc<dyn PublicMatches>) -> Router {
    Router::new()
        .route("/bootstrap", get(bootstrap))
        .route("/api/v1/guests", post(create_guest))
        .route("/api/v1/sessions", post(resume_guest))
        .route("/api/v1/me", get(profile))
        .route("/api/v1/queue", post(queue).delete(leave))
        .route("/api/v1/matches/{id}/accept", post(accept))
        .route("/api/v1/matches/{id}/chat", post(chat))
        .route(
            "/api/v1/matches/{id}/actions",
            get(action_api::inspect)
                .post(action_api::execute)
                .layer(axum::extract::DefaultBodyLimit::max(1024)),
        )
        .route("/api/v1/events", get(poll))
        .route("/api/v1/events/ws", get(websocket))
        .layer(Extension(matches))
        .with_state(state)
}

#[async_trait::async_trait]
impl PublicMatches for AppState {
    async fn player_action(
        &self,
        token: &str,
        room: Uuid,
        call: player_actions::ActionCall,
    ) -> Result<player_actions::ActionReply> {
        let player = self.playing_player(token, room).await?;
        self.host
            .local_action(room, player, call)
            .map_err(Into::into)
    }

    async fn enqueue(
        &self,
        token: &str,
        mode: String,
        difficulty: Option<BotDifficulty>,
    ) -> Result<()> {
        let seats = self.seats_for_mode(&mode);
        self.join_with_difficulty(token, mode, seats, difficulty)
            .await
    }

    async fn cancel_queue(&self, token: &str) -> Result<()> {
        self.leave(token).await
    }

    async fn accept_offer(&self, token: &str, id: Uuid) -> Result<()> {
        self.accept(token, id).await
    }

    async fn match_chat(&self, token: &str, id: Uuid, text: String) -> Result<()> {
        self.send_chat(token, id, text).await
    }
}

pub(crate) fn bearer(headers: &HeaderMap) -> Result<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .filter(|v| v.len() == 64)
        .ok_or(PlatformError::Unauthorized)
}

#[derive(Deserialize, Serialize)]
pub struct CreateGuest {
    pub name: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeGuest {
    pub recovery_token: String,
}

#[derive(Deserialize, Serialize)]
pub struct Queue {
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub difficulty: Option<BotDifficulty>,
}

#[derive(Deserialize, Serialize)]
pub struct Chat {
    pub text: String,
}

#[derive(Clone, Copy, Deserialize)]
struct Cursor {
    #[serde(default)]
    after: i64,
}

async fn bootstrap(State(state): State<AppState>) -> String {
    format!("{}\n", state.config.public_address)
}

async fn create_guest(
    State(state): State<AppState>,
    Json(input): Json<CreateGuest>,
) -> Result<Json<Guest>> {
    state.create_guest(input.name).await.map(Json)
}

async fn resume_guest(
    State(state): State<AppState>,
    Json(input): Json<ResumeGuest>,
) -> Result<Json<Guest>> {
    state.resume_guest(input.recovery_token).await.map(Json)
}

async fn profile(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<PlayerProfile>> {
    state.profile(bearer(&headers)?).await.map(Json)
}

async fn queue(
    Extension(matches): Extension<Arc<dyn PublicMatches>>,
    headers: HeaderMap,
    Json(input): Json<Queue>,
) -> Result<StatusCode> {
    let session = bearer(&headers)?;
    matches
        .enqueue(session, input.mode, input.difficulty)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn leave(
    Extension(matches): Extension<Arc<dyn PublicMatches>>,
    headers: HeaderMap,
) -> Result<StatusCode> {
    matches.cancel_queue(bearer(&headers)?).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn accept(
    Extension(matches): Extension<Arc<dyn PublicMatches>>,
    headers: HeaderMap,
    Path(match_uuid): Path<Uuid>,
) -> Result<StatusCode> {
    matches.accept_offer(bearer(&headers)?, match_uuid).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn chat(
    Extension(matches): Extension<Arc<dyn PublicMatches>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<Chat>,
) -> Result<StatusCode> {
    matches
        .match_chat(bearer(&headers)?, id, input.text)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn poll(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(cursor): Query<Cursor>,
) -> Result<Json<EventBatch>> {
    if cursor.after < 0 {
        return Err(PlatformError::Invalid("invalid event cursor"));
    }
    let key = digest(bearer(&headers)?);
    state.wait_events(&key, cursor.after).await.map(Json)
}

async fn websocket(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(cursor): Query<Cursor>,
    ws: WebSocketUpgrade,
) -> Result<Response> {
    if cursor.after < 0 {
        return Err(PlatformError::Invalid("invalid event cursor"));
    }
    let key = digest(bearer(&headers)?);
    state.read_events(key.clone(), cursor.after).await?;
    Ok(ws
        .max_message_size(1024)
        .on_upgrade(move |socket| stream(state, key, cursor.after, socket)))
}

async fn stream(state: AppState, key: Vec<u8>, mut after: i64, mut socket: WebSocket) {
    loop {
        let update = state.wait_events(&key, after);
        tokio::pin!(update);
        let batch = loop {
            tokio::select! {
                result = &mut update => match result {
                    Ok(batch) => break batch,
                    Err(_) => {
                        let _ = socket.send(Message::Close(None)).await;
                        return;
                    },
                },
                input = socket.recv() => match input {
                    Some(Ok(Message::Pong(_) | Message::Ping(_))) => {},
                    _ => return,
                }
            }
        };
        after = batch.cursor;
        let message = if batch.events.is_empty() {
            Message::Ping(Vec::new().into())
        } else if let Ok(body) = serde_json::to_string(&batch) {
            Message::Text(body.into())
        } else {
            return;
        };
        if !matches!(
            tokio::time::timeout(Duration::from_secs(10), socket.send(message)).await,
            Ok(Ok(()))
        ) {
            return;
        }
    }
}
