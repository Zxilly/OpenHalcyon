use super::*;
use crate::Opcode;
use crate::host::manager::Rooms;
use halcyon_types::{MatchHost, Reservation};
use uuid::Uuid;
#[test]
fn infusion_replacement_and_level_require_the_original_scene_instance() {
    let mut seen = tower_aggro::View::default();
    let mut late = tower_aggro::View::default();
    let apply = |effect| Event::InfusionApplied {
        actor: 1,
        item: 40000,
        effect,
        kind: BasicItem::WeaponInfusion,
        level: 8,
        duration_ticks: 150 * halcyon_sim::TICK_HZ,
    };
    let level = |effect| Event::InfusionLevel {
        actor: 1,
        effect,
        level: 9,
    };
    assert!(witnessed(&mut seen, &apply(5)));
    assert!(!witnessed(&mut late, &level(5)));
    for view in [&mut seen, &mut late] {
        assert!(witnessed(view, &apply(6)));
    }
    let removed = Event::InfusionReplaced {
        actor: 1,
        effect: 5,
    };
    assert!(witnessed(&mut seen, &removed));
    assert!(!witnessed(&mut seen, &removed));
    assert!(!witnessed(&mut late, &removed));
    assert!(!witnessed(&mut seen, &level(5)));
    assert!(witnessed(&mut seen, &level(6)));
}
#[test]
fn reflex_vectors_match_the_item_transaction_fixture() {
    let (_, batch) = action(
        &World::new(1, 6),
        &Event::ReflexActivated {
            actor: 6,
            item: 2006,
            first_effect: 0x27e2 - 1499,
            barrier: 509.09,
        },
    )
    .unwrap();
    for (frame, expected) in [
        (&batch[0], "0448000007d60000"),
        (
            &batch[1],
            "044c000005e1000007d6ffffffff00000000000000000200",
        ),
        (
            &batch[3],
            "043e000005e1000005e13e00000027e3010c000000000000",
        ),
    ] {
        let mut bytes = frame.opcode.to_be_bytes().to_vec();
        bytes.extend_from_slice(&frame.body);
        bytes.resize(bytes.len().next_multiple_of(8), 0);
        let actual: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(actual, expected);
    }
}
#[test]
fn authoritative_cam_and_trap_choose_distinct_witness_prefixes() {
    use halcyon_sim::{Command, Position};
    let mut world = World::new(9, 6);
    world.use_six_human_roster().unwrap();
    world.use_aral_lane().unwrap();
    let trap = world.purchase(1, BasicItem::ScoutTrap).unwrap().granted;
    let here = world.entities[0].position;
    for (command, expected) in [
        (
            Command::DeployVisionTotem {
                instance: 2001,
                point: Position {
                    x: here.x + 1.,
                    ..here
                },
            },
            Opcode::RequestActivateItemPoint,
        ),
        (
            Command::DeployScoutTrap {
                instance: trap.instance,
            },
            Opcode::RequestActivateItemSelf,
        ),
    ] {
        world.command(1, command).unwrap();
        let events = world.step();
        let (owner, batch) = events.iter().find_map(|e| action(&world, e)).unwrap();
        assert_eq!(owner, 1);
        assert_eq!(batch[0].opcode, expected as u16);
        assert_eq!(batch[1].opcode, Opcode::ItemActivated as u16);
        assert!(batch.iter().all(recordable));
    }
}

#[test]
fn replay_allows_only_supported_witness_payloads() {
    let world = World::new(4, 1);
    let (_, mut batch) = action(
        &world,
        &Event::FlaskActivated {
            actor: 1,
            instance: 2000,
        },
    )
    .unwrap();
    assert!(batch.iter().all(recordable));
    batch[1].body[12] = 0x5a;
    assert!(!recordable(&batch[1]));
    batch[0].body.extend([0x50, 0]);
    assert!(!recordable(&batch[0]));
    let held = halcyon_sim::HeldItem {
        kind: BasicItem::Hourglass,
        instance: 2100,
    };
    let (_, sale) = action(
        &world,
        &Event::ItemSold(halcyon_sim::ItemSale {
            actor: 1,
            sold: held,
            refund: 125,
            balance: 1400,
        }),
    )
    .unwrap();
    assert!(recordable(&sale[0]));
    let mut unexpected = sale[0].clone();
    unexpected.body[..4].copy_from_slice(&4000u32.to_be_bytes());
    assert!(!recordable(&unexpected));
}

