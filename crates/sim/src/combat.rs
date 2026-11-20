//! A small authored ability set. Official 4.13 deltas supply the named rank
//! tables; windups, radii, energy and complete effects are validation rules.
use super::*;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AbilitySlot {
    A,
    B,
    C,
}
impl AbilitySlot {
    fn idx(self) -> usize {
        self as usize
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum CastTarget {
    SelfCast,
    Entity(u32),
    Point(Position),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Skill {
    Achilles,
    Twirl,
    Hellfire,
    Gift,
    Agent,
    Sigil,
    Suppress,
    Wind,
    Strike,
    Chakram,
    FishFood,
    Splashdown,
    Waterwall,
}
fn skill(hero: u16, slot: AbilitySlot) -> Option<Skill> {
    use AbilitySlot::*;
    use Skill::*;
    Some(match (EntityTemplate::from_raw(hero)?, slot) {
        (EntityTemplate::Idris, B) => Chakram,
        (EntityTemplate::Lorelai, A) => FishFood,
        (EntityTemplate::Lorelai, B) => Splashdown,
        (EntityTemplate::Lorelai, C) => Waterwall,
        (EntityTemplate::Ringo, A) => Achilles,
        (EntityTemplate::Ringo, B) => Twirl,
        (EntityTemplate::Ringo, C) => Hellfire,
        (EntityTemplate::Adagio, A) => Gift,
        (EntityTemplate::Adagio, B) => Agent,
        (EntityTemplate::Lyra, A) => Sigil,
        (EntityTemplate::Saw, B) => Suppress,
        (EntityTemplate::Karas, B) => Wind,
        (EntityTemplate::Shin, C) => Strike,
        _ => return None,
    })
}
#[derive(Debug, Clone, Copy, Serialize)]
pub(super) struct Cast {
    slot: AbilitySlot,
    target: CastTarget,
    release: u64,
}
#[derive(Clone, Copy, Default, Debug, Serialize)]
pub(super) struct Status {
    pub(super) double_down: Option<u32>,
    pub(super) stamina_spent: f32,
    pub(super) stamina_spent_at: u64,
    pub(super) stun_until: u64,
    pub(super) water_speed_until: u64,
    pub(super) water_slow_until: u64,
    pub(super) water_slow_factor: f32,
    pub(super) barrier: f32,
    pub(super) barrier_until: u64,
    pub slow_until: u64,
    pub slow_factor: f32,
    pub haste_until: u64,
    pub empowered: u8,
    pub empower_damage: f32,
    pub(super) channel: u64,
    pub(super) channel_until: u64,
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct AbilityBook {
    gift_cd: [f32; 5],
    gift_hp: f32,
    empower: [f32; 5],
    sigil_heal: [f32; 5],
    sigil_cd: [f32; 5],
    sigil_hp: f32,
    suppress_dps: [f32; 5],
    suppress_cd: [f32; 5],
    wind_dmg: [f32; 5],
    strike_dmg: f32,
    strike_hits: u8,
    chakram_weapon_ratio: f32,
    pub(super) karas_basic_crystal_ratio: f32,
}
impl Default for AbilityBook {
    fn default() -> Self {
        Self {
            gift_cd: [12., 11., 10., 9., 8.],
            gift_hp: 0.09,
            empower: [5., 5., 5., 5., 7.],
            sigil_heal: [25., 35., 45., 55., 75.],
            sigil_cd: [14., 13., 12., 11., 10.],
            sigil_hp: 0.15,
            suppress_dps: [140., 175., 210., 245., 315.],
            suppress_cd: [15., 15., 15., 15., 13.],
            wind_dmg: [100., 120., 140., 160., 200.],
            strike_dmg: 200.,
            strike_hits: 3,
            chakram_weapon_ratio: 0.8,
            karas_basic_crystal_ratio: 0.6,
        }
    }
}
impl AbilityBook {
    pub(super) fn from_data(data: &GameData) -> Result<Self, String> {
        let changes = data.official_rules["patch_413_ability_deltas"]["changes"]
            .as_array()
            .ok_or("missing patch 4.13 changes")?;
        let field = |hero: &str, key: &str| {
            changes
                .iter()
                .find(|v| v["hero"] == hero && v.get(key).is_some())
                .and_then(|v| v.get(key))
                .ok_or_else(|| format!("missing 4.13 field {hero}.{key}"))
        };
        let five = |hero: &str, key: &str| -> Result<[f32; 5], String> {
            serde_json::from_value(field(hero, key)?.clone())
                .map_err(|e| format!("{hero}.{key}: {e}"))
        };
        let num = |hero: &str, key: &str| {
            field(hero, key)?
                .as_f64()
                .map(|v| v as f32)
                .ok_or_else(|| format!("invalid {hero}.{key}"))
        };
        Ok(Self {
            chakram_weapon_ratio: num("Idris", "B_weapon_ratio")?,
            karas_basic_crystal_ratio: num("Karas", "basic_crystal_ratio").unwrap_or(0.6),
            gift_cd: five("Adagio", "cooldown_s")?,
            gift_hp: num("Adagio", "burst_heal_health_ratio")?,
            empower: five("Adagio", "empowered_attacks")?,
            sigil_heal: five("Lyra", "A_heal_per_second")?,
            sigil_cd: five("Lyra", "A_cooldown_s")?,
            sigil_hp: num("Lyra", "A_hot_health_ratio")?,
            suppress_dps: five("SAW", "B_dps")?,
            suppress_cd: five("SAW", "B_cooldown_s")?,
            wind_dmg: five("Karas", "B_damage")?,
            strike_dmg: num("Shin", "C_damage_per_hit")?,
            strike_hits: field("Shin", "C_hits")?
                .as_u64()
                .and_then(|n| n.try_into().ok())
                .ok_or("invalid Shin.C_hits")?,
        })
    }
}
#[derive(Clone, Copy, Debug)]
struct Rules {
    range: f32,
    cost: f32,
    windup: u64,
    cooldown: u64,
}
impl World {
    fn rules(&self, s: Skill, rank: usize) -> Rules {
        let (range, cost, windup, seconds) = match s {
            Skill::Chakram => (8.5, 10., 4, 8.),
            Skill::FishFood => (
                if rank == 4 { 9.5 } else { 8. },
                60. + 10. * rank as f32,
                1,
                12. - 0.5 * rank as f32,
            ),
            Skill::Splashdown => (
                10.5,
                [50., 60., 70., 80., 40.][rank],
                1,
                8. - 0.5 * rank as f32,
            ),
            Skill::Waterwall => (10., 100. + 25. * rank as f32, 1, 90. - 15. * rank as f32),
            Skill::Achilles => (7., 20., 4, 6.),
            Skill::Twirl => (0., 18., 1, 9.),
            Skill::Hellfire => (12., 45., 22, 28.),
            Skill::Gift => (7., 26., 4, self.abilities.gift_cd[rank]),
            Skill::Agent => (7., 24., 3, 10.),
            Skill::Sigil => (8., 30., 5, self.abilities.sigil_cd[rank]),
            Skill::Suppress => (8., 30., 6, self.abilities.suppress_cd[rank]),
            Skill::Wind => (7., 24., 5, 8.),
            Skill::Strike => (7., 36., 7, 20.),
        };
        Rules {
            range,
            cost,
            windup,
            cooldown: (seconds * TICK_HZ as f32) as u64,
        }
    }
    pub(super) fn can_learn(&self, i: usize, slot: AbilitySlot) -> bool {
        let e = &self.entities[i];
        let rank = e.ability_ranks[slot.idx()];
        let needed = if slot == AbilitySlot::C {
            6 + 3 * rank
        } else {
            1 + 2 * rank
        };
        e.skill_points > 0
            && (skill(e.type_id, slot).is_some()
                || (!e.bot && native_learn_operand(e.type_id, slot).is_some()))
            && rank < if slot == AbilitySlot::C { 3 } else { 5 }
            && e.level >= needed
    }
    pub(super) fn learn(&mut self, i: usize, slot: AbilitySlot) -> Result<(), &'static str> {
        if !self.can_learn(i, slot) {
            return Err("rank unavailable");
        }
        self.entities[i].skill_points -= 1;
        self.entities[i].ability_ranks[slot.idx()] += 1;
        Ok(())
    }
    pub(super) fn can_cast(&self, i: usize, slot: AbilitySlot, target: CastTarget) -> bool {
        self.castable(i, slot, target).is_ok()
    }
    fn castable(
        &self,
        i: usize,
        slot: AbilitySlot,
        target: CastTarget,
    ) -> Result<(Skill, usize, Rules), &'static str> {
        let e = &self.entities[i];
        let s = skill(e.type_id, slot).ok_or("ability unavailable")?;
        let rank = e.ability_ranks[slot.idx()]
            .checked_sub(1)
            .ok_or("ability not learned")? as usize;
        let r = self.rules(s, rank);
        let resource = if s == Skill::Chakram {
            chakram::stamina(e, self.tick)
        } else {
            e.energy
        };
        if e.cooldown[slot.idx()] > self.tick || resource < r.cost {
            return Err("ability not ready");
        }
        let point = match (s, target) {
            (Skill::Twirl | Skill::Gift | Skill::Agent, CastTarget::SelfCast) => e.position,
            (
                Skill::Achilles | Skill::Hellfire | Skill::Gift | Skill::Agent | Skill::Waterwall,
                CastTarget::Entity(id),
            ) => {
                let t = &self.entities[self.index(id).ok_or("unknown target")?];
                let ally = matches!(s, Skill::Gift | Skill::Agent | Skill::Waterwall);
                if t.hp <= 0.
                    || (s == Skill::Waterwall && t.kind != Kind::Hero)
                    || (t.team == e.team) != ally
                    || !matches!(t.kind, Kind::Hero | Kind::Minion)
                    || !self.visible(e.team, t)
                {
                    return Err("invalid cast target");
                }
                t.position
            }
            (
                Skill::Sigil
                | Skill::Suppress
                | Skill::Wind
                | Skill::Strike
                | Skill::Chakram
                | Skill::FishFood
                | Skill::Splashdown,
                CastTarget::Point(p),
            ) if self.layout.walkable(p) => p,
            _ => return Err("invalid cast target"),
        };
        if (s == Skill::Chakram && e.position.dist(point) < 0.001)
            || point.dist(e.position) > r.range
            || !self.layout.clear(e.position, point)
        {
            return Err("cast out of range");
        }
        Ok((s, rank, r))
    }
    pub(super) fn begin_cast(
        &mut self,
        i: usize,
        slot: AbilitySlot,
        target: CastTarget,
    ) -> Result<(), &'static str> {
        if self.entities[i].cast.is_some() {
            return Err("cast in progress");
        }
        let (_, _, rules) = self.castable(i, slot, target)?;
        let e = &mut self.entities[i];
        e.winding = None;
        e.order = Command::Stop;
        e.status.channel = 0;
        e.status.channel_until = 0;
        e.cast = Some(Cast {
            slot,
            target,
            release: self.tick + rules.windup,
        });
        Ok(())
    }
    pub(super) fn advance_cast(&mut self, i: usize, events: &mut Vec<Event>) {
        let e = &self.entities[i];
        let c = e.cast.unwrap();
        if self.tick < c.release {
            return;
        }
        self.entities[i].cast = None;
        let Ok((s, r, rule)) = self.castable(i, c.slot, c.target) else {
            return;
        };
        let e = &mut self.entities[i];
        if s == Skill::Chakram {
            e.status.stamina_spent = 100. - chakram::stamina(e, self.tick) + rule.cost;
            e.status.stamina_spent_at = self.tick;
        } else {
            e.energy -= rule.cost;
        }
        e.cooldown[c.slot.idx()] =
            self.tick + (rule.cooldown as f32 / (1. + e.item_stats.cooldown)).ceil() as u64;
        let src = e.id;
        let team = e.team;
        let origin = e.position;
        let bonus_hp = e.bonus_hp;
        let weapon = e.stats.weapon;
        events.push(Event::Cast {
            source: src,
            slot: c.slot,
            target: c.target,
        });
        let id = match c.target {
            CastTarget::SelfCast => src,
            CastTarget::Entity(t) => t,
            CastTarget::Point(_) => 0,
        };
        let aimed = self.index(id);
        let point = match c.target {
            CastTarget::Point(p) => p,
            _ => origin,
        };
        match s {
            Skill::FishFood | Skill::Splashdown | Skill::Waterwall => {
                self.cast_water(i, c.slot, c.target, r, events);
            }
            Skill::Chakram => {
                self.chakrams.push(chakram::Chakram::new(
                    src,
                    team,
                    origin,
                    point,
                    weapon * self.abilities.chakram_weapon_ratio,
                ));
                if matches!(self.layout, Layout::Aral { .. }) {
                    let id = self.next_id;
                    self.next_id += 1;
                    events.push(Event::ChakramApplied { source: src, id });
                }
            }
            Skill::Achilles => {
                let j = aimed.unwrap();
                self.entities[j].status.slow_until = self.tick + 30;
                self.entities[j].status.slow_factor = 0.7;
                let dmg = shop::mitigated(
                    65. + r as f32 * 30.,
                    self.entities[j].stats.shield,
                    self.entities[i].item_stats.shield_pierce,
                );
                self.hurt(src, team, j, dmg, DamageSchool::Crystal, events);
            }
            Skill::Twirl => {
                let e = &mut self.entities[i];
                e.status.haste_until = self.tick + 60;
                e.next_attack = self.tick;
            }
            Skill::Hellfire => {
                let j = aimed.unwrap();
                let travel =
                    (origin.dist(self.entities[j].position) * TICK_HZ as f32 / 12.).ceil() as u64;
                let damage = 140. + r as f32 * 45.;
                self.effects.push(SpellEffect::impact(
                    self.tick + travel,
                    src,
                    team,
                    id,
                    damage,
                    DamageSchool::True,
                ));
                for n in 1..=3 {
                    self.effects.push(SpellEffect::impact(
                        self.tick + travel + n * TICK_HZ,
                        src,
                        team,
                        id,
                        damage * 0.16,
                        DamageSchool::True,
                    ));
                }
            }
            Skill::Gift => {
                let j = aimed.unwrap();
                self.heal(
                    src,
                    j,
                    45. + 25. * r as f32 + bonus_hp * self.abilities.gift_hp,
                    events,
                );
            }
            Skill::Agent => {
                let t = &mut self.entities[aimed.unwrap()];
                t.status.empowered = self.abilities.empower[r] as u8;
                t.status.empower_damage = 25. + r as f32 * 10.;
            }
            Skill::Sigil => {
                let bonus = bonus_hp;
                let heal = self.abilities.sigil_heal[r] + bonus * self.abilities.sigil_hp;
                for sec in 0..3 {
                    self.effects.push(SpellEffect {
                        due: self.tick + 1 + sec * TICK_HZ,
                        src,
                        team,
                        area: Area::Circle(point, 2.5),
                        damage: 40. + 15. * r as f32,
                        heal,
                        channel: 0,
                    });
                }
            }
            Skill::Suppress | Skill::Strike => {
                let token = self.tick;
                self.entities[i].status.channel = token;
                let (hits, pulse, damage) = if s == Skill::Strike {
                    (self.abilities.strike_hits, 5, self.abilities.strike_dmg)
                } else {
                    (4, 10, self.abilities.suppress_dps[r] * 0.5)
                };
                self.entities[i].status.channel_until = self.tick + 1 + u64::from(hits - 1) * pulse;
                for n in 0..hits {
                    self.effects.push(SpellEffect {
                        due: self.tick + 1 + u64::from(n) * pulse,
                        src,
                        team,
                        area: Area::Line(origin, point, 1.4),
                        damage,
                        heal: 0.,
                        channel: token,
                    });
                }
            }
            Skill::Wind => {
                self.effects.push(SpellEffect {
                    due: self.tick + 1,
                    src,
                    team,
                    area: Area::Circle(point, 2.),
                    damage: self.abilities.wind_dmg[r],
                    heal: 0.,
                    channel: 0,
                });
            }
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize)]
enum Area {
    Target(u32, DamageSchool),
    NpcWindup(u32),
    Circle(Position, f32),
    Line(Position, Position, f32),
}
#[derive(Clone, Copy, Debug, Serialize)]
pub(super) struct SpellEffect {
    due: u64,
    src: u32,
    team: u8,
    area: Area,
    damage: f32,
    heal: f32,
    channel: u64,
}
impl SpellEffect {
    pub(super) fn npc_windup(due: u64, src: u32, team: u8, target: u32, damage: f32) -> Self {
        Self {
            due,
            src,
            team,
            area: Area::NpcWindup(target),
            damage,
            heal: 0.,
            channel: 0,
        }
    }

    pub fn impact(
        due: u64,
        src: u32,
        team: u8,
        id: u32,
        damage: f32,
        school: DamageSchool,
    ) -> Self {
        Self {
            due,
            src,
            team,
            area: Area::Target(id, school),
            damage,
            heal: 0.,
            channel: 0,
        }
    }
}
/// Median launch-to-impact delays by two-unit separation band, quantized to
/// 20 Hz. The sparse outer bands reuse the last populated band.
fn npc_flight_ticks(template: u16, victim: u16, distance: f32) -> u64 {
    if template == EntityTemplate::OuterTurret5v5.as_raw() {
        return match EntityTemplate::from_raw(victim) {
            Some(EntityTemplate::MinionRange5v5 | EntityTemplate::MinionMelee5v5) => 7,
            Some(EntityTemplate::MinionSiege5v5) => 5,
            _ => unreachable!("only measured Solo minion targets schedule a windup"),
        };
    }
    let bins = match EntityTemplate::from_raw(template) {
        Some(EntityTemplate::RangedMinion) => [2, 6, 12, 14, 14],
        Some(EntityTemplate::LeadMinion) => [2, 6, 10, 10, 10],
        Some(EntityTemplate::Turret | EntityTemplate::VainTurret) => [4, 4, 6, 10, 14],
        _ => unreachable!("only measured ARAL templates schedule a windup"),
    };
    bins[((distance / 2.) as usize).min(bins.len() - 1)]
}

pub(super) fn line_distance(p: Position, a: Position, b: Position) -> f32 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let denom = dx * dx + dy * dy;
    if denom == 0. {
        return p.dist(a);
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / denom).clamp(0., 1.);
    p.dist(Position {
        x: a.x + dx * t,
        y: a.y + dy * t,
    })
}
impl World {
    pub(super) fn predicted_hp(&self, e: &Entity, at: u64) -> f32 {
        let incoming: f32 = self
            .effects
            .iter()
            .filter(|v| v.due <= at && matches!(v.area,Area::Target(id, _) if id==e.id))
            .map(|v| v.damage)
            .sum();
        (e.hp - incoming).max(0.)
    }
    pub(super) fn npc_launch_delay(&self, attacker: &Entity, victim: &Entity) -> Option<u64> {
        if matches!(self.layout, Layout::SoloMid { .. }) {
            return (attacker.kind == Kind::Tower
                && attacker.type_id == EntityTemplate::OuterTurret5v5.as_raw()
                && matches!(attacker.id, 510 | 512 | 514)
                && victim.kind == Kind::Minion
                && matches!(
                    EntityTemplate::from_raw(victim.type_id),
                    Some(
                        EntityTemplate::MinionRange5v5
                            | EntityTemplate::MinionMelee5v5
                            | EntityTemplate::MinionSiege5v5
                    )
                ))
            .then_some(2);
        }
        if !matches!(self.layout, Layout::Aral { .. }) {
            return None;
        }
        match (attacker.kind, EntityTemplate::from_raw(attacker.type_id)) {
            (Kind::Tower, Some(EntityTemplate::Turret | EntityTemplate::VainTurret)) => Some(6),
            (Kind::Minion, Some(EntityTemplate::RangedMinion | EntityTemplate::LeadMinion)) => {
                Some(10)
            }
            _ => None,
        }
    }

