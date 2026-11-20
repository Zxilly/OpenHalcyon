use super::*;

/// The validation map and the measured mid-lane subset of the Android Solo map.
#[derive(Default, Serialize)]
pub(super) enum Layout {
    #[default]
    Validation,
    Aral {
        hero_starts: [Position; 6],
    },
    SoloMid {
        hero_starts: [Position; 10],
    },
}

pub(super) fn point(team: u8, x: f32, z: f32) -> Position {
    Position {
        x: if team == 0 { -x } else { x },
        y: z,
    }
}

const STARTS: [(f32, f32); 5] = [(90., 0.), (94., -6.), (101., -3.), (101., 3.), (94., 6.)];

impl Layout {
    pub(super) fn native_xp_required(&self, level: u8) -> Option<f32> {
        match (self, level) {
            (Self::Aral { .. }, 4) => Some(90.),
            (Self::Aral { .. }, 5..=12) => Some(130. + 10. * f32::from(level - 5)),
            (Self::SoloMid { .. }, 1..=12) => Some(68. + 16. * f32::from(level - 1)),
            _ => None,
        }
    }

    pub fn home(&self, actor: &Entity) -> Position {
        match self {
            Self::SoloMid { hero_starts } if actor.kind == Kind::Hero => {
                hero_starts[(actor.id - 1) as usize]
            }
            Self::SoloMid { .. } => point(actor.team, 90., 0.),
            Self::Aral { hero_starts } if actor.kind == Kind::Hero => {
                hero_starts[(actor.id - 1) as usize]
            }
            Self::Aral { .. } => self.wave(actor.team, 0),
            Self::Validation => base(actor.team),
        }
    }

    pub fn at_base(&self, actor: &Entity, inclusive: bool) -> bool {
        let inside = |p| {
            let distance = actor.position.dist(p);
            if inclusive {
                distance <= 3.
            } else {
                distance < 3.
            }
        };
        match self {
            Self::Aral { .. } => aral::PADS[usize::from(actor.team)]
                .iter()
                .any(|&(x, z)| inside(Position { x, y: z })),
            Self::Validation => inside(base(actor.team)),
            Self::SoloMid { .. } => STARTS.iter().any(|&(x, z)| inside(point(actor.team, x, z))),
        }
    }

    pub fn objective(&self, team: u8) -> Position {
        match self {
            Self::Validation => base(team),
            Self::SoloMid { .. } => point(team, 75., 0.),
            Self::Aral { .. } => point(team, 76.12, 19.9),
        }
    }

    pub fn wave(&self, team: u8, slot: usize) -> Position {
        match self {
            Self::Validation => Position {
                y: slot as f32 - 1.5,
                ..base(team)
            },
            Self::SoloMid { .. } => point(team, 70., 0.),
            Self::Aral { .. } => point(team, 71.28, 12.93),
        }
    }

    pub fn walkable(&self, p: Position) -> bool {
        match self {
            Self::Aral { .. } => (-95.0..=95.).contains(&p.x) && (-7.0..=21.).contains(&p.y),
            Self::Validation => nav::walkable(p),
            Self::SoloMid { .. } => (-101.0..=101.).contains(&p.x) && (-6.0..=6.).contains(&p.y),
        }
    }

    pub fn clear(&self, a: Position, b: Position) -> bool {
        match self {
            Self::Validation => nav::clear(a, b),
            Self::SoloMid { .. } | Self::Aral { .. } => self.walkable(a) && self.walkable(b),
        }
    }
}

pub(super) fn solo_inner_towers() -> [Stats; 2] {
    [
        Stats::new(339, 4000., 250., 100., 9., 0.),
        Stats::new(341, 5000., 280., 100., 9., 0.),
    ]
}

impl World {
    pub(super) fn load_solo_towers(&mut self, data: &GameData) {
        for (stats, name) in self
            .solo_inner_towers
            .iter_mut()
            .zip(["Turret5v5", "ArmoryTurret5v5"])
        {
            if let Some(unit) = data.unit(name) {
                *stats = Stats::load(stats.id, unit);
            }
        }
    }

    /// The native Solo replay has independent, staggered home/away mid lanes.
    pub(super) fn spawn_solo_mid_wave(&mut self, events: &mut Vec<Event>) {
        let Some(elapsed) = self.tick.checked_sub(19 * TICK_HZ) else {
            return;
        };
        let offset = elapsed % (25 * TICK_HZ);
        if !offset.is_multiple_of(TICK_HZ) {
            return;
        }
        let wave = elapsed / (25 * TICK_HZ) + 1;
        let lineup = if wave.is_multiple_of(4) {
            [4, 4, 5, 5, 7]
        } else {
            [4, 4, 6, 5, 5]
        };
        let Some(&stat) = lineup.get((offset / TICK_HZ) as usize) else {
            return;
        };
        for team in 0..2 {
            let id = self.spawn(team, Kind::Minion, stat, self.layout.wave(team, 0), false);
            events.push(Event::Spawn { id });
        }
    }

