use super::*;
use crate::{Peer, Rooms, messages};
use halcyon_protocol::{ACK_FRAME, MatchCipher, async_frame::FrameReader, hello};
use halcyon_types::{MatchHost, Reservation};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};
use tokio::time::timeout;
use uuid::Uuid;

async fn connect_observer(
    rooms: &Rooms,
    match_id: Uuid,
    cipher: &MatchCipher,
) -> (TcpStream, FrameReader) {
    let (addr, grant) = rooms.observe(match_id, Uuid::new_v4()).unwrap();
    let mut socket = TcpStream::connect(&addr).await.unwrap();
    socket
        .write_all(&hello("127.0.0.1").unwrap())
        .await
        .unwrap();
    let mut ack = [0; 5];
    socket.read_exact(&mut ack).await.unwrap();
    assert_eq!(ack, ACK_FRAME);
    let identity = Message::known(
        crate::Opcode::RegisterPlayerUuid,
        grant.to_string().as_bytes(),
    );
    socket
        .write_all(&cipher.encode(&identity).unwrap())
        .await
        .unwrap();
    let mut reader = FrameReader::new();
    while let Ok(Ok(Some(raw))) = timeout(Duration::from_millis(50), reader.next(&mut socket)).await
    {
        let _ = cipher.decode_owned(raw.into()).unwrap();
    }
    (socket, reader)
}

fn wire_kick_messages(msgs: Vec<Message>) -> Vec<Message> {
    msgs.into_iter()
        .filter(|m| {
            matches!(
                m.known_opcode(),
                Some(crate::Opcode::AutoActorBounce | crate::Opcode::ApplyBuffCompact)
            )
        })
        .map(|mut m| {
            assert_eq!(m.body.len(), 22);
            assert_eq!(&m.body[16..], &[0; 6]);
            m.body.truncate(16);
            m
        })
        .collect()
}

async fn recv_observer_messages(
    socket: &mut TcpStream,
    reader: &mut FrameReader,
    cipher: &MatchCipher,
) -> Vec<Message> {
    let mut out = Vec::new();
    while let Ok(Ok(Some(raw))) = timeout(Duration::from_millis(50), reader.next(socket)).await {
        out.push(cipher.decode_owned(raw.into()).unwrap());
    }
    out
}

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
    publish(&mut room, 1, 2, 6294 - 1499, KickKind::SuperKick, &[]);
    let expected = kick_frames(1500, 1501, 6294);
    assert_eq!(streams[0].try_recv().unwrap().into_records(), expected);
    assert!(expected.iter().all(recordable));
    assert!(streams.iter_mut().all(|stream| stream.try_recv().is_err()));

    // Ordinary Super Kick stays hero-only even when a minion is visible.
    room.world.use_aral_lane().unwrap();
    for _ in 0..16 * halcyon_sim::TICK_HZ {
        room.world.step();
    }
    let minion = room
        .world
        .entities
        .iter()
        .find(|e| e.kind == Kind::Minion && e.team == 1)
        .unwrap()
        .id;
    room.visible[0].insert(minion);
    publish(&mut room, 1, minion, 6295 - 1499, KickKind::SuperKick, &[]);
    assert!(streams.iter_mut().all(|stream| stream.try_recv().is_err()));
}