    pub(super) fn advance_effects(&mut self, events: &mut Vec<Event>) {
        let mut effects = std::mem::take(&mut self.effects);
        effects.retain_mut(|v| {
            if self.winner.is_some() {
                return false;
            }
            if v.due > self.tick {
                return true;
            }
            if v.channel > 0
                && !self.index(v.src).is_some_and(|i| {
                    self.entities[i].status.channel == v.channel && self.entities[i].hp > 0.
                })
            {
                return false;
            }
            if let Area::NpcWindup(target) = v.area {
                let (Some(a), Some(b)) = (self.index(v.src), self.index(target)) else {
                    return false;
                };
                let (source, victim) = (&self.entities[a], &self.entities[b]);
                if source.hp <= 0. || victim.hp <= 0. {
                    return false;
                }
                let distance = source.position.dist(victim.position);
                let ticks = npc_flight_ticks(source.type_id, victim.type_id, distance);
                let id = self.next_id;
                self.next_id += 1;
                v.area = Area::Target(target, DamageSchool::Weapon);
                v.due = self.tick + ticks;
                events.push(Event::NpcProjectile {
                    id,
                    source: v.src,
                    target,
                });
                return true;
            }
            if let Area::Target(id, school) = v.area {
                if let Some(i) = self.index(id) {
                    self.hurt(v.src, v.team, i, v.damage, school, events)
                }
                return false;
            }
            let pierce = self
                .index(v.src)
                .map_or(0., |i| self.entities[i].item_stats.shield_pierce);
            for i in 0..self.entities.len() {
                let e = &self.entities[i];
                if e.hp <= 0. || !matches!(e.kind, Kind::Hero | Kind::Minion) {
                    continue;
                }
                let hit = match v.area {
                    Area::Circle(p, r) => p.dist(e.position) <= r,
                    Area::Line(a, b, r) => line_distance(e.position, a, b) <= r,
                    Area::Target(..) | Area::NpcWindup(_) => unreachable!(),
                };
                if !hit {
                    continue;
                }
                if e.team == v.team {
                    if v.heal > 0. {
                        self.heal(v.src, i, v.heal, events)
                    }
                } else {
                    let dmg = shop::mitigated(v.damage, e.stats.shield, pierce);
                    self.hurt(v.src, v.team, i, dmg, DamageSchool::Crystal, events)
                }
            }
            false
        });
        self.effects = effects;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn duel() -> World {
        let mut w = World::new(1, 10);
        w.entities[0].position = Position { x: 26., y: 0. };
        w.entities[1].position = Position { x: 29., y: 0. };
        w
    }
    #[test]
    fn tracked_npc_shot_keeps_its_due_time_and_never_reuses_an_actor_id() {
        let mut w = World::new(7, 6);
        w.use_six_human_roster().unwrap();
        w.use_aral_lane().unwrap();
        let tower = w
            .entities
            .iter()
            .position(|e| w.aral_structure_slot(e.id) == Some(7))
            .unwrap();
        let victim = w
            .entities
            .iter()
            .position(|e| e.kind == Kind::Hero && e.team != w.entities[tower].team)
            .unwrap();
        let source = w.entities[tower].id;
        let target = w.entities[victim].id;
        let team = w.entities[tower].team;
        let origin = w.entities[tower].position;
        w.entities[victim].position = Position {
            x: origin.x + 7.,
            y: origin.y,
        };
        let first_id = w.next_id;
        let mut events = Vec::new();
        w.effects
            .push(SpellEffect::npc_windup(6, source, team, target, 42.));
        for tick in 1..=6 {
            w.tick = tick;
            w.advance_effects(&mut events);
            assert_eq!(events.len(), usize::from(tick == 6));
        }
        assert_eq!(
            events,
            [Event::NpcProjectile {
                id: first_id,
                source,
                target
            }]
        );
        assert!(w.index(first_id).is_none());
        assert_eq!(w.next_id, first_id + 1);
        w.entities[tower].hp = 0.;
        let hp = w.entities[victim].hp;
        for tick in 7..=16 {
            w.tick = tick;
            w.advance_effects(&mut events);
            assert_eq!(
                w.entities[victim].hp,
                if tick == 16 { hp - 42. } else { hp }
            );
        }
        assert!(
            matches!(events.last(),Some(Event::Damage{source:a,target:b,..}) if *a==source && *b==target)
        );
        w.effects
            .push(SpellEffect::npc_windup(17, source, team, target, 42.));
        w.tick = 17;
        w.advance_effects(&mut events);
        assert!(w.effects.is_empty());
        assert_eq!(w.next_id, first_id + 1);

        w.entities[tower].hp = 10.;
        w.effects
            .push(SpellEffect::npc_windup(18, source, team, target, 42.));
        w.tick = 18;
        w.advance_effects(&mut events);
        w.entities[victim].hp = 0.;
        let before = events.len();
        w.tick = 28;
        w.advance_effects(&mut events);
        assert_eq!(events.len(), before);
        assert!(w.effects.is_empty());
    }

    #[test]
    fn solo_outer_tower_launches_then_impacts_only_the_measured_minion_profiles() {
        for (stat, flight) in [(5, 7), (4, 7), (6, 5)] {
            let mut w = World::new(12, 10);
            w.use_solo_mid_lane().unwrap();
            let tower = w.index(510).unwrap();
            let p = w.entities[tower].position;
            let target = w.spawn(
                1,
                Kind::Minion,
                stat,
                Position {
                    x: p.x + 3.,
                    y: p.y,
                },
                false,
            );
            let victim = w.index(target).unwrap();
            assert_eq!(
                w.npc_launch_delay(&w.entities[tower], &w.entities[victim]),
                Some(2)
            );
            assert_eq!(w.npc_launch_delay(&w.entities[tower], &w.entities[1]), None);
            w.entities[tower].order = Command::Attack(target);
            w.entities[tower].winding = Some((1, target));
            let events = w.step();
            assert!(
                events
                    .iter()
                    .any(|e| matches!(e,Event::Attack{source:510,target:t} if *t==target))
            );
            for n in 1..=flight + 2 {
                let events = w.step();
                assert_eq!(
                    events.iter().any(
                        |e| matches!(e,Event::NpcProjectile{source:510,target:t,..} if *t==target)
                    ),
                    n == 2
                );
                assert_eq!(
                    events
                        .iter()
                        .any(|e| matches!(e,Event::Damage{source:510,target:t,..} if *t==target)),
                    n == flight + 2
                );
                if n == 2 {
                    w.entities[tower].hp = 0.;
                }
            }
        }
    }

    #[test]
    fn only_original_aral_ranged_npcs_use_the_measured_delays() {
        let mut w = World::new(3, 6);
        let tower = w.entities.iter().find(|e| e.kind == Kind::Tower).unwrap();
        assert_eq!(w.npc_launch_delay(tower, &w.entities[0]), None);
        w.use_six_human_roster().unwrap();
        w.use_aral_lane().unwrap();
        for _ in 0..18 * TICK_HZ {
            w.step();
        }
        for e in &w.entities {
            let delay = match e.type_id {
                300 | 301 => Some(6),
                287 | 288 => Some(10),
                _ => None,
            };
            assert_eq!(w.npc_launch_delay(e, &w.entities[0]), delay);
        }
        assert_eq!(npc_flight_ticks(287, 243, 5.), 12);
        assert_eq!(npc_flight_ticks(288, 243, 5.), 10);
        assert_eq!(npc_flight_ticks(300, 243, 5.), 6);
        assert_eq!(npc_flight_ticks(301, 243, 9.), 14);
    }

    #[test]
    fn windup_interrupt_and_release_charge_once() {
        let mut w = duel();
        let slot = AbilitySlot::A;
        let cast = Command::Cast {
            slot,
            target: CastTarget::Entity(2),
        };
        assert!(w.command(1, cast).is_err());
        w.command(1, Command::Learn(slot)).unwrap();
        w.command(1, cast).unwrap();
        w.step();
        w.command(1, Command::Stop).unwrap();
        for _ in 0..5 {
            w.step();
        }
        assert_eq!(w.entities[0].energy, w.entities[0].max_energy);
        assert_eq!(w.entities[1].hp, 703.);
        w.command(1, cast).unwrap();
        let mut fired = 0;
        for _ in 0..5 {
            fired += w
                .step()
                .iter()
                .filter(|e| matches!(e, Event::Cast { source: 1, .. }))
                .count();
        }
        assert_eq!(fired, 1);
        assert!(w.entities[1].hp < 703.);
        assert!(w.entities[0].energy < 145.);
        assert!(w.command(1, cast).is_err());
    }
    #[test]
    fn native_lorelai_cooldown_uses_release_tick_and_equipped_reduction() {
        let mut w = World::new(0, 10);
        w.entities[0].type_id = 409;
        w.entities[0].gold = 250;
        w.purchase(1, BasicItem::Hourglass).unwrap();
        let slot = AbilitySlot::B;
        w.command(1, Command::Learn(slot)).unwrap();
        let cast = Command::Cast {
            slot,
            target: CastTarget::Point(w.entities[0].position),
        };
        w.command(1, cast).unwrap();
        w.command(1, Command::Stop).unwrap();
        w.step();
        assert_eq!(w.entities[0].cooldown[slot.idx()], 0);
        w.command(1, cast).unwrap();
        let mut released = false;
        for _ in 0..3 {
            if w.step()
                .iter()
                .any(|e| matches!(e, Event::Cast { source: 1, .. }))
            {
                released = true;
                break;
            }
        }
        assert!(released);
        let deadline = w.entities[0].cooldown[slot.idx()];
        assert_eq!(deadline, w.tick + 149); // ceil(8 seconds * 20 Hz / 1.075).
        w.tick = deadline - 1;
        assert!(w.command(1, cast).is_err());
        w.tick = deadline;
        assert!(w.command(1, cast).is_ok());
    }

    #[test]
    fn targeted_spell_rechecks_visibility_and_range_on_release() {
        let mut w = duel();
        w.command(1, Command::Learn(AbilitySlot::A)).unwrap();
        w.command(
            1,
            Command::Cast {
                slot: AbilitySlot::A,
                target: CastTarget::Entity(2),
            },
        )
        .unwrap();
        w.entities[1].position = base(1);
        for _ in 0..8 {
            w.step();
        }
        assert_eq!(w.entities[0].energy, w.entities[0].max_energy);
        assert!(
            w.command(
                1,
                Command::Cast {
                    slot: AbilitySlot::A,
                    target: CastTarget::Entity(2)
                }
            )
            .is_err()
        );
    }
    #[test]
    fn saw_channel_stops_before_next_damage_pulse() {
        let mut w = duel();
        w.entities[0].type_id = 250;
        w.command(1, Command::Learn(AbilitySlot::B)).unwrap();
        w.command(
            1,
            Command::Cast {
                slot: AbilitySlot::B,
                target: CastTarget::Point(Position { x: 33., y: 0. }),
            },
        )
        .unwrap();
        for _ in 0..8 {
            w.step();
        }
        let hp = w.entities[1].hp;
        assert!((hp - (703. - 70. / 1.2)).abs() < 0.01);
        w.command(1, Command::MoveTo(Position { x: 24., y: 0. }))
            .unwrap();
        for _ in 0..40 {
            w.step();
        }
        assert_eq!(w.entities[1].hp, hp);
    }
    #[test]
    fn lyra_sigil_applies_heal_and_shielded_damage_only_inside_zone() {
        let mut w = duel();
        w.entities[0].type_id = 269;
        w.entities[2].position = Position { x: 27.5, y: 0. };
        w.entities[2].hp = 100.;
        w.command(1, Command::Learn(AbilitySlot::A)).unwrap();
        w.command(
            1,
            Command::Cast {
                slot: AbilitySlot::A,
                target: CastTarget::Point(Position { x: 28., y: 0. }),
            },
        )
        .unwrap();
        for _ in 0..8 {
            w.step();
        }
        assert_eq!(w.entities[2].hp, 125.);
        let enemy = w.entities[1].hp;
        w.entities[1].position = Position { x: 33., y: 0. };
        for _ in 0..40 {
            w.step();
        }
        assert_eq!(w.entities[2].hp, 175.);
        assert_eq!(w.entities[1].hp, enemy);
    }
    #[test]
    fn attack_projectile_survives_attacker_stop_and_is_counted_for_last_hits() {
        let mut w = duel();
        let id = w.spawn(1, Kind::Minion, 4, Position { x: 30., y: 0. }, false);
        w.entities.last_mut().unwrap().hp = 10.;
        w.command(1, Command::Attack(id)).unwrap();
        for _ in 0..4 {
            w.step();
        }
        let target = w.index(id).unwrap();
        assert_eq!(w.entities[target].hp, 10.);
        assert_eq!(w.predicted_hp(&w.entities[target], w.tick + 10), 0.);
        w.command(1, Command::Stop).unwrap();
        for _ in 0..8 {
            w.step();
        }
        assert!(w.index(id).is_none());
        assert_eq!(w.entities[0].gold, 35);
    }
}
