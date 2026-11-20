//! Native statistics columns populated from this room's authoritative simulation.
use crate::messages::wire_id;
use halcyon_protocol::{Message, PlayerStatistics, StatisticsReport, TeamStatistics};
use halcyon_sim::{Kind, World};
use std::io;

pub(crate) fn snapshot(world: &World) -> io::Result<Message> {
    let mut players = Vec::new();
    // Match the player-info stream: each actual hero in reserved seat order, including bots.
    for hero in world.entities.iter().filter(|e| e.kind == Kind::Hero) {
        let score = world
            .player_score(hero.id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing hero statistics"))?;
        let mut items = [0x0000ffff; 10];
        for (wire, held) in items.iter_mut().zip(&hero.inventory) {
            *wire = held.kind.template();
        }
        players.push(PlayerStatistics {
            player: u32::from(wire_id(hero.id)),
            team: hero.team + 1,
            slot_flag: 255,
            damage_to_heroes: score.damage_to_heroes as f32,
            weapon_damage_to_heroes: score.weapon_damage_to_heroes as f32,
            crystal_damage_to_heroes: score.crystal_damage_to_heroes as f32,
            true_damage_to_heroes: score.true_damage_to_heroes as f32,
            damage_to_structures: score.damage_to_structures as f32,
            // Project convention: awards received without the killing blow, excluding starting funds.
            ambient_gold_input: (score.gold_income.lane_proximity_awards
                + score.gold_income.passive_awards) as f32,
            lane_farm_at_five: u16::try_from(score.lane_last_hits_at_five.unwrap_or(0))
                .unwrap_or(u16::MAX),
            net_worth_at_five: u16::try_from(score.net_worth_at_five.unwrap_or(0))
                .unwrap_or(u16::MAX),
            deaths_by_turret: u8::try_from(score.turret_deaths).unwrap_or(u8::MAX),
            jungle_farm_at_five: u16::try_from(score.jungle_last_hits_at_five).unwrap_or(u16::MAX),
            counter_jungle_farm: 0,
            scout_cam_uses: u8::try_from(score.scout_cam_uses).unwrap_or(u8::MAX),
            smurf_score: 0.,
            average_interactions_per_minute_alive: 0.,
            item_build: 0,
            items,
        });
    }
    let mut ratio_inputs = [0.; 4];
    let teams = std::array::from_fn(|i| {
        let score = &world.team_scores()[i];
        let five = score.at_five.unwrap_or_default();
        let ten = score.at_ten.unwrap_or_default();
        let ten_kills = score.structure_kills_at_ten.unwrap_or_default();
        let damage = score.structure_damage;
        // Keep the local tower and crystal target sets disjoint, then combine the two teams.
        ratio_inputs[0] += damage.towers.others;
        ratio_inputs[1] += damage.towers.minions;
        ratio_inputs[2] += damage.crystals.others;
        ratio_inputs[3] += damage.crystals.minions;
        TeamStatistics {
            team: i as u8 + 1,
            average_net_worth_at_five: five.average_net_worth as f32,
            average_level_at_five: five.average_level as f32,
            average_net_worth_at_ten: ten.average_net_worth as f32,
            average_level_at_ten: ten.average_level as f32,
            hero_kills_at_five: u8::try_from(five.hero_kills).unwrap_or(u8::MAX),
            hero_kills_at_ten: u8::try_from(ten.hero_kills).unwrap_or(u8::MAX),
            first_armory_kill_time: score.first_enemy_armory_kill_seconds.unwrap_or(0.) as f32,
            first_outer_turret_kill_time: score.first_enemy_outer_tower_kill_seconds.unwrap_or(0.)
                as f32,
            structure_kills_at_ten: u8::try_from(ten_kills.towers + ten_kills.crystals)
                .unwrap_or(u8::MAX),
            structure_kills: u8::try_from(
                score.structure_kills.towers + score.structure_kills.crystals,
            )
            .unwrap_or(u8::MAX),
            first_blackclaw_capture_time: score.first_blackclaw_capture_seconds.unwrap_or(0.)
                as f32,
            first_ghostwing_capture_time: score.first_ghostwing_capture_seconds.unwrap_or(0.)
                as f32,
            blackclaw_captures: u8::try_from(score.blackclaw_captures).unwrap_or(u8::MAX),
            ghostwing_captures: u8::try_from(score.ghostwing_captures).unwrap_or(u8::MAX),
            ..Default::default()
        }
    });
    StatisticsReport {
        players,
        teams,
        auxiliary_words: [0; 19],
        minion_damage_ratio_inputs: ratio_inputs.map(|v| (v as f32).to_bits()),
    }
    .encode()
}

#[cfg(test)]
mod tests {
    use super::*;
    use halcyon_protocol::Fields;
    use halcyon_sim::BasicItem;

    #[test]
    fn player_info_actor_ids_items_and_teams_follow_the_real_roster() {
        for humans in [1, 6] {
            let mut world = World::new(17, humans);
            if humans == 6 {
                world.use_six_human_roster().unwrap();
                world.assign_teams(&[1, 1, 1, 0, 0, 0]).unwrap();
            }
            world.entities[0].gold = 1000;
            world.purchase(1, BasicItem::SixSins).unwrap();
            let msg = snapshot(&world).unwrap();
            let Fields::MatchStatistics(report) = msg.fields().unwrap() else {
                panic!()
            };
            for (seat, hero) in world
                .entities
                .iter()
                .filter(|e| e.kind == Kind::Hero)
                .enumerate()
            {
                let row = report.slot(seat).unwrap();
                assert_eq!(
                    (row.player, row.team, row.slot_flag),
                    (u32::from(wire_id(hero.id)), hero.team + 1, 255)
                );
            }
            let used = if humans == 6 { 6 } else { 10 };
            assert_eq!(report.slot(used).unwrap().player, u32::MAX);
            let row = report.slot(0).unwrap();
            assert_eq!(row.items[0], BasicItem::SixSins.template());
            assert_eq!(row.items[1..], [0xffff; 9]);
            assert_eq!(report.auxiliary_words(), [0; 19]);
        }
    }

    #[test]
    fn report_counts_only_authorized_scout_cam_deployments_in_the_owning_seat() {
        let mut world = World::new(17, 6);
        world.use_six_human_roster().unwrap();
        world.use_aral_lane().unwrap();
        let point = world.entities[0].position;
        let totem = halcyon_sim::AralStarter::VisionTotem.grant();
        let deploy = halcyon_sim::Command::DeployVisionTotem {
            instance: totem.instance,
            point,
        };
        world.command(1, deploy).unwrap();
        world.command(1, deploy).unwrap();
        assert!(world.command(1, deploy).is_err());
        let trap = world.purchase(1, BasicItem::ScoutTrap).unwrap().granted;
        world
            .command(
                1,
                halcyon_sim::Command::DeployScoutTrap {
                    instance: trap.instance,
                },
            )
            .unwrap();
        world
            .command(
                2,
                halcyon_sim::Command::DeployVisionTotem {
                    instance: totem.instance,
                    point: world.entities[1].position,
                },
            )
            .unwrap();
        assert_eq!(world.player_score(1).unwrap().scout_cam_uses, 2);
        let message = snapshot(&world).unwrap();
        let Fields::MatchStatistics(report) = message.fields().unwrap() else {
            panic!("expected match statistics");
        };
        assert_eq!(report.slot(0).unwrap().scout_cam_uses, 2);
        assert_eq!(report.slot(1).unwrap().scout_cam_uses, 1);
        assert_eq!(report.slot(2).unwrap().scout_cam_uses, 0);
    }

    #[test]
    fn completed_match_reports_its_actual_damage_and_crystal_loss() {
        let mut world = World::new(17, 0);
        while world.winner.is_none() && world.tick < 100_000 {
            world.step();
        }
        let winner = usize::from(world.winner.expect("crystal defeat"));
        let msg = snapshot(&world).unwrap();
        let Fields::MatchStatistics(report) = msg.fields().unwrap() else {
            panic!()
        };
        let team = report.team(winner).unwrap();
        let truth = &world.team_scores()[winner];
        assert_eq!(
            u32::from(team.structure_kills),
            truth.structure_kills.towers + 1
        );
        let sums = report.minion_damage_ratio_inputs().map(f32::from_bits);
        assert!(sums[2] + sums[3] > 0.);
        assert_eq!(
            team.first_outer_turret_kill_time,
            truth.first_enemy_outer_tower_kill_seconds.unwrap_or(0.) as f32
        );
        assert!((0..10).any(|i| report.slot(i).unwrap().damage_to_heroes > 0.));
    }
}
