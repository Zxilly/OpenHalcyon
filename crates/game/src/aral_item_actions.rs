use super::*;
use halcyon_protocol::{ItemAction, ItemActivationMode, ResourceChange};
use halcyon_sim::{BasicItem, Event};

/// The ARAL camera recorded other actors' transactions from either team.
fn action(world: &World, event: &Event) -> Option<(u32, Batch)> {
    match *event {
        Event::VisionDeployed { device, instance } => {
            let point = [device.point.x.to_bits(), device.point.y.to_bits()];
            let (request, mode, coordinates) = match device.kind {
                halcyon_sim::VisionDeviceKind::ScoutCam | halcyon_sim::VisionDeviceKind::Flare => (
                    ItemAction::VectorRequest {
                        vector_bits: [point[0], 0, point[1]],
                        argument: instance,
                        suffix: &[],
                    },
                    ItemActivationMode::Position,
                    point,
                ),
                halcyon_sim::VisionDeviceKind::ScoutTrap => (
                    ItemAction::Activate {
                        instance,
                        suffix: &[],
                    },
                    ItemActivationMode::Untargeted,
                    [0, 0],
                ),
            };
            let activation = ItemAction::Activation {
                actor: u32::from(wire_id(device.owner)),
                instance,
                target_word: u32::MAX,
                coordinate_bits: coordinates,
                mode,
                suffix: &[],
            };
            Some((
                device.owner,
                vec![request.to_message().ok()?, activation.to_message().ok()?],
            ))
        }
        Event::InfusionApplied {
            actor,
            item,
            effect,
            kind,
            level,
            duration_ticks,
        } if duration_ticks == 150 * halcyon_sim::TICK_HZ => {
            use halcyon_protocol::{BuffApplication, BuffDefinition, BuffPrefix};
            let definition = match kind {
                BasicItem::WeaponInfusion => BuffDefinition::WeaponInfusion,
                BasicItem::CrystalInfusion => BuffDefinition::CrystalInfusion,
                _ => return None,
            };
            let target = u32::from(wire_id(actor));
            let instance = effect.checked_add(1499)?;
            let application = BuffApplication {
                prefix: BuffPrefix::timed_self(target, instance, definition, 150.),
                extras: None,
                suffix: &[],
            }
            .to_message()
            .ok()?;
            let active = ItemAction::Activation {
                actor: target,
                instance: item,
                target_word: u32::MAX,
                coordinate_bits: [0, 0],
                mode: ItemActivationMode::Untargeted,
                suffix: &[],
            }
            .to_message()
            .ok()?;
            let mut removed = [0; 10];
            removed[..4].copy_from_slice(&target.to_be_bytes());
            removed[4..8].copy_from_slice(&item.to_be_bytes());
            removed[8..].copy_from_slice(&1u16.to_be_bytes());
            Some((
                actor,
                vec![
                    active,
                    application,
                    infusion_level(target, instance, level)?,
                    Message::known(Opcode::ConsumeItem, removed),
                ],
            ))
        }
        Event::InfusionLevel {
            actor,
            effect,
            level,
        } => Some((
            actor,
            vec![infusion_level(
                u32::from(wire_id(actor)),
                effect.checked_add(1499)?,
                level,
            )?],
        )),
        Event::InfusionReplaced { actor, effect } => Some((
            actor,
            vec![
                halcyon_protocol::BuffMutation {
                    target: u32::from(wire_id(actor)),
                    instance: effect.checked_add(1499)?,
                    change: halcyon_protocol::BuffChange::Cancel,
                    padding: &[0; 6],
                }
                .to_message()
                .ok()?,
            ],
        )),
        Event::ReflexExpired { actor, effect } => {
            let m = halcyon_protocol::BuffMutation {
                target: u32::from(wire_id(actor)),
                instance: effect.checked_add(1499)?,
                change: halcyon_protocol::BuffChange::Timeout,
                padding: &[],
            }
            .to_message()
            .ok()?;
            Some((actor, vec![m]))
        }
        Event::ReflexActivated {
            actor,
            item: instance,
            ..
        }
        | Event::FlaskActivated { actor, instance } => {
            let active = ItemAction::Activation {
                actor: u32::from(wire_id(actor)),
                instance,
                target_word: u32::MAX,
                coordinate_bits: [0, 0],
                mode: ItemActivationMode::Untargeted,
                suffix: &[],
            };
            let mut batch = vec![
                ItemAction::Activate {
                    instance,
                    suffix: &[],
                }
                .to_message()
                .ok()?,
                active.to_message().ok()?,
            ];
            if let Event::ReflexActivated {
                first_effect,
                barrier,
                ..
            } = *event
            {
                let actor = u32::from(wire_id(actor));
                let first = first_effect.checked_add(1499)?;
                for (offset, definition, duration_bits) in
                    [(0, 61, 0x4066), (1, 268, 0x3e00), (2, 18, 0x3e00)]
                {
                    if offset == 2 {
                        batch.push(
                            ResourceChange {
                                actor,
                                value: barrier,
                                index: 4,
                                set_mode: 0,
                                flags: [1, 0],
                            }
                            .message(),
                        );
                    }
                    batch.push(
                        halcyon_protocol::BuffApplication {
                            prefix: halcyon_protocol::BuffPrefix {
                                target: actor,
                                source: actor,
                                duration_bits,
                                instance: first.checked_add(offset)?,
                                definition,
                            },
                            extras: None,
                            suffix: &[],
                        }
                        .to_message()
                        .ok()?,
                    );
                }
            }
            Some((actor, batch))
        }
        Event::ItemActivated {
            actor,
            instance,
            kind,
            target,
            ..
        } if !matches!(
            kind,
            BasicItem::Aegis
                | BasicItem::Flare
                | BasicItem::FlareLoader
                | BasicItem::Flaregun
                | BasicItem::ScoutTuff
                | BasicItem::SuperScout2000
                | BasicItem::Contraption
        ) =>
        {
            let wire_actor = u32::from(wire_id(actor));
            let (request, mode, target_word, coordinate_bits) = match target {
                halcyon_sim::ItemTarget::SelfCast => (
                    ItemAction::Activate {
                        instance,
                        suffix: &[],
                    },
                    ItemActivationMode::Untargeted,
                    u32::MAX,
                    [0, 0],
                ),
                halcyon_sim::ItemTarget::Entity(tgt) => {
                    let wire_tgt = u32::from(wire_id(tgt));
                    (
                        ItemAction::TargetRequest {
                            arguments: [wire_tgt, instance],
                            suffix: &[],
                        },
                        ItemActivationMode::Target,
                        wire_tgt,
                        [0, 0],
                    )
                }
                halcyon_sim::ItemTarget::Point(point) => {
                    let coords = [point.x.to_bits(), point.y.to_bits()];
                    (
                        ItemAction::VectorRequest {
                            vector_bits: [coords[0], 0, coords[1]],
                            argument: instance,
                            suffix: &[],
                        },
                        ItemActivationMode::Position,
                        u32::MAX,
                        coords,
                    )
                }
            };
            let activation = ItemAction::Activation {
                actor: wire_actor,
                instance,
                target_word,
                coordinate_bits,
                mode,
                suffix: &[],
            };
            Some((
                actor,
                vec![request.to_message().ok()?, activation.to_message().ok()?],
            ))
        }
        Event::ItemSold(sale) => {
            let actor = u32::from(wire_id(sale.actor));
            Some((sale.actor, encode_sale(world, &sale, actor)))
        }
        _ => None,
    }
}

