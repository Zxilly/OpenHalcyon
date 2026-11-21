//! 5v5 Sovereign's Rise map mechanics:
//! - 12 jungle camps (initial spawn, respawn timers, leash/pursuit reset, team share gold/XP,
//!   Weapon/Crystal/Healing/Gold Treant buffs, and structured `0x042C` visibility transitions).
//! - Ghostwing (`BossCamp_BotLeft`, 9:00 spawn -> team barrier, Ghostwing Wrath, OOC regen).
//! - Blackclaw (`BossCamp_TopRight`, 15:00 spawn -> capture, 15s descent, allied mid-lane push
//!   scaling across consecutive captures).
//! - Recall (`Withdraw` 8s channel with damage/CC/move interruption), Homeguard, `spawn_stages`
//!   (`Buff_SpawnStage_Recharge` & `Buff_SpawnStage_Murder`), 44-brush true-sight/stealth,
//!   river flow speed boost, and River Shops.

use super::*;
use crate::native_camps::NEUTRAL_TEAM;
use halcyon_data::NativeMaps;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const TEMPLATE_GHOSTWING: u16 = 331;
pub const TEMPLATE_BLACKCLAW_UNCAPTURED: u16 = 332;
pub const TEMPLATE_BLACKCLAW_CAPTURED: u16 = 333;

/// Initial ordinary camp spawn (`35s`, 3.9/5v5 rules).
pub const CAMP_SPAWN_TICK: u64 = 35 * TICK_HZ;
/// Standard non-buff camp respawn delay after final unit clears (`105s`).
pub const NORMAL_CAMP_RESPAWN_TICKS: u64 = 105 * TICK_HZ;
/// Weapon/Crystal Treant camp respawn delay after clear (`125s`).
pub const BUFF_CAMP_RESPAWN_TICKS: u64 = 125 * TICK_HZ;
/// Distance from home spawn before an ordinary jungle monster leashes back.
pub const CAMP_LEASH_RADIUS: f32 = 8.5;
/// Distance from pit center before Ghostwing or uncaptured Blackclaw leashes back.
pub const BOSS_LEASH_RADIUS: f32 = 10.0;
/// Ghostwing initial spawn (`9:00`).
pub const GHOSTWING_SPAWN_TICK: u64 = 540 * TICK_HZ;
/// Blackclaw initial spawn (`15:00`).
pub const BLACKCLAW_SPAWN_TICK: u64 = 900 * TICK_HZ;
/// Descent delay between uncaptured Blackclaw kill and allied lane push spawn (`15s`).
pub const BLACKCLAW_DESCENT_TICKS: u64 = 15 * TICK_HZ;
/// Boss respawn timer after Ghostwing defeat or captured Blackclaw death (`4:00`).
pub const BOSS_RESPAWN_TICKS: u64 = 240 * TICK_HZ;
/// Recall (`Withdraw`) channel duration (`8s`).
pub const RECALL_CHANNEL_TICKS: u64 = 8 * TICK_HZ;
/// Homeguard spawn-stage speed boost window (`6s`).
pub const HOMEGUARD_DURATION_TICKS: u64 = 6 * TICK_HZ;
/// River shop activation time (`4:00`).
pub const RIVER_SHOP_SPAWN_TICK: u64 = 240 * TICK_HZ;

const TREANT_BUFF_DURATION_TICKS: u64 = 70 * TICK_HZ;
const WEAPON_TREANT_SLOW_TICKS: u64 = 2 * TICK_HZ;
const WEAPON_TREANT_SLOW_FACTOR: f32 = 0.80;
const WEAPON_TREANT_AMP: f32 = 0.15;
const CRYSTAL_TREANT_AMP: f32 = 0.15;
const CRYSTAL_TREANT_ENERGY_PER_SEC: f32 = 10.0;
const GHOSTWING_DURATION_TICKS: u64 = 90 * TICK_HZ;
const GHOSTWING_COMBAT_AMP: f32 = 0.25;
const GHOSTWING_STRUCTURE_AMP: f32 = 0.15;
const OOC_DELAY_TICKS: u64 = 5 * TICK_HZ;
const COMBAT_LOCKOUT_TICKS: u64 = 3 * TICK_HZ;
const RIVER_FLOW_SPEED_BONUS: f32 = 1.5;
const RIVER_CROSS_SPEED_BONUS: f32 = 0.6;
const HOMEGUARD_SPEED_BONUS: f32 = 2.5;
const STAGE_MURDER_TRUE_DAMAGE_PER_TICK: f32 = 500.0;
const TEAM_SHARE_RADIUS: f32 = 10.0;
const TURRET_TRUE_SIGHT_RADIUS: f32 = 10.5;
const RIVER_SHOP_LEFT: Position = Position { x: -16.3, y: -33.6 };
const RIVER_SHOP_RIGHT: Position = Position { x: 16.3, y: 33.6 };

/// Camp icon visibility codes mirroring wire `0x042C` (`CampVisibilityState`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[repr(u8)]
pub enum CampVisibilityState5v5 {
    EmptyHidden = 0,
    EmptyVisible = 1,
    PopulatedHidden = 2,
    PopulatedVisible = 3,
}

/// Structured camp visibility transition for live `0x042C` replication.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CampVisibilityTransition5v5 {
    pub actor_id: u32,
    pub viewer_index: u8,
    pub state: CampVisibilityState5v5,
}

/// Closed classification of Sovereign's Rise jungle and boss camps.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CampKind5v5 {
    HealingAndSmallBear,
    GoldTreant,
    CrystalTreant,
    BigBearPair,
    WeaponTreant,
}

impl CampKind5v5 {
    pub const fn is_buff_camp(self) -> bool {
        matches!(self, Self::CrystalTreant | Self::WeaponTreant)
    }

    pub const fn marker_template(self) -> u16 {
        if self.is_buff_camp() { 350 } else { 349 }
    }

    pub const fn respawn_ticks(self) -> u64 {
        if self.is_buff_camp() {
            BUFF_CAMP_RESPAWN_TICKS
        } else {
            NORMAL_CAMP_RESPAWN_TICKS
        }
    }

    fn unit_specs(self) -> &'static [(EntityTemplate, &'static str)] {
        match self {
            Self::HealingAndSmallBear => &[
                (EntityTemplate::HealingTreant, "5v5_HealingTreant"),
                (EntityTemplate::SmallBear, "5v5_SmallBear"),
            ],
            Self::GoldTreant => &[(EntityTemplate::GoldTreant, "5v5_GoldTreant")],
            Self::CrystalTreant => &[(EntityTemplate::CrystalTreant, "5v5_CrystalTreant")],
            Self::BigBearPair => &[
                (EntityTemplate::BigBear, "5v5_BigBear"),
                (EntityTemplate::BigBear, "5v5_BigBear"),
            ],
            Self::WeaponTreant => &[(EntityTemplate::WeaponTreant, "5v5_WeaponTreant")],
        }
    }
}

#[derive(Clone, Copy)]
struct CampPlacement5v5 {
    actor_id: u32,
    kind: CampKind5v5,
    base: Position,
    yaw_180: bool,
    local_spawns: [Position; 2],
    local_timer: Position,
}

fn rotate_local(p: Position, yaw_180: bool) -> Position {
    if yaw_180 {
        Position { x: -p.x, y: -p.y }
    } else {
        p
    }
}

impl CampPlacement5v5 {
    fn world_pos(&self, local: Position) -> Position {
        let r = rotate_local(local, self.yaw_180);
        Position {
            x: self.base.x + r.x,
            y: self.base.y + r.y,
        }
    }
}

const CAMP_C1: Position = Position { x: 4.91, y: 2.15 };
const CAMP_C2: Position = Position { x: 1.42, y: 3.49 };
const CAMP_CT: Position = Position { x: 2.165, y: 2.82 };
const CAMP_D1: Position = Position { x: -3.16, y: -4.6 };
const CAMP_BB1: Position = Position { x: -1.36, y: 1.68 };
const CAMP_B1: Position = Position { x: 0.0, y: 0.0 };
const CAMP_B2: Position = Position { x: 0.03, y: 3.45 };
const CAMP_BT: Position = Position { x: 0.015, y: 1.725 };
const CAMP_BA1: Position = Position { x: 2.95, y: -2.88 };
const CAMP_A1: Position = Position { x: 4.17, y: -3.75 };
const CAMP_A2: Position = Position { x: 1.34, y: -5.32 };
const CAMP_AT: Position = Position {
    x: 2.755,
    y: -4.535,
};

const ORDINARY_CAMPS: [CampPlacement5v5; 12] = [
    CampPlacement5v5 {
        actor_id: 6580,
        kind: CampKind5v5::HealingAndSmallBear,
        base: Position { x: 21.96, y: -40.0 },
        yaw_180: true,
        local_spawns: [CAMP_C1, CAMP_C2],
        local_timer: CAMP_CT,
    },
    CampPlacement5v5 {
        actor_id: 6581,
        kind: CampKind5v5::HealingAndSmallBear,
        base: Position { x: -21.96, y: 40.0 },
        yaw_180: false,
        local_spawns: [CAMP_C1, CAMP_C2],
        local_timer: CAMP_CT,
    },
    CampPlacement5v5 {
        actor_id: 6582,
        kind: CampKind5v5::GoldTreant,
        base: Position {
            x: 31.05,
            y: -19.31,
        },
        yaw_180: true,
        local_spawns: [CAMP_D1, CAMP_D1],
        local_timer: CAMP_D1,
    },
    CampPlacement5v5 {
        actor_id: 6583,
        kind: CampKind5v5::GoldTreant,
        base: Position {
            x: -31.05,
            y: 19.31,
        },
        yaw_180: false,
        local_spawns: [CAMP_D1, CAMP_D1],
        local_timer: CAMP_D1,
    },
    CampPlacement5v5 {
        actor_id: 6584,
        kind: CampKind5v5::CrystalTreant,
        base: Position {
            x: 22.93,
            y: -26.66,
        },
        yaw_180: true,
        local_spawns: [CAMP_BB1, CAMP_BB1],
        local_timer: CAMP_BB1,
    },
    CampPlacement5v5 {
        actor_id: 6585,
        kind: CampKind5v5::CrystalTreant,
        base: Position {
            x: -22.93,
            y: 26.66,
        },
        yaw_180: false,
        local_spawns: [CAMP_BB1, CAMP_BB1],
        local_timer: CAMP_BB1,
    },
    CampPlacement5v5 {
        actor_id: 6586,
        kind: CampKind5v5::BigBearPair,
        base: Position { x: 34.47, y: 18.42 },
        yaw_180: true,
        local_spawns: [CAMP_B1, CAMP_B2],
        local_timer: CAMP_BT,
    },
    CampPlacement5v5 {
        actor_id: 6587,
        kind: CampKind5v5::BigBearPair,
        base: Position {
            x: -34.47,
            y: -18.42,
        },
        yaw_180: false,
        local_spawns: [CAMP_B1, CAMP_B2],
        local_timer: CAMP_BT,
    },
    CampPlacement5v5 {
        actor_id: 6588,
        kind: CampKind5v5::WeaponTreant,
        base: Position { x: 20.86, y: 20.11 },
        yaw_180: true,
        local_spawns: [CAMP_BA1, CAMP_BA1],
        local_timer: CAMP_BA1,
    },
    CampPlacement5v5 {
        actor_id: 6589,
        kind: CampKind5v5::WeaponTreant,
        base: Position {
            x: -20.86,
            y: -20.11,
        },
        yaw_180: false,
        local_spawns: [CAMP_BA1, CAMP_BA1],
        local_timer: CAMP_BA1,
    },
    CampPlacement5v5 {
        actor_id: 6590,
        kind: CampKind5v5::HealingAndSmallBear,
        base: Position { x: 22.88, y: 39.42 },
        yaw_180: true,
        local_spawns: [CAMP_A1, CAMP_A2],
        local_timer: CAMP_AT,
    },
    CampPlacement5v5 {
        actor_id: 6591,
        kind: CampKind5v5::HealingAndSmallBear,
        base: Position {
            x: -22.88,
            y: -39.42,
        },
        yaw_180: false,
        local_spawns: [CAMP_A1, CAMP_A2],
        local_timer: CAMP_AT,
    },
];

