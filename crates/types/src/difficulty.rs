//! Queue bot difficulty and talent grant contracts shared across data, hosting and matchmaking.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Numeric ARAL entitlement per hero template. This never identifies a named talent.
pub type TalentLevels = BTreeMap<u16, std::num::NonZeroU8>;

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
    pub const fn decision_ticks(self) -> u64 {
        match self {
            Self::VeryEasy => 16,
            Self::Easy => 10,
            Self::Medium => 5,
            Self::Hard => 3,
            Self::VeryHard => 2,
        }
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

    pub const fn as_str(self) -> &'static str {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bot_difficulty_wire_strings_and_ticks_round_trip() {
        for difficulty in BotDifficulty::ALL {
            assert_eq!(
                BotDifficulty::from_wire(difficulty.as_str()),
                Some(difficulty)
            );
            assert!(difficulty.decision_ticks() > 0);
        }
        assert_eq!(BotDifficulty::from_wire("unknown"), None);
        assert!(BotDifficulty::supports_queue("solo"));
        assert!(!BotDifficulty::supports_queue("ranked"));
    }
}
