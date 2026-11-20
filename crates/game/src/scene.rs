//! Native scene negotiation on an authenticated control stream.
pub(crate) mod mode_variables;
use super::{Batch, Message, Room, messages};
#[derive(Default)]
pub(super) struct Preamble {
    stage: usize,
    implicit: bool,
}

impl Preamble {
    const OPS: [u16; 3] = [
        crate::wire_opcode::RequestScene,
        crate::wire_opcode::SceneOwner,
        crate::wire_opcode::SceneConfirmed,
    ];

    pub fn observe(&mut self, message: &Message, actor: u32) {
        let known = match message.known_opcode() {
            Some(crate::Opcode::RequestScene | crate::Opcode::SceneConfirmed) => {
                message.body == [0; 6]
            }
            Some(crate::Opcode::SceneOwner) => {
                let body = &message.body;
                body.len() == 6 && body[..4] == actor.to_be_bytes() && body[4..] == [1, 0]
            }
            _ => false,
        };
        if known && Self::OPS.get(self.stage) == Some(&message.opcode) {
            self.stage += 1;
        }
    }

    pub fn confirm_world_input(&mut self) {
        debug_assert!(self.scene_requested() && !self.confirmed());
        self.stage = Self::OPS.len();
        self.implicit = true;
    }
    pub fn take_late_confirmation(&mut self, m: &Message) -> bool {
        m.is(crate::Opcode::SceneConfirmed)
            && m.body == [0; 6]
            && std::mem::take(&mut self.implicit)
    }
    pub fn scene_requested(&self) -> bool {
        self.stage >= 2
    }

    pub fn confirmed(&self) -> bool {
        self.stage == Self::OPS.len()
    }
}

/// An already connected draft seat can observe a different reserved seat's first registration.
pub(super) fn announce_aral_registration(room: &Room, entering: usize) {
    let mut recipients = room
        .seats
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != entering)
        .filter_map(|(_, seat)| seat.peers[0].as_ref())
        .peekable();
    if recipients.peek().is_none() {
        return;
    }
    let mut body = [0; 64];
    let id = room.players[entering].as_hyphenated();
    id.encode_lower(&mut body[..36]);
    let notice =
        super::transport::TeamBatch::new(vec![Message::known(crate::Opcode::PlayerUuid, body)]);
    for peer in recipients {
        peer.send(super::transport::Delivery::Team(notice.clone()));
    }
}

/// An ARAL connection already in the scene can observe another seat joining it.
pub(super) fn announce_aral_entry(room: &Room, entering: usize, registration: &Message) {
    let mut recipients = room
        .seats
        .iter()
        .enumerate()
        .filter(|(index, seat)| {
            *index != entering && seat.world_actor_written && seat.preamble.scene_requested()
        })
        .filter_map(|(_, seat)| seat.peers[0].as_ref())
        .peekable();
    if recipients.peek().is_none() {
        return;
    }
    let mut handle = [0; 78];
    let name = messages::GUEST_HANDLE.as_bytes();
    handle[..name.len()].copy_from_slice(name);
    handle[64..68].copy_from_slice(&(entering as u32).to_be_bytes());
    let actor = messages::wire_id(room.world.entities[entering].id);
    handle[68..72].copy_from_slice(&u32::from(actor).to_be_bytes());
    let batch = super::transport::TeamBatch::new(vec![
        Message::known(crate::Opcode::PlayerHandle, handle),
        Message::known(crate::Opcode::RequestScene, [0; 6]),
        registration.clone(),
    ]);
    for peer in recipients {
        peer.send(super::transport::Delivery::Team(batch.clone()));
    }
}

pub(super) fn players(room: &Room) -> impl Iterator<Item = Message> {
    (0..room.picks.len()).map(|i| messages::info(&room.world, &room.players, &room.picks, i, true))
}
pub(super) fn begin_loading(room: &Room) -> Batch {
    players(room)
        .chain(std::iter::once(Message::known(
            crate::Opcode::BeginLoading,
            [0],
        )))
        .collect()
}