pub const BLACKCLAW_PIT_MARKER: u32 = 6566;
pub const GHOSTWING_PIT_MARKER: u32 = 6567;
const BLACKCLAW_PIT_POS: Position = Position { x: 2.85, y: -29.77 };
const GHOSTWING_PIT_POS: Position = Position { x: -2.85, y: 29.77 };

fn fallback_unit_stats_raw(template: u16) -> Stats {
    if let Some(known) = EntityTemplate::from_raw(template) {
        let mut s = match known {
            EntityTemplate::HealingTreant => Stats::new(known, 920., 31., 100., 2.4, 3.6)
                .with_growth(Growth {
                    hp: 200.,
                    weapon: 11.,
                    ..Growth::default()
                })
                .with_bounty(50., 13.),
            EntityTemplate::SmallBear => Stats::new(known, 410., 15., 100., 1.0, 3.6)
                .with_growth(Growth {
                    hp: 120.,
                    weapon: 4.,
                    ..Growth::default()
                })
                .with_bounty(25., 6.),
            EntityTemplate::GoldTreant => Stats::new(known, 1280., 40., 100., 2.4, 3.6)
                .with_growth(Growth {
                    hp: 370.,
                    weapon: 15.,
                    ..Growth::default()
                })
                .with_bounty(220., 40.),
            EntityTemplate::BigBear => Stats::new(known, 710., 24., 100., 2.4, 3.6)
                .with_growth(Growth {
                    hp: 160.,
                    weapon: 7.,
                    ..Growth::default()
                })
                .with_bounty(90., 24.),
            EntityTemplate::WeaponTreant | EntityTemplate::CrystalTreant => {
                Stats::new(known, 1280., 40., 100., 2.4, 3.6)
                    .with_growth(Growth {
                        hp: 370.,
                        weapon: 15.,
                        ..Growth::default()
                    })
                    .with_bounty(80., 30.)
            }
            _ => Stats::new(known, 500., 20., 100., 2.0, 3.6),
        };
        s.shield = 100.;
        return s;
    }
    let mut s = match template {
        TEMPLATE_GHOSTWING => Stats::new(EntityTemplate::GoldTreant, 7500., 70., 100., 9.0, 3.3)
            .with_growth(Growth {
                hp: 1250.,
                weapon: 20.,
                ..Growth::default()
            })
            .with_bounty(0., 0.),
        TEMPLATE_BLACKCLAW_UNCAPTURED => {
            Stats::new(EntityTemplate::GoldTreant, 13500., 90., 100., 4.0, 2.8)
                .with_growth(Growth {
                    hp: 1750.,
                    weapon: 35.,
                    ..Growth::default()
                })
                .with_bounty(0., 0.)
        }
        TEMPLATE_BLACKCLAW_CAPTURED => {
            Stats::new(EntityTemplate::GoldTreant, 13500., 210., 100., 4.0, 4.0)
                .with_growth(Growth {
                    hp: 2500.,
                    weapon: 30.,
                    ..Growth::default()
                })
                .with_bounty(0., 0.)
        }
        _ => Stats::new(EntityTemplate::GoldTreant, 500., 20., 100., 2.0, 3.6),
    };
    s.id = template;
    s.shield = 100.;
    s
}

trait StatsGrowthExt {
    fn with_growth(self, growth: Growth) -> Self;
    fn with_bounty(self, gold: f32, xp: f32) -> Self;
}

impl StatsGrowthExt for Stats {
    fn with_growth(mut self, growth: Growth) -> Self {
        self.growth = growth;
        self
    }

    fn with_bounty(mut self, gold: f32, xp: f32) -> Self {
        self.physiology = Some(Physiology {
            hp_regen: 0.,
            hp_regen_growth: 0.,
            energy_regen: 0.,
            energy_regen_growth: 0.,
            attack_speed: 1.0,
            attack_speed_growth: 0.,
            collision_radius: 0.,
            sight: 10.0,
            gold_bounty: gold,
            xp_bounty: xp,
        });
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecallState {
    pub started_tick: u64,
    pub complete_at: u64,
}

/// Active 5v5 buffs & channel state tracked per hero seat (`0..10`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HeroBuffs5v5 {
    pub weapon_treant_until: u64,
    pub crystal_treant_until: u64,
    pub ghostwing_until: u64,
    pub homeguard_until: u64,
    pub river_bonus_speed: f32,
    pub last_combat_tick: u64,
    pub recall: Option<RecallState>,
}

/// Public snapshot of a hero's active 5v5 buffs/channels for inspection and tests.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hero5v5Status {
    pub weapon_treant_active: bool,
    pub crystal_treant_active: bool,
    pub ghostwing_active: bool,
    pub homeguard_active: bool,
    pub river_bonus_speed: f32,
    pub recalling_until: Option<u64>,
    pub in_own_spawn_stage: bool,
    pub in_enemy_spawn_stage: bool,
}

#[derive(Clone, Debug, Serialize)]
struct CampSlotState {
    actor_id: u32,
    kind: CampKind5v5,
    timer_pos: Position,
    spawns: [Position; 2],
    alive_ids: Vec<u32>,
    next_spawn_tick: Option<u64>,
    last_visibility: [Option<CampVisibilityState5v5>; 3],
}

