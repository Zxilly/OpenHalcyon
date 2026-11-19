use super::{Batch, Message, Room, messages};

pub(super) use halcyon_data::LEGACY_ARAL_SEATS as SEATS;
pub(super) const DRAFT_SECONDS: f32 = 20.;
const MODE: &str = "*GameMode_Aral_Casual*";

pub(super) fn legacy_queue(name: &str) -> bool {
    // This exact ID occurs in the matching iOS-era joinLobby requests.
    name == halcyon_data::LEGACY_ARAL_QUEUE
}

pub(super) fn roster(room: &Room, remaining: f32, private: bool) -> Message {
    use halcyon_protocol::AralRoster;
    let mut body = vec![0; AralRoster::BODY_BYTES];
    body[..4].copy_from_slice(&remaining.clamp(0., DRAFT_SECONDS).to_be_bytes());
    body[4..8].copy_from_slice(&DRAFT_SECONDS.to_be_bytes());
    for index in 0..AralRoster::SLOT_COUNT {
        let start = index * AralRoster::SLOT_BYTES;
        let row = &mut body[start..start + AralRoster::SLOT_BYTES];
        row[9] = index as u8;
        row[12..15].fill(0xff);
        row[17..21].fill(0xff);
        row[21..25].copy_from_slice(&0x10c2bad9u32.to_be_bytes());
        if let (Some(player), Some(pick)) = (room.players.get(index), room.picks.get(index)) {
            let actor = &room.world.entities[index];
            row[8] = 1;
            row[10] = u8::from(pick.selected);
            row[11] = u8::from(pick.locked);
            row[16] = actor.team + 1;
            row[17..19].copy_from_slice(&actor.type_id.to_be_bytes());
            row[19..21].copy_from_slice(&messages::wire_id(actor.id).to_be_bytes());
            row[21..25].copy_from_slice(&pick.skin.to_be_bytes());
            row[25..30].copy_from_slice(b"Guest");
            if !private {
                row[105..141].copy_from_slice(player.to_string().as_bytes());
            }
        }
    }
    Message::new(0x045a, body)
}

pub(super) fn setup(room: &Room, seat: usize, remaining: f32) -> Batch {
    let mut settings = vec![0; 101];
    let actor = messages::wire_id(seat as u32 + 1);
    settings[..4].copy_from_slice(&u32::from(actor).to_be_bytes());
    settings[7] = 1;
    settings[10..12].fill(0xff);
    settings[16..18].fill(3);
    settings[32..32 + MODE.len()].copy_from_slice(MODE.as_bytes());
    settings[96..100].copy_from_slice(&5f32.to_be_bytes());
    settings[100] = room.world.entities[seat].team + 1;
    let mut mode = vec![0; 68];
    mode[..4].copy_from_slice(&3u32.to_be_bytes());
    mode[4..4 + MODE.len()].copy_from_slice(MODE.as_bytes());
    let mut batch = vec![Message::new(0x03e9, settings), Message::new(0x0454, mode)];
    for name in &room.catalog {
        let mut body = [0; 32];
        let count = name.len().min(body.len());
        body[..count].copy_from_slice(&name.as_bytes()[..count]);
        batch.push(Message::new(0x0453, body));
    }
    batch.push(roster(room, remaining, false));
    batch
}

pub(super) fn opening(room: &Room, team: Option<u8>) -> Batch {
    let mut batch: Batch = Vec::new();
    batch.push(messages::scene_mode(&room.queue, 0));
    batch.extend(crate::scene::players(room));
    batch.push(clock(room.world.tick));
    for actor in room
        .world
        .entities
        .iter()
        .take(SEATS)
        .filter(|actor| actor.hp > 0. && team.is_none_or(|id| room.world.visible(id, actor)))
    {
        batch.push(messages::spawn(actor, &room.picks, room.data.as_deref()));
        messages::append_inventory(&mut batch, actor);
    }
    batch
}

