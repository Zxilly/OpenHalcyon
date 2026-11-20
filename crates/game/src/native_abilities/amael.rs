use super::*;
use halcyon_protocol::{BuffApplication, BuffDefinition, BuffPrefix};

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

pub(super) fn publish(room: &mut Room, source: u32, target: u32, instance: u32) {
    let (Some(caster), Some(victim), Some(instance)) = (
        source
            .checked_sub(1)
            .and_then(|i| room.world.entities.get(i as usize)),
        target
            .checked_sub(1)
            .and_then(|i| room.world.entities.get(i as usize)),
        instance.checked_add(1499),
    ) else {
        return;
    };
    if caster.id != source
        || caster.type_id != EntityTemplate::Amael.as_raw()
        || victim.id != target
        || victim.kind != Kind::Hero
        || victim.team == caster.team
    {
        return;
    }
    let frames = kick_frames(
        u32::from(wire_id(source)),
        u32::from(wire_id(target)),
        instance,
    );
    publish_visible(room, &[source, target], &frames);
}

pub(crate) fn recordable(m: &Message) -> bool {
    match m.fields() {
        Ok(halcyon_protocol::Fields::BuffApplication(b)) => {
            b.prefix.definition == BuffDefinition::RevealToBestowersTeam as u16
                && b.prefix.duration() == 1.5
                && b.prefix.source != b.prefix.target
                && b.prefix.instance > 1505
                && [b.prefix.source, b.prefix.target]
                    .into_iter()
                    .all(|id| (1500..=1505).contains(&id))
                && b.extras.is_none()
                && b.suffix.is_empty()
        }
        Ok(halcyon_protocol::Fields::Live(halcyon_protocol::LiveFields::Motion {
            reference,
            ..
        })) => (1500..=1505).contains(&reference) && *m == Message::actor_bounce(reference, 6., 5.),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Peer, Rooms, messages};
    use halcyon_platform::{MatchHost, Reservation};
    use tokio::sync::{mpsc, watch};
    use uuid::Uuid;

    #[tokio::test]
    async fn kick_requires_both_visible_declarations_on_the_same_world_stream() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                talent_levels: vec![],
                match_id: id,
                players: (0..6).map(|_| Uuid::new_v4()).collect(),
                mode: "casual_aral".into(),
                teams: vec![0, 1, 0, 1, 0, 1],
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut room = shared.lock().unwrap();
        room.world.entities[0].type_id = EntityTemplate::Amael.as_raw();
        let mut streams = Vec::new();
        for seat in room.seats.iter_mut().take(3) {
            let (tx, rx) = mpsc::channel(4);
            seat.peers[1] = Some(Peer {
                generation: 1,
                tx,
                stalled: watch::channel(false).0,
            });
            streams.push(rx);
        }
        let mut caster = messages::spawn(&room.world.entities[0], &[], None);
        let mut victim = messages::spawn(&room.world.entities[1], &[], None);
        caster.body[745] = 16;
        victim.body[745] = 17;
        for view in &mut room.scene_views[..3] {
            view.declarations(std::slice::from_ref(&caster));
        }
        for i in [0, 1] {
            room.scene_views[i].declarations(std::slice::from_ref(&victim));
        }
        room.visible[0].extend([1, 2]);
        room.visible[1].remove(&1);
        publish(&mut room, 1, 2, 6294 - 1499);
        let expected = kick_frames(1500, 1501, 6294);
        assert_eq!(streams[0].try_recv().unwrap().into_records(), expected);
        assert!(expected.iter().all(recordable));
        assert!(streams.iter_mut().all(|stream| stream.try_recv().is_err()));
    }
}
