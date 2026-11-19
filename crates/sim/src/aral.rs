use super::*;
use layout::point;

// Slots 0, 2, 8 and 10 are presentation-only shop/beacon declarations.
// Reserve the entire native static block; dynamic entities start at 517.
const FIRST_STATIC_ACTOR: u32 = 501;

pub(super) const PADS: [[(f32, f32); 3]; 2] = [
    [(-78.18, 0.88), (-81.81, 1.97), (-77.03, -1.94)],
    [(79.66, -0.91), (78.6, -5.06), (83.22, 0.72)],
];

// Original placement index within one side, template, unsigned X and world Z.
const TOWERS: [(u8, u16, f32, f32); 5] = [
    (3, 301, 75.48, 11.96),
    (4, 301, 68.59, 19.97),
    (5, 300, 54., 2.92),
    (6, 300, 35.78, 1.17),
    (7, 300, 17.06, 1.93),
];

#[derive(Serialize)]
pub(super) struct Units([Stats; 7]);
impl Default for Units {
    fn default() -> Self {
        let mut rows = [
            Stats::new(287, 350., 10., 0., 4.5, 3.2),
            Stats::new(288, 522., 32., 0., 2.9, 3.2),
            Stats::new(289, 400., 13., 0., 1.2, 3.2),
            Stats::new(292, 10000., 0., 0., 0., 0.),
            Stats::new(293, 10000., 0., 0., 0., 0.),
            Stats::new(300, 3500., 300., 32., 9., 0.),
            Stats::new(301, 2600., 400., 32., 9., 0.),
        ];
        for (s, (hp, weapon)) in rows.iter_mut().zip([(56., 4.), (272., 15.), (96., 3.)]) {
            s.growth.hp = hp;
            s.growth.weapon = weapon;
        }
        Self(rows)
    }
}
impl Units {
    pub fn load(&mut self, data: &GameData) {
        for (stats, name) in self.0.iter_mut().zip([
            "RangedMinion",
            "LeadMinion",
            "TankMinion",
            "VainCrystal_Home_3v3",
            "VainCrystal_Away_3v3",
            "Turret",
            "VainTurret",
        ]) {
            if let Some(unit) = data.unit(name) {
                *stats = Stats::load(stats.id, unit);
            }
        }
    }
}

impl World {
    /// Activate once after assigning the six human ARAL seats, before the first simulated tick.
    pub fn use_aral_lane(&mut self) -> Result<(), String> {
        if self.tick != 0 {
            return Err("ARAL layout requires an unstarted world".into());
        }
        if matches!(self.layout, Layout::Aral { .. }) {
            return Ok(());
        }
        let mut starts = [Position::default(); 6];
        let mut counts = [0usize; 2];
        for hero in self.entities.iter().filter(|e| e.kind == Kind::Hero) {
            let t = usize::from(hero.team);
            let &(x, z) = PADS[t]
                .get(counts[t])
                .ok_or("ARAL requires three seats on each team")?;
            let spot = starts
                .get_mut((hero.id - 1) as usize)
                .ok_or("ARAL requires six roster seats")?;
            *spot = Position { x, y: z };
            counts[t] += 1;
        }
        if counts != [3, 3] {
            return Err("ARAL requires three seats on each team".into());
        }
        self.layout = Layout::Aral {
            hero_starts: starts,
        };
        self.entities.retain(|actor| actor.kind == Kind::Hero);
        for hero in &mut self.entities {
            hero.position = self.layout.home(hero);
            hero.level = 4;
            hero.skill_points = 4;
            hero.apply_growth(3);
            hero.gold = 2000;
            self.scores[(hero.id - 1) as usize]
                .gold_income
                .starting_balance = hero.gold;
        }
        for team in 0..2 {
            let first = FIRST_STATIC_ACTOR + u32::from(team) * 8;
            self.next_id = first + 1;
            self.spawn_with_stats(
                team,
                Kind::Crystal,
                self.aral_units.0[3 + usize::from(team)],
                self.layout.objective(team),
                false,
            );
            for (slot, template, x, z) in TOWERS {
                self.next_id = first + u32::from(slot);
                let stats = self.aral_units.0[if template == 300 { 5 } else { 6 }];
                self.spawn_with_stats(team, Kind::Tower, stats, point(team, x, z), false);
            }
        }
        Ok(())
    }

    pub fn aral_structure_slot(&self, actor: u32) -> Option<u8> {
        if !matches!(self.layout, Layout::Aral { .. }) {
            return None;
        }
        let slot = actor.checked_sub(FIRST_STATIC_ACTOR)?;
        let kind = self.entities[self.index(actor)?].kind;
        (slot < 16 && matches!(kind, Kind::Crystal | Kind::Tower)).then_some(slot as u8)
    }

    pub(super) fn is_outer_tower(&self, actor: &Entity) -> bool {
        actor.kind == Kind::Tower
            && (!matches!(self.layout, Layout::Aral { .. })
                || actor.position == point(actor.team, 17.06, 1.93))
    }

    pub(super) fn spawn_aral_wave(&mut self, events: &mut Vec<Event>) {
        let Some(elapsed) = self.tick.checked_sub(15 * TICK_HZ) else {
            return;
        };
        let offset = elapsed % (23 * TICK_HZ);
        if !offset.is_multiple_of(TICK_HZ) {
            return;
        }
        let Some(&slot) = [2, 2, 1, 0].get((offset / TICK_HZ) as usize) else {
            return;
        };
        for team in 0..2 {
            self.spawn_lane_minion(
                team,
                self.aral_units.0[slot],
                self.layout.wave(team, 0),
                events,
            );
        }
    }