fn encode_sale(world: &World, sale: &halcyon_sim::ItemSale, wire_actor: u32) -> Batch {
    let mut batch = vec![
        ItemAction::Sell {
            actor: wire_actor,
            instance: sale.sold.instance,
            suffix: &[],
        }
        .to_message()
        .expect("native sale prefix"),
    ];
    for (stat, value) in world.item_modifiers(sale.sold.kind) {
        batch.push(messages::modifier(wire_actor, stat, -value));
    }
    let mut removal = [0; 10];
    removal[..4].copy_from_slice(&wire_actor.to_be_bytes());
    removal[4..8].copy_from_slice(&sale.sold.instance.to_be_bytes());
    removal[8..].copy_from_slice(&1u16.to_be_bytes());
    batch.push(Message::known(Opcode::ConsumeItem, removal));
    batch.push(
        ResourceChange {
            actor: wire_actor,
            value: sale.balance as f32,
            index: 6,
            set_mode: 1,
            flags: [0, 0],
        }
        .message(),
    );
    batch
}

pub(super) fn sell(room: &mut Room, seat: usize, actor: u32, instance: u32) -> Batch {
    let Some(entity) = room.world.entities.get(seat) else {
        return Vec::new();
    };
    let entity_id = entity.id;
    if actor != u32::from(wire_id(entity_id)) {
        return Vec::new();
    }
    let Ok(sale) = room.world.sell_item(entity_id, instance) else {
        return Vec::new();
    };
    encode_sale(&room.world, &sale, actor)
}

