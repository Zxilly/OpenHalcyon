use crate::combat::AbilityBook;
#[cfg(test)]
use crate::ecs::score::{self, DamageSchool};
use crate::ecs::world::{NAMES, Stats, base};
#[cfg(test)]
use crate::{AbilitySlot, BasicItem, CastTarget, Command};
use crate::{Entity, Event, Kind, Position, Room, TICK_HZ, World};
use halcyon_data::{EntityTemplate, GameData};
use serde::Serialize;

use std::sync::{Arc, LazyLock};

/// Compiled once per installation. Rooms share numeric tables and keep their own store selection.
pub struct WorldRules {
    pub(crate) stats: [Stats; 8],
    pub(crate) aral_units: crate::maps::aral::Units,
    pub(crate) solo_inner_towers: [Stats; 2],
    pub(crate) solo_lane_rewards: Option<halcyon_data::SoloLaneRewards>,
    pub(crate) first_wave: u64,
    pub(crate) wave_interval: u64,
    pub(crate) abilities: Arc<AbilityBook>,
    pub(crate) ability_intents: Arc<crate::ai::utility::HeroIntentCatalog>,
    pub(crate) shop: crate::items::shop::Shop,
}
pub(crate) static BUILTIN: LazyLock<WorldRules> = LazyLock::new(|| {
    // Confirmed delivery base numbers, usable without distributing extracted assets.
    let stats = [
        Stats::new(EntityTemplate::Ringo, 703., 74., 25., 6.2, 3.6),
        Stats::new(EntityTemplate::OuterTurret5v5, 3000., 190., 100., 9., 0.),
        Stats::new(EntityTemplate::VainCrystalHome5v5, 20000., 0., 0., 22., 0.),
        Stats::new(EntityTemplate::VainCrystalAway5v5, 20000., 0., 0., 22., 0.),
        Stats::new(EntityTemplate::MinionMelee5v5, 575., 13., 0., 1.3, 3.5),
        Stats::new(EntityTemplate::MinionRange5v5, 475., 15., 0., 4.5, 3.5),
        Stats::new(EntityTemplate::MinionSiege5v5, 880., 15., 0., 4., 3.5),
        Stats::new(EntityTemplate::MinionCaptain5v5, 950., 15., 0., 5., 3.5),
    ];

    WorldRules {
        stats,
        aral_units: Default::default(),
        solo_inner_towers: solo_inner_towers(),
        solo_lane_rewards: None,
        first_wave: 900,
        wave_interval: 600,
        abilities: Default::default(),
        ability_intents: Default::default(),
        shop: Default::default(),
    }
});
impl WorldRules {
    pub fn load(data: &GameData) -> Result<Self, String> {
        let units = NAMES.map(|name| {
            let unit = data
                .unit(name)
                .ok_or_else(|| format!("missing unit {name}"))?;
            let id = data
                .type_id(name)
                .ok_or_else(|| format!("missing manifest entry {name}"))?;
            Ok::<_, String>(Stats::load(id, unit, data))
        });
        let stats: Vec<_> = units.into_iter().collect::<Result<_, _>>()?;
        let timing = |key: &str| {
            data.official_rules["five_v_five"]["waves"][key]
                .as_u64()
                .and_then(|v| v.checked_mul(TICK_HZ))
                .filter(|n| *n > 0)
                .ok_or_else(|| format!("invalid wave timing {key}"))
        };

        let mut aral_units = crate::maps::aral::Units::default();
        aral_units.load(data);
        let mut solo_inner_towers = solo_inner_towers();
        for (stats, name) in solo_inner_towers
            .iter_mut()
            .zip(["Turret5v5", "ArmoryTurret5v5"])
        {
            if let Some(unit) = data.unit(name) {
                *stats = Stats::load(stats.id, unit, data);
            }
        }
        Ok(Self {
            stats: stats.try_into().expect("eight unit types"),
            first_wave: timing("first_spawn_s")?,
            wave_interval: timing("interval_s")?,
            aral_units,
            solo_inner_towers,
            solo_lane_rewards: data.solo_lane_rewards(),
            abilities: Arc::new(AbilityBook::from_data(data)?),
            ability_intents: Arc::new(crate::ai::utility::HeroIntentCatalog::from_data(data)),
            shop: crate::items::shop::Shop::load(data)?,
        })
    }
    pub fn world(&self, seed: u64, human_seats: u8) -> World {
        World::build(seed, human_seats, self)
    }
}

