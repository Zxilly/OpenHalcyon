use std::collections::BTreeMap;
use std::num::NonZeroU8;

use halcyon_data::GameData;

use super::{
    ALL_TALENTS, ActiveTalentState, LoadedTalent, LoadedTalentMod, TalentRarity, TalentSlotScope,
    TalentSpec, TalentSpecial, TalentStatBonus,
};
use crate::combat::AbilitySlot;

/// Load all talent definitions from `GameData` into deterministic `BTreeMap`.
pub(crate) fn load_talents_from_data(data: &GameData) -> BTreeMap<String, LoadedTalent> {
    let mut out = BTreeMap::new();
    for t in data.extra.combat_content().talents() {
        let mut mods = BTreeMap::new();
        for m in &t.modifiers {
            mods.insert(
                m.label.clone(),
                LoadedTalentMod {
                    label: m.label.clone(),
                    base: m.base,
                    per_level: m.per_level,
                    overdrive: m.overdrive,
                    cp_ratio: m.cp_ratio,
                    wp_ratio: m.wp_ratio,
                    clamp: m.clamp,
                },
            );
        }
        out.insert(
            t.id.clone(),
            LoadedTalent {
                id: t.id.clone(),
                hero: t.hero.clone(),
                mods,
            },
        );
    }
    out
}

/// Resolve compiled `TalentSpec` by exact talent id.
pub(crate) fn find_talent(id: &str) -> Option<&'static TalentSpec> {
    ALL_TALENTS.iter().find(|t| t.id == id)
}

/// Resolve the canonical published talent ID (`Talent_<Hero>_<Name>`) for `(type_id, rarity)`,
/// preferring published talents with an `official_id` in `GameData` before compiled specs.
pub(crate) fn find_hero_talent_by_rarity(
    type_id: u16,
    rarity: TalentRarity,
    data: Option<&GameData>,
) -> Option<&'static str> {
    let matches_hero = |hero_id: &str| {
        data.is_some_and(|d| d.type_id(hero_id) == Some(type_id))
            || crate::heroes::kits::kit_spec_by_hero_id(hero_id)
                .is_some_and(|ks| ks.type_id == type_id)
            || halcyon_data::EntityTemplate::from_raw(type_id)
                .is_some_and(|t| format!("{t:?}").eq_ignore_ascii_case(hero_id))
    };
    let rarity_str = match rarity {
        TalentRarity::Rare => "rare",
        TalentRarity::Epic => "epic",
        TalentRarity::Legendary => "legendary",
        TalentRarity::Generic => return None,
    };
    if let Some(d) = data {
        for talent in d.extra.combat_content().talents() {
            if talent.official_id.is_none() || talent.rarity.as_deref() != Some(rarity_str) {
                continue;
            }
            if let Some(hero) = talent.hero.as_deref()
                && matches_hero(hero)
                && let Some(spec) = find_talent(&talent.id)
            {
                return Some(spec.id);
            }
        }
    }
    ALL_TALENTS.iter().find_map(|spec| {
        if spec.rarity != rarity || spec.id == "Talent_Ringo_AnotherRound" {
            return None;
        }
        let hero_id = spec.hero_id?;
        matches_hero(hero_id).then_some(spec.id)
    })
}

/// Evaluate a modifier label on `talent_id` at `(level, cp, wp)`, preferring `GameData` if loaded.
pub(crate) fn eval_talent_mod(
    talent_id: &str,
    label: &str,
    level: NonZeroU8,
    cp: f32,
    wp: f32,
    loaded: Option<&BTreeMap<String, LoadedTalent>>,
) -> Option<f32> {
    if let Some(t) = loaded.and_then(|map| map.get(talent_id))
        && let Some(m) = t.mods.get(label)
    {
        return Some(m.value_at(level, cp, wp));
    }
    find_talent(talent_id).and_then(|spec| {
        spec.mods
            .iter()
            .find(|m| m.label == label)
            .map(|m| m.value_at(level, cp, wp))
    })
}