#[test]
fn active_item_events_encode_0x0448_0x0449_0x044a_0x044c_and_cooldown_0x0428() {
    let world = World::new(7, 6);
    let (_, self_cast) = action(
        &world,
        &Event::ItemActivated {
            actor: 1,
            instance: 40001,
            kind: BasicItem::Crucible,
            target: halcyon_sim::ItemTarget::SelfCast,
            cooldown_ticks: 1500,
        },
    )
    .unwrap();
    assert_eq!(self_cast[0].opcode, Opcode::RequestActivateItemSelf as u16);
    assert_eq!(self_cast[1].opcode, Opcode::ItemActivated as u16);

    let (_, target_cast) = action(
        &world,
        &Event::ItemActivated {
            actor: 1,
            instance: 40002,
            kind: BasicItem::TeleportBoots,
            target: halcyon_sim::ItemTarget::Entity(3),
            cooldown_ticks: 3000,
        },
    )
    .unwrap();
    assert_eq!(
        target_cast[0].opcode,
        Opcode::RequestActivateItemTarget as u16
    );
    assert_eq!(target_cast[1].opcode, Opcode::ItemActivated as u16);

    let (_, point_cast) = action(
        &world,
        &Event::ItemActivated {
            actor: 1,
            instance: 40003,
            kind: BasicItem::WarTreads,
            target: halcyon_sim::ItemTarget::Point(halcyon_sim::Position { x: 10., y: 20. }),
            cooldown_ticks: 1200,
        },
    )
    .unwrap();
    assert_eq!(
        point_cast[0].opcode,
        Opcode::RequestActivateItemPoint as u16
    );
    assert_eq!(point_cast[1].opcode, Opcode::ItemActivated as u16);

    let flask_cd = Message::cooldown_adjustment(
        u32::from(wire_id(1)),
        0x1b18_4adf,
        120.0,
        halcyon_protocol::CooldownMode::Set,
    );
    assert_eq!(flask_cd.opcode, Opcode::ModifyCooldown as u16);
    let storm_cd = Message::cooldown_adjustment(
        u32::from(wire_id(1)),
        0xdb16_1949,
        6.0,
        halcyon_protocol::CooldownMode::Set,
    );
    assert_eq!(storm_cd.opcode, Opcode::ModifyCooldown as u16);
}

#[test]
fn all_76_item_sales_encode_0x044d_modifiers_0x044b_and_gold_0x041d() {
    let mut world = World::new(9, 10);
    world.use_catalog_store(halcyon_data::items::StoreSettings::Five);
    for (idx, item) in [
        BasicItem::WeaponBlade,
        BasicItem::Sorrowblade,
        BasicItem::Aegis,
        BasicItem::Crucible,
        BasicItem::FountainOfRenewal,
        BasicItem::AtlasPauldron,
        BasicItem::TeleportBoots,
        BasicItem::BreakingPoint,
        BasicItem::DragonsEye,
        BasicItem::Clockwork,
        BasicItem::Aftershock,
        BasicItem::EveOfHarvest,
        BasicItem::SerpentMask,
        BasicItem::SlumberingHusk,
        BasicItem::SeraphicShell,
        BasicItem::Spellsword,
        BasicItem::Bonesaw,
        BasicItem::BrokenMyth,
        BasicItem::Pulseweave,
        BasicItem::TornadoTrigger,
        BasicItem::TensionBow,
        BasicItem::Shiversteel,
        BasicItem::CapacitorPlate,
        BasicItem::RooksDecree,
        BasicItem::FlareLoader,
        BasicItem::SuperScout2000,
    ]
    .into_iter()
    .enumerate()
    {
        let expected_mods = world.item_modifiers(item).count();
        let sale = halcyon_sim::ItemSale {
            actor: 1,
            sold: halcyon_sim::HeldItem {
                kind: item,
                instance: 3000 + idx as u32,
            },
            refund: 100,
            balance: 5000,
        };
        let (owner, batch) = action(&world, &Event::ItemSold(sale)).unwrap();
        assert_eq!(owner, 1);
        assert_eq!(batch[0].opcode, Opcode::SellItem as u16);
        assert_eq!(batch.len(), 1 + expected_mods + 2);
        assert_eq!(batch[batch.len() - 2].opcode, Opcode::ConsumeItem as u16);
        assert_eq!(
            batch[batch.len() - 1].opcode,
            Opcode::ModifyActorResource as u16
        );
    }
}

