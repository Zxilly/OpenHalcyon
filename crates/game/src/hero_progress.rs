use halcyon_protocol::Message;
use halcyon_sim::{Entity, Event, Kind, World};

use super::messages::{append_combat, wire_id};

pub(super) fn set_spawn(world: &World, message: &mut Message, actor: &Entity) {
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
        message
            .set_spawn_gold(actor.gold as f32, worth as f32)
            .expect("native full hero baseline");
    }
    let body = &mut message.body;
    body[294..298].copy_from_slice(&f32::from(actor.level).to_be_bytes());
    body[318..322].copy_from_slice(&actor.xp.to_be_bytes());
    body[322..326].copy_from_slice(&required.to_be_bytes());
    body[326..330].copy_from_slice(&0u32.to_be_bytes());
    body[705..709].copy_from_slice(&1u32.to_be_bytes());
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
                out.push(Message::entity_stat(wire_id(id), amount, 8));
            }
        }
        Event::Level { id, level } => {
            let Some(required) = world.native_xp_required(level) else {
                return false;
            };
            if previously_declared(id) {
                append_combat(out, event, |_| true);
                out.push(
                    halcyon_protocol::AttributeChange {
                        actor: u32::from(wire_id(id)),
                        source: u32::MAX,
                        value: required,
                        index: 39,
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
            let mut message = Message::known(crate::Opcode::HeroSpawn, [0; 750]);
            set_spawn(&world, &mut message, hero);
            for (offset, value) in [
                (294, if aral { 4.0_f32 } else { 1.0 }),
                (318, 0.0),
                (322, if aral { 90.0 } else { 68.0 }),
            ] {
                assert_eq!(word(&message.body, offset), value.to_bits());
            }
            assert_eq!(word(&message.body, 326), 0);
            assert_eq!(word(&message.body, 705), 1);
        }
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
        assert_eq!(word(&declared.body, 294), 2f32.to_bits());
        assert_eq!(word(&declared.body, 318), 0.18f32.to_bits());
        assert_eq!(word(&declared.body, 322), 84f32.to_bits());
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
