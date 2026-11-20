use super::*;
use halcyon_protocol::{ItemAction, ResourceChange};
use halcyon_sim::{BasicItem, Event};

/// The ARAL camera recorded other actors' transactions from either team.
fn action(world: &World, event: &Event) -> Option<(u32, Batch)> {
    match *event {
        Event::VisionDeployed { device, instance } => {
            let point = [device.point.x.to_bits(), device.point.y.to_bits()];
            let (request, mode, coordinates) = match device.kind {
                halcyon_sim::VisionDeviceKind::ScoutCam => (
                    ItemAction::VectorRequest {
                        vector_bits: [point[0], 0, point[1]],
                        argument: instance,
                        suffix: &[],
                    },
                    1,
                    point,
                ),
                halcyon_sim::VisionDeviceKind::ScoutTrap => (
                    ItemAction::Activate {
                        instance,
                        suffix: &[],
                    },
                    2,
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
        Event::FlaskActivated { actor, instance } => {
            const FLASK_MODE: u8 = 2;
            let active = ItemAction::Activation {
                actor: u32::from(wire_id(actor)),
                instance,
                target_word: u32::MAX,
                coordinate_bits: [0, 0],
                mode: FLASK_MODE,
                suffix: &[],
            };
            Some((
                actor,
                vec![
                    ItemAction::Activate {
                        instance,
                        suffix: &[],
                    }
                    .to_message()
                    .expect("native item prefix"),
                    active.to_message().expect("native activation prefix"),
                ],
            ))
        }
        Event::ItemSold(sale)
            if matches!(
                sale.sold.kind,
                BasicItem::Hourglass
                    | BasicItem::BlazingSalvo
                    | BasicItem::Stormcrown
                    | BasicItem::ScoutTrap
            ) =>
        {
            let actor = u32::from(wire_id(sale.actor));
            let mut batch = vec![
                ItemAction::Sell {
                    actor,
                    instance: sale.sold.instance,
                    suffix: &[],
                }
                .to_message()
                .expect("native sale prefix"),
            ];
            for (stat, value) in world.item_modifiers(sale.sold.kind) {
                batch.push(messages::modifier(actor, stat, -value));
            }
            let mut removal = [0; 10];
            removal[..4].copy_from_slice(&actor.to_be_bytes());
            removal[4..8].copy_from_slice(&sale.sold.instance.to_be_bytes());
            removal[8..].copy_from_slice(&1u16.to_be_bytes());
            batch.push(Message::known(Opcode::RemoveItem, removal));
            batch.push(
                ResourceChange {
                    actor,
                    value: sale.balance as f32,
                    index: 6,
                    set_mode: 1,
                    flags: [0, 0],
                }
                .message(),
            );
            Some((sale.actor, batch))
        }
        _ => None,
    }
}

/// Raw replay bodies must have the exact supported gameplay prefix and no
/// unrelated trailing data. The player writer has already checked visibility.
pub(super) fn recordable(message: &Message) -> bool {
    use halcyon_protocol::{Fields, LiveFields};
    let hero = |actor: u32| (1500..1506).contains(&actor);
    match message.fields() {
        Ok(Fields::Live(LiveFields::Item(ItemAction::Activate { instance, suffix }))) => {
            (instance == 2000 || instance >= 40000) && suffix.iter().all(|&v| v == 0)
        }
        Ok(Fields::Live(LiveFields::Item(ItemAction::Activation {
            actor,
            instance,
            target_word: u32::MAX,
            coordinate_bits: [0, 0],
            mode: 2,
            suffix,
        }))) => {
            hero(actor) && (instance == 2000 || instance >= 40000) && suffix.iter().all(|&v| v == 0)
        }
        Ok(Fields::Live(LiveFields::Item(ItemAction::VectorRequest {
            vector_bits: [x, 0, z],
            argument: 2001,
            suffix,
        }))) => {
            f32::from_bits(x).is_finite()
                && f32::from_bits(z).is_finite()
                && suffix.iter().all(|&v| v == 0)
        }
        Ok(Fields::Live(LiveFields::Item(ItemAction::Activation {
            actor,
            instance: 2001,
            target_word: u32::MAX,
            coordinate_bits: [x, z],
            mode: 1,
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
impl Room {
    pub(super) fn publish_aral_items(&mut self, events: &[Event]) {
        if !self.legacy_aral() {
            return;
        }
        for event in events {
            let Some((id, batch)) = action(&self.world, event) else {
                continue;
            };
            let Some(entity) = self.world.entities.iter().find(|e| e.id == id) else {
                continue;
            };
            for (i, (seat, viewer)) in self.seats.iter().zip(&self.world.entities).enumerate() {
                let team = viewer.team;
                if matches!(event, Event::VisionDeployed{device,..} if team!=device.team && !self.world.sees_position(team,device.point))
                    || viewer.id == id
                    || !seat.world_actor_written
                    || !seat.preamble.confirmed()
                    || !self.scene_views[i].knows(id)
                    || !self.visible[usize::from(team)].contains(&id)
                    || !self.world.visible(team, entity)
                {
                    continue;
                }
                if let Some(peer) = seat.gameplay_peer() {
                    peer.send(transport::Delivery::Private(batch.clone()));
                }
            }
            self.publish_observer_actor(id, &batch);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
                Opcode::RequestActivateItemVariantC,
            ),
            (
                Command::DeployScoutTrap {
                    instance: trap.instance,
                },
                Opcode::RequestActivateItem,
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
}
