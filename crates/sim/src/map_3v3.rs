use super::*;
use crate::layout::point;

pub const FIRST_STATIC_ACTOR: u32 = 501;
pub const FIRST_DYNAMIC_ACTOR: u32 = FIRST_STATIC_ACTOR + 84;

pub const CAMP_FIRST_SPAWN_TICK: u64 = 25 * TICK_HZ;
pub const CAMP_RESPAWN_TICKS: u64 = 50 * TICK_HZ;
pub const CAMP_LEASH_RADIUS: f32 = 8.5;

pub const JUNGLE_SHOP_POS: Position = Position { x: 0.2, y: 42.0 };
pub const JUNGLE_SHOP_RADIUS: f32 = 5.0;
pub const JUNGLE_SHOP_UNLOCK_TICK: u64 = 240 * TICK_HZ;

pub const GOLD_MINE_POS: Position = Position { x: 0.2, y: 39.5 };
pub const GOLD_MINE_FIRST_SPAWN_TICK: u64 = 240 * TICK_HZ;
pub const GOLD_MINE_RESPAWN_TICKS: u64 = 60 * TICK_HZ;
pub const GOLD_MINE_START: f32 = 50.0;
pub const GOLD_MINE_PER_SEC: f32 = 2.5;
pub const GOLD_MINE_CAP: f32 = 400.0;

pub const MINION_MINE_POSITIONS: [Position; 2] = [
    Position {
        x: -35.19,
        y: 36.034,
    },
    Position { x: 35.19, y: 35.73 },
];
pub const MINION_MINE_FIRST_SPAWN_TICK: u64 = 240 * TICK_HZ;
pub const MINION_MINE_RESPAWN_TICKS: u64 = 60 * TICK_HZ;

pub const KRAKEN_POS: Position = Position { x: 0.0, y: 23.6 };
pub const KRAKEN_SPAWN_TICK: u64 = 900 * TICK_HZ;
pub const KRAKEN_RESPAWN_TICKS: u64 = 180 * TICK_HZ;

pub const MINION_WAVE_START: u64 = 20 * TICK_HZ;
pub const MINION_WAVE_INTERVAL: u64 = 25 * TICK_HZ;

