use crate::Event;
use crate::ImpactEffect;
use crate::Kind;
use crate::Position;
use crate::Room;
use crate::TICK_HZ;
use crate::combat::native_combat::{Damage, ranks};
use crate::ecs::score::DamageSchool;
use halcyon_data::GameData;
use halcyon_data::{AbilityParameter as P, DamageOperand};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Stormguard {
    burn: Damage<5>,
    duration: [f32; 5],
    radius: [f32; 5],
    threshold: [f32; 5],
    range: [f32; 5],
    multiplier: [f32; 5],
    loss: [f32; 5],
}
#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) struct Bubble {
    until: u64,
    next_pulse: u64,
    damage: f32,
    radius: f32,
    threshold: f32,
    range: f32,
    multiplier: f32,
    loss: u64,
}
#[derive(Clone, Copy)]
pub(crate) struct Reflection {
    amount: f32,
    range: f32,
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Tremor {
    damage: Damage<3>,
    silence: [f32; 3],
    reveal: [f32; 3],
    pre: [f32; 3],
    post: [f32; 3],
    range: [f32; 3],
}
fn ticks(v: f32) -> u64 {
    (v * TICK_HZ as f32).ceil() as u64
}
impl Stormguard {
    pub fn load(d: &GameData) -> Result<Option<Self>, String> {
        let Some(a) = d.ability("Catherine", "Ability__Catherine__B") else {
            return Ok(None);
        };
        Ok(Some(Self {
            burn: Damage::named(a, DamageOperand::BurnDamagePerSecond)?,
            duration: ranks(a, P::BubbleDuration)?,
            radius: ranks(a, P::BurnRange)?,
            threshold: ranks(a, P::BaseHealthThreshold)?,
            range: ranks(a, P::ReflectRange)?,
            multiplier: ranks(a, P::ReflectMultiplier)?,
            loss: ranks(a, P::DurationLossPerProc)?,
        }))
    }
}
impl Tremor {
    pub fn load(d: &GameData) -> Result<Option<Self>, String> {
        let Some(a) = d.ability("Catherine", "Ability__Catherine__C") else {
            return Ok(None);
        };
        Ok(Some(Self {
            damage: Damage::named(a, DamageOperand::Damage)?,
            silence: ranks(a, P::SilenceDuration)?,
            reveal: ranks(a, P::SelfRevealDuration)?,
            pre: ranks(a, P::PreCastLock)?,
            post: ranks(a, P::PostCastLock)?,
            range: ranks(a, P::Range)?,
        }))
    }
    pub fn windup(&self, r: usize) -> u64 {
        ticks(self.pre[r])
    }
}
impl Room<'_> {
    pub(crate) fn raise_stormguard(&mut self, i: usize, r: usize) {
        let a = self.state.abilities.stormguard.as_ref().unwrap();
        let e = &mut self.entities[i];
        e.status.captain_effects.arm().stormguard = Some(Bubble {
            until: self.state.tick + ticks(a.duration[r]),
            next_pulse: self.state.tick + TICK_HZ,
            damage: a.burn.at(r, e.item_stats.crystal, 0.),
            radius: a.radius[r],
            threshold: (e.max_hp - e.bonus_hp).max(0.) * a.threshold[r],
            range: a.range[r],
            multiplier: 1. + a.multiplier[r],
            loss: ticks(a.loss[r]),
        });
    }
    pub(crate) fn advance_stormguard(&mut self, i: usize, events: &mut Vec<Event>) {
        let e = &mut self.entities[i];
        let Some(b) = e.status.captain_effects.read().stormguard else {
            return;
        };
        if self.state.tick > b.until {
            e.status.captain_effects.arm().stormguard = None;
            return;
        }
        if self.state.tick < b.next_pulse {
            return;
        }
        e.status
            .captain_effects
            .arm()
            .stormguard
            .as_mut()
            .unwrap()
            .next_pulse += TICK_HZ;
        let (src, team, pos, pierce) = (e.id, e.team, e.position, e.item_stats.shield_pierce);
        for j in 0..self.entities.len() {
            let target = &self.entities[j];
            if target.team == team
                || target.hp <= 0.
                || !matches!(target.kind, Kind::Hero | Kind::Minion | Kind::Neutral)
                || target.position.dist(pos) > b.radius
                || !self.combat_clear(&self.entities[i], target)
            {
                continue;
            }
            self.hurt(
                src,
                team,
                j,
                crate::items::shop::mitigated(b.damage, target.stats.shield, pierce),
                DamageSchool::Crystal,
                events,
            );
        }
    }
    pub(crate) fn stormguard_reflection(
        &mut self,
        i: usize,
        amount: f32,
    ) -> (f32, Option<Reflection>) {
        let e = &mut self.entities[i];
        let Some(b) = e
            .status
            .captain_effects
            .active_mut()
            .and_then(|state| state.stormguard.as_mut())
        else {
            return (amount, None);
        };
        if e.hp <= 0. || self.state.tick >= b.until || amount <= b.threshold {
            return (amount, None);
        }
        b.until = b.until.saturating_sub(b.loss);
        (
            b.threshold,
            Some(Reflection {
                amount: (amount - b.threshold) * b.multiplier,
                range: b.range,
            }),
        )
    }
    pub(crate) fn reflect_stormguard(&mut self, i: usize, r: Reflection, events: &mut Vec<Event>) {
        let e = &self.entities[i];
        let (src, team, pierce) = (e.id, e.team, e.item_stats.shield_pierce);
        let mut victims: Vec<_> = self
            .entities
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                t.team != team
                    && t.hp > 0.
                    && matches!(t.kind, Kind::Hero | Kind::Minion | Kind::Neutral)
                    && self.combat_clear(e, t)
            })
            .filter_map(|(j, t)| {
                let d = t.position.dist(e.position);
                (d <= r.range).then_some((j, d))
            })
            .collect();
        victims.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        victims.truncate(3);
        let share = r.amount / victims.len().max(1) as f32;
        for (j, _) in victims {
            let applied =
                crate::items::shop::mitigated(share, self.entities[j].stats.shield, pierce);
            // Reflected damage cannot trigger another reflection.
            self.apply_damage(
                src,
                team,
                j,
                applied,
                (DamageSchool::Crystal, ImpactEffect::Standard),
                events,
            );
        }
    }
    pub(crate) fn blast_tremor(
        &mut self,
        i: usize,
        r: usize,
        point: Position,
        events: &mut Vec<Event>,
    ) {
        let a = self.state.abilities.tremor.as_ref().unwrap();
        let e = &mut self.entities[i];
        let (src, team, pos, pierce) = (e.id, e.team, e.position, e.item_stats.shield_pierce);
        let damage = a.damage.at(r, e.item_stats.crystal, 0.);
        let range = a.range[r];
        let silence = self.state.tick + ticks(a.silence[r]);
        e.status.reveal_until = self.state.tick + ticks(a.reveal[r]);
        e.status.channel_until = self.state.tick + ticks(a.post[r]).saturating_sub(1);
        let len = pos.dist(point);
        let (dx, dy) = ((point.x - pos.x) / len, (point.y - pos.y) / len);
        for j in 0..self.entities.len() {
            let t = &self.entities[j];
            let d = pos.dist(t.position);
            // The localized description specifies a cone. Its 90-degree opening is a host rule.
            if t.team == team
                || t.hp <= 0.
                || !matches!(t.kind, Kind::Hero | Kind::Minion | Kind::Neutral)
                || d > range
                || (t.position.x - pos.x) * dx + (t.position.y - pos.y) * dy
                    < d * std::f32::consts::FRAC_1_SQRT_2
                || !self.combat_clear(&self.entities[i], t)
            {
                continue;
            }
            let t = &mut self.entities[j];
            if !t.status.blocks_debuffs(self.state.tick) {
                t.status.silence_until = t.status.silence_until.max(silence);
                t.cast = None;
                t.status.channel = 0;
                t.status.channel_until = 0;
            }
            let applied = crate::items::shop::mitigated(damage, t.stats.shield, pierce);
            self.hurt(src, team, j, applied, DamageSchool::Crystal, events);
        }
    }
}
