use super::*;
use serde::Serialize;
use std::collections::BTreeMap;

pub(super) fn value(dto: impl Serialize) -> Value {
    serde_json::to_value(dto).expect("room response uses serializable protocol fields")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoomPhase {
    Allocated,
    HeroSelect,
    Loading,
    Running,
    Ending,
    Ended,
}
impl RoomPhase {
    pub const fn as_str(self) -> &'static str {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomEndReason {
    Completed,
    AdminTerminated,
    Error,
}

impl RoomEndReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::AdminTerminated => "admin_terminated",
            Self::Error => "error",
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TickLatencyReport {
    pub count: u64,
    pub total_us: u64,
    pub max_us: u64,
    pub missed_deadlines: u64,
    pub over40ms: u64,
    pub recent_sample_count: usize,
    pub p99_us: Option<u64>,
    pub stages: TickStages,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TickStages {
    pub wake_delay: TickStageReport,
    pub lock_wait: TickStageReport,
    pub room_work: TickStageReport,
}

#[derive(Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TickStageReport {
    pub total_us: u64,
    pub max_us: u64,
    pub over40ms: u64,
    pub dominant_over40ms: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct EventPage<'a> {
    pub items: &'a [RoomEvent],
    pub has_more: bool,
    pub next_after: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Termination {
    pub match_id: Uuid,
    pub requested_at: String,
    pub requested_by: String,
    pub reason: String,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum EventKind {
    PhaseChanged,
    AdminAction,
    Connection,
    Protocol,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RoomEvent {
    pub seq: usize,
    pub at: String,
    pub kind: EventKind,
    pub message: String,
    #[serde(flatten)]
    pub detail: EventDetail,
}
#[derive(Clone, Serialize)]
#[serde(untagged, rename_all_fields = "camelCase")]
pub(super) enum EventDetail {
    Phase {},
    Admin {
        actor: String,
    },
    Connection {
        player_id: Uuid,
        channel: Channel,
        generation: Option<u64>,
        #[serde(flatten)]
        opcode: Option<EventOpcode>,
    },
    Action {
        player_id: Uuid,
        #[serde(flatten)]
        opcode: EventOpcode,
    },
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct EventOpcode {
    opcode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    opcode_name: Option<halcyon_protocol::Opcode>,
}

impl From<u16> for EventOpcode {
    fn from(raw: u16) -> Self {
        Self {
            opcode: format!("0x{raw:04X}"),
            opcode_name: halcyon_protocol::Opcode::try_from(raw).ok(),
        }
    }
}
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Channel {
    Control,
    Game,
}
impl Channel {
    pub fn for_game(game: bool) -> Self {
        if game { Self::Game } else { Self::Control }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomSummary {
    pub id: Uuid,
    pub queue: String,
    pub phase: RoomPhase,
    pub created_at: String,
    pub phase_since: String,
    pub ended_at: Option<String>,
    pub end_reason: Option<&'static str>,
    pub human_players: usize,
    pub bot_players: usize,
    pub pool_id: Option<Uuid>,
    pub tick_latency: TickLatencyReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub difficulty: Option<halcyon_data::BotDifficulty>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub participants: Option<Vec<RoomParticipant>>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomParticipant {
    pub slot: usize,
    pub team: u8,
    pub player_id: Option<Uuid>,
    pub home_instance_id: Option<Uuid>,
    pub bot_profile: Option<&'static str>,
    pub display_name: String,
    pub hero: String,
    pub locked: bool,
    pub control_connected: bool,
    pub game_connected: bool,
    pub last_event_at: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ResultReport<'a> {
    pub reason: &'a str,
    pub surrendered_team: Option<usize>,
    pub winner: Option<u8>,
    pub ticks: u64,
    pub players: BTreeMap<Uuid, PlayerResult<'a>>,
    pub teams: &'a [halcyon_sim::TeamScore; 2],
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PlayerResult<'a> {
    #[serde(flatten)]
    pub score: &'a halcyon_sim::PlayerScore,
    #[serde(flatten)]
    pub combat: &'a Combat,
    pub team: u8,
    pub victory: Option<bool>,
    pub hero: &'a str,
    pub gold: u32,
    pub item_templates: Vec<u32>,
    pub level: u8,
    pub xp: f32,
}
