//! Queue IDs retain their spelling through matchmaking. Multiple IDs may
//! select the same client scene while using different admission policies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMode {
    CasualFive,
    RankedFive,
    CasualFold,
    RankedFold,
    Blitz,
    SoloBots,
    CoopBots,
    Private,
    PrivateDraft,
    PrivateBots,
    Practice,
    Aral,
}

impl NativeMode {
    pub const fn asset(self) -> &'static str {
        match self {
            Self::CasualFive => "*GameMode_5v5_Casual*",
            Self::RankedFive => "*GameMode_5v5_Ranked*",
            Self::CasualFold => "*GameMode_HF_Casual*",
            Self::RankedFold => "*GameMode_HF_Ranked*",
            Self::Blitz => "*GameMode_Blitz_Revamp*",
            Self::SoloBots => "*GameMode_5v5_SoloBots*",
            Self::CoopBots => "*GameMode_5v5_CoopBots*",
            Self::Private => "*GameMode_5v5_Private*",
            Self::PrivateDraft => "*GameMode_5v5_PrivateDraft*",
            Self::PrivateBots => "*GameMode_5v5_PrivateBots*",
            Self::Practice => "*GameMode_5v5_Practice*",
            Self::Aral => "*GameMode_Aral_Casual*",
        }
    }

    pub const fn is_three_per_side(self) -> bool {
        matches!(
            self,
            Self::Aral | Self::CasualFold | Self::RankedFold | Self::Blitz
        )
    }

    pub const fn is_halcyon_fold(self) -> bool {
        matches!(self, Self::CasualFold | Self::RankedFold)
    }

    pub const fn is_blitz(self) -> bool {
        matches!(self, Self::Blitz)
    }
}

// One row per spelling so callers can match either an exact CE lobby or its scene.
macro_rules! queues {
    ($($id:ident = $wire:literal => $scene:ident),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
        pub enum KnownQueue {
            $(#[serde(rename = $wire)] $id,)+
        }

        impl KnownQueue {
            pub const ALL: &'static [Self] = &[$(Self::$id,)+];
            pub fn parse(queue: &str) -> Option<Self> {
                match queue {
                    $($wire => Some(Self::$id),)+
                    _ => None,
                }
            }
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$id => $wire,)+ }
            }
            pub const fn mode(self) -> NativeMode {
                match self { $(Self::$id => NativeMode::$scene,)+ }
            }
        }
    };
}

impl KnownQueue {
    /// Client scene mode asset passed in `MatchSettings` (`0x0458`), `DraftModeAnnouncement` (`0x0459`),
    /// and `SceneSettings` (`0x046E`).
    pub const fn asset(self) -> &'static str {
        match self {
            Self::BlitzBotsSolo => "*GameMode_Blitz_SoloBots*",
            Self::BlitzPvpPure => "*GameMode_Blitz_Pure*",
            Self::SoloBotsFold | Self::StandardBotsSolo => "*GameMode_HF_SoloBots*",
            Self::BattleRoyaleBotsSolo => "*GameMode_Aral_SoloBots*",
            _ => self.mode().asset(),
        }
    }

    /// True for single-human vs AI bots queues (auto-backfilled on room allocation).
    pub const fn is_solo_bots(self) -> bool {
        matches!(
            self,
            Self::SoloBots
                | Self::Solo
                | Self::BlitzBotsSolo
                | Self::SoloBotsFold
                | Self::StandardBotsSolo
                | Self::BattleRoyaleBotsSolo
        )
    }

    pub const fn is_practice(self) -> bool {
        matches!(self, Self::Practice)
    }

    /// True for queues whose matchmaking admission requires one human seat.
    pub const fn is_single_human(self) -> bool {
        self.is_solo_bots() || self.is_practice()
    }

    pub const fn supports_bot_difficulty(self) -> bool {
        self.is_single_human()
            || matches!(
                self.mode(),
                NativeMode::SoloBots | NativeMode::CoopBots | NativeMode::PrivateBots
            )
    }

    /// True for queues played on a 3-per-side (6-hero) map layout, including 1+5 bot matches.
    pub const fn is_three_per_side_map(self) -> bool {
        self.mode().is_three_per_side()
    }

    /// True for PvP 3-per-side queues requiring 6 human seats before matchmaking completes.
    pub const fn requires_six_humans(self) -> bool {
        self.is_three_per_side_map() && !self.is_single_human()
    }

    /// Total hero seats allocated inside the game room (6 for Fold/Blitz/ARAL, 10 for 5v5).
    pub const fn hero_seats(self) -> usize {
        if self.is_three_per_side_map() { 6 } else { 10 }
    }

    /// Human seats expected during matchmaking admission.
    pub const fn human_seats(self, default_seats: usize) -> usize {
        if self.is_single_human() {
            1
        } else if self.is_three_per_side_map() {
            6
        } else {
            default_seats
        }
    }
}