    /// Install the measured Solo mid-lane coordinates after team assignment and before the opening tick.
    pub fn use_solo_mid_lane(&mut self) -> Result<(), String> {
        if self.tick != 0 {
            return Err("Solo layout requires an unstarted world".into());
        }
        if matches!(self.layout, Layout::SoloMid { .. }) {
            return Ok(());
        }
        let mut counts = [0usize; 2];
        let mut starts = [Position::default(); 10];
        for hero in self.entities.iter().filter(|e| e.kind == Kind::Hero) {
            let seat = &mut counts[usize::from(hero.team)];
            let &(x, z) = STARTS
                .get(*seat)
                .ok_or("Solo layout requires five heroes on each team")?;
            starts[(hero.id - 1) as usize] = point(hero.team, x, z);
            *seat += 1;
        }
        if counts != [5, 5] {
            return Err("Solo layout requires five heroes on each team".into());
        }
        self.layout = Layout::SoloMid {
            hero_starts: starts,
        };
        self.entities.retain(|e| e.kind == Kind::Hero);
        for e in &mut self.entities {
            e.gold = 600;
            self.scores[(e.id - 1) as usize]
                .gold_income
                .starting_balance = e.gold;
            e.position = self.layout.home(e);
        }
        // Preserve the original actor namespace, including gaps for other lanes.
        for (wire, team, kind, stats, x) in [
            (2006, 0, Kind::Crystal, self.stats[2], 75.),
            (2007, 1, Kind::Crystal, self.stats[3], 75.),
            (2009, 0, Kind::Tower, self.stats[1], 17.5),
            (2012, 1, Kind::Tower, self.stats[1], 17.5),
            (2060, 0, Kind::Tower, self.solo_inner_towers[0], 34.5),
            (2065, 0, Kind::Tower, self.solo_inner_towers[1], 53.04),
            (2102, 1, Kind::Tower, self.solo_inner_towers[0], 34.5),
            (2107, 1, Kind::Tower, self.solo_inner_towers[1], 53.04),
        ] {
            self.next_id = wire - 1499;
            self.spawn_with_stats(team, kind, stats, point(team, x, 0.), false);
        }
        self.next_id = 2122 - 1499;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solo_seats_shop_navigation_and_respawn_share_one_coordinate_system() {
        let mut world = World::new(8, 10);
        world.assign_teams(&[1, 0, 1, 0, 0, 1, 0, 1, 1, 0]).unwrap();
        world.use_solo_mid_lane().unwrap();
        world.use_solo_mid_lane().unwrap();
        for team in [0, 1] {
            let starts: Vec<_> = world
                .entities
                .iter()
                .filter(|e| e.kind == Kind::Hero && e.team == team)
                .map(|e| e.position)
                .collect();
            assert_eq!(starts, STARTS.map(|(x, z)| point(team, x, z)));
        }
        let home = world.entities[0].position;
        world.entities[0].gold = 600;
        world.purchase(1, BasicItem::WeaponBlade).unwrap();
        assert!(
            world
                .command(1, Command::MoveTo(Position { x: 0., y: 7. }))
                .is_err()
        );
        world
            .command(1, Command::MoveTo(Position { x: 0., y: 0. }))
            .unwrap();
        world.step();
        assert!(world.entities[0].position.x < home.x);
        assert!(world.use_solo_mid_lane().is_err());
        world.entities[0].position = point(1, 17.5, 0.);
        assert!(world.purchase(1, BasicItem::WeaponBlade).is_err());
        world.entities[0].hp = 0.;
        world.entities[0].respawn = world.tick + 1;
        world.step();
        assert_eq!(world.entities[0].position, home);
        assert!(world.visible(1, &world.entities[0]));
        assert!(!world.visible(0, &world.entities[0]));
        let mut aral = World::new(1, 6);
        aral.use_six_human_roster().unwrap();
        assert!(aral.use_solo_mid_lane().is_err());
        assert_eq!(aral.entities[0].position.x, 0.);
    }

    #[test]
    fn negative_lane_coordinates_support_targeted_casts_and_tick_cooldowns() {
        let mut world = World::new(7, 10);
        world.use_solo_mid_lane().unwrap();
        world.entities[0].position = Position { x: -40., y: 0. };
        world.entities[1].position = Position { x: -37., y: 0. };
        let cast = Command::Cast {
            slot: AbilitySlot::A,
            target: CastTarget::Entity(2),
        };
        world.command(1, Command::Learn(AbilitySlot::A)).unwrap();
        world.command(1, cast).unwrap();
        let hp = world.entities[1].hp;
        for _ in 0..5 {
            world.step();
        }
        assert!(world.entities[1].hp < hp);
        assert!(world.command(1, cast).is_err());
    }

    #[test]
    fn three_tower_tiers_award_local_bounties_but_only_outer_records_the_milestone() {
        let mut world = World::new(1, 10);
        world.use_solo_mid_lane().unwrap();
        let mut events = Vec::new();
        for (tick, wire, ty, hp, weapon) in [
            (20, 2107, 341, 5000., 280.),
            (40, 2102, 339, 4000., 250.),
            (60, 2012, 340, 3000., 190.),
        ] {
            let id = wire - 1499;
            let i = world.index(id).unwrap();
            let tower = &world.entities[i];
            assert_eq!(
                (
                    tower.type_id,
                    tower.max_hp,
                    tower.weapon_power(),
                    tower.armor(),
                    tower.shield()
                ),
                (ty, hp, weapon, 100., 100.)
            );
            assert!(!world.visible(0, tower));
            world.tick = tick;
            world.hurt(1, 0, i, hp, score::DamageSchool::True, &mut events);
            assert!(events.contains(&Event::Death { id, killer: 1 }));
            assert!(events.contains(&Event::GoldAward { id: 1, amount: 300 }));
            assert_eq!(
                world.team_scores()[0].first_enemy_outer_tower_kill_seconds,
                (ty == 340).then_some(3.)
            );
            events.clear();
        }
        assert_eq!(world.entities[0].gold, 1500);
        assert_eq!(world.team_scores()[0].structure_kills.towers, 3);
        assert_eq!(world.next_id, 2122 - 1499);
    }

    #[test]
    fn first_five_mid_waves_follow_the_game_clock_without_health_growth() {
        let mut w = World::new(37, 10);
        w.use_solo_mid_lane().unwrap();
        let mut counts = [0; 2];
        for tick in 1..144 * TICK_HZ {
            let events = w.step();
            let spawns: Vec<_> = events
                .iter()
                .filter_map(|ev| {
                    if let Event::Spawn { id } = ev {
                        Some(*id)
                    } else {
                        None
                    }
                })
                .collect();
            let sec = tick / TICK_HZ;
            let expected = tick.is_multiple_of(TICK_HZ) && sec >= 19 && (sec - 19) % 25 < 5;
            assert_eq!(spawns.len(), if expected { 2 } else { 0 }, "tick {tick}");
            if !expected {
                continue;
            }
            let wave = (sec - 19) / 25;
            let slot = ((sec - 19) % 25) as usize;
            let lineup = if wave == 3 {
                [336, 336, 335, 335, 338]
            } else {
                [336, 336, 337, 335, 335]
            };
            for (team, id) in spawns.into_iter().enumerate() {
                let e = &w.entities[w.index(id).unwrap()];
                assert_eq!((e.team, e.type_id, e.level), (team as u8, lineup[slot], 1));
                assert_eq!(
                    e.max_hp,
                    [475., 575., 880., 950.][usize::from(e.type_id - 335)]
                );
                assert_eq!(e.hp, e.max_hp);
                assert!(
                    e.position.dist(point(team as u8, 70., 0.))
                        <= e.stats.speed / TICK_HZ as f32 + 0.001
                );
                counts[team] += 1;
            }
        }
        assert_eq!(counts, [25, 25]);
    }

    #[test]
    fn solo_mid_waves_and_bots_reach_an_authoritative_crystal_victory() {
        let mut w = World::new(17, 0);
        w.use_solo_mid_lane().unwrap();
        let mut wave_seen = false;
        let mut tiers_fired = std::collections::BTreeSet::new();
        while w.winner.is_none() && w.tick < 200_000 {
            let events = w.step();
            for event in &events {
                if let Event::Attack { source, .. } = event
                    && let Some(i) = w.index(*source)
                    && w.entities[i].kind == Kind::Tower
                {
                    tiers_fired.insert(w.entities[i].type_id);
                }
            }
            if !wave_seen {
                let born: Vec<_> = events
                    .iter()
                    .filter_map(|ev| match ev {
                        Event::Spawn { id } => w
                            .index(*id)
                            .map(|i| &w.entities[i])
                            .filter(|e| e.kind == Kind::Minion),
                        _ => None,
                    })
                    .collect();
                if !born.is_empty() {
                    assert_eq!(born.len(), 2);
                    for e in born {
                        assert!(
                            e.position.dist(point(e.team, 70., 0.))
                                <= e.stats.speed / TICK_HZ as f32 + 0.001
                        );
                    }
                    wave_seen = true;
                }
            }
        }
        assert!(wave_seen);
        assert_eq!(tiers_fired, [339, 340, 341].into_iter().collect());
        let winner = w.winner.expect("bot teams must reach a crystal victory");
        assert_eq!(w.team_scores()[winner as usize].structure_kills.crystals, 1);
        assert!(
            w.entities
                .iter()
                .any(|e| e.kind == Kind::Crystal && e.team != winner && e.hp == 0.)
        );
    }
}
