use halcyon_protocol::{AttributeChange, HeroSpawnBody, Message, ResourceChange};
use halcyon_sim::{Entity, Event, Kind, World};

use super::messages::{append_combat, wire_id};

const RESOURCE_XP: u8 = 8;
const ATTRIBUTE_XP_REQUIRED: u8 = 39;

pub(super) fn set_spawn(world: &World, body: &mut HeroSpawnBody, actor: &Entity) {
    if actor.kind != Kind::Hero {
        return;
    }
    let Some(required) = world.native_xp_required(actor.level) else {
        return;
    };
    if let Some(score) = world.player_score(actor.id) {
        let income = score.gold_income;
        let worth = u64::from(income.starting_balance)
            + income.passive_awards
            + income.last_hit_bounties
            + income.lane_proximity_awards;
        body.set_gold(actor.gold as f32, worth as f32)
            .expect("native full hero baseline");
    }
    body.level = f32::from(actor.level);
    body.skill_points = f32::from(actor.skill_points);
    body.xp = actor.xp;
    body.xp_required = required;
    body.alt_stat_mask = 0;
    body.active_flag = HeroSpawnBody::ACTIVE_FLAG;
}

/// Newly declared heroes already have their post-tick XP and level in 0x03F3.
pub(super) fn append(
    world: &World,
    out: &mut Vec<Message>,
    event: &Event,
    previously_declared: impl Fn(u32) -> bool,
) -> bool {
    match *event {
        Event::XpGain { id, amount } => {
            if previously_declared(id) {
                out.push(
                    ResourceChange {
                        actor: u32::from(wire_id(id)),
                        value: amount,
                        index: RESOURCE_XP,
                        set_mode: 0,
                        flags: [0, 0],
                    }
                    .message(),
                );
            }
        }
        Event::Level { id, level } => {
            let Some(required) = world.native_xp_required(level) else {
                return false;
            };
            if previously_declared(id) {
                append_combat(out, event, |_| true);
                out.push(
                    AttributeChange {
                        actor: u32::from(wire_id(id)),
                        source: u32::MAX,
                        value: required,
                        index: ATTRIBUTE_XP_REQUIRED,
                        layer: 0,
                        set_mode: 1,
                    }
                    .message(),
                );
            }
        }
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Peer, Rooms, transport::Delivery};
    use halcyon_platform::{MatchHost, Reservation};
    use tokio::sync::{mpsc, watch};
    use uuid::Uuid;

    const SPAWN_LEVEL_OFFSET: usize = 294;
    const SPAWN_SKILL_POINTS_OFFSET: usize = 314;
    const SPAWN_XP_OFFSET: usize = 318;
    const SPAWN_XP_REQUIRED_OFFSET: usize = 322;
    const SPAWN_STAT_MASK_OFFSET: usize = 326;

    fn word(body: &[u8], offset: usize) -> u32 {
        u32::from_be_bytes(body[offset..offset + 4].try_into().unwrap())
    }

    #[test]
    fn native_snapshot_progress_uses_present_authority() {
        for aral in [false, true] {
            let mut world = World::new(7, if aral { 6 } else { 1 });
            if aral {
                world.use_six_human_roster().unwrap();
                world.use_aral_lane().unwrap();
            } else {
                world.use_solo_mid_lane().unwrap();
            }
            let hero = &world.entities[0];
            let mut spawn = HeroSpawnBody::default();
            set_spawn(&world, &mut spawn, hero);
            let message = spawn.encode();
            for (offset, value) in [
                (294, if aral { 4.0_f32 } else { 1.0 }),
                (314, if aral { 4.0 } else { 1.0 }),
                (318, 0.0),
                (322, if aral { 90.0 } else { 68.0 }),
            ] {
                assert_eq!(word(&message.body, offset), value.to_bits());
            }
            assert_eq!(word(&message.body, 326), 0);
            assert_eq!(word(&message.body, 705), 1);

            let mut spent = hero.clone();
            spent.skill_points = 0;
            let mut spent_spawn = HeroSpawnBody::default();
            set_spawn(&world, &mut spent_spawn, &spent);
            let spent_message = spent_spawn.encode();
            assert_eq!(
                word(&spent_message.body, SPAWN_SKILL_POINTS_OFFSET),
                0.0_f32.to_bits()
            );
            assert_eq!(word(&spent_message.body, SPAWN_STAT_MASK_OFFSET), 0);
        }
    }

    #[test]
    fn level_one_learn_and_level_up_sync_spawn_skill_points() {
        let mut world = World::new(11, 1);
        world.use_solo_mid_lane().unwrap();
        let hero_id = world.entities[0].id;
        assert_eq!(world.entities[0].level, 1);
        assert_eq!(world.entities[0].skill_points, 1);

        let mut spawn_body = HeroSpawnBody::default();
        set_spawn(&world, &mut spawn_body, &world.entities[0]);
        let spawn = spawn_body.encode();
        assert_eq!(
            word(&spawn.body, SPAWN_SKILL_POINTS_OFFSET),
            1.0_f32.to_bits()
        );
        assert_eq!(word(&spawn.body, SPAWN_STAT_MASK_OFFSET), 0);

        world
            .command(
                hero_id,
                halcyon_sim::Command::Learn(halcyon_sim::AbilitySlot::A),
            )
            .unwrap();
        let _ = world.step();
        assert_eq!(world.entities[0].skill_points, 0);

        let mut after_body = HeroSpawnBody::default();
        set_spawn(&world, &mut after_body, &world.entities[0]);
        let after_learn = after_body.encode();
        assert_eq!(
            word(&after_learn.body, SPAWN_SKILL_POINTS_OFFSET),
            0.0_f32.to_bits()
        );
        assert_eq!(word(&after_learn.body, SPAWN_STAT_MASK_OFFSET), 0);
    }

    #[tokio::test]
    async fn xp_deltas_follow_baselines_without_double_leveling() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: id,
                players: vec![Uuid::new_v4()],
                teams: vec![0],
                mode: "5v5_bots_solo".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut room = shared.lock().unwrap();
        room.world.use_solo_mid_lane().unwrap();
        room.world.entities[0].level = 2;
        room.world.entities[0].xp = 0.18;
        room.seats[0].control_world = true;
        let (tx, mut rx) = mpsc::channel(8);
        room.seats[0].peers[1] = Some(Peer {
            generation: 1,
            tx,
            stalled: watch::channel(false).0,
        });
        let target = |m: &Message| word(&m.body, 0) == 1500;
        let first = [
            Event::XpGain { id: 1, amount: 1.8 },
            Event::Level { id: 1, level: 2 },
        ];
        room.replicate(&first);
        let Delivery::Private(out) = rx.try_recv().unwrap() else {
            panic!()
        };
        let declared = out
            .iter()
            .find(|m| m.is(crate::Opcode::HeroSpawn) && word(&m.body, 8) == 1500)
            .unwrap();
        assert_eq!(word(&declared.body, SPAWN_LEVEL_OFFSET), 2f32.to_bits());
        assert_eq!(
            word(&declared.body, SPAWN_SKILL_POINTS_OFFSET),
            1f32.to_bits()
        );
        assert_eq!(word(&declared.body, SPAWN_XP_OFFSET), 0.18f32.to_bits());
        assert_eq!(
            word(&declared.body, SPAWN_XP_REQUIRED_OFFSET),
            84f32.to_bits()
        );
        assert_eq!(word(&declared.body, SPAWN_STAT_MASK_OFFSET), 0);
        assert!(!out.iter().any(|m| matches!(
            m.opcode,
            crate::wire_opcode::ModifyActorResource
                | crate::wire_opcode::LevelUp
                | crate::wire_opcode::ModifyActorAttribute
        ) && target(m)));

        room.world.entities[0].level = 3;
        room.replicate(&[
            Event::XpGain { id: 1, amount: 90. },
            Event::Level { id: 1, level: 3 },
        ]);
        let Delivery::Private(out) = rx.try_recv().unwrap() else {
            panic!()
        };
        let relevant: Vec<_> = out
            .into_iter()
            .filter(|m| {
                matches!(
                    m.opcode,
                    crate::wire_opcode::ModifyActorResource
                        | crate::wire_opcode::LevelUp
                        | crate::wire_opcode::ModifyActorAttribute
                ) && target(m)
            })
            .collect();
        assert_eq!(
            relevant.iter().map(|m| m.opcode).collect::<Vec<_>>(),
            [
                crate::wire_opcode::ModifyActorResource,
                crate::wire_opcode::LevelUp,
                crate::wire_opcode::ModifyActorAttribute
            ]
        );
        assert_eq!(word(&relevant[0].body, 4), 90f32.to_bits());
        assert_eq!(relevant[0].body[8], 8);
        assert_eq!(word(&relevant[1].body, 4), 1);
        assert_eq!(word(&relevant[2].body, 4), u32::MAX);
        assert_eq!(word(&relevant[2].body, 8), 100f32.to_bits());
        assert_eq!(relevant[2].body[12..], [39, 0, 1, 0, 0, 0, 0, 0, 0, 0]);
    }
}