impl CampSlotState {
    fn is_populated(&self) -> bool {
        !self.alive_ids.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
struct NeutralHome {
    home: Position,
    leash_radius: f32,
    camp_index: Option<usize>,
    is_boss: bool,
    resetting: bool,
}

#[derive(Clone, Debug, Serialize)]
struct BossPitState {
    entity_id: Option<u32>,
    next_spawn_tick: Option<u64>,
    captures: [u32; 2],
    pending_descent: Option<(u64, u8, u32)>,
    captured_id: Option<u32>,
    captured_team: Option<u8>,
}

#[derive(Clone, Serialize)]
struct UnitPalette5v5 {
    healing_treant: Stats,
    small_bear: Stats,
    gold_treant: Stats,
    big_bear: Stats,
    weapon_treant: Stats,
    crystal_treant: Stats,
    ghostwing: Stats,
    blackclaw_uncaptured: Stats,
    blackclaw_captured: Stats,
}

impl UnitPalette5v5 {
    fn from_data(data: Option<&GameData>) -> Self {
        let load_one = |template: u16, name: &str| {
            let mut s = data
                .and_then(|d| d.unit(name).map(|u| Stats::load(template, u, d)))
                .unwrap_or_else(|| fallback_unit_stats_raw(template));
            if s.speed <= 0. {
                s.speed = 3.5;
            }
            s
        };
        Self {
            healing_treant: load_one(EntityTemplate::HealingTreant.as_raw(), "5v5_HealingTreant"),
            small_bear: load_one(EntityTemplate::SmallBear.as_raw(), "5v5_SmallBear"),
            gold_treant: load_one(EntityTemplate::GoldTreant.as_raw(), "5v5_GoldTreant"),
            big_bear: load_one(EntityTemplate::BigBear.as_raw(), "5v5_BigBear"),
            weapon_treant: load_one(EntityTemplate::WeaponTreant.as_raw(), "5v5_WeaponTreant"),
            crystal_treant: load_one(EntityTemplate::CrystalTreant.as_raw(), "5v5_CrystalTreant"),
            ghostwing: load_one(TEMPLATE_GHOSTWING, "5v5_Ghostwing"),
            blackclaw_uncaptured: load_one(
                TEMPLATE_BLACKCLAW_UNCAPTURED,
                "5v5_Blackclaw_Uncaptured",
            ),
            blackclaw_captured: load_one(TEMPLATE_BLACKCLAW_CAPTURED, "5v5_Blackclaw_Captured"),
        }
    }

    fn stats_for_raw(&self, template: u16) -> Stats {
        match EntityTemplate::from_raw(template) {
            Some(EntityTemplate::HealingTreant) => self.healing_treant,
            Some(EntityTemplate::SmallBear) => self.small_bear,
            Some(EntityTemplate::GoldTreant) => self.gold_treant,
            Some(EntityTemplate::BigBear) => self.big_bear,
            Some(EntityTemplate::WeaponTreant) => self.weapon_treant,
            Some(EntityTemplate::CrystalTreant) => self.crystal_treant,
            _ => match template {
                TEMPLATE_GHOSTWING => self.ghostwing,
                TEMPLATE_BLACKCLAW_UNCAPTURED => self.blackclaw_uncaptured,
                TEMPLATE_BLACKCLAW_CAPTURED => self.blackclaw_captured,
                _ => fallback_unit_stats_raw(template),
            },
        }
    }
}

/// Complete authoritative 5v5 Sovereign's Rise runtime state.
#[derive(Clone, Serialize)]
pub struct Map5v5State {
    palette: UnitPalette5v5,
    camps: Vec<CampSlotState>,
    neutral_homes: BTreeMap<u32, NeutralHome>,
    ghostwing: BossPitState,
    blackclaw: BossPitState,
    heroes: [HeroBuffs5v5; 10],
    pending_camp_transitions: Vec<CampVisibilityTransition5v5>,
}

impl Map5v5State {
    fn new(data: Option<&GameData>, maps: Option<&NativeMaps>) -> Self {
        let palette = UnitPalette5v5::from_data(data);
        let placed_override = maps.and_then(|m| {
            let level = m.solo()?;
            let mut list: Vec<_> = m.placed_camps(level).collect();
            list.sort_by_key(|c| c.placement.name.as_str());
            Some(list)
        });
        let camps = ORDINARY_CAMPS
            .iter()
            .map(|def| {
                let mut spawns = [
                    def.world_pos(def.local_spawns[0]),
                    def.world_pos(def.local_spawns[1]),
                ];
                let mut timer_pos = def.world_pos(def.local_timer);
                if let Some(ref placed) = placed_override
                    && let Some(found) = placed.iter().find(|c| {
                        let p = Position {
                            x: c.placement.pos[0],
                            y: c.placement.pos[2],
                        };
                        p.dist(def.base) < 1.5
                    })
                {
                    if let Some(m0) = found
                        .definition
                        .local_spawns
                        .iter()
                        .find(|s| s.name == "Spawn1")
                        .and_then(|s| found.spawn_position(s))
                    {
                        spawns[0] = Position { x: m0[0], y: m0[2] };
                    }
                    if let Some(m1) = found
                        .definition
                        .local_spawns
                        .iter()
                        .find(|s| s.name == "Spawn2")
                        .and_then(|s| found.spawn_position(s))
                    {
                        spawns[1] = Position { x: m1[0], y: m1[2] };
                    }
                    if let Some(mt) = found
                        .definition
                        .local_spawns
                        .iter()
                        .find(|s| s.name == "Timer")
                        .and_then(|s| found.spawn_position(s))
                    {
                        timer_pos = Position { x: mt[0], y: mt[2] };
                    }
                }
                CampSlotState {
                    actor_id: def.actor_id,
                    kind: def.kind,
                    timer_pos,
                    spawns,
                    alive_ids: Vec::new(),
                    next_spawn_tick: Some(CAMP_SPAWN_TICK),
                    last_visibility: [None; 3],
                }
            })
            .collect();
        Self {
            palette,
            camps,
            neutral_homes: BTreeMap::new(),
            ghostwing: BossPitState {
                entity_id: None,
                next_spawn_tick: Some(GHOSTWING_SPAWN_TICK),
                captures: [0; 2],
                pending_descent: None,
                captured_id: None,
                captured_team: None,
            },
            blackclaw: BossPitState {
                entity_id: None,
                next_spawn_tick: Some(BLACKCLAW_SPAWN_TICK),
                captures: [0; 2],
                pending_descent: None,
                captured_id: None,
                captured_team: None,
            },
            heroes: [HeroBuffs5v5 {
                weapon_treant_until: 0,
                crystal_treant_until: 0,
                ghostwing_until: 0,
                homeguard_until: HOMEGUARD_DURATION_TICKS * 4,
                river_bonus_speed: 0.,
                last_combat_tick: 0,
                recall: None,
            }; 10],
            pending_camp_transitions: Vec::new(),
        }
    }

    pub(super) fn digest(&self, hash: &mut impl std::io::Write) {
        serde_json::to_writer(hash, self).expect("finite 5v5 map state");
    }

    pub(super) fn at_river_shop(&self, tick: u64, pos: Position) -> bool {
        tick >= RIVER_SHOP_SPAWN_TICK
            && (pos.dist(RIVER_SHOP_LEFT) <= 4.5 || pos.dist(RIVER_SHOP_RIGHT) <= 4.5)
    }

    pub(super) fn is_resetting(&self, id: u32) -> bool {
        self.neutral_homes
            .get(&id)
            .is_some_and(|home| home.resetting)
    }
}

fn scaled_unit_stats(mut base: Stats, level: u8) -> (Stats, f32) {
    let extra = f32::from(level.saturating_sub(1));
    let max_hp = base.hp + base.growth.hp * extra;
    base.hp = max_hp;
    base.weapon += base.growth.weapon * extra;
    (base, max_hp)
}

fn camp_level_at(tick: u64, scaling_start_secs: u64, max_level: u8) -> u8 {
    let secs = tick / TICK_HZ;
    let extra = secs.saturating_sub(scaling_start_secs) / 120;
    (1 + extra as u8).min(max_level)
}

/// Checks whether a position lies inside the diagonal central Sovereign's Rise river channel,
/// returning the outward flow unit direction if present.
pub fn river_outward_flow(p: Position) -> Option<Position> {
    // Axis from center (0, 0) toward RRiverShop (16.3, 33.6), mirrored for LRiverShop (-16.3, -33.6).
    let len = (16.3_f32 * 16.3 + 33.6 * 33.6).sqrt();
    let ux = 16.3 / len;
    let uy = 33.6 / len;
    let proj = p.x * ux + p.y * uy;
    if proj.abs() > 58.0 {
        return None;
    }
    let perp = (-p.x * uy + p.y * ux).abs();
    if perp > 6.0 {
        return None;
    }
    let sign = if proj >= 0. { 1.0 } else { -1.0 };
    Some(Position {
        x: ux * sign,
        y: uy * sign,
    })
}

pub fn in_5v5_bounds(p: Position) -> bool {
    (-110.0..=110.0).contains(&p.x) && (-70.0..=70.0).contains(&p.y)
}

fn fallback_in_spawn_stage(team: u8, p: Position) -> bool {
    let x_ok = if team == 0 {
        (-105.82..=-86.51).contains(&p.x)
    } else {
        (86.51..=105.82).contains(&p.x)
    };
    x_ok && (-10.15..=10.15).contains(&p.y)
}

impl Room<'_> {
    /// Configure full 5v5 Sovereign's Rise mechanics atop the 5v5 20-turret / 6-armory map.
    pub fn use_sovereigns_rise_5v5(&mut self, data: Option<&GameData>) -> Result<(), String> {
        if self.tick != 0 {
            return Err("5v5 Sovereign's Rise setup requires an unstarted world".into());
        }
        self.use_solo_mid_lane()?;
        let maps = data.and_then(|d| d.native_maps());
        if let Some(m) = maps
            && self.native_navigation.is_none()
        {
            self.install_native_navigation(m)?;
        }
        // 5v5 map disables legacy Solo 18-camp preloader so Map5v5 owns camp lifecycle & timers.
        self.native_camps = None;
        self.map_5v5 = Some(Box::new(Map5v5State::new(data, maps)));
        Ok(())
    }

    pub fn is_5v5_enabled(&self) -> bool {
        self.map_5v5.is_some()
    }

    /// Returns the wire shopkeeper actor ID (`6530` left base, `6531` right base,
    /// `6532` left river, `6533` right river) if `id` is an alive hero currently
    /// eligible to buy at an unlocked 5v5 item shop.
    pub fn shop_5v5_keeper_for_actor(&self, id: u32) -> Option<u32> {
        let hero = self.index(id).map(|i| &self.entities[i])?;
        if hero.kind != Kind::Hero || hero.hp <= 0. || self.winner.is_some() {
            return None;
        }
        if self
            .map_5v5
            .as_ref()
            .is_some_and(|map| map.at_river_shop(self.tick, hero.position))
        {
            return Some(if hero.position.x < 0. { 6532 } else { 6533 });
        }
        if self.at_item_shop(hero) {
            return Some(if hero.team == 0 { 6530 } else { 6531 });
        }
        None
    }

    pub fn hero_5v5_status(&self, id: u32) -> Option<Hero5v5Status> {
        let map = self.map_5v5.as_ref()?;
        let idx = usize::try_from(id.checked_sub(1)?).ok()?;
        let buffs = map.heroes.get(idx)?;
        let entity = self.entities.iter().find(|e| e.id == id)?;
        Some(Hero5v5Status {
            weapon_treant_active: buffs.weapon_treant_until > self.tick,
            crystal_treant_active: buffs.crystal_treant_until > self.tick,
            ghostwing_active: buffs.ghostwing_until > self.tick,
            homeguard_active: buffs.homeguard_until > self.tick
                && self.tick >= buffs.last_combat_tick.saturating_add(COMBAT_LOCKOUT_TICKS),
            river_bonus_speed: buffs.river_bonus_speed,
            recalling_until: buffs.recall.map(|r| r.complete_at),
            in_own_spawn_stage: self.in_spawn_stage(entity.team, entity.position),
            in_enemy_spawn_stage: self.in_spawn_stage(1 - entity.team, entity.position),
        })
    }

    /// Drains pending `0x042C` camp visibility transitions emitted on simulated ticks.
    pub fn take_5v5_camp_transitions(&mut self) -> Vec<CampVisibilityTransition5v5> {
        self.map_5v5
            .as_mut()
            .map(|m| std::mem::take(&mut m.pending_camp_transitions))
            .unwrap_or_default()
    }

    /// Current cached camp marker visibility states `[viewer0, viewer1, viewer2]`.
    pub fn camp_5v5_markers(
        &self,
    ) -> Vec<(
        u32,
        CampKind5v5,
        Position,
        [Option<CampVisibilityState5v5>; 3],
    )> {
        let Some(map) = &self.map_5v5 else {
            return Vec::new();
        };
        map.camps
            .iter()
            .map(|c| (c.actor_id, c.kind, c.timer_pos, c.last_visibility))
            .collect()
    }

    /// World placements for camp & boss minimap markers (`6580..=6591`, `6566`, `6567`).
    pub fn camp_5v5_declarations(&self) -> Vec<(u32, u16, Position)> {
        let Some(map) = &self.map_5v5 else {
            return Vec::new();
        };
        let mut out: Vec<_> = map
            .camps
            .iter()
            .map(|c| (c.actor_id, c.kind.marker_template(), c.timer_pos))
            .collect();
        out.push((BLACKCLAW_PIT_MARKER, 351, BLACKCLAW_PIT_POS));
        out.push((GHOSTWING_PIT_MARKER, 352, GHOSTWING_PIT_POS));
        out
    }

    pub fn ghostwing_status(&self) -> Option<(Option<u32>, [u32; 2])> {
        let map = self.map_5v5.as_ref()?;
        Some((map.ghostwing.entity_id, map.ghostwing.captures))
    }

    pub fn blackclaw_status(&self) -> Option<(Option<u32>, Option<u32>, [u32; 2])> {
        let map = self.map_5v5.as_ref()?;
        Some((
            map.blackclaw.entity_id,
            map.blackclaw.captured_id,
            map.blackclaw.captures,
        ))
    }

    /// Directly inspect whether `team` is in its own or enemy `spawn_stage` polygon/bounds.
    pub(super) fn in_spawn_stage(&self, team: u8, pos: Position) -> bool {
        if let Some(nav) = &self.native_navigation {
            nav.in_spawn_stage(team, pos)
        } else {
            fallback_in_spawn_stage(team, pos)
        }
    }

    pub(super) fn is_5v5_resetting(&self, id: u32) -> bool {
        self.map_5v5.as_ref().is_some_and(|m| m.is_resetting(id))
    }

    pub(super) fn begin_recall(&mut self, i: usize) -> Result<(), &'static str> {
        let tick = self.tick;
        let state = &mut *self.state;
        if state.map_5v5.is_none() {
            return Err("recall requires 5v5 mode");
        }
        let e = &mut self.entities[i];
        if e.hp <= 0. {
            return Err("dead");
        }
        if e.status.stun_until > tick || e.status.root_until > tick || e.status.silence_until > tick
        {
            return Err("crowd controlled");
        }
        let seat = usize::try_from(e.id - 1).map_err(|_| "not a hero")?;
        if seat >= 10 {
            return Err("not a hero");
        }
        e.order = Command::Stop;
        e.winding = None;
        e.cast = None;
        let map = state.map_5v5.as_mut().unwrap();
        map.heroes[seat].recall = Some(RecallState {
            started_tick: tick,
            complete_at: tick + RECALL_CHANNEL_TICKS,
        });
        Ok(())
    }

    pub(super) fn interrupt_recall(&mut self, id: u32, events: &mut Vec<Event>) {
        let Some(map) = self.map_5v5.as_mut() else {
            return;
        };
        let Some(seat) = id.checked_sub(1).and_then(|s| usize::try_from(s).ok()) else {
            return;
        };
        if seat < 10 && map.heroes[seat].recall.take().is_some() {
            events.push(Event::RecallInterrupted { id });
        }
    }

