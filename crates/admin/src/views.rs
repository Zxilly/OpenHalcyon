use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ProblemCode {
    InvalidRequest,
    Unauthorized,
    NotFound,
    InternalError,
    OriginRejected,
    RateLimited,
    TerminationConflict,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ProblemResponse {
    pub title: &'static str,
    pub status: u16,
    pub code: ProblemCode,
    pub request_id: Uuid,
}
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Admission {
    Disabled,
    Full,
    Accepting,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Overview {
    pub sampled_at: DateTime<Utc>,
    pub started_at: DateTime<Utc>,
    pub server_version: &'static str,
    pub client_build: u32,
    pub online_players: u64,
    pub queued_players: u64,
    pub active_matches: usize,
    pub hosting: Hosting,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Hosting {
    pub instance_id: Uuid,
    pub admission: Admission,
    pub human_seats: usize,
    pub bot_seats: usize,
    pub reserved_seats: usize,
    pub tick_p99_ms: Option<f64>,
    pub cpu_percent: Option<f64>,
    pub rss_bytes: Option<u64>,
    pub outbound_bytes_per_second: Option<u64>,
}
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum PlayerState {
    Offline,
    Menus,
    PendingAuto,
    MatchPending,
    Playing,
    PostMatch,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PlayerView {
    pub id: Uuid,
    pub display_name: String,
    pub online: bool,
    pub platform_state: PlayerState,
    pub current_match_id: Option<Uuid>,
    pub current_match_host_id: Option<Uuid>,
    pub last_seen_at: Option<DateTime<Utc>>,
}
#[derive(Serialize)]
pub(super) struct Items<T> {
    pub items: Vec<T>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}
#[derive(Clone, Copy)]
pub(super) enum CursorResource {
    Players,
    Matches,
}
impl CursorResource {
    pub fn as_bytes(self) -> &'static [u8] {
        match self {
            Self::Players => b"players",
            Self::Matches => b"matches",
        }
    }
}
