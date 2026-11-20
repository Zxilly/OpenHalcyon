use super::*;
use crate::native_combat::{Damage, ranks};
use halcyon_data::{AbilityParameter as P, DamageOperand};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize)]
pub(super) struct Rules {
    damage: Damage<5>,
    startup: [f32; 5],
    radius: [f32; 5],
    duration: [f32; 5],
    slow: [f32; 5],
    linger: [f32; 5],
    immunity: [f32; 5],
}
#[derive(bevy_ecs::prelude::Component, Clone, Debug, Serialize)]
pub(super) struct Field {
    origin: Position,
    mobile: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    anchor: Option<MobileAnchor>,
    source: u32,
    team: u8,
    end: u64,
    radius: f32,
    damage: f32,
    pierce: f32,
    slow: f32,
    linger: u64,
    immunity: u64,
    targets: BTreeMap<u32, Contact>,
}
#[derive(Clone, Copy, Debug, Serialize)]
struct MobileAnchor {
    id: u32,
    start: u64,
    heading: f32,
}
const ANCHOR_DELAY: u64 = 5;
#[derive(Default, Clone, Debug, Serialize)]
struct Contact {
    inside: bool,
    ready: u64,
}
fn ticks(v: f32) -> u64 {
    (v * TICK_HZ as f32).round() as u64
}
impl Rules {
    pub fn load(d: &GameData) -> Result<Option<Self>, String> {
        let Some(a) = d.ability("Lyra", "Ability__Lyra__B") else {
            return Ok(None);
        };
        Ok(Some(Self {
            damage: Damage::named(a, DamageOperand::Damage)?,
            startup: ranks(a, P::Startup)?,
            radius: ranks(a, P::Radius)?,
            duration: ranks(a, P::Duration)?,
            slow: ranks(a, P::NonDecayingSlowAmount)?,
            linger: ranks(a, P::BulwarkSlowDuration)?,
            immunity: ranks(a, P::ImmunityDuration)?,
        }))
    }
    pub fn windup(&self, r: usize) -> u64 {
        ticks(self.startup[r])
    }
}
impl Field {
    fn sample(&mut self, w: &mut Room<'_>, center: Position, events: &mut Vec<Event>) {
        for j in 0..w.entities.len() {
            let t = &w.entities[j];
            if t.hp <= 0.
                || self.team == t.team
                || !matches!(t.kind, Kind::Hero | Kind::Minion | Kind::Neutral)
            {
                continue;
            }
            let inside = t.position.dist(center) <= self.radius;
            if !inside && !self.targets.contains_key(&t.id) {
                continue;
            }
            let state = self.targets.entry(t.id).or_default();
            if !w.layout.clear(center, t.position) {
                state.inside = false;
                continue;
            }
            let crossed = inside != state.inside;
            state.inside = inside;
            let hit = crossed && w.tick >= state.ready;
            let target = &mut w.entities[j];
            if (inside || hit) && !target.status.blocks_movement_impairment(w.state.tick) {
                let until = w.state.tick + self.linger;
                let previous = if target.status.bulwark_slow_until > w.state.tick {
                    target.status.bulwark_slow_factor
                } else {
                    1.
                };
                target.status.bulwark_slow_until = target.status.bulwark_slow_until.max(until);
                target.status.bulwark_slow_factor = previous.min(self.slow);
            }
            if hit {
                state.ready = w.state.tick + self.immunity;
                let amount = shop::mitigated(self.damage, target.stats.shield, self.pierce);
                w.hurt(
                    self.source,
                    self.team,
                    j,
                    amount,
                    DamageSchool::Crystal,
                    events,
                );
            }
        }
    }
}
impl crate::persistent_spells::TimedSpell for Field {
    fn expired_before_step(&self, tick: u64) -> bool {
        self.anchor.is_none() && tick >= self.end
    }
    fn begin_impact(&mut self) -> Self {
        // Geometry remains visible to leap queries. Contact history is private to
        // the boundary sampler and transfers in O(1), even for large waves.
        let targets = std::mem::take(&mut self.targets);
        Self {
            targets,
            ..self.clone()
        }
    }
    fn advance(&mut self, w: &mut Room<'_>, events: &mut Vec<Event>) -> bool {
        let Some(i) = w.index(self.source) else {
            return false;
        };
        let visual_end = self.end
            + if self.anchor.is_some() {
                ANCHOR_DELAY
            } else {
                0
            };
        if w.entities[i].hp <= 0. || w.tick >= visual_end {
            if let Some(anchor) = self.anchor
                && w.tick > anchor.start
            {
                events.push(Event::MobileBulwarkEnd {
                    source: self.source,
                    anchor: anchor.id,
                    position: w.entities[i].position,
                });
            }
            return false;
        }
        let center = if self.mobile {
            w.entities[i].position
        } else {
            self.origin
        };
        if let Some(anchor) = self.anchor
            && w.tick == anchor.start
        {
            events.push(Event::MobileBulwarkStart {
                source: self.source,
                anchor: anchor.id,
                position: center,
                heading: anchor.heading,
            });
        }
        if w.tick < self.end {
            self.sample(w, center, events);
        }
        true
    }
}
impl Room<'_> {
    pub(super) fn bulwark_reaches(&self, i: usize, p: Position) -> bool {
        let e = &self.entities[i];
        let Some(r) = e.ability_ranks[AbilitySlot::B as usize].checked_sub(1) else {
            return false;
        };
        self.abilities
            .bulwark
            .as_ref()
            .is_some_and(|b| e.position.dist(p) <= b.radius[usize::from(r)])
    }
    pub(super) fn create_bulwark(&mut self, i: usize, r: usize, events: &mut Vec<Event>) {
        let a = self.abilities.bulwark.as_ref().unwrap();
        let e = &self.entities[i];
        let pos = e.position;
        let mut field = Field {
            origin: pos,
            mobile: e.mobile_bulwark_bonus.is_some(),
            anchor: None,
            source: e.id,
            team: e.team,
            end: self.tick + ticks(a.duration[r] + e.mobile_bulwark_bonus.unwrap_or(0.)),
            radius: a.radius[r],
            damage: a.damage.at(r, e.item_stats.crystal, 0.),
            pierce: e.item_stats.shield_pierce,
            slow: 1. - a.slow[r],
            linger: ticks(a.linger[r]),
            immunity: ticks(a.immunity[r]),
            targets: BTreeMap::new(),
        };
        if matches!(self.layout, Layout::Aral { .. })
            && a.duration[r] == 3.0
            && a.radius[r] == 5.5
            && e.mobile_bulwark_bonus == Some(5.0)
            && let Some(heading) = e.status.motion_heading
        {
            let id = self.next_id;
            self.next_id += 4;
            field.anchor = Some(MobileAnchor {
                id,
                start: self.tick + ANCHOR_DELAY,
                heading,
            });
        }
        field.sample(self, pos, events);
        self.entities.spawn_spell(field);
    }
    pub(super) fn bulwark_blocks_leap(&self, i: usize, destination: Position) -> bool {
        if self.entities[i].status.blocks_debuffs(self.tick) {
            return false;
        }
        let e = &self.entities[i];
        self.entities.spells::<Field>().any(|f| {
            if f.team == e.team || self.tick >= f.end {
                return false;
            }
            self.index(f.source).is_some_and(|s| {
                let c = &self.entities[s];
                c.hp > 0.
                    && combat::line_distance(
                        if f.mobile { c.position } else { f.origin },
                        e.position,
                        destination,
                    ) <= f.radius
            })
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    #[test]
    #[ignore = "requires original delivery and supplemental combat data"]
    fn only_level_ten_mobile_field_announces_one_timed_anchor() {
        let data = GameData::load_with_extra(
            std::env::var_os("HALCYON_DELIVERY").unwrap(),
            Some(Path::new(&std::env::var_os("HALCYON_DATA_EXTRA").unwrap())),
        )
        .unwrap();
        let mut w = World::from_data(42, 6, &data).unwrap();
        w.use_six_human_roster().unwrap();
        w.select_hero(0, "Lyra", &data).unwrap();
        w.use_aral_lane().unwrap();
        let mut events = vec![];
        w.entities[0].status.motion_heading = Some(0.3);
        w.create_bulwark(0, 0, &mut events);
        w.entities[0].mobile_bulwark_bonus = Some(5.0);
        w.create_bulwark(0, 0, &mut events);
        let mut opening = None;
        let mut ending = None;
        for t in 1..=170 {
            w.tick = t;
            w.advance_spell_family::<Field>(&mut events);
            for ev in events.drain(..) {
                match ev {
                    Event::MobileBulwarkStart {
                        source: 1,
                        anchor,
                        heading,
                        ..
                    } => {
                        assert_eq!(heading, 0.3);
                        assert!(opening.replace((anchor, t)).is_none());
                    }
                    Event::MobileBulwarkEnd {
                        source: 1, anchor, ..
                    } => {
                        assert!(ending.replace((anchor, t)).is_none());
                    }
                    _ => {}
                }
            }
        }
        let (id, start) = opening.unwrap();
        assert_eq!(start, 5);
        assert_eq!(ending, Some((id, start + 160)));
    }
    #[test]
    #[ignore = "requires original delivery and supplemental combat data"]
    fn start_boundary_immunity_fixed_center_and_leap_gate_share_three_second_clock() {
        let d = GameData::load_with_extra(
            std::env::var_os("HALCYON_DELIVERY").unwrap(),
            Some(Path::new(&std::env::var_os("HALCYON_DATA_EXTRA").unwrap())),
        )
        .unwrap();
        let mut w = World::from_data(42, 6, &d).unwrap();
        w.use_six_human_roster().unwrap();
        w.select_hero(0, "Lyra", &d).unwrap();
        w.select_hero(1, "Yates", &d).unwrap();
        w.use_aral_lane().unwrap();
        for idx in 0..w.entities.len().min(6) {
            let e = &mut w.entities[idx];
            e.position = Position { x: 25., y: 0. };
            e.energy = 500.;
            e.skill_points = 3;
            e.level = 6;
            e.stats.shield = 0.;
        }
        w.entities[0].position = Position::default();
        w.entities[0].item_stats.crystal = 40.;
        w.entities[1].position.x = 5.5;
        w.entities[2].position.x = 1.;
        w.entities[3].position.x = 5.51;
        w.command(1, Command::Learn(AbilitySlot::B)).unwrap();
        w.command(2, Command::Learn(AbilitySlot::C)).unwrap();
        let cmd = Command::Cast {
            slot: AbilitySlot::B,
            target: CastTarget::SelfCast,
        };
        let leap = Command::Cast {
            slot: AbilitySlot::C,
            target: CastTarget::Point(Position { x: -9., y: 0. }),
        };
        assert_eq!(w.decide(0), cmd);
        w.command(1, cmd).unwrap();
        let mut out = Vec::new();
        let hp = w.entities[1].hp;
        let ally = w.entities[2].hp;
        let outside = w.entities[3].hp;
        w.tick = 7;
        w.advance_cast(0, &mut out);
        assert_eq!(w.entities[1].hp, hp);
        assert_eq!(w.entities[0].energy, 500.);
        w.tick = 8;
        w.advance_cast(0, &mut out);
        assert_eq!(w.entities[0].energy, 440.);
        assert_eq!(w.entities[1].hp, hp - 92.);
        assert_eq!(w.entities[2].hp, ally);
        assert_eq!(w.entities[3].hp, outside);
        assert_eq!(w.entities[1].status.bulwark_slow_factor, 0.7);
        assert_eq!(w.entities[1].status.bulwark_slow_until, 38);
        assert!(w.command(2, leap).is_err());
        w.entities[1].position.x = 8.;
        w.tick = 9;
        w.advance_spell_family::<Field>(&mut out);
        assert_eq!(w.entities[1].hp, hp - 92.);
        assert!(w.command(2, leap).is_err());
        w.entities[0].position.x = 3.;
        w.tick = 57;
        w.advance_spell_family::<Field>(&mut out);
        assert_eq!(w.entities[3].hp, outside);
        w.entities[1].position.x = 5.5;
        w.tick = 58;
        w.advance_spell_family::<Field>(&mut out);
        assert_eq!(w.entities[1].hp, hp - 184.);
        assert_eq!(w.entities[3].hp, outside);
        w.tick = 68;
        w.advance_spell_family::<Field>(&mut out);
        assert!(w.entities.spells::<Field>().next().is_none());
        w.command(2, leap).unwrap();
        // The named, home-authorized level ten extends three seconds to eight.
        w.set_named_talent(
            1,
            Some((
                "Talent_Lyra_MobileBulwark",
                std::num::NonZeroU8::new(10).unwrap(),
            )),
            &d,
        )
        .unwrap();
        w.entities[0].cooldown[1] = 0;
        w.entities[0].position = Position::default();
        w.entities[3].position.x = 12.;
        w.command(1, cmd).unwrap();
        w.tick = 76;
        w.advance_cast(0, &mut out);
        assert_eq!(w.entities.spells::<Field>().next().unwrap().end, 236);
        w.entities[0].position.x = 12.;
        w.tick = 77;
        w.advance_spell_family::<Field>(&mut out);
        assert_eq!(w.entities[3].hp, outside - 92.);
        w.set_named_talent(1, None, &d).unwrap();
        assert!(w.entities[0].mobile_bulwark_bonus.is_none());
    }
}
