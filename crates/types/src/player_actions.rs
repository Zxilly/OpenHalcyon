//! Player-facing match actions. The host resolves the actor from its authorized human roster.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum PlayerAction {
    ActivateReflexBlock {
        instance: u32,
    },
    ActivateItem {
        instance: u32,
        #[serde(default)]
        target_actor: Option<u32>,
        #[serde(default)]
        x: Option<f32>,
        #[serde(default)]
        z: Option<f32>,
    },
    ConsumeInfusion {
        instance: u32,
    },
    DeployVisionTotem {
        instance: u32,
        x: f32,
        z: f32,
    },
    DeployScoutTrap {
        instance: u32,
    },
    HitScoutCam {
        device: u32,
    },
    Sell {
        instance: u32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActionRequest {
    pub sequence: u64,
    pub expires_at_ms: i64,
    pub action: PlayerAction,
}

impl ActionRequest {
    pub fn valid_at(self, now: i64) -> bool {
        (1..=9_007_199_254_740_991).contains(&self.sequence)
            && self.expires_at_ms > now
            && self.expires_at_ms <= now.saturating_add(30_000)
            && match self.action {
                PlayerAction::ActivateItem {
                    instance,
                    target_actor,
                    x,
                    z,
                } => {
                    instance != 0
                        && target_actor.is_none_or(|t| t != 0)
                        && match (x, z) {
                            (Some(px), Some(pz)) => px.is_finite() && pz.is_finite(),
                            (None, None) => true,
                            _ => false,
                        }
                }
                PlayerAction::DeployVisionTotem { instance, x, z } => {
                    instance != 0 && x.is_finite() && z.is_finite()
                }
                PlayerAction::HitScoutCam { device } => device != 0,
                PlayerAction::ActivateReflexBlock { instance }
                | PlayerAction::ConsumeInfusion { instance }
                | PlayerAction::DeployScoutTrap { instance }
                | PlayerAction::Sell { instance } => instance != 0,
            }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum ActionCall {
    Inspect,
    Execute(ActionRequest),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeldInstance {
    pub template: u32,
    pub instance: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionContext {
    pub next_sequence: u64,
    pub x: f32,
    pub z: f32,
    pub items: Vec<HeldInstance>,
    pub visible_enemy_cams: Vec<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionDecision {
    Applied,
    Rejected,
    Stale,
    Unavailable,
    Invalid,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ActionReply {
    Context(ActionContext),
    Decision(ActionDecision),
}
