use std::collections::BTreeMap;

use halcyon_data::GameData;
use serde::Serialize;

use crate::heroes::talents::{LoadedTalent, load_talents_from_data};

/// Data-bound rank + scaling formula with compiled fallback from `abilities.json`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ValueSpec {
    pub var_name: Option<&'static str>,
    pub dmg_index: Option<u8>,
    pub ranks: [f32; 5],
    pub cp_ratio: f32,
    pub wp_ratio: f32,
    pub bonus_wp_ratio: f32,
    pub bonus_hp_ratio: f32,
    pub alt_ratio: f32,
    pub bonus_as_ratio: f32,
    pub clamp_min: f32,
    pub clamp_max: f32,
}

#[allow(dead_code)]
impl ValueSpec {
    pub(crate) const fn zero() -> Self {
        Self {
            var_name: None,
            dmg_index: None,
            ranks: [0.0; 5],
            cp_ratio: 0.0,
            wp_ratio: 0.0,
            bonus_wp_ratio: 0.0,
            bonus_hp_ratio: 0.0,
            alt_ratio: 0.0,
            bonus_as_ratio: 0.0,
            clamp_min: 0.0,
            clamp_max: 0.0,
        }
    }

    pub(crate) const fn constant(v: f32) -> Self {
        Self {
            ranks: [v; 5],
            ..Self::zero()
        }
    }

    pub(crate) const fn ranks(ranks: [f32; 5]) -> Self {
        Self {
            ranks,
            ..Self::zero()
        }
    }

    pub(crate) const fn ranks3(r: [f32; 3]) -> Self {
        Self::ranks([r[0], r[1], r[2], r[2], r[2]])
    }

    pub(crate) const fn var(var_name: &'static str, ranks: [f32; 5]) -> Self {
        Self {
            var_name: Some(var_name),
            ranks,
            ..Self::zero()
        }
    }

    pub(crate) const fn var3(var_name: &'static str, r: [f32; 3]) -> Self {
        Self::var(var_name, [r[0], r[1], r[2], r[2], r[2]])
    }

    pub(crate) const fn var_scaled(
        var_name: &'static str,
        ranks: [f32; 5],
        cp_ratio: f32,
        wp_ratio: f32,
        bonus_wp_ratio: f32,
        bonus_hp_ratio: f32,
    ) -> Self {
        Self {
            var_name: Some(var_name),
            ranks,
            cp_ratio,
            wp_ratio,
            bonus_wp_ratio,
            bonus_hp_ratio,
            ..Self::zero()
        }
    }

    pub(crate) const fn dmg(
        dmg_index: u8,
        ranks: [f32; 5],
        cp_ratio: f32,
        wp_ratio: f32,
        bonus_wp_ratio: f32,
    ) -> Self {
        Self {
            dmg_index: Some(dmg_index),
            ranks,
            cp_ratio,
            wp_ratio,
            bonus_wp_ratio,
            ..Self::zero()
        }
    }

    pub(crate) const fn dmg3(
        dmg_index: u8,
        r: [f32; 3],
        cp_ratio: f32,
        wp_ratio: f32,
        bonus_wp_ratio: f32,
    ) -> Self {
        Self::dmg(
            dmg_index,
            [r[0], r[1], r[2], r[2], r[2]],
            cp_ratio,
            wp_ratio,
            bonus_wp_ratio,
        )
    }

    pub(crate) const fn var3_scaled(var_name: &'static str, r: [f32; 3], cp_ratio: f32) -> Self {
        Self::var_scaled(
            var_name,
            [r[0], r[1], r[2], r[2], r[2]],
            cp_ratio,
            0.0,
            0.0,
            0.0,
        )
    }

    pub(crate) const fn with_wp(mut self, wp_ratio: f32) -> Self {
        self.wp_ratio = wp_ratio;
        self
    }

    pub(crate) fn eval_fallback(self, rank: u8, inputs: EvalInputs) -> f32 {
        let idx = usize::from(rank.saturating_sub(1)).min(4);
        let raw = self.ranks[idx]
            + self.cp_ratio * inputs.cp
            + self.wp_ratio * inputs.wp
            + self.bonus_wp_ratio * inputs.bonus_wp
            + self.bonus_hp_ratio * inputs.bonus_hp
            + self.alt_ratio * inputs.alt
            + self.bonus_as_ratio * inputs.bonus_as.max(0.0);
        let min_clamped = if self.clamp_min > 0.0 {
            raw.max(self.clamp_min)
        } else {
            raw
        };
        if self.clamp_max > 0.0 {
            min_clamped.min(self.clamp_max)
        } else {
            min_clamped
        }
    }
}