    pub(super) fn aral_march_goal(&self, actor: &Entity) -> Position {
        // Route through the three measured positions on each side. Exact navmesh
        // splines are unknown. The next objective is always in the enemy direction.
        let direction = if actor.team == 0 { 1. } else { -1. };
        for (x, z) in [
            (-54., 2.92),
            (-35.78, 1.17),
            (-17.06, 1.93),
            (17.06, 1.93),
            (35.78, 1.17),
            (54., 2.92),
            (68.59, 19.97),
        ] {
            if direction * actor.position.x < x - 0.3 {
                return Position {
                    x: direction * x,
                    y: z,
                };
            }
        }
        self.layout.objective(1 - actor.team)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lane() -> World {
        let mut w = World::new(7, 6);
        w.use_six_human_roster().unwrap();
        w.assign_teams(&[1, 0, 1, 0, 0, 1]).unwrap();
        w.use_aral_lane().unwrap();
        w
    }

    #[test]
    fn measured_seats_static_slots_and_staggered_waves() {
        let mut w = lane();
        w.use_aral_lane().unwrap();
        assert_eq!(w.entities.len(), 18);
        assert!(w.entities.is_sorted_by(|left, right| left.id < right.id));
        let sim_slots: Vec<_> = w.entities[6..]
            .iter()
            .map(|e| e.id - FIRST_STATIC_ACTOR)
            .collect();
        assert_eq!(sim_slots, [1, 3, 4, 5, 6, 7, 9, 11, 12, 13, 14, 15]);
        for slot in [0, 2, 8, 10] {
            assert!(w.index(FIRST_STATIC_ACTOR + slot).is_none());
        }
        assert_eq!(w.next_id, 517);
        for team in [0, 1] {
            let positions: Vec<_> = w.entities[..6]
                .iter()
                .filter(|e| e.team == team)
                .map(|e| (e.position.x, e.position.y))
                .collect();
            assert_eq!(positions, PADS[usize::from(team)]);
            let mut slots: Vec<_> = w
                .entities
                .iter()
                .filter(|e| e.team == team)
                .filter_map(|e| {
                    w.aral_structure_slot(e.id)
                        .map(|slot| (slot - team * 8, e.type_id, e.hp as u32))
                })
                .collect();
            slots.sort();
            assert_eq!(
                slots,
                [
                    (1, 292 + u16::from(team), 10000),
                    (3, 301, 2600),
                    (4, 301, 2600),
                    (5, 300, 3500),
                    (6, 300, 3500),
                    (7, 300, 3500)
                ]
            );
        }
        let mut rows = Vec::new();
        for _ in 0..39 * TICK_HZ {
            for ev in w.step() {
                if let Event::Spawn { id } = ev
                    && let Some(i) = w.index(id)
                {
                    let e = &w.entities[i];
                    assert!(id >= 517);
                    if e.kind == Kind::Minion && e.team == 0 {
                        assert!(e.position.dist(point(0, 71.28, 12.93)) < 0.2);
                        rows.push((w.tick / TICK_HZ, e.type_id));
                    }
                }
            }
        }
        assert_eq!(
            rows,
            [
                (15, 289),
                (16, 289),
                (17, 288),
                (18, 287),
                (38, 289),
                (39, 289)
            ]
        );
        assert!(w.use_aral_lane().is_err());
    }

    #[test]
    fn four_archived_idris_inputs_fit_range_and_playable_geometry() {
        for (px, pz, x, z) in [
            (-38.417, 0.785, -40.8939, 1.4944),
            (-65.077, 12.327, -58.2402, 14.9417),
            (-46.192, 8.335, -44.1787, 3.5794),
            (-36.317, 5.817, -34.1510, -0.7185),
        ] {
            let mut w = lane();
            w.entities[0].type_id = 273;
            w.entities[0].position = Position { x: px, y: pz };
            w.command(1, Command::Learn(AbilitySlot::B)).unwrap();
            let target = CastTarget::Point(Position { x, y: z });
            w.command(
                1,
                Command::Cast {
                    slot: AbilitySlot::B,
                    target,
                },
            )
            .unwrap();
            assert!((0..5).flat_map(|_| w.step()).any(|e| matches!(
                e,
                Event::Cast {
                    source: 1,
                    slot: AbilitySlot::B,
                    ..
                }
            )));
        }
    }

    #[test]
    fn native_lane_can_finish_via_normal_commands_and_crystal_combat() {
        let mut w = lane();
        for _ in 0..(60 * 45 * TICK_HZ) {
            if w.tick.is_multiple_of(4) {
                for i in 0..6 {
                    let order = w.decide(i);
                    if w.entities[i].hp > 0. && w.entities[i].order != order {
                        let _ = w.command(w.entities[i].id, order);
                    }
                }
            }
            w.step();
            if w.winner.is_some() {
                break;
            }
        }
        let winner = w
            .winner
            .expect("six active heroes and staggered waves reach a crystal");
        assert_eq!(
            w.team_scores()[usize::from(winner)]
                .structure_kills
                .crystals,
            1
        );
        assert!(
            w.entities
                .iter()
                .any(|e| e.kind == Kind::Crystal && e.team != winner && e.hp == 0.)
        );
    }
}
