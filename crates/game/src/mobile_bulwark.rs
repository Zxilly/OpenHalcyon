use crate::{Batch, Message, Opcode, Room, tower_aggro::View, transport::Delivery};
use halcyon_protocol::{BuffApplication, BuffPrefix, OrbitAction, OrbitPosition, ZoneAction};
use halcyon_sim::{Event, Position};

const GROUND: u32 = 0x3d192e1f;
const END_HEIGHT: u32 = 0x3f86630b;
fn orbit_position(p: Position, height: u32) -> OrbitPosition {
    OrbitPosition {
        x_bits: p.x.to_bits(),
        y_bits: height,
        z_bits: p.y.to_bits(),
    }
}
fn initial(source: u32, id: u32, p: Position, angle: f32, slot: u8, team: u8) -> Batch {
    let mut b = vec![0; 122];
    for (offset, word) in [
        (0, 272),
        (4, 0x34bd643e),
        (8, id),
        (12, p.x.to_bits()),
        (16, GROUND),
        (20, p.y.to_bits()),
        (24, angle.cos().to_bits()),
        (32, angle.sin().to_bits()),
        (88, 0x5937c9ff),
        (92, 0x6537c9ff),
        (112, source),
    ] {
        b[offset..offset + 4].copy_from_slice(&word.to_be_bytes());
    }
    b[97 + usize::from(team)] = 1;
    b[116..].copy_from_slice(&[slot, 1, 0, 1, 255, team + 1]);
    let mut out = vec![Message::known(Opcode::EntitySpawn, b)];
    for (i, definition) in [66, 709].into_iter().enumerate() {
        out.push(
            BuffApplication {
                prefix: BuffPrefix {
                    target: id,
                    source,
                    duration_bits: 0xbc00,
                    instance: id + 1 + i as u32,
                    definition,
                },
                extras: None,
                suffix: &[],
            }
            .to_message()
            .expect("compact buff prefix"),
        );
    }
    out.push(
        OrbitAction::Start {
            actor: id,
            unknown: [0, 0],
            center: source,
            parameter: 0x3e3270a0,
            position: orbit_position(p, GROUND),
            padding: None,
        }
        .to_message(),
    );
    out.push(Message::position(id as u16, p.x, p.y));
    out.push(
        ZoneAction::Create {
            id: id + 3,
            source,
            anchor: id,
            parameters: [5.5f32.to_bits(), (-1f32).to_bits(), angle.to_bits()],
            flag: 1,
            suffix: &[],
        }
        .to_message()
        .expect("compact circular zone"),
    );
    out
}
fn end(id: u32, p: Position) -> Batch {
    vec![
        OrbitAction::Cancel {
            actor: id,
            position: orbit_position(p, END_HEIGHT),
            padding: None,
        }
        .to_message(),
        ZoneAction::Destroy {
            id: id + 3,
            suffix: &[],
        }
        .to_message()
        .expect("compact zone destruction"),
        Message::known(
            Opcode::DestroyEntity,
            [id.to_be_bytes().as_slice(), &[0, 0]].concat(),
        ),
    ]
}

