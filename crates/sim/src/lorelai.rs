//! Lorelai's native point and allied-hero casts. See lorelai.md for evidence and tuning.
use super::*;

pub(super) const HERO: u16 = EntityTemplate::Lorelai.as_raw();
const DURATION: u64 = 8 * TICK_HZ;

#[derive(Clone, Copy, Debug, Serialize)]
enum Pool {
    Fish {
        rank: usize,
        stun: u64,
    },
    Splash {
        slow_factor: f32,
        speed: Option<f32>,
    },
}

impl Pool {
    fn radius(self) -> f32 {
        match self {
            Self::Fish { .. } => 2.,
            Self::Splash { .. } => 3.,
        }
    }
}

#[derive(bevy_ecs::prelude::Component, Clone, Copy, Debug, Serialize)]
pub(super) struct Water {
    owner: u32,
    team: u8,
    position: Position,
    // Waterwall follows its recipient until the protective veil drops.
    follows: Option<u32>,
    start: u64,
    pool: Pool,
}

impl World {
    pub(super) fn lorelai_action(&self, i: usize, enemy: Position) -> Option<Command> {
        let e = &self.entities[i];
        let ally = self
            .entities
            .iter()
            .filter(|a| {
                a.kind == Kind::Hero
                    && a.team == e.team
                    && a.hp > 0.
                    && a.hp < 0.45 * a.max_hp
                    && self.can_cast(i, AbilitySlot::C, CastTarget::Entity(a.id))
            })
            .min_by(|a, b| (a.hp / a.max_hp).total_cmp(&(b.hp / b.max_hp)));
        if let Some(a) = ally {
            return Some(Command::Cast {
                slot: AbilitySlot::C,
                target: CastTarget::Entity(a.id),
            });
        }
        [AbilitySlot::A, AbilitySlot::B]
            .into_iter()
            .find(|&slot| self.can_cast(i, slot, CastTarget::Point(enemy)))
            .map(|slot| Command::Cast {
                slot,
                target: CastTarget::Point(enemy),
            })
    }

