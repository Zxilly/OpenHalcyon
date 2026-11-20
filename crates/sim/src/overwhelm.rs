//! Yates gathers energy before his shockwave tests enemies around his current position.
use super::*;
use crate::native_combat::{Damage, ranks};
use halcyon_data::{AbilityParameter, DamageOperand};

#[derive(Clone, Debug, Serialize)]
pub(super) struct Overwhelm {
    damage: Damage<5>,
    radius: [f32; 5],
    duration: [f32; 5],
    stun: [f32; 5],
}

#[derive(Clone, Copy, Debug, Serialize)]
pub(super) struct Charge {
    due: u64,
    radius: f32,
    stun_ticks: u64,
    damage: f32,
}

impl Overwhelm {
    pub fn load(data: &GameData) -> Result<Option<Self>, String> {
        let Some(b) = data.ability("Yates", "Ability__Yates__B") else {
            return Ok(None);
        };
        Ok(Some(Self {
            damage: Damage::named(b, DamageOperand::UppercaseDamage)?,
            radius: ranks(b, AbilityParameter::Radius)?,
            duration: ranks(b, AbilityParameter::EffectDuration)?,
            stun: ranks(b, AbilityParameter::AreaStunDuration)?,
        }))
    }
    pub fn charge(&self, r: usize, tick: u64, cp: f32) -> Charge {
        Charge {
            due: tick + (self.duration[r] * TICK_HZ as f32).round() as u64,
            radius: self.radius[r],
            stun_ticks: (self.stun[r] * TICK_HZ as f32).round() as u64,
            damage: self.damage.at(r, cp, 0.),
        }
    }
}
impl Room<'_> {
    pub(super) fn overwhelm_reaches(&self, i: usize, point: Position) -> bool {
        let e = &self.entities[i];
        let Some(r) = e.ability_ranks[AbilitySlot::B as usize].checked_sub(1) else {
            return false;
        };
        self.abilities
            .overwhelm
            .as_ref()
            .is_some_and(|rules| e.position.dist(point) <= rules.radius[usize::from(r)])
    }
    pub(super) fn advance_overwhelm(&mut self, i: usize, events: &mut Vec<Event>) {
        let source = &self.entities[i];
        let Some(charge) = source.status.captain_effects.read().overwhelm else {
            return;
        };
        if charge.due > self.tick {
            return;
        }
        let (team, src, pos, pierce) = (
            source.team,
            source.id,
            source.position,
            source.item_stats.shield_pierce,
        );
        self.entities[i].status.captain_effects.arm().overwhelm = None;
        for j in 0..self.entities.len() {
            let e = &mut self.entities[j];
            if e.team == team
                || e.hp <= 0.
                || !matches!(e.kind, Kind::Hero | Kind::Minion | Kind::Neutral)
                || e.position.dist(pos) > charge.radius
                || !self.state.layout.clear(pos, e.position)
            {
                continue;
            }
            e.status.stun_until = e.status.stun_until.max(self.state.tick + charge.stun_ticks);
            e.winding = None;
            e.cast = None;
            e.status.channel = 0;
            e.status.channel_until = 0;
            let damage = shop::mitigated(charge.damage, e.stats.shield, pierce);
            self.hurt(src, team, j, damage, DamageSchool::Crystal, events);
        }
    }
}