/// Raw replay bodies must have the exact supported gameplay prefix and no
/// unrelated trailing data. The player writer has already checked visibility.
pub(super) fn recordable(message: &Message) -> bool {
    use halcyon_protocol::{Fields, LiveFields};
    const STARTER_FLASK: u32 = halcyon_sim::AralStarter::HealingFlask.grant().instance;
    const STARTER_TOTEM: u32 = halcyon_sim::AralStarter::VisionTotem.grant().instance;
    let hero = |actor: u32| (1500..1506).contains(&actor);
    match message.fields() {
        Ok(Fields::Live(LiveFields::Item(ItemAction::Activate { instance, suffix }))) => {
            (instance == STARTER_FLASK || instance >= 40000) && suffix.iter().all(|&v| v == 0)
        }
        Ok(Fields::Live(LiveFields::Item(ItemAction::Activation {
            actor,
            instance,
            target_word: u32::MAX,
            coordinate_bits: [0, 0],
            mode: ItemActivationMode::Untargeted,
            suffix,
        }))) => {
            hero(actor)
                && (instance == STARTER_FLASK || instance >= 40000)
                && suffix.iter().all(|&v| v == 0)
        }
        Ok(Fields::Live(LiveFields::Item(ItemAction::VectorRequest {
            vector_bits: [x, 0, z],
            argument: STARTER_TOTEM,
            suffix,
        }))) => {
            f32::from_bits(x).is_finite()
                && f32::from_bits(z).is_finite()
                && suffix.iter().all(|&v| v == 0)
        }
        Ok(Fields::Live(LiveFields::Item(ItemAction::Activation {
            actor,
            instance: STARTER_TOTEM,
            target_word: u32::MAX,
            coordinate_bits: [x, z],
            mode: ItemActivationMode::Position,
            suffix,
        }))) => {
            hero(actor)
                && f32::from_bits(x).is_finite()
                && f32::from_bits(z).is_finite()
                && suffix.iter().all(|&v| v == 0)
        }
        Ok(Fields::Live(LiveFields::Item(ItemAction::Sell {
            actor,
            instance,
            suffix,
        }))) => hero(actor) && instance != 0 && suffix.iter().all(|&v| v == 0),
        _ => false,
    }
}
fn infusion_level(target: u32, instance: u32, level: u8) -> Option<Message> {
    halcyon_protocol::BuffStack {
        target,
        instance,
        count: u32::from(level),
        suffix: &[],
    }
    .to_message()
    .ok()
}
#[derive(Default)]
pub(super) struct InfusionWitness {
    active: Option<u32>,
    preceding: Option<u32>,
}
fn witnessed(view: &mut tower_aggro::View, event: &Event) -> bool {
    let slot_idx = |actor: u32| actor.checked_sub(1).map(|n| n as usize);
    match *event {
        Event::InfusionApplied { actor, effect, .. } => {
            let Some(seen) = slot_idx(actor).and_then(|idx| view.infusion_effects.get_mut(idx))
            else {
                return actor != 0;
            };
            seen.preceding = seen.active.replace(effect);
            true
        }
        Event::InfusionLevel { actor, effect, .. } => {
            let Some(seen) = slot_idx(actor).and_then(|idx| view.infusion_effects.get(idx)) else {
                return actor != 0;
            };
            seen.active == Some(effect)
        }
        Event::InfusionReplaced { actor, effect } => {
            let Some(seen) = slot_idx(actor).and_then(|idx| view.infusion_effects.get_mut(idx))
            else {
                return actor != 0;
            };
            if seen.preceding == Some(effect) {
                seen.preceding = None;
                true
            } else if seen.active == Some(effect) {
                seen.active = None;
                true
            } else {
                false
            }
        }
        Event::ReflexActivated {
            actor,
            first_effect,
            ..
        } => {
            if let Some(slot) = slot_idx(actor).and_then(|idx| view.reflex_effects.get_mut(idx)) {
                *slot = Some(first_effect + 1);
            }
            actor != 0
        }
        Event::ReflexExpired { actor, effect } => {
            let Some(slot) = slot_idx(actor).and_then(|idx| view.reflex_effects.get_mut(idx))
            else {
                return actor != 0;
            };
            slot.take() == Some(effect)
        }
        _ => true,
    }
}
impl Room {
    pub(super) fn publish_aral_items(&mut self, events: &[Event]) {
        for event in events {
            if let Event::ItemCooldownUpdated {
                actor,
                key,
                remaining_seconds,
            } = *event
            {
                let msg = Message::cooldown_adjustment(
                    u32::from(wire_id(actor)),
                    key,
                    remaining_seconds,
                    halcyon_protocol::CooldownMode::Set,
                );
                for (seat, viewer) in self.seats.iter().zip(&self.world.entities) {
                    let ready = seat.peers[1].is_some()
                        || (seat.world_actor_written && seat.preamble.confirmed());
                    if viewer.id == actor
                        && ready
                        && let Some(peer) = seat.gameplay_peer()
                    {
                        peer.send(transport::Delivery::Private(vec![msg.clone()]));
                    }
                }
                continue;
            }
            let Some((id, batch)) = action(&self.world, event) else {
                continue;
            };
            let Some(entity) = self.world.entities.iter().find(|e| e.id == id) else {
                continue;
            };
            if matches!(event, Event::ReflexExpired { .. }) && entity.hp <= 0. {
                continue;
            }
            for (i, (seat, viewer)) in self.seats.iter().zip(&self.world.entities).enumerate() {
                let team = viewer.team;
                let ch1 = seat.peers[1].is_some();
                let stream_ready = ch1
                    || (seat.world_actor_written
                        && seat.preamble.confirmed()
                        && self.scene_views[i].knows(id));
                if matches!(event, Event::VisionDeployed{device,..} if team!=device.team && !self.world.sees_position(team,device.point))
                    || (viewer.id == id
                        && !matches!(
                            event,
                            Event::ReflexActivated { .. }
                                | Event::ReflexExpired { .. }
                                | Event::InfusionApplied { .. }
                                | Event::InfusionReplaced { .. }
                                | Event::InfusionLevel { .. }
                                | Event::ItemActivated { .. }
                        ))
                    || !stream_ready
                    || !self.visible[usize::from(team)].contains(&id)
                    || !self.world.visible(team, entity)
                {
                    continue;
                }
                if let Some(peer) = seat.gameplay_peer()
                    && witnessed(&mut self.scene_views[i], event)
                {
                    // The initiating client already sent its button press.
                    let start = usize::from(
                        viewer.id == id
                            && matches!(
                                event,
                                Event::ReflexActivated { .. } | Event::ItemActivated { .. }
                            ),
                    );
                    peer.send(transport::Delivery::Private(batch[start..].to_vec()));
                }
            }
            self.publish_observer_actor_if(id, &batch, |view| witnessed(view, event));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