pub(super) fn publish(room: &mut Room, event: &Event) {
    let (source, anchor, position, heading) = match *event {
        Event::MobileBulwarkStart {
            source,
            anchor,
            position,
            heading,
        } => (source, anchor, position, Some(heading)),
        Event::MobileBulwarkEnd {
            source,
            anchor,
            position,
        } => (source, anchor, position, None),
        _ => return,
    };
    let Some(id) = anchor
        .checked_add(1499)
        .filter(|v| *v <= u32::from(u16::MAX))
    else {
        return;
    };
    let Some(caster) = source
        .checked_sub(1)
        .and_then(|i| room.world.entities.get(i as usize))
        .filter(|e| e.id == source && e.type_id == halcyon_data::EntityTemplate::Lyra.as_raw())
    else {
        return;
    };
    let team = caster.team;
    let audience = 1 - team;
    let batch = if let Some(angle) = heading {
        let Some(slot) = room.aral_slots.dynamic_slot(anchor, audience) else {
            return;
        };
        initial(source + 1499, id, position, angle, slot, team)
    } else {
        end(id, position)
    };
    let eligible = |view: &mut View| {
        if heading.is_some() {
            if !view.knows(source) || view.knows(anchor) {
                return false;
            }
            view.declarations(&batch[..1]);
            true
        } else {
            view.release_effect(anchor)
        }
    };
    for (i, seat) in room.seats.iter().enumerate() {
        if room.world.entities[i].team != audience
            || !(seat.world_actor_written || seat.peers[1].is_some())
        {
            continue;
        }
        let Some(peer) = seat.gameplay_peer() else {
            continue;
        };
        let visible = room.visible[audience as usize].contains(&source);
        if heading.is_some() && !visible {
            continue;
        }
        if eligible(&mut room.scene_views[i]) {
            let records = if visible {
                batch.clone()
            } else {
                batch[1..].to_vec()
            };
            peer.send(Delivery::Private(records));
        }
    }
    room.publish_observer_actor_if(source, &batch, eligible);
    if heading.is_none() {
        room.aral_slots.destroy_sent(anchor, audience);
    }
}

