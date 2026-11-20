use super::*;
impl Entity {
    pub(super) fn apply_growth(&mut self, levels: u8) {
        let g = self.stats.growth;
        let levels = f32::from(levels);
        self.max_hp += g.hp * levels;
        self.hp = (self.hp + g.hp * levels).min(self.max_hp);
        self.max_energy += g.energy * levels;
        self.energy += g.energy * levels;
        self.stats.weapon += g.weapon * levels;
        self.stats.armor += g.armor * levels;
        self.stats.shield += g.shield * levels;
    }
}

impl World {
    /// The measured XP threshold attribute in modeled native scene layouts.
    pub fn native_xp_required(&self, level: u8) -> Option<f32> {
        self.layout.native_xp_required(level)
    }

    /// Time until a fallen hero's authoritative respawn tick.
    pub fn respawn_seconds(&self, id: u32) -> Option<f32> {
        let actor = &self.entities[self.index(id)?];
        if actor.kind != Kind::Hero || actor.hp > 0.0 || actor.respawn <= self.tick {
            return None;
        }
        Some((actor.respawn - self.tick) as f32 / TICK_HZ as f32)
    }

    pub(super) fn award_xp(&mut self, i: usize, xp: f32, events: &mut Vec<Event>) {
        let layout = &self.layout;
        let e = &mut self.entities[i];
        let native = layout.native_xp_required(e.level).is_some();
        if e.kind != Kind::Hero || (!native && e.level >= 12) || !xp.is_finite() || xp <= 0.0 {
            return;
        }
        e.xp += xp;
        if native {
            events.push(Event::XpGain {
                id: e.id,
                amount: xp,
            });
        }
        while e.level < 12 {
            let need = layout
                .native_xp_required(e.level)
                .unwrap_or_else(|| 90. + 8. * f32::from(e.level - 1));
            if e.xp < need {
                break;
            }
            e.xp -= need;
            e.level += 1;
            e.skill_points += 1;
            let was_dead = e.hp <= 0.0;
            e.apply_growth(1);
            if native && was_dead {
                e.hp = 0.0;
            }
            events.push(Event::Level {
                id: e.id,
                level: e.level,
            });
        }
        if !native && e.level == 12 {
            e.xp = 0.;
        }
    }
    pub(super) fn heal(&mut self, src: u32, i: usize, n: f32, events: &mut Vec<Event>) {
        let e = &mut self.entities[i];
        if e.hp <= 0. {
            return;
        }
        let was = e.hp;
        e.hp = (was + n).min(e.max_hp);
        if e.hp > was {
            events.push(Event::Heal {
                source: src,
                target: e.id,
                amount: e.hp - was,
                hp: e.hp,
            })
        }
    }
    pub(super) fn hurt(
        &mut self,
        src: u32,
        team: u8,
        i: usize,
        n: f32,
        school: DamageSchool,
        events: &mut Vec<Event>,
    ) {
        let t = &self.entities[i];
        if t.hp <= 0. || n <= 0. {
            return;
        }
        let is_outer_tower = self.is_outer_tower(t);
        let t = &mut self.entities[i];
        let absorbed = if t.status.barrier_until > self.tick {
            n.min(t.status.barrier)
        } else {
            0.
        };
        t.status.barrier -= absorbed;
        let amount = (n - absorbed).min(t.hp);
        if amount <= 0. {
            return;
        }
        let t = &self.entities[i];
        if team != t.team && matches!(t.kind, Kind::Tower | Kind::Crystal) {
            // Only hero basic attacks and spells can outlive their origin actor's turn.
            // Hero and structure entities persist through death; minions attack immediately.
            let minion = self
                .index(src)
                .is_some_and(|j| self.entities[j].kind == Kind::Minion);
            let damage = &mut self.team_scores[usize::from(team)].structure_damage;
            let bucket = if t.kind == Kind::Tower {
                &mut damage.towers
            } else {
                &mut damage.crystals
            };
            if minion {
                bucket.minions += f64::from(amount);
            } else {
                bucket.others += f64::from(amount);
            }
        }
        let t = &mut self.entities[i];
        t.hp -= amount;
        if t.kind == Kind::Armory {
            t.respawn = self.tick
                + if t.hp == 0. {
                    armory::REBUILD
                } else {
                    armory::REGENERATE
                };
        }
        let id = t.id;
        if team != t.team
            && let Some(score) = src
                .checked_sub(1)
                .and_then(|seat| self.scores.get_mut(seat as usize))
        {
            score.hit(t.kind, amount, school, t.hp == 0.);
        }
        events.push(Event::Damage {
            source: src,
            target: id,
            amount,
            hp: t.hp,
        });
        if t.hp > 0. {
            return;
        }
        let pos = t.position;
        let kind = t.kind;
        let enemy = t.team != team;
        if enemy {
            let scoring = &mut self.team_scores[usize::from(team)];
            match kind {
                Kind::Tower => {
                    scoring.structure_kills.towers += 1;
                    if is_outer_tower {
                        scoring
                            .first_enemy_outer_tower_kill_seconds
                            .get_or_insert(self.tick as f64 / TICK_HZ as f64);
                    }
                }
                Kind::Crystal => scoring.structure_kills.crystals += 1,
                Kind::Armory => {
                    scoring.armories_destroyed += 1;
                    scoring
                        .first_enemy_armory_kill_seconds
                        .get_or_insert(self.tick as f64 / TICK_HZ as f64);
                }
                _ => {}
            }
        }
        let (bounty, xp) = match kind {
            Kind::Hero => (250 + if self.first_blood { 0 } else { 125 }, 90.),
            Kind::Tower => (300, 60.),
            Kind::Crystal | Kind::Armory => (0, 0.),
            Kind::Minion => match t.type_id {
                335 => (25, 6.),
                337 => (60, 12.),
                338 => (100, 12.),
                _ => (35, 6.),
            },
        };
        t.order = Command::Stop;
        t.winding = None;
        t.cast = None;
        t.status = Default::default();
        t.last_seen = None;
        events.push(Event::Death { id, killer: src });
        if kind == Kind::Armory {
            return;
        }
        t.respawn = self.tick + RESPAWN + (self.tick / 60).min(900);
        if enemy
            && matches!(self.layout, Layout::Aral { .. })
            && let Some(killer) = src
                .checked_sub(1)
                .and_then(|s| self.entities.get_mut(s as usize))
            && killer.id == src
            && killer.kind == Kind::Hero
            && killer.type_id == 243
            && killer.hp > 0.
            && !killer.status.double_down
            && killer.team == team
        {
            killer.status.double_down = true;
            events.push(Event::DoubleDown {
                source: src,
                attack_target: None,
            });
        }
        if kind == Kind::Crystal {
            self.winner = Some(team);
            events.push(Event::End { winner: team });
            return;
        }
        if kind == Kind::Hero {
            self.first_blood = true;
        }
        // Ten stable hero seats. A death awards its killer even if the projectile
        // travelled beyond the project assist radius after release.
        let killer_kind = self.index(src).map(|j| self.entities[j].kind);
        let hero_kill = killer_kind == Some(Kind::Hero);
        if kind == Kind::Hero && self.entities[i].team != team {
            let scoring_team = &mut self.team_scores[usize::from(team)];
            match killer_kind {
                Some(Kind::Hero) => scoring_team.hero_kills += 1,
                Some(Kind::Tower) => {
                    scoring_team.turret_hero_kills += 1;
                    self.scores[(id - 1) as usize].turret_deaths += 1;
                }
                _ => {}
            }
        }
        let mut allies = [0usize; 10];
        let mut count = 0;
        for (j, a) in self.entities.iter_mut().take(10).enumerate() {
            if a.team != team || a.kind != Kind::Hero {
                continue;
            }
            let nearby = a.hp > 0. && (a.position.dist(pos) <= 10. || a.id == src);
            let amount = if a.id == src {
                bounty
            } else if nearby && kind == Kind::Minion {
                if hero_kill {
                    bounty / 2
                } else {
                    bounty * 3 / 10
                }
            } else {
                0
            };
            if amount > 0 {
                a.gold += amount;
                let income = &mut self.scores[(a.id - 1) as usize].gold_income;
                if a.id == src {
                    income.last_hit_bounties += u64::from(amount);
                } else {
                    income.lane_proximity_awards += u64::from(amount);
                }
                events.push(Event::GoldAward { id: a.id, amount });
            }
            if !nearby {
                continue;
            }
            allies[count] = j;
            count += 1;
        }
        if count > 0 {
            let shares = if count == 1 {
                xp
            } else {
                xp * 1.2 / count as f32
            };
            for j in &allies[..count] {
                self.award_xp(*j, shares, events)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remaining_respawn_time_uses_the_authoritative_tick() {
        let mut world = World::new(7, 2);
        assert_eq!(world.respawn_seconds(2), None);
        world.tick = 137;
        let mut events = vec![];
        world.hurt(1, 0, 1, 5000.0, DamageSchool::True, &mut events);
        assert!(events.contains(&Event::Death { id: 2, killer: 1 }));
        let respawn = world.entities[1].respawn;
        assert_eq!(
            world.respawn_seconds(2),
            Some((respawn - world.tick) as f32 / TICK_HZ as f32)
        );
        world.tick = respawn - 1;
        assert_eq!(world.respawn_seconds(2), Some(0.05));
        assert!(
            world
                .step()
                .iter()
                .any(|event| matches!(event, Event::Respawn { id: 2, .. }))
        );
        assert_eq!(world.respawn_seconds(2), None);
        assert_eq!(world.respawn_seconds(9999), None);
    }

    #[test]
    fn native_modes_use_independent_thresholds_and_single_gain_events() {
        let mut aral = World::new(9, 6);
        aral.use_six_human_roster().unwrap();
        aral.assign_teams(&[]).unwrap();
        aral.use_aral_lane().unwrap();
        let original = &aral.entities[0];
        assert_eq!(
            (original.level, original.skill_points, original.xp),
            (4, 4, 0.)
        );
        assert!((original.max_hp - (703. + 3. * 127.64)).abs() < 0.001);
        assert_eq!(original.max_energy, 163. + 3. * 23.);
        assert_eq!(aral.native_xp_required(4), Some(90.));
        let mut events = vec![];
        aral.award_xp(0, 89., &mut events);
        assert_eq!(events, [Event::XpGain { id: 1, amount: 89. }]);
        assert_eq!(aral.entities[0].level, 4);
        events.clear();
        aral.award_xp(0, 131., &mut events);
        assert_eq!(
            events,
            [
                Event::XpGain {
                    id: 1,
                    amount: 131.
                },
                Event::Level { id: 1, level: 5 },
                Event::Level { id: 1, level: 6 },
            ]
        );
        assert_eq!(aral.entities[0].xp, 0.);
        assert_eq!(aral.native_xp_required(6), Some(140.));

        let mut solo = World::new(9, 1);
        solo.use_solo_mid_lane().unwrap();
        let mut events = vec![];
        solo.award_xp(0, 153., &mut events);
        assert_eq!(
            events,
            [
                Event::XpGain {
                    id: 1,
                    amount: 153.
                },
                Event::Level { id: 1, level: 2 },
                Event::Level { id: 1, level: 3 },
            ]
        );
        assert_eq!(solo.entities[0].xp, 1.);
        assert_eq!(solo.native_xp_required(3), Some(100.));
        solo.entities[0].hp = 0.;
        events.clear();
        solo.award_xp(0, 100., &mut events);
        assert_eq!(solo.entities[0].hp, 0.);
        assert_eq!(solo.entities[0].level, 4);
        assert_eq!(solo.entities[0].xp, 1.);
        solo.entities[0].level = 12;
        solo.entities[0].hp = 0.;
        events.clear();
        solo.award_xp(0, 0.6, &mut events);
        assert_eq!(events, [Event::XpGain { id: 1, amount: 0.6 }]);
        assert_eq!(solo.entities[0].xp, 1.6);
    }

    #[test]
    fn draft_reselection_restores_base_stats_then_applies_growth_once() {
        use halcyon_data::ManifestEntry;
        use serde_json::{Value, json};

        let unit = serde_json::from_value(json!({
            "name":"Lorelai", "hp": 700.0, "energy": 300.0, "weapon": 40.0,
            "armor": 10.0, "shield": 15.0, "range": 5.0, "attack_speed": 1.0,
            "move_speed": 3.5, "hp_per_level": 120.0, "energy_per_level": 50.0,
            "weapon_per_level": 8.0, "armor_per_level": 3.0, "shield_per_level": 2.0
        }))
        .unwrap();
        let data = GameData {
            units: vec![unit],
            items: vec![],
            manifest: vec![ManifestEntry {
                name: "*Lorelai*".into(),
                entity_type_id: 409,
                pair_index: 0,
                path: "build://Characters/Hero009/Lorelai.def".into(),
            }],
            rules: vec![],
            official_rules: Value::Null,
            abilities: vec![],
            combat_constants: Value::Null,
            content_digest: String::new(),
            hero_names: vec!["Lorelai".into()],
            skin_names: vec![],
        };
        let mut aral = World::new(1, 6);
        aral.use_six_human_roster().unwrap();
        aral.assign_teams(&[]).unwrap();
        aral.use_aral_lane().unwrap();
        for _ in 0..2 {
            aral.select_hero(0, "Lorelai", &data).unwrap();
            let hero = &aral.entities[0];
            assert_eq!((hero.level, hero.skill_points, hero.xp), (4, 4, 0.));
            assert_eq!(
                (hero.hp, hero.max_hp, hero.energy, hero.max_energy),
                (1060., 1060., 450., 450.)
            );
            assert_eq!(
                (hero.weapon_power(), hero.armor(), hero.shield()),
                (64., 19., 21.)
            );
        }
    }

    #[test]
    fn rank_growth_and_base_shop_are_authoritative() {
        let mut w = World::new(1, 1);
        let mut events = Vec::new();
        w.award_xp(0, 90., &mut events);
        assert_eq!(w.entities[0].level, 2);
        assert_eq!(w.entities[0].stats.weapon, 80.);
        assert!((w.entities[0].max_hp - 830.64).abs() < 0.01);
        w.command(1, Command::Learn(AbilitySlot::A)).unwrap();
        assert!(w.command(1, Command::Learn(AbilitySlot::A)).is_err());
        w.command(1, Command::Learn(AbilitySlot::B)).unwrap();
        w.entities[0].gold = 300;
        w.entities[0].position = Position { x: 10., y: 0. };
        assert!(w.command(1, Command::Buy(BasicItem::WeaponBlade)).is_err());
        w.entities[0].position = base(0);
        w.command(1, Command::Buy(BasicItem::WeaponBlade)).unwrap();
        assert_eq!(w.entities[0].gold, 0);
        assert_eq!(w.entities[0].stats.weapon, 90.);
        assert!(w.command(1, Command::Buy(BasicItem::WeaponBlade)).is_err());
    }
    #[test]
    fn roster_assignment_fills_teams_and_rejects_irreversible_partial_assignment() {
        let mut w = World::new(32, 6);
        assert!(w.assign_teams(&[1; 6]).is_err());
        assert_eq!(w.entities[0].team, 0);
        w.assign_teams(&[1, 1, 1, 1, 1, 0]).unwrap();
        assert_eq!(w.entities[..10].iter().filter(|h| h.team == 1).count(), 5);
        for h in &w.entities[..10] {
            assert_eq!(h.position.x, base(h.team).x)
        }
        w.step();
        assert!(w.assign_teams(&[]).is_err());
    }
}

#[cfg(test)]
mod double_down_tests {
    use super::*;

    #[test]
    fn aral_ringo_arms_only_once_then_consumes_on_release() {
        let mut world = World::new(7, 6);
        world.use_six_human_roster().unwrap();
        world.use_aral_lane().unwrap();
        world.entities[0].type_id = 243;
        let team = world.entities[0].team;
        let pos = Position { x: 0., y: 2. };
        let mut events = Vec::new();
        for target_team in [team, 1 - team, 1 - team] {
            let id = world.spawn(target_team, Kind::Minion, 0, pos, false);
            let index = world.index(id).unwrap();
            world.hurt(1, team, index, 100000., DamageSchool::True, &mut events);
            if target_team == team {
                assert!(!world.entities[0].status.double_down);
            }
        }
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(
                    e,
                    Event::DoubleDown {
                        attack_target: None,
                        ..
                    }
                ))
                .count(),
            1
        );
        let target = world
            .entities
            .iter()
            .find(|e| e.kind == Kind::Hero && e.team != team)
            .unwrap()
            .id;
        let index = world.index(target).unwrap();
        world.entities[0].position = pos;
        world.entities[0].stats.range = 6.;
        world.entities[0].item_stats.crit_damage = 0.25;
        world.entities[index].position = Position { x: 4., y: 2. };
        world.entities[index].stats.armor = 0.;
        let weapon = world.entities[0].stats.weapon;
        let original_hp = world.entities[index].hp;
        world.command(1, Command::Attack(target)).unwrap();
        world.step();
        world.command(1, Command::Stop).unwrap();
        world.step();
        assert!(world.entities[0].status.double_down);
        world.command(1, Command::Attack(target)).unwrap();
        let mut releases = 0;
        for _ in 0..20 {
            let events = world.step();
            if events.iter().any(
                |e| matches!(e,Event::DoubleDown{source:1,attack_target:Some(t)} if *t==target),
            ) {
                releases += 1;
                world.command(1, Command::Stop).unwrap();
            }
        }
        assert_eq!(releases, 1);
        assert!(!world.entities[0].status.double_down);
        assert!((original_hp - world.entities[index].hp - weapon * 1.75).abs() < 0.01);
    }
}
