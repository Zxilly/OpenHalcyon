use halcyon_platform::PlayerProfile;
use serde_json::{Value, json};
use std::sync::LazyLock;

pub(crate) fn empty_currency() -> Value {
    json!({"gold":0,"silver":0,"essence":0,"opal":0,"epic_key":0,"seasonal_key":0})
}

/// CE reads these fields both from getPlayerInfo and from the nested session
/// profile. Persistent player data is projected over the new-guest defaults.
pub(crate) fn player_info(p: &PlayerProfile) -> Value {
    static NEW_GUEST: LazyLock<Value> = LazyLock::new(new_guest);
    let mut info = NEW_GUEST.clone();
    info["handle"] = json!(p.name);
    info["playerHandle"] = json!(p.name);
    info
}

fn new_guest() -> Value {
    let ranked = json!({
        "skillTier":0, "seasonMaxSkillTier":0, "eloBucket":0, "eloEarned":0,
        "eloEarnedDecayStart":0, "eloEarnedDecay":0.0, "skillTierProgress":0.0,
        "skillTierSilverLine":0.0, "skillTierGoldLine":0.0, "skillTierBronzeLine":0.0
    });
    let spender = json!({"spenderLevel":0,"lifetimeSpend":0,"lastSpendTime":0,
        "lifetimePoints":0,"lifetimeSpendCount":0});
    let mut result = json!({
        "level":1, "xp":0, "levelMinXP":0,
        "levelMaxXP":100, "karmaLevel":0, "karmaSilverBonus":0.0,
        "wins":0, "completed":0, "completed_non_tutorial":0, "winStreak":0,
        "lossStreak":0, "winsToday":0, "wins_aral":0, "wins_blitz":0,
        "wins_blitz_rounds_casual":0, "wins_casual":0, "wins_ranked":0,
        "skillTier":0, "skillTier5v5":0, "seasonMaxSkillTier":0, "mmrBucket":0,
        "skillTierBronzeLine":0.0, "skillTierSilverLine":0.0, "skillTierGoldLine":0.0,
        "skillTierProgress":0.0, "elo_earned":0.0, "elo_earned_5v5":0.0,
        "elo_earned_blitz":0.0, "elo_earned_decay":0.0, "elo_earned_decay_start":0,
        "skillProgressionInfo":{"ranked":ranked,"5v5_pvp_ranked":ranked,
            "blitz_pvp_ranked":{"eloBucket":0,"eloEarned":0.0}}
    });
    let social = json!({
        "guildUUID":null,"partyUUID":null,"teamUUID":null,"pendingPartyInvites":0,
        "pendingMessages":0,"pendingGuildInvites":0,"pendingTeamInvites":0,
        "accountPenaltyBoxData":{"matchRejectionCount":0,"reason":"None","ttl":-2},
        "tutorialState":{"state":-1,"clientMode":-1}, "canUseAllHeroes":true,
        "entitlement_casual":true,"entitlement_5v5_casual":true,"entitlement_coopbots":true,
        "entitlement_practice":true,"entitlement_ranked":false,"entitlement_5v5_ranked":false,
        "entitlement_draft":false,"isDev":false,"isOrganizer":false,"last_login":0.0,
        "freeunlocks":[],"unlocks":[],"playerBuffs":[],"playerChests":[],"trophyCase":[],
        "vgSupporterEndTime":0,"sinisterSevenProgress":0.0,"currency":empty_currency(),
        "targeting":{"currency":empty_currency(),"offers":{},"cohort":0,
            "spenderProfile":spender,"level":1}
    });
    let fields = result.as_object_mut().unwrap();
    fields.extend(social.as_object().unwrap().clone());
    fields.extend(spender.as_object().unwrap().clone());
    result
}

pub(crate) fn tutorial_state(value: &Value) -> Option<Value> {
    if let Some(state) = value.as_i64() {
        return Some(json!({"state":state,"clientMode":-1}));
    }
    let state = value.get("state")?.as_i64()?;
    let mode = value.get("clientMode").map_or(Some(-1), Value::as_i64)?;
    Some(json!({"state":state,"clientMode":mode}))
}
