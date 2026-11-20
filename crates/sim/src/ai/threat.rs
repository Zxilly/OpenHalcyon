//! Deterministic, zero-allocation Layer 4 threat & influence map evaluation.

use super::super::*;

/// Normalized influence and danger assessment around an actor or probe point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThreatAssessment {
    pub probe: Position,
    pub in_enemy_turret_range: bool,
    pub allied_minion_tanking_turret: bool,
    pub enemy_hero_under_turret: bool,
    pub undefended_turret_threat: f32,
    pub nearest_turret_id: Option<u32>,
    pub nearest_turret_pos: Position,
    pub nearest_turret_range: f32,
    pub hero_burst_threat: f32,
    pub hazard_threat: f32,
    pub allied_turret_cover: bool,
    pub allied_minion_cover: u8,
    pub nearby_allied_heroes: u8,
    pub nearby_enemy_heroes: u8,
    pub enemy_heroes_in_attack_range: u8,
    pub lowest_ally_id: Option<u32>,
    pub lowest_ally_hp_ratio: f32,
    pub bush_concealed: bool,
    pub fog_ambush_risk: f32,
    pub threat_centroid: Position,
    pub hazard_repulsion: Position,
    pub strongest_threat_id: Option<u32>,
    pub total_threat: f32,
}

impl Default for ThreatAssessment {
    fn default() -> Self {
        Self {
            probe: Position::default(),
            in_enemy_turret_range: false,
            allied_minion_tanking_turret: false,
            enemy_hero_under_turret: false,
            undefended_turret_threat: 0.0,
            nearest_turret_id: None,
            nearest_turret_pos: Position::default(),
            nearest_turret_range: 0.0,
            hero_burst_threat: 0.0,
            hazard_threat: 0.0,
            allied_turret_cover: false,
            allied_minion_cover: 0,
            nearby_allied_heroes: 0,
            nearby_enemy_heroes: 0,
            enemy_heroes_in_attack_range: 0,
            lowest_ally_id: None,
            lowest_ally_hp_ratio: 1.0,
            bush_concealed: false,
            fog_ambush_risk: 0.0,
            threat_centroid: Position::default(),
            hazard_repulsion: Position::default(),
            strongest_threat_id: None,
            total_threat: 0.0,
        }
    }
}