queues! {
    Ranked = "ranked" => RankedFold,
    PvpRankedFold = "3v3_pvp_ranked" => RankedFold,
    FoldRanked = "3v3_ranked" => RankedFold,
    RankedFold = "ranked_3v3" => RankedFold,
    PvpCasualFold = "3v3_pvp_casual" => CasualFold,
    FoldCasual = "3v3_casual" => CasualFold,
    CasualFold = "casual_3v3" => CasualFold,
    SoloBotsFold = "solo_bots" => CasualFold,
    StandardBotsSolo = "standard_bots_solo" => CasualFold,
    BlitzThree = "blitz_3v3" => Blitz,
    Blitz = "blitz" => Blitz,
    BlitzPvpRanked = "blitz_pvp_ranked" => Blitz,
    BlitzPvpRevamp = "blitz_pvp_revamp" => Blitz,
    BlitzPvpPure = "blitz_pvp_pure" => Blitz,
    BlitzBotsSolo = "blitz_bots_solo" => Blitz,
    BlitzRanked = "blitz_ranked" => Blitz,
    BlitzCasual = "blitz_casual" => Blitz,
    PvpRankedFive = "5v5_pvp_ranked" => RankedFive,
    FiveRanked = "5v5_ranked" => RankedFive,
    RankedFive = "ranked_5v5" => RankedFive,
    PvpCasualFive = "5v5_pvp_casual" => CasualFive,
    FiveCasual = "5v5_casual" => CasualFive,
    CasualFive = "casual_5v5" => CasualFive,
    SoloBots = "5v5_bots_solo" => SoloBots,
    Solo = "solo" => SoloBots,
    CoopBots = "5v5_bots_coop" => CoopBots,
    Coop = "5v5_coop" => CoopBots,
    Private = "5v5_private" => Private,
    PvpPrivate = "5v5_pvp_private" => Private,
    PrivateDraft = "5v5_private_draft" => PrivateDraft,
    PrivateBots = "5v5_private_bots" => PrivateBots,
    Practice = "5v5_practice" => Practice,
    CasualAral = "casual_aral" => Aral,
    AralCasual = "aral_casual" => Aral,
    Aral = "aral" => Aral,
    BattleRoyaleBotsSolo = "battle_royale_bots_solo" => Aral,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queue_identity_survives_scene_aliasing() {
        for &queue in KnownQueue::ALL {
            assert_eq!(KnownQueue::parse(queue.as_str()), Some(queue));
            let json = serde_json::to_string(&queue).unwrap();
            assert_eq!(serde_json::from_str::<KnownQueue>(&json).unwrap(), queue);
            assert_eq!(
                serde_json::from_str::<String>(&json).unwrap(),
                queue.as_str()
            );
        }
        assert_eq!(KnownQueue::AralCasual.mode(), KnownQueue::CasualAral.mode());
        assert_ne!(KnownQueue::AralCasual, KnownQueue::CasualAral);
        assert_eq!(KnownQueue::PvpRankedFold.mode(), KnownQueue::Ranked.mode());
        assert_eq!(KnownQueue::PvpCasualFold.mode(), NativeMode::CasualFold);
        assert_eq!(KnownQueue::BlitzThree.mode(), NativeMode::Blitz);
        assert_eq!(KnownQueue::Blitz.mode(), NativeMode::Blitz);
        assert!(NativeMode::Blitz.is_three_per_side());
        assert!(NativeMode::CasualFold.is_three_per_side());
        assert!(NativeMode::RankedFold.is_three_per_side());
        assert_ne!(KnownQueue::Ranked.mode(), KnownQueue::RankedFive.mode());
        for unknown in ["bench_4", "CASUAL_ARAL", " solo", "3v3_standard"] {
            assert_eq!(KnownQueue::parse(unknown), None);
        }
    }

    #[test]
    fn missing_live_and_solo_queue_aliases_resolve_mode_assets_and_seats() {
        for (wire, mode, asset, hero_seats, human_seats, solo, diff) in [
            (
                "blitz_bots_solo",
                NativeMode::Blitz,
                "*GameMode_Blitz_SoloBots*",
                6,
                1,
                true,
                true,
            ),
            (
                "solo_bots",
                NativeMode::CasualFold,
                "*GameMode_HF_SoloBots*",
                6,
                1,
                true,
                true,
            ),
            (
                "standard_bots_solo",
                NativeMode::CasualFold,
                "*GameMode_HF_SoloBots*",
                6,
                1,
                true,
                true,
            ),
            (
                "battle_royale_bots_solo",
                NativeMode::Aral,
                "*GameMode_Aral_SoloBots*",
                6,
                1,
                true,
                true,
            ),
            (
                "blitz_pvp_revamp",
                NativeMode::Blitz,
                "*GameMode_Blitz_Revamp*",
                6,
                6,
                false,
                false,
            ),
            (
                "blitz_pvp_pure",
                NativeMode::Blitz,
                "*GameMode_Blitz_Pure*",
                6,
                6,
                false,
                false,
            ),
            (
                "5v5_practice",
                NativeMode::Practice,
                "*GameMode_5v5_Practice*",
                10,
                1,
                false,
                true,
            ),
        ] {
            let queue = KnownQueue::parse(wire).expect(wire);
            assert_eq!(queue.mode(), mode);
            assert_eq!(queue.asset(), asset);
            assert_eq!(queue.hero_seats(), hero_seats);
            assert_eq!(queue.human_seats(10), human_seats);
            assert_eq!(queue.is_solo_bots(), solo);
            assert_eq!(queue.is_single_human(), human_seats == 1);
            assert_eq!(queue.requires_six_humans(), human_seats == 6);
            assert_eq!(queue.supports_bot_difficulty(), diff);
        }
    }
}
