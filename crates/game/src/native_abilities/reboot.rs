use super::*;
use halcyon_protocol::{
    BuffApplication, BuffChange, BuffDefinition, BuffMutation, BuffPrefix, CooldownMode,
    ResourceChange,
};

#[repr(u32)]
enum AlphaCooldown {
    PrimeDirective = 0xb5dc2aea,
    CoreCharge = 0xb4dc2957,
    Reboot = 0xcd8619d8,
}

fn recovery(actor: u32, instance: u32, seconds: f32, hp: f32) -> Batch {
    let absolute_resource = |index, value| {
        ResourceChange {
            actor,
            value,
            index,
            set_mode: 1,
            flags: [1, 0],
        }
        .message()
    };
    let mut batch = vec![absolute_resource(0, hp), absolute_resource(3, 0.)];
    for (key, value) in [
        (AlphaCooldown::PrimeDirective, 0.),
        (AlphaCooldown::CoreCharge, 0.),
        (AlphaCooldown::Reboot, seconds),
    ] {
        batch.push(Message::cooldown_adjustment(
            actor,
            key as u32,
            value,
            CooldownMode::Set,
        ));
    }
    batch.push(
        BuffApplication {
            prefix: BuffPrefix::timed_self(
                actor,
                instance,
                BuffDefinition::RebootCooldown,
                seconds,
            ),
            extras: None,
            suffix: &[],
        }
        .to_message()
        .expect("fixed cooldown application"),
    );
    batch.push(
        BuffMutation {
            target: actor,
            instance,
            change: BuffChange::Duration(seconds),
            padding: &[],
        }
        .to_message()
        .expect("fixed cooldown duration"),
    );
    batch
}

pub(super) fn publish(room: &mut Room, actor: u32, instance: u32, seconds: f32, hp: f32) {
    let Some(instance) = instance.checked_add(1499) else {
        return;
    };
    if !room
        .world
        .entities
        .iter()
        .take(room.seats.len())
        .any(|e| e.id == actor && e.type_id == EntityTemplate::Alpha.as_raw())
    {
        return;
    }
    let batch = recovery(u32::from(wire_id(actor)), instance, seconds, hp);
    let mut shared = None;
    for (index, seat) in room.seats.iter().enumerate() {
        let team = usize::from(room.world.entities[index].team);
        if room.scene_views[index].knows(actor)
            && room.visible[team].contains(&actor)
            && (seat.world_actor_written || seat.peers[1].is_some())
            && let Some(peer) = seat.gameplay_peer()
        {
            peer.send(Delivery::Team(
                shared
                    .get_or_insert_with(|| super::super::transport::TeamBatch::new(batch.clone()))
                    .clone(),
            ));
        }
    }
    room.publish_observer_actor(actor, &batch);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cooldown_update_names_the_same_instance_as_the_preceding_application() {
        let batch = recovery(1501, 15302, 75., 1560.);
        let application = match batch[5].fields().unwrap() {
            halcyon_protocol::Fields::BuffApplication(application) => application,
            other => panic!("{other:?}"),
        };
        assert_eq!(application.prefix.duration_bits, 0x54b0);
        assert_eq!(
            (application.prefix.definition, application.prefix.instance),
            (684, 15302)
        );
        assert_eq!(
            batch[6].buff_mutation(),
            Some(BuffMutation {
                target: 1501,
                instance: 15302,
                change: BuffChange::Duration(75.),
                padding: &[]
            })
        );
        assert_eq!(
            batch[4],
            Message::cooldown_adjustment(1501, 0xcd8619d8, 75., CooldownMode::Set)
        );
    }
}