/// Stat snapshot used to evaluate `ValueSpec` and `GameData` formulas deterministically.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct EvalInputs {
    pub cp: f32,
    pub wp: f32,
    pub bonus_wp: f32,
    pub bonus_hp: f32,
    pub alt: f32,
    pub bonus_as: f32,
}

/// Loaded `GameData` variable formula matching `DATA_SPEC.md`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct LoadedVarFormula {
    pub base: f32,
    pub per_rank: f32,
    pub overdrive: f32,
    pub cp_ratio: f32,
    pub wp_ratio: f32,
    pub alt_ratio: f32,
    pub bonus_as_ratio: f32,
    pub clamp: f32,
    pub ranks: Vec<f32>,
}

impl LoadedVarFormula {
    pub(crate) fn eval(
        &self,
        rank: u8,
        max_rank: u8,
        inputs: EvalInputs,
        use_bonus_wp: bool,
    ) -> f32 {
        let idx = usize::from(rank.saturating_sub(1));
        let base_val = self.ranks.get(idx).copied().unwrap_or_else(|| {
            let od = if max_rank == 5 && rank == 5 {
                self.overdrive
            } else {
                0.0
            };
            self.base + f32::from(rank.saturating_sub(1)) * self.per_rank + od
        });
        let wp_term = if use_bonus_wp {
            self.wp_ratio * inputs.bonus_wp
        } else {
            self.wp_ratio * inputs.wp
        };
        let v = base_val
            + self.cp_ratio * inputs.cp
            + wp_term
            + self.alt_ratio * inputs.alt
            + self.bonus_as_ratio * inputs.bonus_as.max(0.0);
        if self.clamp > 0.0 {
            if self.per_rank < 0.0 || self.clamp < self.base {
                v.max(self.clamp)
            } else {
                v.min(self.clamp)
            }
        } else {
            v
        }
    }
}

/// Loaded `GameData` damage variable formula.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct LoadedDmgFormula {
    pub header_u32: u32,
    pub tooltip_operand: Option<String>,
    pub base: f32,
    pub per_rank: f32,
    pub overdrive: f32,
    pub cp_ratio: f32,
    pub wp_ratio: f32,
    pub f2: f32,
    pub f6: f32,
    pub f7: f32,
    pub ranks: Vec<f32>,
}

impl LoadedDmgFormula {
    pub(crate) fn eval(
        &self,
        rank: u8,
        max_rank: u8,
        inputs: EvalInputs,
        use_bonus_wp: bool,
    ) -> f32 {
        let idx = usize::from(rank.saturating_sub(1));
        let base_val = self.ranks.get(idx).copied().unwrap_or_else(|| {
            let od = if max_rank == 5 && rank == 5 {
                self.overdrive
            } else {
                0.0
            };
            self.base + f32::from(rank.saturating_sub(1)) * self.per_rank + od
        });
        let wp_coeff = if self.wp_ratio != 0.0 {
            self.wp_ratio
        } else {
            self.f6
        };
        let wp_term = if use_bonus_wp && self.f6 == 0.0 {
            wp_coeff * inputs.bonus_wp
        } else {
            wp_coeff * inputs.wp
        };
        base_val
            + self.cp_ratio * inputs.cp
            + wp_term
            + self.f2 * inputs.alt
            + self.f7 * inputs.bonus_hp
    }
}

/// Loaded ability record from `abilities.json`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct LoadedAbility {
    pub max_rank: u8,
    pub vars: BTreeMap<String, LoadedVarFormula>,
    pub dmg_vars: Vec<LoadedDmgFormula>,
}

/// Loaded hero kit parameters from `abilities.json`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct LoadedHero {
    pub hero_id: String,
    pub type_id: u16,
    pub abilities: BTreeMap<String, LoadedAbility>,
}

/// Deterministic data-driven book attached optionally to `AbilityBook::hero_kits`.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub(crate) struct HeroKitBook {
    pub heroes: BTreeMap<String, LoadedHero>,
    pub by_type_id: BTreeMap<u16, String>,
    pub talents: BTreeMap<String, LoadedTalent>,
}

