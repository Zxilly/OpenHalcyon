use crate::Event;
use crate::Kind;
use crate::Room;
use crate::TICK_HZ;
use crate::ecs::rules::Layout;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) enum DamageSchool {
    Weapon,
    Crystal,
    True,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoldIncome {
    pub starting_balance: u32,
    pub last_hit_bounties: u64,
    pub lane_proximity_awards: u64,
    pub passive_awards: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureKills {
    pub towers: u32,
    pub crystals: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct SourceDamage {
    pub minions: f64,
    pub others: f64,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct StructureDamage {
    pub towers: SourceDamage,
    pub crystals: SourceDamage,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamMilestone {
    pub average_net_worth: f64,
    pub average_level: f64,
    pub hero_kills: u32,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamScore {
    pub structure_damage: StructureDamage,
    pub armories_destroyed: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_enemy_armory_kill_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_enemy_outer_tower_kill_seconds: Option<f64>,
    pub hero_kills: u32,
    pub turret_hero_kills: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at_five: Option<TeamMilestone>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at_ten: Option<TeamMilestone>,
    pub structure_kills: StructureKills,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structure_kills_at_five: Option<StructureKills>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structure_kills_at_ten: Option<StructureKills>,
    #[serde(skip_serializing_if = "is_zero")]
    pub blackclaw_captures: u32,
    #[serde(skip_serializing_if = "is_zero")]
    pub ghostwing_captures: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_blackclaw_capture_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_ghostwing_capture_seconds: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerScore {
    pub gold_income: GoldIncome,
    pub turret_deaths: u32,
    #[serde(skip_serializing_if = "is_zero")]
    pub scout_cam_uses: u32,
    pub damage_to_heroes: f64,
    pub damage_to_structures: f64,
    pub weapon_damage_to_heroes: f64,
    pub crystal_damage_to_heroes: f64,
    pub true_damage_to_heroes: f64,
    pub lane_last_hits: u32,
    #[serde(skip_serializing_if = "is_zero")]
    pub jungle_last_hits: u32,
    #[serde(skip_serializing_if = "is_zero")]
    pub jungle_last_hits_at_five: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lane_last_hits_at_five: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net_worth_at_five: Option<u64>,
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

impl PlayerScore {
    pub(crate) fn hit(&mut self, target: Kind, amount: f32, school: DamageSchool, lethal: bool) {
        let hp = f64::from(amount);
        match target {
            Kind::Hero => {
                self.damage_to_heroes += hp;
                match school {
                    DamageSchool::Weapon => self.weapon_damage_to_heroes += hp,
                    DamageSchool::Crystal => self.crystal_damage_to_heroes += hp,
                    DamageSchool::True => self.true_damage_to_heroes += hp,
                }
            }
            Kind::Armory | Kind::Tower | Kind::Crystal => self.damage_to_structures += hp,
            Kind::Minion if lethal => self.lane_last_hits += 1,
            Kind::Neutral if lethal => self.jungle_last_hits += 1,
            Kind::Minion | Kind::Neutral => {}
        }
    }
}

impl Room<'_> {
    pub(crate) fn native_passive_income(&mut self, events: &mut Vec<Event>) {
        let (first_second, gold, xp) = match self.layout {
            Layout::SoloMid { .. } => (40, 3, 0.6),
            Layout::HalcyonFold { .. } => (35, 4, 0.6),
            Layout::Blitz { .. } => (5, 10, 1.5),
            Layout::Aral { .. } => (30, 15, 1.8),
            Layout::Validation => return,
        };
        if self.tick < first_second * TICK_HZ {
            return;
        }
        // Heroes occupy the first seats. Their income continues while they await respawn.
        let count = self
            .entities
            .iter()
            .take_while(|e| e.kind == Kind::Hero)
            .count();
        for i in 0..count {
            let hero = &mut self.entities[i];
            let amount = gold.min(u32::MAX - hero.gold);
            hero.gold += amount;
            self.state.scores[i].gold_income.passive_awards += u64::from(amount);
            if amount > 0 {
                events.push(Event::GoldAward {
                    id: hero.id,
                    amount,
                });
            }
            self.award_xp(i, xp, events);
        }
    }

    pub fn player_score(&self, actor: u32) -> Option<&PlayerScore> {
        self.index(actor)
            .filter(|i| self.entities[*i].kind == Kind::Hero)
            .and_then(|_| self.scores.get(actor.checked_sub(1)? as usize))
    }

    pub(crate) fn record_milestone(&mut self, five: bool) {
        let mut totals = [(0u32, 0u64, 0u32); 2];
        for hero in self.entities.iter().take_while(|e| e.kind == Kind::Hero) {
            let equipment: u64 = hero
                .inventory
                .iter()
                .map(|i| self.acquisition_value(i.kind))
                .sum();
            let worth = u64::from(hero.gold) + equipment;
            let (count, net_worth, levels) = &mut totals[usize::from(hero.team)];
            *count += 1;
            *net_worth += worth;
            *levels += u32::from(hero.level);
            if five {
                let score = &mut self.state.scores[(hero.id - 1) as usize];
                score.lane_last_hits_at_five = Some(score.lane_last_hits);
                score.jungle_last_hits_at_five = score.jungle_last_hits;
                score.net_worth_at_five = Some(worth);
            }
        }
        for (score, (count, net_worth, levels)) in self.team_scores.iter_mut().zip(totals) {
            let snapshot = TeamMilestone {
                average_net_worth: net_worth as f64 / f64::from(count),
                average_level: f64::from(levels) / f64::from(count),
                hero_kills: score.hero_kills,
            };
            if five {
                score.at_five = Some(snapshot);
                score.structure_kills_at_five = Some(score.structure_kills);
            } else {
                score.at_ten = Some(snapshot);
                score.structure_kills_at_ten = Some(score.structure_kills);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::BasicItem;
    use crate::World;
    use crate::combat::SpellEffect;
    use crate::ecs::world::base;
    #[test]
    fn actual_hp_removed_is_attributed_by_source_actor_and_target_kind() {
        let mut world = World::new(13, 6);
        world.use_six_human_roster().unwrap();
        world.assign_teams(&[1, 1, 1, 0, 0, 0]).unwrap();
        let mut events = Vec::new();
        let attacker = world.entities[2].id;
        let victim = 4;
        world.entities[victim].hp = 30.;
        for (amount, school) in [
            (10., DamageSchool::Weapon),
            (11., DamageSchool::Crystal),
            (90., DamageSchool::True),
        ] {
            world.hurt(attacker, 1, victim, amount, school, &mut events);
        }
        let tower = world
            .entities
            .iter()
            .position(|e| e.kind == Kind::Tower && e.team == 0)
            .unwrap();
        world.hurt(attacker, 1, tower, 7., DamageSchool::Weapon, &mut events);
        let minion = world.spawn(0, Kind::Minion, 4, base(0), false);
        let i = world.index(minion).unwrap();
        world.hurt(attacker, 1, i, 1., DamageSchool::Weapon, &mut events);
        assert_eq!(world.player_score(attacker).unwrap().lane_last_hits, 0);
        world.hurt(attacker, 1, i, 9999., DamageSchool::Weapon, &mut events);
        world.hurt(attacker, 1, i, 9999., DamageSchool::Weapon, &mut events);
        let score = world.player_score(attacker).unwrap();
        assert_eq!(
            (
                score.damage_to_heroes,
                score.damage_to_structures,
                score.lane_last_hits
            ),
            (30., 7., 1)
        );
        assert_eq!(
            (
                score.weapon_damage_to_heroes,
                score.crystal_damage_to_heroes,
                score.true_damage_to_heroes
            ),
            (10., 11., 9.)
        );
        assert_eq!(world.player_score(1).unwrap().lane_last_hits, 0);
        assert!(world.player_score(minion).is_none());
    }

    #[test]
    fn team_milestones_use_actual_roster_and_count_the_terminal_impact() {
        for seats in [6, 10] {
            let mut w = World::new(1, seats);
            if seats == 6 {
                w.use_six_human_roster().unwrap();
            }
            w.entities[0].gold = 1001;
            w.entities[0].level = 3;
            w.purchase(1, BasicItem::SixSins).unwrap();
            let other_tower = w
                .entities
                .iter()
                .find(|e| e.kind == Kind::Tower && e.team == 1)
                .unwrap()
                .id;
            let own_tower = w
                .entities
                .iter()
                .position(|e| e.kind == Kind::Tower && e.team == 0)
                .unwrap();
            let crystal = w
                .entities
                .iter()
                .find(|e| e.kind == Kind::Crystal && e.team == 0)
                .unwrap()
                .id;
            let mut events = vec![];
            w.hurt(other_tower, 1, 0, 10000., DamageSchool::Weapon, &mut events);
            w.hurt(2, 1, 2, 10000., DamageSchool::Weapon, &mut events);
            w.tick = 300 * TICK_HZ - 1;
            w.step();
            let denom = f64::from(seats / 2);
            let sample = w.team_scores()[0].at_five.unwrap();
            assert_eq!(sample.average_net_worth, 1001. / denom);
            assert_eq!(sample.average_level, (denom + 2.) / denom);
            assert_eq!(w.team_scores()[1].at_five.unwrap().hero_kills, 1);
            assert_eq!(w.team_scores()[1].turret_hero_kills, 1);
            assert_eq!(w.player_score(1).unwrap().turret_deaths, 1);
            assert!(w.team_scores()[1].at_ten.is_none());
            let minion = w.spawn(1, Kind::Minion, 4, base(1), false);
            w.hurt(
                minion,
                1,
                own_tower,
                100000.,
                DamageSchool::Weapon,
                &mut events,
            );
            w.tick = 600 * TICK_HZ - 1;
            w.entities.spawn_spell(SpellEffect::impact(
                w.tick + 1,
                2,
                1,
                crystal,
                100000.,
                DamageSchool::Weapon,
            ));
            w.step();
            assert_eq!(w.winner, Some(1));
            let team = &w.team_scores()[1];
            assert_eq!(
                team.structure_kills_at_five.unwrap(),
                StructureKills::default()
            );
            assert_eq!(
                team.structure_kills_at_ten.unwrap(),
                StructureKills {
                    towers: 1,
                    crystals: 1
                }
            );
            assert_eq!(team.at_ten.unwrap().hero_kills, 1);
        }
    }

    #[test]
    fn structure_damage_splits_sources_and_targets_and_keeps_first_kill_time() {
        let mut w = World::new(1, 10);
        let minion = w.spawn(0, Kind::Minion, 4, base(1), false);
        let tower = w
            .entities
            .iter()
            .position(|e| e.kind == Kind::Tower && e.team == 1)
            .unwrap();
        let crystal = w
            .entities
            .iter()
            .position(|e| e.kind == Kind::Crystal && e.team == 1)
            .unwrap();
        let mut events = vec![];
        w.entities[tower].hp = 12.;
        w.entities[crystal].hp = 10.;
        w.tick = 121;
        w.hurt(minion, 0, tower, 5., DamageSchool::Weapon, &mut events);
        assert!(
            w.team_scores()[0]
                .first_enemy_outer_tower_kill_seconds
                .is_none()
        );
        w.hurt(1, 0, tower, 40., DamageSchool::Weapon, &mut events);
        w.tick = 140;
        let second = w.spawn(1, Kind::Tower, 1, base(1), false);
        let index = w.index(second).unwrap();
        w.entities[index].hp = 1.;
        w.hurt(minion, 0, index, 20., DamageSchool::Weapon, &mut events);
        w.hurt(1, 0, crystal, 2., DamageSchool::Weapon, &mut events);
        w.hurt(minion, 0, crystal, 35., DamageSchool::Weapon, &mut events);
        w.hurt(minion, 0, crystal, 35., DamageSchool::Weapon, &mut events);
        let score = &w.team_scores()[0];
        assert_eq!(score.first_enemy_outer_tower_kill_seconds, Some(6.05));
        assert_eq!(
            score.structure_damage.towers,
            SourceDamage {
                minions: 6.,
                others: 7.
            }
        );
        assert_eq!(
            score.structure_damage.crystals,
            SourceDamage {
                minions: 8.,
                others: 2.
            }
        );
        assert_eq!(
            w.team_scores()[1].structure_damage.towers,
            SourceDamage::default()
        );
    }

    #[test]
    fn jungle_farm_at_five_freezes_after_the_boundary_hit() {
        let mut world = World::new(5, 10);
        let due = 300 * TICK_HZ;
        for tick in [due, due + 1] {
            let neutral = world.spawn(
                crate::maps::native_camps::NEUTRAL_TEAM,
                Kind::Neutral,
                0,
                base(0),
                false,
            );
            world.entities.spawn_spell(SpellEffect::impact(
                tick,
                1,
                0,
                neutral,
                100000.,
                DamageSchool::Weapon,
            ));
        }
        world.tick = due - 1;
        world.step();
        assert_eq!(world.player_score(1).unwrap().jungle_last_hits_at_five, 1);
        world.step();
        let hero = world.player_score(1).unwrap();
        assert_eq!(
            (hero.jungle_last_hits, hero.jungle_last_hits_at_five),
            (2, 1)
        );
        assert_eq!(hero.lane_last_hits, 0);
        assert_eq!(world.player_score(2).unwrap().jungle_last_hits, 0);
    }

    #[test]
    fn five_minute_snapshot_includes_boundary_impacts_and_upgrade_component_once() {
        let mut world = World::new(0, 10);
        for actor in [1, 3] {
            world.entities[(actor - 1) as usize].gold = 2000;
        }
        world.purchase(1, BasicItem::WeaponBlade).unwrap();
        world.purchase(1, BasicItem::HeavySteel).unwrap();
        world.purchase(3, BasicItem::HeavySteel).unwrap();
        let minion = world.spawn(1, Kind::Minion, 4, base(1), false);
        let due = 300 * TICK_HZ;
        world.entities.spawn_spell(SpellEffect::impact(
            due,
            1,
            0,
            minion,
            100000.,
            DamageSchool::Weapon,
        ));
        world.tick = due - 2;
        world.step();
        assert!(
            serde_json::to_value(world.player_score(1))
                .unwrap()
                .get("netWorthAtFive")
                .is_none()
        );
        world.step();
        let worth = world.player_score(1).unwrap().net_worth_at_five;
        assert_eq!(worth, Some(1150 + u64::from(world.entities[0].gold)));
        assert_eq!(
            world.player_score(3).unwrap().net_worth_at_five,
            Some(1150 + u64::from(world.entities[2].gold))
        );
        assert_eq!(
            world.player_score(1).unwrap().lane_last_hits_at_five,
            Some(1)
        );
        world.entities[0].gold += 99;
        world.step();
        assert_eq!(world.player_score(1).unwrap().net_worth_at_five, worth);
        let mut early = World::new(0, 10);
        early.tick = due - 1;
        early.winner = Some(1);
        early.step();
        assert!(
            early
                .player_score(1)
                .unwrap()
                .lane_last_hits_at_five
                .is_none()
        );
    }
}

#[cfg(test)]
mod native_income_tests {
    use super::*;
    use crate::World;
    #[test]
    fn native_modes_pay_each_hero_at_the_measured_simulation_second() {
        for (aral, starting_gold, first, amount, xp) in
            [(true, 2000, 30, 15, 1.8), (false, 600, 40, 3, 0.6)]
        {
            let mut world = World::new(11, if aral { 6 } else { 1 });
            if aral {
                world.use_six_human_roster().unwrap();
                world.assign_teams(&[]).unwrap();
                world.use_aral_lane().unwrap();
            } else {
                world.use_solo_mid_lane().unwrap();
            }
            let count = if aral { 6 } else { 10 };
            world.entities[0].hp = 0.;
            world.entities[0].respawn = u64::MAX;
            let mut events = Vec::new();
            world.tick = (first - 1) * TICK_HZ;
            world.native_passive_income(&mut events);
            assert!(events.is_empty());
            world.tick += TICK_HZ;
            world.native_passive_income(&mut events);
            for hero in world.entities.iter().take(count) {
                assert_eq!(hero.gold, starting_gold + amount);
                assert_eq!(hero.xp, xp);
                assert!(events.iter().any(
                    |e| matches!(e, Event::GoldAward {id,amount:n} if *id==hero.id && *n==amount)
                ));
                assert!(
                    events.iter().any(
                        |e| matches!(e, Event::XpGain {id,amount:n} if *id==hero.id && *n==xp)
                    )
                );
            }
            let income = world.player_score(1).unwrap().gold_income;
            assert_eq!(income.starting_balance, starting_gold);
            assert_eq!(income.passive_awards, u64::from(amount));
        }
    }
}