pub(super) fn clock(tick: u64) -> Message {
    Message::new(
        0x0451,
        (tick as f32 / halcyon_sim::TICK_HZ as f32).to_be_bytes(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Rooms, transport};
    use halcyon_platform::{MatchHost, Reservation};
    use halcyon_protocol::{ACK_FRAME, Fields, MatchCipher, hello};
    use halcyon_sim::Kind;
    use std::{
        net::Ipv4Addr,
        time::{Duration, Instant},
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpStream,
        time::timeout,
    };
    use uuid::Uuid;

    async fn next(socket: &mut TcpStream, cipher: &MatchCipher) -> Message {
        let bytes = timeout(Duration::from_secs(3), transport::frame(socket))
            .await
            .unwrap()
            .unwrap();
        cipher.decode_owned(bytes).unwrap()
    }

    async fn until(socket: &mut TcpStream, cipher: &MatchCipher, opcode: u16) -> Message {
        timeout(Duration::from_secs(3), async {
            loop {
                let message = next(socket, cipher).await;
                if message.opcode == opcode {
                    return message;
                }
            }
        })
        .await
        .unwrap()
    }

    async fn request_scene(socket: &mut TcpStream, cipher: &MatchCipher, actor: u32) {
        let probe = Message::new(0x046e, [0; 6]);
        socket
            .write_all(&cipher.encode(&probe).unwrap())
            .await
            .unwrap();
        let mut words = [0u8; 6];
        words[..4].copy_from_slice(&actor.to_be_bytes());
        words[4] = 1;
        let request = Message::new(0x0471, words);
        socket
            .write_all(&cipher.encode(&request).unwrap())
            .await
            .unwrap();
        let name = next(socket, cipher).await;
        assert_eq!(name.opcode, 0x046f);
        assert_eq!(name.body[68], 0);
        for _ in 0..SEATS {
            assert_eq!(next(socket, cipher).await.opcode, 0x03ee);
        }
        let clock = next(socket, cipher).await;
        assert_eq!(clock.opcode, 0x0451);
        assert_eq!(clock.body, [0; 6]);
        assert_eq!(next(socket, cipher).await.opcode, 0x03f3);
        until(socket, cipher, probe.opcode).await;
        assert_eq!(next(socket, cipher).await.opcode, request.opcode);
    }

    async fn ready(socket: &mut TcpStream, cipher: &MatchCipher) {
        let done = Message::new(0x046d, [0; 6]);
        socket
            .write_all(&cipher.encode(&done).unwrap())
            .await
            .unwrap();
        until(socket, cipher, done.opcode).await;
    }

    #[test]
    fn readiness_requires_order_and_authenticated_actor_id() {
        let mut preamble = crate::scene::Preamble::default();
        preamble.observe(&Message::new(0x046d, [0; 6]), 1505);
        preamble.observe(&Message::new(0x046e, [0; 6]), 1505);
        preamble.observe(&Message::new(0x0471, [0, 0, 5, 224, 1, 0]), 1505);
        assert!(!preamble.scene_requested());
        preamble.observe(&Message::new(0x0471, [0, 0, 5, 225, 1, 0]), 1505);
        assert!(preamble.scene_requested());
        assert!(!preamble.confirmed());
        preamble.observe(&Message::new(0x046d, [0; 6]), 1505);
        assert!(preamble.confirmed());
    }

    #[tokio::test]
    async fn six_authenticated_aral_streams_cross_into_game_without_reconnect() {
        let room_id = Uuid::new_v4();
        let rooms = Rooms::new(Ipv4Addr::LOCALHOST.into(), "127.0.0.1".into(), 1, None);
        let players: Vec<_> = (0..SEATS).map(|_| Uuid::new_v4()).collect();
        let reservation = Reservation {
            match_id: room_id,
            players: players.clone(),
            teams: vec![],
            mode: "casual_aral".into(),
            difficulty: None,
        };
        assert!(
            rooms
                .reserve(Reservation {
                    players: players[..5].to_vec(),
                    ..reservation.clone()
                })
                .is_err()
        );
        let address = rooms.reserve(reservation).unwrap().address;
        let cipher = MatchCipher::new(&room_id.to_string());
        let mut clients = Vec::new();
        for (index, player) in players.iter().enumerate() {
            let mut socket = TcpStream::connect(&address).await.unwrap();
            socket
                .write_all(&hello("localhost").unwrap())
                .await
                .unwrap();
            let mut ack = [0; 5];
            socket.read_exact(&mut ack).await.unwrap();
            assert_eq!(ack, ACK_FRAME);
            let mut auth = [0; 70];
            auth[..36].copy_from_slice(player.to_string().as_bytes());
            socket
                .write_all(&cipher.encode(&Message::new(0x03e8, auth)).unwrap())
                .await
                .unwrap();
            let setting = next(&mut socket, &cipher).await;
            assert_eq!(setting.opcode, 0x03e9);
            assert_eq!(setting.body[16..18], [3, 3]);
            assert_eq!(setting.body[100], (index % 2 + 1) as u8);
            let mode = next(&mut socket, &cipher).await;
            assert_eq!(mode.opcode, 0x0454);
            assert_eq!(mode.body[..4], 3u32.to_be_bytes());
            let snapshot = until(&mut socket, &cipher, 0x045a).await;
            let Fields::AralRoster(view) = snapshot.fields().unwrap() else {
                panic!("expected ARAL roster")
            };
            assert_eq!(view.allotted_seconds(), 20.);
            assert_eq!(view.identity(index).unwrap(), player.to_string().as_bytes());
            assert!(!view.slot(6).unwrap().occupied);
            clients.push(socket);
        }

        let room = rooms.room(room_id).unwrap();
        {
            let mut room = room.lock().unwrap();
            room.phase_start = Instant::now() - Duration::from_secs(21);
            room.tick();
            assert_eq!(room.phase, "loading");
            assert_eq!(
                room.world
                    .entities
                    .iter()
                    .filter(|actor| actor.kind == Kind::Hero)
                    .count(),
                6
            );
            assert!(
                !room
                    .world
                    .entities
                    .iter()
                    .any(|actor| (7..=10).contains(&actor.id))
            );
        }
        for socket in &mut clients {
            let mut roster_count = 0;
            loop {
                let frame = next(socket, &cipher).await;
                match frame.opcode {
                    0x045a => {}
                    0x03ee => roster_count += 1,
                    0x046c => break,
                    other => panic!("unexpected frame {other:#x} before world request"),
                }
            }
            assert_eq!(roster_count, SEATS);
        }
        for (index, socket) in clients.iter_mut().enumerate() {
            request_scene(socket, &cipher, 1500 + index as u32).await;
        }
        assert_eq!(rooms.detail(room_id).unwrap()["phase"], "loading");
        for socket in clients.iter_mut().take(5) {
            ready(socket, &cipher).await;
        }
        assert_eq!(rooms.detail(room_id).unwrap()["phase"], "loading");
        ready(&mut clients[5], &cipher).await;
        for socket in &mut clients {
            assert_eq!(until(socket, &cipher, 0x03ef).await.body, [0; 6]);
        }
        assert_eq!(rooms.detail(room_id).unwrap()["botPlayers"], 0);
        clients[0]
            .write_all(&cipher.encode(&Message::movement(4., 0.)).unwrap())
            .await
            .unwrap();
        until(&mut clients[0], &cipher, 0x042e).await;
        assert_eq!(rooms.detail(room_id).unwrap()["phase"], "running");
    }
}
