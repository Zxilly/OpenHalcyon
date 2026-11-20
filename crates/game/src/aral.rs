pub(super) mod announcements;
use super::{Batch, Message, Room, messages};

pub(super) use halcyon_data::LEGACY_ARAL_SEATS as SEATS;
pub(super) const DRAFT_SECONDS: f32 = 20.;
const MODE: &str = "*GameMode_Aral_Casual*";

pub(super) fn legacy_queue(name: &str) -> bool {
    // This exact ID occurs in the matching iOS-era joinLobby requests.
    name == halcyon_data::LEGACY_ARAL_QUEUE
}

/// The level belongs to the reserved seat and selected hero. The client supplies only a slot.
pub(super) fn talent_response(
    room: &mut Room,
    seat: usize,
    channel: usize,
    selection: halcyon_protocol::TalentSelection,
) -> Batch {
    use halcyon_protocol::{EquippedTalent, TalentSelection};
    let session = &room.seats[seat];
    if !room.legacy_aral()
        || channel != 0
        || session.peers[0].is_none()
        || !session.world_actor_written
        || !session.preamble.confirmed()
        || room.phase != "running"
    {
        return Vec::new();
    }
    let hero = &room.world.entities[seat];
    let level = if selection == TalentSelection::NoTalent {
        0
    } else {
        let Some(level) = room
            .talent_levels
            .get(seat)
            .and_then(|levels| levels.get(&hero.type_id))
        else {
            return Vec::new();
        };
        level.get()
    };
    let equipped = EquippedTalent {
        actor: u32::from(messages::wire_id(hero.id)),
        selection,
        level,
    };
    let batch = vec![
        selection.request(),
        equipped.message().expect("validated numeric equip"),
    ];
    if room.seats[seat].equipped_talent.replace(equipped) != Some(equipped) {
        let shared = super::transport::TeamBatch::new(batch.clone());
        for (i, other) in room.seats.iter().enumerate() {
            if i != seat
                && let Some(peer) = other.gameplay_peer()
            {
                peer.send(super::transport::Delivery::Team(shared.clone()));
            }
        }
        room.publish_observers(&batch);
    }
    batch
}

pub(super) fn talent_snapshot(room: &Room) -> impl Iterator<Item = Message> + '_ {
    room.seats
        .iter()
        .filter_map(|s| s.equipped_talent?.message().ok())
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
            row[25..25 + messages::GUEST_HANDLE.len()]
                .copy_from_slice(messages::GUEST_HANDLE.as_bytes());
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
    let mut batch = vec![Message::new(0x03e9, settings)];
    batch.extend(draft_catalog(room, remaining));
    batch
}

pub(super) fn draft_catalog(room: &Room, remaining: f32) -> Batch {
    let mut mode = vec![0; 68];
    mode[..4].copy_from_slice(&3u32.to_be_bytes());
    mode[4..4 + MODE.len()].copy_from_slice(MODE.as_bytes());
    let mut batch = Vec::with_capacity(room.catalog.len() + 2);
    batch.push(Message::new(0x0454, mode));
    batch.extend(
        room.catalog
            .iter()
            .map(|entry| Message::new(0x0453, *entry)),
    );
    batch.push(roster(room, remaining, false));
    batch
}

pub(super) fn refresh(room: &mut Room, seat: usize) -> Batch {
    let mut batch = draft_catalog(room, room.aral_remaining());
    if !room.seats[seat].draft_sync_sent {
        // The first refresh of all eight recorded ARAL connections used these four constants.
        let mut body = [0; 86];
        body[..4].copy_from_slice(&1_u32.to_be_bytes());
        body[4..8].copy_from_slice(&5_u32.to_be_bytes());
        body[76] = 1;
        body[77..81].copy_from_slice(&3_u32.to_be_bytes());
        batch.push(Message::new(0x0480, body));
        batch.push(roster(room, room.aral_remaining(), false));
        room.seats[seat].draft_sync_sent = true;
    }
    batch
}