impl HeroKitBook {
    pub(crate) fn load(data: &GameData) -> Result<Option<Self>, String> {
        let content = data.extra.combat_content();
        if content.heroes().len() == 0 && content.talents().len() == 0 {
            return Ok(None);
        }
        let mut book = Self::default();
        let unit_type_ids: BTreeMap<String, u16> = content
            .units()
            .filter_map(|u| {
                u.entity_type_id
                    .map(|tid| (u.name.to_ascii_lowercase(), tid))
            })
            .collect();

        for hero in content.heroes() {
            let tid = crate::heroes::kits::kit_spec_by_hero_id(&hero.hero_id)
                .map(|s| s.type_id)
                .or_else(|| data.type_id(&hero.hero_id))
                .or_else(|| {
                    unit_type_ids
                        .get(&hero.hero_id.to_ascii_lowercase())
                        .copied()
                })
                .unwrap_or(0);
            let mut abilities = BTreeMap::new();
            for ab in &hero.abilities {
                let mut vars = BTreeMap::new();
                for v in &ab.variables {
                    vars.insert(
                        v.name.clone(),
                        LoadedVarFormula {
                            base: v.base,
                            per_rank: v.per_rank,
                            overdrive: v.overdrive,
                            cp_ratio: v.cp_ratio,
                            wp_ratio: v.wp_ratio,
                            alt_ratio: v.alt_ratio,
                            bonus_as_ratio: v.bonus_as_ratio,
                            clamp: v.clamp,
                            ranks: v.ranks.clone().unwrap_or_default(),
                        },
                    );
                }
                let dmg_vars = ab
                    .damage_variables
                    .iter()
                    .map(|d| LoadedDmgFormula {
                        header_u32: d.header_u32,
                        tooltip_operand: d.tooltip_operand.clone(),
                        base: d.base,
                        per_rank: d.per_rank,
                        overdrive: d.overdrive,
                        cp_ratio: d.cp_ratio,
                        wp_ratio: d.wp_ratio,
                        f2: d.f2,
                        f6: d.f6,
                        f7: d.f7,
                        ranks: d.ranks.clone(),
                    })
                    .collect();
                abilities.insert(
                    ab.ability_id.clone(),
                    LoadedAbility {
                        max_rank: (ab.max_rank_guess.max(1) as u8).min(5),
                        vars,
                        dmg_vars,
                    },
                );
            }
            if tid != 0 {
                book.by_type_id.insert(tid, hero.hero_id.clone());
            }
            book.heroes.insert(
                hero.hero_id.clone(),
                LoadedHero {
                    hero_id: hero.hero_id.clone(),
                    type_id: tid,
                    abilities,
                },
            );
        }
        book.talents = load_talents_from_data(data);
        Ok(Some(book))
    }

    #[allow(dead_code)]
    pub(crate) fn builtin() -> Self {
        Self::default()
    }

    pub(crate) fn hero_by_type(&self, type_id: u16) -> Option<&LoadedHero> {
        if let Some(id) = self.by_type_id.get(&type_id)
            && let Some(h) = self.heroes.get(id)
        {
            return Some(h);
        }
        let spec = crate::heroes::kits::kit_spec(type_id)?;
        self.heroes.get(spec.hero_id)
    }

    pub(crate) fn eval_value(
        &self,
        type_id: u16,
        ability_id: &str,
        rank: u8,
        fallback: ValueSpec,
        inputs: EvalInputs,
    ) -> f32 {
        if let Some(hero) = self.hero_by_type(type_id)
            && let Some(ab) = hero.abilities.get(ability_id)
        {
            if let Some(var_name) = fallback.var_name
                && let Some(v) = ab.vars.get(var_name).or_else(|| {
                    ab.vars
                        .iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case(var_name))
                        .map(|(_, v)| v)
                })
            {
                let use_bwp = fallback.bonus_wp_ratio > 0.0 && fallback.wp_ratio == 0.0;
                let mut val = v.eval(rank, ab.max_rank, inputs, use_bwp);
                if v.cp_ratio == 0.0 && fallback.cp_ratio != 0.0 {
                    val += fallback.cp_ratio * inputs.cp;
                }
                if fallback.bonus_hp_ratio != 0.0 {
                    val += fallback.bonus_hp_ratio * inputs.bonus_hp;
                }
                return val;
            }
            if let Some(dmg_idx) = fallback.dmg_index
                && let Some(d) = ab.dmg_vars.get(usize::from(dmg_idx))
            {
                let use_bwp = fallback.bonus_wp_ratio > 0.0 && fallback.wp_ratio == 0.0;
                return d.eval(rank, ab.max_rank, inputs, use_bwp);
            }
        }
        fallback.eval_fallback(rank, inputs)
    }
}