/// Check whether `talent_id` is eligible for hero (`hero_id`, e.g. `"Amael"`, `"Hero009"`, `"Sayoc"`).
#[allow(dead_code)]
pub(crate) fn talent_matches_hero(
    talent_id: &str,
    hero_id: &str,
    loaded: Option<&BTreeMap<String, LoadedTalent>>,
) -> Result<&'static TalentSpec, &'static str> {
    let spec = find_talent(talent_id).ok_or("unknown talent")?;
    if let Some(expected_hero) = spec.hero_id
        && !expected_hero.eq_ignore_ascii_case(hero_id)
    {
        return Err("talent belongs to another hero");
    }
    if spec.hero_id.is_none()
        && let Some(t) = loaded.and_then(|map| map.get(talent_id))
        && let Some(data_hero) = &t.hero
        && !data_hero.eq_ignore_ascii_case(hero_id)
    {
        return Err("talent belongs to another hero");
    }
    Ok(spec)
}

/// Helper checking cooldown modifier multiplier vs flat reduction vs attack CDR.
pub(crate) fn apply_cooldown_modifier(
    talent: &ActiveTalentState,
    slot: AbilitySlot,
    base_cd_sec: f32,
    cp: f32,
    wp: f32,
    loaded: Option<&BTreeMap<String, LoadedTalent>>,
) -> f32 {
    let Some(spec) = find_talent(&talent.id) else {
        return base_cd_sec;
    };
    if !spec.scope.matches(slot) {
        return base_cd_sec;
    }
    // Check explicit cooldown overrides first (e.g. Petal Bouncer = 1.0s override).
    if spec.special == TalentSpecial::PetalFastTrampoline
        && let Some(v) = eval_talent_mod(&talent.id, "COOLDOWN", talent.level, cp, wp, loaded)
    {
        return v.max(0.1);
    }
    if spec.special == TalentSpecial::ChurnwalkerTormentCloud
        && let Some(add_sec) = eval_talent_mod(
            &talent.id,
            "B_INCREASED_COOLDOWN",
            talent.level,
            cp,
            wp,
            loaded,
        )
    {
        return (base_cd_sec + add_sec).max(0.5);
    }
    // Skip Saw RoadRage where COOLDOWN_MODIFIER is flat reduction on basic attack, and Magnus C where it's per-hit CDR.
    if matches!(
        spec.special,
        TalentSpecial::SawRoadieGrantsSpinUpAndCdr | TalentSpecial::MagnusHitReducesAllCdr
    ) {
        return base_cd_sec;
    }
    if let Some(m) = eval_talent_mod(
        &talent.id,
        "COOLDOWN_MODIFIER",
        talent.level,
        cp,
        wp,
        loaded,
    ) {
        if m < 0.0 {
            return (base_cd_sec + m).max(0.5);
        }
        return (base_cd_sec * m).max(0.2);
    }
    base_cd_sec
}

/// Helper computing damage multiplier from an equipped talent for `slot` (or Perk/all).
pub(crate) fn talent_damage_multiplier(
    talent: &ActiveTalentState,
    slot: AbilitySlot,
    cp: f32,
    wp: f32,
    loaded: Option<&BTreeMap<String, LoadedTalent>>,
) -> f32 {
    let Some(spec) = find_talent(&talent.id) else {
        return 1.0;
    };
    // Bespoke legacy talents apply their own multipliers in combat.rs.
    if matches!(
        spec.special,
        TalentSpecial::ModeledLongShot
            | TalentSpecial::ModeledMobileBulwark
            | TalentSpecial::ModeledSpeedLoader
    ) {
        return 1.0;
    }
    if !spec.scope.matches(slot) {
        return 1.0;
    }
    if let Some(v) = eval_talent_mod(&talent.id, "DAMAGE_MODIFIER", talent.level, cp, wp, loaded)
        .or_else(|| eval_talent_mod(&talent.id, "DAMAGE_MODIFER", talent.level, cp, wp, loaded))
        .or_else(|| {
            eval_talent_mod(
                &talent.id,
                "ABILITY_DAMAGE_MODIFIER",
                talent.level,
                cp,
                wp,
                loaded,
            )
        })
    {
        return v.max(0.05);
    }
    1.0
}

