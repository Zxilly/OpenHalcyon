use super::*;
use halcyon_protocol::PlayPfxAtLocation;

const CHILL_WINDS_RANGE: u32 = 0xe91e17d5;
const EFFECT_HEIGHT: f32 = f32::from_bits(0x3d192e1f);

pub(super) fn publish(room: &mut Room, source: u32, position: Position, heading: Option<f32>) {
    if !room.legacy_aral() {
        return;
    }
    let Some(actor) = source
        .checked_sub(1)
        .and_then(|i| room.world.entities.get(i as usize))
        .filter(|a| a.id == source && a.type_id == EntityTemplate::Reim.as_raw())
    else {
        return;
    };
    let team = actor.team;
    let wire = u32::from(wire_id(source));
    let mut out = vec![Message::ability_target_used(wire, u32::MAX, 1)];
    if let Some(heading) = heading {
        out.push(
            PlayPfxAtLocation {
                effect_key: CHILL_WINDS_RANGE,
                actor: wire,
                position: [position.x, EFFECT_HEIGHT, position.y],
                unknown_word: heading.to_bits(),
                padding: None,
            }
            .to_message(),
        );
    }
    deliver_registered(room, source, team, None, &out);
    room.publish_observer_actor(source, &out);
}

pub(super) fn root(room: &mut Room, source: u32, target: u32, instance: u32) {
    if !room.legacy_aral() {
        return;
    }
    let Some(caster) = source
        .checked_sub(1)
        .and_then(|i| room.world.entities.get(i as usize))
        .filter(|e| e.type_id == EntityTemplate::Reim.as_raw())
    else {
        return;
    };
    let Some(wire_instance) = instance.checked_add(1499) else {
        return;
    };
    let buff = halcyon_protocol::BuffApplication {
        prefix: halcyon_protocol::BuffPrefix {
            target: u32::from(wire_id(target)),
            source: u32::from(wire_id(source)),
            duration_bits: 0x3c00,
            instance: wire_instance,
            definition: 654,
        },
        extras: None,
        suffix: &[0; 6],
    }
    .to_message()
    .expect("fixed buff fields");
    deliver_registered(
        room,
        source,
        caster.team,
        Some(target),
        std::slice::from_ref(&buff),
    );
    room.publish_observer_actors(&[source, target], &[buff]);
}
pub(in crate::native_abilities) fn recordable_pfx(m: &Message) -> bool {
    m.play_pfx_at_location().is_some_and(|p| {
        p.effect_key == CHILL_WINDS_RANGE
            && (1500..=1505).contains(&p.actor)
            && p.padding.is_none()
            && p.position[1] == EFFECT_HEIGHT
            && p.position.iter().all(|v| v.is_finite())
            && f32::from_bits(p.unknown_word).abs() <= std::f32::consts::PI
    })
}