pub(super) fn solo_table(room: &Room) -> Message {
    let mut body = [0u8; 96];
    for (row, actor) in body
        .as_chunks_mut::<6>()
        .0
        .iter_mut()
        .zip(room.world.entities.iter().take(room.picks.len()))
    {
        row[..4].copy_from_slice(&u32::from(messages::wire_id(actor.id)).to_be_bytes());
        row[4] = 1;
        row[5] = u8::from(actor.bot);
    }
    Message::known(crate::Opcode::SceneReadiness, body)
}
pub(super) fn solo_opening(room: &mut Room, seat: usize) -> std::io::Result<Batch> {
    let team = room.world.entities[seat].team;
    let initial = room.phase == crate::RoomPhase::Loading && room.world.tick == 0;
    if initial {
        room.world
            .use_solo_mid_lane()
            .map_err(std::io::Error::other)?;
        if let Some(data) = &room.data {
            room.world
                .install_map_data(data)
                .map_err(std::io::Error::other)?;
        }
    }
    let mut out = vec![
        messages::scene_mode(&room.queue, 1),
        Message::known(crate::Opcode::SceneTransition, [0]),
    ];
    out.extend(players(room));
    out.push(super::aral::clock(room.world.tick));
    room.visible[team as usize].clear();
    if !initial {
        room.solo_slots.retain_living(&room.world.entities);
    }
    for e in &room.world.entities {
        if e.hp <= 0. {
            continue;
        }
        let visible = room.world.visible(team, e);
        // The recorded initial scene declares all ten starting heroes. On reconnect
        // live opposing positions are subject to the team's usual visibility.
        if (initial && e.kind == halcyon_sim::Kind::Hero) || (!initial && visible) {
            let Some(message) =
                room.solo_slots
                    .declaration(&room.world, e, &room.picks, room.data.as_deref())
            else {
                continue;
            };
            out.push(message);
            if initial {
                super::native_cooldown::opening(&mut out, e, false);
            }
            messages::append_inventory(&mut out, &room.world, e);
            if visible {
                room.visible[team as usize].insert(e.id);
            }
        }
    }
    if initial {
        out.extend(mode_variables::Opening::Solo.messages());
    }
    out.extend(room.recommended_paths(Some(team)));
    Ok(out)
}

