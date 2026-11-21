//! Catherine's self-buff resets her basic attack timer. Only a released attack consumes it.
use crate::AbilitySlot;
use crate::Event;
use crate::Position;
use crate::Room;
use crate::TICK_HZ;
use crate::combat::native_combat::{Damage, ranks};
use crate::ecs::score::DamageSchool;
use halcyon_data::GameData;
use halcyon_data::{AbilityParameter, DamageOperand};
use serde::Serialize;
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Pursuit {
    damage: Damage<5>,
    range: [f32; 5],
    speed: [f32; 5],
    duration: [f32; 5],
    window: [f32; 5],
    stun: [f32; 5],
    lock: [f32; 5],
}
#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct PursuitAttack {
    pub range: f32,
    pub speed: f32,
    pub speed_until: u64,
    pub armed_until: u64,
    stun: u64,
    lock: u64,
    damage: f32,
}
impl Pursuit {
    pub fn load(d: &GameData) -> Result<Option<Self>, String> {
        let Some(a) = d.ability("Catherine", "Ability__Catherine__A") else {
            return Ok(None);
        };
        Ok(Some(Self {
            damage: Damage::named(a, DamageOperand::Damage)?,
            range: ranks(a, AbilityParameter::BonusBasicAttackRange)?,
            speed: ranks(a, AbilityParameter::SpeedBoostStrength)?,
            duration: ranks(a, AbilityParameter::SpeedBoostDuration)?,
            window: ranks(a, AbilityParameter::StunWindow)?,
            stun: ranks(a, AbilityParameter::StunDuration)?,
            lock: ranks(a, AbilityParameter::PostStunAttackLock)?,
        }))
    }
    pub fn arm(&self, r: usize, tick: u64, cp: f32) -> PursuitAttack {
        let ticks = |v: f32| (v * TICK_HZ as f32).round() as u64;
        PursuitAttack {
            range: self.range[r],
            speed: self.speed[r],
            speed_until: tick + ticks(self.duration[r]),
            armed_until: tick + ticks(self.window[r]),
            stun: ticks(self.stun[r]),
            lock: ticks(self.lock[r]),
            damage: self.damage.at(r, cp, 0.),
        }
    }
}
impl Room<'_> {
    pub(crate) fn pursuit_reaches(&self, i: usize, target: Position) -> bool {
        let e = &self.entities[i];
        let Some(r) = e.ability_ranks[AbilitySlot::A as usize].checked_sub(1) else {
            return false;
        };
        self.abilities.pursuit.as_ref().is_some_and(|a| {
            let r = usize::from(r);
            e.position.dist(target)
                <= e.stats.range + a.range[r] + a.duration[r] * (e.stats.speed + a.speed[r])
        })
    }
    pub(crate) fn pursuit_impact(
        &mut self,
        i: usize,
        j: usize,
        p: PursuitAttack,
        events: &mut Vec<Event>,
    ) {
        let e = &mut self.entities[i];
        e.status
            .captain_effects
            .arm()
            .pursuit
            .as_mut()
            .unwrap()
            .armed_until = 0;
        e.next_attack = e.next_attack.max(self.state.tick + p.lock);
        let (src, team, pierce) = (e.id, e.team, e.item_stats.shield_pierce);
        let t = &mut self.entities[j];
        if t.hp <= 0. {
            return;
        }
        if !t.status.blocks_movement_impairment(self.state.tick) {
            t.status.stun_until = t.status.stun_until.max(self.state.tick + p.stun);
            t.winding = None;
            t.cast = None;
            t.status.channel = 0;
            t.status.channel_until = 0;
        }
        let amount = crate::items::shop::mitigated(p.damage, t.stats.shield, pierce);
        self.hurt(src, team, j, amount, DamageSchool::Crystal, events);
    }
}
