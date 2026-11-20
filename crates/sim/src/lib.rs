//! Deterministic straight-lane validation map. Coordinates are project world units.
//! Vision, spawn offsets, windup, attack cadence, retreat and respawn are project rules.
use halcyon_data::EntityTemplate;
mod ai;
mod aral;
mod arcane_fire;
#[cfg(feature = "ecs-visibility-bench")]
pub mod ecs_experiment;
mod vision_devices;
pub use vision_devices::{VisionDevice, VisionDeviceKind};
mod aral_inventory;
mod armory;
mod bot_diagnostics;
pub use aral_inventory::{ARAL_ITEM_CAPACITY, AralInventory, AralItem, AralStarter};
pub use bot_diagnostics::{BotDiagnostics, BotHeroDiagnostics};
mod bulwark;
#[cfg(test)]
mod captain_tests;
mod catherine;
mod chakram;
mod combat;
mod layout;
mod lorelai;
mod native_camps;
mod native_combat;
mod native_learn;
mod native_navigation;
mod nav;
use layout::Layout;
mod overwhelm;
mod progression;
mod pursuit;
mod score;
mod shop;
mod solo_route;
mod solo_side;
mod world_rules;
pub use world_rules::WorldRules;
mod sigil;
mod twister;
mod yates;
use combat::{AbilityBook, Cast, SpellEffect, Status};
pub use combat::{AbilitySlot, CastTarget};
use halcyon_data::{GameData, Unit};
pub use native_learn::{known_native_learn_operand, native_learn_operand, native_learn_request};
use score::DamageSchool;
pub use score::{PlayerScore, TeamScore};
use serde::{Deserialize, Serialize};
pub use shop::{BasicItem, HeldItem, ItemSale, ItemStats, NativeItemStat, Purchase};