#[cfg(test)]
mod world_rules_tests {
    use super::*;

    #[test]
    fn rooms_share_rules_and_keep_independent_shop_selections() {
        let mut first = BUILTIN.world(1, 6);
        let second = BUILTIN.world(1, 6);
        assert!(Arc::ptr_eq(&first.abilities, &second.abilities));
        assert!(first.shop.shares_offers_with(&second.shop));
        assert_eq!(first.state_digest(), second.state_digest());
        first.use_item_store(&[]);
        assert_ne!(first.state_digest(), second.state_digest());
        assert!(first.shop.shares_offers_with(&second.shop));
    }
}

/// Room-local wire instance IDs for the six alternating startup applications
/// per crystal, in declaration order (away, then home). The fixed Solo
/// structure effects already occupy IDs through 2169.
pub const SOLO_CRYSTAL_START_INSTANCES: std::ops::Range<u32> = 2170..2182;

/// The validation map and the measured lanes of the Android Solo map.
#[derive(Default, Serialize)]
pub(crate) enum Layout {
    #[default]
    Validation,
    Aral {
        hero_starts: [Position; 6],
    },
    HalcyonFold {
        hero_starts: [Position; 6],
    },
    Blitz {
        hero_starts: [Position; 6],
    },
    SoloMid {
        hero_starts: [Position; 10],
    },
}

pub(crate) fn point(team: u8, x: f32, z: f32) -> Position {
    Position {
        x: if team == 0 { -x } else { x },
        y: z,
    }
}

const STARTS: [(f32, f32); 5] = [(90., 0.), (94., -6.), (101., -3.), (101., 3.), (94., 6.)];

impl Layout {
    pub(crate) fn native_xp_required(&self, level: u8) -> Option<f32> {
        match (self, level) {
            (Self::Aral { .. }, 4) => Some(90.),
            (Self::Aral { .. }, 5..=12) => Some(130. + 10. * f32::from(level - 5)),
            (Self::HalcyonFold { .. } | Self::Blitz { .. } | Self::SoloMid { .. }, 1..=12) => {
                Some(68. + 16. * f32::from(level - 1))
            }
            _ => None,
        }
    }

    pub fn home(&self, actor: &Entity) -> Position {
        match self {
            Self::SoloMid { hero_starts } if actor.kind == Kind::Hero => {
                hero_starts[(actor.id - 1) as usize]
            }
            Self::SoloMid { .. } => point(actor.team, 90., 0.),
            Self::Aral { hero_starts }
            | Self::HalcyonFold { hero_starts }
            | Self::Blitz { hero_starts }
                if actor.kind == Kind::Hero =>
            {
                hero_starts[(actor.id - 1) as usize]
            }
            Self::Aral { .. } | Self::HalcyonFold { .. } | Self::Blitz { .. } => {
                self.wave(actor.team, 0)
            }
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
            Self::Aral { .. } | Self::HalcyonFold { .. } | Self::Blitz { .. } => {
                crate::maps::aral::PADS[usize::from(actor.team)]
                    .iter()
                    .any(|&(x, z)| inside(Position { x, y: z }))
            }
            Self::Validation => inside(base(actor.team)),
            Self::SoloMid { .. } => STARTS.iter().any(|&(x, z)| inside(point(actor.team, x, z))),
        }
    }

    pub fn objective(&self, team: u8) -> Position {
        match self {
            Self::Validation => base(team),
            Self::SoloMid { .. } => point(team, 75., 0.),
            Self::Aral { .. } | Self::HalcyonFold { .. } | Self::Blitz { .. } => {
                point(team, 76.12, 19.9)
            }
        }
    }

