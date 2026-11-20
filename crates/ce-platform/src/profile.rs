use crate::wire::Empty;
use halcyon_platform::PlayerProfile;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

#[derive(Default, Serialize)]
struct Currency {
    gold: u32,
    silver: u32,
    essence: u32,
    opal: u32,
    epic_key: u32,
    seasonal_key: u32,
}

pub(crate) fn empty_currency() -> Value {
    serde_json::to_value(Currency::default()).expect("native currency defaults")
}

/// CE reads these fields both from getPlayerInfo and from the nested session
/// profile. Persistent player data is projected over the new-guest defaults.
pub(crate) fn player_info(p: &PlayerProfile) -> Value {
    serde_json::to_value(PlayerInfo::new(&p.name)).expect("native player profile")
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct RankedProgress {
    skill_tier: u8,
    season_max_skill_tier: u8,
    elo_bucket: u8,
    elo_earned: u32,
    elo_earned_decay_start: u64,
    elo_earned_decay: f64,
    skill_tier_progress: f64,
    skill_tier_silver_line: f64,
    skill_tier_gold_line: f64,
    skill_tier_bronze_line: f64,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct BlitzProgress {
    elo_bucket: u8,
    elo_earned: f64,
}

#[derive(Default, Serialize)]
struct RankedModes {
    ranked: RankedProgress,
    #[serde(rename = "5v5_pvp_ranked")]
    five_vs_five: RankedProgress,
    blitz_pvp_ranked: BlitzProgress,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Spender {
    spender_level: u32,
    lifetime_spend: u64,
    last_spend_time: u64,
    lifetime_points: u64,
    lifetime_spend_count: u64,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Targeting {
    currency: Currency,
    offers: Empty,
    cohort: u32,
    spender_profile: Spender,
    level: u16,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Penalties {
    match_rejection_count: u32,
    reason: PenaltyReason,
    ttl: i32,
}

#[derive(Default, Serialize)]
enum PenaltyReason {
    #[default]
    None,
}

impl Default for Penalties {
    fn default() -> Self {
        Self {
            match_rejection_count: 0,
            reason: PenaltyReason::None,
            ttl: -2,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TutorialState {
    state: i64,
    pub client_mode: i64,
}

impl Default for TutorialState {
    fn default() -> Self {
        Self {
            state: -1,
            client_mode: -1,
        }
    }
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlayerInfo<'a> {
    handle: &'a str,
    player_handle: &'a str,
    level: u16,
    xp: u64,
    #[serde(rename = "levelMinXP")]
    level_min_xp: u64,
    #[serde(rename = "levelMaxXP")]
    level_max_xp: u64,
    karma_level: u8,
    karma_silver_bonus: f64,
    wins: u64,
    completed: u64,
    #[serde(rename = "completed_non_tutorial")]
    completed_non_tutorial: u64,
    win_streak: u64,
    loss_streak: u64,
    wins_today: u64,
    #[serde(rename = "wins_aral")]
    wins_aral: u64,
    #[serde(rename = "wins_blitz")]
    wins_blitz: u64,
    #[serde(rename = "wins_blitz_rounds_casual")]
    wins_blitz_rounds_casual: u64,
    #[serde(rename = "wins_casual")]
    wins_casual: u64,
    #[serde(rename = "wins_ranked")]
    wins_ranked: u64,
    skill_tier: u8,
    skill_tier5v5: u8,
    season_max_skill_tier: u8,
    mmr_bucket: u8,
    skill_tier_bronze_line: f64,
    skill_tier_silver_line: f64,
    skill_tier_gold_line: f64,
    skill_tier_progress: f64,
    #[serde(rename = "elo_earned")]
    elo_earned: f64,
    #[serde(rename = "elo_earned_5v5")]
    elo_earned_5v5: f64,
    #[serde(rename = "elo_earned_blitz")]
    elo_earned_blitz: f64,
    #[serde(rename = "elo_earned_decay")]
    elo_earned_decay: f64,
    #[serde(rename = "elo_earned_decay_start")]
    elo_earned_decay_start: u64,
    skill_progression_info: RankedModes,
    #[serde(rename = "guildUUID")]
    guild_uuid: Option<Uuid>,
    #[serde(rename = "partyUUID")]
    party_uuid: Option<Uuid>,
    #[serde(rename = "teamUUID")]
    team_uuid: Option<Uuid>,
    pending_party_invites: u32,
    pending_messages: u32,
    pending_guild_invites: u32,
    pending_team_invites: u32,
    account_penalty_box_data: Penalties,
    tutorial_state: TutorialState,
    can_use_all_heroes: bool,
    #[serde(flatten)]
    entitlements: Entitlements,
    is_dev: bool,
    is_organizer: bool,
    #[serde(rename = "last_login")]
    last_login: f64,
    freeunlocks: [(); 0],
    unlocks: [(); 0],
    player_buffs: [(); 0],
    player_chests: [(); 0],
    trophy_case: [(); 0],
    vg_supporter_end_time: u64,
    sinister_seven_progress: f64,
    currency: Currency,
    targeting: Targeting,
    #[serde(flatten)]
    spender: Spender,
}

#[derive(Default, Serialize)]
struct Entitlements {
    entitlement_casual: bool,
    entitlement_5v5_casual: bool,
    entitlement_coopbots: bool,
    entitlement_practice: bool,
    entitlement_ranked: bool,
    entitlement_5v5_ranked: bool,
    entitlement_draft: bool,
}

impl<'a> PlayerInfo<'a> {
    fn new(handle: &'a str) -> Self {
        Self {
            handle,
            player_handle: handle,
            level: 1,
            level_max_xp: 100,
            can_use_all_heroes: true,
            entitlements: Entitlements {
                entitlement_casual: true,
                entitlement_5v5_casual: true,
                entitlement_coopbots: true,
                entitlement_practice: true,
                ..Default::default()
            },
            targeting: Targeting {
                level: 1,
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

pub(crate) fn tutorial_state(value: &Value) -> Option<TutorialState> {
    if let Some(state) = value.as_i64() {
        return Some(TutorialState {
            state,
            client_mode: -1,
        });
    }
    Some(TutorialState {
        state: value.get("state")?.as_i64()?,
        client_mode: value.get("clientMode").map_or(Some(-1), Value::as_i64)?,
    })
}
