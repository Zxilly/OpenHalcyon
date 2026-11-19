use super::*;

#[derive(Clone, Copy, Debug, Serialize)]
pub(super) enum DamageSchool {
    Weapon,
    Crystal,
    True,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerScore {
    pub damage_to_heroes: f64,
    pub damage_to_structures: f64,
    pub weapon_damage_to_heroes: f64,
    pub crystal_damage_to_heroes: f64,
    pub true_damage_to_heroes: f64,
    pub lane_last_hits: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lane_last_hits_at_five: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net_worth_at_five: Option<u64>,
}

impl PlayerScore {
    pub(super) fn hit(&mut self, target: Kind, amount: f32, school: DamageSchool, lethal: bool) {
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
            Kind::Tower | Kind::Crystal => self.damage_to_structures += hp,
            Kind::Minion if lethal => self.lane_last_hits += 1,
            Kind::Minion => {}
        }
    }
}

impl World {
    pub fn player_score(&self, actor: u32) -> Option<&PlayerScore> {
        self.index(actor)
            .filter(|i| self.entities[*i].kind == Kind::Hero)
            .and_then(|_| self.scores.get(actor.checked_sub(1)? as usize))
    }

    pub(super) fn record_five_minutes(&mut self) {
        for hero in self.entities.iter().take_while(|e| e.kind == Kind::Hero) {
            let equipment: u64 = hero
                .inventory
                .iter()
                .map(|i| self.acquisition_value(i.kind))
                .sum();
            let score = &mut self.scores[(hero.id - 1) as usize];
            score.lane_last_hits_at_five = Some(score.lane_last_hits);
            score.net_worth_at_five = Some(u64::from(hero.gold) + equipment);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        world.effects.push(SpellEffect::impact(
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
