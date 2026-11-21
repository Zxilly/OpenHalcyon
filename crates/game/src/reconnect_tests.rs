use super::*;
use crate::Rooms;
use halcyon_protocol::{ACK_FRAME, Fields, MatchCipher, Opcode, hello, wire_opcode};
use halcyon_types::{MatchHost, Reservation};
use std::time::{Duration, Instant};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};
use uuid::Uuid;

async fn send(socket: &mut TcpStream, cipher: &MatchCipher, message: Message) {
    socket
        .write_all(&cipher.encode(&message).unwrap())
        .await
        .unwrap();
}

async fn next(socket: &mut TcpStream, cipher: &MatchCipher) -> Message {
    let body = timeout(Duration::from_secs(3), crate::transport::frame(socket))
        .await
        .unwrap()
        .unwrap();
    cipher.decode_owned(body).unwrap()
}

async fn read_until(socket: &mut TcpStream, cipher: &MatchCipher, end: u16) -> Batch {
    timeout(Duration::from_secs(3), async {
        let mut out = Vec::new();
        loop {
            let message = next(socket, cipher).await;
            let done = message.opcode == end;
            out.push(message);
            if done {
                return out;
            }
        }
    })
    .await
    .unwrap()
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
        Message::known(Opcode::RegisterPlayerUuid, player.to_string().as_bytes()),
    )
    .await;
    socket
}

