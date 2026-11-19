//! Native scene negotiation on an authenticated control stream.
use super::{Batch, Message, Room, messages};
#[derive(Default)]
pub(super) struct Preamble {
    stage: usize,
    implicit: bool,
}

impl Preamble {
    const OPS: [u16; 3] = [0x046e, 0x0471, 0x046d];

    pub fn observe(&mut self, message: &Message, actor: u32) {
        let known = match message.opcode {
            0x046e | 0x046d => message.body == [0; 6],
            0x0471 => {
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
        m.opcode == 0x046d && m.body == [0; 6] && std::mem::take(&mut self.implicit)
    }
    pub fn scene_requested(&self) -> bool {
        self.stage >= 2
    }

    pub fn confirmed(&self) -> bool {
        self.stage == Self::OPS.len()
    }
}

pub(super) fn players(room: &Room) -> impl Iterator<Item = Message> {
    (0..room.picks.len()).map(|i| messages::info(&room.world, &room.players, &room.picks, i, true))
}
pub(super) fn begin_loading(room: &Room) -> Batch {
    players(room)
        .chain(std::iter::once(Message::new(0x046c, [0])))
        .collect()
}

pub(super) fn solo_table(room: &Room) -> Message {
    let mut body = [0u8; 96];
    for (row, actor) in body
        .chunks_exact_mut(6)
        .zip(room.world.entities.iter().take(room.picks.len()))
    {
        row[..4].copy_from_slice(&u32::from(messages::wire_id(actor.id)).to_be_bytes());
        row[4] = 1;
        row[5] = u8::from(actor.bot);
    }
    Message::new(0x045c, body)
}
pub(super) fn solo_opening(room: &mut Room, seat: usize) -> Batch {
    let team = room.world.entities[seat].team;
    let initial = room.phase == "loading" && room.world.tick == 0;
    let mut out = vec![
        messages::scene_mode(&room.queue, 1),
        Message::new(0x0470, [0]),
    ];
    out.extend(players(room));
    out.push(super::aral::clock(room.world.tick));
    room.visible[team as usize].clear();
    for e in &room.world.entities {
        if e.hp <= 0. {
            continue;
        }
        let visible = room.world.visible(team, e);
        // The recorded initial scene declares all ten starting heroes. On reconnect
        // live opposing positions are subject to the team's usual visibility.
        if (initial && e.kind == halcyon_sim::Kind::Hero) || (!initial && visible) {
            out.push(messages::spawn(e, &room.picks, room.data.as_deref()));
            messages::append_inventory(&mut out, e);
            if visible {
                room.visible[team as usize].insert(e.id);
            }
        }
    }
    out
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
                Message::new(0x3e8, player.to_string().as_bytes()),
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
                match_id: id,
                players: vec![player],
                teams: vec![0],
                mode: "5v5_bots_solo".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut socket = login(&hosted.address, player, &cipher).await;
        until(&mut socket, &cipher, 0x459).await;
        send(&mut socket, &cipher, Message::new(0x463, [0; 6])).await;
        until(&mut socket, &cipher, 0x463).await;
        shared.lock().unwrap().phase_start = Instant::now() - Duration::from_secs(8);
        until(&mut socket, &cipher, 0x46c).await;
        for reconnect in [false, true] {
            send(&mut socket, &cipher, Message::new(0x46e, [0; 6])).await;
            // A different player's request and an early confirmation cannot enter the world.
            send(
                &mut socket,
                &cipher,
                Message::new(0x471, [0, 0, 5, 221, 1, 0]),
            )
            .await;
            send(&mut socket, &cipher, Message::new(0x46d, [0; 6])).await;
            send(
                &mut socket,
                &cipher,
                Message::new(0x471, [0, 0, 5, 220, 1, 0]),
            )
            .await;
            let mode = next(&mut socket, &cipher).await;
            assert_eq!((mode.opcode, mode.body[68]), (0x46f, 1));
            assert_eq!(next(&mut socket, &cipher).await.opcode, 0x470);
            for _ in 0..10 {
                assert_eq!(next(&mut socket, &cipher).await.opcode, 0x3ee);
            }
            assert_eq!(next(&mut socket, &cipher).await.opcode, 0x451);
            let mut hero_ids = Vec::new();
            loop {
                let m = next(&mut socket, &cipher).await;
                if m.opcode == 0x46e {
                    break;
                }
                if m.opcode == 0x3f3 {
                    let id = u16::from_be_bytes(m.body[10..12].try_into().unwrap());
                    if (1500..1510).contains(&id) {
                        hero_ids.push(id);
                    }
                }
            }
            if reconnect {
                assert!(hero_ids.len() < 10 && hero_ids.contains(&1500));
            } else {
                assert_eq!(hero_ids.len(), 10);
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
            assert_eq!(next(&mut socket, &cipher).await.opcode, 0x471);
            let table = next(&mut socket, &cipher).await;
            assert_eq!(table.opcode, 0x45c);
            let Fields::ControlTable { entries, .. } = table.fields().unwrap() else {
                panic!()
            };
            assert_eq!(
                (entries[0].reference, entries[0].gate, entries[0].flag),
                (1500, 1, 0)
            );
            assert!(entries[1..10].iter().all(|r| r.gate == 1 && r.flag == 1));
            if !reconnect {
                let clock = Message::new(0, [0, 0, 0, 0, 0, 0]);
                send(&mut socket, &cipher, clock.clone()).await;
                assert_eq!(next(&mut socket, &cipher).await, clock);
                assert_eq!(next(&mut socket, &cipher).await.opcode, 0x3ef);
            }
            send(&mut socket, &cipher, Message::new(0x46d, [0; 6])).await;
            until(&mut socket, &cipher, 0x470).await;
            assert_eq!(next(&mut socket, &cipher).await.opcode, 0x46d);
            assert_eq!(next(&mut socket, &cipher).await.opcode, 0x470);
            if reconnect {
                assert_eq!(next(&mut socket, &cipher).await.opcode, 0x3ef);
            } else {
                send(&mut socket, &cipher, Message::new(0, 1.25f32.to_be_bytes())).await;
                loop {
                    let m = next(&mut socket, &cipher).await;
                    assert_ne!(m.opcode, 0x3ef);
                    if m.opcode == 0 {
                        break;
                    }
                }
            }
            assert_eq!(shared.lock().unwrap().phase, "running");
            if !reconnect {
                drop(socket);
                tokio::time::timeout(Duration::from_secs(2), async {
                    while shared.lock().unwrap().seats[0].peers[0].is_some() {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await
                .unwrap();
                socket = login(&hosted.address, player, &cipher).await;
                assert_eq!(next(&mut socket, &cipher).await.opcode, 0x3ee);
                until(&mut socket, &cipher, 0x46c).await;
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
                        if ops.contains(&0x3ef) {
                            assert_eq!(ops.first(), Some(&0x46f));
                            assert!(ops.contains(&0x45c) && !ops.contains(&0x459));
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
        for op in [0x3f4, 0x3f5] {
            let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
            let player = Uuid::new_v4();
            let id = Uuid::new_v4();
            rooms
                .reserve(Reservation {
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
            room.receive(0, 0, Message::new(0x463, [0; 6])).unwrap();
            room.phase_start = Instant::now() - Duration::from_secs(8);
            room.tick();
            rx.try_recv().unwrap();
            room.receive(0, 0, Message::new(0x46e, [0; 6])).unwrap();
            room.receive(0, 0, Message::new(0x471, [0, 0, 5, 220, 1, 0]))
                .unwrap();
            let target = |x: f32| {
                let mut body = [0u8; 14];
                body[..4].copy_from_slice(&x.to_be_bytes());
                Message::new(op, body)
            };
            assert!(room.receive(0, 0, target(6.)).unwrap().is_empty());
            assert_eq!(room.phase, "loading");
            room.seats[0].scene_written = true;
            assert!(room.receive(0, 0, target(f32::NAN)).unwrap().is_empty());
            assert!(room.receive(0, 0, target(10000.)).unwrap().is_empty());
            assert_eq!(room.phase, "loading");
            let before = room.world.entities[0].position;
            let out = room.receive(0, 0, target(6.)).unwrap();
            assert_eq!(out.iter().map(|m| m.opcode).collect::<Vec<_>>(), [0x3ef]);
            assert_eq!(room.phase, "running");
            room.tick();
            assert_ne!(room.world.entities[0].position, before);
            assert!(room.receive(0, 0, target(6.)).unwrap().is_empty());
        }
    }
    #[tokio::test]
    async fn solo_loads_selected_roster_once_and_allows_separate_data_until_scene_request() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        let player = Uuid::new_v4();
        rooms
            .reserve(Reservation {
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
        r.receive(0, 0, Message::new(0x463, [0; 6])).unwrap();
        assert_eq!(r.phase, "loading");
        r.tick();
        assert!(rx.try_recv().is_err());
        r.phase_start = Instant::now() - Duration::from_secs(8);
        r.tick();
        let Delivery::Private(batch) = rx.try_recv().unwrap() else {
            panic!()
        };
        assert_eq!(batch.len(), 11);
        assert_eq!(batch.last().unwrap().opcode, 0x46c);
        for (i, m) in batch[..10].iter().enumerate() {
            assert_eq!(m.opcode, 0x3ee);
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
        assert_eq!(r.phase, "running");
        assert!(!r.seats[0].control_world);
    }
}