pub(super) fn opening(room: &mut Room, team: Option<u8>) -> Batch {
    let mut batch: Batch = Vec::new();
    batch.push(messages::scene_mode(&room.queue, 0));
    batch.extend(crate::scene::players(room));
    batch.push(clock(room.world.tick));
    let mut map_objects: Vec<_> = super::aral_spawn::scenery().collect();
    let mut lane_minions = Vec::new();
    for actor in &room.world.entities {
        if actor.hp <= 0.0 || team.is_some_and(|side| !room.world.visible(side, actor)) {
            continue;
        }
        if let Some(declaration) = room.aral_slots.declaration(
            &room.world,
            actor,
            &room.picks,
            room.data.as_deref(),
            team.unwrap_or(actor.team),
            false,
        ) {
            match actor.kind {
                halcyon_sim::Kind::Hero => {
                    batch.push(declaration);
                    messages::append_inventory(&mut batch, &room.world, actor);
                }
                halcyon_sim::Kind::Minion => lane_minions.push(declaration),
                _ => map_objects.push(declaration),
            }
        }
    }
    map_objects.sort_unstable_by_key(|m| std::cmp::Reverse(m.body[116]));
    batch.extend(map_objects);
    batch.extend(lane_minions);
    batch.extend(room.recommended_paths(team));
    batch.extend(talent_snapshot(room));
    if let Some(team) = team {
        batch.extend(room.surrender_snapshot(team));
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
#[path = "aral_shop_test.rs"]
mod shop_tests;

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

    pub(super) async fn next(socket: &mut TcpStream, cipher: &MatchCipher) -> Message {
        let bytes = timeout(Duration::from_secs(3), transport::frame(socket))
            .await
            .unwrap()
            .unwrap();
        cipher.decode_owned(bytes).unwrap()
    }

    pub(super) async fn until(
        socket: &mut TcpStream,
        cipher: &MatchCipher,
        opcode: u16,
    ) -> Message {
        timeout(Duration::from_secs(3), async {
            loop {
                let message = next(socket, cipher).await;
                if message.opcode == opcode {
                    return message;
                }
            }
        })
        .await
        .unwrap_or_else(|_| panic!("ARAL opcode {opcode:#06x} did not arrive"))
    }

    async fn request_scene(
        socket: &mut TcpStream,
        cipher: &MatchCipher,
        actor: u32,
        initial: bool,
    ) {
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
        if initial {
            assert_eq!(clock.body, [0; 6]);
        }
        let mut declared = false;
        let mut structures = Vec::new();
        let mut talents = Vec::new();
        let pads = [
            (-78.18_f32, 0.88_f32),
            (79.66, -0.91),
            (-81.81, 1.97),
            (78.6, -5.06),
            (-77.03, -1.94),
            (83.22, 0.72),
        ];
        loop {
            let frame = next(socket, cipher).await;
            if frame.opcode == probe.opcode {
                break;
            }
            if frame.opcode == 0x0481 {
                talents.push(frame.body.clone());
            }
            if frame.opcode == 0x03f3 {
                let id = u32::from_be_bytes(frame.body[8..12].try_into().unwrap());
                assert!((1500..1506).contains(&id));
                assert_eq!(frame.body[745], 16 + (id - 1500) as u8);
                declared |= id == actor;
                if initial {
                    assert_eq!(frame.spawn_gold(), Some((2000., 2000.)));
                    let (x, z) = pads[(id - 1500) as usize];
                    assert_eq!(frame.body[18..22], x.to_be_bytes());
                    assert_eq!(frame.body[26..30], z.to_be_bytes());
                    let height = if id % 2 == 0 {
                        1.297_397_4_f32
                    } else {
                        1.777_397_4
                    };
                    assert_eq!(frame.body[22..26], height.to_be_bytes());
                }
            }
            if frame.opcode == 0x03f2 {
                let slot = frame.body[116];
                structures.push(slot);
                assert_eq!(
                    u32::from_be_bytes(frame.body[8..12].try_into().unwrap()),
                    2000 + u32::from(slot)
                );
                assert!(crate::aral_spawn::recordable(&frame.body[..122]));
            }
        }
        if initial {
            assert!(talents.is_empty());
        } else {
            assert_eq!(
                talents,
                [vec![0, 0, 5, 220, 0, 1], vec![0, 0, 5, 223, 255, 0]]
            );
        }
        assert!(declared, "own actor declaration precedes readiness");
        if initial {
            assert_eq!(structures, (0..16).rev().collect::<Vec<_>>());
        } else {
            assert!(structures.is_sorted_by(|left, right| left > right));
            assert_eq!(
                structures
                    .into_iter()
                    .filter(|slot| matches!(slot, 0 | 2 | 8 | 10))
                    .collect::<Vec<_>>(),
                [10, 8, 2, 0]
            );
        }
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
        play_six_seats(Scenario::Crystal).await;
    }

    #[tokio::test]
    async fn six_authenticated_seats_receive_one_result_after_a_team_surrenders() {
        play_six_seats(Scenario::Surrender).await;
    }

    #[tokio::test]
    async fn encrypted_aral_shop_replays_measured_components_and_reconnects() {
        play_six_seats(Scenario::Shop).await;
    }

    #[tokio::test]
    async fn encrypted_player_and_observer_receive_declared_wave_destinations() {
        play_six_seats(Scenario::LanePaths).await;
    }

    #[tokio::test]
    async fn tower_release_reaches_the_registered_players_and_observer_before_impact() {
        play_six_seats(Scenario::NpcAttack).await;
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Scenario {
        NpcAttack,
        Crystal,
        Surrender,
        Shop,
        LanePaths,
    }

    async fn play_six_seats(scenario: Scenario) {
        let surrender = scenario == Scenario::Surrender;
        let room_id = Uuid::new_v4();
        let rooms = Rooms::new(Ipv4Addr::LOCALHOST.into(), "127.0.0.1".into(), 1, None);
        let players: Vec<_> = (0..SEATS).map(|_| Uuid::new_v4()).collect();
        let mut levels = vec![halcyon_data::TalentLevels::new(); SEATS];
        levels[0].insert(273, std::num::NonZeroU8::new(1).unwrap());
        let reservation = Reservation {
            talent_levels: levels,
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
        let address = rooms.reserve(reservation.clone()).unwrap().address;
        assert!(rooms.reserve(reservation.clone()).is_ok());
        let mut changed = reservation;
        changed.talent_levels[0].clear();
        assert!(rooms.reserve(changed).is_err());
        // Test roster uses Idris, one of the three evidenced numeric grants.
        rooms.room(room_id).unwrap().lock().unwrap().world.entities[0].type_id = 273;
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

        for player in &players[1..] {
            let notice = until(&mut clients[0], &cipher, 0x03e8).await;
            assert_eq!(
                notice.fields().unwrap(),
                Fields::PlayerUuid(&player.to_string())
            );
        }
        for refresh in 0..3 {
            let request = Message::new(0x0458, [0; 6]);
            clients[0]
                .write_all(&cipher.encode(&request).unwrap())
                .await
                .unwrap();
            assert_eq!(next(&mut clients[0], &cipher).await.opcode, 0x0454);
            let catalog_len = rooms.room(room_id).unwrap().lock().unwrap().catalog.len();
            for _ in 0..catalog_len {
                assert_eq!(next(&mut clients[0], &cipher).await.opcode, 0x0453);
            }
            let first = next(&mut clients[0], &cipher).await;
            assert_eq!(first.opcode, 0x045a);
            if refresh == 0 {
                let sync = next(&mut clients[0], &cipher).await;
                let mut expected = [0; 86];
                for (i, value) in [(3, 1), (7, 5), (76, 1), (80, 3)] {
                    expected[i] = value;
                }
                assert_eq!(sync, Message::new(0x0480, expected));
                let second = next(&mut clients[0], &cipher).await;
                assert_eq!(second.opcode, 0x045a);
                assert_eq!(first.body[8..], second.body[8..]);
            }
            assert_eq!(next(&mut clients[0], &cipher).await, request);
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
                    0x03e8 | 0x045a => {}
                    0x03ee => roster_count += 1,
                    0x046c => break,
                    other => panic!("unexpected frame {other:#x} before world request"),
                }
            }
            assert_eq!(roster_count, SEATS);
        }
        for (index, socket) in clients.iter_mut().enumerate() {
            request_scene(socket, &cipher, 1500 + index as u32, true).await;
        }
        // The first socket had entered before the other five made their requests.
        for seat in 1..SEATS {
            let actor = 1500 + seat as u32;
            let handle = until(&mut clients[0], &cipher, 0x03ed).await;
            assert!(
                matches!(handle.fields().unwrap(), Fields::HandleRecord {text:b"Guest", field64,lookup, ..}
                if field64 == seat as u32 && lookup == actor)
            );
            assert_eq!(
                next(&mut clients[0], &cipher).await,
                Message::new(0x046e, [0; 6])
            );
            let confirmation = next(&mut clients[0], &cipher).await;
            assert_eq!(confirmation.opcode, 0x0471);
            assert_eq!(confirmation.body[..4], actor.to_be_bytes());
            assert_eq!(confirmation.body[4..], [1, 0]);
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
        assert!(
            room.lock()
                .unwrap()
                .seats
                .iter()
                .all(|seat| seat.world_actor_written)
        );
        if scenario == Scenario::Shop {
            super::shop_tests::shop_sequence(
                &mut clients,
                &cipher,
                &rooms,
                room_id,
                &address,
                players[0],
            )
            .await;
            return;
        }
        if !surrender {
            let socket = &mut clients[0];
            let learn = Message::new(0x0436, [1, 0, 0, 0, 0, 0]);
            socket
                .write_all(&cipher.encode(&learn).unwrap())
                .await
                .unwrap();
            until(socket, &cipher, 0x043a).await;
            let mut body = [0; 14];
            body[4..8].copy_from_slice(&(-0.0_f32).to_be_bytes());
            body[8..12].copy_from_slice(&0.88_f32.to_be_bytes());
            body[12] = 3;
            body[..4].copy_from_slice(&1000_f32.to_be_bytes());
            let invalid = Message::new(0x0412, body);
            socket
                .write_all(&cipher.encode(&invalid).unwrap())
                .await
                .unwrap();
            let barrier = Message::new(0, [0; 6]);
            socket
                .write_all(&cipher.encode(&barrier).unwrap())
                .await
                .unwrap();
            loop {
                let frame = next(socket, &cipher).await;
                assert!(!matches!(frame.opcode, 0x0412 | 0x0416));
                if frame == barrier {
                    break;
                }
            }
            body[..4].copy_from_slice(&(-74.0_f32).to_be_bytes());
            let cast = Message::new(0x0412, body);
            socket
                .write_all(&cipher.encode(&cast).unwrap())
                .await
                .unwrap();
            loop {
                let frame = next(socket, &cipher).await;
                assert_ne!(frame.opcode, 0x0416);
                if frame.opcode == 0x0412 {
                    assert_eq!(frame, cast);
                    break;
                }
            }
            let release = until(socket, &cipher, 0x0416).await;
            assert_eq!(release.body[..4], 1500_u32.to_be_bytes());
            assert_eq!(release.body[4..17], cast.body[..13]);
        }
        let point =
            |x: f32, z: f32| Message::new(0x03f5, [x.to_be_bytes(), z.to_be_bytes()].concat());
        for (seat, opcode, x, z) in [
            (0, 0x03f5, -74.0, 0.88),
            (3, 0x03f4, 75.0, -5.06),
            (0, 0x03f4, -74.0, 0.88),
        ] {
            let mut request = point(x, z);
            request.opcode = opcode;
            clients[seat]
                .write_all(&cipher.encode(&request).unwrap())
                .await
                .unwrap();
            let reply = until(&mut clients[seat], &cipher, 0x03f8).await;
            assert_eq!(
                reply.fields().unwrap(),
                Fields::MoveAction {
                    selector: 16 + seat as u8,
                    x,
                    y: z,
                    remaining: &[0; 5]
                }
            );
        }
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let m = next(&mut clients[0], &cipher).await;
                if let Fields::Position { entity: 1500, x, y } = m.fields().unwrap() {
                    assert!((-78.18..=-74.0).contains(&x) && x > -78.18);
                    assert!((y - 0.88).abs() < 0.01);
                    break;
                }
            }
        })
        .await
        .unwrap();
        let barrier = Message::new(0, [0; 6]);
        // At the two starting pads, each side sees only its own routes.
        for (viewer, expected) in [(1, vec![19]), (2, vec![16, 16])] {
            clients[viewer]
                .write_all(&cipher.encode(&barrier).unwrap())
                .await
                .unwrap();
            let mut slots = Vec::new();
            loop {
                let response = next(&mut clients[viewer], &cipher).await;
                if response == barrier {
                    break;
                }
                if response.opcode == 0x03f8 {
                    slots.push(response.body[0]);
                }
            }
            assert_eq!(slots, expected);
        }
        // The server acknowledges the requested target only after validating the attack.
        {
            let mut state = room.lock().unwrap();
            state.world.entities[0].position = halcyon_sim::Position { x: 1., y: 0. };
            state.world.entities[1].position = halcyon_sim::Position { x: 4., y: 0. };
        }
        clients[0]
            .write_all(&cipher.encode(&point(2., 0.)).unwrap())
            .await
            .unwrap();
        assert_eq!(until(&mut clients[0], &cipher, 0x03f8).await.body[0], 16);
        assert_eq!(until(&mut clients[1], &cipher, 0x03f8).await.body[0], 16);
        for target in [1500_u32, 19999, 1501] {
            let mut body = target.to_be_bytes().to_vec();
            body.push(0);
            clients[0]
                .write_all(&cipher.encode(&Message::new(0x0424, body)).unwrap())
                .await
                .unwrap();
        }
        let ack = until(&mut clients[0], &cipher, 0x0424).await;
        assert_eq!(ack.body, [0, 0, 5, 221, 0, 0]);
        let chosen = halcyon_protocol::TalentSelection::Slot0;
        let expected = halcyon_protocol::EquippedTalent {
            actor: 1500,
            selection: chosen,
            level: 1,
        };
        clients[0]
            .write_all(&cipher.encode(&chosen.request()).unwrap())
            .await
            .unwrap();
        for socket in &mut clients {
            let announced = until(socket, &cipher, 0x0484).await;
            assert_eq!(announced.body, [0; 6]);
            let equipped = next(socket, &cipher).await;
            assert_eq!(
                equipped.fields().unwrap(),
                Fields::Live(halcyon_protocol::LiveFields::TalentEquipped(expected))
            );
        }
        clients[0]
            .write_all(&cipher.encode(&chosen.request()).unwrap())
            .await
            .unwrap();
        until(&mut clients[0], &cipher, 0x0481).await;
        clients[1]
            .write_all(&cipher.encode(&barrier).unwrap())
            .await
            .unwrap();
        loop {
            let frame = next(&mut clients[1], &cipher).await;
            if frame == barrier {
                break;
            }
            assert!(!matches!(frame.opcode, 0x0484 | 0x0481));
        }
        let (observer_address, grant) = rooms.observe(room_id, Uuid::new_v4()).unwrap();
        let mut observer = TcpStream::connect(observer_address).await.unwrap();
        observer
            .write_all(&hello("localhost").unwrap())
            .await
            .unwrap();
        let mut ack = [0; 5];
        observer.read_exact(&mut ack).await.unwrap();
        assert_eq!(ack, ACK_FRAME);
        observer
            .write_all(
                &cipher
                    .encode(&Message::new(0x03e8, grant.to_string().as_bytes()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let restored = until(&mut observer, &cipher, 0x0481).await;
        assert_eq!(
            restored.fields().unwrap(),
            Fields::Live(halcyon_protocol::LiveFields::TalentEquipped(expected))
        );
        let clear = halcyon_protocol::TalentSelection::NoTalent;
        clients[0]
            .write_all(&cipher.encode(&clear.request()).unwrap())
            .await
            .unwrap();
        for socket in clients.iter_mut().chain(std::iter::once(&mut observer)) {
            let cleared = until(socket, &cipher, 0x0481).await;
            assert_eq!(cleared.body, [0, 0, 5, 220, 255, 0]);
        }
        if scenario == Scenario::NpcAttack {
            let wire_tower = {
                let mut state = room.lock().unwrap();
                let tower = state
                    .world
                    .entities
                    .iter()
                    .find(|e| state.world.aral_structure_slot(e.id) == Some(7))
                    .unwrap();
                let (tower_id, pos) = (tower.id, tower.position);
                state.world.entities[1].position = halcyon_sim::Position {
                    x: pos.x + 1.,
                    y: pos.y,
                };
                for _ in 0..14 {
                    state.tick();
                }
                u32::from(crate::messages::wire_id(tower_id))
            };
            for socket in clients
                .iter_mut()
                .take(2)
                .chain(std::iter::once(&mut observer))
            {
                tokio::time::timeout(Duration::from_secs(2), async {
                    let mut releases = 0;
                    loop {
                        let msg = next(socket, &cipher).await;
                        if msg.opcode == 0x415
                            && msg.body[..4] == wire_tower.to_be_bytes()
                            && msg.body[4..8] == 1501u32.to_be_bytes()
                        {
                            assert_eq!(msg.body[8], 0);
                            releases += 1;
                        }
                        if msg.opcode == 0x41e
                            && msg.body[..4] == 1501u32.to_be_bytes()
                            && msg.body[4..8] == wire_tower.to_be_bytes()
                        {
                            assert_eq!(releases, 1);
                            break;
                        }
                    }
                })
                .await
                .unwrap();
            }
            rooms.revoke_observer(room_id, grant).unwrap();
            return;
        }
        if scenario == Scenario::LanePaths {
            let (wire_actor, destination) = {
                let mut state = room.lock().unwrap();
                state.world.tick = 15 * halcyon_sim::TICK_HZ - 1;
                for _ in 0..10 {
                    state.tick();
                }
                let wave = state
                    .world
                    .entities
                    .iter()
                    .find(|e| e.kind == Kind::Minion && e.team == 0)
                    .unwrap();
                (
                    u32::from(crate::messages::wire_id(wave.id)),
                    wave.lane_step().unwrap().0,
                )
            };
            for viewer in [&mut clients[2], &mut observer] {
                let mut registered = None;
                let mut rows = Vec::new();
                let mut saw_path = false;
                tokio::time::timeout(Duration::from_secs(2), async {
                    loop {
                        let m = next(viewer, &cipher).await;
                        if matches!(m.opcode, 0x03f2 | 0x03f3)
                            && m.body[8..12] == wire_actor.to_be_bytes()
                        {
                            registered = Some(m.body[if m.opcode == 0x03f2 { 116 } else { 745 }]);
                        }
                        if m.opcode == 0x03f8 && Some(m.body[0]) == registered {
                            let expected =
                                crate::point_move::destination(registered.unwrap(), destination);
                            assert_eq!(m.body[..9], expected.body);
                            saw_path = true;
                        }
                        if m.opcode == 0x042b && m.body[..4] == wire_actor.to_be_bytes() {
                            assert!(saw_path && registered.is_some());
                            rows.push(<[u8; 4]>::try_from(&m.body[4..8]).unwrap());
                            if rows.len() == 7 {
                                assert_eq!(
                                    rows,
                                    [
                                        [0, 1, 0, 0],
                                        [1, 1, 15, 0],
                                        [3, 1, 0, 0],
                                        [4, 1, 0, 0],
                                        [5, 1, 0, 0],
                                        [6, 1, 0, 0],
                                        [7, 1, 0, 0]
                                    ]
                                );
                                break;
                            }
                        }
                    }
                })
                .await
                .unwrap();
            }
            let hidden = wire_actor.to_be_bytes();
            clients[3]
                .write_all(&cipher.encode(&barrier).unwrap())
                .await
                .unwrap();
            loop {
                let m = next(&mut clients[3], &cipher).await;
                if m == barrier {
                    break;
                }
                if m.opcode == 0x042b {
                    assert_ne!(m.body[..4], hidden);
                }
                if matches!(m.opcode, 0x03f2 | 0x03f3) {
                    assert_ne!(m.body[8..12], hidden);
                }
            }
        }
        rooms.revoke_observer(room_id, grant).unwrap();
        if scenario == Scenario::LanePaths {
            return;
        }
        // Re-equip so the reconnect below restores both a positive selection and an explicit clear.
        clients[0]
            .write_all(&cipher.encode(&chosen.request()).unwrap())
            .await
            .unwrap();
        for socket in &mut clients {
            until(socket, &cipher, 0x0481).await;
        }

        // A paid/earned slot request has no trusted level in this room.
        // The next authoritative response must be for the explicit NoTalent request.
        for selection in [
            halcyon_protocol::TalentSelection::Slot2,
            halcyon_protocol::TalentSelection::NoTalent,
        ] {
            clients[3]
                .write_all(&cipher.encode(&selection.request()).unwrap())
                .await
                .unwrap();
        }
        let talent = until(&mut clients[3], &cipher, 0x0481).await;
        assert_eq!(
            talent.fields().unwrap(),
            Fields::Live(halcyon_protocol::LiveFields::TalentEquipped(
                halcyon_protocol::EquippedTalent {
                    actor: 1503,
                    selection: halcyon_protocol::TalentSelection::NoTalent,
                    level: 0
                }
            ))
        );
        assert_eq!(rooms.detail(room_id).unwrap()["botPlayers"], 0);
        clients[0]
            .write_all(&cipher.encode(&Message::movement(4., 0.)).unwrap())
            .await
            .unwrap();
        until(&mut clients[0], &cipher, 0x042e).await;
        assert_eq!(rooms.detail(room_id).unwrap()["phase"], "running");

        clients[3].shutdown().await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while room.lock().unwrap().seats[3].peers[0].is_some() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let mut returned = TcpStream::connect(&address).await.unwrap();
        returned
            .write_all(&hello("localhost").unwrap())
            .await
            .unwrap();
        let mut ack = [0; 5];
        returned.read_exact(&mut ack).await.unwrap();
        assert_eq!(ack, ACK_FRAME);
        returned
            .write_all(
                &cipher
                    .encode(&Message::new(0x03e8, players[3].to_string().as_bytes()))
                    .unwrap(),
            )
            .await
            .unwrap();
        until(&mut returned, &cipher, 0x046c).await;
        request_scene(&mut returned, &cipher, 1503, false).await;
        ready(&mut returned, &cipher).await;
        returned
            .write_all(&cipher.encode(&point(75.0, -5.06)).unwrap())
            .await
            .unwrap();
        assert_eq!(until(&mut returned, &cipher, 0x03f8).await.body[0], 19);
        clients[3] = returned;

        if surrender {
            let yes = cipher.encode(&Message::new(0x0472, [1])).unwrap();
            for (votes, seat) in [1, 3, 5].into_iter().enumerate() {
                clients[seat].write_all(&yes).await.unwrap();
                for teammate in [1, 3, 5] {
                    let board = until(&mut clients[teammate], &cipher, 0x0473).await;
                    assert_eq!(board.body[148..151], [2, if votes == 2 { 2 } else { 1 }, 3]);
                    assert_eq!(
                        board.body[68..71].iter().filter(|v| **v == 1).count(),
                        votes + 1
                    );
                }
            }
            let mut room = room.lock().unwrap();
            assert_eq!(room.phase, "ending");
            assert!(
                room.world
                    .entities
                    .iter()
                    .filter(|e| e.kind == Kind::Crystal)
                    .all(|e| e.hp > 0.)
            );
            room.phase_start = Instant::now() - Duration::from_secs(14);
            room.tick();
            assert!(!*room.stop.borrow());
            room.phase_start = Instant::now() - Duration::from_secs(15);
            room.tick();
            assert!(*room.stop.borrow());
        } else {
            let mut room = room.lock().unwrap();
            let crystal = room
                .world
                .entities
                .iter_mut()
                .find(|e| e.kind == Kind::Crystal && e.team == 1)
                .unwrap();
            let (target, position) = (crystal.id, crystal.position);
            crystal.hp = 0.1;
            room.world.entities[0].position = position;
            room.world.tick = 15;
            room.world
                .command(1, halcyon_sim::Command::Attack(target))
                .unwrap();
            for _ in 0..5 {
                room.tick();
            }
            assert_eq!(room.phase, "ending");
            assert!(!*room.stop.borrow());
            room.phase_start = Instant::now() - Duration::from_secs(6);
            room.tick();
            assert!(*room.stop.borrow());
        }
        for socket in &mut clients {
            let message = if surrender {
                loop {
                    let message = next(socket, &cipher).await;
                    assert!(!matches!(message.opcode, 0x0452 | 0x0473));
                    if message.opcode == 0x048d {
                        break message;
                    }
                }
            } else {
                let postgame = until(socket, &cipher, 0x0452).await;
                assert_eq!(postgame.body, [0; 6]);
                let death = until(socket, &cipher, 0x0430).await;
                assert_eq!(
                    u32::from_be_bytes(death.body[..4].try_into().unwrap()),
                    2009
                );
                until(socket, &cipher, 0x048d).await
            };
            let Fields::MatchStatistics(report) = message.fields().unwrap() else {
                panic!("expected a native statistics report");
            };
            for seat in 0..SEATS {
                let stats = report.slot(seat).unwrap();
                assert_eq!(stats.player, 1500 + seat as u32);
                assert_eq!(stats.team, 1 + (seat % 2) as u8);
            }
            assert_eq!(report.slot(SEATS).unwrap().player, u32::MAX);
            let victory = next(socket, &cipher).await;
            assert_eq!(
                (victory.opcode, victory.body.as_slice()),
                (0x03f1, &[0, 0, 0, 1, if surrender { 2 } else { 0 }, 0][..])
            );
        }
    }
}