    pub(super) fn cast_water(
        &mut self,
        caster: usize,
        slot: AbilitySlot,
        target: CastTarget,
        rank: usize,
        events: &mut Vec<Event>,
    ) {
        let e = &self.entities[caster];
        let owner = e.id;
        let team = e.team;
        let bonus_hp = e.bonus_hp.max(0.);
        let center = e.position;
        let tick = self.tick;
        let empowered = self
            .entities
            .remove_spell::<Water>(|w| {
                w.owner == owner
                    && w.start <= tick
                    && tick < w.start + DURATION
                    && center.dist(w.position) <= w.pool.radius()
            })
            .is_some();
        let e = &self.entities[caster];
        let rules = self.abilities.lorelai.as_ref();
        let splash_rank = if slot == AbilitySlot::B {
            rank
        } else {
            e.ability_ranks[1].saturating_sub(1) as usize
        };
        let skip_pool = rules.is_some() && slot == AbilitySlot::C && e.ability_ranks[1] == 0;
        let crystal = e.item_stats.crystal;
        let splash = Pool::Splash {
            speed: rules.map(|r| r.speed[splash_rank]),
            slow_factor: 1.
                - rules.map_or_else(
                    || (0.25 + 0.00017 * bonus_hp).min(0.5),
                    |r| {
                        (r.slow[splash_rank] + r.hp_to_slow[splash_rank] * bonus_hp)
                            .min(r.slow_cap[splash_rank])
                    },
                ),
        };
        let (position, follows, delay, pool) = match (slot, target) {
            (AbilitySlot::A, CastTarget::Point(p)) => {
                let stun_seconds = rules.map_or_else(
                    || (0.5 + 0.0003 * bonus_hp).min(0.95) + if empowered { 0.35 } else { 0. },
                    |r| {
                        (r.stun[rank] + r.hp_to_stun[rank] * bonus_hp).min(r.stun_cap[rank])
                            * if empowered {
                                r.empowered_stun[rank]
                            } else {
                                1.
                            }
                    },
                );
                (
                    p,
                    None,
                    rules.map_or(14, |r| (r.snap_delay[rank] * TICK_HZ as f32).round() as u64),
                    Pool::Fish {
                        rank,
                        stun: (stun_seconds * TICK_HZ as f32).round() as u64,
                    },
                )
            }
            (AbilitySlot::B, CastTarget::Point(p)) => (
                p,
                None,
                rules.map_or_else(
                    || if empowered { 2 } else { 6 },
                    |r| {
                        ((if empowered {
                            r.empowered_delay[rank]
                        } else {
                            r.splash_delay[rank]
                        }) * TICK_HZ as f32)
                            .round() as u64
                    },
                ),
                splash,
            ),
            (AbilitySlot::C, CastTarget::Entity(id)) => {
                let i = self.index(id).expect("validated Waterwall target");
                let ally = &mut self.entities[i];
                ally.status.stun_until = 0;
                ally.status.root_until = 0;
                ally.status.silence_until = 0;
                ally.status.bulwark_slow_until = 0;
                ally.status.slow_until = 0;
                ally.status.water_slow_until = 0;
                let strength = rules.map_or_else(
                    || {
                        (500. + rank as f32 * 250. + 0.15 * bonus_hp)
                            * if empowered { 1.25 } else { 1. }
                    },
                    |r| {
                        let (base, ratio) = if empowered {
                            (r.empowered_barrier[rank], r.crystal_to_empowered_barrier)
                        } else {
                            (r.barrier[rank], r.crystal_to_barrier)
                        };
                        base + ratio * crystal + r.hp_to_barrier[rank] * bonus_hp
                    },
                );
                // Concurrent veils refresh duration and preserve the larger remaining barrier.
                if ally.status.barrier_until <= self.tick {
                    ally.status.barrier = 0.;
                }
                if !empowered && id != owner && ally.status.barrier <= 0. {
                    let instance = self.next_id;
                    self.next_id += 1;
                    events.push(Event::WaterwallApplied {
                        source: owner,
                        target: id,
                        id: instance,
                    });
                }
                ally.status.barrier = ally.status.barrier.max(strength);
                let duration = rules.map_or_else(
                    || if empowered { 6. } else { 4. },
                    |r| {
                        if empowered {
                            r.empowered_duration[rank]
                        } else {
                            r.duration[rank]
                        }
                    },
                );
                let duration = (duration * TICK_HZ as f32).round() as u64;
                ally.status.barrier_until = self.tick + duration;
                (ally.position, Some(id), duration, splash)
            }
            _ => unreachable!("validated Lorelai cast"),
        };
        if skip_pool {
            return;
        }
        self.entities.spawn_spell(Water {
            owner,
            team,
            position,
            follows,
            start: self.tick + delay,
            pool,
        });
    }

    pub(super) fn advance_water(&mut self, events: &mut Vec<Event>) {
        self.advance_spell_family::<Water>(events);
    }
}