    pub(super) fn interrupt_recall_from_command(&mut self, i: usize) {
        let id = self.entities[i].id;
        let Some(map) = self.map_5v5.as_mut() else {
            return;
        };
        let Some(seat) = id.checked_sub(1).and_then(|s| usize::try_from(s).ok()) else {
            return;
        };
        if seat < 10 && map.heroes[seat].recall.take().is_some() {
            self.pending_item_events
                .push(Event::RecallInterrupted { id });
        }
    }

    pub(super) fn move_speed_5v5_bonus(&self, hero_id: u32) -> f32 {
        let Some(map) = self.map_5v5.as_ref() else {
            return 0.;
        };
        let Some(seat) = hero_id.checked_sub(1).and_then(|s| usize::try_from(s).ok()) else {
            return 0.;
        };
        let Some(buffs) = map.heroes.get(seat) else {
            return 0.;
        };
        let mut bonus = buffs.river_bonus_speed;
        if buffs.homeguard_until > self.tick
            && self.tick >= buffs.last_combat_tick.saturating_add(COMBAT_LOCKOUT_TICKS)
        {
            bonus += HOMEGUARD_SPEED_BONUS;
        }
        bonus
    }

    pub(super) fn hero_5v5_weapon_multiplier(&self, src_id: u32, target_kind: Kind) -> f32 {
        let Some(map) = self.map_5v5.as_ref() else {
            return 1.0;
        };
        let Some(seat) = src_id.checked_sub(1).and_then(|s| usize::try_from(s).ok()) else {
            return 1.0;
        };
        let Some(buffs) = map.heroes.get(seat) else {
            return 1.0;
        };
        let mut mult = 1.0;
        if buffs.weapon_treant_until > self.tick {
            mult += WEAPON_TREANT_AMP;
        }
        if buffs.ghostwing_until > self.tick {
            mult += if matches!(target_kind, Kind::Tower | Kind::Crystal | Kind::Armory) {
                GHOSTWING_STRUCTURE_AMP
            } else {
                GHOSTWING_COMBAT_AMP
            };
        }
        mult
    }

    pub(super) fn hero_5v5_crystal_multiplier(&self, src_id: u32, target_kind: Kind) -> f32 {
        let Some(map) = self.map_5v5.as_ref() else {
            return 1.0;
        };
        let Some(seat) = src_id.checked_sub(1).and_then(|s| usize::try_from(s).ok()) else {
            return 1.0;
        };
        let Some(buffs) = map.heroes.get(seat) else {
            return 1.0;
        };
        let mut mult = 1.0;
        if buffs.crystal_treant_until > self.tick {
            mult += CRYSTAL_TREANT_AMP;
        }
        if buffs.ghostwing_until > self.tick {
            mult += if matches!(target_kind, Kind::Tower | Kind::Crystal | Kind::Armory) {
                GHOSTWING_STRUCTURE_AMP
            } else {
                GHOSTWING_COMBAT_AMP
            };
        }
        mult
    }

    /// True-sight check for turrets (`10.5m`) and globally visible captured Blackclaw.
    pub(super) fn has_5v5_true_sight(&self, team: u8, target: &Entity) -> bool {
        if self.map_5v5.is_none() {
            return false;
        }
        if self
            .map_5v5
            .as_ref()
            .is_some_and(|m| m.blackclaw.captured_id == Some(target.id))
        {
            return true;
        }
        self.entities.any_unordered(|e| {
            e.team == team
                && e.hp > 0.
                && e.kind == Kind::Tower
                && e.position.dist(target.position) <= TURRET_TRUE_SIGHT_RADIUS
        })
    }

    pub(super) fn on_5v5_damage(&mut self, src_id: u32, target_id: u32, events: &mut Vec<Event>) {
        let tick = self.tick;
        self.interrupt_recall(target_id, events);
        let Some(map) = self.map_5v5.as_mut() else {
            return;
        };
        for id in [src_id, target_id] {
            if let Some(seat) = id.checked_sub(1).and_then(|s| usize::try_from(s).ok())
                && seat < 10
            {
                map.heroes[seat].last_combat_tick = tick;
                map.heroes[seat].river_bonus_speed = 0.;
            }
        }
    }

    pub(super) fn on_5v5_basic_hit(&mut self, src_id: u32, target_idx: usize) {
        let Some(map) = self.map_5v5.as_ref() else {
            return;
        };
        let Some(seat) = src_id.checked_sub(1).and_then(|s| usize::try_from(s).ok()) else {
            return;
        };
        if seat >= 10 || map.heroes[seat].weapon_treant_until <= self.tick {
            return;
        }
        let target = &mut self.entities[target_idx];
        if !target.status.blocks_movement_impairment(self.state.tick) {
            target.status.slow_until = target
                .status
                .slow_until
                .max(self.state.tick + WEAPON_TREANT_SLOW_TICKS);
            target.status.slow_factor =
                if target.status.slow_until > self.state.tick && target.status.slow_factor > 0. {
                    target.status.slow_factor.min(WEAPON_TREANT_SLOW_FACTOR)
                } else {
                    WEAPON_TREANT_SLOW_FACTOR
                };
        }
    }
}

/// Context describing a slain neutral or captured boss unit in 5v5 mode.
#[derive(Clone, Copy, Debug)]
pub(super) struct NeutralDeath5v5 {
    pub(super) dead_id: u32,
    pub(super) dead_pos: Position,
    pub(super) dead_level: u8,
    pub(super) dead_type_id: u16,
    pub(super) killer_id: u32,
    pub(super) killer_team: u8,
    pub(super) bounty: u32,
    pub(super) xp: f32,
}

