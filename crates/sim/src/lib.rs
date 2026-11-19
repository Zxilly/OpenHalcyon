//! Deterministic straight-lane validation map. Coordinates are project world units.
//! Vision, spawn offsets, windup, attack cadence, retreat and respawn are project rules.
use halcyon_data::GameData;
use serde::{Deserialize, Serialize};

pub const TICK_HZ: u64 = 20;
const SIGHT: f32 = 14.;
const LANE: f32 = 60.;
const RESPAWN: u64 = 240;
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct Position {
    pub x: f32,
    pub y: f32,
}
impl Position {
    fn dist(self, b: Self) -> f32 {
        (self.x - b.x).hypot(self.y - b.y)
    }
    fn advance(self, b: Self, speed: f32) -> Self {
        let d = self.dist(b);
        if d <= speed {
            return b;
        }
        let t = speed / d;
        Self {
            x: self.x + (b.x - self.x) * t,
            y: self.y + (b.y - self.y) * t,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Hero,
    Minion,
    Tower,
    Crystal,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Command {
    MoveTo(Position),
    Attack(u32),
    Stop,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Event {
    Spawn {
        id: u32,
    },
    Move {
        id: u32,
        position: Position,
    },
    Attack {
        source: u32,
        target: u32,
    },
    Damage {
        source: u32,
        target: u32,
        amount: f32,
        hp: f32,
    },
    Death {
        id: u32,
        killer: u32,
    },
    End {
        winner: u8,
    },
}
#[derive(Clone, Copy, Debug)]
struct Stats {
    id: u16,
    hp: f32,
    weapon: f32,
    armor: f32,
    range: f32,
    speed: f32,
}
const NAMES: [&str; 8] = [
    "Ringo",
    "OuterTurret5v5",
    "VainCrystal_Home_5v5",
    "VainCrystal_Away_5v5",
    "5v5_Minion_Melee",
    "5v5_Minion_Range",
    "5v5_Minion_Siege",
    "5v5_Minion_Captain",
];
impl Stats {
    const fn new(id: u16, hp: f32, weapon: f32, armor: f32, range: f32, speed: f32) -> Self {
        Self {
            id,
            hp,
            weapon,
            armor,
            range,
            speed,
        }
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct Entity {
    pub id: u32,
    pub team: u8,
    pub kind: Kind,
    pub type_id: u16,
    pub position: Position,
    pub hp: f32,
    pub max_hp: f32,
    pub gold: u32,
    pub bot: bool,
    #[serde(skip)]
    stats: Stats,
    #[serde(skip)]
    order: Command,
    #[serde(skip)]
    next_attack: u64,
    #[serde(skip)]
    winding: Option<(u64, u32)>,
    #[serde(skip)]
    respawn: u64,
}
pub struct World {
    pub tick: u64,
    pub winner: Option<u8>,
    pub entities: Vec<Entity>,
    stats: [Stats; 8],
    next_id: u32,
    seed: u64,
    first_wave: u64,
    wave_interval: u64,
}
fn base(team: u8) -> Position {
    Position {
        x: if team == 0 { 0. } else { LANE },
        y: 0.,
    }
}
impl World {
    pub fn new(seed: u64, human_seats: u8) -> Self {
        // Confirmed delivery base numbers, usable without distributing extracted assets.
        let stats = [
            Stats::new(243, 703., 74., 25., 6.2, 3.6),
            Stats::new(340, 3000., 190., 100., 9., 0.),
            Stats::new(295, 20000., 0., 0., 22., 0.),
            Stats::new(296, 20000., 0., 0., 22., 0.),
            Stats::new(336, 575., 13., 0., 1.3, 3.5),
            Stats::new(335, 475., 15., 0., 4.5, 3.5),
            Stats::new(337, 880., 15., 0., 4., 3.5),
            Stats::new(338, 950., 15., 0., 5., 3.5),
        ];
        Self::build(seed, human_seats, stats, 900, 600)
    }
    pub fn from_data(seed: u64, human_seats: u8, data: &GameData) -> Result<Self, String> {
        let units = NAMES.map(|name| {
            let unit = data
                .unit(name)
                .ok_or_else(|| format!("missing unit {name}"))?;
            let id = data
                .type_id(name)
                .ok_or_else(|| format!("missing manifest entry {name}"))?;
            Ok::<_, String>(Stats::new(
                id,
                unit.hp,
                unit.weapon,
                unit.armor,
                unit.range,
                unit.move_speed,
            ))
        });
        let stats: Vec<_> = units.into_iter().collect::<Result<_, _>>()?;
        let timing = |key: &str| {
            data.official_rules["five_v_five"]["waves"][key]
                .as_u64()
                .and_then(|v| v.checked_mul(TICK_HZ))
                .filter(|n| *n > 0)
                .ok_or_else(|| format!("invalid wave timing {key}"))
        };
        Ok(Self::build(
            seed,
            human_seats,
            stats.try_into().expect("eight unit types"),
            timing("first_spawn_s")?,
            timing("interval_s")?,
        ))
    }
    /// Select the catalog hero for a zero-based roster seat during draft.
    pub fn select_hero(&mut self, seat: usize, name: &str, data: &GameData) -> Result<(), String> {
        if self.tick != 0 {
            return Err("draft has ended".into());
        }
        let hero = self
            .entities
            .get_mut(seat)
            .filter(|e| e.kind == Kind::Hero)
            .ok_or("invalid roster seat")?;
        if !(data.hero_names.iter().any(|h| h == name)
            || (data.hero_names.is_empty() && name == "Ringo"))
        {
            return Err(format!("unknown hero {name}"));
        }
        let unit = data
            .unit(name)
            .ok_or_else(|| format!("missing unit {name}"))?;
        let id = data
            .type_id(name)
            .ok_or_else(|| format!("missing manifest entry {name}"))?;
        hero.stats = Stats::new(
            id,
            unit.hp,
            unit.weapon,
            unit.armor,
            unit.range,
            unit.move_speed,
        );
        hero.type_id = id;
        hero.max_hp = unit.hp;
        hero.hp = unit.hp;
        Ok(())
    }
    fn build(
        seed: u64,
        human_seats: u8,
        stats: [Stats; 8],
        first_wave: u64,
        wave_interval: u64,
    ) -> Self {
        let mut w = Self {
            tick: 0,
            winner: None,
            entities: Vec::with_capacity(50),
            next_id: 1,
            stats,
            seed,
            first_wave,
            wave_interval,
        };
        for seat in 0..10 {
            let team = seat % 2;
            let jitter = ((seed.rotate_left(seat as u32 * 5) % 21) as f32 - 10.) / 20.;
            let mut p = base(team);
            p.y = seat as f32 / 3. - 1.5 + jitter;
            w.spawn(team, Kind::Hero, 0, p, seat >= human_seats);
        }
        for team in 0..2 {
            let p = base(team);
            w.spawn(team, Kind::Crystal, 2 + team as usize, p, false);
            w.spawn(
                team,
                Kind::Tower,
                1,
                Position {
                    x: if team == 0 { 12. } else { 48. },
                    y: 0.,
                },
                false,
            );
        }
        w
    }
    fn spawn(&mut self, team: u8, kind: Kind, slot: usize, position: Position, bot: bool) -> u32 {
        let s = self.stats[slot];
        let id = self.next_id;
        self.next_id += 1;
        self.entities.push(Entity {
            id,
            team,
            kind,
            type_id: s.id,
            position,
            hp: s.hp,
            max_hp: s.hp,
            gold: 0,
            bot,
            stats: s,
            order: Command::Stop,
            next_attack: 0,
            winding: None,
            respawn: 0,
        });
        id
    }
    fn index(&self, id: u32) -> Option<usize> {
        self.entities.binary_search_by_key(&id, |e| e.id).ok()
    }
    pub fn visible(&self, team: u8, e: &Entity) -> bool {
        e.team == team
            || (e.hp > 0.
                && self
                    .entities
                    .iter()
                    .any(|a| a.team == team && a.hp > 0. && a.position.dist(e.position) <= SIGHT))
    }
    pub fn command(&mut self, id: u32, cmd: Command) -> Result<(), &'static str> {
        if self.winner.is_some() {
            return Err("finished");
        }
        let i = self.index(id).ok_or("unknown entity")?;
        if self.entities[i].bot || self.entities[i].kind != Kind::Hero {
            return Err("not a human seat");
        }
        self.order(i, cmd)
    }
    fn order(&mut self, i: usize, cmd: Command) -> Result<(), &'static str> {
        let e = &self.entities[i];
        if e.hp <= 0. {
            return Err("dead");
        }
        match cmd {
            Command::MoveTo(p) if !(0.0..=LANE).contains(&p.x) || !(-5.0..=5.0).contains(&p.y) => {
                return Err("outside lane");
            }
            Command::Attack(id) => {
                let t = self
                    .index(id)
                    .map(|j| &self.entities[j])
                    .ok_or("unknown target")?;
                if t.team == e.team || t.hp <= 0. || !self.visible(e.team, t) {
                    return Err("invalid target");
                }
            }
            _ => {}
        }
        if self.entities[i].order != cmd {
            self.entities[i].winding = None;
        }
        self.entities[i].order = cmd;
        Ok(())
    }
    fn decide(&self, i: usize) -> Command {
        let e = &self.entities[i];
        let home = base(e.team);
        let enemy = base(1 - e.team);
        if e.kind == Kind::Hero && e.hp < e.max_hp * 0.23 {
            return Command::MoveTo(home);
        }
        let sensor = if matches!(e.kind, Kind::Tower | Kind::Crystal) {
            e.stats.range.min(SIGHT)
        } else {
            SIGHT
        };
        let mut candidate = None;
        let mut best = (u8::MAX, f32::MAX, u32::MAX);
        for t in &self.entities {
            if t.hp <= 0. || t.team == e.team {
                continue;
            }
            let d = e.position.dist(t.position);
            if d > sensor {
                continue;
            }
            let priority = match t.kind {
                Kind::Minion if e.kind == Kind::Hero && t.hp <= e.stats.weapon => 0,
                Kind::Minion if e.kind == Kind::Tower => 0,
                Kind::Tower | Kind::Crystal if e.kind == Kind::Minion => 0,
                Kind::Hero => 1,
                Kind::Minion => 2,
                Kind::Tower => 3,
                Kind::Crystal => 4,
            };
            let stick = if e.order == Command::Attack(t.id) {
                0.6
            } else {
                0.
            };
            let rank = (priority, d - stick, t.id);
            if rank < best {
                best = rank;
                candidate = Some(t);
            }
        }
        if let Some(t) = candidate {
            if e.kind == Kind::Hero
                && t.kind == Kind::Tower
                && !self.entities.iter().any(|a| {
                    a.team == e.team
                        && a.kind == Kind::Minion
                        && a.hp > 0.
                        && a.position.dist(t.position) <= t.stats.range
                })
            {
                let mut p = t.position;
                p.x += (if e.team == 0 { -1. } else { 1. }) * (t.stats.range + 1.);
                return Command::MoveTo(p);
            }
            if e.kind == Kind::Hero
                && t.kind == Kind::Hero
                && e.next_attack > self.tick + 4
                && e.position.dist(t.position) < 4.
            {
                return Command::MoveTo(e.position.advance(home, 1.));
            }
            return Command::Attack(t.id);
        }
        if e.stats.speed > 0. {
            Command::MoveTo(enemy)
        } else {
            Command::Stop
        }
    }
    fn walk(&mut self, i: usize, p: Position, events: &mut Vec<Event>) {
        let e = &mut self.entities[i];
        let pos = e.position.advance(p, e.stats.speed / TICK_HZ as f32);
        if pos != e.position {
            e.position = pos;
            events.push(Event::Move {
                id: e.id,
                position: pos,
            })
        }
    }
    pub fn step(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        if self.winner.is_some() {
            return events;
        }
        self.tick += 1;
        if self.tick >= self.first_wave
            && (self.tick - self.first_wave).is_multiple_of(self.wave_interval)
        {
            let wave = (self.tick - self.first_wave) / self.wave_interval + 1;
            for team in 0..2 {
                for (j, slot) in [4, 4, 5, if wave.is_multiple_of(4) { 7 } else { 6 }]
                    .into_iter()
                    .enumerate()
                {
                    let mut p = base(team);
                    p.y = j as f32 - 1.5;
                    let id = self.spawn(team, Kind::Minion, slot, p, false);
                    events.push(Event::Spawn { id });
                }
            }
        }
        let n = self.entities.len();
        // Rotate priority when actions coincide. Stable indices and the match seed determine all ties.
        let first = (self.tick.wrapping_add(self.seed) % n as u64) as usize;
        for offset in 0..n {
            let i = (first + offset) % n;
            let e = &mut self.entities[i];
            if e.hp <= 0. {
                if e.kind == Kind::Hero && e.respawn <= self.tick {
                    e.hp = e.max_hp;
                    e.position = base(e.team);
                    events.push(Event::Spawn { id: e.id });
                }
                continue;
            }
            if e.kind == Kind::Hero && e.position.dist(base(e.team)) < 3. {
                e.hp = (e.hp + e.max_hp * 0.003).min(e.max_hp);
            }
            if e.winding.is_none()
                && (e.bot || e.kind != Kind::Hero)
                && (self.tick + e.id as u64).is_multiple_of(5)
            {
                let order = self.decide(i);
                let _ = self.order(i, order);
            }
            match self.entities[i].order {
                Command::Stop => {}
                Command::MoveTo(p) => self.walk(i, p, &mut events),
                Command::Attack(id) => {
                    let Some(j) = self.index(id) else {
                        self.entities[i].winding = None;
                        self.entities[i].order = Command::Stop;
                        continue;
                    };
                    let e = &self.entities[i];
                    let t = &self.entities[j];
                    if !self.visible(e.team, t) || t.hp <= 0. {
                        self.entities[i].order = Command::Stop;
                        self.entities[i].winding = None;
                        continue;
                    }
                    if e.position.dist(t.position) > e.stats.range {
                        let p = t.position;
                        self.entities[i].winding = None;
                        self.walk(i, p, &mut events);
                        continue;
                    }
                    if self.tick < e.next_attack {
                        continue;
                    }
                    if e.winding.is_none() {
                        self.entities[i].winding = Some((self.tick + 3, id));
                        continue;
                    }
                    if e.winding.unwrap().0 > self.tick {
                        continue;
                    }
                    let source = e.id;
                    let team = e.team;
                    let damage = if e.kind == Kind::Crystal {
                        t.max_hp * 0.04
                    } else {
                        e.stats.weapon * 100. / (100. + t.stats.armor)
                    };
                    self.entities[i].winding = None;
                    self.entities[i].next_attack = self.tick + 17;
                    let t = &mut self.entities[j];
                    t.hp = (t.hp - damage).max(0.);
                    events.push(Event::Attack { source, target: id });
                    events.push(Event::Damage {
                        source,
                        target: id,
                        amount: damage,
                        hp: t.hp,
                    });
                    if t.hp == 0. {
                        let kind = t.kind;
                        let bounty = match kind {
                            Kind::Hero => 250,
                            Kind::Tower => 300,
                            Kind::Crystal => 0,
                            Kind::Minion => match t.type_id {
                                335 => 25,
                                337 => 60,
                                338 => 100,
                                _ => 35,
                            },
                        };
                        t.order = Command::Stop;
                        t.winding = None;
                        t.respawn = self.tick + RESPAWN + (self.tick / 60).min(900);
                        if self.entities[i].kind == Kind::Hero {
                            self.entities[i].gold += bounty;
                        }
                        events.push(Event::Death { id, killer: source });
                        if kind == Kind::Crystal {
                            self.winner = Some(team);
                            events.push(Event::End { winner: team });
                            break;
                        }
                    }
                }
            }
        }
        self.entities
            .retain(|e| e.kind != Kind::Minion || e.hp > 0.);
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_commands_validate_authority_and_fog() {
        let mut w = World::new(3, 1);
        assert!(w.command(1, Command::Attack(2)).is_err());
        assert!(w.command(2, Command::Stop).is_err());
        assert!(
            w.command(1, Command::MoveTo(Position { x: f32::NAN, y: 0. }))
                .is_err()
        );
        w.command(1, Command::MoveTo(Position { x: 6., y: 0. }))
            .unwrap();
        w.step();
        assert!(w.entities[0].position.x > 0.);
    }

    #[test]
    fn losing_contact_cancels_attack_before_release() {
        let mut w = World::new(3, 10);
        w.entities[1].position = Position { x: 4., y: 0. };
        w.command(1, Command::Attack(2)).unwrap();
        w.step();
        assert!(w.entities[0].winding.is_some());
        w.entities[1].position = base(1);
        w.step();
        assert_eq!(w.entities[0].order, Command::Stop);
        assert!(w.entities[0].winding.is_none());
        assert_eq!(w.entities[1].hp, w.entities[1].max_hp);
    }

    #[test]
    fn ringo_last_hits_and_waits_for_wave_before_tower() {
        let mut w = World::new(0, 0);
        w.entities[0].position = Position { x: 35., y: 0. };
        assert!(matches!(w.decide(0),Command::MoveTo(p) if p.x < 48.-9.));
        let id = w.spawn(1, Kind::Minion, 4, Position { x: 38., y: 0. }, false);
        w.entities.last_mut().unwrap().hp = 10.;
        assert_eq!(w.decide(0), Command::Attack(id));
        w.entities[0].hp = 50.;
        assert_eq!(w.decide(0), Command::MoveTo(base(0)));
    }

    #[test]
    fn same_seed_replays_each_event_until_crystal_death() {
        let mut a = World::new(2, 0);
        let mut b = World::new(2, 0);
        for _ in 0..48000 {
            let events = a.step();
            assert_eq!(events, b.step());
            if let Some(team) = a.winner {
                assert_eq!(events.last(), Some(&Event::End { winner: team }));
                assert!(
                    a.entities
                        .iter()
                        .any(|e| e.kind == Kind::Crystal && e.team != team && e.hp == 0.)
                );
                assert!(a.step().is_empty());
                return;
            }
        }
        panic!("crystal survived");
    }
}
