//! Deterministic straight-lane validation map. Coordinates are project world units.
//! Vision, spawn offsets, windup, attack cadence, retreat and respawn are project rules.
mod ai;
mod aral;
mod aral_inventory;
pub use aral_inventory::{ARAL_ITEM_CAPACITY, AralInventory, AralItem};
mod chakram;
mod combat;
mod layout;
mod lorelai;
mod native_learn;
mod nav;
use layout::Layout;
mod progression;
mod score;
mod shop;
use combat::{AbilityBook, Cast, SpellEffect, Status};
pub use combat::{AbilitySlot, CastTarget};
use halcyon_data::{GameData, Unit};
pub use native_learn::{known_native_learn_operand, native_learn_operand, native_learn_request};
use score::DamageSchool;
pub use score::{PlayerScore, TeamScore};
use serde::{Deserialize, Serialize};
pub use shop::{BasicItem, HeldItem, ItemStats, Purchase};

/// Different simulation sources or content packs receive distinct federation queue keys.
pub fn rules_digest(data: Option<&GameData>) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for source in [
        include_str!("lib.rs"),
        include_str!("combat.rs"),
        include_str!("chakram.rs"),
        include_str!("lorelai.rs"),
        include_str!("native_learn.rs"),
        include_str!("progression.rs"),
        include_str!("ai.rs"),
        include_str!("aral.rs"),
        include_str!("nav.rs"),
        include_str!("layout.rs"),
        include_str!("shop.rs"),
        include_str!("score.rs"),
    ] {
        let normalized = source.replace("\r\n", "\n");
        h.update((normalized.len() as u64).to_be_bytes());
        h.update(normalized.as_bytes());
    }
    h.update(data.map_or("built-in", |d| &d.content_digest));
    format!("{:x}", h.finalize())
}

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
    Cast {
        slot: AbilitySlot,
        target: CastTarget,
    },
    Learn(AbilitySlot),
    Buy(BasicItem),
    Stop,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Event {
    Learned {
        source: u32,
        slot: AbilitySlot,
    },
    Purchase(Purchase),
    GoldAward {
        id: u32,
        amount: u32,
    },
    XpGain {
        id: u32,
        amount: f32,
    },
    Spawn {
        id: u32,
    },
    /// A previously dead hero has returned to its home position.
    Respawn {
        id: u32,
        position: Position,
    },
    Move {
        id: u32,
        position: Position,
    },
    Attack {
        source: u32,
        target: u32,
    },
    /// A native NPC projectile has launched and now owns a pending impact.
    NpcProjectile {
        id: u32,
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
    Cast {
        source: u32,
        slot: AbilitySlot,
        target: CastTarget,
    },
    Heal {
        source: u32,
        target: u32,
        amount: f32,
        hp: f32,
    },
    Level {
        id: u32,
        level: u8,
    },
    End {
        winner: u8,
    },
}
#[derive(Clone, Copy, Debug, Serialize)]
struct Stats {
    id: u16,
    hp: f32,
    weapon: f32,
    armor: f32,
    range: f32,
    speed: f32,
    energy: f32,
    shield: f32,
    growth: Growth,
}
#[derive(Clone, Copy, Debug, Default, Serialize)]
struct Growth {
    hp: f32,
    energy: f32,
    weapon: f32,
    armor: f32,
    shield: f32,
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
            energy: if id == 243 { 163. } else { 0. },
            shield: if id == 243 { 20. } else { armor },
            growth: if id == 243 {
                Growth {
                    hp: 127.64,
                    energy: 23.,
                    weapon: 6.,
                    armor: 4.546,
                    shield: 3.182,
                }
            } else {
                let (hp, weapon) = match id {
                    335 => (30., 5.),
                    336 => (53., 4.),
                    337 => (142., 5.),
                    338 => (169., 5.),
                    _ => (0., 0.),
                };
                Growth {
                    hp,
                    energy: 0.,
                    weapon,
                    armor: 0.,
                    shield: 0.,
                }
            },
        }
    }
}
impl Stats {
    fn load(id: u16, u: &Unit) -> Self {
        Self {
            id,
            hp: u.hp,
            energy: u.energy,
            weapon: u.weapon,
            armor: u.armor,
            shield: u.shield,
            range: u.range,
            speed: u.move_speed,
            growth: Growth {
                hp: u.hp_per_level,
                energy: u.energy_per_level,
                weapon: u.weapon_per_level,
                armor: u.armor_per_level,
                shield: u.shield_per_level,
            },
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
    pub energy: f32,
    pub max_energy: f32,
    pub level: u8,
    pub xp: f32,
    pub skill_points: u8,
    pub inventory: Vec<HeldItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aral_inventory: Option<Box<AralInventory>>,
    pub item_stats: ItemStats,
    #[serde(skip)]
    bonus_hp: f32,
    pub ability_ranks: [u8; 3],
    #[serde(skip)]
    cast: Option<Cast>,
    #[serde(skip)]
    cooldown: [u64; 3],
    #[serde(skip)]
    status: Status,
    #[serde(skip)]
    path: nav::Path,
    #[serde(skip)]
    last_seen: Option<(u32, Position, u64)>,
    // Last waypoint actually stepped toward by this lane minion. A pursued
    // actor is retained so network projections can filter its position by sight.
    #[serde(skip)]
    lane_step: Option<(Position, Option<u32>)>,
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
    layout: Layout,
    aral_units: aral::Units,
    solo_inner_towers: [Stats; 2],
    next_id: u32,
    next_item: u32,
    seed: u64,
    bot_difficulty: Option<halcyon_data::BotDifficulty>,
    first_wave: u64,
    wave_interval: u64,
    abilities: AbilityBook,
    shop: shop::Shop,
    effects: Vec<SpellEffect>,
    chakrams: Vec<chakram::Chakram>,
    water: Vec<lorelai::Water>,
    nav_budget: u8,
    first_blood: bool,
    scores: [PlayerScore; 10],
    team_scores: [TeamScore; 2],
    rally: [Option<Position>; 2],
}
impl Entity {
    pub fn lane_step(&self) -> Option<(Position, Option<u32>)> {
        self.lane_step
    }
    pub fn attack_target(&self) -> Option<u32> {
        match self.order {
            Command::Attack(target) if self.hp > 0. => Some(target),
            _ => None,
        }
    }

    pub fn weapon_power(&self) -> f32 {
        self.stats.weapon
    }
    pub fn armor(&self) -> f32 {
        self.stats.armor
    }
    pub fn shield(&self) -> f32 {
        self.stats.shield
    }
}

fn base(team: u8) -> Position {
    Position {
        x: if team == 0 { 0. } else { LANE },
        y: 0.,
    }
}
impl World {
    /// Hash both visible and hidden authoritative state for same-build replay checkpoints.
    pub fn state_digest(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut hash = Sha256::new();
        serde_json::to_writer(
            &mut hash,
            &(
                self.tick,
                self.winner,
                self.seed,
                self.next_id,
                self.next_item,
                self.first_wave,
                self.wave_interval,
                (&self.stats, &self.layout, &self.aral_units),
                &self.abilities,
                &self.shop,
                self.first_blood,
                (&self.scores, &self.team_scores),
                self.rally,
                self.nav_budget,
                (&self.effects, &self.chakrams, &self.water),
            ),
        )
        .expect("finite simulation state");
        if let Some(difficulty) = self.bot_difficulty {
            hash.update(difficulty.as_str().as_bytes());
        }
        for e in &self.entities {
            serde_json::to_writer(
                &mut hash,
                &(
                    e,
                    e.stats,
                    e.order,
                    e.next_attack,
                    e.winding,
                    e.respawn,
                    e.cast,
                    e.cooldown,
                    e.status,
                    &e.path,
                    e.last_seen,
                    e.bonus_hp,
                ),
            )
            .expect("finite simulation entity");
        }
        format!("{:x}", hash.finalize())
    }
    pub fn set_bot_difficulty(&mut self, difficulty: Option<halcyon_data::BotDifficulty>) {
        self.bot_difficulty = difficulty;
    }

    fn decision_interval(&self, entity: &Entity) -> u64 {
        if entity.bot && entity.kind == Kind::Hero {
            self.bot_difficulty
                .map_or(5, halcyon_data::BotDifficulty::decision_ticks)
        } else {
            5
        }
    }

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
            Ok::<_, String>(Stats::load(id, unit))
        });
        let stats: Vec<_> = units.into_iter().collect::<Result<_, _>>()?;
        let timing = |key: &str| {
            data.official_rules["five_v_five"]["waves"][key]
                .as_u64()
                .and_then(|v| v.checked_mul(TICK_HZ))
                .filter(|n| *n > 0)
                .ok_or_else(|| format!("invalid wave timing {key}"))
        };
        let mut world = Self::build(
            seed,
            human_seats,
            stats.try_into().expect("eight unit types"),
            timing("first_spawn_s")?,
            timing("interval_s")?,
        );
        world.aral_units.load(data);
        world.load_solo_towers(data);
        world.abilities = AbilityBook::from_data(data)?;
        world.shop = shop::Shop::load(data)?;
        Ok(world)
    }
    /// Remove the synthetic hero slots from a full six-human roster. Stable
    /// actor IDs for the humans and structures remain unchanged.
    pub fn use_six_human_roster(&mut self) -> Result<(), String> {
        if self.tick != 0
            || !matches!(self.layout, Layout::Validation)
            || self
                .entities
                .iter()
                .filter(|actor| actor.kind == Kind::Hero && !actor.bot)
                .count()
                != 6
        {
            return Err("six-human roster required before simulation".into());
        }
        self.entities
            .retain(|actor| actor.kind != Kind::Hero || !actor.bot);
        Ok(())
    }

    /// Apply team slots from the reserved roster before the first simulation step.
    pub fn assign_teams(&mut self, human_teams: &[u8]) -> Result<(), String> {
        if self.tick != 0 || !matches!(self.layout, Layout::Validation) {
            return Err("team assignment closed".into());
        }
        let heroes = self
            .entities
            .iter()
            .take_while(|e| e.kind == Kind::Hero)
            .count();
        let humans = self.entities.iter().take(heroes).filter(|e| !e.bot).count();
        let team_capacity = heroes / 2;
        let mut teams = [0u8; 10];
        let mut counts = [0usize; 2];
        if !human_teams.is_empty() && human_teams.len() != humans {
            return Err("incorrect human team count".into());
        }
        for (i, team) in teams.iter_mut().take(heroes).enumerate() {
            let requested = human_teams.get(i).copied().unwrap_or((i % 2) as u8);
            if requested > 1 {
                return Err("unknown team".into());
            }
            let selected = if counts[requested as usize] < team_capacity {
                requested
            } else if i >= humans {
                1 - requested
            } else {
                return Err("too many players on team".into());
            };
            *team = selected;
            counts[selected as usize] += 1;
        }
        for (e, team) in self.entities.iter_mut().take(heroes).zip(teams) {
            e.team = team;
            e.position.x = base(team).x;
        }
        Ok(())
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
        hero.stats = Stats::load(id, unit);
        hero.energy = unit.energy;
        hero.max_energy = unit.energy;
        hero.type_id = id;
        hero.max_hp = unit.hp;
        hero.hp = unit.hp;
        hero.apply_growth(hero.level - 1);
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
            layout: Layout::default(),
            aral_units: aral::Units::default(),
            solo_inner_towers: layout::solo_inner_towers(),
            next_item: 40000,
            stats,
            seed,
            bot_difficulty: None,
            first_wave,
            wave_interval,
            abilities: AbilityBook::default(),
            shop: Default::default(),
            effects: Vec::new(),
            chakrams: Vec::new(),
            water: Vec::new(),
            nav_budget: 0,
            first_blood: false,
            scores: Default::default(),
            team_scores: Default::default(),
            rally: [None; 2],
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
        self.spawn_with_stats(team, kind, self.stats[slot], position, bot)
    }

    fn spawn_with_stats(
        &mut self,
        team: u8,
        kind: Kind,
        s: Stats,
        position: Position,
        bot: bool,
    ) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        let starting_gold = 0;
        if kind == Kind::Hero {
            self.scores[(id - 1) as usize].gold_income.starting_balance = starting_gold;
        }
        self.entities.push(Entity {
            id,
            team,
            kind,
            type_id: s.id,
            position,
            hp: s.hp,
            max_hp: s.hp,
            gold: starting_gold,
            bot,
            inventory: Vec::new(),
            aral_inventory: None,
            item_stats: ItemStats::default(),
            bonus_hp: 0.,
            level: 1,
            xp: 0.,
            skill_points: u8::from(kind == Kind::Hero),
            ability_ranks: [0; 3],
            energy: s.energy,
            max_energy: s.energy,
            cast: None,
            cooldown: [0; 3],
            status: Status::default(),
            last_seen: None,
            lane_step: None,
            path: Default::default(),
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
                && self.entities.iter().any(|a| {
                    a.team == team
                        && a.hp > 0.
                        && (a.position.x - e.position.x).abs() <= SIGHT
                        && a.position.dist(e.position) <= SIGHT
                        && self.layout.clear(a.position, e.position)
                }))
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
        if e.status.stun_until > self.tick
            && matches!(
                cmd,
                Command::MoveTo(_) | Command::Attack(_) | Command::Cast { .. }
            )
        {
            return Err("stunned");
        }
        match cmd {
            Command::MoveTo(p) if !self.layout.walkable(p) => {
                return Err("outside lane");
            }
            Command::Buy(item) => return self.buy(i, item).map(|_| ()),
            Command::Learn(slot) => return self.learn(i, slot),
            Command::Cast { slot, target } => return self.begin_cast(i, slot, target),
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
        self.entities[i].cast = None;
        self.entities[i].status.channel = 0;
        self.entities[i].status.channel_until = 0;
        if self.entities[i].order != cmd {
            self.entities[i].winding = None;
        }
        self.entities[i].order = cmd;
        Ok(())
    }
    fn walk(&mut self, i: usize, p: Position, events: &mut Vec<Event>) {
        let e = &mut self.entities[i];
        if e.stats.speed <= 0. {
            return;
        }
        let next = match &self.layout {
            Layout::Validation => e.path.next(e.position, p, &mut self.nav_budget),
            Layout::SoloMid { .. } | Layout::Aral { .. } => {
                self.layout.clear(e.position, p).then_some(p)
            }
        };
        let Some(waypoint) = next else {
            return;
        };
        let pace = if e.status.slow_until > self.tick {
            e.status.slow_factor
        } else {
            1.
        }
        .min(if e.status.water_slow_until > self.tick {
            e.status.water_slow_factor
        } else {
            1.
        }) * if e.status.haste_until.max(e.status.water_speed_until) > self.tick {
            1.25
        } else {
            1.
        };
        let pos = e.position.advance(
            waypoint,
            e.stats.speed * (1. + e.item_stats.move_speed_amp) * pace / TICK_HZ as f32,
        );
        if pos != e.position {
            if e.kind == Kind::Minion {
                let followed = match e.order {
                    Command::Attack(id) => Some(id),
                    _ => None,
                };
                e.lane_step = Some((waypoint, followed));
            }
            e.position = pos;
            events.push(Event::Move {
                id: e.id,
                position: pos,
            })
        }
    }
    pub fn team_scores(&self) -> &[TeamScore; 2] {
        &self.team_scores
    }

    pub fn step(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        if self.winner.is_some() {
            return events;
        }
        self.tick += 1;
        self.step_tick(&mut events);
        if self.tick == 300 * TICK_HZ {
            self.record_milestone(true);
        } else if self.tick == 600 * TICK_HZ {
            self.record_milestone(false);
        }
        events
    }

    fn spawn_lane_minion(
        &mut self,
        team: u8,
        stats: Stats,
        position: Position,
        events: &mut Vec<Event>,
    ) {
        let id = self.spawn_with_stats(team, Kind::Minion, stats, position, false);
        let minion = self.entities.last_mut().unwrap();
        let level = (2 + self.tick.saturating_sub(120 * TICK_HZ) / (120 * TICK_HZ)).min(12) as u8;
        let extra = f32::from(level - 1);
        minion.level = level;
        minion.max_hp += minion.stats.growth.hp * extra;
        minion.hp = minion.max_hp;
        minion.stats.weapon += minion.stats.growth.weapon * extra;
        events.push(Event::Spawn { id });
    }
    fn step_tick(&mut self, events: &mut Vec<Event>) {
        self.nav_budget = 6;
        self.advance_effects(events);
        self.advance_chakrams(events);
        self.advance_water(events);
        if self.winner.is_some() {
            return;
        }
        if matches!(self.layout, Layout::Aral { .. }) {
            self.spawn_aral_wave(events);
        } else if matches!(self.layout, Layout::SoloMid { .. }) {
            self.spawn_solo_mid_wave(events);
        } else if self.tick >= self.first_wave
            && (self.tick - self.first_wave).is_multiple_of(self.wave_interval)
        {
            let wave = (self.tick - self.first_wave) / self.wave_interval + 1;
            for team in 0..2 {
                for (j, slot) in [4, 4, 5, if wave.is_multiple_of(4) { 7 } else { 6 }]
                    .into_iter()
                    .enumerate()
                {
                    self.spawn_lane_minion(
                        team,
                        self.stats[slot],
                        self.layout.wave(team, j),
                        events,
                    );
                }
            }
        }
        if self.tick.is_multiple_of(TICK_HZ) {
            self.native_passive_income(events);
            self.update_rally();
        }
        let n = self.entities.len();
        // Rotate priority when actions coincide. Stable indices and the match seed determine all ties.
        let first = (self.tick.wrapping_add(self.seed) % n as u64) as usize;
        for offset in 0..n {
            let i = (first + offset) % n;
            let e = &mut self.entities[i];
            e.lane_step = None;
            if e.hp <= 0. {
                if e.kind == Kind::Hero && e.respawn <= self.tick {
                    e.hp = e.max_hp;
                    e.energy = e.max_energy;
                    e.status = Default::default();
                    e.path = Default::default();
                    e.position = self.layout.home(e);
                    events.push(Event::Spawn { id: e.id });
                    events.push(Event::Respawn {
                        id: e.id,
                        position: e.position,
                    });
                }
                continue;
            }
            if e.kind == Kind::Hero && self.layout.at_base(e, false) {
                e.hp = (e.hp + e.max_hp * 0.003).min(e.max_hp);
            }
            if e.kind == Kind::Hero {
                e.energy = (e.energy
                    + e.item_stats.energy_regen / TICK_HZ as f32
                    + if self.layout.at_base(e, false) {
                        1.2
                    } else {
                        0.10
                    })
                .min(e.max_energy);
                if self.tick >= 35 * TICK_HZ
                    && self.tick.is_multiple_of(TICK_HZ)
                    && matches!(self.layout, Layout::Validation)
                {
                    self.award_xp(i, 0.45, events);
                }
            }
            let hero = &self.entities[i];
            if hero.status.stun_until > self.tick {
                continue;
            }
            if hero.bot
                && hero.hp < hero.max_hp * 0.14
                && (hero.cast.is_some()
                    || hero.winding.is_some()
                    || hero.status.channel_until >= self.tick)
            {
                let _ = self.order(i, Command::MoveTo(self.layout.home(hero)));
            }
            if self.entities[i].status.channel_until >= self.tick {
                continue;
            }
            if self.entities[i].cast.is_some() {
                self.advance_cast(i, events);
                continue;
            }
            let e = &self.entities[i];
            let automated = e.winding.is_none() && (e.bot || e.kind != Kind::Hero);
            let offset = self.tick + u64::from(e.id);
            let decide = automated && offset.is_multiple_of(self.decision_interval(e));
            if automated && e.bot && offset.is_multiple_of(5) {
                if e.gold >= 300 && e.inventory.len() < 6 && self.layout.at_base(e, true) {
                    let item = if e.inventory.len() == 2 {
                        BasicItem::LightArmor
                    } else {
                        BasicItem::WeaponBlade
                    };
                    if let Ok(receipt) = self.buy(i, item) {
                        events.push(Event::Purchase(receipt));
                    }
                }
                for slot in [AbilitySlot::C, AbilitySlot::A, AbilitySlot::B] {
                    if self.learn(i, slot).is_ok() {
                        events.push(Event::Learned {
                            source: self.entities[i].id,
                            slot,
                        });
                        break;
                    }
                }
            }
            if decide {
                let order = self.decide(i);
                if let Command::Attack(id)
                | Command::Cast {
                    target: CastTarget::Entity(id),
                    ..
                } = order
                    && let Some(j) = self.index(id)
                    && self.entities[j].team != self.entities[i].team
                {
                    self.entities[i].last_seen = Some((id, self.entities[j].position, self.tick));
                }
                let _ = self.order(i, order);
            }
            match self.entities[i].order {
                Command::Stop | Command::Buy(_) | Command::Learn(_) | Command::Cast { .. } => {}
                Command::MoveTo(p) => self.walk(i, p, events),
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
                    if e.position.dist(t.position) > e.stats.range
                        || !self.layout.clear(e.position, t.position)
                    {
                        let p = t.position;
                        self.entities[i].winding = None;
                        self.walk(i, p, events);
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
                    let bonus = if e.status.empowered > 0 {
                        e.status.empower_damage
                    } else {
                        0.
                    };
                    let school = if e.type_id == lorelai::HERO {
                        DamageSchool::Crystal
                    } else if e.kind == Kind::Crystal {
                        DamageSchool::True
                    } else {
                        DamageSchool::Weapon
                    };
                    let damage = if e.type_id == lorelai::HERO {
                        shop::mitigated(
                            10. + e.stats.weapon + bonus,
                            t.stats.shield,
                            e.item_stats.shield_pierce,
                        )
                    } else if e.kind == Kind::Crystal {
                        t.max_hp * 0.04
                    } else {
                        shop::mitigated(
                            e.stats.weapon + bonus,
                            t.stats.armor,
                            e.item_stats.armor_pierce,
                        )
                    };
                    let haste = e.status.haste_until > self.tick;
                    let crystal = e.kind == Kind::Crystal;
                    // Karas's 4.13 basic-attack coefficient is explicitly named in official_rules.
                    let karas_crystal = if e.type_id == 926 {
                        shop::mitigated(
                            e.item_stats.crystal * self.abilities.karas_basic_crystal_ratio,
                            t.stats.shield,
                            e.item_stats.shield_pierce,
                        )
                    } else {
                        0.
                    };
                    let npc_launch = self.npc_launch_delay(e);
                    let ranged = e.kind == Kind::Hero && e.stats.range >= 4.;
                    let travel = (e.position.dist(t.position) * TICK_HZ as f32 / 18.).ceil() as u64;
                    let interval =
                        (if haste { 10.0 } else { 17.0 }) / (1. + e.item_stats.attack_speed);
                    self.entities[i].winding = None;
                    self.entities[i].next_attack = self.tick + interval.ceil() as u64;
                    self.entities[i].status.empowered =
                        self.entities[i].status.empowered.saturating_sub(1);
                    events.push(Event::Attack { source, target: id });
                    if crystal {
                        self.entities[j].status.slow_until = self.tick + 20;
                        self.entities[j].status.slow_factor = 0.4;
                    }
                    if let Some(delay) = npc_launch {
                        self.effects.push(SpellEffect::npc_windup(
                            self.tick + delay,
                            source,
                            team,
                            id,
                            damage,
                        ));
                    } else if ranged {
                        self.effects.push(SpellEffect::impact(
                            self.tick + travel,
                            source,
                            team,
                            id,
                            damage,
                            school,
                        ));
                    } else {
                        self.hurt(source, team, j, damage, school, events);
                    }
                    if karas_crystal > 0. {
                        if ranged {
                            self.effects.push(SpellEffect::impact(
                                self.tick + travel,
                                source,
                                team,
                                id,
                                karas_crystal,
                                DamageSchool::Crystal,
                            ));
                        } else {
                            self.hurt(
                                source,
                                team,
                                j,
                                karas_crystal,
                                DamageSchool::Crystal,
                                events,
                            );
                        }
                    }
                    if self.winner.is_some() {
                        break;
                    }
                }
            }
        }
        self.entities
            .retain(|e| e.kind != Kind::Minion || e.hp > 0.);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visibility_matches_radial_check_at_the_sight_boundary_and_during_play() {
        fn baseline(world: &World, team: u8, entity: &Entity) -> bool {
            entity.team == team
                || (entity.hp > 0.
                    && world.entities.iter().any(|ally| {
                        ally.team == team
                            && ally.hp > 0.
                            && ally.position.dist(entity.position) <= SIGHT
                            && world.layout.clear(ally.position, entity.position)
                    }))
        }
        let mut world = World::new(1, 0);
        for _ in 0..2000 {
            world.step();
            if world.tick.is_multiple_of(50) {
                for entity in &world.entities {
                    for team in 0..2 {
                        assert_eq!(world.visible(team, entity), baseline(&world, team, entity));
                    }
                }
            }
        }
        let mut world = World::new(1, 0);
        world.entities.truncate(1);
        world.entities[0].position = Position { x: 0., y: 0. };
        let team = world.entities[0].team;
        let mut enemy = world.entities[0].clone();
        enemy.team = 1 - team;
        for x in [
            0.,
            SIGHT.next_down(),
            SIGHT,
            SIGHT.next_up(),
            -SIGHT,
            -SIGHT.next_up(),
            f32::MAX,
            f32::NAN,
        ] {
            for y in [0., 0.001, SIGHT, SIGHT.next_up()] {
                enemy.position = Position { x, y };
                assert_eq!(world.visible(team, &enemy), baseline(&world, team, &enemy));
            }
        }
    }

    #[test]
    fn only_the_hero_return_tick_emits_a_respawn_with_the_home_position() {
        let mut world = World::new(0, 1);
        world.entities[0].hp = 0.;
        world.entities[0].respawn = 2;
        world.entities[0].position = Position { x: 30., y: 4. };
        assert!(
            !world
                .step()
                .iter()
                .any(|e| matches!(e, Event::Respawn { .. }))
        );
        let events = world.step();
        let index = events
            .iter()
            .position(|e| *e == Event::Spawn { id: 1 })
            .unwrap();
        assert_eq!(
            events[index + 1],
            Event::Respawn {
                id: 1,
                position: base(0)
            }
        );
        assert_eq!(world.entities[0].hp, world.entities[0].max_hp);
        assert!(
            !world
                .step()
                .iter()
                .any(|e| matches!(e, Event::Respawn { .. }))
        );
    }

    #[test]
    fn difficulty_changes_only_hero_bot_decision_interval() {
        use halcyon_data::BotDifficulty::*;
        for (difficulty, first_decision) in [
            (None, 3),
            (Some(VeryEasy), 14),
            (Some(Easy), 8),
            (Some(Medium), 3),
            (Some(Hard), 1),
            (Some(VeryHard), 2),
        ] {
            let mut world = World::new(0, 1);
            let mut replay = World::new(0, 1);
            world.set_bot_difficulty(difficulty);
            replay.set_bot_difficulty(difficulty);
            // The first bot has entity id 2 and decides on its fixed, phased tick.
            let initial = world.entities[1].position;
            for tick in 1..=first_decision {
                assert_eq!(world.step(), replay.step());
                assert_eq!(world.state_digest(), replay.state_digest());
                let moved = world.entities[1].position.dist(initial) > 0.;
                assert_eq!(moved, tick == first_decision);
                assert_eq!(world.entities[0].order, Command::Stop);
                let structure = &world.entities[10];
                assert_eq!(world.decision_interval(structure), 5);
            }
        }
    }

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
            if a.tick.is_multiple_of(1000) {
                assert_eq!(a.state_digest(), b.state_digest());
            }
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