pub(super) fn recordable(m: &Message) -> bool {
    let b = &m.body;
    let dynamic = |id: u32| (2084..=65535).contains(&id);
    let hero = |id: u32| (1500..1506).contains(&id);
    let word = |i| u32::from_be_bytes(b[i..i + 4].try_into().unwrap());
    let finite = |value| f32::from_bits(value).is_finite();
    match m.known_opcode() {
        Some(Opcode::EntitySpawn) if b.len() == 122 => {
            word(0) == 272
                && word(4) == 0x34bd643e
                && dynamic(word(8))
                && [12, 20, 24, 32].into_iter().all(|i| finite(word(i)))
                && word(16) == GROUND
                && word(28) == 0
                && b[36..88].iter().all(|v| *v == 0)
                && word(88) == 0x5937c9ff
                && word(92) == 0x6537c9ff
                && matches!(&b[96..100], [0, 1, 0, 0] | [0, 0, 1, 0])
                && b[100..112].iter().all(|v| *v == 0)
                && hero(word(112))
                && matches!(&b[116..], [22..=199, 1, 0, 1, 255, 1 | 2])
        }
        Some(Opcode::ApplyBuffCompact) => {
            matches!(m.fields(),Ok(halcyon_protocol::Fields::BuffApplication(e)) if dynamic(e.prefix.target) && hero(e.prefix.source) && e.prefix.duration_bits==0xbc00 && e.suffix.is_empty() && matches!((e.prefix.definition,e.prefix.instance.checked_sub(e.prefix.target)),(66,Some(1))|(709,Some(2))))
        }
        Some(Opcode::AutoOrbit) => {
            matches!(m.orbit_action(),Some(OrbitAction::Start{actor,unknown:[0,0],center,parameter:0x3e3270a0,position,padding:None}) if dynamic(actor)&&hero(center)&&position.y_bits==GROUND&&finite(position.x_bits)&&finite(position.z_bits))
        }
        Some(Opcode::CancelAutoOrbit) => {
            matches!(m.orbit_action(),Some(OrbitAction::Cancel{actor,position,padding:None}) if dynamic(actor)&&position.y_bits==END_HEIGHT&&finite(position.x_bits)&&finite(position.z_bits))
        }
        Some(Opcode::CreateZoneOfControl) => {
            matches!(m.zone_action(),Some(ZoneAction::Create{id,source,anchor,parameters:[r,c,a],flag:1,suffix:[]}) if dynamic(anchor)&&id==anchor+3&&hero(source)&&r==5.5f32.to_bits()&&c==(-1f32).to_bits()&&finite(a))
        }
        Some(Opcode::DestroyZoneOfControl) => {
            matches!(m.zone_action(),Some(ZoneAction::Destroy{id,suffix:[]}) if dynamic(id.saturating_sub(3)))
        }
        _ => false,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn orbit_lifecycle_is_scoped_to_an_enemy_with_the_declared_caster() {
        use crate::{Peer, Rooms};
        use halcyon_platform::{MatchHost, Reservation};
        use tokio::sync::{mpsc, watch};
        use uuid::Uuid;
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                match_id: id,
                players: (0..6).map(|_| Uuid::new_v4()).collect(),
                mode: "casual_aral".into(),
                teams: vec![0, 1, 0, 1, 0, 1],
                difficulty: None,
                talent_levels: vec![],
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut room = shared.lock().unwrap();
        room.world.entities[0].type_id = halcyon_data::EntityTemplate::Lyra.as_raw();
        let mut hero = crate::messages::spawn(&room.world.entities[0], &room.picks, None);
        hero.body[745] = 16;
        let mut streams = Vec::new();
        for i in 0..4 {
            let (tx, rx) = mpsc::channel(8);
            streams.push(rx);
            room.seats[i].peers[1] = Some(Peer {
                generation: 1,
                tx,
                stalled: watch::channel(false).0,
            });
            if i != 3 {
                room.scene_views[i].declarations(std::slice::from_ref(&hero));
            }
        }
        room.visible[1].insert(1);
        let start = Event::MobileBulwarkStart {
            source: 1,
            anchor: 800,
            position: Position::default(),
            heading: 0.5,
        };
        publish(&mut room, &start);
        assert_eq!(streams[1].try_recv().unwrap().into_records().len(), 6);
        assert!(
            [0, 2, 3]
                .into_iter()
                .all(|i| streams[i].try_recv().is_err())
        );
        room.visible[1].remove(&1);
        let finish = Event::MobileBulwarkEnd {
            source: 1,
            anchor: 800,
            position: Position { x: 7., y: 3. },
        };
        publish(&mut room, &finish);
        let cleanup = streams[1].try_recv().unwrap().into_records();
        assert_eq!(
            cleanup.iter().map(|m| m.opcode).collect::<Vec<_>>(),
            [0x423, 0x40b]
        );
        publish(&mut room, &finish);
        assert!(streams.iter_mut().all(|rx| rx.try_recv().is_err()));
    }
    #[test]
    fn anchor_and_zone_share_heading_and_end_once_per_registered_view() {
        let start = initial(
            1502,
            4815,
            Position {
                x: -1.49413,
                y: 9.07119,
            },
            -2.79274,
            32,
            1,
        );
        assert_eq!(
            start.iter().map(|m| m.opcode).collect::<Vec<_>>(),
            [0x3f2, 0x43e, 0x43e, 0x406, 0x42e, 0x421]
        );
        assert_eq!(&start[0].body[116..], [32, 1, 0, 1, 255, 2]);
        assert_eq!(&start[0].body[112..116], &1502u32.to_be_bytes());
        let (
            Some(OrbitAction::Start {
                actor,
                center,
                position,
                ..
            }),
            Some(ZoneAction::Create {
                id,
                source,
                anchor,
                parameters,
                ..
            }),
        ) = (start[3].orbit_action(), start[5].zone_action())
        else {
            panic!("missing anchor linkage")
        };
        assert_eq!(
            (actor, center, anchor, source, id),
            (4815, 1502, 4815, 1502, 4818)
        );
        assert_eq!(position.y_bits, GROUND);
        let word = |i| f32::from_be_bytes(start[0].body[i..i + 4].try_into().unwrap());
        assert!((word(32).atan2(word(24)) - f32::from_bits(parameters[2])).abs() < 0.00001);
        let mut view = View::default();
        view.declarations(&start);
        assert_eq!(view.slot(4815 - 1499), Some(32));
        assert!(view.release_effect(4815 - 1499));
        assert!(!view.release_effect(4815 - 1499));
        let finish = end(actor, Position { x: -2., y: 3. });
        assert_eq!(
            finish.iter().map(|m| m.opcode).collect::<Vec<_>>(),
            [0x407, 0x423, 0x40b]
        );
    }
}
