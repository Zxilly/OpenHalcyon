//! Match UI, HUD, flyouts, surrender voting, post-match statistics, announcements, and match lifecycle clock/end packets.

mod announcements;
mod clock_end;
mod flyout;
mod hud;
pub(crate) mod statistics;
mod surrender;

pub use announcements::{AnnouncementBody, AnnouncementKey, KickPlayerBody};
pub use clock_end::{
    ClockSyncBody, CrystalShatteredBody, EndMatchBody, EndMatchReason, ModifyGameModeVarBody,
    StartMatchBody, WorldClockBody,
};
pub use flyout::{Flyout, FlyoutText};
pub use hud::{HudQuickMessage, MapPing};
pub use statistics::{
    MatchStatistics, MatchStatisticsMut, PlayerStatistics, StatisticsReport, StatisticsWireBody,
    TeamStatistics,
};
pub use surrender::{
    AralSurrenderBoard, MAX_SURRENDER_SLOTS, SurrenderPhase, SurrenderRequestBody, SurrenderVote,
    TeamSurrenderBoard,
};
