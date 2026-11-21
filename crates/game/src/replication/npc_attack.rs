use super::{Batch, messages::wire_id, slots::tower_aggro::View};
use crate::abilities::legacy::ringo;
use crate::modes::aral;
use crate::transport;
use crate::{Message, Seat};
use halcyon_data::EntityTemplate;
use halcyon_sim::{Event, Kind, World};

/// Native releases refer to actor IDs. The projectile record has a separate slot namespace.
pub(crate) fn release(
    world: &World,
    event: &Event,
    visible: impl Fn(u32) -> bool,
) -> Option<Message> {
    let Event::Attack { source, target } = *event else {
        return None;
    };
    if source == target || !visible(source) || !visible(target) {
        return None;
    }
    let actor = &world.entities[world
        .entities
        .binary_search_by_key(&source, |e| e.id)
        .ok()?];
    let template = EntityTemplate::from_raw(actor.type_id)?;
    let supported = match actor.kind {
        Kind::Tower => matches!(
            template,
            EntityTemplate::Turret | EntityTemplate::VainTurret | EntityTemplate::OuterTurret5v5
        ),
        Kind::Minion => template.is_aral_lane_minion() || template.is_solo_lane_minion(),
        _ => false,
    };
    if !supported {
        return None;
    }
    let (a, b) = (wire_id(source), wire_id(target));
    if a == u16::MAX || b == u16::MAX {
        return None;
    }
    Some(Message::ability_target_used(u32::from(a), u32::from(b), 0))
}

pub(crate) fn actors(m: &Message) -> Option<(u32, u32)> {
    if !m.is(crate::Opcode::PlayTargetAbility) {
        return None;
    }
    let body = halcyon_protocol::PlayTargetAbilityBody::read_exact(&m.body).ok()?;
    if body.selector != 0
        || !(1510..65535).contains(&body.actor)
        || !(1500..65535).contains(&body.target)
        || body.actor == body.target
    {
        return None;
    }
    Some((body.actor - 1499, body.target - 1499))
}

pub(crate) fn registered(view: &View, message: &Message) -> bool {
    actors(message).is_none_or(|(a, b)| view.knows(a) && view.knows(b))
}

/// Each admitted connection may be at a different point in scene registration.
pub(crate) fn deliver(
    mut out: Batch,
    seats: &[Seat],
    world: &World,
    views: &mut [View],
    team: usize,
    facing_targets: &[Option<u32>; aral::SEATS],
) {
    if out.iter().any(|m| {
        matches!(
            m.opcode,
            crate::wire_opcode::ApplyBuffCompact | crate::wire_opcode::CancelBuff
        ) && ringo::references(m).is_some()
    }) {
        for ((seat, actor), view) in seats
            .iter()
            .zip(&world.entities)
            .zip(views.iter_mut())
            .filter(|((_, actor), _)| usize::from(actor.team) == team)
        {
            let Some(peer) = seat.gameplay_peer() else {
                continue;
            };
            let ready =
                seat.world_actor_written && seat.preamble.confirmed() && seat.peers[1].is_none();
            let mut batch = Vec::with_capacity(out.len());
            for m in &out {
                if can_receive(m, ready, view, actor.id) && ringo::track(view, m) {
                    ringo::face_attack_target(&mut batch, m, world, facing_targets, view);
                    batch.push(m.clone());
                }
            }
            if !batch.is_empty() {
                peer.send(transport::Delivery::Private(batch));
            }
        }
        return;
    }

    let mut recipients = seats
        .iter()
        .zip(&world.entities)
        .zip(views)
        .filter(|((_, actor), _)| usize::from(actor.team) == team)
        .filter_map(|((seat, actor), view)| {
            let ready =
                seat.world_actor_written && seat.preamble.confirmed() && seat.peers[1].is_none();
            Some((seat.gameplay_peer()?, ready, view, actor.id))
        });
    let Some(first) = recipients.next() else {
        return;
    };
    let Some(second) = recipients.next() else {
        let (peer, ready, view, viewer) = first;
        out.retain(|m| can_receive(m, ready, view, viewer));
        if !out.is_empty() {
            peer.send(transport::Delivery::Private(out));
        }
        return;
    };

    let common = transport::TeamBatch::new(out);
    for (peer, ready, view, viewer) in [first, second].into_iter().chain(recipients) {
        let messages = common.messages();
        if messages.iter().all(|m| can_receive(m, ready, view, viewer)) {
            peer.send(transport::Delivery::Team(common.clone()));
        } else {
            let batch: Batch = messages
                .iter()
                .filter(|m| can_receive(m, ready, view, viewer))
                .cloned()
                .collect();
            if !batch.is_empty() {
                peer.send(transport::Delivery::Private(batch));
            }
        }
    }
}

fn can_receive(message: &Message, ready: bool, view: &View, viewer: u32) -> bool {
    if let Some((actor, target)) = ringo::references(message) {
        return ready
            && viewer != actor
            && view.knows(actor)
            && target.is_none_or(|id| view.knows(id));
    }
    actors(message).is_none_or(|(source, target)| ready && view.knows(source) && view.knows(target))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authoritative_tower_release_precedes_impact_and_requires_both_declared_actors() {
        let mut world = World::new(7, 6);
        world.use_six_human_roster().unwrap();
        world.use_aral_lane().unwrap();
        let tower = world
            .entities
            .iter()
            .find(|e| world.aral_structure_slot(e.id) == Some(7))
            .unwrap();
        let (tid, pos, team) = (tower.id, tower.position, tower.team);
        let hero = world
            .entities
            .find_mut(|e| e.kind == Kind::Hero && e.team != team)
            .unwrap();
        let hid = hero.id;
        hero.position = halcyon_sim::Position {
            x: pos.x + 1.,
            y: pos.y,
        };
        let mut released = 0;
        let mut hit = false;
        for _ in 0..70 {
            for ev in world.step() {
                match ev {
                    Event::Attack { source, target } if source == tid && target == hid => {
                        let frame = release(&world, &ev, |_| true).unwrap();
                        assert_eq!(actors(&frame), Some((tid, hid)));
                        assert!(release(&world, &ev, |id| id == tid).is_none());
                        assert!(release(&world, &ev, |id| id == hid).is_none());
                        let mut view = View::default();
                        let mut spawn = [0; 122];
                        spawn[8..12].copy_from_slice(&u32::from(wire_id(tid)).to_be_bytes());
                        spawn[116] = 7;
                        view.declarations(&[Message::known(crate::Opcode::EntitySpawn, spawn)]);
                        assert!(!registered(&view, &frame));
                        spawn[8..12].copy_from_slice(&u32::from(wire_id(hid)).to_be_bytes());
                        spawn[116] = 17;
                        view.declarations(&[Message::known(crate::Opcode::EntitySpawn, spawn)]);
                        assert!(registered(&view, &frame));
                        released += 1;
                    }
                    Event::Damage { source, target, .. } if source == tid && target == hid => {
                        assert!(release(&world, &ev, |_| true).is_none());
                        assert_eq!(released, 1);
                        hit = true;
                        break;
                    }
                    _ => {}
                }
            }
            if hit {
                break;
            }
        }
        assert!(hit);
        assert!(
            release(
                &world,
                &Event::Attack {
                    source: hid,
                    target: tid
                },
                |_| true
            )
            .is_none()
        );
        assert!(
            release(
                &world,
                &Event::Attack {
                    source: tid,
                    target: tid
                },
                |_| true
            )
            .is_none()
        );
    }
}