impl Room<'_> {
    pub(super) fn handle_5v5_neutral_death(
        &mut self,
        dead: NeutralDeath5v5,
        events: &mut Vec<Event>,
    ) -> bool {
        let NeutralDeath5v5 {
            dead_id,
            dead_level,
            dead_type_id,
            killer_id,
            killer_team,
            ..
        } = dead;
        let Some(mut map) = self.map_5v5.take() else {
            return false;
        };
        let tick = self.tick;
        let is_blackclaw_captured = map.blackclaw.captured_id == Some(dead_id);
        let is_blackclaw_pit = map.blackclaw.entity_id == Some(dead_id);
        let is_ghostwing = map.ghostwing.entity_id == Some(dead_id);
        let home_entry = map.neutral_homes.remove(&dead_id);

        if is_blackclaw_captured {
            map.blackclaw.captured_id = None;
            map.blackclaw.captured_team = None;
            map.blackclaw.next_spawn_tick = Some(tick + BOSS_RESPAWN_TICKS);
            self.map_5v5 = Some(map);
            return true;
        }

        if let Some(entry) = home_entry
            && let Some(camp_idx) = entry.camp_index
        {
            let camp = &mut map.camps[camp_idx];
            camp.alive_ids.retain(|&id| id != dead_id);
            if camp.alive_ids.is_empty() {
                camp.next_spawn_tick = Some(tick + camp.kind.respawn_ticks());
            }
            // Team-sharing gold and XP among killer + nearby alive teammates.
            let extra_gold = if dead_type_id == EntityTemplate::GoldTreant.as_raw() {
                60
            } else {
                0
            };
            self.distribute_5v5_camp_rewards(&dead, extra_gold, events);
            // Apply specific Treant buffs to killer hero.
            if let Some(killer_idx) = self.index(killer_id)
                && self.entities[killer_idx].kind == Kind::Hero
                && self.entities[killer_idx].hp > 0.
                && let Some(seat) = killer_id
                    .checked_sub(1)
                    .and_then(|s| usize::try_from(s).ok())
                && seat < 10
            {
                let hero = &mut self.entities[killer_idx];
                match EntityTemplate::from_raw(dead_type_id) {
                    Some(EntityTemplate::WeaponTreant) => {
                        map.heroes[seat].weapon_treant_until = tick + TREANT_BUFF_DURATION_TICKS;
                        events.push(Event::CampBuffApplied {
                            actor: killer_id,
                            source: dead_id,
                            template: dead_type_id,
                        });
                    }
                    Some(EntityTemplate::CrystalTreant) => {
                        map.heroes[seat].crystal_treant_until = tick + TREANT_BUFF_DURATION_TICKS;
                        events.push(Event::CampBuffApplied {
                            actor: killer_id,
                            source: dead_id,
                            template: dead_type_id,
                        });
                    }
                    Some(EntityTemplate::HealingTreant) => {
                        let heal_hp = 120. + 15. * f32::from(dead_level);
                        let restore_e = 60. + 5. * f32::from(dead_level);
                        let barrier = 150. + 25. * f32::from(dead_level);
                        let before = hero.hp;
                        hero.hp = (hero.hp + heal_hp).min(hero.max_hp);
                        hero.energy = (hero.energy + restore_e).min(hero.max_energy);
                        hero.status.barrier = hero.status.barrier.max(barrier);
                        hero.status.barrier_until =
                            hero.status.barrier_until.max(tick + 8 * TICK_HZ);
                        if hero.hp > before {
                            events.push(Event::Heal {
                                source: dead_id,
                                target: killer_id,
                                amount: hero.hp - before,
                                hp: hero.hp,
                            });
                        }
                        events.push(Event::CampBuffApplied {
                            actor: killer_id,
                            source: dead_id,
                            template: dead_type_id,
                        });
                    }
                    _ => {}
                }
            }
            self.map_5v5 = Some(map);
            return true;
        }

        if is_ghostwing && killer_team < 2 {
            map.ghostwing.entity_id = None;
            map.ghostwing.captures[usize::from(killer_team)] += 1;
            map.ghostwing.next_spawn_tick = Some(tick + BOSS_RESPAWN_TICKS);
            let ts = &mut self.state.team_scores[usize::from(killer_team)];
            ts.ghostwing_captures += 1;
            ts.first_ghostwing_capture_seconds
                .get_or_insert(tick as f64 / TICK_HZ as f64);
            for i in 0..self.entities.len().min(10) {
                let hero = &mut self.entities[i];
                if hero.kind == Kind::Hero && hero.team == killer_team {
                    hero.gold += 160;
                    self.state.scores[(hero.id - 1) as usize]
                        .gold_income
                        .last_hit_bounties += 160;
                    events.push(Event::GoldAward {
                        id: hero.id,
                        amount: 160,
                    });
                    if hero.hp > 0. {
                        let barrier = 300. + 40. * f32::from(hero.level.saturating_sub(1));
                        hero.status.barrier = hero.status.barrier.max(barrier);
                        hero.status.barrier_until = hero
                            .status
                            .barrier_until
                            .max(tick + GHOSTWING_DURATION_TICKS);
                        if let Some(seat) =
                            hero.id.checked_sub(1).and_then(|s| usize::try_from(s).ok())
                            && seat < 10
                        {
                            map.heroes[seat].ghostwing_until = tick + GHOSTWING_DURATION_TICKS;
                        }
                    }
                }
            }
            events.push(Event::GhostwingCaptured { team: killer_team });
            self.map_5v5 = Some(map);
            return true;
        }

        if is_blackclaw_pit && killer_team < 2 {
            map.blackclaw.entity_id = None;
            map.blackclaw.captures[usize::from(killer_team)] += 1;
            let cap_count = map.blackclaw.captures[usize::from(killer_team)];
            map.blackclaw.pending_descent =
                Some((tick + BLACKCLAW_DESCENT_TICKS, killer_team, cap_count));
            let ts = &mut self.state.team_scores[usize::from(killer_team)];
            ts.blackclaw_captures += 1;
            ts.first_blackclaw_capture_seconds
                .get_or_insert(tick as f64 / TICK_HZ as f64);
            for i in 0..self.entities.len().min(10) {
                let hero = &mut self.entities[i];
                if hero.kind == Kind::Hero && hero.team == killer_team {
                    hero.gold += 160;
                    self.state.scores[(hero.id - 1) as usize]
                        .gold_income
                        .last_hit_bounties += 160;
                    events.push(Event::GoldAward {
                        id: hero.id,
                        amount: 160,
                    });
                }
            }
            events.push(Event::BlackclawCaptured { team: killer_team });
            self.map_5v5 = Some(map);
            return true;
        }

        self.map_5v5 = Some(map);
        false
    }

    fn distribute_5v5_camp_rewards(
        &mut self,
        dead: &NeutralDeath5v5,
        extra_gold: u32,
        events: &mut Vec<Event>,
    ) {
        let &NeutralDeath5v5 {
            dead_pos: pos,
            killer_id,
            killer_team,
            bounty,
            xp,
            ..
        } = dead;
        if killer_team >= 2 {
            return;
        }
        let mut allies = [0usize; 5];
        let mut count = 0usize;
        for j in 0..self.entities.len().min(10) {
            let a = &mut self.entities[j];
            if a.team != killer_team || a.kind != Kind::Hero || a.hp <= 0. {
                continue;
            }
            let near = a.id == killer_id || a.position.dist(pos) <= TEAM_SHARE_RADIUS;
            if !near {
                continue;
            }
            let share = if a.id == killer_id {
                bounty + extra_gold
            } else {
                (bounty * 3 / 10) + (extra_gold / 2)
            };
            if share > 0 {
                a.gold += share;
                let income = &mut self.state.scores[(a.id - 1) as usize].gold_income;
                if a.id == killer_id {
                    income.last_hit_bounties += u64::from(share);
                } else {
                    income.lane_proximity_awards += u64::from(share);
                }
                events.push(Event::GoldAward {
                    id: a.id,
                    amount: share,
                });
            }
            if count < allies.len() {
                allies[count] = j;
                count += 1;
            }
        }
        if count > 0 && xp > 0. {
            let share_xp = if count == 1 {
                xp
            } else {
                xp * 1.2 / count as f32
            };
            for &j in &allies[..count] {
                self.award_xp(j, share_xp, events);
            }
        }
    }

    pub(super) fn advance_5v5(&mut self, events: &mut Vec<Event>) {
        let Some(mut map) = self.map_5v5.take() else {
            return;
        };
        let tick = self.tick;

        // 1. Spawn / respawn the 12 ordinary jungle camps.
        let camp_level = camp_level_at(tick, 120, 7);
        for idx in 0..map.camps.len() {
            if map.camps[idx].next_spawn_tick == Some(tick) && map.camps[idx].alive_ids.is_empty() {
                map.camps[idx].next_spawn_tick = None;
                let specs = map.camps[idx].kind.unit_specs();
                for (u_i, &(template, _)) in specs.iter().enumerate() {
                    let base = map.palette.stats_for_raw(template.as_raw());
                    let (scaled, max_hp) = scaled_unit_stats(base, camp_level);
                    let pos = map.camps[idx].spawns[u_i.min(1)];
                    let id = self.spawn_with_stats(NEUTRAL_TEAM, Kind::Neutral, scaled, pos, false);
                    let ent = self.entities.last_mut().unwrap();
                    ent.level = camp_level;
                    ent.max_hp = max_hp;
                    ent.hp = max_hp;
                    map.camps[idx].alive_ids.push(id);
                    map.neutral_homes.insert(
                        id,
                        NeutralHome {
                            home: pos,
                            leash_radius: CAMP_LEASH_RADIUS,
                            camp_index: Some(idx),
                            is_boss: false,
                            resetting: false,
                        },
                    );
                    events.push(Event::Spawn { id });
                }
            }
        }

        // 2. Spawn / respawn Ghostwing (`BossCamp_BotLeft`, 9:00).
        if map.ghostwing.next_spawn_tick == Some(tick) && map.ghostwing.entity_id.is_none() {
            map.ghostwing.next_spawn_tick = None;
            let level = camp_level_at(tick, 360, 10);
            let (scaled, max_hp) =
                scaled_unit_stats(map.palette.stats_for_raw(TEMPLATE_GHOSTWING), level);
            let id = self.spawn_with_stats(
                NEUTRAL_TEAM,
                Kind::Neutral,
                scaled,
                GHOSTWING_PIT_POS,
                false,
            );
            let ent = self.entities.last_mut().unwrap();
            ent.level = level;
            ent.max_hp = max_hp;
            ent.hp = max_hp;
            map.ghostwing.entity_id = Some(id);
            map.neutral_homes.insert(
                id,
                NeutralHome {
                    home: GHOSTWING_PIT_POS,
                    leash_radius: BOSS_LEASH_RADIUS,
                    camp_index: None,
                    is_boss: true,
                    resetting: false,
                },
            );
            events.push(Event::Spawn { id });
        }

        // 3. Spawn / respawn uncaptured Blackclaw (`BossCamp_TopRight`, 15:00).
        if map.blackclaw.next_spawn_tick == Some(tick)
            && map.blackclaw.entity_id.is_none()
            && map.blackclaw.captured_id.is_none()
            && map.blackclaw.pending_descent.is_none()
        {
            map.blackclaw.next_spawn_tick = None;
            let level = camp_level_at(tick, 720, 7);
            let (scaled, max_hp) = scaled_unit_stats(
                map.palette.stats_for_raw(TEMPLATE_BLACKCLAW_UNCAPTURED),
                level,
            );
            let id = self.spawn_with_stats(
                NEUTRAL_TEAM,
                Kind::Neutral,
                scaled,
                BLACKCLAW_PIT_POS,
                false,
            );
            let ent = self.entities.last_mut().unwrap();
            ent.level = level;
            ent.max_hp = max_hp;
            ent.hp = max_hp;
            map.blackclaw.entity_id = Some(id);
            map.neutral_homes.insert(
                id,
                NeutralHome {
                    home: BLACKCLAW_PIT_POS,
                    leash_radius: BOSS_LEASH_RADIUS,
                    camp_index: None,
                    is_boss: true,
                    resetting: false,
                },
            );
            events.push(Event::Spawn { id });
        }

        // 4. Launch captured Blackclaw down mid lane after 15s descent delay.
        if let Some((due, team, captures)) = map.blackclaw.pending_descent
            && tick >= due
        {
            map.blackclaw.pending_descent = None;
            let level = camp_level_at(tick, 720, 7);
            let effective_level = level.saturating_add(((captures.saturating_sub(1)) * 2) as u8);
            let (mut scaled, mut max_hp) = scaled_unit_stats(
                map.palette.stats_for_raw(TEMPLATE_BLACKCLAW_CAPTURED),
                effective_level,
            );
            // Explicit consecutive-capture scaling (each capture adds +20% HP and +15% WP).
            if captures > 1 {
                let stack = (captures - 1) as f32;
                max_hp *= 1.0 + 0.20 * stack;
                scaled.hp = max_hp;
                scaled.weapon *= 1.0 + 0.15 * stack;
            }
            let pos = Position { x: 0.0, y: 0.0 };
            let goal = self.layout.objective(1 - team);
            let id = self.spawn_with_stats(team, Kind::Minion, scaled, pos, false);
            let ent = self.entities.last_mut().unwrap();
            ent.level = effective_level;
            ent.max_hp = max_hp;
            ent.hp = max_hp;
            ent.order = Command::MoveTo(goal);
            map.blackclaw.captured_id = Some(id);
            map.blackclaw.captured_team = Some(team);
            events.push(Event::Spawn { id });
            events.push(Event::BlackclawUnleashed { id, team });
        }

        // 5. Captured Blackclaw mid-lane targeting: prioritize enemy mid structures/minions/heroes.
        if let (Some(bc_id), Some(bc_team)) =
            (map.blackclaw.captured_id, map.blackclaw.captured_team)
            && let Some(bc_idx) = self.index(bc_id)
            && self.entities[bc_idx].hp > 0.
        {
            let bc_pos = self.entities[bc_idx].position;
            let bc_range = self.entities[bc_idx].stats.range;
            let goal = self.layout.objective(1 - bc_team);
            let mut best_target: Option<(u8, f32, u32)> = None;
            for other in self.entities.iter() {
                if other.team == bc_team || other.hp <= 0. || other.team >= 2 {
                    continue;
                }
                if self.solo_structure_protected(other) {
                    continue;
                }
                let d = bc_pos.dist(other.position);
                if d > bc_range + 4.0 {
                    continue;
                }
                let prio = match other.kind {
                    Kind::Tower | Kind::Armory | Kind::Crystal
                        if other.position.y.abs() <= 14.0 =>
                    {
                        0
                    }
                    Kind::Minion if other.position.y.abs() <= 12.0 => 1,
                    Kind::Hero if other.position.y.abs() <= 12.0 => 2,
                    _ => continue,
                };
                let candidate = (prio, d, other.id);
                if best_target.is_none_or(|b| candidate < b) {
                    best_target = Some(candidate);
                }
            }
            if let Some((_, _, target_id)) = best_target {
                self.entities[bc_idx].order = Command::Attack(target_id);
            } else {
                self.entities[bc_idx].order = Command::MoveTo(goal);
            }
        }

        // 6. Jungle & Boss Leash / Pursuit Reset.
        let ids: Vec<u32> = map.neutral_homes.keys().copied().collect();
        for nid in ids {
            let Some(ni) = self.index(nid) else {
                continue;
            };
            let (e_hp, e_pos, e_order, e_range, e_speed, e_max_hp) = {
                let e = &self.entities[ni];
                (
                    e.hp,
                    e.position,
                    e.order,
                    e.stats.range,
                    e.stats.speed,
                    e.max_hp,
                )
            };
            if e_hp <= 0. {
                continue;
            }
            let Some((home, leash_radius, was_resetting)) = map
                .neutral_homes
                .get(&nid)
                .map(|h| (h.home, h.leash_radius, h.resetting))
            else {
                continue;
            };
            let mut now_resetting = was_resetting;
            if !now_resetting {
                let out_of_leash = e_pos.dist(home) > leash_radius;
                let target_escaped = match e_order {
                    Command::Attack(tid) => self.index(tid).is_none_or(|tj| {
                        let tgt = &self.entities[tj];
                        tgt.hp <= 0. || tgt.position.dist(home) > leash_radius + e_range + 1.5
                    }),
                    _ => false,
                };
                if out_of_leash || target_escaped {
                    now_resetting = true;
                    let e = &mut self.entities[ni];
                    e.order = Command::Stop;
                    e.winding = None;
                }
            }
            if now_resetting {
                let speed = (e_speed.max(3.5) * 1.8) / TICK_HZ as f32;
                let next_pos = e_pos.advance(home, speed);
                let e = &mut self.entities[ni];
                e.position = next_pos;
                e.hp = (e_hp + e_max_hp * 0.01).min(e_max_hp);
                if next_pos != e_pos {
                    events.push(Event::Move {
                        id: e.id,
                        position: next_pos,
                    });
                }
                if next_pos.dist(home) <= 0.35 {
                    e.position = home;
                    e.hp = e_max_hp;
                    e.order = Command::Stop;
                    now_resetting = false;
                }
            }
            if let Some(home_info) = map.neutral_homes.get_mut(&nid) {
                home_info.resetting = now_resetting;
            }
        }

        // 7. Per-hero 5v5 stage buffs (`Recharge` / `Murder` / `Homeguard`), Treant/Ghostwing
        //    regens, River speed boost, and Recall (`Withdraw`) completion/CC checks.
        for idx in 0..self.entities.len().min(10) {
            let (id, team, _pos, hp, cc_blocked) = {
                let e = &self.entities[idx];
                if e.kind != Kind::Hero {
                    continue;
                }
                let cc = e.status.stun_until > tick
                    || e.status.root_until > tick
                    || e.status.silence_until > tick;
                (e.id, e.team, e.position, e.hp, cc)
            };
            let Some(seat) = id.checked_sub(1).and_then(|s| usize::try_from(s).ok()) else {
                continue;
            };
            if seat >= 10 {
                continue;
            }
            if hp <= 0. {
                map.heroes[seat].recall = None;
                map.heroes[seat].river_bonus_speed = 0.;
                continue;
            }
            // Recall CC interruption or completion.
            if let Some(recall) = map.heroes[seat].recall {
                if cc_blocked {
                    map.heroes[seat].recall = None;
                    events.push(Event::RecallInterrupted { id });
                } else if tick >= recall.complete_at {
                    map.heroes[seat].recall = None;
                    let home_pos = self.layout.home(&self.entities[idx]);
                    let e = &mut self.entities[idx];
                    e.position = home_pos;
                    e.order = Command::Stop;
                    e.winding = None;
                    e.path = Default::default();
                    if let Some(nav) = &mut self.native_navigation {
                        nav.forget_route(id);
                    }
                    map.heroes[seat].homeguard_until = tick + HOMEGUARD_DURATION_TICKS;
                    events.push(Event::RecallComplete {
                        id,
                        position: home_pos,
                    });
                    events.push(Event::Move {
                        id,
                        position: home_pos,
                    });
                    continue;
                }
            }

            let cur_pos = self.entities[idx].position;
            let in_own = self.in_spawn_stage(team, cur_pos);
            let in_enemy = self.in_spawn_stage(1 - team, cur_pos);
            if in_own {
                // Buff_SpawnStage_Recharge + Homeguard refresh.
                map.heroes[seat].homeguard_until = tick + HOMEGUARD_DURATION_TICKS;
                let e = &mut self.entities[idx];
                let heal_amt = e.max_hp * 0.003;
                if e.hp < e.max_hp {
                    e.hp = (e.hp + heal_amt).min(e.max_hp);
                }
                e.energy = (e.energy + 1.5).min(e.max_energy);
            }
            if in_enemy {
                // Buff_SpawnStage_Murder laser: bypasses shields/barriers to kill divers.
                let crystal_src = self
                    .entities
                    .iter()
                    .find(|e| e.kind == Kind::Crystal && e.team == 1 - team)
                    .map_or(0, |c| c.id);
                let e = &mut self.entities[idx];
                e.status.barrier = 0.;
                e.status.barrier_until = 0;
                self.map_5v5 = Some(map);
                self.apply_damage(
                    crystal_src,
                    1 - team,
                    idx,
                    STAGE_MURDER_TRUE_DAMAGE_PER_TICK,
                    (DamageSchool::True, ImpactEffect::Standard),
                    events,
                );
                map = self.map_5v5.take().unwrap();
                if self.entities[idx].hp <= 0. {
                    continue;
                }
            }

            // Crystal Treant mana regen.
            if map.heroes[seat].crystal_treant_until > tick {
                let e = &mut self.entities[idx];
                e.energy =
                    (e.energy + CRYSTAL_TREANT_ENERGY_PER_SEC / TICK_HZ as f32).min(e.max_energy);
            }

            // Ghostwing out-of-combat HP/energy regen + regenerating barrier.
            if map.heroes[seat].ghostwing_until > tick
                && tick
                    >= map.heroes[seat]
                        .last_combat_tick
                        .saturating_add(OOC_DELAY_TICKS)
            {
                let e = &mut self.entities[idx];
                let hp_tick = e.max_hp * 0.04 / TICK_HZ as f32;
                let before = e.hp;
                e.hp = (e.hp + hp_tick).min(e.max_hp);
                if tick.is_multiple_of(TICK_HZ) && e.hp > before {
                    events.push(Event::Heal {
                        source: id,
                        target: id,
                        amount: e.hp - before,
                        hp: e.hp,
                    });
                }
                e.energy = (e.energy + e.max_energy * 0.03 / TICK_HZ as f32).min(e.max_energy);
                let target_barrier = 300. + 40. * f32::from(e.level.saturating_sub(1));
                e.status.barrier = e.status.barrier.max(target_barrier);
                e.status.barrier_until = map.heroes[seat].ghostwing_until;
            }

            // River speed boost (`Buff_GameObjectives_River5v5Bonus`).
            let river_speed = if tick
                >= map.heroes[seat]
                    .last_combat_tick
                    .saturating_add(COMBAT_LOCKOUT_TICKS)
                && let Some(flow) = river_outward_flow(cur_pos)
            {
                match self.entities[idx].order {
                    Command::MoveTo(goal) => {
                        let d = cur_pos.dist(goal);
                        if d > 0.05 {
                            let dir_x = (goal.x - cur_pos.x) / d;
                            let dir_y = (goal.y - cur_pos.y) / d;
                            let dot = dir_x * flow.x + dir_y * flow.y;
                            if dot > 0.1 {
                                RIVER_FLOW_SPEED_BONUS
                            } else if dot >= -0.35 {
                                RIVER_CROSS_SPEED_BONUS
                            } else {
                                0.
                            }
                        } else {
                            RIVER_CROSS_SPEED_BONUS
                        }
                    }
                    _ => RIVER_CROSS_SPEED_BONUS,
                }
            } else {
                0.
            };
            map.heroes[seat].river_bonus_speed = river_speed;
        }

        // 8. Evaluate `0x042C` CampVisibility transitions across observer (`0`) and teams (`1`, `2`).
        // Restore `self.map_5v5` before visibility queries so `sees_position` honors 5v5 free-roam rules.
        let timer_positions: Vec<Position> = map.camps.iter().map(|c| c.timer_pos).collect();
        self.map_5v5 = Some(map);
        let seen_by_team: Vec<[bool; 2]> = timer_positions
            .iter()
            .map(|&timer_pos| {
                [
                    self.sees_position(0, timer_pos),
                    self.sees_position(1, timer_pos),
                ]
            })
            .collect();
        let map = self.map_5v5.as_mut().expect("restored 5v5 map state");
        for (idx, camp) in map.camps.iter_mut().enumerate() {
            let populated = camp.is_populated();
            let next_obs = if tick >= CAMP_SPAWN_TICK {
                if populated {
                    CampVisibilityState5v5::PopulatedVisible
                } else {
                    CampVisibilityState5v5::EmptyVisible
                }
            } else {
                CampVisibilityState5v5::EmptyHidden
            };
            if camp.last_visibility[0] != Some(next_obs) {
                camp.last_visibility[0] = Some(next_obs);
                map.pending_camp_transitions
                    .push(CampVisibilityTransition5v5 {
                        actor_id: camp.actor_id,
                        viewer_index: 0,
                        state: next_obs,
                    });
            }
            for (team, &seen) in seen_by_team[idx].iter().enumerate() {
                let viewer_idx = team + 1;
                let prev = camp.last_visibility[viewer_idx];
                let next_state = if seen {
                    if populated {
                        CampVisibilityState5v5::PopulatedVisible
                    } else {
                        CampVisibilityState5v5::EmptyVisible
                    }
                } else if prev.is_none() {
                    CampVisibilityState5v5::EmptyHidden
                } else if populated && matches!(prev, Some(CampVisibilityState5v5::EmptyHidden)) {
                    // Initial spawn / timed respawn reveals populated icon state on minimap.
                    CampVisibilityState5v5::PopulatedHidden
                } else if matches!(
                    prev,
                    Some(
                        CampVisibilityState5v5::PopulatedVisible
                            | CampVisibilityState5v5::PopulatedHidden
                    )
                ) {
                    CampVisibilityState5v5::PopulatedHidden
                } else {
                    CampVisibilityState5v5::EmptyHidden
                };
                if prev != Some(next_state) {
                    camp.last_visibility[viewer_idx] = Some(next_state);
                    map.pending_camp_transitions
                        .push(CampVisibilityTransition5v5 {
                            actor_id: camp.actor_id,
                            viewer_index: viewer_idx as u8,
                            state: next_state,
                        });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use halcyon_data::GameData;

    fn test_data() -> Option<GameData> {
        let delivery = std::env::var_os("HALCYON_DELIVERY")?;
        let extra = std::env::var_os("HALCYON_DATA_EXTRA");
        GameData::load_with_extra(
            std::path::Path::new(&delivery),
            extra.as_deref().map(std::path::Path::new),
        )
        .ok()
    }

    fn setup_5v5_world() -> (World, Option<GameData>) {
        let data = test_data();
        let mut world = World::new(42, 10);
        world.assign_teams(&[0, 0, 0, 0, 0, 1, 1, 1, 1, 1]).unwrap();
        if let Some(d) = data.as_ref() {
            for seat in 0..10 {
                let _ = world.select_hero(seat, "Ringo", d);
            }
        }
        world.use_sovereigns_rise_5v5(data.as_ref()).unwrap();
        (world, data)
    }

    #[test]
    fn all_12_jungle_camps_spawn_clear_respawn_and_emit_camp_visibility_transitions() {
        let (mut world, _) = setup_5v5_world();
        let decls = world.camp_5v5_declarations();
        assert_eq!(decls.len(), 14);
        assert_eq!(
            decls
                .iter()
                .filter(|(id, _, _)| (6580..=6591).contains(id))
                .count(),
            12
        );
        assert!(decls.iter().any(|(id, tpl, _)| *id == 6566 && *tpl == 351));
        assert!(decls.iter().any(|(id, tpl, _)| *id == 6567 && *tpl == 352));

        // Initial tick initializes all 12 camps across observer (0) + teams (1, 2):
        // out-of-sight camps start EmptyHidden, while turret-watched back camps start EmptyVisible.
        world.step();
        let init_transitions = world.take_5v5_camp_transitions();
        assert_eq!(init_transitions.len(), 36);
        let markers_init = world.camp_5v5_markers();
        for t in &init_transitions {
            let pos = markers_init
                .iter()
                .find(|(id, _, _, _)| *id == t.actor_id)
                .unwrap()
                .2;
            let expected = if t.viewer_index == 0 {
                CampVisibilityState5v5::EmptyHidden
            } else if world.sees_position(t.viewer_index - 1, pos) {
                CampVisibilityState5v5::EmptyVisible
            } else {
                CampVisibilityState5v5::EmptyHidden
            };
            assert_eq!(t.state, expected, "unexpected init transition {t:?}");
        }

        // Advance up to camp spawn (35s = 700 ticks).
        while world.tick < CAMP_SPAWN_TICK {
            world.step();
        }
        let mut transitions = world.take_5v5_camp_transitions();
        assert_eq!(world.camp_5v5_markers().len(), 12);
        assert!(
            world
                .entities
                .iter()
                .filter(|e| e.kind == Kind::Neutral && e.hp > 0.)
                .count()
                >= 12
        );
        assert!(transitions.iter().any(|t| {
            t.viewer_index == 0 && t.state == CampVisibilityState5v5::PopulatedVisible
        }));
        assert!(transitions.iter().any(|t| {
            t.viewer_index == 1 && t.state == CampVisibilityState5v5::PopulatedHidden
        }));

        // Find Weapon Treant camp (team 0 side) and move hero 1 to see + kill it.
        let weapon_unit = world
            .entities
            .iter()
            .find(|e| {
                e.kind == Kind::Neutral
                    && e.hp > 0.
                    && e.type_id == EntityTemplate::WeaponTreant.as_raw()
                    && e.position.x < 0.
            })
            .unwrap()
            .id;
        let wi = world.index(weapon_unit).unwrap();
        let camp_pos = world.entities[wi].position;
        world.entities[0].position = camp_pos;
        world.entities[0].max_hp = 5000.;
        world.entities[0].hp = 5000.;
        world.entities[0].stats.weapon = 5000.;
        world.step();
        transitions.extend(world.take_5v5_camp_transitions());

        let left_weapon_actor = world
            .camp_5v5_markers()
            .into_iter()
            .find(|(_, k, p, _)| *k == CampKind5v5::WeaponTreant && p.dist(camp_pos) < 5.0)
            .unwrap()
            .0;
        assert!(
            transitions.iter().any(|t| {
                t.actor_id == left_weapon_actor
                    && t.viewer_index == 1
                    && t.state == CampVisibilityState5v5::PopulatedVisible
            }),
            "expected PopulatedVisible for {left_weapon_actor} (sees={}) in {:?}",
            world.sees_position(0, camp_pos),
            transitions
                .iter()
                .filter(|t| t.actor_id == left_weapon_actor)
                .collect::<Vec<_>>()
        );

        // Clear the Weapon Treant -> observer (0) and clearing team (1) see EmptyVisible,
        // while unvisited team 1 (viewer_index = 2) keeps PopulatedHidden.
        world.command(1, Command::Attack(weapon_unit)).unwrap();
        let mut clear_transitions = Vec::new();
        for _ in 0..80 {
            world.step();
            clear_transitions.extend(world.take_5v5_camp_transitions());
            if world
                .index(weapon_unit)
                .is_none_or(|idx| world.entities[idx].hp <= 0.)
            {
                break;
            }
        }
        assert!(
            world
                .index(weapon_unit)
                .is_none_or(|idx| world.entities[idx].hp <= 0.)
        );
        world.step();
        clear_transitions.extend(world.take_5v5_camp_transitions());
        assert!(clear_transitions.iter().any(|t| {
            t.actor_id == left_weapon_actor
                && t.viewer_index == 0
                && t.state == CampVisibilityState5v5::EmptyVisible
        }));
        assert!(clear_transitions.iter().any(|t| {
            t.actor_id == left_weapon_actor
                && t.viewer_index == 1
                && t.state == CampVisibilityState5v5::EmptyVisible
        }));
        let marker_state = world
            .camp_5v5_markers()
            .into_iter()
            .find(|(id, _, _, _)| *id == left_weapon_actor)
            .unwrap()
            .3;
        assert_eq!(
            marker_state[2],
            Some(CampVisibilityState5v5::PopulatedHidden)
        );

        // Enemy hero walks into sight -> viewer_index 2 learns EmptyVisible.
        world.entities[0].position = Position { x: -50., y: 0. };
        world.entities[5].position = camp_pos;
        world.step();
        let scout_transitions = world.take_5v5_camp_transitions();
        assert!(scout_transitions.iter().any(|t| {
            t.actor_id == left_weapon_actor
                && t.viewer_index == 2
                && t.state == CampVisibilityState5v5::EmptyVisible
        }));
        world.entities[5].position = Position { x: 50., y: 0. };

        // Step past buff camp respawn (125s = 2500 ticks) and confirm Weapon Treant respawns.
        for _ in 0..2550 {
            world.step();
        }
        let respawn_transitions = world.take_5v5_camp_transitions();
        assert!(respawn_transitions.iter().any(|t| {
            t.actor_id == left_weapon_actor
                && t.viewer_index == 0
                && t.state == CampVisibilityState5v5::PopulatedVisible
        }));
        assert!(world.entities.iter().any(|e| {
            e.kind == Kind::Neutral
                && e.hp > 0.
                && e.type_id == EntityTemplate::WeaponTreant.as_raw()
                && e.position.dist(camp_pos) < 3.0
        }));

        // Two-monster camp emits EmptyVisible ONLY after BOTH units die (never on partial kill).
        let duo_actor = world
            .camp_5v5_markers()
            .into_iter()
            .find(|(_, k, p, _)| *k == CampKind5v5::HealingAndSmallBear && p.x < 0.)
            .unwrap();
        let duo_units: Vec<u32> = world
            .entities
            .iter()
            .filter(|e| e.kind == Kind::Neutral && e.hp > 0. && e.position.dist(duo_actor.2) < 6.0)
            .map(|e| e.id)
            .collect();
        assert_eq!(duo_units.len(), 2);
        world.entities[0].position = duo_actor.2;
        world.entities[0].stats.weapon = 2000.;
        world.command(1, Command::Attack(duo_units[0])).unwrap();
        for _ in 0..40 {
            world.step();
        }
        let partial = world.take_5v5_camp_transitions();
        assert!(
            !partial.iter().any(|t| {
                t.actor_id == duo_actor.0 && t.state == CampVisibilityState5v5::EmptyVisible
            }),
            "partial kill in 2-unit camp must not emit EmptyVisible"
        );
        world.command(1, Command::Attack(duo_units[1])).unwrap();
        for _ in 0..40 {
            world.step();
        }
        let complete = world.take_5v5_camp_transitions();
        assert!(complete.iter().any(|t| {
            t.actor_id == duo_actor.0
                && t.viewer_index == 1
                && t.state == CampVisibilityState5v5::EmptyVisible
        }));
    }

    #[test]
    fn jungle_camp_leash_reset_and_proximity_team_sharing() {
        let (mut world, _) = setup_5v5_world();
        while world.tick < CAMP_SPAWN_TICK {
            world.step();
        }
        let target = world
            .entities
            .iter()
            .find(|e| {
                e.kind == Kind::Neutral
                    && e.hp > 0.
                    && e.type_id == EntityTemplate::BigBear.as_raw()
                    && e.position.x < 0.
            })
            .unwrap();
        let bear_id = target.id;
        let home = target.position;

        // Poke bear from near leash edge so it pursues, then kite beyond leash_radius -> resets & heals.
        world.entities[0].position = Position {
            x: home.x + 4.5,
            y: home.y,
        };
        world.entities[0].stats.weapon = 80.;
        world.command(1, Command::Attack(bear_id)).unwrap();
        for _ in 0..30 {
            world.step();
        }
        world.command(1, Command::Stop).unwrap();
        world.entities[0].position = Position {
            x: home.x + 20.0,
            y: home.y + 20.0,
        };
        for _ in 0..90 {
            world.step();
        }
        let bi = world.index(bear_id).unwrap();
        let bear = &world.entities[bi];
        assert!(bear.position.dist(home) < 1.0, "bear must leash back home");
        let resetting = world.is_5v5_resetting(bear_id);
        assert!(
            (bear.hp - bear.max_hp).abs() < 1.0,
            "resetting bear heals to full (hp={}, max={}, resetting={resetting}, dist={})",
            bear.hp,
            bear.max_hp,
            bear.position.dist(home)
        );

        // Nearby teammate shares camp bounty and XP cleanly.
        let other_bear = world
            .entities
            .iter()
            .find(|e| e.kind == Kind::Neutral && e.hp > 0. && e.id != bear_id && e.position.x < 0.)
            .unwrap();
        let other_id = other_bear.id;
        let other_pos = other_bear.position;
        world.entities[0].position = other_pos;
        world.entities[1].position = Position {
            x: other_pos.x + 2.0,
            y: other_pos.y,
        };
        world.entities[0].gold = 0;
        world.entities[1].gold = 0;
        world.entities[0].xp = 0.;
        world.entities[1].xp = 0.;
        world.entities[0].stats.weapon = 3000.;
        world.command(1, Command::Attack(other_id)).unwrap();
        for _ in 0..40 {
            world.step();
        }
        assert!(world.entities[0].gold > 0);
        assert!(
            world.entities[1].gold > 0,
            "nearby ally must receive shared camp gold"
        );
        assert!(
            world.entities[1].xp > 0.,
            "nearby ally must receive shared camp XP"
        );
    }

    #[test]
    fn weapon_crystal_healing_and_gold_treant_buffs_apply_verified_effects() {
        let (mut world, _) = setup_5v5_world();
        while world.tick < CAMP_SPAWN_TICK {
            world.step();
        }
        world.entities[0].stats.weapon = 5000.;
        world.entities[0].max_hp = 2000.;

        // 1. Weapon Treant -> weapon_amp = +15% & slow on basic attack.
        let wp = world
            .entities
            .iter()
            .find(|e| e.kind == Kind::Neutral && e.type_id == EntityTemplate::WeaponTreant.as_raw())
            .unwrap()
            .id;
        let wpi = world.index(wp).unwrap();
        world.entities[0].position = world.entities[wpi].position;
        world.command(1, Command::Attack(wp)).unwrap();
        for _ in 0..35 {
            world.step();
        }
        let st = world.hero_5v5_status(1).unwrap();
        assert!(st.weapon_treant_active);
        world.entities[5].position = Position { x: 0.0, y: 0.0 };
        world.entities[0].position = Position { x: -1.5, y: 0.0 };
        world.entities[0].stats.weapon = 100.;
        world.command(1, Command::Attack(6)).unwrap();
        for _ in 0..25 {
            world.step();
        }
        assert!(world.entities[5].status.slow_until > world.tick);

        // 2. Crystal Treant -> crystal amp + energy regen.
        let cp = world
            .entities
            .iter()
            .find(|e| {
                e.kind == Kind::Neutral && e.type_id == EntityTemplate::CrystalTreant.as_raw()
            })
            .unwrap()
            .id;
        let cpi = world.index(cp).unwrap();
        world.entities[0].stats.weapon = 5000.;
        world.entities[0].energy = 10.;
        world.entities[0].position = world.entities[cpi].position;
        world.command(1, Command::Attack(cp)).unwrap();
        for _ in 0..35 {
            world.step();
        }
        assert!(world.hero_5v5_status(1).unwrap().crystal_treant_active);
        let e_before = world.entities[0].energy;
        for _ in 0..20 {
            world.step();
        }
        assert!(world.entities[0].energy > e_before + 5.);

        // 3. Healing Treant -> instant heal burst + barrier.
        let heal_id = world
            .entities
            .iter()
            .find(|e| {
                e.kind == Kind::Neutral && e.type_id == EntityTemplate::HealingTreant.as_raw()
            })
            .unwrap()
            .id;
        let hi = world.index(heal_id).unwrap();
        world.entities[0].position = world.entities[hi].position;
        world.entities[0].hp = 500.;
        world.entities[0].status.barrier = 0.;
        world.command(1, Command::Attack(heal_id)).unwrap();
        for _ in 0..35 {
            world.step();
        }
        assert!(world.entities[0].hp > 700.);
        assert!(world.entities[0].status.barrier > 50.);

        // 4. Gold Treant -> bonus gold burst (+40 bonus beyond unit bounty).
        let gold_id = world
            .entities
            .iter()
            .find(|e| e.kind == Kind::Neutral && e.type_id == EntityTemplate::GoldTreant.as_raw())
            .unwrap()
            .id;
        let gi = world.index(gold_id).unwrap();
        world.entities[0].position = world.entities[gi].position;
        let bounty = world.entities[gi]
            .stats
            .physiology
            .map_or(0, |p| p.gold_bounty as u32);
        let gold_before = world.entities[0].gold;
        world.command(1, Command::Attack(gold_id)).unwrap();
        for _ in 0..35 {
            world.step();
        }
        assert!(world.entities[0].gold >= gold_before + bounty + 40);
    }

    #[test]
    #[ignore = "slow soak; run with cargo nextest run --run-ignored all"]
    fn ghostwing_and_blackclaw_spawn_capture_push_scale_and_score() {
        let (mut world, _) = setup_5v5_world();
        // Spawn Ghostwing at 9:00 (10800 ticks).
        while world.tick < GHOSTWING_SPAWN_TICK {
            world.step();
        }
        let gw_id = world
            .ghostwing_status()
            .unwrap()
            .0
            .expect("Ghostwing spawned at 9:00");
        let gwi = world.index(gw_id).unwrap();
        world.entities[0].position = world.entities[gwi].position;
        world.entities[0].max_hp = 5000.;
        world.entities[0].hp = 3000.;
        world.entities[0].stats.weapon = 8000.;
        world.command(1, Command::Attack(gw_id)).unwrap();
        let mut gw_events = Vec::new();
        for _ in 0..45 {
            gw_events.extend(world.step());
        }
        assert!(
            gw_events
                .iter()
                .any(|e| matches!(e, Event::GhostwingCaptured { team: 0, .. }))
        );
        let (_, gw_caps) = world.ghostwing_status().unwrap();
        assert_eq!(gw_caps, [1, 0]);
        assert_eq!(world.team_scores()[0].ghostwing_captures, 1);
        assert!(
            world.team_scores()[0]
                .first_ghostwing_capture_seconds
                .is_some()
        );
        assert!(
            world.entities[1].status.barrier >= 250.,
            "team receives Ghostwing barrier"
        );
        // Advance out-of-combat window (6s = 120 ticks) and confirm Ghostwing OOC regen ticks.
        let hp_low = world.entities[1].max_hp * 0.5;
        world.entities[1].hp = hp_low;
        for _ in 0..150 {
            world.step();
        }
        assert!(
            world.entities[1].hp > hp_low,
            "Ghostwing OOC regen must restore ally HP"
        );

        // Advance to Blackclaw 15:00 (18000 ticks) + capture -> 15s descent -> unleashed push.
        while world.tick < BLACKCLAW_SPAWN_TICK {
            world.step();
        }
        let (bc_wild, _, _) = world.blackclaw_status().unwrap();
        let bc_id = bc_wild.expect("Blackclaw spawned at 15:00");
        let bci = world.index(bc_id).unwrap();
        let bc_hp = world.entities[bci].hp;
        world.entities[0].hp = 5000.;
        world.entities[0].max_hp = 5000.;
        world.entities[0].stats.weapon = 50_000.;
        world.entities[0].position = world.entities[bci].position;
        world.command(1, Command::Attack(bc_id)).unwrap();
        let mut bc_events = Vec::new();
        for _ in 0..50 {
            bc_events.extend(world.step());
        }
        assert!(
            bc_events
                .iter()
                .any(|e| matches!(e, Event::BlackclawCaptured { team: 0, .. })),
            "expected BlackclawCaptured (initial bc_hp={bc_hp}, post={:?}, hero_hp={})",
            world.index(bc_id).map(|idx| world.entities[idx].hp),
            world.entities[0].hp
        );
        assert_eq!(world.team_scores()[0].blackclaw_captures, 1);
        assert!(
            world.team_scores()[0]
                .first_blackclaw_capture_seconds
                .is_some()
        );

        // Advance through 15s descent (300 ticks) -> BlackclawUnleashed marching down mid lane.
        let mut unleashed = Vec::new();
        for _ in 0..320 {
            unleashed.extend(world.step());
        }
        assert!(
            unleashed
                .iter()
                .any(|e| matches!(e, Event::BlackclawUnleashed { team: 0, .. }))
        );
        let (_, cap_id, cap_counts) = world.blackclaw_status().unwrap();
        let cap_eid = cap_id.unwrap();
        let ci = world.index(cap_eid).unwrap();
        assert_eq!(cap_counts, [1, 0]);
        assert_eq!(world.entities[ci].team, 0);
        let first_bc_hp = world.entities[ci].max_hp;
        // Enemy hero (id 6) kills captured Blackclaw through combat -> wait 240s respawn -> 2nd capture scales HP.
        let bc_pos = world.entities[ci].position;
        world.entities[5].position = Position {
            x: bc_pos.x + 2.0,
            y: bc_pos.y,
        };
        world.entities[5].hp = 5000.;
        world.entities[5].max_hp = 5000.;
        world.entities[5].stats.weapon = 50_000.;
        world.command(6, Command::Attack(cap_eid)).unwrap();
        for _ in 0..50 {
            world.step();
            if world.blackclaw_status().unwrap().1.is_none() {
                break;
            }
        }
        assert!(
            world.blackclaw_status().unwrap().1.is_none(),
            "captured Blackclaw defeated"
        );
        world.command(6, Command::Stop).unwrap();
        world.entities[5].position = Position { x: 105., y: 0. };
        for _ in 0..BOSS_RESPAWN_TICKS + 40 {
            world.step();
        }
        let bc2_id = world
            .blackclaw_status()
            .unwrap()
            .0
            .expect("second Blackclaw respawn");
        let b2i = world.index(bc2_id).unwrap();
        world.entities[0].position = world.entities[b2i].position;
        world.command(1, Command::Attack(bc2_id)).unwrap();
        for _ in 0..400 {
            world.step();
        }
        let (_, cap2_id, cap2_counts) = world.blackclaw_status().unwrap();
        assert_eq!(cap2_counts, [2, 0]);
        let c2i = world.index(cap2_id.unwrap()).unwrap();
        let second_bc_hp = world.entities[c2i].max_hp;
        assert!(
            second_bc_hp > first_bc_hp,
            "second Blackclaw capture must scale HP"
        );
    }

    #[test]
    fn recall_channel_interruptions_spawn_stage_recharge_murder_river_and_brushes() {
        let (mut world, _) = setup_5v5_world();
        // Move hero 1 out to mid lane -> Recall finishes in 160 ticks & grants Homeguard.
        let away = Position { x: -5.0, y: 0.0 };
        world.entities[0].position = away;
        world.command(1, Command::Recall).unwrap();
        let mut recall_ev = Vec::new();
        for _ in 0..RECALL_CHANNEL_TICKS + 5 {
            recall_ev.extend(world.step());
        }
        assert!(
            recall_ev
                .iter()
                .any(|e| matches!(e, Event::RecallComplete { id: 1, .. }))
        );
        let home = world.layout.home(&world.entities[0]);
        assert!(world.entities[0].position.dist(home) < 0.5);
        assert!(
            world.move_speed_5v5_bonus(1) > 0.5,
            "Homeguard must boost speed"
        );

        // Recall interruption by MoveTo and by stun CC.
        world.entities[0].position = away;
        world.command(1, Command::Recall).unwrap();
        world.step();
        world
            .command(1, Command::MoveTo(Position { x: 0., y: 0. }))
            .unwrap();
        let ev_move = world.step();
        assert!(
            ev_move
                .iter()
                .any(|e| matches!(e, Event::RecallInterrupted { id: 1 }))
        );

        world.entities[0].position = away;
        world.command(1, Command::Recall).unwrap();
        world.step();
        world.entities[0].status.stun_until = world.tick + 20;
        let ev_cc = world.step();
        assert!(
            ev_cc
                .iter()
                .any(|e| matches!(e, Event::RecallInterrupted { id: 1 }))
        );
        world.entities[0].status.stun_until = 0;

        // Spawn stage recharge heals damaged ally, Murder laser kills enemy diving enemy spawn_stage.
        world.entities[0].position = home;
        world.entities[0].hp = 100.;
        for _ in 0..10 {
            world.step();
        }
        assert!(
            world.entities[0].hp > 110.,
            "Buff_SpawnStage_Recharge must heal"
        );

        let enemy_home = world.layout.home(&world.entities[5]);
        world.entities[0].position = enemy_home;
        world.entities[0].status.barrier = 10_000.;
        world.entities[0].status.barrier_until = world.tick + 200;
        for _ in 0..5 {
            world.step();
        }
        assert_eq!(
            world.entities[0].hp, 0.,
            "Buff_SpawnStage_Murder laser kills divers bypassing barrier"
        );

        // Brush stealth vs ally entering same brush & turret true-sight radius.
        if let Some(brush_pos) = world
            .native_navigation
            .as_ref()
            .and_then(|nav| nav.first_brush_center())
        {
            world.entities[5].position = brush_pos;
            world.entities[1].position = Position {
                x: brush_pos.x + 8.0,
                y: brush_pos.y,
            };
            assert!(
                !world.visible(0, &world.entities[5]),
                "hero inside 5v5 brush hidden from team outside"
            );
            world.entities[1].position = brush_pos;
            assert!(
                world.visible(0, &world.entities[5]),
                "ally entering the same brush reveals hidden enemy"
            );
        }

        // River speed boost activates out of combat along river axis and zeroes in combat.
        world.entities[2].position = Position { x: 0.0, y: 10.0 };
        world
            .command(3, Command::MoveTo(Position { x: 0.0, y: 30.0 }))
            .unwrap();
        for _ in 0..2 {
            world.step();
        }
        assert!(world.move_speed_5v5_bonus(3) >= RIVER_CROSS_SPEED_BONUS);

        // River shop unlocks at 4:00 (4800 ticks).
        world.command(3, Command::Stop).unwrap();
        world.entities[2].position = RIVER_SHOP_LEFT;
        assert!(!world.at_item_shop(&world.entities[2]));
        while world.tick < RIVER_SHOP_SPAWN_TICK {
            world.step();
        }
        world.entities[2].position = RIVER_SHOP_LEFT;
        assert!(world.at_item_shop(&world.entities[2]));
    }

    #[test]
    fn sovereigns_rise_5v5_state_digest_and_camp_transitions_are_deterministic() {
        fn run_trace() -> (String, Vec<CampVisibilityTransition5v5>) {
            let (mut world, _) = setup_5v5_world();
            let mut log = Vec::new();
            for t in 0..800 {
                if t == CAMP_SPAWN_TICK + 20 {
                    world.entities[0].position = Position { x: -28., y: 20. };
                    world.command(1, Command::Recall).unwrap();
                }
                world.step();
                log.extend(world.take_5v5_camp_transitions());
            }
            (world.state_digest(), log)
        }
        let a = run_trace();
        let b = run_trace();
        assert_eq!(a, b);
    }
}
