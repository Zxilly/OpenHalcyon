use super::{Batch, Message, Seat, messages::wire_id, tower_aggro::View, transport};
use halcyon_sim::{Event, Kind, World};

/// Native releases refer to actor IDs. The projectile record has a separate slot namespace.
pub(super) fn release(
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
    if !matches!((actor.kind, actor.type_id),
        (Kind::Tower, 300 | 301 | 340) | (Kind::Minion, 287..=289 | 335..=338))
    {
        return None;
    }
    let (a, b) = (wire_id(source), wire_id(target));
    if a == u16::MAX || b == u16::MAX {
        return None;
    }
    Some(Message::ability_target_used(u32::from(a), u32::from(b), 0))
}

pub(super) fn actors(m: &Message) -> Option<(u32, u32)> {
    if m.opcode != 0x415 || m.body.len() != 9 || m.body[8] != 0 {
        return None;
    }
    let a = u32::from_be_bytes(m.body[..4].try_into().ok()?);
    let b = u32::from_be_bytes(m.body[4..8].try_into().ok()?);
    if !(1510..65535).contains(&a) || !(1500..65535).contains(&b) || a == b {
        return None;
    }
    Some((a - 1499, b - 1499))
}

pub(super) fn registered(view: &View, message: &Message) -> bool {
    actors(message).is_none_or(|(a, b)| view.knows(a) && view.knows(b))
}

/// Each admitted connection may be at a different point in scene registration.
pub(super) fn deliver(mut out: Batch, seats: &[Seat], world: &World, views: &[View], team: usize) {
    let mut recipients = seats
        .iter()
        .zip(&world.entities)
        .zip(views)
        .filter(|((_, actor), _)| usize::from(actor.team) == team)
        .filter_map(|((seat, _), view)| {
            let ready =
                seat.world_actor_written && seat.preamble.confirmed() && seat.peers[1].is_none();
            Some((seat.gameplay_peer()?, ready, view))
        });
    let Some(first) = recipients.next() else {
        return;
    };
    let Some(second) = recipients.next() else {
        let (peer, ready, view) = first;
        out.retain(|m| can_receive(m, ready, view));
        if !out.is_empty() {
            peer.send(transport::Delivery::Private(out));
        }
        return;
    };

    let common = transport::TeamBatch::new(out);
    for (peer, ready, view) in [first, second].into_iter().chain(recipients) {
        let messages = common.messages();
        if messages.iter().all(|m| can_receive(m, ready, view)) {
            peer.send(transport::Delivery::Team(common.clone()));
        } else {
            let batch: Batch = messages
                .iter()
                .filter(|m| can_receive(m, ready, view))
                .cloned()
                .collect();
            if !batch.is_empty() {
                peer.send(transport::Delivery::Private(batch));
            }
        }
    }
}

fn can_receive(message: &Message, ready: bool, view: &View) -> bool {
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
            .iter_mut()
            .find(|e| e.kind == Kind::Hero && e.team != team)
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
                        view.declarations(&[Message::new(0x3f2, spawn)]);
                        assert!(!registered(&view, &frame));
                        spawn[8..12].copy_from_slice(&u32::from(wire_id(hid)).to_be_bytes());
                        spawn[116] = 17;
                        view.declarations(&[Message::new(0x3f2, spawn)]);
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
