use crate::replication::slots::tower_aggro;
use crate::replication::{Batch, messages, messages::wire_id};
use crate::{Message, Room, transport};
use halcyon_protocol::{ItemAction, ItemActivationMode, ResourceChange};
use halcyon_sim::{BasicItem, Event, World};

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
            Some((
                actor,
                vec![
                    active,
                    application,
                    infusion_level(target, instance, level)?,
                    halcyon_protocol::ConsumeItemBody {
                        actor: target,
                        instance: item,
                        flags: 1,
                    }
                    .encode(),
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
    batch.push(
        halcyon_protocol::ConsumeItemBody {
            actor: wire_actor,
            instance: sale.sold.instance,
            flags: 1,
        }
        .encode(),
    );
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

pub(crate) fn sell(room: &mut Room, seat: usize, actor: u32, instance: u32) -> Batch {
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
pub(crate) fn recordable(message: &Message) -> bool {
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
pub(crate) struct InfusionWitness {
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
    pub(crate) fn publish_aral_items(&mut self, events: &[Event]) {
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
#[path = "item_actions_tests.rs"]
mod tests;