/// Helper computing barrier multiplier from an equipped talent.
pub(crate) fn talent_barrier_multiplier(
    talent: &ActiveTalentState,
    slot: AbilitySlot,
    cp: f32,
    wp: f32,
    loaded: Option<&BTreeMap<String, LoadedTalent>>,
) -> f32 {
    let Some(spec) = find_talent(&talent.id) else {
        return 1.0;
    };
    if !spec.scope.matches(slot) && spec.scope != TalentSlotScope::Perk {
        return 1.0;
    }
    eval_talent_mod(&talent.id, "BARRIER_MODIFIER", talent.level, cp, wp, loaded)
        .unwrap_or(1.0)
        .max(0.1)
}

/// Helper computing heal multiplier from an equipped talent.
pub(crate) fn talent_heal_multiplier(
    talent: &ActiveTalentState,
    slot: AbilitySlot,
    cp: f32,
    wp: f32,
    loaded: Option<&BTreeMap<String, LoadedTalent>>,
) -> f32 {
    let Some(spec) = find_talent(&talent.id) else {
        return 1.0;
    };
    if !spec.scope.matches(slot) && spec.scope != TalentSlotScope::Perk {
        return 1.0;
    }
    eval_talent_mod(&talent.id, "HEAL_MODIFIER", talent.level, cp, wp, loaded)
        .or_else(|| eval_talent_mod(&talent.id, "HEAL_MULTIPLIER", talent.level, cp, wp, loaded))
        .or_else(|| eval_talent_mod(&talent.id, "HEALING_MODIFIER", talent.level, cp, wp, loaded))
        .unwrap_or(1.0)
        .max(0.1)
}

/// Helper computing bonus range added to `slot`.
pub(crate) fn talent_bonus_range(
    talent: &ActiveTalentState,
    slot: AbilitySlot,
    cp: f32,
    wp: f32,
    loaded: Option<&BTreeMap<String, LoadedTalent>>,
) -> f32 {
    let Some(spec) = find_talent(&talent.id) else {
        return 0.0;
    };
    if !spec.scope.matches(slot) {
        return 0.0;
    }
    if let Some(less) = eval_talent_mod(&talent.id, "LESS_RANGE", talent.level, cp, wp, loaded) {
        return -less;
    }
    eval_talent_mod(&talent.id, "BONUS_RANGE", talent.level, cp, wp, loaded)
        .or_else(|| eval_talent_mod(&talent.id, "BONUS_SHOT_RANGE", talent.level, cp, wp, loaded))
        .or_else(|| eval_talent_mod(&talent.id, "RANGE", talent.level, cp, wp, loaded))
        .unwrap_or(0.0)
}

/// Helper computing additional charges added to `slot`.
pub(crate) fn talent_bonus_charges(
    talent: &ActiveTalentState,
    slot: AbilitySlot,
    cp: f32,
    wp: f32,
    loaded: Option<&BTreeMap<String, LoadedTalent>>,
) -> u8 {
    let Some(spec) = find_talent(&talent.id) else {
        return 0;
    };
    if !spec.scope.matches(slot) {
        return 0;
    }
    let val = eval_talent_mod(&talent.id, "CHARGES", talent.level, cp, wp, loaded)
        .or_else(|| {
            eval_talent_mod(
                &talent.id,
                "ADDITIONAL_CHARGES",
                talent.level,
                cp,
                wp,
                loaded,
            )
        })
        .unwrap_or(0.0);
    val.max(0.0).round() as u8
}

