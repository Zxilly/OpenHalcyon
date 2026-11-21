use super::*;
use crate::layout::point;
use crate::map_3v3::{
    FIRST_DYNAMIC_ACTOR, FIRST_STATIC_ACTOR, GOLD_MINE_POS, KRAKEN_POS, MINION_MINE_POSITIONS,
};

pub const BLITZ_DURATION_TICKS: u64 = 5 * 60 * TICK_HZ;
pub const BLITZ_TARGET_POINTS: u32 = 15;
pub const BLITZ_START_GOLD: u32 = 6000;
pub const BLITZ_START_LEVEL: u8 = 9;
pub const BLITZ_WAVE_START: u64 = 10 * TICK_HZ;
pub const BLITZ_WAVE_INTERVAL: u64 = 20 * TICK_HZ;
pub const BLITZ_OBJECTIVE_RESPAWN_TICKS: u64 = 45 * TICK_HZ;

const BLITZ_TOWERS: [(u8, usize, f32, f32); 5] = [
    (3, 6, 75.48, 11.96),
    (4, 6, 68.59, 19.97),
    (5, 5, 54.0, 2.92),
    (6, 4, 35.78, 1.17),
    (7, 3, 17.06, 1.93),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum BlitzTalentSlot {
    Slot0 = 0,
    Slot1 = 1,
    Slot2 = 2,
    NoTalent = 255,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum BlitzObjectiveKind {
    MiddleSentry,
    AllySentryLeft,
    AllySentryRight,
    GoldMiner,
}

#[derive(Clone, Debug, Serialize)]
pub struct BlitzObjective {
    pub kind: BlitzObjectiveKind,
    pub team: u8,
    pub anchor: Position,
    pub entity_id: Option<u32>,
    pub respawn_tick: Option<u64>,
    pub last_combat_tick: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct BlitzTalentApplied {
    pub slot: BlitzTalentSlot,
    pub level: u8,
    pub weapon_bonus: f32,
    pub crystal_bonus: f32,
    pub hp_bonus: f32,
    pub armor_bonus: f32,
    pub shield_bonus: f32,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct BlitzUnits {
    /// `[RangedMinion, LeadMinion, TankMinion, HfOuterTurret, HfMiddleTurret, HfBaseTurret, HfVainTurret, VainCrystalHome3v3, VainCrystalAway3v3, MiddleSentry, AllySentry, GoldMiner]`
    pub(crate) rows: [Stats; 12],
}

fn phys(attack_speed: f32, gold_bounty: f32, xp_bounty: f32) -> Option<Physiology> {
    Some(Physiology {
        hp_regen: 0.0,
        hp_regen_growth: 0.0,
        energy_regen: 0.0,
        energy_regen_growth: 0.0,
        attack_speed,
        attack_speed_growth: 0.0,
        collision_radius: 0.6,
        sight: 10.0,
        gold_bounty,
        xp_bounty,
    })
}

fn with_phys(mut s: Stats, hp: f32, wp: f32, p: Option<Physiology>) -> Stats {
    s.growth.hp = hp;
    s.growth.weapon = wp;
    if s.physiology.is_none() {
        s.physiology = p;
    }
    s
}

impl Default for BlitzUnits {
    fn default() -> Self {
        Self {
            rows: [
                with_phys(
                    Stats::new(EntityTemplate::RangedMinion, 350.0, 10.0, 0.0, 4.5, 3.2),
                    56.0,
                    4.0,
                    phys(1.0, 25.0, 6.0),
                ),
                with_phys(
                    Stats::new(EntityTemplate::LeadMinion, 522.0, 32.0, 0.0, 2.9, 3.2),
                    272.0,
                    15.0,
                    phys(1.0, 60.0, 12.0),
                ),
                with_phys(
                    Stats::new(EntityTemplate::TankMinion, 400.0, 13.0, 0.0, 1.2, 3.2),
                    96.0,
                    3.0,
                    phys(1.0, 35.0, 6.0),
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
                with_phys(
                    Stats::new(
                        EntityTemplate::BlitzMiddleSentry,
                        3600.0,
                        100.0,
                        60.0,
                        5.0,
                        3.2,
                    ),
                    700.0,
                    12.0,
                    phys(1.65, 125.0, 15.0),
                ),
                with_phys(
                    Stats::new(
                        EntityTemplate::BlitzAllySentry,
                        1800.0,
                        120.0,
                        65.0,
                        5.0,
                        3.2,
                    ),
                    350.0,
                    15.0,
                    phys(1.65, 125.0, 15.0),
                ),
                with_phys(
                    Stats::new(
                        EntityTemplate::JungleMinionGoldMiner,
                        2500.0,
                        50.0,
                        0.0,
                        4.0,
                        3.2,
                    ),
                    250.0,
                    12.0,
                    phys(1.5, 300.0, 20.0),
                ),
            ],
        }
    }
}

impl BlitzUnits {
    pub fn load(&mut self, data: &GameData) {
        const NAMES: [&str; 12] = [
            "RangedMinion",
            "LeadMinion",
            "TankMinion",
            "HF_OuterTurret",
            "HF_MiddleTurret",
            "HF_BaseTurret",
            "HF_VainTurret",
            "VainCrystal_Home_3v3",
            "VainCrystal_Away_3v3",
            "JungleMinion_BlitzRevamp_MiddleSentry",
            "JungleMinion_Blitz_AllySentry",
            "JungleMinion_GoldMiner",
        ];
        for (stats, name) in self.rows.iter_mut().zip(NAMES) {
            if let Some(unit) = data.unit(name) {
                *stats = Stats::load(stats.id, unit, data);
            }
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct BlitzState {
    pub units: BlitzUnits,
    pub remaining_ticks: u64,
    pub team_points: [u32; 2],
    pub target_points: u32,
    pub sudden_death: bool,
    pub objectives: [BlitzObjective; 4],
    pub talents: [Option<BlitzTalentApplied>; 6],
}

impl BlitzState {
    fn new(units: BlitzUnits) -> Self {
        Self {
            units,
            remaining_ticks: BLITZ_DURATION_TICKS,
            team_points: [0, 0],
            target_points: BLITZ_TARGET_POINTS,
            sudden_death: false,
            objectives: [
                BlitzObjective {
                    kind: BlitzObjectiveKind::MiddleSentry,
                    team: crate::native_camps::NEUTRAL_TEAM,
                    anchor: KRAKEN_POS,
                    entity_id: None,
                    respawn_tick: Some(0),
                    last_combat_tick: 0,
                },
                BlitzObjective {
                    kind: BlitzObjectiveKind::AllySentryLeft,
                    team: 0,
                    anchor: MINION_MINE_POSITIONS[0],
                    entity_id: None,
                    respawn_tick: Some(0),
                    last_combat_tick: 0,
                },
                BlitzObjective {
                    kind: BlitzObjectiveKind::AllySentryRight,
                    team: 1,
                    anchor: MINION_MINE_POSITIONS[1],
                    entity_id: None,
                    respawn_tick: Some(0),
                    last_combat_tick: 0,
                },
                BlitzObjective {
                    kind: BlitzObjectiveKind::GoldMiner,
                    team: crate::native_camps::NEUTRAL_TEAM,
                    anchor: GOLD_MINE_POS,
                    entity_id: None,
                    respawn_tick: Some(0),
                    last_combat_tick: 0,
                },
            ],
            talents: [None; 6],
        }
    }

    pub fn remaining_seconds(&self) -> f32 {
        self.remaining_ticks as f32 / TICK_HZ as f32
    }
}

pub(super) fn respawn_ticks(tick: u64) -> u64 {
    10 * TICK_HZ + (tick / (60 * TICK_HZ)).min(5) * TICK_HZ
}

impl Room<'_> {
    /// Activate 3v3 Blitz (`Blitz_Revamp_Level`) after team assignment.
    pub fn use_blitz(&mut self) -> Result<(), String> {
        if self.tick != 0 {
            return Err("Blitz requires an unstarted world".into());
        }
        if matches!(self.layout, Layout::Blitz { .. }) {
            return Ok(());
        }
        let mut starts = [Position::default(); 6];
        let mut counts = [0usize; 2];
        for hero in self.entities.iter().filter(|e| e.kind == Kind::Hero) {
            let t = usize::from(hero.team);
            let &(x, z) = crate::aral::PADS[t]
                .get(counts[t])
                .ok_or("Blitz requires three seats on each team")?;
            let spot = starts
                .get_mut((hero.id - 1) as usize)
                .ok_or("Blitz requires six roster seats")?;
            *spot = Position { x, y: z };
            counts[t] += 1;
        }
        if counts != [3, 3] {
            return Err("Blitz requires three seats on each team".into());
        }
        self.layout = Layout::Blitz {
            hero_starts: starts,
        };
        self.entities.retain(|actor| actor.kind == Kind::Hero);
        let units = BlitzUnits::default();
        for idx in 0..self.entities.len() {
            let hero = &mut self.entities[idx];
            hero.position = self.state.layout.home(hero);
            hero.level = BLITZ_START_LEVEL;
            hero.skill_points = BLITZ_START_LEVEL;
            hero.apply_growth(BLITZ_START_LEVEL - 1);
            hero.gold = BLITZ_START_GOLD;
            self.state.scores[(hero.id - 1) as usize]
                .gold_income
                .starting_balance = BLITZ_START_GOLD;
        }
        for team in 0..2 {
            let first = FIRST_STATIC_ACTOR + u32::from(team) * 8;
            self.next_id = first + 1;
            self.spawn_with_stats(
                team,
                Kind::Crystal,
                units.rows[7 + usize::from(team)],
                self.layout.objective(team),
                false,
            );
            for (slot, row_idx, x, z) in BLITZ_TOWERS {
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
        let mut blitz = BlitzState::new(units);
        for obj in &mut blitz.objectives {
            let row = match obj.kind {
                BlitzObjectiveKind::MiddleSentry => units.rows[9],
                BlitzObjectiveKind::AllySentryLeft | BlitzObjectiveKind::AllySentryRight => {
                    units.rows[10]
                }
                BlitzObjectiveKind::GoldMiner => units.rows[11],
            };
            let id = self.spawn_with_stats(obj.team, Kind::Neutral, row, obj.anchor, false);
            obj.entity_id = Some(id);
            obj.respawn_tick = None;
        }
        self.state.blitz = Some(Box::new(blitz));
        Ok(())
    }

    pub fn blitz_state(&self) -> Option<&BlitzState> {
        self.state.blitz.as_deref()
    }

    pub fn blitz_structure_slot(&self, actor: u32) -> Option<u8> {
        if !matches!(self.layout, Layout::Blitz { .. }) {
            return None;
        }
        let slot = actor.checked_sub(FIRST_STATIC_ACTOR)?;
        let kind = self.entities[self.index(actor)?].kind;
        (slot < 16 && matches!(kind, Kind::Crystal | Kind::Tower)).then_some(slot as u8)
    }

    pub(super) fn is_blitz_outer_tower(&self, actor: &Entity) -> bool {
        actor.kind == Kind::Tower
            && matches!(self.layout, Layout::Blitz { .. })
            && actor.position == point(actor.team, 17.06, 1.93)
    }

    pub(super) fn install_blitz_units(&mut self, data: &GameData) {
        if let Some(blitz) = &mut self.state.blitz {
            blitz.units.load(data);
        }
        let Some(units) = self.state.blitz.as_ref().map(|b| b.units) else {
            return;
        };
        for entity in 0..self.entities.len() {
            let id = self.entities[entity].id;
            if let Some(slot) = self.blitz_structure_slot(id) {
                let team = usize::from(self.entities[entity].team);
                let replacement = match slot % 8 {
                    1 => units.rows[7 + team],
                    3 | 4 => units.rows[6],
                    5 => units.rows[5],
                    6 => units.rows[4],
                    7 => units.rows[3],
                    _ => continue,
                };
                let target = &mut self.entities[entity];
                target.type_id = replacement.id;
                target.stats = replacement;
                target.max_hp = replacement.hp;
                target.hp = replacement.hp;
            }
        }
    }

    /// Equips and applies a Blitz talent for a hero seat, reverting any prior talent modifiers.
    pub fn equip_blitz_talent(
        &mut self,
        actor: u32,
        slot: BlitzTalentSlot,
        level: u8,
        data: Option<&GameData>,
    ) -> Result<(), &'static str> {
        if !matches!(self.layout, Layout::Blitz { .. }) {
            return Err("Blitz talent requires Blitz mode");
        }
        let i = self.index(actor).ok_or("unknown actor")?;
        let seat = (actor.checked_sub(1).ok_or("invalid hero actor")?) as usize;
        if seat >= 6 || self.entities[i].kind != Kind::Hero {
            return Err("not a hero");
        }
        if slot == BlitzTalentSlot::NoTalent && level != 0 {
            return Err("NoTalent requires level zero");
        }
        if slot != BlitzTalentSlot::NoTalent && level == 0 {
            return Err("equipped talent requires non-zero level");
        }
        // Revert any previous Blitz generic modifiers cleanly.
        let prev = self
            .state
            .blitz
            .as_mut()
            .and_then(|b| b.talents[seat].take());
        if let Some(old) = prev {
            let h = &mut self.entities[i];
            h.stats.weapon = (h.stats.weapon - old.weapon_bonus).max(0.0);
            h.item_stats.crystal = (h.item_stats.crystal - old.crystal_bonus).max(0.0);
            h.stats.armor = (h.stats.armor - old.armor_bonus).max(0.0);
            h.stats.shield = (h.stats.shield - old.shield_bonus).max(0.0);
            h.bonus_hp = (h.bonus_hp - old.hp_bonus).max(0.0);
            h.max_hp = (h.max_hp - old.hp_bonus).max(1.0);
            h.hp = h.hp.min(h.max_hp);
        }
        // Clear any modeled or HeroKit talent state first, then re-apply if the active slot resolves.
        {
            let h = &mut self.entities[i];
            h.long_shot = None;
            h.mobile_bulwark_bonus = None;
            h.speed_loader = None;
            if let Some(k) = h.hero_kit.as_deref_mut() {
                k.talent = None;
            }
        }
        if slot == BlitzTalentSlot::NoTalent {
            return Ok(());
        }
        let mut applied_real_talent = false;
        if let Some(d) = data
            && let Some(nz) = std::num::NonZeroU8::new(level)
        {
            let type_id = self.entities[i].type_id;
            if let Some(name) = Self::hero_talent_id(type_id, slot as u8, Some(d)) {
                applied_real_talent = self.set_named_talent(actor, Some((name, nz)), d).is_ok();
            }
        }
        if applied_real_talent {
            return Ok(());
        }
        let lvl = f32::from(level);
        let applied = match slot {
            BlitzTalentSlot::Slot0 => BlitzTalentApplied {
                slot,
                level,
                weapon_bonus: 3.0 * lvl,
                crystal_bonus: 2.0 * lvl,
                hp_bonus: 15.0 * lvl,
                armor_bonus: 0.0,
                shield_bonus: 0.0,
            },
            BlitzTalentSlot::Slot1 => BlitzTalentApplied {
                slot,
                level,
                weapon_bonus: 1.5 * lvl,
                crystal_bonus: 3.5 * lvl,
                hp_bonus: 25.0 * lvl,
                armor_bonus: 1.5 * lvl,
                shield_bonus: 1.5 * lvl,
            },
            BlitzTalentSlot::Slot2 => BlitzTalentApplied {
                slot,
                level,
                weapon_bonus: 2.0 * lvl,
                crystal_bonus: 5.0 * lvl,
                hp_bonus: 20.0 * lvl,
                armor_bonus: 1.0 * lvl,
                shield_bonus: 1.0 * lvl,
            },
            BlitzTalentSlot::NoTalent => unreachable!(),
        };
        let h = &mut self.entities[i];
        h.stats.weapon += applied.weapon_bonus;
        h.item_stats.crystal += applied.crystal_bonus;
        h.stats.armor += applied.armor_bonus;
        h.stats.shield += applied.shield_bonus;
        h.bonus_hp += applied.hp_bonus;
        h.max_hp += applied.hp_bonus;
        if h.hp > 0.0 {
            h.hp = (h.hp + applied.hp_bonus).min(h.max_hp);
        }
        if let Some(blitz) = &mut self.state.blitz {
            blitz.talents[seat] = Some(applied);
        }
        Ok(())
    }

    pub(super) fn advance_blitz(&mut self, events: &mut Vec<Event>) {
        if !matches!(self.layout, Layout::Blitz { .. }) {
            return;
        }
        self.advance_blitz_timer(events);
        if self.state.winner.is_some() {
            return;
        }
        self.spawn_blitz_wave(events);
        self.advance_blitz_objectives(events);
    }

    fn advance_blitz_timer(&mut self, events: &mut Vec<Event>) {
        let Some(blitz) = &mut self.state.blitz else {
            return;
        };
        if blitz.remaining_ticks == 0 {
            return;
        }
        blitz.remaining_ticks -= 1;
        if blitz.remaining_ticks == 0 {
            let [a, b] = blitz.team_points;
            if a != b {
                let winner = u8::from(b > a);
                self.state.winner = Some(winner);
                events.push(Event::End { winner });
            } else if !blitz.sudden_death {
                blitz.sudden_death = true;
                events.push(Event::BlitzSuddenDeath);
            }
        }
    }

    fn spawn_blitz_wave(&mut self, events: &mut Vec<Event>) {
        let Some(elapsed) = self.tick.checked_sub(BLITZ_WAVE_START) else {
            return;
        };
        let offset = elapsed % BLITZ_WAVE_INTERVAL;
        if !offset.is_multiple_of(TICK_HZ) {
            return;
        }
        // Blitz_3v3_Revamp_MinionSettings: Tank x3, Lead, Ranged.
        let Some(&slot) = [2, 2, 2, 1, 0].get((offset / TICK_HZ) as usize) else {
            return;
        };
        let Some(blitz) = self.state.blitz.as_ref() else {
            return;
        };
        let stats = blitz.units.rows[slot];
        for team in 0..2 {
            self.spawn_lane_minion(team, stats, self.layout.wave(team, 0), events);
        }
    }

    fn advance_blitz_objectives(&mut self, events: &mut Vec<Event>) {
        let tick = self.state.tick;
        let Some(mut blitz) = self.state.blitz.take() else {
            return;
        };
        for obj in &mut blitz.objectives {
            if let Some(id) = obj.entity_id {
                self.leash_neutral_unit(id, obj.anchor, &mut obj.last_combat_tick);
                continue;
            }
            if let Some(due) = obj.respawn_tick
                && tick >= due
            {
                obj.respawn_tick = None;
                let base = match obj.kind {
                    BlitzObjectiveKind::MiddleSentry => blitz.units.rows[9],
                    BlitzObjectiveKind::AllySentryLeft | BlitzObjectiveKind::AllySentryRight => {
                        blitz.units.rows[10]
                    }
                    BlitzObjectiveKind::GoldMiner => blitz.units.rows[11],
                };
                let level = (1 + tick / (60 * TICK_HZ)).min(12) as u8;
                let extra = f32::from(level - 1);
                let mut stats = base;
                stats.hp += stats.growth.hp * extra;
                stats.weapon += stats.growth.weapon * extra;
                let id = self.spawn_with_stats(obj.team, Kind::Neutral, stats, obj.anchor, false);
                self.entities.last_mut().unwrap().level = level;
                obj.entity_id = Some(id);
                obj.last_combat_tick = tick;
                events.push(Event::Spawn { id });
            }
        }
        self.state.blitz = Some(blitz);
    }

    pub(super) fn award_blitz_points(
        &mut self,
        team: u8,
        points: u8,
        source: u32,
        target: u32,
        events: &mut Vec<Event>,
    ) {
        if team >= 2 || points == 0 {
            return;
        }
        let Some(blitz) = &mut self.state.blitz else {
            return;
        };
        blitz.team_points[usize::from(team)] += u32::from(points);
        let total = blitz.team_points[usize::from(team)];
        let target_points = blitz.target_points;
        let sudden_death = blitz.sudden_death;
        let [a, b] = blitz.team_points;
        events.push(Event::BlitzPoints {
            team,
            points,
            source,
            target,
            total,
        });
        if (total >= target_points || (sudden_death && a != b)) && self.state.winner.is_none() {
            self.state.winner = Some(team);
            events.push(Event::End { winner: team });
        }
    }

    /// Applies Blitz point scoring and objective respawn scheduling upon entity lethal defeat.
    pub(super) fn on_blitz_lethal(
        &mut self,
        victim_idx: usize,
        killer_id: u32,
        killer_team: u8,
        events: &mut Vec<Event>,
    ) -> bool {
        if !matches!(self.layout, Layout::Blitz { .. }) {
            return false;
        }
        let v = &self.entities[victim_idx];
        let v_id = v.id;
        let v_kind = v.kind;
        let v_team = v.team;
        let bounty = v
            .stats
            .physiology
            .map_or((0, 0.0), |p| (p.gold_bounty as u32, p.xp_bounty));
        if v_kind == Kind::Hero && killer_team < 2 && killer_team != v_team {
            self.award_blitz_points(killer_team, 1, killer_id, v_id, events);
            return false;
        }
        if v_kind == Kind::Tower && killer_team < 2 && killer_team != v_team {
            self.award_blitz_points(killer_team, 3, killer_id, v_id, events);
            return false;
        }
        let Some(mut blitz) = self.state.blitz.take() else {
            return false;
        };
        let mut matched_obj = false;
        for obj in &mut blitz.objectives {
            if obj.entity_id == Some(v_id) {
                matched_obj = true;
                obj.entity_id = None;
                obj.respawn_tick = Some(self.state.tick + BLITZ_OBJECTIVE_RESPAWN_TICKS);
                break;
            }
        }
        self.state.blitz = Some(blitz);
        if !matched_obj {
            return false;
        }
        if killer_team < 2 && killer_team != v_team {
            // Award +3 points and objective bounty to killer team heroes.
            if bounty.0 > 0 {
                let per_hero = bounty.0 / 3;
                for seat in 0..self.entities.len().min(6) {
                    if self.entities[seat].kind == Kind::Hero
                        && self.entities[seat].team == killer_team
                    {
                        self.entities[seat].gold += per_hero;
                        let hid = self.entities[seat].id;
                        self.state.scores[(hid - 1) as usize]
                            .gold_income
                            .lane_proximity_awards += u64::from(per_hero);
                        events.push(Event::GoldAward {
                            id: hid,
                            amount: per_hero,
                        });
                    }
                }
            }
            self.award_blitz_points(killer_team, 3, killer_id, v_id, events);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blitz_world() -> World {
        let mut w = World::new(21, 6);
        w.use_six_human_roster().unwrap();
        w.assign_teams(&[0, 1, 0, 1, 0, 1]).unwrap();
        w.use_blitz().unwrap();
        w
    }

    #[test]
    fn blitz_starts_at_level_9_with_6000_gold_sentries_and_5_minute_timer() {
        let mut w = blitz_world();
        w.use_blitz().unwrap();
        assert_eq!(
            w.entities.iter().filter(|e| e.kind == Kind::Hero).count(),
            6
        );
        for h in w.entities.iter().take(6) {
            assert_eq!(h.level, BLITZ_START_LEVEL);
            assert_eq!(h.gold, BLITZ_START_GOLD);
            assert_eq!(h.skill_points, BLITZ_START_LEVEL);
        }
        let b = w.blitz_state().unwrap();
        assert_eq!(b.remaining_ticks, BLITZ_DURATION_TICKS);
        assert_eq!(b.team_points, [0, 0]);
        assert!(!b.sudden_death);
        assert!(b.objectives.iter().all(|o| o.entity_id.is_some()));
        assert_eq!(w.entities.len(), 22);
    }

    #[test]
    fn blitz_points_hero_1_turret_and_objectives_3_reach_15_to_win() {
        let mut w = blitz_world();
        let mut ev = Vec::new();
        // 3 hero kills on team 1 (+3 pts for team 0).
        for hero_idx in [1, 3, 5] {
            w.entities[hero_idx].hp = 1.0;
            w.hurt(1, 0, hero_idx, 50.0, DamageSchool::True, &mut ev);
        }
        assert_eq!(w.blitz_state().unwrap().team_points, [3, 0]);
        assert!(w.winner.is_none());

        // Fast respawn timer check (much shorter than 3v3/5v5 late timer).
        let respawn = w.respawn_seconds(w.entities[1].id).unwrap();
        assert!((respawn - 10.0).abs() <= 0.1);

        // Destroy 2 enemy turrets (+6 -> 9 pts) and Middle Sentry (+3 -> 12 pts) and Gold Miner (+3 -> 15 pts -> Win!).
        let enemy_towers: Vec<u32> = w
            .entities
            .iter()
            .filter(|e| e.kind == Kind::Tower && e.team == 1)
            .take(2)
            .map(|e| e.id)
            .collect();
        for tid in enemy_towers {
            let idx = w.index(tid).unwrap();
            w.hurt(1, 0, idx, 10_000.0, DamageSchool::True, &mut ev);
        }
        assert_eq!(w.blitz_state().unwrap().team_points, [9, 0]);

        let mid_sentry = w.blitz_state().unwrap().objectives[0].entity_id.unwrap();
        let m_idx = w.index(mid_sentry).unwrap();
        w.hurt(1, 0, m_idx, 10_000.0, DamageSchool::True, &mut ev);
        assert_eq!(w.blitz_state().unwrap().team_points, [12, 0]);
        assert!(w.winner.is_none());

        let gold_miner = w.blitz_state().unwrap().objectives[3].entity_id.unwrap();
        let g_idx = w.index(gold_miner).unwrap();
        w.hurt(1, 0, g_idx, 10_000.0, DamageSchool::True, &mut ev);
        assert_eq!(w.blitz_state().unwrap().team_points, [15, 0]);
        assert_eq!(w.winner, Some(0));
        assert!(ev.contains(&Event::End { winner: 0 }));
    }

    #[test]
    fn blitz_timeout_higher_score_wins_and_tie_enters_sudden_death() {
        // Case 1: Higher score at 5:00 wins immediately.
        let mut w = blitz_world();
        let mut ev = Vec::new();
        w.entities[1].hp = 1.0;
        w.hurt(1, 0, 1, 100.0, DamageSchool::True, &mut ev);
        assert_eq!(w.blitz_state().unwrap().team_points, [1, 0]);
        w.state.blitz.as_mut().unwrap().remaining_ticks = 1;
        let events = w.step();
        assert_eq!(w.winner, Some(0));
        assert!(events.contains(&Event::End { winner: 0 }));

        // Case 2: Tied at 5:00 triggers sudden death, then next point wins.
        let mut w2 = blitz_world();
        w2.state.blitz.as_mut().unwrap().remaining_ticks = 1;
        let sd_events = w2.step();
        assert!(w2.winner.is_none());
        assert!(w2.blitz_state().unwrap().sudden_death);
        assert!(sd_events.contains(&Event::BlitzSuddenDeath));

        w2.entities[0].hp = 1.0;
        let mut kill_ev = Vec::new();
        w2.hurt(2, 1, 0, 100.0, DamageSchool::True, &mut kill_ev);
        assert_eq!(w2.winner, Some(1));
        assert!(kill_ev.contains(&Event::End { winner: 1 }));
    }

    #[test]
    fn blitz_talent_equip_scales_with_level_and_reverts_cleanly_deterministically() {
        let mut a = blitz_world();
        let mut b = blitz_world();
        let base_weapon = a.entities[0].weapon_power();
        let base_hp = a.entities[0].max_hp;

        a.equip_blitz_talent(1, BlitzTalentSlot::Slot0, 10, None)
            .unwrap();
        b.equip_blitz_talent(1, BlitzTalentSlot::Slot0, 10, None)
            .unwrap();
        assert!(a.entities[0].weapon_power() > base_weapon);
        assert!(a.entities[0].max_hp > base_hp);
        assert_eq!(a.state_digest(), b.state_digest());

        // Switching to Slot1 and unequipping reverts exact stats without leaking modifiers.
        a.equip_blitz_talent(1, BlitzTalentSlot::Slot1, 5, None)
            .unwrap();
        assert!((a.entities[0].weapon_power() - (base_weapon + 7.5)).abs() < 0.001);
        a.equip_blitz_talent(1, BlitzTalentSlot::NoTalent, 0, None)
            .unwrap();
        assert!((a.entities[0].weapon_power() - base_weapon).abs() < 0.001);
        assert!((a.entities[0].max_hp - base_hp).abs() < 0.001);
    }

    #[test]
    fn blitz_minions_and_bots_march_past_turrets_without_stalling() {
        let mut w = blitz_world();
        w.set_modern_ai(true);
        for idx in 0..w.entities.len() {
            if w.entities[idx].kind == Kind::Hero {
                w.entities[idx].bot = true;
            }
        }
        // Step past BLITZ_WAVE_START (10s) + 30s of lane march.
        for _ in 0..(40 * TICK_HZ) {
            w.step();
        }
        let max_t0_minion_x = w
            .entities
            .iter()
            .filter(|e| e.kind == Kind::Minion && e.team == 0)
            .map(|e| e.position.x)
            .fold(f32::NEG_INFINITY, f32::max);
        let min_t1_minion_x = w
            .entities
            .iter()
            .filter(|e| e.kind == Kind::Minion && e.team == 1)
            .map(|e| e.position.x)
            .fold(f32::INFINITY, f32::min);
        assert!(
            max_t0_minion_x > -8.0,
            "Team 0 Blitz minions stalled before mid: max_x={max_t0_minion_x}"
        );
        assert!(
            min_t1_minion_x < 8.0,
            "Team 1 Blitz minions stalled before mid: min_x={min_t1_minion_x}"
        );

        let team0_primary = w
            .entities
            .iter()
            .find(|e| e.kind == Kind::Hero && e.team == 0)
            .unwrap();
        let team1_primary = w
            .entities
            .iter()
            .find(|e| e.kind == Kind::Hero && e.team == 1)
            .unwrap();
        assert!(
            team0_primary.position.x > -25.0 && team0_primary.position.y < 18.0,
            "Team 0 primary laner must push lane instead of stalling/jungling: {:?}",
            team0_primary.position
        );
        assert!(
            team1_primary.position.x < 25.0 && team1_primary.position.y < 18.0,
            "Team 1 primary laner must push lane instead of stalling/jungling: {:?}",
            team1_primary.position
        );
    }

    #[test]
    #[ignore = "requires local delivery and A001/server_data_extra map assets"]
    fn blitz_loads_a001_navmesh_blitz_store_sentries_and_modeled_talents() {
        let base = std::env::var_os("HALCYON_DELIVERY").unwrap();
        let extra = std::path::PathBuf::from(std::env::var_os("HALCYON_DATA_EXTRA").unwrap());
        let data = GameData::load_with_extra(base, Some(&extra)).unwrap();
        let mut w = World::from_data(9, 6, &data).unwrap();
        w.use_six_human_roster().unwrap();
        w.assign_teams(&[0, 1, 0, 1, 0, 1]).unwrap();
        w.select_hero(0, "Ringo", &data).unwrap();
        w.select_hero(1, "Lyra", &data).unwrap();
        w.select_hero(2, "Baron", &data).unwrap();
        w.use_blitz().unwrap();
        w.use_catalog_store(halcyon_data::items::StoreSettings::BlitzRevamp);
        if let Some(entries) =
            data.store_for_settings(halcyon_data::items::StoreSettings::BlitzRevamp.key())
        {
            w.use_item_store(entries);
        }
        w.install_map_data(&data).unwrap();
        assert!(matches!(w.layout, Layout::Blitz { .. }));
        assert!(w.native_navigation.is_some());
        assert_eq!(w.entities[0].level, BLITZ_START_LEVEL);
        assert_eq!(w.entities[0].gold, BLITZ_START_GOLD);

        // Infusions are disallowed in ItemStore_Blitz_3v3_Revamp.
        assert_eq!(
            w.purchase(1, BasicItem::WeaponInfusion),
            Err("item unavailable")
        );
        // Sorrowblade succeeds with 6,000 starting Blitz gold.
        w.purchase(1, BasicItem::Sorrowblade).unwrap();

        // Equip Ringo LongShot (Slot0, Lv5) + Lyra MobileBulwark (Slot1, Lv10) + Baron Epic (Slot1, Lv10).
        let baron_wp_before = w.entities[2].weapon_power();
        w.equip_blitz_talent(1, BlitzTalentSlot::Slot0, 5, Some(&data))
            .unwrap();
        assert!(w.entities[0].long_shot.is_some());
        w.equip_blitz_talent(2, BlitzTalentSlot::Slot1, 10, Some(&data))
            .unwrap();
        assert!(w.entities[1].mobile_bulwark_bonus.is_some());
        w.equip_blitz_talent(3, BlitzTalentSlot::Slot1, 10, Some(&data))
            .unwrap();
        assert_eq!(
            w.entities[2]
                .hero_kit
                .as_deref()
                .and_then(|k| k.talent.as_ref())
                .map(|t| (t.id.as_str(), t.level.get())),
            Some(("Talent_Baron_EndlessMortars", 10))
        );
        assert!(
            (w.entities[2].weapon_power() - baron_wp_before).abs() < 0.001,
            "real Baron talent must not add synthetic flat weapon bonuses"
        );

        // Verify real A001 navmesh lane marching for minions and bots without turret stalls.
        w.set_modern_ai(true);
        for idx in 0..w.entities.len() {
            if w.entities[idx].kind == Kind::Hero {
                w.entities[idx].bot = true;
            }
        }
        for _ in 0..(40 * TICK_HZ) {
            w.step();
        }
        let max_t0_minion_x = w
            .entities
            .iter()
            .filter(|e| e.kind == Kind::Minion && e.team == 0)
            .map(|e| e.position.x)
            .fold(f32::NEG_INFINITY, f32::max);
        let min_t1_minion_x = w
            .entities
            .iter()
            .filter(|e| e.kind == Kind::Minion && e.team == 1)
            .map(|e| e.position.x)
            .fold(f32::INFINITY, f32::min);
        assert!(
            max_t0_minion_x > -10.0,
            "A001 navmesh Team 0 Blitz minions stalled: max_x={max_t0_minion_x}"
        );
        assert!(
            min_t1_minion_x < 10.0,
            "A001 navmesh Team 1 Blitz minions stalled: min_x={min_t1_minion_x}"
        );
    }
}
