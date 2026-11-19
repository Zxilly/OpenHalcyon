use halcyon_game::{Rooms, TerminateError};
use halcyon_platform::{MatchHost, Reservation};
use halcyon_protocol::{ACK_FRAME, MatchCipher, Message, hello_with_port};
use std::{net::Ipv4Addr, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};
use uuid::Uuid;

async fn login(addr: &str, player: Uuid, cipher: &MatchCipher) -> TcpStream {
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(&hello_with_port("127.0.0.1", s.peer_addr().unwrap().port()).unwrap())
        .await
        .unwrap();
    let mut ack = [0; 5];
    s.read_exact(&mut ack).await.unwrap();
    assert_eq!(ack, ACK_FRAME);
    let mut id = vec![0; 70];
    id[..36].copy_from_slice(player.to_string().as_bytes());
    s.write_all(&cipher.encode(&Message::new(0x3e8, id)).unwrap())
        .await
        .unwrap();
    s
}
async fn until(s: &mut TcpStream, c: &MatchCipher, op: u16) -> Message {
    timeout(Duration::from_secs(2), async {
        loop {
            let len = s.read_u16().await.unwrap();
            let mut b = vec![0; len as usize];
            s.read_exact(&mut b).await.unwrap();
            let m = c.decode(&b).unwrap();
            if m.opcode == op {
                return m;
            }
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn reservation_and_two_authenticated_sockets() {
    let rooms = Rooms::new(Ipv4Addr::LOCALHOST.into(), "127.0.0.1".into(), 1, None);
    let id = Uuid::new_v4();
    let player = Uuid::new_v4();
    let req = Reservation {
        match_id: id,
        players: vec![player],
        mode: "5v5_bots_solo".into(),
    };
    let addr = rooms.reserve(req.clone()).unwrap().address;
    assert_eq!(rooms.reserve(req).unwrap().address, addr);
    let c = MatchCipher::new(&id.to_string());
    let mut rejected = login(&addr, Uuid::new_v4(), &c).await;
    let mut byte = [0];
    assert_eq!(
        timeout(Duration::from_secs(1), rejected.read(&mut byte))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    let mut control = login(&addr, player, &c).await;
    until(&mut control, &c, 0x453).await;
    let roster = until(&mut control, &c, 0x459).await;
    assert_eq!(&roster.body[105..141], player.to_string().as_bytes());
    let mut pick = vec![0; 14];
    pick[..4].copy_from_slice(&243u32.to_be_bytes());
    pick[4..8].copy_from_slice(&0xf240ebfeu32.to_be_bytes());
    control
        .write_all(&c.encode(&Message::new(0x45e, pick)).unwrap())
        .await
        .unwrap();
    let updated = until(&mut control, &c, 0x459).await;
    assert_eq!(&updated.body[17..19], &243u16.to_be_bytes());
    assert_eq!(&updated.body[21..25], &0xf240ebfeu32.to_be_bytes());
    control
        .write_all(&c.encode(&Message::new(0x455, [0; 6])).unwrap())
        .await
        .unwrap();
    until(&mut control, &c, 0x455).await;
    let mut game = login(&addr, player, &c).await;
    let info = until(&mut game, &c, 0x3ee).await;
    assert_eq!(&info.body[160..164], &1500u32.to_be_bytes());
    let spawn = until(&mut game, &c, 0x3f3).await;
    assert_eq!(&spawn.body[4..8], &0xf240ebfeu32.to_be_bytes());

    let info = rooms.detail(id).unwrap();
    assert_eq!(info["phase"], "running");
    assert_eq!(info["participants"][0]["controlConnected"], true);
    assert_eq!(info["participants"][0]["gameConnected"], true);
    game.write_all(&c.encode(&Message::movement(5., 2.)).unwrap())
        .await
        .unwrap();
    until(&mut game, &c, 0x42e).await;
    let (pending, record) = rooms.terminate(id, "test", "finished").unwrap();
    assert!(!pending);
    assert_eq!(
        rooms.terminate(id, "test", "different"),
        Err(TerminateError::Conflict)
    );
    timeout(Duration::from_secs(2), async {
        loop {
            if rooms.detail(id).unwrap()["phase"] == "ended" {
                break;
            }
            tokio::task::yield_now().await
        }
    })
    .await
    .unwrap();
    assert_eq!(
        rooms.terminate(id, "test", "finished").unwrap(),
        (true, record)
    );
    assert!(TcpStream::connect(addr).await.is_err());
    assert!(rooms.outbound_bytes() > 1000);
    assert_eq!(rooms.events(id, 0, 1).unwrap()["hasMore"], true);
}

#[tokio::test]
async fn admission_precedes_authentication() {
    let rooms = Rooms::new(Ipv4Addr::LOCALHOST.into(), "127.0.0.1".into(), 1, None);
    let player = Uuid::new_v4();
    let id = Uuid::new_v4();
    let req = Reservation {
        match_id: id,
        players: vec![player],
        mode: "solo".into(),
    };
    let addr = rooms.prepare(req.clone()).unwrap().address;
    let mut s = TcpStream::connect(&addr).await.unwrap();
    let mut b = [0];
    assert_eq!(
        timeout(Duration::from_secs(1), s.read(&mut b))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    rooms.discard(id).unwrap();
    assert!(rooms.detail(id).is_none());
    let addr = rooms.prepare(req).unwrap().address;
    rooms.activate(id).unwrap();
    assert!(rooms.discard(id).is_err());
    let c = MatchCipher::new(&id.to_string());
    let mut s = login(&addr, player, &c).await;
    until(&mut s, &c, 0x3e9).await;
}