/// Compute aggregate passive stat adjustments from an equipped talent.
pub(crate) fn talent_passive_stats(
    talent: &ActiveTalentState,
    cp: f32,
    wp: f32,
    loaded: Option<&BTreeMap<String, LoadedTalent>>,
) -> TalentStatBonus {
    let mut b = TalentStatBonus::default();
    let Some(spec) = find_talent(&talent.id) else {
        return b;
    };
    let lvl = f32::from(talent.level.get().saturating_sub(1));
    match spec.special {
        TalentSpecial::BonusAttackRange => b.attack_range += 0.5 + 0.05 * lvl,
        TalentSpecial::BonusAttackSpeed => b.attack_speed += 0.1 + 0.02 * lvl,
        TalentSpecial::BonusAttackSpeedCooldown => {
            b.attack_speed += 0.075 + 0.015 * lvl;
            b.cooldown_accel += 0.075 + 0.015 * lvl;
        }
        TalentSpecial::BonusCooldown => b.cooldown_accel += 0.1 + 0.02 * lvl,
        TalentSpecial::BonusCritChance => b.crit_chance += 0.1 + 0.02 * lvl,
        TalentSpecial::BonusCritChanceDamage => {
            b.crit_chance += 0.075 + 0.015 * lvl;
            b.crit_damage += 0.1 + 0.02 * lvl;
        }
        TalentSpecial::BonusCritDamage => b.crit_damage += 0.15 + 0.03 * lvl,
        TalentSpecial::BonusCrystal => b.crystal_flat += 15.0 + 3.0 * lvl,
        TalentSpecial::BonusCrystalPierce => b.shield_pierce += 0.08 + 0.02 * lvl,
        TalentSpecial::BonusCrystalVamp => b.lifesteal += 0.08 + 0.015 * lvl,
        TalentSpecial::BonusDamageAttackSpeed => {
            b.weapon_pct += 0.05 + 0.01 * lvl;
            b.crystal_pct += 0.05 + 0.01 * lvl;
            b.attack_speed += 0.05 + 0.01 * lvl;
        }
        TalentSpecial::BonusDefense => {
            b.armor_flat += 10.0 + 3.0 * lvl;
            b.shield_flat += 10.0 + 3.0 * lvl;
        }
        TalentSpecial::BonusMovementSpeed => b.move_speed_flat += 0.25 + 0.05 * lvl,
        TalentSpecial::BonusRegen => b.hp_regen_per_sec += 5.0 + 1.5 * lvl,
        TalentSpecial::BonusWeapon => b.weapon_flat += 10.0 + 2.5 * lvl,
        TalentSpecial::BonusWeaponPierce => b.armor_pierce += 0.08 + 0.02 * lvl,
        TalentSpecial::BonusWeaponVamp => b.lifesteal += 0.08 + 0.015 * lvl,
        _ => {}
    }

    for m in spec.mods {
        let v = eval_talent_mod(&talent.id, m.label, talent.level, cp, wp, loaded)
            .unwrap_or_else(|| m.value_at(talent.level, cp, wp));
        match m.label {
            "PASSIVE_WEAPON_POWER" | "PASSIVE_WEAPON" => b.weapon_pct += v,
            "PASSIVE_CRYSTAL_POWER" | "PASSIVE_CRYSTAL" | "BONUS_CRYSTAL_POWER" => {
                b.crystal_pct += v;
            }
            "PASSIVE_ATTACK_SPEED" | "ATTACK_SPEED_BONUS" => b.attack_speed += v,
            "PASSIVE_ARMOR" => b.armor_pct += v,
            "PASSIVE_SHIELD" => b.shield_pct += v,
            "PASSIVE_MOVEMENT" => b.move_speed_pct += v,
            "PASSIVE_BONUS_ATTACK_RANGE" => b.attack_range += v,
            "PASSIVE_CRITICAL_DAMAGE" => b.crit_damage += v,
            "BONUS_ALT_ENERGY" => b.max_resource_bonus += v,
            _ => {}
        }
    }
    b
}
