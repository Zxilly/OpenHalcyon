use serde::{Deserialize, Serialize};

/// Bot difficulty carried by queue and reservation data.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BotDifficulty {
    VeryEasy,
    Easy,
    Medium,
    Hard,
    VeryHard,
}

impl BotDifficulty {
    pub const ALL: [Self; 5] = [
        Self::VeryEasy,
        Self::Easy,
        Self::Medium,
        Self::Hard,
        Self::VeryHard,
    ];

    /// OpenHalcyon hero-bot reaction policy, in 20 Hz simulation ticks.
    pub fn decision_ticks(self) -> u64 {
        match self {
            Self::VeryEasy => 16,
            Self::Easy => 10,
            Self::Medium => 5,
            Self::Hard => 3,
            Self::VeryHard => 2,
        }
    }

    /// Bind a rules variant to the existing federation compatibility digest.
    pub fn rules_digest(self, base: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut hash = Sha256::new();
        hash.update(b"openhalcyon/hero-bot-cadence/v1\0");
        hash.update(self.as_str().as_bytes());
        hash.update(b"\0");
        hash.update(self.decision_ticks().to_be_bytes());
        hash.update(base.as_bytes());
        format!("{:x}", hash.finalize())
    }

    pub fn from_wire(value: &str) -> Option<Self> {
        Some(match value {
            "very_easy" => Self::VeryEasy,
            "easy" => Self::Easy,
            "medium" => Self::Medium,
            "hard" => Self::Hard,
            "very_hard" => Self::VeryHard,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::VeryEasy => "very_easy",
            Self::Easy => "easy",
            Self::Medium => "medium",
            Self::Hard => "hard",
            Self::VeryHard => "very_hard",
        }
    }

    pub fn supports_queue(mode: &str) -> bool {
        crate::KnownQueue::parse(mode).is_some_and(crate::KnownQueue::supports_bot_difficulty)
    }
}