/// First authoritative tick, before stepping the pristine world. Mark only structures
/// actually sent to this team; normal replication handles everything else.
pub(super) fn initial_structures(room: &mut Room) {
    let Some(peer) = room.seats[0].gameplay_peer() else {
        return;
    };
    let team = room.world.entities[0].team;
    let known = &mut room.visible[usize::from(team)];
    let mut batch = Vec::new();
    for entity in &room.world.entities {
        if !known.contains(&entity.id)
            && room.world.visible(team, entity)
            && let Some(message) = super::compact::structure(room.world.tick, entity)
        {
            known.insert(entity.id);
            batch.push(message);
        }
    }
    if !batch.is_empty() {
        room.scene_views[0].declarations(&batch);
        peer.send(super::transport::Delivery::Private(batch));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Rooms, transport::Delivery};
    use halcyon_platform::{MatchHost, Reservation};
    use std::time::{Duration, Instant};
    use tokio::sync::{mpsc, watch};
    use uuid::Uuid;

    #[tokio::test]
    async fn aral_draft_registers_new_seats_once_to_connected_players() {
        use halcyon_protocol::{ACK_FRAME, Fields, MatchCipher, hello};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpStream;
        use tokio::time::timeout;

        async fn login(address: &str, player: Uuid, cipher: &MatchCipher) -> TcpStream {
            let mut socket = TcpStream::connect(address).await.unwrap();
            socket
                .write_all(&hello("127.0.0.1").unwrap())
                .await
                .unwrap();
            let mut ack = [0; 5];
            socket.read_exact(&mut ack).await.unwrap();
            assert_eq!(ack, ACK_FRAME);
            let identity = Message::known(crate::Opcode::PlayerUuid, player.to_string().as_bytes());
            socket
                .write_all(&cipher.encode(&identity).unwrap())
                .await
                .unwrap();
            socket
        }
        async fn read_until(socket: &mut TcpStream, cipher: &MatchCipher, end: u16) -> Batch {
            timeout(Duration::from_secs(3), async {
                let mut messages = Vec::new();
                loop {
                    let body = crate::transport::frame(socket).await.unwrap();
                    let message = cipher.decode_owned(body).unwrap();
                    let done = message.opcode == end;
                    messages.push(message);
                    if done {
                        return messages;
                    }
                }
            })
            .await
            .unwrap()
        }
        async fn registrations(socket: &mut TcpStream, cipher: &MatchCipher) -> Batch {
            let barrier = Message::known(crate::Opcode::ClockSync, [0; 6]);
            socket
                .write_all(&cipher.encode(&barrier).unwrap())
                .await
                .unwrap();
            read_until(socket, cipher, 0)
                .await
                .into_iter()
                .filter(|m| m.is(crate::Opcode::PlayerUuid))
                .collect()
        }
        fn assert_notice(messages: Batch, expected: Uuid) {
            assert_eq!(messages.len(), 1);
            let notice = &messages[0];
            assert_eq!(
                notice.fields().unwrap(),
                Fields::PlayerUuid(&expected.to_string())
            );
            assert_eq!(notice.body.len(), 70);
            assert_eq!(notice.body[36..], [0; 34]);
        }

        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let room_id = Uuid::new_v4();
        let players: Vec<_> = (0..6).map(|_| Uuid::new_v4()).collect();
        let address = rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: room_id,
                players: players.clone(),
                teams: Vec::new(),
                mode: "casual_aral".into(),
                difficulty: None,
            })
            .unwrap()
            .address;
        let cipher = MatchCipher::new(&room_id.to_string());
        let mut a = login(&address, players[0], &cipher).await;
        read_until(&mut a, &cipher, crate::wire_opcode::SnapshotAral).await;
        let mut b = login(&address, players[1], &cipher).await;
        assert!(
            read_until(&mut b, &cipher, crate::wire_opcode::SnapshotAral)
                .await
                .iter()
                .all(|m| !m.is(crate::Opcode::PlayerUuid))
        );
        assert_notice(registrations(&mut a, &cipher).await, players[1]);
        assert!(registrations(&mut b, &cipher).await.is_empty());

        for denied in [players[1], Uuid::new_v4()] {
            let mut socket = login(&address, denied, &cipher).await;
            assert_eq!(
                timeout(Duration::from_secs(3), socket.read(&mut [0; 1]))
                    .await
                    .unwrap()
                    .unwrap(),
                0,
            );
        }
        assert!(registrations(&mut a, &cipher).await.is_empty());

        assert!(rooms.observe(room_id, Uuid::new_v4()).is_err());
        b.shutdown().await.unwrap();
        timeout(Duration::from_secs(3), b.read_to_end(&mut Vec::new()))
            .await
            .unwrap()
            .unwrap();
        let mut b = login(&address, players[1], &cipher).await;
        read_until(&mut b, &cipher, crate::wire_opcode::SnapshotAral).await;
        assert!(registrations(&mut a, &cipher).await.is_empty());

        let mut c = login(&address, players[2], &cipher).await;
        read_until(&mut c, &cipher, crate::wire_opcode::SnapshotAral).await;
        assert_notice(registrations(&mut a, &cipher).await, players[2]);
        assert_notice(registrations(&mut b, &cipher).await, players[2]);
        assert!(registrations(&mut c, &cipher).await.is_empty());
    }

    #[tokio::test]
    async fn solo_promotes_only_the_requesting_socket_and_reconnects_with_fog() {
        use halcyon_protocol::{ACK_FRAME, Fields, MatchCipher, hello};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpStream;
        async fn send(socket: &mut TcpStream, cipher: &MatchCipher, m: Message) {
            socket.write_all(&cipher.encode(&m).unwrap()).await.unwrap();
        }
        async fn next(socket: &mut TcpStream, cipher: &MatchCipher) -> Message {
            let b = tokio::time::timeout(Duration::from_secs(2), crate::transport::frame(socket))
                .await
                .unwrap()
                .unwrap();
            cipher.decode_owned(b).unwrap()
        }
        async fn login(address: &str, player: Uuid, cipher: &MatchCipher) -> TcpStream {
            let mut socket = TcpStream::connect(address).await.unwrap();
            socket
                .write_all(&hello("127.0.0.1").unwrap())
                .await
                .unwrap();
            let mut ack = [0; 5];
            socket.read_exact(&mut ack).await.unwrap();
            assert_eq!(ack, ACK_FRAME);
            send(
                &mut socket,
                cipher,
                Message::known(crate::Opcode::PlayerUuid, player.to_string().as_bytes()),
            )
            .await;
            socket
        }
        async fn until(socket: &mut TcpStream, cipher: &MatchCipher, op: u16) -> Message {
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    let m = next(socket, cipher).await;
                    if m.opcode == op {
                        return m;
                    }
                }
            })
            .await
            .unwrap()
        }
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("replay");
        rooms.set_replay_directory(&root).unwrap();
        let player = Uuid::new_v4();
        let id = Uuid::new_v4();
        let cipher = MatchCipher::new(&id.to_string());
        let hosted = rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: id,
                players: vec![player],
                teams: vec![0],
                mode: "5v5_bots_solo".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut socket = login(&hosted.address, player, &cipher).await;
        until(&mut socket, &cipher, crate::wire_opcode::Snapshot5V5).await;
        send(
            &mut socket,
            &cipher,
            Message::known(crate::Opcode::LockSelection, [0; 6]),
        )
        .await;
        until(&mut socket, &cipher, crate::wire_opcode::LockSelection).await;
        shared.lock().unwrap().phase_start = Instant::now() - Duration::from_secs(8);
        until(&mut socket, &cipher, crate::wire_opcode::BeginLoading).await;
        for reconnect in [false, true] {
            send(
                &mut socket,
                &cipher,
                Message::known(crate::Opcode::RequestScene, [0; 6]),
            )
            .await;
            // A different player's request and an early confirmation cannot enter the world.
            send(
                &mut socket,
                &cipher,
                Message::known(crate::Opcode::SceneOwner, [0, 0, 5, 221, 1, 0]),
            )
            .await;
            send(
                &mut socket,
                &cipher,
                Message::known(crate::Opcode::SceneConfirmed, [0; 6]),
            )
            .await;
            send(
                &mut socket,
                &cipher,
                Message::known(crate::Opcode::SceneOwner, [0, 0, 5, 220, 1, 0]),
            )
            .await;
            let mode = next(&mut socket, &cipher).await;
            assert_eq!(
                (mode.opcode, mode.body[68]),
                (crate::wire_opcode::ModeName, 1)
            );
            assert_eq!(
                next(&mut socket, &cipher).await.opcode,
                crate::wire_opcode::SceneTransition
            );
            for _ in 0..10 {
                assert_eq!(
                    next(&mut socket, &cipher).await.opcode,
                    crate::wire_opcode::PlayerInfo
                );
            }
            assert_eq!(
                next(&mut socket, &cipher).await.opcode,
                crate::wire_opcode::OpeningWorldClock
            );
            let mut hero_ids = Vec::new();
            let mut variables = Vec::new();
            let mut full_structures = Vec::new();
            let mut compact_structures = Vec::new();
            loop {
                let m = next(&mut socket, &cipher).await;
                if m.is(crate::Opcode::RequestScene) {
                    break;
                }
                if m.is(crate::Opcode::Snapshot) {
                    compact_structures.push(u32::from_be_bytes(m.body[..4].try_into().unwrap()));
                }
                if m.is(crate::Opcode::ModifyGameModeVar) {
                    variables.push(m.clone());
                }
                if m.is(crate::Opcode::EntitySpawn) {
                    let template = u16::from_be_bytes(m.body[2..4].try_into().unwrap());
                    if matches!(template, 295 | 296 | 339..=341) {
                        full_structures.push(template);
                        if reconnect {
                            let actor = u32::from_be_bytes(m.body[8..12].try_into().unwrap());
                            let expected = match actor {
                                2008 => (8, 3000f32),
                                2009 => (9, 3000.),
                                2010 => (10, 3000.),
                                2046 => (24, 4000.),
                                2051 => (25, 5000.),
                                2060 => (26, 3800.),
                                2065 => (27, 5000.),
                                2074 => (28, 4000.),
                                2079 => (29, 5000.),
                                _ => panic!("unexpected friendly tower {actor}"),
                            };
                            assert_eq!(m.body[745], expected.0);
                            assert_eq!(m.body[42..46], expected.1.to_be_bytes());
                        }
                    }
                    let id = u16::from_be_bytes(m.body[10..12].try_into().unwrap());
                    if (1500..1510).contains(&id) {
                        if !reconnect && id == 1500 {
                            assert_eq!(m.body[18..22], (-90.0_f32).to_be_bytes());
                            assert_eq!(m.body[22..26], 0x3d75_be29_u32.to_be_bytes());
                            assert_eq!(f32::from_be_bytes(m.body[26..30].try_into().unwrap()), 0.);
                            assert_eq!(m.body[745], 14);
                        }
                        hero_ids.push(id);
                    }
                }
            }
            if reconnect {
                assert!(variables.is_empty());
                assert!(hero_ids.len() < 10 && hero_ids.contains(&1500));
                full_structures.sort_unstable();
                assert_eq!(
                    full_structures,
                    [339, 339, 339, 340, 340, 340, 341, 341, 341]
                );
                assert_eq!(compact_structures, [295]);
            } else {
                assert_eq!(
                    variables,
                    mode_variables::Opening::Solo.messages().collect::<Vec<_>>()
                );
                assert_eq!(hero_ids.len(), 10);
                assert!(full_structures.is_empty());
                assert!(compact_structures.is_empty());
                assert_eq!(shared.lock().unwrap().world.tick, 0);
                let (tx, _) = mpsc::channel(8);
                assert!(
                    shared
                        .lock()
                        .unwrap()
                        .attach(player, tx, watch::channel(false).0)
                        .is_none()
                );
            }
            assert_eq!(
                next(&mut socket, &cipher).await.opcode,
                crate::wire_opcode::SceneOwner
            );
            let table = next(&mut socket, &cipher).await;
            assert_eq!(table.opcode, crate::wire_opcode::SceneReadiness);
            let Fields::ControlTable { entries, .. } = table.fields().unwrap() else {
                panic!()
            };
            assert_eq!(
                (entries[0].reference, entries[0].gate, entries[0].flag),
                (1500, 1, 0)
            );
            assert!(entries[1..10].iter().all(|r| r.gate == 1 && r.flag == 1));
            if !reconnect {
                let clock = Message::known(crate::Opcode::ClockSync, [0, 0, 0, 0, 0, 0]);
                send(&mut socket, &cipher, clock.clone()).await;
                assert_eq!(next(&mut socket, &cipher).await, clock);
                assert_eq!(
                    next(&mut socket, &cipher).await.opcode,
                    crate::wire_opcode::TickSyncA
                );
                let expected: Vec<_> = {
                    let room = shared.lock().unwrap();
                    room.world
                        .entities
                        .iter()
                        .filter(|e| e.team == 0 && crate::compact::structure_slot(e).is_some())
                        .map(|e| {
                            (
                                crate::compact::structure_slot(e).unwrap(),
                                u32::from(e.type_id),
                                u32::from(messages::wire_id(e.id)),
                                e.hp,
                                e.max_hp,
                            )
                        })
                        .collect()
                };
                assert_eq!(expected.len(), 13);
                for (slot, template, actor, hp, max_hp) in expected {
                    let m = next(&mut socket, &cipher).await;
                    assert_eq!(m.opcode, crate::wire_opcode::Snapshot);
                    assert_eq!(m.body.len(), 126);
                    assert_eq!(m.body[121], 1);
                    assert_eq!(m.body[116], slot);
                    assert_eq!(
                        u32::from_be_bytes(m.body[0..4].try_into().unwrap()),
                        template
                    );
                    assert_eq!(u32::from_be_bytes(m.body[8..12].try_into().unwrap()), actor);
                    assert_eq!(f32::from_be_bytes(m.body[36..40].try_into().unwrap()), hp);
                    assert_eq!(
                        f32::from_be_bytes(m.body[40..44].try_into().unwrap()),
                        max_hp
                    );
                }
            }
            send(
                &mut socket,
                &cipher,
                Message::known(crate::Opcode::SceneConfirmed, [0; 6]),
            )
            .await;
            until(&mut socket, &cipher, crate::wire_opcode::SceneTransition).await;
            assert_eq!(
                next(&mut socket, &cipher).await.opcode,
                crate::wire_opcode::SceneConfirmed
            );
            assert_eq!(
                next(&mut socket, &cipher).await.opcode,
                crate::wire_opcode::SceneTransition
            );
            if reconnect {
                assert_eq!(
                    next(&mut socket, &cipher).await.opcode,
                    crate::wire_opcode::TickSyncA
                );
            } else {
                send(
                    &mut socket,
                    &cipher,
                    Message::known(crate::Opcode::ClockSync, 1.25f32.to_be_bytes()),
                )
                .await;
                loop {
                    let m = next(&mut socket, &cipher).await;
                    assert_ne!(m.opcode, crate::wire_opcode::TickSyncA);
                    assert_ne!(m.opcode, crate::wire_opcode::Snapshot);
                    if m.is(crate::Opcode::EntitySpawn) {
                        let template = u16::from_be_bytes(m.body[2..4].try_into().unwrap());
                        assert!(!matches!(template, 295 | 296 | 339..=341));
                    }
                    if m.is(crate::Opcode::ClockSync) {
                        break;
                    }
                }
            }
            assert_eq!(shared.lock().unwrap().phase, crate::RoomPhase::Running);
            if !reconnect {
                drop(socket);
                tokio::time::timeout(Duration::from_secs(2), async {
                    while shared.lock().unwrap().seats[0].peers[0].is_some() {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await
                .unwrap();
                {
                    let mut room = shared.lock().unwrap();
                    room.world
                        .entities
                        .find_mut(|e| e.id == 2060 - 1499)
                        .unwrap()
                        .hp = 3800.;
                }
                socket = login(&hosted.address, player, &cipher).await;
                for _ in 0..10 {
                    assert_eq!(
                        next(&mut socket, &cipher).await.opcode,
                        crate::wire_opcode::PlayerInfo
                    );
                }
                let settings = next(&mut socket, &cipher).await;
                assert_eq!(&settings.body[32..56], b"*GameMode_5v5_SoloBots*\0");
                assert_eq!(settings.opcode, crate::wire_opcode::GameSetup);
                assert_eq!(&settings.body[..8], &[0, 0, 5, 220, 0, 0, 0, 0]);
                assert_eq!(
                    next(&mut socket, &cipher).await.opcode,
                    crate::wire_opcode::BeginLoading
                );
            }
        }
        drop(socket);
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let mut complete = 0;
                if let Ok(dirs) = std::fs::read_dir(root.join(id.to_string())) {
                    for dir in dirs.flatten() {
                        let path = dir.path().join("00000000.vgr.partial");
                        let Ok(bytes) = std::fs::read(path) else {
                            continue;
                        };
                        let identity = player.to_string();
                        assert!(!bytes.windows(36).any(|w| w == identity.as_bytes()));
                        let mut reader = halcyon_protocol::ReplayReader::new(bytes.as_slice());
                        let mut ops = Vec::new();
                        while let Ok(Some(record)) = reader.next_record() {
                            ops.push(record.message.opcode);
                        }
                        if ops.contains(&crate::wire_opcode::TickSyncA) {
                            assert_eq!(ops.first(), Some(&crate::wire_opcode::ModeName));
                            assert!(
                                ops.contains(&crate::wire_opcode::SceneReadiness)
                                    && !ops.contains(&crate::wire_opcode::Snapshot5V5)
                            );
                            complete += 1;
                        }
                    }
                }
                if complete == 2 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn first_valid_solo_move_only_starts_after_the_scene_write_and_changes_position() {
        for op in [crate::wire_opcode::MoveCmd, crate::wire_opcode::StateUpdA] {
            let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
            let player = Uuid::new_v4();
            let id = Uuid::new_v4();
            rooms
                .reserve(Reservation {
                    talent_levels: Vec::new(),
                    match_id: id,
                    players: vec![player],
                    teams: vec![0],
                    mode: "solo".into(),
                    difficulty: None,
                })
                .unwrap();
            let shared = rooms.room(id).unwrap();
            let mut room = shared.lock().unwrap();
            let (tx, mut rx) = mpsc::channel(16);
            room.attach(player, tx, watch::channel(false).0).unwrap();
            let locked = room
                .receive(0, 0, Message::known(crate::Opcode::LockSelection, [0; 6]))
                .unwrap();
            assert_eq!(
                locked.iter().map(|m| m.opcode).collect::<Vec<_>>(),
                [
                    crate::wire_opcode::Snapshot5V5,
                    crate::wire_opcode::LockSelection
                ]
            );
            room.phase_start = Instant::now() - Duration::from_secs(8);
            room.tick();
            rx.try_recv().unwrap();
            room.receive(0, 0, Message::known(crate::Opcode::RequestScene, [0; 6]))
                .unwrap();
            room.receive(
                0,
                0,
                Message::known(crate::Opcode::SceneOwner, [0, 0, 5, 220, 1, 0]),
            )
            .unwrap();
            let target = |x: f32| {
                let mut body = [0u8; 14];
                body[..4].copy_from_slice(&x.to_be_bytes());
                Message::new(op, body)
            };
            assert!(room.receive(0, 0, target(6.)).unwrap().is_empty());
            assert_eq!(room.phase, crate::RoomPhase::Loading);
            room.seats[0].scene_written = true;
            if op == crate::wire_opcode::StateUpdA {
                assert!(room.receive(0, 0, target(6.)).unwrap().is_empty());
                assert_eq!(room.phase, crate::RoomPhase::Loading);
            }
            room.seats[0].world_actor_written = true;
            if op == crate::wire_opcode::StateUpdA {
                let mut invalid = target(6.);
                invalid.body[13] = 1;
                assert!(room.receive(0, 0, invalid).unwrap().is_empty());
                assert_eq!(room.phase, crate::RoomPhase::Loading);
            }
            assert!(room.receive(0, 0, target(f32::NAN)).unwrap().is_empty());
            assert!(room.receive(0, 0, target(10000.)).unwrap().is_empty());
            assert_eq!(room.phase, crate::RoomPhase::Loading);
            let before = room.world.entities[0].position;
            let out = room.receive(0, 0, target(6.)).unwrap();
            let expected = if op == crate::wire_opcode::StateUpdA {
                &[
                    crate::wire_opcode::TickSyncA,
                    crate::wire_opcode::EntityFloat,
                ][..]
            } else {
                &[crate::wire_opcode::TickSyncA][..]
            };
            assert_eq!(out.iter().map(|m| m.opcode).collect::<Vec<_>>(), expected);
            if op == crate::wire_opcode::StateUpdA {
                assert_eq!(out[1].body, [14, 0x40, 0xc0, 0, 0, 0, 0, 0, 0]);
            }
            assert_eq!(room.phase, crate::RoomPhase::Running);
            room.tick();
            assert_ne!(room.world.entities[0].position, before);
            let repeated = room.receive(0, 0, target(6.)).unwrap();
            assert_eq!(repeated, out[1..]);
        }
    }
    #[tokio::test]
    async fn solo_build_request_after_scene_write_can_confirm_and_rehydrate() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        let player = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: id,
                players: vec![player],
                teams: vec![0],
                mode: "solo".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut room = shared.lock().unwrap();
        let (tx, _rx) = mpsc::channel(16);
        room.attach(player, tx, watch::channel(false).0).unwrap();
        room.receive(0, 0, Message::known(crate::Opcode::LockSelection, [0; 6]))
            .unwrap();
        room.phase_start = Instant::now() - Duration::from_secs(8);
        room.tick();
        room.receive(0, 0, Message::known(crate::Opcode::RequestScene, [0; 6]))
            .unwrap();
        room.receive(
            0,
            0,
            Message::known(crate::Opcode::SceneOwner, [0, 0, 5, 220, 1, 0]),
        )
        .unwrap();
        let request = || {
            Message::known(
                crate::Opcode::RequestRecommendedBuildPath,
                [2, 0, 0, 0, 0, 0],
            )
        };
        assert!(room.receive(0, 0, request()).unwrap().is_empty());
        room.seats[0].scene_written = true;
        for body in [vec![3], vec![0xff], vec![1, 1, 0, 0, 0, 0], vec![2, 0]] {
            assert!(
                room.receive(
                    0,
                    0,
                    Message::known(crate::Opcode::RequestRecommendedBuildPath, body)
                )
                .unwrap()
                .is_empty()
            );
        }
        assert_eq!(room.phase, crate::RoomPhase::Loading);
        let out = room.receive(0, 0, request()).unwrap();
        assert_eq!(
            out.iter().map(|m| m.opcode).collect::<Vec<_>>(),
            [
                crate::wire_opcode::RequestRecommendedBuildPath,
                crate::wire_opcode::SetRecommendedBuildPath,
                crate::wire_opcode::TickSyncA
            ]
        );
        assert_eq!(out[1].body, [0, 0, 5, 220, 2]);
        assert_eq!(room.phase, crate::RoomPhase::Running);
        assert_eq!(room.receive(0, 0, request()).unwrap().len(), 2);
        let hydrated = solo_opening(&mut room, 0).unwrap();
        assert_eq!(
            hydrated
                .iter()
                .filter(|m| m.is(crate::Opcode::SetRecommendedBuildPath))
                .count(),
            1
        );
        assert_eq!(hydrated.last().unwrap(), &out[1]);
    }

    #[tokio::test]
    async fn solo_loads_selected_roster_once_and_allows_separate_data_until_scene_request() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        let player = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: id,
                players: vec![player],
                teams: vec![0],
                mode: "5v5_bots_solo".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut r = shared.lock().unwrap();
        let (tx, mut rx) = mpsc::channel(16);
        let (_, ch, _, _) = r.attach(player, tx, watch::channel(false).0).unwrap();
        assert_eq!(ch, 0);
        r.receive(0, 0, Message::known(crate::Opcode::LockSelection, [0; 6]))
            .unwrap();
        assert_eq!(r.phase, crate::RoomPhase::Loading);
        r.tick();
        assert!(rx.try_recv().is_err());
        r.phase_start = Instant::now() - Duration::from_secs(8);
        r.tick();
        let Delivery::Private(batch) = rx.try_recv().unwrap() else {
            panic!()
        };
        assert_eq!(batch.len(), 11);
        assert_eq!(
            batch.last().unwrap().opcode,
            crate::wire_opcode::BeginLoading
        );
        for (i, m) in batch[..10].iter().enumerate() {
            assert_eq!(m.opcode, crate::wire_opcode::PlayerInfo);
            assert_eq!(m.body.len(), 222);
            assert_eq!(
                u32::from_be_bytes(m.body[160..164].try_into().unwrap()),
                1500 + i as u32
            );
        }
        r.last_roster = Instant::now() - Duration::from_secs(2);
        r.tick();
        assert!(rx.try_recv().is_err());
        let (tx, _) = mpsc::channel(16);
        assert_eq!(r.attach(player, tx, watch::channel(false).0).unwrap().1, 1);
        assert_eq!(r.phase, crate::RoomPhase::Running);
        assert!(!r.seats[0].control_world);
    }
}
