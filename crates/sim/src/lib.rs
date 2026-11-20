//! Deterministic straight-lane validation map. Coordinates are project world units.
//! Vision, spawn offsets, windup, attack cadence, retreat and respawn are project rules.
use halcyon_data::EntityTemplate;
mod actors;
mod ai;
pub use ai::{
    AbilityRole, ActiveItemCategory, ActiveItemIntent, ClassifiedAbility, HeroIntentCatalog,
    HeroKitIntent, MacroMode, ObjectiveInfo, ObjectiveKind, TargetPattern, ThreatAssessment,
};
mod amael;
pub use amael::KickKind;
mod aral;
mod arcane_fire;
mod fixed_tick;
mod item_actives;
pub use item_actives::ItemTarget;
mod item_passives;
mod verse;
pub use actors::Actors;
mod persistent_spells;
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
#[cfg(test)]
mod effects_replay;
mod hero_kit;
mod hero_kits;
#[cfg(test)]
mod item_effects_tests;
mod layout;
pub use layout::SOLO_CRYSTAL_START_INSTANCES;
mod lorelai;
pub mod map_3v3;
pub mod map_5v5;
pub use map_5v5::{
    CampKind5v5, CampVisibilityState5v5, CampVisibilityTransition5v5, Hero5v5Status,
};
pub mod map_blitz;
pub use map_blitz::BlitzTalentSlot;
mod native_camps;
mod native_combat;
mod native_learn;
mod native_navigation;
mod nav;
use layout::Layout;
mod overwhelm;
mod progression;
mod pursuit;
mod reboot;
mod reflex;
mod score;
mod shop;
mod solo_route;
mod solo_side;
mod steering;
mod talents_runtime;
mod world_rules;
pub use world_rules::WorldRules;
mod sigil;
mod skedaddle;
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
    ActivateReflexBlock {
        instance: u32,
    },
    ActivateHealingFlask {
        instance: u32,
    },
    ActivateSprintBoots {
        instance: u32,
    },
    ActivateItem {
        instance: u32,
        target: ItemTarget,
    },
    Recall,
    Stop,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImpactEffect {
    #[default]
    Standard,
    ChillWinds,
    AmaelKick,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Event {
    BasicTargetChanged {
        actor: u32,
        target: u32,
    },
    VisionDeployed {
        device: VisionDevice,
        instance: u32,
    },
    RebootStarted {
        actor: u32,
        source: u32,
        instance: u32,
        cooldown_seconds: f32,
    },
    RebootRecovered {
        actor: u32,
        instance: u32,
        cooldown_seconds: f32,
        hp: f32,
    },
    Skedaddle {
        source: u32,
        immunity_instance: u32,
        speed_instance: u32,
        immunity_seconds: f32,
        speed_seconds: f32,
        talent_reset: bool,
    },
    AmaelLunge {
        source: u32,
        target: u32,
        goal: Position,
        speed: f32,
    },
    AmaelKick {
        source: u32,
        target: u32,
        instance: u32,
        kind: KickKind,
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
    MobileBulwarkStart {
        source: u32,
        anchor: u32,
        position: Position,
        heading: f32,
    },
    MobileBulwarkEnd {
        source: u32,
        anchor: u32,
        position: Position,
    },
    TwisterDash {
        source: u32,
        goal: Position,
        speed: f32,
    },
    WinterSpireProjectile {
        source: u32,
        id: u32,
        point: Position,
    },
    ChakramLaunch {
        source: u32,
        id: u32,
        origin: Position,
        aim: Position,
    },
    ChakramCatch {
        source: u32,
        id: u32,
        position: Position,
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
    InfusionApplied {
        actor: u32,
        item: u32,
        effect: u32,
        kind: BasicItem,
        level: u8,
        duration_ticks: u64,
    },
    InfusionReplaced {
        actor: u32,
        effect: u32,
    },
    InfusionLevel {
        actor: u32,
        effect: u32,
        level: u8,
    },
    ReflexActivated {
        actor: u32,
        item: u32,
        first_effect: u32,
        barrier: f32,
    },
    ReflexExpired {
        actor: u32,
        effect: u32,
    },
    FlaskActivated {
        actor: u32,
        instance: u32,
    },
    ItemActivated {
        actor: u32,
        instance: u32,
        kind: BasicItem,
        target: ItemTarget,
        cooldown_ticks: u64,
    },
    ItemCooldownUpdated {
        actor: u32,
        key: u32,
        remaining_seconds: f32,
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
    /// Legal navigation segment taken by a native Solo bot hero.
    SoloBotPath {
        id: u32,
        waypoint: Position,
        goal: Position,
        target: Option<u32>,
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
    ObjectiveCaptured {
        team: u8,
        actor: u32,
        gold: u32,
    },
    BlitzPoints {
        team: u8,
        points: u8,
        source: u32,
        target: u32,
        total: u32,
    },
    BlitzSuddenDeath,
    RecallComplete {
        id: u32,
        position: Position,
    },
    RecallInterrupted {
        id: u32,
    },
    CampBuffApplied {
        actor: u32,
        source: u32,
        template: u16,
    },
    GhostwingCaptured {
        team: u8,
    },
    BlackclawCaptured {
        team: u8,
    },
    BlackclawUnleashed {
        id: u32,
        team: u8,
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
#[derive(bevy_ecs::component::Component, Debug, Clone, Serialize)]
pub struct Entity {
    mobile_bulwark_bonus: Option<f32>,
    long_shot: Option<native_combat::LongShot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    speed_loader: Option<std::num::NonZeroU8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reboot: Option<reboot::Phase>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) hero_kit: Option<Box<hero_kit::HeroKitState>>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    item_runtime: Option<Box<item_actives::ItemRuntimeState>>,
    #[serde(skip)]
    pub(crate) bonus_hp: f32,
    pub ability_ranks: [u8; 3],
    #[serde(skip)]
    cast: Option<Cast>,
    #[serde(skip)]
    cooldown: [u64; 3],
    #[serde(skip)]
    status: Status,
    #[serde(skip)]
    path: nav::Path,
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
/// An owned room at the host boundary. Fixed-tick systems borrow the same actors and state.
pub type World = Room<'static>;
pub struct Room<'w> {
    pub entities: Actors<'w>,
    state: fixed_tick::StateSlot<'w>,
}
impl std::ops::Deref for Room<'_> {
    type Target = RoomState;
    fn deref(&self) -> &RoomState {
        &self.state
    }
}
impl std::ops::DerefMut for Room<'_> {
    fn deref_mut(&mut self) -> &mut RoomState {
        &mut self.state
    }
}
pub struct RoomState {
    pub tick: u64,
    pub winner: Option<u8>,
    stats: [Stats; 8],
    layout: Layout,
    aral_units: aral::Units,
    solo_inner_towers: [Stats; 2],
    solo_lane_rewards: Option<halcyon_data::SoloLaneRewards>,
    next_id: u32,
    vision_device_ids: Vec<bevy_ecs::entity::Entity>,
    vision_charges: [vision_devices::Charges; 6],
    next_item: u32,
    pending_item_events: Vec<Event>,
    seed: u64,
    crit_rng: u64,
    bot_difficulty: Option<halcyon_data::BotDifficulty>,
    modern_ai_override: Option<bool>,
    bot_brains: ai::BotBrains,
    first_wave: u64,
    wave_interval: u64,
    abilities: std::sync::Arc<AbilityBook>,
    ability_intents: std::sync::Arc<ai::utility::HeroIntentCatalog>,
    shop: shop::Shop,
    nav_budget: u8,
    steering_override: Option<bool>,
    native_navigation: Option<Box<native_navigation::NativeNavigation>>,
    native_camps: Option<Vec<native_camps::NativeCampUnit>>,
    halcyon_fold: Option<Box<map_3v3::FoldState>>,
    blitz: Option<Box<map_blitz::BlitzState>>,
    map_5v5: Option<Box<map_5v5::Map5v5State>>,
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
impl Room<'_> {
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
                    self.entities.spell_snapshot::<SpellEffect>(),
                    self.entities.spell_snapshot::<chakram::Chakram>(),
                    self.entities.spell_snapshot::<lorelai::Water>(),
                    self.entities.spell_snapshot::<sigil::Field>(),
                    self.entities.spell_snapshot::<twister::Dash>(),
                    self.entities.spell_snapshot::<arcane_fire::Pulse>(),
                    self.entities.spell_snapshot::<bulwark::Field>(),
                ),
            ),
        )
        .expect("finite simulation state");
        serde_json::to_writer(
            &mut hash,
            &(
                self.vision_devices().collect::<Vec<_>>(),
                &self.vision_charges,
            ),
        )
        .expect("finite vision device state");
        let kit_effects = self.entities.spell_snapshot::<hero_kit::KitEffect>();
        if !kit_effects.is_empty() {
            serde_json::to_writer(&mut hash, &kit_effects).expect("finite hero kit effect state");
        }
        if let Some(rewards) = self.solo_lane_rewards {
            serde_json::to_writer(&mut hash, &rewards).expect("finite unit rewards");
        }
        if let Some(map) = &self.native_navigation {
            map.digest(&mut hash);
        }
        if let Some(camps) = &self.native_camps {
            serde_json::to_writer(&mut hash, camps).expect("finite camp placements");
        }
        if let Some(fold) = &self.halcyon_fold {
            serde_json::to_writer(&mut hash, fold).expect("finite halcyon fold state");
        }
        if let Some(blitz) = &self.blitz {
            serde_json::to_writer(&mut hash, blitz).expect("finite blitz state");
        }
        if let Some(map_5v5) = &self.map_5v5 {
            map_5v5.digest(&mut hash);
        }
        if let Some(difficulty) = self.bot_difficulty {
            hash.update(difficulty.as_str().as_bytes());
        }
        if let Some(modern) = self.modern_ai_override {
            hash.update([u8::from(modern)]);
        }
        if !self.bot_brains.is_empty() {
            serde_json::to_writer(&mut hash, &self.bot_brains).expect("finite bot brain state");
        }
        if self.steering_active() {
            self.entities.steering_digest(&mut hash);
        }
        for (i, e) in self.entities.iter().enumerate() {
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
                    &e.status,
                    &e.path,
                    self.entities.last_seen(i),
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

    /// Explicitly enable or disable deterministic Layer-2 ORCA steering.
    /// By default, steering is active on `SoloMid`, `Aral`, and native maps,
    /// while `Layout::Validation` stays on the legacy baseline unless opted in.
    pub fn set_steering(&mut self, enabled: bool) {
        self.steering_override = Some(enabled);
    }

    fn steering_active(&self) -> bool {
        self.steering_override.unwrap_or_else(|| {
            !matches!(self.layout, Layout::Validation) || self.native_navigation.is_some()
        })
    }

    fn basic_attack_interval(&self, e: &Entity, extra_attack_speed: f32) -> u64 {
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
        let eff = if self.hero_kit_enabled() {
            hero_kits::kit_spec(e.type_id).map_or(1.0, |s| s.perk.attack_speed_efficiency)
        } else {
            1.0
        };
        let bonus_as = (e.item_stats.attack_speed + extra_attack_speed) * eff;
        let interval = (if haste && twirl.is_none() { 10.0 } else { 17.0 })
            / ((speed + bonus_as + twirl.map_or(0., |(b, r)| b.attack_speed[r]))
                * item_actives::atlas_attack_speed_multiplier(e, self.tick))
            .max(0.05);
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
        for (idx, team) in teams.into_iter().take(heroes).enumerate() {
            let e = &mut self.entities[idx];
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
        hero.speed_loader = None;
        hero.reboot = None;
        hero.item_runtime = None;
        hero.status.spell_shield_up = false;
        hero.hero_kit = None;
        hero.stats = Stats::load(id, unit, data);
        hero.energy = unit.energy;
        hero.max_energy = unit.energy;
        hero.type_id = id;
        hero.max_hp = unit.hp;
        hero.hp = unit.hp;
        hero.apply_growth(hero.level - 1);
        let hero_id = hero.id;
        self.state.bot_brains.remove(hero_id);
        Ok(())
    }
    fn build(seed: u64, human_seats: u8, rules: &WorldRules) -> Self {
        let mut w = Self {
            entities: Actors::default(),
            state: fixed_tick::StateSlot::owned(RoomState {
                tick: 0,
                winner: None,
                next_id: 1,
                layout: Layout::default(),
                aral_units: rules.aral_units,
                solo_lane_rewards: rules.solo_lane_rewards,
                solo_inner_towers: rules.solo_inner_towers,
                vision_device_ids: Vec::new(),
                vision_charges: Default::default(),
                next_item: 40000,
                stats: rules.stats,
                seed,
                crit_rng: seed,
                bot_difficulty: None,
                modern_ai_override: None,
                bot_brains: ai::BotBrains::default(),
                first_wave: rules.first_wave,
                wave_interval: rules.wave_interval,
                abilities: rules.abilities.clone(),
                ability_intents: rules.ability_intents.clone(),
                shop: rules.shop.clone(),
                pending_item_events: Vec::new(),
                nav_budget: 0,
                steering_override: None,
                native_navigation: None,
                native_camps: None,
                halcyon_fold: None,
                blitz: None,
                map_5v5: None,
                first_blood: false,
                scores: Default::default(),
                team_scores: Default::default(),
                rally: [None; 2],
            }),
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
            speed_loader: None,
            reboot: None,
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
            item_runtime: None,
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
            lane_step: None,
            solo_route_index: 0,
            solo_side: 0,
            path: Default::default(),
            stats: s,
            order: Command::Stop,
            next_attack: 0,
            winding: None,
            respawn: 0,
            hero_kit: None,
        });
        id
    }
    fn index(&self, id: u32) -> Option<usize> {
        self.entities.gameplay_index(id)
    }
    pub fn sees_position(&self, team: u8, p: Position) -> bool {
        (self.device_sight(team, p) || self.sigil_sight(team, p))
            || self.entities.any_unordered(|e| {
                e.team == team
                    && e.hp > 0.
                    && e.position.dist(p) <= e.stats.sight()
                    && (self.map_5v5.is_some() || self.layout.clear(e.position, p))
                    && self
                        .native_navigation
                        .as_ref()
                        .is_none_or(|map| map.can_see(e.position, p))
            })
    }
    pub fn visible(&self, team: u8, e: &Entity) -> bool {
        if e.team == team {
            return true;
        }
        if e.hp <= 0. {
            return false;
        }
        let revealed = self.device_sight(team, e.position)
            || self.sigil_sight(team, e.position)
            || self.has_5v5_true_sight(team, e)
            || e.status.reveal_until > self.tick;
        if e.hero_kit
            .as_deref()
            .is_some_and(|k| self.tick < k.stealth_until)
        {
            return revealed;
        }
        revealed
            || self.entities.any_unordered(|a| {
                a.team == team
                    && a.hp > 0.
                    && (a.position.x - e.position.x).abs() <= a.stats.sight()
                    && a.position.dist(e.position) <= a.stats.sight()
                    && (self.map_5v5.is_some() || self.combat_clear(a, e))
                    && self
                        .native_navigation
                        .as_ref()
                        .is_none_or(|map| map.can_see(a.position, e.position))
            })
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
        if e.is_rebooting() {
            return Err("hero is rebooting");
        }
        if (e.status.stun_until > self.tick
            || e.hero_kit.as_deref().is_some_and(|k| {
                self.tick < k.sleep_until || self.tick < k.fear_until || self.tick < k.taunt_until
            }))
            && matches!(
                cmd,
                Command::MoveTo(_) | Command::Attack(_) | Command::Cast { .. }
            )
        {
            return Err("stunned");
        }
        match cmd {
            Command::MoveTo(p)
                if !(self.map_5v5.is_some() && map_5v5::in_5v5_bounds(p))
                    && !self.layout.walkable(p)
                    && !self
                        .native_navigation
                        .as_ref()
                        .is_some_and(|map| map.can_project(p)) =>
            {
                return Err("outside lane");
            }
            Command::Recall => return self.begin_recall(i),
            Command::Buy(item) => return self.buy(i, item).map(|_| ()),
            Command::Sell { instance } => {
                return self.sell_item(self.entities[i].id, instance).map(|_| ());
            }
            Command::DeployVisionTotem { instance, point } => {
                self.interrupt_recall_from_command(i);
                return self.deploy_device(i, instance, Some(point));
            }
            Command::DeployScoutTrap { instance } => {
                self.interrupt_recall_from_command(i);
                return self.deploy_device(i, instance, None);
            }
            Command::HitScoutCam { device } => {
                self.interrupt_recall_from_command(i);
                return self.hit_device(i, device);
            }
            Command::ConsumeInfusion { instance } => {
                self.interrupt_recall_from_command(i);
                return self.consume_infusion(i, instance);
            }
            Command::ActivateReflexBlock { instance } => {
                self.interrupt_recall_from_command(i);
                return self.activate_reflex(i, instance);
            }
            Command::ActivateHealingFlask { instance } => {
                self.interrupt_recall_from_command(i);
                return self.activate_healing_flask(self.entities[i].id, instance);
            }
            Command::ActivateSprintBoots { instance } => {
                self.interrupt_recall_from_command(i);
                return self.activate_sprint_boots(self.entities[i].id, instance);
            }
            Command::ActivateItem { instance, target } => {
                return self.activate_item(i, instance, target);
            }
            Command::Learn(slot) => return self.learn(i, slot),
            Command::Cast { slot, target } => {
                self.interrupt_recall_from_command(i);
                return self.begin_cast(i, slot, target);
            }
            Command::Attack(id) => {
                let t = self
                    .index(id)
                    .map(|j| &self.entities[j])
                    .ok_or("unknown target")?;
                if t.team == e.team
                    || t.hp <= 0.
                    || !self.visible(e.team, t)
                    || t.hero_kit
                        .as_deref()
                        .is_some_and(|k| self.tick < k.untargetable_until)
                {
                    return Err("invalid target");
                }
                self.interrupt_recall_from_command(i);
            }
            Command::MoveTo(_) => {
                self.interrupt_recall_from_command(i);
            }
            Command::Stop => {}
        }
        self.entities[i].cast = None;
        self.entities[i].status.channel = 0;
        self.entities[i].status.channel_until = 0;
        item_actives::cancel_teleport_channel(&mut self.entities[i]);
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
        let kit_bonus = if self.hero_kit_enabled() || self.entities[i].hero_kit.is_some() {
            self.kit_bonus_stats(i)
        } else {
            hero_kit::KitStatBonus::default()
        };
        let move_5v5 = self.move_speed_5v5_bonus(self.entities[i].id);
        let steering_active = self.steering_active();
        if steering_active {
            self.entities.prepare_steering_if_needed(self.state.tick);
        }
        let state = &mut *self.state;
        let (e, steering) = self.entities.get_mut_with_steering(i);
        if e.stats.speed <= 0. || e.status.root_until > state.tick {
            return;
        }
        let next = if e.kind == Kind::Hero
            && let Some(map) = &mut state.native_navigation
        {
            map.next(e, state.tick, p, &mut state.nav_budget)
        } else {
            match &state.layout {
                Layout::Validation => e.path.next(e.position, p, &mut state.nav_budget),
                Layout::SoloMid { .. }
                    if e.kind == Kind::Minion
                        && e.order == Command::MoveTo(p)
                        && p == state.layout.objective(1 - e.team) =>
                {
                    if e.solo_side == 0 {
                        Some(solo_route::march(e, p))
                    } else {
                        let marker = solo_side::march(e, p);
                        solo_side::steer(e.position, marker)
                    }
                }
                Layout::SoloMid { .. } if state.map_5v5.is_some() => {
                    if e.kind == Kind::Minion
                        && e.order == Command::MoveTo(p)
                        && p == state.layout.objective(1 - e.team)
                    {
                        if e.solo_side == 0 {
                            Some(solo_route::march(e, p))
                        } else {
                            let marker = solo_side::march(e, p);
                            solo_side::steer(e.position, marker)
                        }
                    } else {
                        Some(p)
                    }
                }
                Layout::SoloMid { .. } => solo_side::steer(e.position, p),
                Layout::Aral { .. } => state.layout.clear(e.position, p).then_some(p),
                Layout::HalcyonFold { .. } | Layout::Blitz { .. } => {
                    if e.kind == Kind::Minion
                        && e.order == Command::MoveTo(p)
                        && p == state.layout.objective(1 - e.team)
                    {
                        let goal = aral::march_goal(e, p);
                        state.layout.clear(e.position, goal).then_some(goal)
                    } else {
                        state.layout.clear(e.position, p).then_some(p)
                    }
                }
            }
        };
        let Some(waypoint) = next else {
            return;
        };
        let twirl = state.abilities.ringo_b.as_ref().zip(
            e.status
                .twirl_rank
                .filter(|_| e.status.haste_until > state.tick),
        );
        let pace = if e.status.slow_until > state.tick {
            e.status.slow_factor
        } else {
            1.
        }
        .min(if e.status.bulwark_slow_until > state.tick {
            e.status.bulwark_slow_factor
        } else {
            1.
        })
        .min(if e.status.water_slow_until > state.tick {
            e.status.water_slow_factor
        } else {
            1.
        }) * if (e.status.water_speed_until > state.tick
            && e.status.water_speed_bonus.is_none())
            || (twirl.is_none() && e.status.haste_until > state.tick)
        {
            1.25
        } else {
            1.
        };
        let move_speed = (e.stats.speed
            + move_5v5
            + skedaddle::speed(&e.status, state.tick)
            + e.status
                .water_speed_bonus
                .filter(|_| e.status.water_speed_until > state.tick)
                .unwrap_or(0.)
            + e.status
                .captain_effects
                .read()
                .pursuit
                .filter(|p| state.tick < p.speed_until)
                .map_or(0., |p| p.speed)
            + twirl.map_or(0., |(b, r)| b.move_speed[r])
            + e.status.sigil_speed * e.status.sigil_speed_until.saturating_sub(state.tick) as f32
                / e.status.sigil_speed_duration.max(1) as f32
            + e.item_stats.boot_speed
            + item_actives::extra_move_speed(e, &state.shop, state.tick)
            + kit_bonus.move_speed_flat
            + if state.tick < e.item_stats.sprint_until {
                e.item_stats.sprint_speed
            } else {
                0.
            })
            * (1. + e.item_stats.move_speed_amp + kit_bonus.move_speed_pct)
            * pace;
        let step_dist = move_speed / TICK_HZ as f32;
        let old_pos = e.position;
        let mut ground_ok = |pt: Position| -> bool {
            if let Some(map) = &state.native_navigation {
                map.has_ground(pt)
            } else if state.map_5v5.is_some() {
                map_5v5::in_5v5_bounds(pt)
            } else {
                state.layout.walkable(pt)
            }
        };
        let pos = if steering_active {
            let candidate = steering.steer(
                steering::SteerRequest {
                    id: e.id,
                    position: old_pos,
                    waypoint,
                    speed: move_speed,
                    radius: steering::agent_radius(e),
                    responsibility: steering::agent_responsibility(e),
                    attack_target: match e.order {
                        Command::Attack(id) => Some(id),
                        _ => None,
                    },
                    attack_range: e.stats.range,
                },
                &mut ground_ok,
            );
            if ground_ok(candidate) {
                candidate
            } else {
                let unsteered = old_pos.advance(waypoint, step_dist);
                if ground_ok(unsteered) {
                    unsteered
                } else if let Some(map) = &state.native_navigation {
                    map.project_within(candidate, 0.5)
                        .or_else(|| map.project_within(unsteered, 0.5))
                        .unwrap_or(old_pos)
                } else {
                    old_pos
                }
            }
        } else {
            old_pos.advance(waypoint, step_dist)
        };
        if pos != old_pos {
            if steering_active {
                steering.record_move(e.id, old_pos, pos);
            }
            if e.type_id == EntityTemplate::Reim.as_raw()
                || (e.type_id == EntityTemplate::Lyra.as_raw()
                    && e.mobile_bulwark_bonus == Some(5.0))
            {
                e.status.motion_heading = Some((pos.y - old_pos.y).atan2(pos.x - old_pos.x));
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
                && let Some(map) = &mut state.native_navigation
            {
                map.moved(e.id, pos);
            }
            if e.kind == Kind::Hero && e.bot && matches!(state.layout, Layout::SoloMid { .. }) {
                events.push(Event::SoloBotPath {
                    id: e.id,
                    waypoint,
                    goal: p,
                    target: match e.order {
                        Command::Attack(id) => Some(id),
                        _ => None,
                    },
                });
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

    fn spawn_lane_minion(
        &mut self,
        team: u8,
        stats: Stats,
        position: Position,
        events: &mut Vec<Event>,
    ) {
        let id = self.spawn_with_stats(team, Kind::Minion, stats, position, false);
        let minion = self.entities.last_mut().unwrap();
        let level =
            (2 + self.state.tick.saturating_sub(120 * TICK_HZ) / (120 * TICK_HZ)).min(12) as u8;
        let extra = f32::from(level - 1);
        minion.level = level;
        minion.max_hp += minion.stats.growth.hp * extra;
        minion.hp = minion.max_hp;
        minion.stats.weapon += minion.stats.growth.weapon * extra;
        events.push(Event::Spawn { id });
    }
    fn spawn_tick_waves(&mut self, events: &mut Vec<Event>) {
        if matches!(self.layout, Layout::Aral { .. }) {
            self.spawn_aral_wave(events);
        } else if matches!(self.layout, Layout::HalcyonFold { .. }) {
            self.advance_halcyon_fold(events);
        } else if matches!(self.layout, Layout::Blitz { .. }) {
            self.advance_blitz(events);
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
    }
    fn team_upkeep(&mut self, events: &mut Vec<Event>) {
        if self.tick.is_multiple_of(TICK_HZ) {
            self.native_passive_income(events);
            self.update_rally();
        }
    }
    fn resolve_actor_turns<const DIAG: bool>(
        &mut self,
        events: &mut Vec<Event>,
        mut diagnostics: Option<&mut BotDiagnostics>,
    ) {
        self.tick_hero_kits(events);
        let n = self.entities.len();
        let steering_active = self.steering_active();
        if steering_active {
            self.entities.begin_steering_step(self.state.tick);
        }
        // Rotate priority when actions coincide. Stable indices and the match seed determine all ties.
        let first = (self.tick.wrapping_add(self.seed) % n as u64) as usize;
        for offset in 0..n {
            let i = (first + offset) % n;
            let e = &mut self.entities[i];
            if e.kind == Kind::Hero {
                shop::infusions::update(e, self.state.tick, &self.state.shop, events);
                reflex::advance(e, self.state.tick, events);
                if reboot::advance(e, self.state.tick, events) {
                    continue;
                }
            }
            e.lane_step = None;
            if e.kind == Kind::Armory {
                self.advance_armory(i, events);
                continue;
            }
            if e.hp <= 0. {
                if e.kind == Kind::Hero
                    && matches!(self.state.layout, Layout::Aral { .. })
                    && e.respawn.checked_sub(self.state.tick) == Some(6)
                {
                    events.push(Event::RespawnApproach {
                        id: e.id,
                        team: e.team,
                        home: self.state.layout.home(e),
                    });
                }
                if e.kind == Kind::Hero && e.respawn <= self.state.tick {
                    e.hp = e.max_hp;
                    e.energy = e.max_energy;
                    e.status = Default::default();
                    item_actives::clear_ephemeral_on_death(e);
                    e.path = Default::default();
                    e.position = self.state.layout.home(e);
                    let id = e.id;
                    let home = e.position;
                    if let Some(map) = &mut self.state.native_navigation {
                        map.forget_route(id);
                    }
                    events.push(Event::Spawn { id });
                    events.push(Event::Respawn { id, position: home });
                    self.state.bot_brains.reset_hero(id, home);
                    if steering_active {
                        self.entities.forget_steering(id);
                    }
                }
                continue;
            }
            if e.status.flask_until >= self.state.tick && e.status.flask_until != 0 {
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
                && self.state.native_navigation.as_ref().map_or_else(
                    || self.state.layout.at_base(e, false),
                    |map| map.at_fountain(e),
                );
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
            if self.entities[i]
                .status
                .captain_effects
                .read()
                .stormguard
                .is_some()
            {
                self.advance_stormguard(i, events);
            }
            if self.entities[i]
                .status
                .captain_effects
                .read()
                .flail
                .is_some()
            {
                self.advance_flail(i, events);
            }
            if self.entities[i]
                .status
                .captain_effects
                .read()
                .overwhelm
                .is_some()
                && !self.yates_is_leaping(i)
            {
                self.advance_overwhelm(i, events);
            }
            let hero = &self.entities[i];
            if hero.status.stun_until > self.tick
                || hero
                    .hero_kit
                    .as_deref()
                    .is_some_and(|k| self.tick < k.sleep_until)
                || (hero.type_id == EntityTemplate::Karas.as_raw()
                    && self
                        .entities
                        .spells::<twister::Dash>()
                        .any(|d| d.source == hero.id))
            {
                continue;
            }
            if let Some(fear_src) = hero
                .hero_kit
                .as_deref()
                .and_then(|k| (self.tick < k.fear_until).then_some(k.fear_source))
            {
                let hpos = hero.position;
                let hteam = hero.team;
                let away = self
                    .index(fear_src)
                    .map(|fi| self.entities[fi].position)
                    .filter(|p| *p != hpos)
                    .map_or_else(
                        || base(hteam),
                        |p| Position {
                            x: hpos.x + (hpos.x - p.x),
                            y: hpos.y + (hpos.y - p.y),
                        },
                    );
                self.walk_diagnosed::<DIAG>(i, away, events, diagnostics.as_deref_mut());
                continue;
            }
            let taunted = hero.hero_kit.as_deref().and_then(|k| {
                (self.tick < k.taunt_until && k.taunt_target != 0).then_some(k.taunt_target)
            });
            if let Some(tt) = taunted {
                self.entities[i].cast = None;
                self.entities[i].order = Command::Attack(tt);
            }
            let hero = &self.entities[i];
            if taunted.is_none()
                && hero.bot
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
            let automated = taunted.is_none()
                && e.kind != Kind::Neutral
                && e.winding.is_none()
                && (e.bot || e.kind != Kind::Hero);
            let offset = self.tick + u64::from(e.id);
            let decide = automated && offset.is_multiple_of(self.decision_interval(e));
            let modern_hero_bot = self.uses_modern_ai() && e.kind == Kind::Hero && e.bot;
            if decide
                && !modern_hero_bot
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
                && !modern_hero_bot
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
                let order = if modern_hero_bot {
                    let threat = self.evaluate_threat(i);
                    let bb = self.step_bot_macro_bt(i, &threat);
                    let cmd = self.decide_with_context(i, &bb, &threat);
                    let move_goal_dist = match cmd {
                        Command::MoveTo(goal) => self.entities[i].position.dist(goal),
                        _ => 0.0,
                    };
                    let quiet_recall = matches!(cmd, Command::Recall)
                        || (self
                            .hero_5v5_status(self.entities[i].id)
                            .is_some_and(|s| s.recalling_until.is_some())
                            && threat.nearby_enemy_heroes == 0);
                    if !quiet_recall
                        && let Some(item_cmd) =
                            self.best_executable_active_item(i, &threat, bb.mode, move_goal_dist)
                        && self.order(i, item_cmd).is_ok()
                    {
                        events.append(&mut self.pending_item_events);
                    }
                    cmd
                } else {
                    self.decide(i)
                };
                let elapsed = started.map(|start| start.elapsed());
                let result = self.order(i, order);
                if result.is_ok()
                    && let Command::Attack(id)
                    | Command::Cast {
                        target: CastTarget::Entity(id),
                        ..
                    } = order
                    && let Some(j) = self.index(id)
                    && self.entities[j].team != self.entities[i].team
                {
                    let position = self.entities[j].position;
                    self.entities.remember_target(i, id, position, self.tick);
                }
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
                | Command::Recall
                | Command::Buy(_)
                | Command::Sell { .. }
                | Command::DeployVisionTotem { .. }
                | Command::DeployScoutTrap { .. }
                | Command::HitScoutCam { .. }
                | Command::ConsumeInfusion { .. }
                | Command::ActivateReflexBlock { .. }
                | Command::ActivateHealingFlask { .. }
                | Command::ActivateSprintBoots { .. }
                | Command::ActivateItem { .. }
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
                    let kit_active = self.hero_kit_enabled()
                        || self.entities[i].hero_kit.is_some()
                        || self.entities[j].hero_kit.is_some();
                    let kit_atk = if kit_active {
                        self.kit_bonus_stats(i)
                    } else {
                        hero_kit::KitStatBonus::default()
                    };
                    let kit_def = if kit_active {
                        self.kit_bonus_stats(j)
                    } else {
                        hero_kit::KitStatBonus::default()
                    };
                    let e = &self.entities[i];
                    let t = &self.entities[j];
                    if (e.kind != Kind::Neutral && !self.visible(e.team, t))
                        || t.hp <= 0.
                        || t.hero_kit
                            .as_deref()
                            .is_some_and(|k| self.tick < k.untargetable_until)
                    {
                        self.entities[i].order = Command::Stop;
                        self.entities[i].winding = None;
                        continue;
                    }
                    let pursuit = e.status.captain_effects.read().pursuit.filter(|p| {
                        p.armed_until > self.tick
                            && matches!(t.kind, Kind::Hero | Kind::Minion | Kind::Neutral)
                    });
                    if e.position.dist(t.position)
                        > e.stats.range + kit_atk.attack_range + pursuit.map_or(0., |p| p.range)
                        || !self.combat_clear(e, t)
                    {
                        let p = t.position;
                        self.entities[i].winding = None;
                        if self.entities[i].kind == Kind::Neutral && self.map_5v5.is_none() {
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
                    let target_kind = t.kind;
                    let bonus = if e.status.empowered > 0 && e.status.agent_crystal_until.is_none()
                    {
                        e.status.empower_damage
                    } else {
                        0.
                    };
                    let basic_crystal = self.hero_kit_enabled()
                        && hero_kits::kit_spec(e.type_id)
                            .is_some_and(|s| s.perk.basic_attack_is_crystal);
                    let aa_dmg_mult = if self.hero_kit_enabled() {
                        hero_kits::kit_spec(e.type_id)
                            .map_or(1.0, |s| s.perk.basic_attack_damage_mult)
                    } else {
                        1.0
                    };
                    let school = if e.type_id == lorelai::HERO || basic_crystal {
                        DamageSchool::Crystal
                    } else if e.kind == Kind::Crystal {
                        DamageSchool::True
                    } else {
                        DamageSchool::Weapon
                    };
                    let eff_wp = if kit_atk.weapon_flat == 0. && kit_atk.weapon_pct == 0. {
                        e.stats.weapon
                    } else {
                        (e.stats.weapon + kit_atk.weapon_flat) * (1. + kit_atk.weapon_pct)
                    } * aa_dmg_mult;
                    let base_armor = item_actives::armor_with_shred(t, self.tick);
                    let eff_armor = if kit_def.armor_flat == 0. && kit_def.armor_pct == 0. {
                        base_armor
                    } else {
                        (base_armor + kit_def.armor_flat) * (1. + kit_def.armor_pct)
                    };
                    let eff_shield = if kit_def.shield_flat == 0. && kit_def.shield_pct == 0. {
                        t.stats.shield
                    } else {
                        (t.stats.shield + kit_def.shield_flat) * (1. + kit_def.shield_pct)
                    };
                    let double_down = e.status.double_down;
                    let mut consume_minions_foot = false;
                    let (damage, was_crit) = if e.type_id == lorelai::HERO {
                        (
                            shop::mitigated(
                                10. + eff_wp + bonus,
                                eff_shield,
                                e.item_stats.shield_pierce + kit_atk.shield_pierce,
                            ),
                            false,
                        )
                    } else if e.kind == Kind::Crystal {
                        (t.max_hp * 0.04, false)
                    } else {
                        let multiplier = if e.kind == Kind::Hero
                            && matches!(t.kind, Kind::Hero | Kind::Minion)
                        {
                            if double_down.is_some() {
                                1.5 + e.item_stats.crit_damage + kit_atk.crit_damage
                            } else if t.kind == Kind::Hero
                                && e.rt().is_some_and(|rt| rt.minions_foot_ready)
                            {
                                consume_minions_foot = true;
                                1.5 + e.item_stats.crit_damage + kit_atk.crit_damage
                            } else if kit_atk.crit_chance == 0. && kit_atk.crit_damage == 0. {
                                shop::weapon_multiplier(&mut self.state.crit_rng, e.item_stats)
                            } else {
                                let mut items = e.item_stats;
                                items.crit_chance =
                                    (items.crit_chance + kit_atk.crit_chance).min(1.);
                                items.crit_damage += kit_atk.crit_damage;
                                shop::weapon_multiplier(&mut self.state.crit_rng, items)
                            }
                        } else {
                            1.
                        };
                        let buff_mult = if e.kind == Kind::Hero {
                            if basic_crystal {
                                self.hero_5v5_crystal_multiplier(source, t.kind)
                            } else {
                                self.hero_5v5_weapon_multiplier(source, t.kind)
                            }
                        } else {
                            1.
                        };
                        let raw = if kit_atk.damage_amp == 0. {
                            (eff_wp * multiplier + bonus) * buff_mult
                        } else {
                            (eff_wp * multiplier + bonus) * buff_mult * (1. + kit_atk.damage_amp)
                        };
                        let dmg = if basic_crystal {
                            shop::mitigated(
                                raw,
                                eff_shield,
                                e.item_stats.shield_pierce + kit_atk.shield_pierce,
                            )
                        } else {
                            shop::mitigated(
                                raw,
                                eff_armor,
                                e.item_stats.armor_pierce + kit_atk.armor_pierce,
                            )
                        };
                        (
                            dmg,
                            double_down.is_some() || consume_minions_foot || multiplier > 1.01,
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
                            eff_shield,
                            e.item_stats.shield_pierce + kit_atk.shield_pierce,
                        )
                    } else if let Some((b, r)) = twirl {
                        shop::mitigated(
                            b.damage.at(r, e.item_stats.crystal, 0.),
                            eff_shield,
                            e.item_stats.shield_pierce + kit_atk.shield_pierce,
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
                                eff_shield,
                                e.item_stats.shield_pierce + kit_atk.shield_pierce,
                            )
                        } else {
                            0.
                        };
                    let item_attack = e.kind == Kind::Hero
                        && (e.item_stats.has_attack_passives() || e.item_runtime.is_some());
                    let impact_basic = item_attack || t.item_runtime.is_some();
                    let npc_launch = self.npc_launch_delay(e, t);
                    let ranged = e.kind == Kind::Hero && e.stats.range >= 4.;
                    let travel = (e.position.dist(t.position) * TICK_HZ as f32 / 18.).ceil() as u64;
                    let interval = self.basic_attack_interval(e, kit_atk.attack_speed);
                    self.entities[i].winding = None;
                    if consume_minions_foot
                        && let Some(rt) = self.entities[i].item_runtime.as_deref_mut()
                    {
                        rt.minions_foot_ready = false;
                    }
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
                    if crystal
                        && !self.entities[j]
                            .status
                            .blocks_movement_impairment(self.state.tick)
                    {
                        self.entities[j].status.slow_until = self.tick + 20;
                        self.entities[j].status.slow_factor = 0.4;
                    }
                    if let Some(delay) = npc_launch {
                        self.entities.spawn_spell(SpellEffect::npc_windup(
                            self.tick + delay,
                            source,
                            team,
                            id,
                            damage,
                        ));
                    } else if ranged {
                        let impact = if impact_basic {
                            SpellEffect::basic_impact
                        } else {
                            SpellEffect::impact
                        };
                        self.entities.spawn_spell(impact(
                            self.tick + travel,
                            source,
                            team,
                            id,
                            damage,
                            school,
                        ));
                    } else if impact_basic {
                        self.basic_hit(source, team, j, damage, school, events);
                        if self.entities[i].kind == Kind::Hero {
                            self.on_5v5_basic_hit(source, j);
                        }
                    } else {
                        self.hurt_basic(source, team, j, damage, school, events);
                        if self.entities[i].kind == Kind::Hero {
                            self.on_5v5_basic_hit(source, j);
                        }
                    }
                    if let Some(p) = pursuit {
                        self.pursuit_impact(i, j, p, events);
                    }
                    if kit_atk.lifesteal > 0.
                        && damage > 0.
                        && matches!(target_kind, Kind::Hero | Kind::Minion | Kind::Neutral)
                    {
                        let a = &mut self.entities[i];
                        if a.hp > 0. {
                            let healed = (damage * kit_atk.lifesteal).min(a.max_hp - a.hp);
                            if healed > 0. {
                                a.hp += healed;
                                events.push(Event::Heal {
                                    source,
                                    target: source,
                                    amount: healed,
                                    hp: a.hp,
                                });
                            }
                        }
                    }
                    if bonus_crystal > 0. {
                        if ranged {
                            let fx = if impact_basic {
                                SpellEffect::item_impact
                            } else {
                                SpellEffect::impact
                            };
                            self.entities.spawn_spell(fx(
                                self.tick + travel,
                                source,
                                team,
                                id,
                                bonus_crystal,
                                DamageSchool::Crystal,
                            ));
                        } else {
                            self.hurt_item(
                                source,
                                team,
                                j,
                                bonus_crystal,
                                DamageSchool::Crystal,
                                events,
                            );
                        }
                    }
                    self.kit_on_basic_attack(i, j, damage, was_crit, events);
                    if self.winner.is_some() {
                        break;
                    }
                }
            }
        }
        if steering_active {
            self.entities.end_steering_step();
        }
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
#[cfg(test)]
mod phase_cost;
