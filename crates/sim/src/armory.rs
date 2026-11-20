use super::*;

// Update 3.7 sets living regeneration; 3.5 sets the independent rebuild timer.
pub(super) const REGENERATE: u64 = 180 * TICK_HZ;
pub(super) const REBUILD: u64 = 270 * TICK_HZ;

impl World {
    pub(super) fn spawn_solo_armories(&mut self) {
        let stats = Stats::new(297, 4350., 0., 100., 0., 0.);
        self.next_id = 2000 - 1499;
        for team in 0..2 {
            for (x, z) in [(70., -11.), (63., 0.), (70., 11.)] {
                self.spawn_with_stats(team, Kind::Armory, stats, layout::point(team, x, z), false);
            }
        }
    }

    pub(super) fn living_armories(&self, team: u8) -> usize {
        let first = 501 + 3 * u32::from(team);
        (first..first + 3)
            .filter(|&id| self.index(id).is_some_and(|i| self.entities[i].hp > 0.))
            .count()
    }

    pub(super) fn advance_armory(&mut self, i: usize, events: &mut Vec<Event>) {
        let e = &mut self.entities[i];
        if e.hp >= e.max_hp || self.tick < e.respawn {
            return;
        }
        if e.hp == 0. {
            e.hp = e.max_hp;
            events.push(Event::Spawn { id: e.id });
        } else {
            let (id, max_hp) = (e.id, e.max_hp);
            self.heal(id, i, max_hp, events);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solo_crystal_resumes_shooting_after_an_armory_returns() {
        let mut w = World::new(10, 10);
        w.use_solo_mid_lane().unwrap();
        let crystal = 2007 - 1499;
        let i = w.index(crystal).unwrap();
        for id in 504..=506 {
            let j = w.index(id).unwrap();
            w.entities[j].hp = 0.;
            w.entities[j].respawn = REBUILD;
        }
        w.entities[0].position = w.entities[i].position;
        let fired = |events: &[Event]| {
            events
                .iter()
                .any(|event| matches!(event,Event::Attack{source,target:1} if *source == crystal))
        };
        for _ in 0..30 {
            assert!(!fired(&w.step()));
        }
        let j = w.index(504).unwrap();
        w.entities[j].respawn = w.tick + 1;
        let mut released = false;
        for _ in 0..40 {
            let events = w.step();
            if fired(&events) {
                assert!(events.iter().any(
                    |event| matches!(event,Event::Damage{source,target:1,amount,..}
                    if *source == crystal && (*amount-w.entities[0].max_hp*0.04).abs()<0.01)
                ));
                assert_eq!(w.entities[0].status.slow_factor, 0.4);
                released = true;
                break;
            }
        }
        assert!(released);
    }

    #[test]
    fn six_armories_recover_or_rebuild_without_turret_credit_or_gold() {
        let mut world = World::new(23, 10);
        assert!(!world.entities.iter().any(|e| e.kind == Kind::Armory));
        world.use_solo_mid_lane().unwrap();
        assert_eq!(world.living_armories(0), 3);
        assert_eq!(world.living_armories(1), 3);
        for id in 501..=506 {
            let e = &world.entities[world.index(id).unwrap()];
            assert_eq!((e.kind, e.type_id), (Kind::Armory, 297));
            assert_eq!(
                (e.max_hp, e.stats.armor, e.stats.shield),
                (4350., 100., 100.)
            );
            assert_eq!((e.stats.speed, e.stats.weapon, e.stats.range), (0., 0., 0.));
        }
        let i = world.index(504).unwrap();
        let (gold, xp) = (world.entities[0].gold, world.entities[0].xp);
        let mut events = vec![];
        world.tick = 125;
        world.hurt(1, 0, i, 100., DamageSchool::True, &mut events);
        world.tick += REGENERATE - 1;
        world.hurt(1, 0, i, 100., DamageSchool::True, &mut events);
        world.tick += REGENERATE - 1;
        world.advance_armory(i, &mut events);
        assert_eq!(world.entities[i].hp, 4150.);
        world.tick += 1;
        world.advance_armory(i, &mut events);
        assert_eq!(world.entities[i].hp, 4350.);
        let kill_tick = world.tick;
        for id in 504..=506 {
            let j = world.index(id).unwrap();
            world.hurt(1, 0, j, 4350., DamageSchool::True, &mut events);
        }
        assert_eq!(world.winner, None);
        assert_eq!(
            world.team_scores[0].structure_kills,
            score::StructureKills::default()
        );
        assert_eq!(world.team_scores[0].armories_destroyed, 3);
        assert_eq!(
            world.team_scores[0].first_enemy_armory_kill_seconds,
            Some(kill_tick as f64 / TICK_HZ as f64)
        );
        assert_eq!((world.entities[0].gold, world.entities[0].xp), (gold, xp));
        assert_eq!(world.living_armories(1), 0);
        world.tick += REBUILD - 1;
        world.advance_armory(i, &mut events);
        assert_eq!(world.living_armories(1), 0);
        world.tick += 1;
        world.advance_armory(i, &mut events);
        assert_eq!(world.entities[i].hp, 4350.);
        assert_eq!(world.living_armories(1), 1);
        assert_eq!(events.last(), Some(&Event::Spawn { id: 504 }));
    }
}