/// Different simulation sources or content packs receive distinct federation queue keys.
pub fn rules_digest(data: Option<&GameData>) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(include_bytes!(concat!(
        env!("OUT_DIR"),
        "/simulation.sha256"
    )));
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
    Armory,
    Neutral,
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
    Sell {
        instance: u32,
    },
    DeployVisionTotem {
        instance: u32,
        point: Position,
    },
    DeployScoutTrap {
        instance: u32,
    },
    HitScoutCam {
        device: u32,
    },
    ConsumeInfusion {
        instance: u32,
    },
    ActivateHealingFlask {
        instance: u32,
    },
    ActivateSprintBoots {
        instance: u32,
    },
    Stop,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImpactEffect {
    #[default]
    Standard,
    ChillWinds,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Event {
    VisionDeployed {
        device: VisionDevice,
        instance: u32,
    },
    ChillWindsRoot {
        source: u32,
        target: u32,
        instance: u32,
    },
    ChillWinds {
        source: u32,
        position: Position,
        heading: Option<f32>,
    },
    /// An ARAL Ringo last hit armed the perk, or its next released basic consumed it.
    DoubleDown {
        source: u32,
        instance: u32,
        attack_target: Option<u32>,
    },
    ChakramApplied {
        source: u32,
        id: u32,
    },
    WaterwallApplied {
        source: u32,
        target: u32,
        id: u32,
    },
    Learned {
        source: u32,
        slot: AbilitySlot,
    },
    Purchase(Purchase),
    ItemSold(ItemSale),
    FlaskActivated {
        actor: u32,
        instance: u32,
    },
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
    /// Native ARAL presentation only. The hero is still dead.
    RespawnApproach {
        id: u32,
        team: u8,
        home: Position,
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
        #[serde(default)]
        effect: ImpactEffect,
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
    physiology: Option<Physiology>,
}
#[derive(Clone, Copy, Debug, Serialize)]
struct Physiology {
    hp_regen: f32,
    hp_regen_growth: f32,
    energy_regen: f32,
    energy_regen_growth: f32,
    attack_speed: f32,
    attack_speed_growth: f32,
    collision_radius: f32,
    sight: f32,
    gold_bounty: f32,
    xp_bounty: f32,
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
    const fn new(
        template: EntityTemplate,
        hp: f32,
        weapon: f32,
        armor: f32,
        range: f32,
        speed: f32,
    ) -> Self {
        Self {
            id: template.as_raw(),
            hp,
            weapon,
            armor,
            range,
            speed,
            energy: if matches!(template, EntityTemplate::Ringo) {
                163.
            } else {
                0.
            },
            shield: if matches!(template, EntityTemplate::Ringo) {
                20.
            } else {
                armor
            },
            physiology: None,
            growth: if matches!(template, EntityTemplate::Ringo) {
                Growth {
                    hp: 127.64,
                    energy: 23.,
                    weapon: 6.,
                    armor: 4.546,
                    shield: 3.182,
                }
            } else {
                let (hp, weapon) = match template {
                    EntityTemplate::MinionRange5v5 => (30., 5.),
                    EntityTemplate::MinionMelee5v5 => (53., 4.),
                    EntityTemplate::MinionSiege5v5 => (142., 5.),
                    EntityTemplate::MinionCaptain5v5 => (169., 5.),
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
    fn sight(self) -> f32 {
        self.physiology.map_or(SIGHT, |p| p.sight)
    }
    fn load(id: u16, u: &Unit, data: &GameData) -> Self {
        Self {
            id,
            physiology: data
                .full_unit(&u.name)
                .filter(|unit| unit.entity_type_id == Some(id))
                .map(|unit| Physiology {
                    hp_regen: unit.hp_regen,
                    hp_regen_growth: unit.hp_regen_per_level,
                    energy_regen: unit.energy_regen,
                    energy_regen_growth: unit.energy_regen_per_level,
                    attack_speed: unit.attack_speed,
                    attack_speed_growth: unit.attack_speed_per_level,
                    collision_radius: unit.collision_radius,
                    sight: unit.vision_range,
                    gold_bounty: unit.gold_bounty,
                    xp_bounty: unit.xp_bounty,
                }),
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
    mobile_bulwark_bonus: Option<f32>,
    long_shot: Option<native_combat::LongShot>,
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
    solo_route_index: u8,
    /// Solo side-lane assignment (-1, 0 mid, +1).
    #[serde(skip)]
    pub solo_side: i8,
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
    solo_lane_rewards: Option<halcyon_data::SoloLaneRewards>,
    next_id: u32,
    vision_devices: Vec<VisionDevice>,
    vision_charges: [vision_devices::Charges; 6],
    next_item: u32,
    pending_item_events: Vec<Event>,
    seed: u64,
    crit_rng: u64,
    bot_difficulty: Option<halcyon_data::BotDifficulty>,
    first_wave: u64,
    wave_interval: u64,
    abilities: std::sync::Arc<AbilityBook>,
    shop: shop::Shop,
    effects: Vec<SpellEffect>,
    chakrams: Vec<chakram::Chakram>,
    sigils: Vec<sigil::Field>,
    twisters: Vec<twister::Dash>,
    arcane_fire: Vec<arcane_fire::Pulse>,
    bulwarks: Vec<bulwark::Field>,
    water: Vec<lorelai::Water>,
    nav_budget: u8,
    native_navigation: Option<Box<native_navigation::NativeNavigation>>,
    native_camps: Option<Vec<native_camps::NativeCampUnit>>,
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
                (self.seed, self.crit_rng),
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
                (
                    &self.effects,
                    &self.chakrams,
                    &self.water,
                    &self.sigils,
                    &self.twisters,
                    &self.arcane_fire,
                    &self.bulwarks,
                ),
            ),
        )
        .expect("finite simulation state");
        serde_json::to_writer(&mut hash, &(&self.vision_devices, &self.vision_charges))
            .expect("finite vision device state");
        if let Some(rewards) = self.solo_lane_rewards {
            serde_json::to_writer(&mut hash, &rewards).expect("finite unit rewards");
        }
        if let Some(map) = &self.native_navigation {
            map.digest(&mut hash);
        }
        if let Some(camps) = &self.native_camps {
            serde_json::to_writer(&mut hash, camps).expect("finite camp placements");
        }
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
            if e.solo_side != 0 {
                hash.update(e.solo_side.to_be_bytes());
            }
            if e.solo_route_index != 0 {
                hash.update([e.solo_route_index]);
            }
        }
        format!("{:x}", hash.finalize())
    }
    pub fn set_bot_difficulty(&mut self, difficulty: Option<halcyon_data::BotDifficulty>) {
        self.bot_difficulty = difficulty;
    }

    fn basic_attack_interval(&self, e: &Entity) -> u64 {
        let haste = e.status.haste_until > self.tick;
        let twirl = self
            .abilities
            .ringo_b
            .as_ref()
            .zip(e.status.twirl_rank.filter(|_| haste));
        let speed = e
            .stats
            .physiology
            .filter(|p| p.attack_speed > 0.)
            .map_or(1., |p| {
                p.attack_speed + f32::from(e.level.saturating_sub(1)) * p.attack_speed_growth
            });
        let interval = (if haste && twirl.is_none() { 10.0 } else { 17.0 })
            / (speed + e.item_stats.attack_speed + twirl.map_or(0., |(b, r)| b.attack_speed[r]));
        interval.ceil() as u64
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
        world_rules::BUILTIN.world(seed, human_seats)
    }
    pub fn from_data(seed: u64, human_seats: u8, data: &GameData) -> Result<Self, String> {
        Ok(WorldRules::load(data)?.world(seed, human_seats))
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
        hero.mobile_bulwark_bonus = None;
        hero.long_shot = None;
        hero.stats = Stats::load(id, unit, data);
        hero.energy = unit.energy;
        hero.max_energy = unit.energy;
        hero.type_id = id;
        hero.max_hp = unit.hp;
        hero.hp = unit.hp;
        hero.apply_growth(hero.level - 1);
        Ok(())
    }
    fn build(seed: u64, human_seats: u8, rules: &WorldRules) -> Self {
        let mut w = Self {
            tick: 0,
            winner: None,
            entities: Vec::with_capacity(50),
            next_id: 1,
            layout: Layout::default(),
            aral_units: rules.aral_units,
            solo_lane_rewards: rules.solo_lane_rewards,
            solo_inner_towers: rules.solo_inner_towers,
            vision_devices: Vec::new(),
            vision_charges: Default::default(),
            next_item: 40000,
            stats: rules.stats,
            seed,
            crit_rng: seed,
            bot_difficulty: None,
            first_wave: rules.first_wave,
            wave_interval: rules.wave_interval,
            abilities: rules.abilities.clone(),
            shop: rules.shop.clone(),
            pending_item_events: Vec::new(),
            effects: Vec::new(),
            chakrams: Vec::new(),
            sigils: Vec::new(),
            twisters: Vec::new(),
            arcane_fire: Vec::new(),
            bulwarks: Vec::new(),
            water: Vec::new(),
            nav_budget: 0,
            native_navigation: None,
            native_camps: None,
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
            mobile_bulwark_bonus: None,
            long_shot: None,
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
            solo_route_index: 0,
            solo_side: 0,
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
    pub fn sees_position(&self, team: u8, p: Position) -> bool {
        (self.device_sight(team, p) || self.sigil_sight(team, p))
            || self.entities.iter().any(|e| {
                e.team == team
                    && e.hp > 0.
                    && e.position.dist(p) <= e.stats.sight()
                    && self.layout.clear(e.position, p)
                    && self
                        .native_navigation
                        .as_ref()
                        .is_none_or(|map| map.can_see(e.position, p))
            })
    }
    pub fn visible(&self, team: u8, e: &Entity) -> bool {
        e.team == team
            || (e.hp > 0.
                && (self.device_sight(team, e.position) || self.sigil_sight(team, e.position)))
            || (e.hp > 0. && e.status.reveal_until > self.tick)
            || (e.hp > 0.
                && self.entities.iter().any(|a| {
                    a.team == team
                        && a.hp > 0.
                        && (a.position.x - e.position.x).abs() <= a.stats.sight()
                        && a.position.dist(e.position) <= a.stats.sight()
                        && self.combat_clear(a, e)
                        && self
                            .native_navigation
                            .as_ref()
                            .is_none_or(|map| map.can_see(a.position, e.position))
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
            Command::MoveTo(p)
                if !self.layout.walkable(p)
                    && !self
                        .native_navigation
                        .as_ref()
                        .is_some_and(|map| map.can_project(p)) =>
            {
                return Err("outside lane");
            }
            Command::Buy(item) => return self.buy(i, item).map(|_| ()),
            Command::Sell { instance } => {
                return self.sell_item(self.entities[i].id, instance).map(|_| ());
            }
            Command::DeployVisionTotem { instance, point } => {
                return self.deploy_device(i, instance, Some(point));
            }
            Command::DeployScoutTrap { instance } => return self.deploy_device(i, instance, None),
            Command::HitScoutCam { device } => return self.hit_device(i, device),
            Command::ConsumeInfusion { instance } => return self.consume_infusion(i, instance),
            Command::ActivateHealingFlask { instance } => {
                return self.activate_healing_flask(self.entities[i].id, instance);
            }
            Command::ActivateSprintBoots { instance } => {
                return self.activate_sprint_boots(self.entities[i].id, instance);
            }
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
    #[inline]
    fn walk_diagnosed<const DIAG: bool>(
        &mut self,
        i: usize,
        goal: Position,
        events: &mut Vec<Event>,
        diagnostics: Option<&mut BotDiagnostics>,
    ) {
        if DIAG && self.entities[i].kind == Kind::Hero && self.entities[i].bot {
            let before = self.entities[i].position;
            self.walk(i, goal, events);
            diagnostics
                .unwrap()
                .walk(&self.entities[i], self.tick, before, goal);
        } else {
            self.walk(i, goal, events);
        }
    }

    fn walk(&mut self, i: usize, p: Position, events: &mut Vec<Event>) {
        let e = &mut self.entities[i];
        if e.stats.speed <= 0. || e.status.root_until > self.tick {
            return;
        }
        let next = if e.kind == Kind::Hero
            && let Some(map) = &mut self.native_navigation
        {
            map.next(e, self.tick, p, &mut self.nav_budget)
        } else {
            match &self.layout {
                Layout::Validation => e.path.next(e.position, p, &mut self.nav_budget),
                Layout::SoloMid { .. }
                    if e.kind == Kind::Minion
                        && e.order == Command::MoveTo(p)
                        && p == self.layout.objective(1 - e.team) =>
                {
                    if e.solo_side == 0 {
                        Some(solo_route::march(e, p))
                    } else {
                        let marker = solo_side::march(e, p);
                        solo_side::steer(e.position, marker)
                    }
                }
                Layout::SoloMid { .. } => solo_side::steer(e.position, p),
                Layout::Aral { .. } => self.layout.clear(e.position, p).then_some(p),
            }
        };
        let Some(waypoint) = next else {
            return;
        };
        let twirl = self.abilities.ringo_b.as_ref().zip(
            e.status
                .twirl_rank
                .filter(|_| e.status.haste_until > self.tick),
        );
        let pace = if e.status.slow_until > self.tick {
            e.status.slow_factor
        } else {
            1.
        }
        .min(if e.status.bulwark_slow_until > self.tick {
            e.status.bulwark_slow_factor
        } else {
            1.
        })
        .min(if e.status.water_slow_until > self.tick {
            e.status.water_slow_factor
        } else {
            1.
        }) * if (e.status.water_speed_until > self.tick
            && e.status.water_speed_bonus.is_none())
            || (twirl.is_none() && e.status.haste_until > self.tick)
        {
            1.25
        } else {
            1.
        };
        let pos = e.position.advance(
            waypoint,
            (e.stats.speed
                + e.status
                    .water_speed_bonus
                    .filter(|_| e.status.water_speed_until > self.tick)
                    .unwrap_or(0.)
                + e.status
                    .pursuit
                    .filter(|p| self.tick < p.speed_until)
                    .map_or(0., |p| p.speed)
                + twirl.map_or(0., |(b, r)| b.move_speed[r])
                + e.status.sigil_speed
                    * e.status.sigil_speed_until.saturating_sub(self.tick) as f32
                    / e.status.sigil_speed_duration.max(1) as f32
                + e.item_stats.boot_speed
                + if self.tick < e.item_stats.sprint_until {
                    e.item_stats.sprint_speed
                } else {
                    0.
                })
                * (1. + e.item_stats.move_speed_amp)
                * pace
                / TICK_HZ as f32,
        );
        if pos != e.position {
            if e.type_id == EntityTemplate::Reim.as_raw() {
                e.status.reim_heading = Some((pos.y - e.position.y).atan2(pos.x - e.position.x));
            }
            if e.kind == Kind::Minion {
                let followed = match e.order {
                    Command::Attack(id) => Some(id),
                    _ => None,
                };
                e.lane_step = Some((waypoint, followed));
            }
            e.position = pos;
            if e.kind == Kind::Hero
                && let Some(map) = &mut self.native_navigation
            {
                map.moved(e.id, pos);
            }
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
        self.step_with::<false>(None)
    }

    /// Run the same simulation with wall-clock diagnostics for hero bots.
    pub fn step_diagnosed(&mut self, diagnostics: &mut BotDiagnostics) -> Vec<Event> {
        self.step_with::<true>(Some(diagnostics))
    }

    fn step_with<const DIAG: bool>(
        &mut self,
        diagnostics: Option<&mut BotDiagnostics>,
    ) -> Vec<Event> {
        let mut events = std::mem::take(&mut self.pending_item_events);
        if self.winner.is_some() {
            return events;
        }
        self.tick += 1;
        self.step_tick::<DIAG>(&mut events, diagnostics);
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
    fn step_tick<const DIAG: bool>(
        &mut self,
        events: &mut Vec<Event>,
        mut diagnostics: Option<&mut BotDiagnostics>,
    ) {
        self.advance_devices();
        self.nav_budget = 6;
        self.advance_effects(events);
        self.advance_chakrams(events);
        self.advance_water(events);
        self.advance_bulwarks(events);
        self.advance_arcane_fire(events);
        self.advance_twisters(events);
        self.advance_sigils(events);
        if self.winner.is_some() {
            return;
        }
        if matches!(self.layout, Layout::Aral { .. }) {
            self.spawn_aral_wave(events);
        } else if matches!(self.layout, Layout::SoloMid { .. }) {
            self.spawn_native_camps(events);
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
            if e.kind == Kind::Hero {
                shop::infusions::update(e, self.tick, &self.shop);
            }
            e.lane_step = None;
            if e.kind == Kind::Armory {
                self.advance_armory(i, events);
                continue;
            }
            if e.hp <= 0. {
                if e.kind == Kind::Hero
                    && matches!(self.layout, Layout::Aral { .. })
                    && e.respawn.checked_sub(self.tick) == Some(6)
                {
                    events.push(Event::RespawnApproach {
                        id: e.id,
                        team: e.team,
                        home: self.layout.home(e),
                    });
                }
                if e.kind == Kind::Hero && e.respawn <= self.tick {
                    e.hp = e.max_hp;
                    e.energy = e.max_energy;
                    e.status = Default::default();
                    e.path = Default::default();
                    e.position = self.layout.home(e);
                    if let Some(map) = &mut self.native_navigation {
                        map.forget_route(e.id);
                    }
                    events.push(Event::Spawn { id: e.id });
                    events.push(Event::Respawn {
                        id: e.id,
                        position: e.position,
                    });
                }
                continue;
            }
            if e.status.flask_until >= self.tick && e.status.flask_until != 0 {
                let before = e.hp;
                e.hp = (e.hp + e.status.flask_heal_per_tick).min(e.max_hp);
                e.energy = (e.energy + e.status.flask_energy_per_tick).min(e.max_energy);
                if e.hp > before {
                    events.push(Event::Heal {
                        source: e.id,
                        target: e.id,
                        amount: e.hp - before,
                        hp: e.hp,
                    });
                }
            }
            let levels = f32::from(e.level.saturating_sub(1));
            if let Some(p) = e.stats.physiology
                && matches!(e.kind, Kind::Hero | Kind::Minion)
            {
                let regen = p.hp_regen + levels * p.hp_regen_growth;
                e.hp = (e.hp + regen / TICK_HZ as f32).min(e.max_hp);
            }
            let fountain = e.kind == Kind::Hero
                && self
                    .native_navigation
                    .as_ref()
                    .map_or_else(|| self.layout.at_base(e, false), |map| map.at_fountain(e));
            if fountain {
                e.hp = (e.hp + e.max_hp * 0.003).min(e.max_hp);
            }
            if e.kind == Kind::Hero {
                e.energy = (e.energy
                    + e.item_stats.energy_regen / TICK_HZ as f32
                    + e.stats.physiology.map_or(0.1, |p| {
                        (p.energy_regen + levels * p.energy_regen_growth) / TICK_HZ as f32
                    })
                    + if fountain { 1.1 } else { 0. })
                .min(e.max_energy);
                if self.tick >= 35 * TICK_HZ
                    && self.tick.is_multiple_of(TICK_HZ)
                    && matches!(self.layout, Layout::Validation)
                {
                    self.award_xp(i, 0.45, events);
                }
            }
            if self.entities[i].status.stormguard.is_some() {
                self.advance_stormguard(i, events);
            }
            if self.entities[i].status.flail.is_some() {
                self.advance_flail(i, events);
            }
            if self.entities[i].status.overwhelm.is_some() && !self.yates_is_leaping(i) {
                self.advance_overwhelm(i, events);
            }
            let hero = &self.entities[i];
            if hero.status.stun_until > self.tick
                || self.twisters.iter().any(|d| d.source == hero.id)
            {
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
            let automated =
                e.kind != Kind::Neutral && e.winding.is_none() && (e.bot || e.kind != Kind::Hero);
            let offset = self.tick + u64::from(e.id);
            let decide = automated && offset.is_multiple_of(self.decision_interval(e));
            if decide
                && e.bot
                && e.kind == Kind::Hero
                && e.hp < 0.3 * e.max_hp
                && self.flask_available(i)
            {
                let id = e.id;
                if let Some(instance) = self.flask_instance(i)
                    && self.activate_healing_flask(id, instance).is_ok()
                {
                    events.append(&mut self.pending_item_events);
                }
            }
            let e = &self.entities[i];
            if automated && e.bot && offset.is_multiple_of(5) {
                if e.inventory.len() < 6
                    && self.at_item_shop(e)
                    && let Some(item) = self.shop.bot_item(
                        e.inventory.len(),
                        e.gold,
                        matches!(self.layout, Layout::SoloMid { .. }),
                    )
                    && let Ok(receipt) = self.buy(i, item)
                {
                    events.push(Event::Purchase(receipt));
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
            if decide
                && self.entities[i].kind == Kind::Hero
                && self.entities[i].bot
                && self.tick >= self.entities[i].item_stats.sprint_ready
                && matches!(self.entities[i].order,Command::MoveTo(goal) if self.entities[i].position.dist(goal)>8.)
                && let Some(boot) = self.entities[i]
                    .inventory
                    .iter()
                    .find(|item| item.kind == BasicItem::SprintBoots)
            {
                let _ = self.activate_sprint_boots(self.entities[i].id, boot.instance);
            }
            if decide {
                let started = if DIAG && self.entities[i].kind == Kind::Hero && self.entities[i].bot
                {
                    Some(std::time::Instant::now())
                } else {
                    None
                };
                let order = self.decide(i);
                let elapsed = started.map(|start| start.elapsed());
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
                let result = self.order(i, order);
                if DIAG && let Some(elapsed) = elapsed {
                    diagnostics.as_deref_mut().unwrap().decision(
                        &self.entities[i],
                        elapsed,
                        result,
                    );
                }
            }
            match self.entities[i].order {
                Command::Stop
                | Command::Buy(_)
                | Command::Sell { .. }
                | Command::DeployVisionTotem { .. }
                | Command::DeployScoutTrap { .. }
                | Command::HitScoutCam { .. }
                | Command::ConsumeInfusion { .. }
                | Command::ActivateHealingFlask { .. }
                | Command::ActivateSprintBoots { .. }
                | Command::Learn(_)
                | Command::Cast { .. } => {}
                Command::MoveTo(p) => {
                    self.walk_diagnosed::<DIAG>(i, p, events, diagnostics.as_deref_mut());
                }
                Command::Attack(id) => {
                    let Some(j) = self.index(id) else {
                        self.entities[i].winding = None;
                        self.entities[i].order = Command::Stop;
                        continue;
                    };
                    let e = &self.entities[i];
                    let t = &self.entities[j];
                    if (e.kind != Kind::Neutral && !self.visible(e.team, t)) || t.hp <= 0. {
                        self.entities[i].order = Command::Stop;
                        self.entities[i].winding = None;
                        continue;
                    }
                    let pursuit = e.status.pursuit.filter(|p| {
                        p.armed_until > self.tick
                            && matches!(t.kind, Kind::Hero | Kind::Minion | Kind::Neutral)
                    });
                    if e.position.dist(t.position) > e.stats.range + pursuit.map_or(0., |p| p.range)
                        || !self.combat_clear(e, t)
                    {
                        let p = t.position;
                        self.entities[i].winding = None;
                        if self.entities[i].kind == Kind::Neutral {
                            self.entities[i].order = Command::Stop;
                            continue;
                        }
                        self.walk_diagnosed::<DIAG>(i, p, events, diagnostics.as_deref_mut());
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
                    if e.kind == Kind::Crystal
                        && matches!(self.layout, Layout::SoloMid { .. })
                        && self.living_armories(e.team) == 0
                    {
                        self.entities[i].order = Command::Stop;
                        self.entities[i].winding = None;
                        continue;
                    }
                    let source = e.id;
                    let team = e.team;
                    let bonus = if e.status.empowered > 0 && e.status.agent_crystal_until.is_none()
                    {
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
                    let double_down = e.status.double_down;
                    let damage = if e.type_id == lorelai::HERO {
                        shop::mitigated(
                            10. + e.stats.weapon + bonus,
                            t.stats.shield,
                            e.item_stats.shield_pierce,
                        )
                    } else if e.kind == Kind::Crystal {
                        t.max_hp * 0.04
                    } else {
                        let multiplier = if e.kind == Kind::Hero
                            && matches!(t.kind, Kind::Hero | Kind::Minion)
                        {
                            if double_down.is_some() {
                                1.5 + e.item_stats.crit_damage
                            } else {
                                shop::weapon_multiplier(&mut self.crit_rng, e.item_stats)
                            }
                        } else {
                            1.
                        };
                        shop::mitigated(
                            e.stats.weapon * multiplier + bonus,
                            t.stats.armor,
                            e.item_stats.armor_pierce,
                        )
                    };
                    let haste = e.status.haste_until > self.tick;
                    let twirl = self
                        .abilities
                        .ringo_b
                        .as_ref()
                        .zip(e.status.twirl_rank.filter(|_| haste));
                    let crystal = e.kind == Kind::Crystal;
                    // Karas's 4.13 basic-attack coefficient is explicitly named in official_rules.
                    let bonus_crystal = if e.type_id == EntityTemplate::Karas.as_raw() {
                        shop::mitigated(
                            e.item_stats.crystal * self.abilities.karas_basic_crystal_ratio,
                            t.stats.shield,
                            e.item_stats.shield_pierce,
                        )
                    } else if let Some((b, r)) = twirl {
                        shop::mitigated(
                            b.damage.at(r, e.item_stats.crystal, 0.),
                            t.stats.shield,
                            e.item_stats.shield_pierce,
                        )
                    } else {
                        0.
                    };
                    let bonus_crystal = bonus_crystal
                        + if e.status.empowered > 0
                            && e.status
                                .agent_crystal_until
                                .is_some_and(|until| until > self.tick)
                        {
                            shop::mitigated(
                                e.status.empower_damage
                                    + if self.has_arcane_fire(t.id, e.status.agent_source) {
                                        e.status.agent_burn_bonus
                                    } else {
                                        0.
                                    },
                                t.stats.shield,
                                e.item_stats.shield_pierce,
                            )
                        } else {
                            0.
                        };
                    let item_attack = e.kind == Kind::Hero && e.item_stats.has_attack_passives();
                    let npc_launch = self.npc_launch_delay(e, t);
                    let ranged = e.kind == Kind::Hero && e.stats.range >= 4.;
                    let travel = (e.position.dist(t.position) * TICK_HZ as f32 / 18.).ceil() as u64;
                    let interval = self.basic_attack_interval(e);
                    self.entities[i].winding = None;
                    self.entities[i].next_attack = self.tick + interval;
                    self.entities[i].status.empowered =
                        self.entities[i].status.empowered.saturating_sub(1);
                    events.push(Event::Attack { source, target: id });
                    if let Some(instance) = double_down {
                        self.entities[i].status.double_down = None;
                        events.push(Event::DoubleDown {
                            source,
                            instance,
                            attack_target: Some(id),
                        });
                    }
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
                        let impact = if item_attack {
                            SpellEffect::basic_impact
                        } else {
                            SpellEffect::impact
                        };
                        self.effects.push(impact(
                            self.tick + travel,
                            source,
                            team,
                            id,
                            damage,
                            school,
                        ));
                    } else if item_attack {
                        self.basic_hit(source, team, j, damage, school, events);
                    } else {
                        self.hurt(source, team, j, damage, school, events);
                    }
                    if let Some(p) = pursuit {
                        self.pursuit_impact(i, j, p, events);
                    }
                    if bonus_crystal > 0. {
                        if ranged {
                            self.effects.push(SpellEffect::impact(
                                self.tick + travel,
                                source,
                                team,
                                id,
                                bonus_crystal,
                                DamageSchool::Crystal,
                            ));
                        } else {
                            self.hurt(
                                source,
                                team,
                                j,
                                bonus_crystal,
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
            .retain(|e| !matches!(e.kind, Kind::Minion | Kind::Neutral) || e.hp > 0.);
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
