use crate::activity::PlayerActivityFields;
use crate::wire::Empty;
use halcyon_platform::PlayerProfile;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

/// Currency IDs are operator-defined. These six are the native initial balances.
#[derive(Clone, Serialize)]
#[serde(transparent)]
pub(crate) struct Currency(BTreeMap<String, u32>);

impl Default for Currency {
    fn default() -> Self {
        Self(
            [
                "gold",
                "silver",
                "essence",
                "opal",
                "epic_key",
                "seasonal_key",
            ]
            .into_iter()
            .map(|id| (id.to_owned(), 0))
            .collect(),
        )
    }
}

impl Currency {
    pub(crate) fn extend(&mut self, balances: impl IntoIterator<Item = (String, u32)>) {
        self.0.extend(balances);
    }
}

pub(crate) fn player_info(p: &PlayerProfile, fields: PlayerActivityFields) -> Value {
    serde_json::to_value(PlayerInfo::new(&p.name, fields)).expect("native player profile")
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
    #[serde(flatten)]
    activity: PlayerActivityFields,
    account_penalty_box_data: Penalties,
    can_use_all_heroes: bool,
    #[serde(flatten)]
    entitlements: Entitlements,
    is_dev: bool,
    is_organizer: bool,
    freeunlocks: [(); 0],
    unlocks: [(); 0],
    player_buffs: [(); 0],
    player_chests: [(); 0],
    trophy_case: [(); 0],
    vg_supporter_end_time: u64,
    sinister_seven_progress: f64,
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
    fn new(handle: &'a str, activity: PlayerActivityFields) -> Self {
        let activity = activity.with_defaults();
        let currency = activity.currency.clone();
        Self {
            handle,
            player_handle: handle,
            level: 1,
            level_max_xp: 100,
            can_use_all_heroes: true,
            activity,
            entitlements: Entitlements {
                entitlement_casual: true,
                entitlement_5v5_casual: true,
                entitlement_coopbots: true,
                entitlement_practice: true,
                ..Default::default()
            },
            targeting: Targeting {
                level: 1,
                currency,
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