    pub fn wave(&self, team: u8, slot: usize) -> Position {
        match self {
            Self::Validation => Position {
                y: slot as f32 - 1.5,
                ..base(team)
            },
            Self::SoloMid { .. } => point(team, 70., 0.),
            Self::Aral { .. } | Self::HalcyonFold { .. } | Self::Blitz { .. } => {
                point(team, 71.28, 12.93)
            }
        }
    }

    pub fn walkable(&self, p: Position) -> bool {
        match self {
            Self::Aral { .. } => (-95.0..=95.).contains(&p.x) && (-7.0..=21.).contains(&p.y),
            Self::HalcyonFold { .. } | Self::Blitz { .. } => {
                (-95.0..=95.).contains(&p.x) && (-12.0..=55.).contains(&p.y)
            }
            Self::Validation => crate::nav::grid::walkable(p),
            Self::SoloMid { .. } => crate::maps::solo_side::walkable(p),
        }
    }

    pub fn clear(&self, a: Position, b: Position) -> bool {
        match self {
            Self::Validation => crate::nav::grid::clear(a, b),
            Self::SoloMid { .. } => crate::maps::solo_side::clear(a, b),
            Self::Aral { .. } | Self::HalcyonFold { .. } | Self::Blitz { .. } => {
                self.walkable(a) && self.walkable(b)
            }
        }
    }
}

pub(crate) fn solo_inner_towers() -> [Stats; 2] {
    [
        Stats::new(EntityTemplate::Turret5v5, 4000., 250., 100., 9., 0.),
        Stats::new(EntityTemplate::ArmoryTurret5v5, 5000., 280., 100., 9., 0.),
    ]
}

