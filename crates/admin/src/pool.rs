use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolHealth {
    Online,
    NoQuorum,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolStatus {
    pub pool_id: Uuid,
    pub state: PoolHealth,
    pub local_role: PoolRole,
    pub leader_id: Option<Uuid>,
    pub voter_count: usize,
    pub last_contact_at: Option<chrono::DateTime<chrono::Utc>>,
    pub eligible_hosts: Option<usize>,
    pub local_tickets: usize,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PoolRole {
    Learner,
    Follower,
    Candidate,
    Leader,
    Shutdown,
}
