pub use halcyon_types::BotDifficulty;

/// Bind a rules variant to the existing federation compatibility digest.
pub fn bot_difficulty_rules_digest(difficulty: BotDifficulty, base: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(b"openhalcyon/hero-bot-cadence/v1\0");
    hash.update(difficulty.as_str().as_bytes());
    hash.update(b"\0");
    hash.update(difficulty.decision_ticks().to_be_bytes());
    hash.update(base.as_bytes());
    format!("{:x}", hash.finalize())
}

/// Extension trait preserving `BotDifficulty::rules_digest` without pulling `sha2` into `halcyon-types`.
pub trait BotDifficultyExt {
    fn rules_digest(self, base: &str) -> String;
}

impl BotDifficultyExt for BotDifficulty {
    fn rules_digest(self, base: &str) -> String {
        bot_difficulty_rules_digest(self, base)
    }
}