impl crate::persistent_spells::TimedSpell for Water {
    fn expired_before_step(&self, tick: u64) -> bool {
        tick >= self.start + DURATION
    }
    fn begin_impact(&mut self) -> Self {
        *self
    }
    fn advance(&mut self, world: &mut World, events: &mut Vec<Event>) -> bool {
        if world.winner.is_some() {
            return false;
        }
        if let Some(id) = self.follows {
            if let Some(i) = world.index(id) {
                self.position = world.entities[i].position;
            }
            if world.tick >= self.start {
                self.follows = None;
            }
        }
        if world.tick < self.start {
            return true;
        }
        let elapsed = world.tick - self.start;
        if matches!(self.pool, Pool::Fish { .. }) && !elapsed.is_multiple_of(TICK_HZ) {
            return true;
        }
        let cp = world
            .index(self.owner)
            .map_or(0., |i| world.entities[i].item_stats.crystal);
        let pierce = world
            .index(self.owner)
            .map_or(0., |i| world.entities[i].item_stats.shield_pierce);
        for i in 0..world.entities.len() {
            let e = &mut world.entities[i];
            if e.hp <= 0.
                || !matches!(e.kind, Kind::Hero | Kind::Minion)
                || (e.position.x - self.position.x).abs() > self.pool.radius()
                || e.position.dist(self.position) > self.pool.radius()
                || !world.layout.clear(self.position, e.position)
            {
                continue;
            }
            match self.pool {
                Pool::Fish { rank, stun } if e.team != self.team => {
                    if elapsed == 0 {
                        e.status.stun_until = e.status.stun_until.max(world.tick + stun);
                        e.winding = None;
                        e.cast = None;
                        e.status.channel = 0;
                        e.status.channel_until = 0;
                    }
                    let raw = if let Some(rules) = &world.abilities.lorelai {
                        (if elapsed == 0 {
                            &rules.impact
                        } else {
                            &rules.per_second
                        })
                        .at(rank, cp, 0.)
                    } else if elapsed == 0 {
                        80. + 35. * rank as f32
                    } else {
                        [55., 95., 135., 175., 255.][rank]
                    };
                    let damage = shop::mitigated(raw, e.stats.shield, pierce);
                    world.hurt(
                        self.owner,
                        self.team,
                        i,
                        damage,
                        DamageSchool::Crystal,
                        events,
                    );
                }
                Pool::Splash { speed, .. } if e.team == self.team => {
                    e.status.water_speed_bonus = if e.status.water_speed_until > world.tick {
                        match (e.status.water_speed_bonus, speed) {
                            (Some(a), Some(b)) => Some(a.max(b)),
                            (a, b) => a.or(b),
                        }
                    } else {
                        speed
                    };
                    e.status.water_speed_until = world.tick + 1;
                }
                Pool::Splash { slow_factor, .. } => {
                    e.status.water_slow_factor = if e.status.water_slow_until > world.tick {
                        e.status.water_slow_factor.min(slow_factor)
                    } else {
                        slow_factor
                    };
                    e.status.water_slow_until = world.tick + 1;
                }
                _ => {}
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> World {
        let mut w = World::new(1, 10);
        let e = &mut w.entities[0];
        e.type_id = HERO;
        e.energy = 360.;
        e.max_energy = 360.;
        e.level = 7;
        e.skill_points = 3;
        e.position = Position { x: 26., y: 0. };
        w.entities[1].position = Position { x: 29., y: 0. };
        w.entities[2].position = Position { x: 26., y: 0. };
        for slot in [AbilitySlot::A, AbilitySlot::B, AbilitySlot::C] {
            w.command(1, Command::Learn(slot)).unwrap();
        }
        w
    }
    fn cast(w: &mut World, slot: AbilitySlot, target: CastTarget) {
        w.command(1, Command::Cast { slot, target }).unwrap();
        assert!(
            w.step()
                .iter()
                .any(|e| matches!(e, Event::Cast { source: 1, .. }))
        );
    }
    #[test]
    fn delayed_fish_stuns_and_pulses_only_living_enemies_in_the_pool() {
        let mut w = setup();
        let point = w.entities[1].position;
        let starting = w.entities[1].hp;
        cast(&mut w, AbilitySlot::A, CastTarget::Point(point));
        for _ in 0..13 {
            w.step();
        }
        assert_eq!(starting, w.entities[1].hp);
        w.step();
        let snapped = w.entities[1].hp;
        assert!(snapped < starting);
        assert!(w.command(2, Command::Attack(1)).is_err());
        w.entities[1].position = Position { x: 36., y: 0. };
        for _ in 0..TICK_HZ {
            w.step();
        }
        assert_eq!(w.entities[1].hp, snapped);
        w.entities[1].position = point;
        for _ in 0..TICK_HZ {
            w.step();
        }
        assert!(w.entities[1].hp < snapped);
        assert_eq!(w.entities[0].hp, w.entities[0].max_hp);
        while w.tick < 177 {
            w.step();
        }
        assert!(w.entities.spells::<Water>().next().is_none());
    }
    #[test]
    fn splash_affects_movement_and_its_owner_can_consume_it() {
        let mut w = setup();
        let p = Position { x: 27., y: 0. };
        cast(&mut w, AbilitySlot::B, CastTarget::Point(p));
        for _ in 0..6 {
            w.step();
        }
        assert!(w.entities[2].status.water_speed_until > w.tick);
        assert!(w.entities[1].status.water_slow_until > w.tick);
        assert_eq!(w.entities[2].status.haste_until, 0);
        cast(&mut w, AbilitySlot::A, CastTarget::Point(p));
        assert_eq!(w.entities.spells::<Water>().count(), 1);
        assert_eq!(
            w.entities.spells::<Water>().next().unwrap().start,
            w.tick + 14
        );
        assert!(
            matches!(w.entities.spells::<Water>().next().unwrap().pool,Pool::Fish {stun,..} if stun>10)
        );
    }
    #[test]
    fn only_a_fresh_normal_allied_wall_acquires_a_buff_instance_after_release() {
        for (target, existing, empowered, expected) in [
            (3, false, false, true),
            (1, false, false, false),
            (3, true, false, false),
            (3, false, true, false),
        ] {
            let mut w = setup();
            if existing {
                w.entities[2].status.barrier = 35.;
                w.entities[2].status.barrier_until = 80;
            }
            if empowered {
                let p = w.entities[0].position;
                cast(&mut w, AbilitySlot::B, CastTarget::Point(p));
                for _ in 0..6 {
                    w.step();
                }
            }
            let before = w.next_id;
            w.command(
                1,
                Command::Cast {
                    slot: AbilitySlot::C,
                    target: CastTarget::Entity(target),
                },
            )
            .unwrap();
            let events = w.step();
            let release = events
                .iter()
                .position(|e| {
                    matches!(
                        e,
                        Event::Cast {
                            slot: AbilitySlot::C,
                            ..
                        }
                    )
                })
                .unwrap();
            let applied=events.iter().position(|e|matches!(e,Event::WaterwallApplied{source:1,target:t,id} if *t==target && *id==before));
            assert_eq!(applied.is_some(), expected);
            assert!(applied.is_none_or(|i| i > release));
            assert_eq!(w.next_id, before + u32::from(expected));
        }
    }

    #[test]
    fn waterwall_cleanses_absorbs_then_drops_at_the_recipients_new_position() {
        let mut w = setup();
        let slot = AbilitySlot::C;
        assert!(
            w.command(
                1,
                Command::Cast {
                    slot,
                    target: CastTarget::Entity(2)
                }
            )
            .is_err()
        );
        let e = &mut w.entities[2];
        e.status.stun_until = 40;
        e.status.silence_until = 40;
        e.status.bulwark_slow_until = 40;
        e.status.water_slow_until = 20;
        let starting = e.hp;
        cast(&mut w, slot, CastTarget::Entity(3));
        assert_eq!(w.entities[2].status.stun_until, 0);
        assert_eq!(w.entities[2].status.silence_until, 0);
        assert_eq!(w.entities[2].status.bulwark_slow_until, 0);
        assert_eq!(w.entities[2].status.water_slow_until, 0);
        let mut events = vec![];
        w.hurt(2, 1, 2, 490., DamageSchool::Weapon, &mut events);
        assert!(events.is_empty());
        w.hurt(2, 1, 2, 24., DamageSchool::Weapon, &mut events);
        assert_eq!(w.entities[2].hp, starting - 14.);
        let p = Position { x: 35., y: 0. };
        w.entities[2].position = p;
        while w.tick < 81 {
            w.step();
        }
        assert_eq!(w.entities.spells::<Water>().next().unwrap().position, p);
        assert!(
            w.entities
                .spells::<Water>()
                .next()
                .unwrap()
                .follows
                .is_none()
        );
        assert!(w.entities[2].status.water_speed_until > w.tick);
    }
}
