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

fn is_alpha(room: &Room, actor: u32) -> bool {
    actor
        .checked_sub(1)
        .and_then(|i| room.world.entities.get(i as usize))
        .is_some_and(|e| {
            e.id == actor && e.kind == Kind::Hero && e.type_id == EntityTemplate::Alpha.as_raw()
        })
}

pub(super) fn start(room: &mut Room, actor: u32, instance: u32) {
    let Some(instance) = instance.checked_add(1499) else {
        return;
    };
    if !is_alpha(room, actor) {
        return;
    }
    let wire = u32::from(wire_id(actor));
    let effect = BuffApplication {
        prefix: BuffPrefix::timed_self(wire, instance, BuffDefinition::Rebooting, 3.5),
        extras: None,
        suffix: &[],
    }
    .to_message()
    .expect("fixed reboot effect");
    publish_visible(
        room,
        &[actor],
        &[
            halcyon_protocol::ActorUndeadBody { actor: wire }.encode(),
            effect,
        ],
    );
}

pub(super) fn publish(room: &mut Room, actor: u32, instance: u32, seconds: f32, hp: f32) {
    let Some(instance) = instance.checked_add(1499) else {
        return;
    };
    if is_alpha(room, actor) {
        publish_visible(
            room,
            &[actor],
            &recovery(u32::from(wire_id(actor)), instance, seconds, hp),
        );
    }
}

pub(crate) fn recordable(m: &Message) -> bool {
    use halcyon_protocol::Fields;
    let hero = |actor| (1500..=1505).contains(&actor);
    let recharge = |duration| matches!(duration, 65. | 75. | 85.);
    if let Some(mutation) = m.buff_mutation() {
        return matches!(mutation.change, BuffChange::Duration(seconds) if recharge(seconds))
            && hero(mutation.target)
            && mutation.instance > 1505
            && mutation.padding.is_empty();
    }
    match m.fields() {
        Ok(Fields::BuffApplication(b)) => {
            hero(b.prefix.target)
                && b.prefix.source == b.prefix.target
                && b.prefix.instance > 1505
                && b.extras.is_none()
                && b.suffix.is_empty()
                && ((b.prefix.definition == BuffDefinition::RebootCooldown as u16
                    && recharge(b.prefix.duration()))
                    || (b.prefix.definition == BuffDefinition::Rebooting as u16
                        && b.prefix.duration() == 3.5))
        }
        Ok(Fields::CooldownValue {
            owner,
            key,
            value,
            mode,
            ..
        }) if hero(owner) && m.body.len() == 13 && mode == CooldownMode::Set as u8 => {
            (key == AlphaCooldown::Reboot as u32 && recharge(value))
                || (matches!(key, k if k == AlphaCooldown::PrimeDirective as u32 || k == AlphaCooldown::CoreCharge as u32)
                    && value == 0.)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cooldown_update_names_the_same_instance_as_the_preceding_application() {
        let batch = recovery(1501, 15302, 75., 1560.);
        assert!(batch[2..].iter().all(recordable));
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
