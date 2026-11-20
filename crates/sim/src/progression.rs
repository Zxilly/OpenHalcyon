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

#[derive(Clone, Copy)]
struct DamageSpec {
    src: u32,
    team: u8,
    target_idx: usize,
    amount: f32,
    school: DamageSchool,
    effect: ImpactEffect,
    origin: item_actives::DamageOrigin,
}

impl Room<'_> {
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
        let layout = &self.state.layout;
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
            let reboot_hp = e.is_rebooting().then_some(e.hp);
            e.apply_growth(1);
            if let Some(hp) = reboot_hp {
                e.hp = hp;
            }
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
        if self.entities[i].hp <= 0. || self.entities[i].is_rebooting() || n <= 0. {
            return;
        }
        let tick = self.state.tick;
        let rules = self.state.shop.effect_rules.active;
        let (src_idx, adjusted) =
            item_actives::adjusted_heal_amount(&self.entities, src, i, n, tick, &rules);
        if adjusted <= 0. {
            return;
        }
        let e = &mut self.entities[i];
        let was = e.hp;
        e.hp = (was + adjusted).min(e.max_hp);
        if e.hp > was {
            events.push(Event::Heal {
                source: src,
                target: e.id,
                amount: e.hp - was,
                hp: e.hp,
            });
            item_actives::on_heal_or_barrier_applied(&mut self.entities, src_idx, i, tick, &rules);
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
        self.hurt_with_origin(
            DamageSpec {
                src,
                team,
                target_idx: i,
                amount: n,
                school,
                effect: ImpactEffect::Standard,
                origin: item_actives::DamageOrigin::Ability,
            },
            events,
        );
    }
    pub(super) fn hurt_basic(
        &mut self,
        src: u32,
        team: u8,
        i: usize,
        n: f32,
        school: DamageSchool,
        events: &mut Vec<Event>,
    ) {
        self.hurt_with_origin(
            DamageSpec {
                src,
                team,
                target_idx: i,
                amount: n,
                school,
                effect: ImpactEffect::Standard,
                origin: item_actives::DamageOrigin::BasicAttack,
            },
            events,
        );
    }
    pub(super) fn hurt_item(
        &mut self,
        src: u32,
        team: u8,
        i: usize,
        n: f32,
        school: DamageSchool,
        events: &mut Vec<Event>,
    ) {
        self.hurt_with_origin(
            DamageSpec {
                src,
                team,
                target_idx: i,
                amount: n,
                school,
                effect: ImpactEffect::Standard,
                origin: item_actives::DamageOrigin::ItemProc,
            },
            events,
        );
    }
    pub(super) fn hurt_with_effect(
        &mut self,
        src: u32,
        team: u8,
        i: usize,
        n: f32,
        (school, effect): (DamageSchool, ImpactEffect),
        events: &mut Vec<Event>,
    ) {
        self.hurt_with_origin(
            DamageSpec {
                src,
                team,
                target_idx: i,
                amount: n,
                school,
                effect,
                origin: item_actives::DamageOrigin::Ability,
            },
            events,
        );
    }
    fn hurt_with_origin(&mut self, mut spec: DamageSpec, events: &mut Vec<Event>) {
        if matches!(spec.school, DamageSchool::Crystal)
            && spec.src != self.entities[spec.target_idx].id
        {
            let mult =
                self.hero_5v5_crystal_multiplier(spec.src, self.entities[spec.target_idx].kind);
            spec.amount *= mult;
        }
        let (applied, reflection) = self.stormguard_reflection(spec.target_idx, spec.amount);
        let idx = spec.target_idx;
        spec.amount = applied;
        self.apply_damage_with_origin(spec, events);
        if let Some(reflected) = reflection {
            self.reflect_stormguard(idx, reflected, events);
        }
    }
    pub(super) fn apply_damage(
        &mut self,
        src: u32,
        team: u8,
        i: usize,
        n: f32,
        (school, effect): (DamageSchool, ImpactEffect),
        events: &mut Vec<Event>,
    ) {
        self.apply_damage_with_origin(
            DamageSpec {
                src,
                team,
                target_idx: i,
                amount: n,
                school,
                effect,
                origin: item_actives::DamageOrigin::Ability,
            },
            events,
        );
    }
    fn apply_damage_with_origin(&mut self, spec: DamageSpec, events: &mut Vec<Event>) {
        let DamageSpec {
            src,
            team,
            target_idx: i,
            amount: n,
            school,
            effect,
            origin,
        } = spec;
        let t = &self.entities[i];
        if t.hp <= 0. || n <= 0. || self.solo_structure_protected(t) || self.is_5v5_resetting(t.id)
        {
            return;
        }
        let passive_rules = self.state.shop.effect_rules.passive;
        let mut ctx = item_actives::DamageContext {
            src,
            target_idx: i,
            amount: n,
            hp_loss: 0.,
            school,
            origin,
        };
        let Some(n) = item_passives::filter_incoming_damage(
            &mut self.entities,
            ctx,
            self.state.tick,
            &passive_rules,
        ) else {
            return;
        };
        if n <= 0. {
            return;
        }
        ctx.amount = n;
        let t = &self.entities[i];
        let is_outer_tower = self.is_outer_tower(t);
        let t = &mut self.entities[i];
        let absorbed = if t.status.barrier_until > self.state.tick {
            n.min(t.status.barrier)
        } else {
            0.
        };
        t.status.barrier -= absorbed;
        let remaining = n - absorbed;
        let flask_absorbed = if self.state.tick < t.status.flask_until {
            remaining.min(t.status.flask_barrier)
        } else {
            0.
        };
        t.status.flask_barrier -= flask_absorbed;
        let remaining = remaining - flask_absorbed;
        let remaining = if let Some(ref mut active) = t.status.reflex
            && active.until > self.state.tick
        {
            let absorbed = remaining.min(active.barrier);
            active.barrier -= absorbed;
            remaining - absorbed
        } else {
            remaining
        };
        let remaining = if let Some(channel) = t.cast.as_mut().and_then(|c| c.verse.as_mut()) {
            let saved = channel.fortified.min(remaining * 0.5);
            channel.fortified -= saved;
            remaining - saved
        } else {
            item_passives::apply_slumbering_fortify(t, remaining, self.state.tick)
        };
        let remaining = if self.hero_kit_enabled() || self.entities[i].hero_kit.is_some() {
            self.kit_modify_incoming_damage(src, i, remaining, school, events)
        } else {
            remaining
        };
        let t = &mut self.entities[i];
        let amount = remaining.min(t.hp);
        if amount <= 0. {
            item_passives::on_damage_landed(self, ctx, events);
            return;
        }
        let t = &self.entities[i];
        if team < 2 && team != t.team && matches!(t.kind, Kind::Tower | Kind::Crystal) {
            // Only hero basic attacks and spells can outlive their origin actor's turn.
            // Hero and structure entities persist through death; minions attack immediately.
            let minion = self
                .index(src)
                .is_some_and(|j| self.entities[j].kind == Kind::Minion);
            let damage = &mut self.state.team_scores[usize::from(team)].structure_damage;
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
        let (id, post_kind, post_order) = {
            let t = &mut self.entities[i];
            t.hp -= amount;
            if t.kind == Kind::Armory {
                t.respawn = self.state.tick
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
                    .and_then(|seat| self.state.scores.get_mut(seat as usize))
            {
                score.hit(t.kind, amount, school, t.hp == 0.);
            }
            events.push(Event::Damage {
                effect,
                source: src,
                target: id,
                amount,
                hp: t.hp,
            });
            (id, t.kind, t.order)
        };
        ctx.hp_loss = amount;
        item_passives::on_damage_landed(self, ctx, events);
        self.on_5v5_damage(src, id, events);
        if self.entities[i].hp > 0. {
            if post_kind == Kind::Neutral && team < 2 && post_order == Command::Stop {
                self.entities[i].order = Command::Attack(src);
            }
            return;
        }
        let aral = matches!(self.state.layout, Layout::Aral { .. });
        let tick = self.state.tick;
        if aral
            && reboot::on_lethal(
                &mut self.entities[i],
                src,
                tick,
                &mut self.state.next_id,
                events,
            )
        {
            return;
        }
        let t = &mut self.entities[i];
        if t.kind == Kind::Hero {
            item_actives::clear_ephemeral_on_death(t);
        }
        let pos = t.position;
        let kind = t.kind;
        let dead_level = t.level;
        let dead_type_id = t.type_id;
        let enemy = t.team != team;
        if enemy && team < 2 {
            let state = &mut *self.state;
            let scoring = &mut state.team_scores[usize::from(team)];
            match kind {
                Kind::Tower => {
                    scoring.structure_kills.towers += 1;
                    if is_outer_tower {
                        scoring
                            .first_enemy_outer_tower_kill_seconds
                            .get_or_insert(state.tick as f64 / TICK_HZ as f64);
                    }
                }
                Kind::Crystal => scoring.structure_kills.crystals += 1,
                Kind::Armory => {
                    scoring.armories_destroyed += 1;
                    scoring
                        .first_enemy_armory_kill_seconds
                        .get_or_insert(state.tick as f64 / TICK_HZ as f64);
                }
                _ => {}
            }
        }
        let (bounty, xp) = {
            let t = &self.entities[i];
            match kind {
                Kind::Neutral => t
                    .stats
                    .physiology
                    .map_or((0, 0.), |p| (p.gold_bounty as u32, p.xp_bounty)),
                Kind::Hero => (250 + if self.state.first_blood { 0 } else { 125 }, 90.),
                Kind::Tower => (300, 60.),
                Kind::Crystal | Kind::Armory => (0, 0.),
                Kind::Minion => {
                    let template = EntityTemplate::from_raw(t.type_id);
                    let measured = if matches!(self.state.layout, Layout::SoloMid { .. }) {
                        self.state
                            .solo_lane_rewards
                            .and_then(|table| table.for_template(template?))
                    } else if matches!(self.state.layout, Layout::Aral { .. })
                        && template.is_some_and(EntityTemplate::is_aral_lane_minion)
                    {
                        // All three recorded templates pay twice their unit base before proximity sharing.
                        t.stats
                            .physiology
                            .map(|p| ((2. * p.gold_bounty) as u32, 2. * p.xp_bounty))
                    } else {
                        None
                    };
                    measured.unwrap_or(match template {
                        Some(EntityTemplate::MinionRange5v5) => (25, 6.),
                        Some(EntityTemplate::MinionSiege5v5) => (60, 12.),
                        Some(EntityTemplate::MinionCaptain5v5) => (100, 12.),
                        _ => (35, 6.),
                    })
                }
            }
        };
        let dead_team = {
            let respawn_tick = self.state.tick
                + match self.state.layout {
                    Layout::HalcyonFold { .. } => crate::map_3v3::respawn_ticks(self.state.tick),
                    Layout::Blitz { .. } => crate::map_blitz::respawn_ticks(self.state.tick),
                    _ => RESPAWN + (self.state.tick / 60).min(900),
                };
            let t = &mut self.entities[i];
            t.order = Command::Stop;
            t.winding = None;
            t.cast = None;
            t.status = Default::default();
            events.push(Event::Death { id, killer: src });
            if kind != Kind::Armory {
                t.respawn = respawn_tick;
            }
            t.team
        };
        self.entities.forget_target(i);
        if self.hero_kit_enabled() || self.entities[i].hero_kit.is_some() {
            self.kit_on_death(i);
        }
        if self.handle_5v5_neutral_death(
            crate::map_5v5::NeutralDeath5v5 {
                dead_id: id,
                dead_pos: pos,
                dead_level,
                dead_type_id,
                killer_id: src,
                killer_team: team,
                bounty,
                xp,
            },
            events,
        ) && kind == Kind::Neutral
        {
            return;
        }
        if kind == Kind::Armory {
            return;
        }
        let _ = dead_team;
        if enemy
            && matches!(self.state.layout, Layout::Aral { .. })
            && let Some(killer) = src
                .checked_sub(1)
                .and_then(|s| self.entities.get_mut(s as usize))
            && killer.id == src
            && killer.kind == Kind::Hero
            && killer.type_id == EntityTemplate::Ringo.as_raw()
            && killer.hp > 0.
            && killer.status.double_down.is_none()
            && killer.team == team
        {
            let instance = self.state.next_id;
            self.state.next_id += 1;
            killer.status.double_down = Some(instance);
            events.push(Event::DoubleDown {
                source: src,
                instance,
                attack_target: None,
            });
        }
        if kind == Kind::Crystal {
            self.winner = Some(team);
            events.push(Event::End { winner: team });
            return;
        }
        if kind == Kind::Hero && team < 2 {
            self.first_blood = true;
        }
        if self.on_halcyon_fold_lethal(i, src, team, events) {
            return;
        }
        if self.on_blitz_lethal(i, src, team, events) {
            return;
        }
        // Ten stable hero seats. A death awards its killer even if the projectile
        // travelled beyond the project assist radius after release.
        let killer_kind = self.index(src).map(|j| self.entities[j].kind);
        let hero_kill = killer_kind == Some(Kind::Hero);
        if kind == Kind::Hero && team < 2 && self.entities[i].team != team {
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
        for j in 0..self.entities.len().min(10) {
            let a = &mut self.entities[j];
            if a.team != team || a.kind != Kind::Hero {
                continue;
            }
            let nearby =
                a.hp > 0. && (a.id == src || kind != Kind::Neutral && a.position.dist(pos) <= 10.);
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
                let income = &mut self.state.scores[(a.id - 1) as usize].gold_income;
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
        if self.hero_kit_enabled() {
            let killer_idx = self.index(src);
            if let Some(kj) = killer_idx
                && kj != i
                && self.entities[kj].kind == Kind::Hero
                && self.entities[kj].hp > 0.
            {
                self.kit_on_kill_assist(kj, i, true, events);
            }
            for &aj in &allies[..count] {
                if Some(aj) != killer_idx && aj != i && self.entities[aj].hp > 0. {
                    self.kit_on_kill_assist(aj, i, false, events);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original delivery and 4.13.4 units_full.json"]
    fn supplemented_solo_minion_kills_distribute_base_gold_and_xp() {
        let base = std::env::var_os("HALCYON_DELIVERY").unwrap();
        let extra = std::env::var_os("HALCYON_DATA_EXTRA").unwrap();
        let data = GameData::load_with_extra(base, Some(std::path::Path::new(&extra))).unwrap();
        for solo in [false, true] {
            let mut w = World::from_data(1, 10, &data).unwrap();
            if solo {
                w.use_solo_mid_lane().unwrap();
            }
            let pos = Position::default();
            // Keep exactly one allied recipient within sharing distance.
            for idx in 0..10 {
                let e = &mut w.entities[idx];
                e.position.x = 50.;
            }
            w.entities[0].position = pos;
            let killer = w.entities[0].id;
            let team = w.entities[0].team;
            for (t, gold, extracted, legacy) in [
                (EntityTemplate::MinionRange5v5, 25, 6., 6.),
                (EntityTemplate::MinionMelee5v5, 35, 8., 6.),
                (EntityTemplate::MinionSiege5v5, 60, 9., 12.),
                (EntityTemplate::MinionCaptain5v5, 100, 11., 12.),
            ] {
                let id = w.spawn(1 - team, Kind::Minion, 0, pos, false);
                let idx = w.index(id).unwrap();
                w.entities[idx].type_id = t.as_raw();
                let before = w.entities[0].xp;
                let mut events = Vec::new();
                w.hurt(killer, team, idx, 10000., DamageSchool::True, &mut events);
                assert!(events.contains(&Event::GoldAward {
                    id: killer,
                    amount: gold
                }));
                assert_eq!(
                    w.entities[0].xp - before,
                    if solo { extracted } else { legacy }
                );
            }
            if solo {
                let ally = w
                    .entities
                    .iter()
                    .take(10)
                    .position(|a| a.team == team && a.id != killer)
                    .unwrap();
                w.entities[ally].position = pos;
                let ally_id = w.entities[ally].id;
                let id = w.spawn(1 - team, Kind::Minion, 0, pos, false);
                let index = w.index(id).unwrap();
                w.entities[index].type_id = EntityTemplate::MinionMelee5v5.as_raw();
                let mut events = Vec::new();
                w.hurt(killer, team, index, 10000., DamageSchool::True, &mut events);
                for id in [killer, ally_id] {
                    assert!(events.contains(&Event::XpGain { id, amount: 4.8 }));
                }
                assert!(events.contains(&Event::GoldAward {
                    id: ally_id,
                    amount: 17
                }));
            }
        }
    }

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
        use serde_json::Value;

        let unit = halcyon_data::Unit {
            name: "Lorelai".into(),
            hp: 700.,
            energy: 300.,
            weapon: 40.,
            armor: 10.,
            shield: 15.,
            range: 5.,
            attack_speed: 1.,
            move_speed: 3.5,
            hp_per_level: 120.,
            energy_per_level: 50.,
            weapon_per_level: 8.,
            armor_per_level: 3.,
            shield_per_level: 2.,
            other: Default::default(),
        };
        let data = GameData {
            extra: Default::default(),
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
        assert_eq!(
            w.entities.iter().take(10).filter(|h| h.team == 1).count(),
            5
        );
        for h in w.entities.iter().take(10) {
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
                assert!(world.entities[0].status.double_down.is_none());
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
        assert!(world.entities[0].status.double_down.is_some());
        world.command(1, Command::Attack(target)).unwrap();
        let armed = world.entities[0].status.double_down.unwrap();
        let mut releases = 0;
        for _ in 0..20 {
            let events = world.step();
            if events.iter().any(
                |e| matches!(e,Event::DoubleDown{source:1,attack_target:Some(t),instance} if *t==target && *instance==armed),
            ) {
                releases += 1;
                world.command(1, Command::Stop).unwrap();
            }
        }
        assert_eq!(releases, 1);
        assert!(world.entities[0].status.double_down.is_none());
        assert!((original_hp - world.entities[index].hp - weapon * 1.75).abs() < 0.01);
    }
}