#[tokio::test]
async fn elbow_drop_delivers_per_victim_only_when_visible_and_known() {
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
    let cipher = MatchCipher::new(&id.to_string());
    let shared = rooms.room(id).unwrap();
    let (minion_visible, minion_hidden) = {
        let mut room = shared.lock().unwrap();
        room.phase(crate::RoomPhase::Running);
        room.world.use_aral_lane().unwrap();
        room.world.entities[0].type_id = EntityTemplate::Amael.as_raw();
        for _ in 0..16 * halcyon_sim::TICK_HZ {
            room.world.step();
        }
        let mut team1_minions = room
            .world
            .entities
            .iter()
            .filter(|e| e.kind == Kind::Minion && e.team == 1)
            .map(|e| e.id);
        (team1_minions.next().unwrap(), team1_minions.next().unwrap())
    };

    // Attach obs_full after the first wave spawns (knows minion_visible & minion_hidden),
    // then step 1 more second to spawn extra_minion that obs_full has not seen declared.
    let (mut obs_full, mut obs_full_rd) = connect_observer(&rooms, id, &cipher).await;
    let _drain = recv_observer_messages(&mut obs_full, &mut obs_full_rd, &cipher).await;

    let hidden_hero = 6u32;
    let extra_minion = {
        let mut room = shared.lock().unwrap();
        for _ in 0..halcyon_sim::TICK_HZ {
            room.world.step();
        }
        room.world
            .entities
            .iter()
            .rev()
            .find(|e| {
                e.kind == Kind::Minion
                    && e.team == 1
                    && e.id != minion_visible
                    && e.id != minion_hidden
            })
            .unwrap()
            .id
    };
    let (mut obs_late, mut obs_late_rd) = connect_observer(&rooms, id, &cipher).await;
    let _drain = recv_observer_messages(&mut obs_late, &mut obs_late_rd, &cipher).await;

    let mut streams = Vec::new();
    {
        let mut room = shared.lock().unwrap();
        for seat in room.seats.iter_mut().take(3) {
            let (tx, rx) = mpsc::channel(16);
            seat.peers[1] = Some(Peer {
                generation: 1,
                tx,
                stalled: watch::channel(false).0,
            });
            streams.push(rx);
        }
        let mut caster = messages::spawn(&room.world.entities[0], &[], None);
        caster.body[745] = 16;
        let mut primary = messages::spawn(&room.world.entities[1], &[], None);
        primary.body[745] = 17;
        let mut m_vis = messages::spawn(
            room.world
                .entities
                .iter()
                .find(|e| e.id == minion_visible)
                .unwrap(),
            &[],
            None,
        );
        m_vis.body[745] = 22;

        // Seat 0 knows & sees source(1), primary(2), minion_visible, and does NOT see/know hidden_hero(6), minion_hidden, or extra_minion.
        // Seat 2 knows & sees source(1), primary(2) only.
        room.scene_views[0].declarations(&[caster.clone(), primary.clone(), m_vis]);
        room.scene_views[2].declarations(&[caster, primary]);
        room.visible[0].extend([1, 2, minion_visible]);
        room.visible[0].remove(&hidden_hero);
        room.visible[0].remove(&minion_hidden);
        room.visible[0].remove(&extra_minion);
        // Enemy seat 1 does not see caster(1)
        room.visible[1].remove(&1);

        publish(&mut room, 1, 2, 7000 - 1499, KickKind::ElbowDrop, &[]);
        publish(
            &mut room,
            1,
            minion_visible,
            7001 - 1499,
            KickKind::ElbowDrop,
            &[],
        );
        publish(
            &mut room,
            1,
            hidden_hero,
            7002 - 1499,
            KickKind::ElbowDrop,
            &[],
        );
        publish(
            &mut room,
            1,
            minion_hidden,
            7003 - 1499,
            KickKind::ElbowDrop,
            &[],
        );
        publish(
            &mut room,
            1,
            extra_minion,
            7004 - 1499,
            KickKind::ElbowDrop,
            &[],
        );
    }

    let expected_primary = kick_frames(1500, u32::from(wire_id(2)), 7000);
    let expected_minion_vis = kick_frames(1500, u32::from(wire_id(minion_visible)), 7001);
    let expected_minion_hid = kick_frames(1500, u32::from(wire_id(minion_hidden)), 7003);
    let expected_extra = kick_frames(1500, u32::from(wire_id(extra_minion)), 7004);
    assert!(
        expected_primary
            .iter()
            .chain(&expected_minion_vis)
            .all(recordable)
    );

    // Seat 0 receives primary(2) + minion_visible only; zero frames for hidden_hero, minion_hidden, extra_minion.
    assert_eq!(
        streams[0].try_recv().unwrap().into_records(),
        expected_primary
    );
    assert_eq!(
        streams[0].try_recv().unwrap().into_records(),
        expected_minion_vis
    );
    assert!(streams[0].try_recv().is_err());

    // Seat 1 cannot see caster -> zero frames.
    assert!(streams[1].try_recv().is_err());

    // Seat 2 knows primary(2) only -> receives primary(2) only.
    assert_eq!(
        streams[2].try_recv().unwrap().into_records(),
        expected_primary
    );
    assert!(streams[2].try_recv().is_err());

    // obs_full knew primary(2), hidden_hero(6), minion_visible, minion_hidden, NOT extra_minion!
    let obs_full_msgs =
        wire_kick_messages(recv_observer_messages(&mut obs_full, &mut obs_full_rd, &cipher).await);
    let expected_hidden_hero = kick_frames(1500, u32::from(wire_id(hidden_hero)), 7002);
    let mut expected_obs_full = Vec::new();
    expected_obs_full.extend(expected_primary);
    expected_obs_full.extend(expected_minion_vis);
    expected_obs_full.extend(expected_hidden_hero);
    expected_obs_full.extend(expected_minion_hid);
    assert_eq!(obs_full_msgs, expected_obs_full);
    assert!(!obs_full_msgs.iter().any(|m| expected_extra.contains(m)));

    // obs_late attached after extra_minion spawned -> received all 5 victims including extra_minion.
    let obs_late_msgs =
        wire_kick_messages(recv_observer_messages(&mut obs_late, &mut obs_late_rd, &cipher).await);
    expected_obs_full.extend(expected_extra);
    assert_eq!(obs_late_msgs, expected_obs_full);
}
