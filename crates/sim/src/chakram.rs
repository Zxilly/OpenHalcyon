//! Idris B for the hosted lane. Local evidence and assumptions are documented in idris.md.
use super::*;

/// Idris has zero native mana. This project's stamina pool recharges lazily at 4 units/s.
pub(super) fn stamina(e: &Entity, tick: u64) -> f32 {
    let recovery = tick.saturating_sub(e.status.stamina_spent_at) as f32 * 4. / TICK_HZ as f32;
    100. - (e.status.stamina_spent - recovery).max(0.)
}

#[derive(bevy_ecs::prelude::Component, Clone, Debug, Serialize)]
pub(super) struct Chakram {
    source: u32,
    team: u8,
    position: Position,
    aim: Position,
    returning: bool,
    hit: Vec<u32>,
    damage: f32,
    stamina_refund: Option<f32>,
}

impl Chakram {
    pub fn new(
        source: u32,
        team: u8,
        position: Position,
        aim: Position,
        damage: f32,
        stamina_refund: Option<f32>,
    ) -> Self {
        Self {
            source,
            team,
            position,
            aim,
            returning: false,
            hit: Vec::new(),
            damage,
            stamina_refund,
        }
    }
}

impl Room<'_> {
    pub(super) fn advance_chakrams(&mut self, events: &mut Vec<Event>) {
        self.advance_spell_family::<Chakram>(events);
    }
}