impl Room<'_> {
    /// Returns `true` when `p` is valid movement ground in the active layout or navmesh.
    pub(crate) fn is_position_walkable(&self, p: Position) -> bool {
        p.x.is_finite()
            && p.y.is_finite()
            && (self.layout.walkable(p)
                || (self.map_5v5.is_some() && map_5v5::in_5v5_bounds(p))
                || self
                    .native_navigation
                    .as_ref()
                    .is_some_and(|map| map.can_project(p)))
    }

    /// Returns `true` when a hero at `from` can legally path toward `to`.
    pub(crate) fn is_position_reachable(&self, from: Position, to: Position) -> bool {
        if !self.is_position_walkable(to) {
            return false;
        }
        if self.native_navigation.is_some() || self.map_5v5.is_some() {
            return true;
        }
        match self.layout {
            Layout::SoloMid { .. } => solo_side::steer(from, to).is_some(),
            Layout::Validation
            | Layout::Aral { .. }
            | Layout::HalcyonFold { .. }
            | Layout::Blitz { .. } => true,
        }
    }

    /// Evaluates deterministic threat & influence around hero `hero_idx` at its live position.
    pub fn evaluate_threat(&self, hero_idx: usize) -> ThreatAssessment {
        let probe = self.entities[hero_idx].position;
        self.evaluate_threat_at(hero_idx, probe)
    }

    /// Evaluates deterministic threat & influence for hero `hero_idx` at candidate `probe`.
    /// Performs zero heap allocations and breaks all ties deterministically by `entity.id`.
    pub fn evaluate_threat_at(&self, hero_idx: usize, probe: Position) -> ThreatAssessment {
        let hero = &self.entities[hero_idx];
        let mut out = ThreatAssessment {
            probe,
            ..ThreatAssessment::default()
        };
        let hero_max_hp = hero.max_hp.max(1.0);
        let enemy_team = 1 - hero.team;

        // 1. Scan actors in deterministic ActorMetadata order (ties broken by entity.id).
        let mut nearest_turret_rank: Option<(f32, u32)> = None;
        let mut strongest_threat_rank: Option<(f32, u32)> = None;
        let mut lowest_ally_rank: Option<(f32, u32)> = None;
        let mut weighted_threat_x = 0.0_f32;
        let mut weighted_threat_y = 0.0_f32;
        let mut threat_weight_sum = 0.0_f32;

        for actor in &self.entities {
            if actor.hp <= 0.0 {
                continue;
            }
            let d = probe.dist(actor.position);
            if actor.team == hero.team {
                match actor.kind {
                    Kind::Minion if d <= 7.5 => {
                        out.allied_minion_cover = out.allied_minion_cover.saturating_add(1);
                    }
                    Kind::Tower if actor.stats.range > 0.0 && d <= actor.stats.range + 1.5 => {
                        out.allied_turret_cover = true;
                    }
                    Kind::Hero if actor.id != hero.id && d <= 12.0 => {
                        out.nearby_allied_heroes = out.nearby_allied_heroes.saturating_add(1);
                        let ratio = (actor.hp / actor.max_hp.max(1.0)).clamp(0.0, 1.0);
                        let better = lowest_ally_rank
                            .is_none_or(|(best_r, best_id)| (ratio, actor.id) < (best_r, best_id));
                        if better {
                            lowest_ally_rank = Some((ratio, actor.id));
                            out.lowest_ally_id = Some(actor.id);
                            out.lowest_ally_hp_ratio = ratio;
                        }
                    }
                    _ => {}
                }
                continue;
            }

            if actor.kind == Kind::Tower {
                let range = actor.stats.range.max(1.0);
                let visible_or_near = d <= range + 4.5 || self.visible(hero.team, actor);
                if visible_or_near {
                    let rank = (d, actor.id);
                    if nearest_turret_rank.is_none_or(|best| rank < best) {
                        nearest_turret_rank = Some(rank);
                        out.nearest_turret_id = Some(actor.id);
                        out.nearest_turret_pos = actor.position;
                        out.nearest_turret_range = range;
                    }
                    if d <= range + 0.5 {
                        out.in_enemy_turret_range = true;
                        let tanked = self.entities.iter().any(|ally| {
                            ally.team == hero.team
                                && ally.kind == Kind::Minion
                                && ally.hp > 0.0
                                && ally.position.dist(actor.position) <= range
                        });
                        if tanked {
                            out.allied_minion_tanking_turret = true;
                        } else {
                            let depth = ((range + 0.5 - d) / range).clamp(0.25, 1.0);
                            out.undefended_turret_threat =
                                (out.undefended_turret_threat + 0.75 + 0.35 * depth).min(1.5);
                        }
                        if self.entities.iter().any(|eh| {
                            eh.team != hero.team
                                && eh.kind == Kind::Hero
                                && eh.hp > 0.0
                                && self.visible(hero.team, eh)
                                && eh.position.dist(actor.position) <= range + 1.0
                        }) {
                            out.enemy_hero_under_turret = true;
                        }
                    }
                }
                continue;
            }

            if actor.kind == Kind::Hero {
                let eff_range = actor.stats.range.max(2.0);
                let threat_horizon = eff_range + 5.0;
                if d <= threat_horizon && self.hero_sees(hero, actor, d) {
                    out.nearby_enemy_heroes = out.nearby_enemy_heroes.saturating_add(1);
                    if d <= eff_range + 0.75 {
                        out.enemy_heroes_in_attack_range =
                            out.enemy_heroes_in_attack_range.saturating_add(1);
                    }
                    let offensive_power = actor.stats.weapon
                        + 0.55 * actor.item_stats.crystal
                        + 18.0 * f32::from(actor.level);
                    let proximity = ((threat_horizon - d) / threat_horizon).clamp(0.12, 1.0);
                    let hp_factor = (actor.hp / actor.max_hp.max(1.0)).clamp(0.25, 1.0);
                    let single_threat =
                        (offensive_power / (hero_max_hp * 0.48)) * proximity * hp_factor;
                    out.hero_burst_threat += single_threat;
                    weighted_threat_x += actor.position.x * single_threat;
                    weighted_threat_y += actor.position.y * single_threat;
                    threat_weight_sum += single_threat;
                    // Higher threat sorts first; lower actor.id breaks exact ties.
                    let rank = (-single_threat, actor.id);
                    if strongest_threat_rank.is_none_or(|best| rank < best) {
                        strongest_threat_rank = Some(rank);
                        out.strongest_threat_id = Some(actor.id);
                    }
                }
            }
        }

        // 2. Evaluate active spell hazard zones & visible/detonating traps.
        let mut repulse_x = 0.0_f32;
        let mut repulse_y = 0.0_f32;
        let mut add_hazard = |pos: Position, radius: f32, weight: f32| {
            let d = probe.dist(pos);
            if d <= radius + 1.5 {
                let intensity = ((radius + 1.5 - d) / (radius + 1.5)).clamp(0.1, 1.0) * weight;
                out.hazard_threat += intensity;
                let dx = probe.x - pos.x;
                let dy = probe.y - pos.y;
                let norm = dx.hypot(dy).max(0.001);
                repulse_x += (dx / norm) * intensity;
                repulse_y += (dy / norm) * intensity;
            }
        };

        for field in self.entities.spells::<sigil::Field>() {
            let (team, center, radius) = field.hazard_circle();
            if team != hero.team && self.sees_position(hero.team, center) {
                add_hazard(center, radius, 0.35);
            }
        }
        for field in self.entities.spells::<bulwark::Field>() {
            let (team, center, radius) = field.hazard_circle(self);
            if team != hero.team && self.sees_position(hero.team, center) {
                add_hazard(center, radius, 0.40);
            }
        }
        for water in self.entities.spells::<lorelai::Water>() {
            if let Some((team, pos, radius)) = water.hazard_circle()
                && team != hero.team
                && self.sees_position(hero.team, pos)
            {
                add_hazard(pos, radius, 0.35);
            }
        }
        for device in self.vision_devices() {
            if device.team != hero.team && device.kind == VisionDeviceKind::ScoutTrap {
                // Only react to traps that are visibly exposed or triggered/detonating.
                if self.sees_position(hero.team, device.point) {
                    add_hazard(device.point, 3.2, 0.45);
                }
            }
        }

        // Active status impairments on the hero itself increase immediate danger.
        if probe == hero.position {
            if hero.status.root_until > self.tick || hero.status.silence_until > self.tick {
                out.hazard_threat += 0.30;
            }
            if hero.status.slow_until > self.tick
                || hero.status.bulwark_slow_until > self.tick
                || hero.status.water_slow_until > self.tick
            {
                out.hazard_threat += 0.15;
            }
        }

        // 3. Bush safety & fog ambush risk.
        out.bush_concealed = if self.native_navigation.is_some() || hero.hp < hero_max_hp * 0.35 {
            !self.sees_position(enemy_team, probe)
        } else {
            out.nearby_enemy_heroes == 0 && !out.in_enemy_turret_range
        };
        if let Some((_, last_pos, at)) = self.entities.last_seen(hero_idx)
            && self.tick.saturating_sub(at) <= 3 * TICK_HZ
            && !self.sees_position(hero.team, last_pos)
        {
            let d = probe.dist(last_pos);
            if d <= 10.0 {
                out.fog_ambush_risk = ((10.0 - d) / 10.0) * 0.25;
            }
        }

        if threat_weight_sum > 0.0001 {
            out.threat_centroid = Position {
                x: weighted_threat_x / threat_weight_sum,
                y: weighted_threat_y / threat_weight_sum,
            };
        } else if let Some(tid) = out.nearest_turret_id
            && out.in_enemy_turret_range
        {
            let _ = tid;
            out.threat_centroid = out.nearest_turret_pos;
        } else {
            out.threat_centroid = probe;
        }
        out.hazard_repulsion = Position {
            x: repulse_x,
            y: repulse_y,
        };

        // 4. Combine raw danger minus allied cover & bush concealment.
        let cover_mitigation = f32::from(out.allied_minion_cover.min(4)) * 0.05
            + f32::from(out.nearby_allied_heroes.min(3)) * 0.12
            + if out.allied_turret_cover { 0.28 } else { 0.0 }
            + if out.bush_concealed { 0.18 } else { 0.0 };
        let turret_danger = out.undefended_turret_threat
            + if out.in_enemy_turret_range && out.enemy_hero_under_turret {
                0.22
            } else {
                0.0
            };
        let raw = turret_danger + out.hero_burst_threat + out.hazard_threat + out.fog_ambush_risk;
        out.total_threat = (raw - cover_mitigation.min(raw * 0.65)).max(0.0);
        out
    }

    /// Computes a deterministic, walkable retreat waypoint outside an enemy turret's range.
    pub(crate) fn safe_turret_exit_point(
        &self,
        hero: &Entity,
        turret_pos: Position,
        turret_range: f32,
        home: Position,
    ) -> Position {
        let mut preferred = turret_pos;
        preferred.x += (if hero.team == 0 { -1.0 } else { 1.0 }) * (turret_range + 1.0);
        if self.is_position_walkable(preferred)
            && (self.map_5v5.is_some() || self.layout.clear(hero.position, preferred))
            && hero.position.dist(preferred) > 0.05
        {
            return preferred;
        }
        let radial = turret_pos.advance(home, turret_range + 1.25);
        if self.is_position_walkable(radial)
            && (self.map_5v5.is_some() || self.layout.clear(hero.position, radial))
            && hero.position.dist(radial) > 0.05
        {
            return radial;
        }
        self.retreat_step(hero, home)
    }

    /// Computes a deterministic, walkable kite step away from threats/hazards toward `home`.
    pub(crate) fn safe_kite_point(
        &self,
        hero: &Entity,
        threat: &ThreatAssessment,
        home: Position,
    ) -> Position {
        if threat.hazard_threat > 0.20 {
            let rx = threat.hazard_repulsion.x;
            let ry = threat.hazard_repulsion.y;
            let mag = rx.hypot(ry);
            if mag > 0.01 {
                let candidate = Position {
                    x: hero.position.x + (rx / mag) * 1.5,
                    y: hero.position.y + (ry / mag) * 1.5,
                };
                if self.is_position_walkable(candidate)
                    && (self.map_5v5.is_some() || self.layout.clear(hero.position, candidate))
                    && !self.point_inside_undefended_enemy_turret(hero.team, candidate)
                {
                    return candidate;
                }
            }
        }
        self.retreat_step(hero, home)
    }

    /// Returns `true` if `point` lies inside an enemy turret's range with no allied minion cover.
    pub(crate) fn point_inside_undefended_enemy_turret(&self, team: u8, point: Position) -> bool {
        self.entities.iter().any(|t| {
            t.team != team
                && t.kind == Kind::Tower
                && t.hp > 0.0
                && point.dist(t.position) <= t.stats.range + 0.5
                && !self.entities.iter().any(|ally| {
                    ally.team == team
                        && ally.kind == Kind::Minion
                        && ally.hp > 0.0
                        && ally.position.dist(t.position) <= t.stats.range
                })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threat_map_kites_undefended_turret_and_engages_when_minions_tank() {
        let mut w = World::new(3, 0);
        w.set_modern_ai(true);
        // Team 0 hero inside Team 1 outer turret at (48, 0), range 9.0, no allied minions.
        w.entities[0].position = Position { x: 43.0, y: 0.0 };
        let threat = w.evaluate_threat(0);
        assert!(threat.in_enemy_turret_range);
        assert!(!threat.allied_minion_tanking_turret);
        assert!(threat.undefended_turret_threat > 0.7);

        let cmd = w.decide(0);
        let Command::MoveTo(exit) = cmd else {
            panic!("expected turret exit move, got {cmd:?}");
        };
        assert!(exit.x < 48.0 - 9.0, "exit={exit:?}");

        // Spawn an allied minion inside turret range -> tanked, hero pushes tower!
        w.spawn(0, Kind::Minion, 4, Position { x: 45.0, y: 0.0 }, false);
        let tanked_threat = w.evaluate_threat(0);
        assert!(tanked_threat.in_enemy_turret_range);
        assert!(tanked_threat.allied_minion_tanking_turret);
        assert_eq!(tanked_threat.undefended_turret_threat, 0.0);
        assert!(matches!(w.decide(0), Command::Attack(_)));
    }

    #[test]
    fn threat_map_detects_hazard_zones_and_breaks_ties_by_entity_id() {
        let mut w = World::new(5, 0);
        w.set_modern_ai(true);
        w.entities.truncate(4);
        // Hero 0 at (20, 0); two identical enemy heroes (id=2 and id=4) at exact equal distance.
        w.entities[0].team = 0;
        w.entities[0].position = Position { x: 20.0, y: 0.0 };
        w.entities[1].team = 1;
        w.entities[1].id = 4;
        w.entities[1].position = Position { x: 24.0, y: 0.0 };
        w.entities[2].team = 1;
        w.entities[2].id = 2;
        w.entities[2].position = Position { x: 24.0, y: 0.0 };
        w.entities[3].team = 0;
        w.entities[3].hp = 0.0;
        w.entities.sort_unstable_by_key(|e| e.id);

        let idx0 = w.index(1).unwrap();
        let threat = w.evaluate_threat(idx0);
        assert_eq!(threat.nearby_enemy_heroes, 2);
        assert_eq!(threat.strongest_threat_id, Some(2));

        // Deploy a hostile visible ScoutTrap right next to hero 0 and verify repulsion/hazard.
        w.entities[idx0].team = 0;
        let mut enemy_trap_owner = w.entities[0].clone();
        enemy_trap_owner.id = 99;
        enemy_trap_owner.team = 1;
        enemy_trap_owner.position = Position { x: 21.0, y: 1.0 };
        w.entities.push(enemy_trap_owner);
        w.entities.spawn_spell(lorelai::Water::test_hazard_at(
            1,
            Position { x: 21.5, y: 0.0 },
        ));
        let hazard_threat = w.evaluate_threat(w.index(1).unwrap());
        assert!(hazard_threat.hazard_threat > 0.15);
        assert!(hazard_threat.hazard_repulsion.x < 0.0);
    }

    #[test]
    #[ignore = "requires native ARAL brush geometry"]
    fn threat_map_bush_concealment_reduces_threat_on_native_map() {
        let base = std::env::var_os("HALCYON_DELIVERY").unwrap();
        let extra = std::path::PathBuf::from(std::env::var_os("HALCYON_DATA_EXTRA").unwrap());
        let data = GameData::load_with_extra(base, Some(&extra)).unwrap();
        let mut w = World::from_data(0, 6, &data).unwrap();
        w.use_six_human_roster().unwrap();
        w.use_aral_lane().unwrap();
        w.install_map_data(&data).unwrap();
        for idx in 2..w.entities.len() {
            w.entities[idx].hp = 0.0;
        }
        // Place hero 0 inside ARAL brush (-22.94, -5.37) while enemy is outside at (-22.0, 1.0).
        w.entities[0].position = Position {
            x: -22.94,
            y: -5.37,
        };
        w.entities[1].position = Position { x: -22.0, y: 1.0 };
        let concealed = w.evaluate_threat(0);
        assert!(concealed.bush_concealed);

        // Step outside brush into enemy sight -> bush_concealed flips false.
        w.entities[0].position = Position { x: -22.0, y: -1.0 };
        let exposed = w.evaluate_threat(0);
        assert!(!exposed.bush_concealed);
        assert!(exposed.total_threat > concealed.total_threat);
    }
}