const HF_TOWERS: [(u8, usize, f32, f32); 5] = [
    (3, 11, 75.48, 11.96),
    (4, 11, 68.59, 19.97),
    (5, 10, 54.0, 2.92),
    (6, 9, 35.78, 1.17),
    (7, 8, 17.06, 1.93),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum CampUnitKind {
    Treant,
    ElderTreant,
    BigBear,
    SmallBear,
}

#[derive(Clone, Copy, Debug)]
struct CampSpawnSpec {
    unit: CampUnitKind,
    anchor: Position,
}

#[derive(Clone, Debug, Serialize)]
pub struct CampMember {
    pub entity_id: u32,
    pub unit: CampUnitKind,
    pub anchor: Position,
    pub last_combat_tick: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct CampSlot {
    pub members: Vec<CampMember>,
    pub respawn_tick: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum KrakenPhase {
    Dormant,
    Jungle { entity_id: u32 },
    Captured { entity_id: u32, team: u8 },
    Respawning { until_tick: u64 },
}

#[derive(Clone, Debug, Serialize)]
pub struct GoldMineState {
    pub entity_id: Option<u32>,
    pub owner: Option<u8>,
    pub stored_gold: f32,
    pub respawn_tick: Option<u64>,
    pub retired: bool,
    pub last_combat_tick: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct MinionMineSlot {
    pub anchor: Position,
    pub entity_id: Option<u32>,
    pub owner: Option<u8>,
    pub respawn_tick: Option<u64>,
    pub last_combat_tick: u64,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct FoldUnits {
    /// `[RangedMinion, LeadMinion, TankMinion, Treant, ElderTreant, BigBear, SmallBear, GoldMiner, HfOuterTurret, HfMiddleTurret, HfBaseTurret, HfVainTurret, VainCrystalHome3v3, VainCrystalAway3v3, CrystalMiner, KrakenJungle, KrakenCaptured]`
    pub(crate) rows: [Stats; 17],
}

fn physiology(
    attack_speed: f32,
    sight: f32,
    gold_bounty: f32,
    xp_bounty: f32,
) -> Option<Physiology> {
    Some(Physiology {
        hp_regen: 0.0,
        hp_regen_growth: 0.0,
        energy_regen: 0.0,
        energy_regen_growth: 0.0,
        attack_speed,
        attack_speed_growth: 0.0,
        collision_radius: 0.6,
        sight,
        gold_bounty,
        xp_bounty,
    })
}

fn with_growth(mut s: Stats, hp: f32, weapon: f32, phys: Option<Physiology>) -> Stats {
    s.growth.hp = hp;
    s.growth.weapon = weapon;
    if s.physiology.is_none() {
        s.physiology = phys;
    }
    s
}

impl Default for FoldUnits {
    fn default() -> Self {
        Self {
            rows: [
                with_growth(
                    Stats::new(EntityTemplate::RangedMinion, 350.0, 10.0, 0.0, 4.5, 3.2),
                    56.0,
                    4.0,
                    physiology(1.0, 10.0, 25.0, 6.0),
                ),
                with_growth(
                    Stats::new(EntityTemplate::LeadMinion, 522.0, 32.0, 0.0, 2.9, 3.2),
                    272.0,
                    15.0,
                    physiology(1.0, 10.0, 60.0, 12.0),
                ),
                with_growth(
                    Stats::new(EntityTemplate::TankMinion, 400.0, 13.0, 0.0, 1.2, 3.2),
                    96.0,
                    3.0,
                    physiology(1.0, 10.0, 35.0, 6.0),
                ),
                with_growth(
                    Stats::new(EntityTemplate::HfTreant, 750.0, 40.0, 0.0, 2.4, 3.6),
                    180.0,
                    10.0,
                    physiology(1.0, 10.0, 80.0, 28.0),
                ),
                with_growth(
                    Stats::new(EntityTemplate::HfElderTreant, 1500.0, 80.0, 0.0, 2.7, 3.6),
                    360.0,
                    20.0,
                    physiology(1.0, 10.0, 140.0, 28.0),
                ),
                with_growth(
                    Stats::new(EntityTemplate::HfBigBear, 600.0, 35.0, 0.0, 2.2, 3.6),
                    130.0,
                    7.0,
                    physiology(1.0, 10.0, 70.0, 16.0),
                ),
                with_growth(
                    Stats::new(EntityTemplate::HfSmallBear, 480.0, 30.0, 0.0, 2.2, 3.6),
                    100.0,
                    5.0,
                    physiology(1.0, 10.0, 45.0, 10.0),
                ),
                with_growth(
                    Stats::new(EntityTemplate::HfGoldMiner, 4500.0, 100.0, 100.0, 5.0, 3.6),
                    450.0,
                    25.0,
                    physiology(1.0, 10.0, 0.0, 30.0),
                ),
                Stats::new(
                    EntityTemplate::HfOuterTurret,
                    2500.0,
                    200.0,
                    100.0,
                    9.0,
                    0.0,
                ),
                Stats::new(
                    EntityTemplate::HfMiddleTurret,
                    3000.0,
                    250.0,
                    100.0,
                    9.0,
                    0.0,
                ),
                Stats::new(EntityTemplate::HfBaseTurret, 3500.0, 300.0, 100.0, 9.0, 0.0),
                Stats::new(EntityTemplate::HfVainTurret, 3000.0, 300.0, 100.0, 9.0, 0.0),
                Stats::new(
                    EntityTemplate::VainCrystalHome3v3,
                    10000.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                ),
                Stats::new(
                    EntityTemplate::VainCrystalAway3v3,
                    10000.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                ),
                with_growth(
                    Stats::new(
                        EntityTemplate::HfCrystalMiner,
                        3000.0,
                        80.0,
                        100.0,
                        5.0,
                        3.6,
                    ),
                    300.0,
                    20.0,
                    physiology(1.0, 10.0, 100.0, 20.0),
                ),
                with_growth(
                    Stats::new(
                        EntityTemplate::HfKrakenJungle,
                        9500.0,
                        250.0,
                        100.0,
                        2.5,
                        2.5,
                    ),
                    1000.0,
                    25.0,
                    physiology(1.0, 12.0, 0.0, 100.0),
                ),
                with_growth(
                    Stats::new(
                        EntityTemplate::HfKrakenCaptured,
                        9500.0,
                        400.0,
                        100.0,
                        2.5,
                        2.5,
                    ),
                    1500.0,
                    75.0,
                    physiology(1.0, 12.0, 0.0, 100.0),
                ),
            ],
        }
    }
}

impl FoldUnits {
    pub fn load(&mut self, data: &GameData) {
        const NAMES: [&str; 17] = [
            "RangedMinion",
            "LeadMinion",
            "TankMinion",
            "HF_Treant",
            "HF_ElderTreant",
            "HF_BigBear",
            "HF_SmallBear",
            "HF_GoldMiner",
            "HF_OuterTurret",
            "HF_MiddleTurret",
            "HF_BaseTurret",
            "HF_VainTurret",
            "VainCrystal_Home_3v3",
            "VainCrystal_Away_3v3",
            "HF_CrystalMiner",
            "HF_Kraken_Jungle",
            "HF_Kraken_Captured",
        ];
        for (stats, name) in self.rows.iter_mut().zip(NAMES) {
            if let Some(unit) = data.unit(name) {
                *stats = Stats::load(stats.id, unit, data);
            }
        }
    }

    fn camp_stats(&self, kind: CampUnitKind, tick: u64) -> (Stats, u8) {
        let base = match kind {
            CampUnitKind::Treant => self.rows[3],
            CampUnitKind::ElderTreant => self.rows[4],
            CampUnitKind::BigBear => self.rows[5],
            CampUnitKind::SmallBear => self.rows[6],
        };
        scale_jungle_stats(base, tick, 7)
    }
}

fn scale_jungle_stats(mut stats: Stats, tick: u64, max_level: u8) -> (Stats, u8) {
    let level = (1 + tick / (120 * TICK_HZ)).min(u64::from(max_level)) as u8;
    let extra = f32::from(level - 1);
    stats.hp += stats.growth.hp * extra;
    stats.weapon += stats.growth.weapon * extra;
    stats.armor += stats.growth.armor * extra;
    stats.shield += stats.growth.shield * extra;
    (stats, level)
}

#[derive(Clone, Debug, Serialize)]
pub struct FoldState {
    pub units: FoldUnits,
    pub camps: [CampSlot; 8],
    pub gold_mine: GoldMineState,
    pub minion_mines: [MinionMineSlot; 2],
    pub kraken: KrakenPhase,
}

impl Default for FoldState {
    fn default() -> Self {
        Self::new(FoldUnits::default())
    }
}

impl FoldState {
    fn new(units: FoldUnits) -> Self {
        let camps = std::array::from_fn(|_| CampSlot {
            members: Vec::new(),
            respawn_tick: Some(CAMP_FIRST_SPAWN_TICK),
        });
        Self {
            units,
            camps,
            gold_mine: GoldMineState {
                entity_id: None,
                owner: None,
                stored_gold: GOLD_MINE_START,
                respawn_tick: Some(GOLD_MINE_FIRST_SPAWN_TICK),
                retired: false,
                last_combat_tick: 0,
            },
            minion_mines: std::array::from_fn(|i| MinionMineSlot {
                anchor: MINION_MINE_POSITIONS[i],
                entity_id: None,
                owner: None,
                respawn_tick: Some(MINION_MINE_FIRST_SPAWN_TICK),
                last_combat_tick: 0,
            }),
            kraken: KrakenPhase::Dormant,
        }
    }

    pub fn jungle_shop_unlocked(&self, tick: u64) -> bool {
        tick >= JUNGLE_SHOP_UNLOCK_TICK
    }

    pub fn owned_minion_mines(&self, team: u8) -> u8 {
        self.minion_mines
            .iter()
            .filter(|slot| slot.owner == Some(team))
            .count() as u8
    }

    pub fn minion_empowerment(&self, team: u8) -> (f32, f32) {
        let bonus = 0.2 * f32::from(self.owned_minion_mines(team));
        (1.0 + bonus, 1.0 + bonus)
    }
}

fn camp_specs(index: usize) -> &'static [CampSpawnSpec] {
    match index {
        0 => &[CampSpawnSpec {
            unit: CampUnitKind::Treant,
            anchor: Position {
                x: -40.915,
                y: 20.251,
            },
        }],
        1 => &[
            CampSpawnSpec {
                unit: CampUnitKind::BigBear,
                anchor: Position {
                    x: -44.42 - 1.11,
                    y: 31.91 + 0.32,
                },
            },
            CampSpawnSpec {
                unit: CampUnitKind::SmallBear,
                anchor: Position {
                    x: -44.42 + 1.0,
                    y: 31.91 - 0.8,
                },
            },
        ],
        2 => &[CampSpawnSpec {
            unit: CampUnitKind::ElderTreant,
            anchor: Position { x: -21.95, y: 24.0 },
        }],
        3 => &[
            CampSpawnSpec {
                unit: CampUnitKind::BigBear,
                anchor: Position {
                    x: -13.47 - 0.93,
                    y: 37.67,
                },
            },
            CampSpawnSpec {
                unit: CampUnitKind::SmallBear,
                anchor: Position {
                    x: -13.47 + 0.96,
                    y: 37.67,
                },
            },
        ],
        4 => &[CampSpawnSpec {
            unit: CampUnitKind::Treant,
            anchor: Position { x: 40.59, y: 20.75 },
        }],
        5 => &[
            CampSpawnSpec {
                unit: CampUnitKind::BigBear,
                anchor: Position {
                    x: 44.6 - 1.11,
                    y: 31.6 - 0.32,
                },
            },
            CampSpawnSpec {
                unit: CampUnitKind::SmallBear,
                anchor: Position {
                    x: 44.6 + 1.0,
                    y: 31.6 + 0.8,
                },
            },
        ],
        6 => &[CampSpawnSpec {
            unit: CampUnitKind::ElderTreant,
            anchor: Position { x: 22.5, y: 23.5 },
        }],
        7 => &[
            CampSpawnSpec {
                unit: CampUnitKind::BigBear,
                anchor: Position {
                    x: 13.635 - 0.745,
                    y: 37.46 - 0.72,
                },
            },
            CampSpawnSpec {
                unit: CampUnitKind::SmallBear,
                anchor: Position {
                    x: 13.635 + 0.745,
                    y: 37.46 + 0.72,
                },
            },
        ],
        _ => &[],
    }
}

pub(super) fn respawn_ticks(tick: u64) -> u64 {
    let min_ticks = 4 * TICK_HZ;
    if tick < 12 * 60 * TICK_HZ {
        let ramp = (tick * 20) / (12 * 60);
        min_ticks + ramp
    } else if tick < 24 * 60 * TICK_HZ {
        let ramp = ((tick - 12 * 60 * TICK_HZ) * 36) / (12 * 60);
        24 * TICK_HZ + ramp
    } else {
        60 * TICK_HZ
    }
}

impl Room<'_> {
    /// Activate standard 3v3 Halcyon Fold (`HalcyonFoldLevel_Base`) after team assignment.
    pub fn use_halcyon_fold(&mut self) -> Result<(), String> {
        if self.tick != 0 {
            return Err("Halcyon Fold requires an unstarted world".into());
        }
        if matches!(self.layout, Layout::HalcyonFold { .. }) {
            return Ok(());
        }
        let mut starts = [Position::default(); 6];
        let mut counts = [0usize; 2];
        for hero in self.entities.iter().filter(|e| e.kind == Kind::Hero) {
            let t = usize::from(hero.team);
            let &(x, z) = crate::aral::PADS[t]
                .get(counts[t])
                .ok_or("Halcyon Fold requires three seats on each team")?;
            let spot = starts
                .get_mut((hero.id - 1) as usize)
                .ok_or("Halcyon Fold requires six roster seats")?;
            *spot = Position { x, y: z };
            counts[t] += 1;
        }
        if counts != [3, 3] {
            return Err("Halcyon Fold requires three seats on each team".into());
        }
        self.layout = Layout::HalcyonFold {
            hero_starts: starts,
        };
        self.entities.retain(|actor| actor.kind == Kind::Hero);
        let units = FoldUnits::default();
        for idx in 0..self.entities.len() {
            let hero = &mut self.entities[idx];
            hero.position = self.state.layout.home(hero);
            hero.gold = 600;
            self.state.scores[(hero.id - 1) as usize]
                .gold_income
                .starting_balance = hero.gold;
        }
        for team in 0..2 {
            let first = FIRST_STATIC_ACTOR + u32::from(team) * 8;
            self.next_id = first + 1;
            self.spawn_with_stats(
                team,
                Kind::Crystal,
                units.rows[12 + usize::from(team)],
                self.layout.objective(team),
                false,
            );
            for (slot, row_idx, x, z) in HF_TOWERS {
                self.next_id = first + u32::from(slot);
                self.spawn_with_stats(
                    team,
                    Kind::Tower,
                    units.rows[row_idx],
                    point(team, x, z),
                    false,
                );
            }
        }
        self.next_id = FIRST_DYNAMIC_ACTOR;
        self.state.halcyon_fold = Some(Box::new(FoldState::new(units)));
        Ok(())
    }

    pub fn halcyon_fold_state(&self) -> Option<&FoldState> {
        self.state.halcyon_fold.as_deref()
    }

    pub fn halcyon_fold_structure_slot(&self, actor: u32) -> Option<u8> {
        if !matches!(self.layout, Layout::HalcyonFold { .. }) {
            return None;
        }
        let slot = actor.checked_sub(FIRST_STATIC_ACTOR)?;
        let kind = self.entities[self.index(actor)?].kind;
        (slot < 16 && matches!(kind, Kind::Crystal | Kind::Tower)).then_some(slot as u8)
    }

    pub(super) fn is_halcyon_fold_outer_tower(&self, actor: &Entity) -> bool {
        actor.kind == Kind::Tower
            && matches!(self.layout, Layout::HalcyonFold { .. })
            && actor.position == point(actor.team, 17.06, 1.93)
    }

    pub(super) fn at_jungle_shop(&self, actor: &Entity) -> bool {
        let unlocked = self
            .state
            .halcyon_fold
            .as_ref()
            .is_some_and(|fold| fold.jungle_shop_unlocked(self.state.tick))
            || self.state.blitz.is_some();
        unlocked && actor.position.dist(JUNGLE_SHOP_POS) <= JUNGLE_SHOP_RADIUS
    }

    /// Returns the wire shopkeeper actor ID (`2000` left base, `2008` right base,
    /// `2016` center jungle shop) if `id` is an alive hero currently eligible to
    /// buy at a 3v3 Halcyon Fold or Blitz item shop.
    pub fn shop_3v3_blitz_keeper_for_actor(&self, id: u32) -> Option<u32> {
        if !matches!(
            self.layout,
            Layout::HalcyonFold { .. } | Layout::Blitz { .. }
        ) {
            return None;
        }
        let hero = self.index(id).map(|i| &self.entities[i])?;
        if hero.kind != Kind::Hero || hero.hp <= 0. || self.winner.is_some() {
            return None;
        }
        if self.at_jungle_shop(hero) {
            return Some(2016);
        }
        if self.at_item_shop(hero) {
            return Some(if hero.team == 0 { 2000 } else { 2008 });
        }
        None
    }

    pub(super) fn install_halcyon_fold_units(&mut self, data: &GameData) {
        if let Some(fold) = &mut self.state.halcyon_fold {
            fold.units.load(data);
        }
        let Some(units) = self.state.halcyon_fold.as_ref().map(|f| f.units) else {
            return;
        };
        for entity in 0..self.entities.len() {
            let id = self.entities[entity].id;
            let Some(slot) = self.halcyon_fold_structure_slot(id) else {
                continue;
            };
            let team = usize::from(self.entities[entity].team);
            let replacement = match slot % 8 {
                1 => units.rows[12 + team],
                3 | 4 => units.rows[11],
                5 => units.rows[10],
                6 => units.rows[9],
                7 => units.rows[8],
                _ => continue,
            };
            let target = &mut self.entities[entity];
            target.type_id = replacement.id;
            target.stats = replacement;
            target.max_hp = replacement.hp;
            target.hp = replacement.hp;
        }
    }

    pub(super) fn advance_halcyon_fold(&mut self, events: &mut Vec<Event>) {
        if !matches!(self.layout, Layout::HalcyonFold { .. }) {
            return;
        }
        self.spawn_halcyon_fold_wave(events);
        self.advance_halcyon_fold_camps(events);
        self.advance_halcyon_fold_minion_mines(events);
        self.advance_halcyon_fold_gold_mine(events);
        self.advance_halcyon_fold_kraken(events);
    }

    fn spawn_halcyon_fold_wave(&mut self, events: &mut Vec<Event>) {
        let Some(elapsed) = self.tick.checked_sub(MINION_WAVE_START) else {
            return;
        };
        let offset = elapsed % MINION_WAVE_INTERVAL;
        if !offset.is_multiple_of(TICK_HZ) {
            return;
        }
        let Some(&slot) = [2, 2, 1, 0].get((offset / TICK_HZ) as usize) else {
            return;
        };
        let Some(fold) = self.state.halcyon_fold.as_ref() else {
            return;
        };
        let units = fold.units;
        let empowers = [fold.minion_empowerment(0), fold.minion_empowerment(1)];
        for team in 0..2 {
            let mut stats = units.rows[slot];
            let (hp_mult, dmg_mult) = empowers[usize::from(team)];
            stats.hp *= hp_mult;
            stats.growth.hp *= hp_mult;
            stats.weapon *= dmg_mult;
            stats.growth.weapon *= dmg_mult;
            self.spawn_lane_minion(team, stats, self.layout.wave(team, 0), events);
        }
    }

    fn advance_halcyon_fold_camps(&mut self, events: &mut Vec<Event>) {
        let tick = self.state.tick;
        let Some(mut fold) = self.state.halcyon_fold.take() else {
            return;
        };
        for (camp_idx, slot) in fold.camps.iter_mut().enumerate() {
            if let Some(due) = slot.respawn_tick {
                if tick >= due && slot.members.is_empty() {
                    slot.respawn_tick = None;
                    for spec in camp_specs(camp_idx) {
                        let (stats, level) = fold.units.camp_stats(spec.unit, tick);
                        let id = self.spawn_with_stats(
                            crate::native_camps::NEUTRAL_TEAM,
                            Kind::Neutral,
                            stats,
                            spec.anchor,
                            false,
                        );
                        self.entities.last_mut().unwrap().level = level;
                        slot.members.push(CampMember {
                            entity_id: id,
                            unit: spec.unit,
                            anchor: spec.anchor,
                            last_combat_tick: tick,
                        });
                        events.push(Event::Spawn { id });
                    }
                }
                continue;
            }
            for member in &mut slot.members {
                self.leash_neutral_unit(
                    member.entity_id,
                    member.anchor,
                    &mut member.last_combat_tick,
                );
            }
        }
        self.state.halcyon_fold = Some(fold);
    }

    fn advance_halcyon_fold_minion_mines(&mut self, events: &mut Vec<Event>) {
        let tick = self.state.tick;
        let Some(mut fold) = self.state.halcyon_fold.take() else {
            return;
        };
        for mine in &mut fold.minion_mines {
            if let Some(entity_id) = mine.entity_id {
                self.leash_neutral_unit(entity_id, mine.anchor, &mut mine.last_combat_tick);
                continue;
            }
            if let Some(due) = mine.respawn_tick
                && tick >= due
            {
                mine.respawn_tick = None;
                let (stats, level) = scale_jungle_stats(fold.units.rows[14], tick, 12);
                let team = mine.owner.unwrap_or(crate::native_camps::NEUTRAL_TEAM);
                let id = self.spawn_with_stats(team, Kind::Neutral, stats, mine.anchor, false);
                self.entities.last_mut().unwrap().level = level;
                mine.entity_id = Some(id);
                mine.last_combat_tick = tick;
                events.push(Event::Spawn { id });
            }
        }
        self.state.halcyon_fold = Some(fold);
    }

    fn advance_halcyon_fold_gold_mine(&mut self, events: &mut Vec<Event>) {
        let tick = self.state.tick;
        let Some(mut fold) = self.state.halcyon_fold.take() else {
            return;
        };
        if fold.gold_mine.retired {
            self.state.halcyon_fold = Some(fold);
            return;
        }
        if let Some(entity_id) = fold.gold_mine.entity_id {
            self.leash_neutral_unit(
                entity_id,
                GOLD_MINE_POS,
                &mut fold.gold_mine.last_combat_tick,
            );
            if tick.is_multiple_of(TICK_HZ) {
                fold.gold_mine.stored_gold =
                    (fold.gold_mine.stored_gold + GOLD_MINE_PER_SEC).min(GOLD_MINE_CAP);
                if let Some(owner) = fold.gold_mine.owner
                    && fold.gold_mine.stored_gold >= GOLD_MINE_CAP
                {
                    let payout = fold.gold_mine.stored_gold.round() as u32;
                    self.distribute_team_gold(owner, payout, None, events);
                    fold.gold_mine.stored_gold = GOLD_MINE_START;
                    events.push(Event::ObjectiveCaptured {
                        team: owner,
                        actor: entity_id,
                        gold: payout,
                    });
                }
            }
        } else if let Some(due) = fold.gold_mine.respawn_tick
            && tick >= due
            && tick < KRAKEN_SPAWN_TICK
        {
            fold.gold_mine.respawn_tick = None;
            let (stats, level) = scale_jungle_stats(fold.units.rows[7], tick, 12);
            let team = fold
                .gold_mine
                .owner
                .unwrap_or(crate::native_camps::NEUTRAL_TEAM);
            let id = self.spawn_with_stats(team, Kind::Neutral, stats, GOLD_MINE_POS, false);
            self.entities.last_mut().unwrap().level = level;
            fold.gold_mine.entity_id = Some(id);
            fold.gold_mine.last_combat_tick = tick;
            events.push(Event::Spawn { id });
        }
        self.state.halcyon_fold = Some(fold);
    }

    fn advance_halcyon_fold_kraken(&mut self, events: &mut Vec<Event>) {
        let tick = self.state.tick;
        let Some(mut fold) = self.state.halcyon_fold.take() else {
            return;
        };
        if tick >= KRAKEN_SPAWN_TICK && !fold.gold_mine.retired {
            fold.gold_mine.retired = true;
            fold.gold_mine.respawn_tick = None;
            if let Some(gold_id) = fold.gold_mine.entity_id.take()
                && let Some(idx) = self.index(gold_id)
            {
                self.entities[idx].hp = 0.0;
                self.entities[idx].order = Command::Stop;
                events.push(Event::Death {
                    id: gold_id,
                    killer: 0,
                });
            }
        }
        match fold.kraken {
            KrakenPhase::Dormant if tick >= KRAKEN_SPAWN_TICK => {
                let (stats, level) = scale_jungle_stats(fold.units.rows[15], tick, 12);
                let id = self.spawn_with_stats(
                    crate::native_camps::NEUTRAL_TEAM,
                    Kind::Neutral,
                    stats,
                    KRAKEN_POS,
                    false,
                );
                self.entities.last_mut().unwrap().level = level;
                fold.kraken = KrakenPhase::Jungle { entity_id: id };
                events.push(Event::Spawn { id });
            }
            KrakenPhase::Jungle { entity_id } => {
                let mut dummy = tick;
                self.leash_neutral_unit(entity_id, KRAKEN_POS, &mut dummy);
            }
            KrakenPhase::Respawning { until_tick } if tick >= until_tick => {
                let (stats, level) = scale_jungle_stats(fold.units.rows[15], tick, 12);
                let id = self.spawn_with_stats(
                    crate::native_camps::NEUTRAL_TEAM,
                    Kind::Neutral,
                    stats,
                    KRAKEN_POS,
                    false,
                );
                self.entities.last_mut().unwrap().level = level;
                fold.kraken = KrakenPhase::Jungle { entity_id: id };
                events.push(Event::Spawn { id });
            }
            _ => {}
        }
        self.state.halcyon_fold = Some(fold);
    }

    pub(super) fn leash_neutral_unit(
        &mut self,
        entity_id: u32,
        anchor: Position,
        last_combat_tick: &mut u64,
    ) {
        let Some(idx) = self.index(entity_id) else {
            return;
        };
        let tick = self.state.tick;
        let actor = &self.entities[idx];
        if actor.hp <= 0.0 {
            return;
        }
        if actor.order != Command::Stop {
            *last_combat_tick = tick;
        }
        let target_too_far = match actor.order {
            Command::Attack(target) => self.index(target).is_none_or(|j| {
                let t = &self.entities[j];
                t.hp <= 0.0 || t.position.dist(anchor) > CAMP_LEASH_RADIUS + actor.stats.range
            }),
            _ => false,
        };
        if actor.position.dist(anchor) > CAMP_LEASH_RADIUS || target_too_far {
            let step = actor.stats.speed / TICK_HZ as f32;
            let next = actor.position.advance(anchor, step);
            let e = &mut self.entities[idx];
            e.order = Command::Stop;
            e.winding = None;
            e.position = next;
            e.hp = (e.hp + e.max_hp * 0.02).min(e.max_hp);
            return;
        }
        if actor.order == Command::Stop {
            if actor.position.dist(anchor) > 0.1 {
                let step = actor.stats.speed / TICK_HZ as f32;
                let next = actor.position.advance(anchor, step);
                self.entities[idx].position = next;
            }
            if tick.saturating_sub(*last_combat_tick) >= 3 * TICK_HZ
                && self.entities[idx].hp < self.entities[idx].max_hp
            {
                let heal = self.entities[idx].max_hp * 0.005;
                let e = &mut self.entities[idx];
                e.hp = (e.hp + heal).min(e.max_hp);
            }
        }
    }

    fn distribute_team_gold(
        &mut self,
        team: u8,
        total: u32,
        prefer_killer: Option<u32>,
        events: &mut Vec<Event>,
    ) {
        let seats: Vec<usize> = self
            .entities
            .iter()
            .enumerate()
            .filter(|(_, e)| e.kind == Kind::Hero && e.team == team)
            .map(|(i, _)| i)
            .collect();
        if seats.is_empty() || total == 0 {
            return;
        }
        let share = total / seats.len() as u32;
        let mut remainder = total % seats.len() as u32;
        let killer_seat = prefer_killer
            .and_then(|k| seats.iter().copied().find(|&i| self.entities[i].id == k))
            .unwrap_or(seats[0]);
        for &seat in &seats {
            let mut amount = share;
            if remainder > 0 && seat == killer_seat {
                amount += remainder;
                remainder = 0;
            }
            if amount == 0 {
                continue;
            }
            let hero = &mut self.entities[seat];
            hero.gold += amount;
            let id = hero.id;
            self.state.scores[(id - 1) as usize]
                .gold_income
                .lane_proximity_awards += u64::from(amount);
            events.push(Event::GoldAward { id, amount });
        }
    }

    /// Resolve Halcyon Fold neutral & structure kill effects. Returns true if objective death
    /// replaced standard neutral bounty distribution.
    pub(super) fn on_halcyon_fold_lethal(
        &mut self,
        victim_idx: usize,
        killer_id: u32,
        killer_team: u8,
        events: &mut Vec<Event>,
    ) -> bool {
        if !matches!(self.layout, Layout::HalcyonFold { .. }) {
            return false;
        }
        let victim = &self.entities[victim_idx];
        let victim_id = victim.id;
        let victim_pos = victim.position;
        let bounty = victim
            .stats
            .physiology
            .map_or((0, 0.0), |p| (p.gold_bounty as u32, p.xp_bounty));
        let Some(mut fold) = self.state.halcyon_fold.take() else {
            return false;
        };

        // Captured Kraken died: schedule jungle Kraken respawn.
        if matches!(
            fold.kraken,
            KrakenPhase::Captured { entity_id, .. } if entity_id == victim_id
        ) {
            fold.kraken = KrakenPhase::Respawning {
                until_tick: self.state.tick + KRAKEN_RESPAWN_TICKS,
            };
            self.state.halcyon_fold = Some(fold);
            return false;
        }

        // Jungle Kraken captured: award team gold + XP and spawn allied Captured Kraken.
        if matches!(fold.kraken, KrakenPhase::Jungle { entity_id } if entity_id == victim_id) {
            if killer_team < 2 {
                let (mut stats, level) =
                    scale_jungle_stats(fold.units.rows[16], self.state.tick, 12);
                let _ = &mut stats;
                let captured_id =
                    self.spawn_with_stats(killer_team, Kind::Minion, stats, KRAKEN_POS, false);
                self.entities.last_mut().unwrap().level = level;
                fold.kraken = KrakenPhase::Captured {
                    entity_id: captured_id,
                    team: killer_team,
                };
                self.state.halcyon_fold = Some(fold);
                self.award_jungle_xp(killer_team, killer_id, victim_pos, bounty.1, events);
                events.push(Event::Spawn { id: captured_id });
                events.push(Event::ObjectiveCaptured {
                    team: killer_team,
                    actor: captured_id,
                    gold: 0,
                });
                return true;
            }
            fold.kraken = KrakenPhase::Respawning {
                until_tick: self.state.tick + KRAKEN_RESPAWN_TICKS,
            };
            self.state.halcyon_fold = Some(fold);
            return true;
        }

        // Gold Miner killed: pay out partial or full accumulated pool, flip owner, schedule respawn.
        if fold.gold_mine.entity_id == Some(victim_id) {
            fold.gold_mine.entity_id = None;
            let payout = if killer_team < 2 {
                let stored = fold.gold_mine.stored_gold.round() as u32;
                fold.gold_mine.owner = Some(killer_team);
                fold.gold_mine.stored_gold = GOLD_MINE_START;
                if !fold.gold_mine.retired && self.state.tick < KRAKEN_SPAWN_TICK {
                    fold.gold_mine.respawn_tick = Some(self.state.tick + GOLD_MINE_RESPAWN_TICKS);
                }
                self.state.halcyon_fold = Some(fold);
                self.distribute_team_gold(killer_team, stored, Some(killer_id), events);
                self.award_jungle_xp(killer_team, killer_id, victim_pos, bounty.1, events);
                events.push(Event::ObjectiveCaptured {
                    team: killer_team,
                    actor: victim_id,
                    gold: stored,
                });
                stored
            } else {
                self.state.halcyon_fold = Some(fold);
                0
            };
            let _ = payout;
            return true;
        }

        // Minion Mine captured: flip owner to capturing team, award bounty, schedule respawn.
        for mine in &mut fold.minion_mines {
            if mine.entity_id == Some(victim_id) {
                mine.entity_id = None;
                if killer_team < 2 {
                    mine.owner = Some(killer_team);
                    mine.respawn_tick = Some(self.state.tick + MINION_MINE_RESPAWN_TICKS);
                    self.state.halcyon_fold = Some(fold);
                    self.award_camp_rewards(
                        killer_team,
                        killer_id,
                        victim_pos,
                        bounty.0,
                        bounty.1,
                        None,
                        events,
                    );
                    events.push(Event::ObjectiveCaptured {
                        team: killer_team,
                        actor: victim_id,
                        gold: bounty.0,
                    });
                } else {
                    self.state.halcyon_fold = Some(fold);
                }
                return true;
            }
        }

        // Jungle camp unit killed: heal on treant kill, gold + ambient XP share, respawn on clear.
        for slot in &mut fold.camps {
            if let Some(pos) = slot
                .members
                .iter()
                .position(|member| member.entity_id == victim_id)
            {
                let unit = slot.members.remove(pos).unit;
                if slot.members.is_empty() {
                    slot.respawn_tick = Some(self.state.tick + CAMP_RESPAWN_TICKS);
                }
                self.state.halcyon_fold = Some(fold);
                self.award_camp_rewards(
                    killer_team,
                    killer_id,
                    victim_pos,
                    bounty.0,
                    bounty.1,
                    Some(unit),
                    events,
                );
                return true;
            }
        }

        self.state.halcyon_fold = Some(fold);
        false
    }

    fn award_jungle_xp(
        &mut self,
        team: u8,
        killer_id: u32,
        pos: Position,
        xp: f32,
        events: &mut Vec<Event>,
    ) {
        if xp <= 0.0 || team >= 2 {
            return;
        }
        let mut allies = [0usize; 6];
        let mut count = 0;
        for j in 0..self.entities.len().min(6) {
            let a = &self.entities[j];
            if a.team == team
                && a.kind == Kind::Hero
                && a.hp > 0.0
                && (a.id == killer_id || a.position.dist(pos) <= 10.0)
            {
                allies[count] = j;
                count += 1;
            }
        }
        if count > 0 {
            let share = if count == 1 {
                xp
            } else {
                xp * 1.2 / count as f32
            };
            for &j in &allies[..count] {
                self.award_xp(j, share, events);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn award_camp_rewards(
        &mut self,
        killer_team: u8,
        killer_id: u32,
        pos: Position,
        bounty: u32,
        xp: f32,
        treant: Option<CampUnitKind>,
        events: &mut Vec<Event>,
    ) {
        if killer_team >= 2 {
            return;
        }
        if let Some(k_idx) = self.index(killer_id) {
            let killer = &mut self.entities[k_idx];
            if killer.kind == Kind::Hero && killer.team == killer_team {
                if bounty > 0 {
                    killer.gold += bounty;
                    self.state.scores[(killer.id - 1) as usize]
                        .gold_income
                        .last_hit_bounties += u64::from(bounty);
                    events.push(Event::GoldAward {
                        id: killer.id,
                        amount: bounty,
                    });
                }
                if let Some(unit) = treant
                    && matches!(unit, CampUnitKind::Treant | CampUnitKind::ElderTreant)
                    && killer.hp > 0.0
                {
                    let (flat, ratio) = if unit == CampUnitKind::ElderTreant {
                        (180.0, 0.20)
                    } else {
                        (120.0, 0.15)
                    };
                    let heal = flat + (killer.max_hp - killer.hp).max(0.0) * ratio;
                    let before = killer.hp;
                    killer.hp = (killer.hp + heal).min(killer.max_hp);
                    let gained = killer.hp - before;
                    if gained > 0.0 {
                        events.push(Event::Heal {
                            source: killer.id,
                            target: killer.id,
                            amount: gained,
                            hp: killer.hp,
                        });
                    }
                }
            }
        }
        // Ambient support gold share (40% of camp gold) to nearby living ally heroes within 10 units.
        let ambient_gold = (bounty * 2) / 5;
        if ambient_gold > 0 {
            for j in 0..self.entities.len().min(6) {
                let a = &mut self.entities[j];
                if a.kind == Kind::Hero
                    && a.team == killer_team
                    && a.hp > 0.0
                    && a.id != killer_id
                    && a.position.dist(pos) <= 10.0
                {
                    a.gold += ambient_gold;
                    let aid = a.id;
                    self.state.scores[(aid - 1) as usize]
                        .gold_income
                        .lane_proximity_awards += u64::from(ambient_gold);
                    events.push(Event::GoldAward {
                        id: aid,
                        amount: ambient_gold,
                    });
                }
            }
        }
        self.award_jungle_xp(killer_team, killer_id, pos, xp, events);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fold_world() -> World {
        let mut w = World::new(11, 6);
        w.use_six_human_roster().unwrap();
        w.assign_teams(&[0, 1, 0, 1, 0, 1]).unwrap();
        w.use_halcyon_fold().unwrap();
        w
    }

    #[test]
    fn fold_initializes_six_seats_16_slot_structures_and_600_starting_gold() {
        let mut w = fold_world();
        w.use_halcyon_fold().unwrap();
        assert_eq!(w.entities.len(), 18);
        let heroes: Vec<_> = w.entities.iter().take(6).collect();
        assert!(
            heroes
                .iter()
                .all(|h| h.kind == Kind::Hero && h.gold == 600 && h.level == 1)
        );
        let mut slots: Vec<_> = w
            .entities
            .iter()
            .filter_map(|e| w.halcyon_fold_structure_slot(e.id))
            .collect();
        slots.sort_unstable();
        assert_eq!(slots, [1, 3, 4, 5, 6, 7, 9, 11, 12, 13, 14, 15]);
        assert_eq!(w.native_xp_required(1), Some(68.0));
        assert_eq!(w.native_xp_required(12), Some(68.0 + 16.0 * 11.0));
    }

    #[test]
    fn jungle_shop_unlocks_at_4_minutes_and_allows_purchase() {
        let mut w = fold_world();
        w.use_catalog_store(halcyon_data::items::StoreSettings::HalcyonFold);
        w.entities[0].position = JUNGLE_SHOP_POS;
        w.entities[0].gold = 1000;
        assert_eq!(
            w.purchase(1, BasicItem::WeaponBlade),
            Err("outside base shop")
        );
        w.tick = JUNGLE_SHOP_UNLOCK_TICK;
        assert!(w.halcyon_fold_state().unwrap().jungle_shop_unlocked(w.tick));
        assert!(w.purchase(1, BasicItem::WeaponBlade).is_ok());
        assert_eq!(w.entities[0].gold, 700);
    }

    #[test]
    fn jungle_camps_leash_heal_on_treant_kill_share_ambient_xp_and_respawn() {
        let mut w = fold_world();
        w.tick = CAMP_FIRST_SPAWN_TICK - 1;
        let events = w.step();
        let spawned_camps: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Event::Spawn { id } => Some(*id),
                _ => None,
            })
            .collect();
        assert_eq!(spawned_camps.len(), 12);
        let treant_id = w.halcyon_fold_state().unwrap().camps[0].members[0].entity_id;
        let t_idx = w.index(treant_id).unwrap();
        let anchor = w.entities[t_idx].position;

        // Displace treant beyond leash radius and verify it drops aggro and pulls back.
        w.entities[t_idx].position = Position {
            x: anchor.x + CAMP_LEASH_RADIUS + 2.0,
            y: anchor.y,
        };
        w.entities[t_idx].order = Command::Attack(1);
        w.step();
        let t_idx = w.index(treant_id).unwrap();
        assert_eq!(w.entities[t_idx].order, Command::Stop);
        assert!(w.entities[t_idx].position.dist(anchor) < CAMP_LEASH_RADIUS + 2.0);

        // Place hero 1 (team 0) as killer & hero 3 (team 0) nearby for ambient XP/gold.
        let t_pos = w.entities[t_idx].position;
        w.entities[0].position = t_pos;
        w.entities[0].hp = 200.0;
        w.entities[2].position = Position {
            x: t_pos.x + 3.0,
            y: t_pos.y,
        };
        let gold0_before = w.entities[0].gold;
        let gold2_before = w.entities[2].gold;
        let xp0_before = w.entities[0].xp;
        let xp2_before = w.entities[2].xp;

        let mut ev = Vec::new();
        w.hurt(1, 0, t_idx, 5000.0, DamageSchool::True, &mut ev);
        assert!(w.entities[0].hp > 200.0, "treant kill heals killer");
        assert!(
            ev.iter()
                .any(|e| matches!(e, Event::Heal { target: 1, .. }))
        );
        assert_eq!(w.entities[0].gold - gold0_before, 80);
        assert_eq!(w.entities[2].gold - gold2_before, 32);
        assert!(w.entities[0].xp > xp0_before);
        assert!(w.entities[2].xp > xp2_before);
        assert!(w.halcyon_fold_state().unwrap().camps[0].members.is_empty());

        let respawn_at = w.halcyon_fold_state().unwrap().camps[0]
            .respawn_tick
            .unwrap();
        w.tick = respawn_at - 1;
        let respawn_events = w.step();
        assert!(
            respawn_events
                .iter()
                .any(|e| matches!(e, Event::Spawn { .. }))
        );
        assert_eq!(w.halcyon_fold_state().unwrap().camps[0].members.len(), 1);
    }

    #[test]
    fn gold_mine_grows_pays_partial_and_full_and_minion_mines_empower_waves() {
        let mut w = fold_world();
        w.tick = GOLD_MINE_FIRST_SPAWN_TICK - 1;
        w.step();

        let gm_id = w.halcyon_fold_state().unwrap().gold_mine.entity_id.unwrap();
        assert_eq!(
            w.halcyon_fold_state().unwrap().gold_mine.stored_gold,
            GOLD_MINE_START
        );

        // Advance 20 seconds -> 50 + 20 * 2.5 = 100g partial pool.
        for _ in 0..(20 * TICK_HZ) {
            w.step();
        }
        assert!((w.halcyon_fold_state().unwrap().gold_mine.stored_gold - 100.0).abs() < 0.01);

        let g_before: u32 = [0, 2, 4].iter().map(|&i| w.entities[i].gold).sum();
        let gm_idx = w.index(gm_id).unwrap();
        let mut ev = Vec::new();
        w.hurt(1, 0, gm_idx, 9000.0, DamageSchool::True, &mut ev);
        let g_after: u32 = [0, 2, 4].iter().map(|&i| w.entities[i].gold).sum();
        assert_eq!(
            g_after - g_before,
            100,
            "partial payout distributed across team"
        );
        assert_eq!(w.halcyon_fold_state().unwrap().gold_mine.owner, Some(0));

        // Respawn Gold Mine and advance until cap 400g -> auto full payout on owned mine.
        let gm_respawn = w
            .halcyon_fold_state()
            .unwrap()
            .gold_mine
            .respawn_tick
            .unwrap();
        w.tick = gm_respawn - 1;
        w.step();
        assert!(
            w.halcyon_fold_state()
                .unwrap()
                .gold_mine
                .entity_id
                .is_some()
        );
        let g_cap_before: u32 = [0, 2, 4].iter().map(|&i| w.entities[i].gold).sum();
        let mut saw_cap_capture = false;
        for _ in 0..(142 * TICK_HZ) {
            let step_ev = w.step();
            if step_ev.iter().any(|e| {
                matches!(
                    e,
                    Event::ObjectiveCaptured {
                        team: 0,
                        gold: 400,
                        ..
                    }
                )
            }) {
                saw_cap_capture = true;
                break;
            }
        }
        assert!(saw_cap_capture);
        let g_cap_after: u32 = [0, 2, 4].iter().map(|&i| w.entities[i].gold).sum();
        assert!(g_cap_after - g_cap_before >= 400);

        // Capture both Minion Mines for team 0 and compare team 0 vs team 1 wave minion stats.
        let mut ev = Vec::new();
        for mine_idx in 0..2 {
            let mid = w.halcyon_fold_state().unwrap().minion_mines[mine_idx]
                .entity_id
                .unwrap();
            let i = w.index(mid).unwrap();
            w.hurt(1, 0, i, 9000.0, DamageSchool::True, &mut ev);
        }
        assert_eq!(w.halcyon_fold_state().unwrap().owned_minion_mines(0), 2);
        let elapsed_since_start = (w.tick - MINION_WAVE_START) % MINION_WAVE_INTERVAL;
        w.tick += MINION_WAVE_INTERVAL - elapsed_since_start - 1;
        let wave_ev = w.step();
        let spawned_minions: Vec<_> = wave_ev
            .iter()
            .filter_map(|e| match e {
                Event::Spawn { id } => w.index(*id).map(|idx| &w.entities[idx]),
                _ => None,
            })
            .filter(|e| e.kind == Kind::Minion)
            .collect();
        let t0 = spawned_minions.iter().find(|e| e.team == 0).unwrap();
        let t1 = spawned_minions.iter().find(|e| e.team == 1).unwrap();
        assert!(t0.max_hp > t1.max_hp * 1.35);
        assert!(t0.weapon_power() > t1.weapon_power() * 1.35);
    }

    #[test]
    fn kraken_spawns_at_15_min_replacing_gold_mine_and_breaks_structures_to_win() {
        let mut w = fold_world();
        w.tick = GOLD_MINE_FIRST_SPAWN_TICK - 1;
        w.step();
        assert!(
            w.halcyon_fold_state()
                .unwrap()
                .gold_mine
                .entity_id
                .is_some()
        );

        w.tick = KRAKEN_SPAWN_TICK - 1;
        let ev = w.step();
        assert!(w.halcyon_fold_state().unwrap().gold_mine.retired);
        let KrakenPhase::Jungle {
            entity_id: kraken_j,
        } = w.halcyon_fold_state().unwrap().kraken
        else {
            panic!("Kraken should spawn in jungle at 15:00");
        };
        assert!(
            ev.iter()
                .any(|e| matches!(e, Event::Spawn { id } if *id == kraken_j))
        );

        // Capture Kraken for team 0.
        let k_idx = w.index(kraken_j).unwrap();
        let mut cap_ev = Vec::new();
        w.hurt(1, 0, k_idx, 50_000.0, DamageSchool::True, &mut cap_ev);
        let KrakenPhase::Captured {
            entity_id: kraken_cap,
            team: 0,
        } = w.halcyon_fold_state().unwrap().kraken
        else {
            panic!("Captured Kraken should belong to team 0");
        };
        let cap_idx = w.index(kraken_cap).unwrap();
        assert_eq!(w.entities[cap_idx].kind, Kind::Minion);
        assert_eq!(w.entities[cap_idx].team, 0);

        // Put Captured Kraken in front of team 1's Outer Turret -> breaks turret then Vain Crystal deterministically.
        let mut replay = fold_world();
        replay.tick = GOLD_MINE_FIRST_SPAWN_TICK - 1;
        replay.step();
        replay.tick = KRAKEN_SPAWN_TICK - 1;
        replay.step();
        let KrakenPhase::Jungle {
            entity_id: r_kraken_j,
        } = replay.halcyon_fold_state().unwrap().kraken
        else {
            panic!("Kraken should spawn in replay jungle");
        };
        assert_eq!(r_kraken_j, kraken_j);
        let replay_k_idx = replay.index(r_kraken_j).unwrap();
        let mut r_cap_ev = Vec::new();
        replay.hurt(
            1,
            0,
            replay_k_idx,
            50_000.0,
            DamageSchool::True,
            &mut r_cap_ev,
        );
        let KrakenPhase::Captured {
            entity_id: r_kraken_cap,
            ..
        } = replay.halcyon_fold_state().unwrap().kraken
        else {
            panic!("Captured Kraken should belong to replay team 0");
        };
        let r_cap_idx = replay.index(r_kraken_cap).unwrap();
        let outer_turret_id = w
            .entities
            .iter()
            .find(|e| {
                e.team == 1
                    && e.kind == Kind::Tower
                    && e.type_id == EntityTemplate::HfOuterTurret.as_raw()
            })
            .unwrap()
            .id;
        for room in [&mut w, &mut replay] {
            for e in 0..room.entities.len() {
                if room.entities[e].team == 1 && room.entities[e].kind == Kind::Tower {
                    if room.entities[e].id == outer_turret_id {
                        room.entities[e].hp = 200.0;
                    } else {
                        room.entities[e].hp = 0.0;
                    }
                }
                if room.entities[e].team == 1 && room.entities[e].kind == Kind::Crystal {
                    room.entities[e].hp = 300.0;
                }
            }
        }
        // Position Kraken at mid-outer turret to break outer turret first, then crystal.
        w.entities[cap_idx].position = Position { x: 16.0, y: 1.93 };
        replay.entities[r_cap_idx].position = Position { x: 16.0, y: 1.93 };
        for _ in 0..40 {
            let a = w.step();
            let b = replay.step();
            assert_eq!(a, b);
            assert_eq!(w.state_digest(), replay.state_digest());
            if w.entities[w.index(outer_turret_id).unwrap()].hp <= 0.0 {
                break;
            }
        }
        assert!(w.entities[w.index(outer_turret_id).unwrap()].hp <= 0.0);

        let cap_idx = w.index(kraken_cap).unwrap();
        let r_cap_idx = replay.index(r_kraken_cap).unwrap();
        w.entities[cap_idx].position = Position { x: 74.5, y: 19.9 };
        replay.entities[r_cap_idx].position = Position { x: 74.5, y: 19.9 };
        for _ in 0..40 {
            let a = w.step();
            let b = replay.step();
            assert_eq!(a, b);
            assert_eq!(w.state_digest(), replay.state_digest());
            if w.winner.is_some() {
                break;
            }
        }
        assert_eq!(w.winner, Some(0));
    }

    #[test]
    #[ignore = "requires local delivery and A001/server_data_extra map assets"]
    fn halcyon_fold_loads_a001_navmesh_hf_units_and_standard_3v3_store() {
        let base = std::env::var_os("HALCYON_DELIVERY").unwrap();
        let extra = std::path::PathBuf::from(std::env::var_os("HALCYON_DATA_EXTRA").unwrap());
        let data = GameData::load_with_extra(base, Some(&extra)).unwrap();
        let mut w = World::from_data(5, 6, &data).unwrap();
        w.use_six_human_roster().unwrap();
        w.assign_teams(&[0, 1, 0, 1, 0, 1]).unwrap();
        w.use_halcyon_fold().unwrap();
        w.use_catalog_store(halcyon_data::items::StoreSettings::HalcyonFold);
        if let Some(entries) =
            data.store_for_settings(halcyon_data::items::StoreSettings::HalcyonFold.key())
        {
            w.use_item_store(entries);
        }
        w.install_map_data(&data).unwrap();
        assert!(matches!(w.layout, Layout::HalcyonFold { .. }));
        assert_eq!(w.entities.len(), 18);
        assert!(w.native_navigation.is_some());

        // Move to A001 Jungle Shop after 4:00 unlock and buy WeaponBlade from Standard_3v3 store.
        w.tick = JUNGLE_SHOP_UNLOCK_TICK;
        w.entities[0].position = JUNGLE_SHOP_POS;
        w.entities[0].gold = 600;
        w.purchase(1, BasicItem::WeaponBlade).unwrap();
        assert_eq!(w.entities[0].gold, 300);

        // Teleport Boots are 5v5-only, unavailable in ItemStore_Standard_3v3.
        assert_eq!(
            w.purchase(1, BasicItem::TeleportBoots),
            Err("item unavailable")
        );
    }
}