impl crate::persistent_spells::TimedSpell for Chakram {
    fn expired_before_step(&self, _: u64) -> bool {
        false
    }
    fn begin_impact(&mut self) -> Self {
        let hit = std::mem::take(&mut self.hit);
        Self {
            hit,
            ..self.clone()
        }
    }
    fn advance(&mut self, world: &mut Room<'_>, events: &mut Vec<Event>) -> bool {
        let Some(owner) = world.index(self.source) else {
            return false;
        };
        if world.winner.is_some() || world.entities[owner].hp <= 0. {
            return false;
        }
        let goal = if self.returning {
            world.entities[owner].position
        } else {
            self.aim
        };
        let next = self.position.advance(goal, 18. / TICK_HZ as f32);
        if !world.layout.clear(self.position, next) {
            return false;
        }
        for i in 0..world.entities.len() {
            let target = &world.entities[i];
            if target.hp <= 0.
                || target.team == self.team
                || !matches!(target.kind, Kind::Hero | Kind::Minion)
                || self.hit.contains(&target.id)
                || combat::line_distance(target.position, self.position, next)
                    > 0.75 + target.stats.physiology.map_or(0., |p| p.collision_radius)
                || !world.layout.clear(self.position, target.position)
            {
                continue;
            }
            self.hit.push(target.id);
            let hero = target.kind == Kind::Hero;
            let (resist, pierce, school) = if self.stamina_refund.is_some() {
                (
                    target.stats.shield,
                    world.entities[owner].item_stats.shield_pierce,
                    DamageSchool::Crystal,
                )
            } else {
                (
                    target.stats.armor,
                    world.entities[owner].item_stats.armor_pierce,
                    DamageSchool::Weapon,
                )
            };
            let damage = shop::mitigated(self.damage, resist, pierce);
            world.hurt(self.source, self.team, i, damage, school, events);
            if hero && let Some(refund) = self.stamina_refund {
                let available = stamina(&world.entities[owner], world.state.tick);
                let status = &mut world.entities[owner].status;
                status.stamina_spent = (100. - available - refund).max(0.);
                status.stamina_spent_at = world.state.tick;
            }
        }
        self.position = next;
        if next == goal {
            if self.returning {
                return false;
            }
            self.returning = true;
            self.hit.clear();
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> World {
        let mut w = World::new(8, 10);
        let actor = &mut w.entities[0];
        actor.type_id = 273;
        actor.position = Position { x: 28., y: 0. };
        actor.energy = 0.;
        actor.max_energy = 0.;
        w.entities[1].position = Position { x: 31., y: 0. };
        w
    }
    fn cast(p: Position) -> Command {
        Command::Cast {
            slot: AbilitySlot::B,
            target: CastTarget::Point(p),
        }
    }
    #[test]
    fn human_point_cast_hits_once_per_leg_and_uses_stamina_with_zero_mana() {
        let mut w = setup();
        let ability = cast(Position { x: 35., y: 0. });
        assert!(w.command(1, ability).is_err());
        w.command(1, Command::Learn(AbilitySlot::B)).unwrap();
        assert!(w.command(1, cast(Position { x: 40., y: 0. })).is_err());
        assert!(w.command(1, cast(w.entities[0].position)).is_err());
        w.entities[0].status.stamina_spent = 99.;
        assert!(w.command(1, ability).is_err());
        w.entities[0].status.stamina_spent = 0.;
        w.command(1, ability).unwrap();
        w.command(1, Command::Stop).unwrap();
        assert_eq!(stamina(&w.entities[0], w.tick), 100.);
        w.command(1, ability).unwrap();
        let mut hits = vec![];
        let mut releases = 0;
        for _ in 0..30 {
            for event in w.step() {
                match event {
                    Event::Cast {
                        source: 1,
                        slot: AbilitySlot::B,
                        ..
                    } => {
                        releases += 1;
                        assert_eq!(stamina(&w.entities[0], w.tick), 90.);
                        assert_eq!(w.entities[0].energy, 0.);
                    }
                    Event::Damage {
                        source: 1,
                        target: 2,
                        amount,
                        ..
                    } => hits.push(amount),
                    _ => {}
                }
            }
        }
        assert_eq!(releases, 1);
        let expected = w.entities[0].weapon_power() * 0.8 * 100. / (100. + w.entities[1].armor());
        assert_eq!(hits, [expected, expected]);
        assert!(w.entities.spells::<Chakram>().next().is_none() && w.command(1, ability).is_err());
    }

    #[test]
    fn bot_aims_at_visible_enemy_and_wall_or_owner_death_ends_flight() {
        let mut w = setup();
        w.command(1, Command::Learn(AbilitySlot::B)).unwrap();
        assert_eq!(w.decide(0), cast(w.entities[1].position));
        w.entities[0].position = Position { x: 17., y: 3. };
        assert!(w.command(1, cast(Position { x: 24., y: 3. })).is_err());
        w.entities[0].position = Position { x: 28., y: 0. };
        w.command(1, cast(Position { x: 35., y: 0. })).unwrap();
        for _ in 0..4 {
            w.step();
        }
        assert_eq!(w.entities.spells::<Chakram>().count(), 1);
        w.entities[0].hp = 0.;
        w.entities[0].respawn = w.tick + 2;
        w.step();
        assert!(w.entities.spells::<Chakram>().next().is_none());
        w.step();
        assert!(w.entities[0].hp > 0.);
        assert_eq!(stamina(&w.entities[0], w.tick), 100.);
    }

    #[test]
    fn return_tracks_a_moving_caster_and_can_hit_a_different_enemy() {
        let mut w = setup();
        w.command(1, Command::Learn(AbilitySlot::B)).unwrap();
        w.command(1, cast(Position { x: 35., y: 0. })).unwrap();
        let mut outbound = 0;
        for _ in 0..12 {
            outbound += w
                .step()
                .iter()
                .filter(|e| {
                    matches!(
                        e,
                        Event::Damage {
                            source: 1,
                            target: 2,
                            ..
                        }
                    )
                })
                .count();
        }
        assert_eq!(outbound, 1);
        assert!(w.entities.spells::<Chakram>().next().unwrap().returning);
        w.entities[0].position = Position { x: 28., y: -4. };
        w.entities[3].position = Position { x: 31.5, y: -2. };
        let mut back = vec![];
        for _ in 0..12 {
            back.extend(w.step().into_iter().filter_map(|e| {
                if let Event::Damage {
                    source: 1, target, ..
                } = e
                {
                    Some(target)
                } else {
                    None
                }
            }));
        }
        assert_eq!(back, [4]);
        assert!(w.entities.spells::<Chakram>().next().is_none());
    }
}