async fn disconnect_and_wait(socket: TcpStream, shared: &crate::Shared, seat: usize) {
    drop(socket);
    timeout(Duration::from_secs(2), async {
        while shared.lock().unwrap().seats[seat].peers[0].is_some() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

fn assert_solo_rejoin_header(
    batch: &[Message],
    expected_actor: u32,
    expected_hero: u16,
    expected_skin: u32,
    expected_team_wire: u8,
) {
    assert_eq!(batch.len(), 12);
    for (slot, message) in batch[..10].iter().enumerate() {
        assert_eq!(message.opcode, wire_opcode::PlayerInfo);
        assert_eq!(message.body.len(), 222);
        let actor = u32::from_be_bytes(message.body[160..164].try_into().unwrap());
        let hero = u32::from_be_bytes(message.body[164..168].try_into().unwrap());
        let skin = u32::from_be_bytes(message.body[168..172].try_into().unwrap());
        assert_eq!(actor, 1500 + slot as u32);
        assert_ne!(hero, 0);
        assert_ne!(skin, 0);
        if slot == 0 {
            assert_eq!(hero, u32::from(expected_hero));
            assert_eq!(skin, expected_skin);
        }
    }
    let settings = &batch[10];
    assert_eq!(settings.opcode, wire_opcode::MatchSettings);
    let Fields::GameSetup {
        first_word,
        state_input,
        word8,
        word12,
        mode,
        remaining,
        ..
    } = settings.fields().unwrap()
    else {
        panic!("expected MatchSettings fields");
    };
    assert_eq!(first_word, expected_actor);
    assert_eq!(state_input, 0);
    assert_eq!(word8, u32::from(expected_hero));
    assert_eq!(word12, expected_skin);
    assert_eq!(mode, b"*GameMode_5v5_SoloBots*");
    assert_eq!(remaining.first().copied(), Some(expected_team_wire));
    assert_eq!(
        halcyon_protocol::MatchSettingsBody::read_padded(&settings.body)
            .unwrap()
            .0
            .team,
        expected_team_wire
    );
    assert_eq!(batch[11].opcode, wire_opcode::DraftTransitionToLoading);
    assert!(
        !batch[11].body.is_empty() && batch[11].body.iter().all(|&byte| byte == 0),
        "unexpected DraftTransitionToLoading payload: {:?}",
        batch[11].body
    );
}

#[tokio::test]
async fn solo_5v5_running_reconnect_populates_match_settings_and_restores_world() {
    let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
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
    read_until(&mut socket, &cipher, wire_opcode::DraftRosterFull).await;
    send(
        &mut socket,
        &cipher,
        Message::known(Opcode::DraftLockHero, [0; 6]),
    )
    .await;
    read_until(&mut socket, &cipher, wire_opcode::DraftLockHero).await;
    shared.lock().unwrap().phase_start = Instant::now() - Duration::from_secs(8);
    read_until(&mut socket, &cipher, wire_opcode::DraftTransitionToLoading).await;
    send(
        &mut socket,
        &cipher,
        Message::known(Opcode::RequestSceneSnapshot, [0; 6]),
    )
    .await;
    send(
        &mut socket,
        &cipher,
        Message::known(Opcode::ConfirmSceneOwner, [0, 0, 5, 220, 1, 0]),
    )
    .await;
    read_until(&mut socket, &cipher, wire_opcode::SceneReadinessRows).await;
    send(
        &mut socket,
        &cipher,
        Message::known(Opcode::SceneReadyConfirm, [0; 6]),
    )
    .await;
    read_until(&mut socket, &cipher, wire_opcode::StartMatch).await;

    let (expected_hero, expected_skin, expected_team_wire) = {
        let mut room = shared.lock().unwrap();
        assert_eq!(room.phase, crate::RoomPhase::Running);
        // Exercise a non-default hero template and skin hash (e.g. Ardan) so the
        // assertion verifies actual seat state rather than Ringo's fallback defaults.
        room.world.entities[0].type_id = halcyon_data::EntityTemplate::Ardan.as_raw();
        room.picks[0].name = "Ardan".into();
        room.picks[0].skin = halcyon_protocol::skin_hash("Ardan_DefaultSkin");
        for _ in 0..10 {
            room.tick();
        }
        assert!(room.world.tick >= 10);
        (
            room.world.entities[0].type_id,
            room.picks[0].skin,
            room.world.entities[0].team + 1,
        )
    };

    disconnect_and_wait(socket, &shared, 0).await;

    let mut reconnected = login(&hosted.address, player, &cipher).await;
    let rejoin_batch = read_until(
        &mut reconnected,
        &cipher,
        wire_opcode::DraftTransitionToLoading,
    )
    .await;
    assert_solo_rejoin_header(
        &rejoin_batch,
        1500,
        expected_hero,
        expected_skin,
        expected_team_wire,
    );

    send(
        &mut reconnected,
        &cipher,
        Message::known(Opcode::RequestSceneSnapshot, [0; 6]),
    )
    .await;
    send(
        &mut reconnected,
        &cipher,
        Message::known(Opcode::ConfirmSceneOwner, [0, 0, 5, 220, 1, 0]),
    )
    .await;
    let opening = read_until(&mut reconnected, &cipher, wire_opcode::SceneReadinessRows).await;
    assert_eq!(opening[0].opcode, wire_opcode::SceneSettings);
    assert_eq!(opening[1].opcode, wire_opcode::SceneLoadedDismiss);
    let seat0_info = &opening[2];
    assert_eq!(seat0_info.opcode, wire_opcode::PlayerInfo);
    assert!(seat0_info.body.starts_with(b"Guest\0"));
    assert_eq!(&seat0_info.body[64..100], player.to_string().as_bytes());
    assert_eq!(opening[12].opcode, wire_opcode::SyncWorldClock);
    assert!(f32::from_be_bytes(opening[12].body[..4].try_into().unwrap()) > 0.0);

    timeout(Duration::from_secs(2), async {
        while !shared.lock().unwrap().seats[0].scene_written {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();

    // Real Android Solo clients confirm scene entry with ClockSync (0x0000).
    let clock = Message::known(Opcode::ClockSync, [0; 6]);
    send(&mut reconnected, &cipher, clock.clone()).await;
    assert_eq!(next(&mut reconnected, &cipher).await, clock);
    assert_eq!(
        next(&mut reconnected, &cipher).await.opcode,
        wire_opcode::StartMatch
    );
    assert!(shared.lock().unwrap().seats[0].preamble.confirmed());
}

#[tokio::test]
async fn solo_5v5_loading_reconnect_before_and_after_preamble_restores_without_draft_race() {
    for (after_7s, after_preamble) in [(false, false), (true, false), (true, true)] {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
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
        read_until(&mut socket, &cipher, wire_opcode::DraftRosterFull).await;
        send(
            &mut socket,
            &cipher,
            Message::known(Opcode::DraftLockHero, [0; 6]),
        )
        .await;
        read_until(&mut socket, &cipher, wire_opcode::DraftLockHero).await;
        assert_eq!(shared.lock().unwrap().phase, crate::RoomPhase::Loading);

        if after_7s {
            shared.lock().unwrap().phase_start = Instant::now() - Duration::from_secs(8);
            read_until(&mut socket, &cipher, wire_opcode::DraftTransitionToLoading).await;
        }
        if after_preamble {
            send(
                &mut socket,
                &cipher,
                Message::known(Opcode::RequestSceneSnapshot, [0; 6]),
            )
            .await;
            send(
                &mut socket,
                &cipher,
                Message::known(Opcode::ConfirmSceneOwner, [0, 0, 5, 220, 1, 0]),
            )
            .await;
            read_until(&mut socket, &cipher, wire_opcode::SceneReadinessRows).await;
            assert!(shared.lock().unwrap().seats[0].control_world);
            assert_eq!(shared.lock().unwrap().phase, crate::RoomPhase::Loading);
        }

        let (expected_hero, expected_skin, expected_team_wire) = {
            let room = shared.lock().unwrap();
            (
                room.world.entities[0].type_id,
                room.picks[0].skin,
                room.world.entities[0].team + 1,
            )
        };

        disconnect_and_wait(socket, &shared, 0).await;

        let mut reconnected = login(&hosted.address, player, &cipher).await;
        let rejoin_batch = read_until(
            &mut reconnected,
            &cipher,
            wire_opcode::DraftTransitionToLoading,
        )
        .await;
        assert_solo_rejoin_header(
            &rejoin_batch,
            1500,
            expected_hero,
            expected_skin,
            expected_team_wire,
        );
        // Subsequent ticks must not resend begin_loading or DraftRosterFull.
        {
            let mut room = shared.lock().unwrap();
            room.phase_start = Instant::now() - Duration::from_secs(10);
            room.last_roster = Instant::now() - Duration::from_secs(5);
            room.tick();
        }

        send(
            &mut reconnected,
            &cipher,
            Message::known(Opcode::RequestSceneSnapshot, [0; 6]),
        )
        .await;
        send(
            &mut reconnected,
            &cipher,
            Message::known(Opcode::ConfirmSceneOwner, [0, 0, 5, 220, 1, 0]),
        )
        .await;
        let opening = read_until(&mut reconnected, &cipher, wire_opcode::SceneReadinessRows).await;
        assert_eq!(opening[0].opcode, wire_opcode::SceneSettings);
        let heroes = opening.iter().filter(|m| m.is(Opcode::HeroSpawn)).count();
        assert_eq!(heroes, 10);

        send(
            &mut reconnected,
            &cipher,
            Message::known(Opcode::SceneReadyConfirm, [0; 6]),
        )
        .await;
        read_until(&mut reconnected, &cipher, wire_opcode::StartMatch).await;
        assert_eq!(shared.lock().unwrap().phase, crate::RoomPhase::Running);
    }
}

#[tokio::test]
async fn game_channel_world_header_populates_match_settings_hero_skin_and_team() {
    let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
    let players: Vec<_> = (0..2).map(|_| Uuid::new_v4()).collect();
    let id = Uuid::new_v4();
    rooms
        .reserve(Reservation {
            talent_levels: Vec::new(),
            match_id: id,
            players: players.clone(),
            teams: vec![0, 1],
            mode: "casual_5v5".into(),
            difficulty: None,
        })
        .unwrap();
    let shared = rooms.room(id).unwrap();
    let mut room = shared.lock().unwrap();
    room.seats[1].locked = true;
    room.picks[1].selected = true;
    room.picks[1].locked = true;
    room.phase(crate::RoomPhase::Running);

    let expected_hero = u32::from(room.world.entities[1].type_id);
    let expected_skin = room.picks[1].skin;
    let expected_team_wire = room.world.entities[1].team + 1;
    assert_eq!(expected_team_wire, 2);

    let (tx, _rx) = tokio::sync::mpsc::channel(16);
    let (seat, ch, _, header) = room
        .attach(players[1], tx, tokio::sync::watch::channel(false).0)
        .unwrap();
    assert_eq!((seat, ch), (1, 1));
    let settings = header
        .iter()
        .find(|m| m.is(Opcode::MatchSettings))
        .expect("world_header must include MatchSettings");
    let Fields::GameSetup {
        first_word,
        state_input,
        word8,
        word12,
        remaining,
        ..
    } = settings.fields().unwrap()
    else {
        panic!("expected MatchSettings fields");
    };
    assert_eq!(first_word, 1501);
    assert_eq!(state_input, 0);
    assert_eq!(word8, expected_hero);
    assert_eq!(word12, expected_skin);
    assert_eq!(remaining, &[expected_team_wire]);
}