impl Room<'_> {
    /// The native Solo replay has three staggered lanes for each team.
    pub(crate) fn spawn_solo_mid_wave(&mut self, events: &mut Vec<Event>) {
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
        let slot = (offset / TICK_HZ) as usize;
        if slot < 4 {
            let side_stat = if wave.is_multiple_of(4) {
                [4, 4, 5, 7]
            } else {
                [4, 4, 6, 5]
            };
            for team in 0..2 {
                for side in [-1, 1] {
                    let id = self.spawn(
                        team,
                        Kind::Minion,
                        side_stat[slot],
                        crate::maps::solo_side::anchor(team, side),
                        false,
                    );
                    self.entities.last_mut().unwrap().solo_side = side;
                    events.push(Event::Spawn { id });
                }
            }
        }
    }

    /// Install the measured Solo lane coordinates after team assignment and before the opening tick.
    pub fn use_solo_mid_lane(&mut self) -> Result<(), String> {
        if self.tick != 0 {
            return Err("Solo layout requires an unstarted world".into());
        }
        if matches!(self.layout, Layout::SoloMid { .. }) {
            return Ok(());
        }
        let mut counts = [0usize; 2];
        let mut starts = [Position::default(); 10];
        let mut lanes = [0i8; 10];
        for hero in self.entities.iter().filter(|e| e.kind == Kind::Hero) {
            let seat = &mut counts[usize::from(hero.team)];
            let &(x, z) = STARTS
                .get(*seat)
                .ok_or("Solo layout requires five heroes on each team")?;
            starts[(hero.id - 1) as usize] = point(hero.team, x, z);
            lanes[(hero.id - 1) as usize] = [0, -1, 1, 0, -1][*seat];
            *seat += 1;
        }
        if counts != [5, 5] {
            return Err("Solo layout requires five heroes on each team".into());
        }
        self.layout = Layout::SoloMid {
            hero_starts: starts,
        };
        self.entities.retain(|e| e.kind == Kind::Hero);
        for idx in 0..self.entities.len() {
            let e = &mut self.entities[idx];
            e.solo_side = lanes[(e.id - 1) as usize];
            e.gold = 600;
            self.state.scores[(e.id - 1) as usize]
                .gold_income
                .starting_balance = e.gold;
            e.position = self.state.layout.home(e);
        }
        self.spawn_solo_armories();
        // Preserve the original actor namespace, including gaps between static actors.
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
        for (wire, team, tier, x, z) in [
            (2008, 0, 0, 21.30, -57.50),
            (2010, 0, 0, 21.30, 57.50),
            (2011, 1, 0, 21.30, -57.50),
            (2013, 1, 0, 21.30, 57.50),
            (2046, 0, 1, 49.24, -37.84),
            (2051, 0, 2, 63., -22.41),
            (2074, 0, 1, 49.24, 37.84),
            (2079, 0, 2, 63., 22.41),
            (2088, 1, 1, 49.24, -37.84),
            (2093, 1, 2, 63., -22.41),
            (2116, 1, 1, 49.24, 37.84),
            (2121, 1, 2, 63., 22.41),
        ] {
            self.next_id = wire - 1499;
            let stats = [
                self.stats[1],
                self.solo_inner_towers[0],
                self.solo_inner_towers[1],
            ][tier];
            self.spawn_with_stats(team, Kind::Tower, stats, point(team, x, z), false);
        }
        self.entities.sort_unstable_by_key(|e| e.id);
        self.next_id = SOLO_CRYSTAL_START_INSTANCES.end - 1499;
        Ok(())
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    use crate::SOLO_CRYSTAL_START_INSTANCES;
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
        assert_eq!(world.next_id + 1499, SOLO_CRYSTAL_START_INSTANCES.end);
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
                        w.index(*id)
                            .filter(|i| w.entities[*i].solo_side == 0)
                            .map(|_| *id)
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
    #[ignore = "slow soak; run with cargo nextest run --run-ignored all"]
    fn solo_mid_waves_and_bots_reach_an_authoritative_crystal_victory() {
        let mut w = World::new(17, 0);
        w.use_solo_mid_lane().unwrap();
        let mut wave_seen = false;
        let mut side_towers_fired = std::collections::BTreeSet::new();
        let mut tiers_fired = std::collections::BTreeSet::new();
        while w.winner.is_none() && w.tick < 200_000 {
            let events = w.step();
            for event in &events {
                if let Event::Attack { source, .. } = event
                    && let Some(i) = w.index(*source)
                    && w.entities[i].kind == Kind::Tower
                {
                    if w.entities[i].position.y.abs() > 6. {
                        side_towers_fired.insert(w.entities[i].position.y.is_sign_positive());
                    }
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
                            .filter(|e| e.kind == Kind::Minion && e.solo_side == 0),
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
        assert_eq!(side_towers_fired.len(), 2);
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

#[cfg(test)]
mod assignments {
    use super::*;

    #[test]
    fn solo_lane_assignments_follow_team_slot_with_grouped_or_alternating_actors() {
        for teams in [
            [0, 0, 0, 0, 0, 1, 1, 1, 1, 1],
            [1, 0, 1, 0, 1, 0, 1, 0, 1, 0],
            [0, 0, 0, 1, 1, 1, 0, 0, 1, 1],
        ] {
            let mut w = World::new(19, 10);
            w.assign_teams(&teams).unwrap();
            w.use_solo_mid_lane().unwrap();
            for team in 0..2 {
                let lanes: Vec<_> = w
                    .entities
                    .iter()
                    .filter(|e| e.kind == Kind::Hero && e.team == team)
                    .map(|e| e.solo_side)
                    .collect();
                assert_eq!(lanes, [0, -1, 1, 0, -1]);
            }
        }
    }
}

// Update 3.7 sets living regeneration; 3.5 sets the independent rebuild timer.
pub(crate) const REGENERATE: u64 = 180 * TICK_HZ;
pub(crate) const REBUILD: u64 = 270 * TICK_HZ;

impl Room<'_> {
    pub(crate) fn spawn_solo_armories(&mut self) {
        let stats = Stats::new(EntityTemplate::VainNode, 4350., 0., 100., 0., 0.);
        self.next_id = 2000 - 1499;
        for team in 0..2 {
            for (x, z) in [(70., -11.), (63., 0.), (70., 11.)] {
                self.spawn_with_stats(team, Kind::Armory, stats, point(team, x, z), false);
            }
        }
    }

    pub(crate) fn solo_structure_protected(&self, target: &Entity) -> bool {
        match target.kind {
            Kind::Armory => self.armory_protected(target),
            Kind::Crystal if matches!(self.layout, Layout::SoloMid { .. }) => {
                self.team_scores[usize::from(1 - target.team)].armories_destroyed == 0
            }
            _ => false,
        }
    }

    fn armory_protected(&self, node: &Entity) -> bool {
        if !matches!(self.layout, Layout::SoloMid { .. }) {
            return false;
        }
        // Static actor order is home top/mid/bottom, then away top/mid/bottom.
        const BASE_TURRETS: [u32; 6] = [2051, 2065, 2079, 2093, 2107, 2121];
        let Some(slot) = node.id.checked_sub(2000 - 1499) else {
            return false;
        };
        let Some(&wire) = BASE_TURRETS.get(slot as usize) else {
            return false;
        };
        self.index(wire - 1499).is_some_and(|i| {
            let tower = &self.entities[i];
            tower.team == node.team
                && tower.kind == Kind::Tower
                && tower.type_id == EntityTemplate::ArmoryTurret5v5.as_raw()
                && tower.hp > 0.
        })
    }

    pub(crate) fn living_armories(&self, team: u8) -> usize {
        let first = 501 + 3 * u32::from(team);
        (first..first + 3)
            .filter(|&id| self.index(id).is_some_and(|i| self.entities[i].hp > 0.))
            .count()
    }

    pub(crate) fn advance_armory(&mut self, i: usize, events: &mut Vec<Event>) {
        let e = &mut self.entities[i];
        if e.hp >= e.max_hp || self.state.tick < e.respawn {
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
mod armory_tests {
    use super::*;

    #[test]
    fn only_the_same_side_and_lane_base_turret_protects_each_node() {
        for (wire_node, wire_tower) in [
            (2000, 2051),
            (2001, 2065),
            (2002, 2079),
            (2003, 2093),
            (2004, 2107),
            (2005, 2121),
        ] {
            let mut room = World::new(23, 10);
            room.use_solo_mid_lane().unwrap();
            let node = room.index(wire_node - 1499).unwrap();
            let guard = room.index(wire_tower - 1499).unwrap();
            let team = 1 - room.entities[node].team;
            let source = room
                .entities
                .iter()
                .find(|e| e.kind == Kind::Hero && e.team == team)
                .unwrap()
                .id;
            for i in 0..room.entities.len() {
                if i != guard && room.entities[i].kind == Kind::Tower {
                    room.entities[i].hp = 0.;
                }
            }
            let mut events = Vec::new();
            room.hurt(source, team, node, 20., DamageSchool::True, &mut events);
            assert!(events.is_empty());
            assert_eq!(
                (room.entities[node].hp, room.entities[node].respawn),
                (4350., 0)
            );
            room.entities[guard].hp = 0.;
            room.hurt(source, team, node, 20., DamageSchool::True, &mut events);
            assert_eq!(room.entities[node].hp, 4330.);
            assert!(matches!(events.as_slice(), [Event::Damage { .. }]));
        }
    }
    #[test]
    fn first_armory_breach_opens_only_its_own_crystal_to_damage() {
        for (team, crystal, opposite, node, turret) in
            [(0, 2007, 2006, 2004, 2107), (1, 2006, 2007, 2001, 2065)]
        {
            let mut w = World::new(0, 10);
            w.use_solo_mid_lane().unwrap();
            let source = w
                .entities
                .iter()
                .find(|e| e.kind == Kind::Hero && e.team == team)
                .unwrap()
                .id;
            let crystal = w.index(crystal - 1499).unwrap();
            let other = w.index(opposite - 1499).unwrap();
            let node = w.index(node - 1499).unwrap();
            let turret = w.index(turret - 1499).unwrap();
            let hp = w.entities[crystal].hp;
            let mut events = vec![];
            w.hurt(source, team, crystal, hp, DamageSchool::True, &mut events);
            assert!(events.is_empty());
            assert_eq!(w.entities[crystal].hp, hp);
            assert!(w.winner.is_none());
            w.hurt(
                source,
                team,
                turret,
                100_000.,
                DamageSchool::True,
                &mut events,
            );
            events.clear();
            w.hurt(source, team, crystal, hp, DamageSchool::True, &mut events);
            assert!(events.is_empty());
            w.hurt(
                source,
                team,
                node,
                100_000.,
                DamageSchool::True,
                &mut events,
            );
            events.clear();
            w.hurt(
                source,
                team,
                crystal,
                20.,
                DamageSchool::Weapon,
                &mut events,
            );
            assert_eq!(w.entities[crystal].hp, hp - 20.);
            assert!(matches!(events.as_slice(), [Event::Damage { .. }]));
            events.clear();
            let enemy = w
                .entities
                .iter()
                .find(|e| e.kind == Kind::Hero && e.team != team)
                .unwrap()
                .id;
            w.hurt(
                enemy,
                1 - team,
                other,
                50.,
                DamageSchool::Crystal,
                &mut events,
            );
            assert!(events.is_empty());
            w.tick = w.entities[node].respawn;
            w.advance_armory(node, &mut events);
            assert_eq!(w.entities[node].hp, w.entities[node].max_hp);
            w.hurt(
                source,
                team,
                crystal,
                25.,
                DamageSchool::Crystal,
                &mut events,
            );
            assert_eq!(w.entities[crystal].hp, hp - 45.);
        }
    }

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
        for wire in [2093, 2107, 2121] {
            let guard = world.index(wire - 1499).unwrap();
            world.entities[guard].hp = 0.;
        }
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

#[cfg(test)]
mod phase_cost {
    use super::*;
    use std::{hint::black_box, path::Path, time::Instant};

    #[test]
    #[ignore = "release microbenchmark with original delivery and supplemental data"]
    fn effect_phase_cost() {
        let data = GameData::load_with_extra(
            std::env::var_os("HALCYON_DELIVERY").unwrap(),
            Some(Path::new(&std::env::var_os("HALCYON_DATA_EXTRA").unwrap())),
        )
        .unwrap();
        let mut w = crate::heroes::native::effects_replay::field_replay_world(&data);
        for i in 0..w.entities.len().min(6) {
            let a = &mut w.entities[i];
            a.position = Position {
                x: i as f32 * 0.6,
                y: 0.,
            };
            a.max_hp = 4000.;
            a.hp = 2000.;
        }
        let mut events = vec![];
        let mut costs = Vec::new();
        let mut event_count = 0u64;
        for block in 0..25 {
            let mut ns = 0;
            for index in 0..1200 {
                w.tick += 1;
                if index % 100 == 0 {
                    for idx in 0..w.entities.len().min(6) {
                        let a = &mut w.entities[idx];
                        a.hp = 2000.;
                    }
                    w.gift_of_fire(1, 1, 0, &mut events);
                    w.gift_of_fire(3, 3, 0, &mut events);
                    w.create_bulwark(0, 0, &mut events);
                    w.create_bulwark(2, 0, &mut events);
                }
                let t = Instant::now();
                w.advance_spell_family::<crate::heroes::native::bulwark::Field>(&mut events);
                w.advance_spell_family::<crate::heroes::native::arcane_fire::Pulse>(&mut events);
                ns += t.elapsed().as_nanos();
                event_count += black_box(events.len()) as u64;
                events.clear();
            }
            if block > 0 {
                costs.push(ns / 1200);
            }
        }
        assert_eq!(event_count, 13_200);
        assert_eq!(
            w.state_digest(),
            "ca376a3efa419bf364c4d5a2cb5ebf23d9ef1f99f74ced77908fde8fbb9f63c8"
        );
        costs.sort_unstable();
        println!(
            "phase median_block_ns {} min {} max {} events {} final {}",
            costs[12],
            costs[0],
            costs[23],
            event_count,
            w.state_digest()
        );
    }
}
