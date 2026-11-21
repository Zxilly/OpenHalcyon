use super::*;
use halcyon_protocol::{BuffApplication, BuffDefinition, BuffPrefix};
use halcyon_sim::KickKind;

fn kick_frames(source: u32, target: u32, instance: u32) -> [Message; 2] {
    let reveal = BuffApplication {
        prefix: BuffPrefix {
            source,
            ..BuffPrefix::timed_self(target, instance, BuffDefinition::RevealToBestowersTeam, 1.5)
        },
        extras: None,
        suffix: &[],
    };
    [
        Message::actor_bounce(target, 6., 5.),
        reveal.to_message().expect("fixed target reveal"),
    ]
}

pub(super) fn publish(
    room: &mut Room,
    source: u32,
    target: u32,
    instance: u32,
    kind: KickKind,
    events: &[Event],
) {
    let (Some(caster), Some(instance)) = (
        room.world.entities.iter().find(|e| e.id == source),
        instance.checked_add(1499),
    ) else {
        return;
    };
    let just_died = events
        .iter()
        .any(|event| matches!(event, Event::Death { id, .. } if *id == target));
    let victim_ok = match (room.world.entities.iter().find(|e| e.id == target), kind) {
        (Some(victim), KickKind::SuperKick) => {
            victim.kind == Kind::Hero && victim.team != caster.team
        }
        (Some(victim), KickKind::ElbowDrop) => {
            matches!(victim.kind, Kind::Hero | Kind::Minion) && victim.team != caster.team
        }
        // Defeated minions are removed before post-step replication, so accept an in-tick Death event.
        (None, KickKind::ElbowDrop) => just_died,
        (None, KickKind::SuperKick) => false,
    };
    let (source_wire, target_wire) = (wire_id(source), wire_id(target));
    if caster.type_id != EntityTemplate::Amael.as_raw()
        || !victim_ok
        || source_wire == u16::MAX
        || target_wire == u16::MAX
    {
        return;
    }
    let frames = kick_frames(u32::from(source_wire), u32::from(target_wire), instance);
    publish_visible(room, &[source, target], &frames);
}

pub(crate) fn recordable(m: &Message) -> bool {
    match m.fields() {
        Ok(halcyon_protocol::Fields::BuffApplication(b)) => {
            b.prefix.definition == BuffDefinition::RevealToBestowersTeam as u16
                && b.prefix.duration() == 1.5
                && b.prefix.source != b.prefix.target
                && b.prefix.instance > 1505
                && (1500..=1505).contains(&b.prefix.source)
                && (1500..u32::from(u16::MAX)).contains(&b.prefix.target)
                && b.extras.is_none()
                && b.suffix.is_empty()
        }
        Ok(halcyon_protocol::Fields::Live(halcyon_protocol::LiveFields::Motion {
            reference,
            ..
        })) => {
            (1500..u32::from(u16::MAX)).contains(&reference)
                && *m == Message::actor_bounce(reference, 6., 5.)
        }
        _ => false,
    }
}

#[cfg(test)]
#[path = "amael_tests.rs"]
mod tests;