#[tokio::test]
async fn publish_aral_items_delivers_ch1_sales_reflex_infusions_and_cooldowns() {
    let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
    let id = Uuid::new_v4();
    let players: Vec<_> = (0..10).map(|_| Uuid::new_v4()).collect();
    rooms
        .reserve(Reservation {
            match_id: id,
            mode: "5v5_pvp_casual".into(),
            players,
            teams: vec![0, 0, 0, 0, 0, 1, 1, 1, 1, 1],
            difficulty: None,
            talent_levels: Vec::new(),
        })
        .unwrap();
    let shared = rooms.room(id).unwrap();
    let mut room = shared.lock().unwrap();
    let mut rx0 = {
        let (tx, rx) = tokio::sync::mpsc::channel(32);
        room.seats[0].peers[1] = Some(crate::Peer {
            generation: 1,
            tx,
            stalled: tokio::sync::watch::channel(false).0,
        });
        rx
    };
    let mut rx1 = {
        let (tx, rx) = tokio::sync::mpsc::channel(32);
        room.seats[1].peers[1] = Some(crate::Peer {
            generation: 2,
            tx,
            stalled: tokio::sync::watch::channel(false).0,
        });
        rx
    };
    let mut rx6 = {
        let (tx, rx) = tokio::sync::mpsc::channel(32);
        room.seats[6].peers[1] = Some(crate::Peer {
            generation: 3,
            tx,
            stalled: tokio::sync::watch::channel(false).0,
        });
        rx
    };
    room.phase(crate::RoomPhase::Running);
    let ally0_id = room.world.entities[0].id;
    let ally1_id = room.world.entities[1].id;
    let seat7_id = room.world.entities[6].id;
    for visible in &mut room.visible {
        visible.insert(ally0_id);
        visible.insert(ally1_id);
        visible.insert(seat7_id);
    }
    for seat_idx in [0usize, 1, 6] {
        room.world.entities[seat_idx].gold = 50_000;
    }
    let inf = room
        .world
        .purchase(ally0_id, BasicItem::WeaponInfusion)
        .unwrap()
        .granted;
    let blade = room
        .world
        .purchase(ally0_id, BasicItem::WeaponBlade)
        .unwrap()
        .granted;
    let aegis_seat7 = room
        .world
        .purchase(seat7_id, BasicItem::Aegis)
        .unwrap()
        .granted;

    // Direct sell helper rejects wrong wire actor and sells cleanly for the owner.
    assert!(sell(&mut room, 0, u32::from(wire_id(ally1_id)), blade.instance).is_empty());
    let sold_batch = sell(&mut room, 0, u32::from(wire_id(ally0_id)), blade.instance);
    assert_eq!(sold_batch[0].opcode, Opcode::SellItem as u16);

    room.world
        .command(
            ally0_id,
            halcyon_sim::Command::ConsumeInfusion {
                instance: inf.instance,
            },
        )
        .unwrap();
    room.world
        .command(
            seat7_id,
            halcyon_sim::Command::ActivateItem {
                instance: aegis_seat7.instance,
                target: halcyon_sim::ItemTarget::SelfCast,
            },
        )
        .unwrap();
    let events = vec![
        Event::ItemCooldownUpdated {
            actor: ally0_id,
            key: 0xdb16_1949,
            remaining_seconds: 6.0,
        },
        Event::ItemSold(halcyon_sim::ItemSale {
            actor: ally0_id,
            sold: blade,
            refund: 150,
            balance: 49_850,
        }),
        Event::InfusionApplied {
            actor: ally0_id,
            item: inf.instance,
            effect: 90,
            kind: BasicItem::WeaponInfusion,
            level: 1,
            duration_ticks: 150 * halcyon_sim::TICK_HZ,
        },
        Event::ReflexActivated {
            actor: seat7_id,
            item: aegis_seat7.instance,
            first_effect: 91,
            barrier: 100.0,
        },
    ];
    room.publish_aral_items(&events);

    let drain = |rx: &mut tokio::sync::mpsc::Receiver<transport::Delivery>| -> Vec<u16> {
        let mut ops = Vec::new();
        while let Ok(transport::Delivery::Private(batch)) = rx.try_recv() {
            ops.extend(batch.into_iter().map(|m| m.opcode));
        }
        ops
    };
    let ops0 = drain(&mut rx0);
    let ops1 = drain(&mut rx1);
    let ops6 = drain(&mut rx6);
    assert!(ops0.contains(&(Opcode::ModifyCooldown as u16)));
    assert!(ops0.contains(&(Opcode::ConsumeItem as u16)));
    assert!(!ops0.contains(&(Opcode::SellItem as u16)));
    assert!(ops1.contains(&(Opcode::SellItem as u16)));
    assert!(ops1.contains(&(Opcode::ApplyBuffCompact as u16)));
    assert!(ops6.contains(&(Opcode::ItemActivated as u16)));
    assert!(ops6.contains(&(Opcode::ApplyBuffCompact as u16)));
}
