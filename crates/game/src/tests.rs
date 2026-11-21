use super::*;
#[tokio::test]
async fn returning_socket_records_its_own_first_message() {
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
    for _ in 0..2 {
        let (tx, _rx) = mpsc::channel(16);
        let (seat, ch, generation, _) = room.attach(player, tx, watch::channel(false).0).unwrap();
        for _ in 0..2 {
            room.receive(
                seat,
                ch,
                Message::known(crate::Opcode::RequestSceneSnapshot, [0; 6]),
            )
            .unwrap();
        }
        room.detach(seat, ch, generation);
    }
    let events: Vec<_> = room
        .events
        .iter()
        .map(responses::value)
        .filter(|event| event["opcode"] == "0x046E")
        .collect();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["generation"], 1);
    assert_eq!(events[1]["generation"], 2);
}
#[tokio::test]
async fn ranked_and_solo_draft_requests_repeat_the_registered_catalog_and_current_roster() {
    for (queue, mode, count) in [
        ("ranked", "*GameMode_HF_Ranked*", 6),
        ("solo", "*GameMode_5v5_SoloBots*", 1),
    ] {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        let players = (0..count).map(|_| Uuid::new_v4()).collect::<Vec<_>>();
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: id,
                players: players.clone(),
                teams: vec![],
                mode: queue.into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut room = shared.lock().unwrap();
        let (tx, _rx) = mpsc::channel(16);
        let (_, _, _, initial) = room
            .attach(players[0], tx, watch::channel(false).0)
            .unwrap();
        assert_eq!(initial[0].opcode, crate::wire_opcode::MatchSettings);
        assert_eq!(initial[1].opcode, crate::wire_opcode::DraftModeAnnouncement);
        assert_eq!(&initial[1].body[4..4 + mode.len()], mode.as_bytes());
        for _ in 0..3 {
            let request = Message::known(crate::Opcode::DraftRefreshRequest, [0; 6]);
            let result = room.receive(0, 0, request.clone()).unwrap();
            assert_eq!(result.last(), Some(&request));
            assert_eq!(result[..result.len() - 1], initial[1..]);
        }
        let malformed = Message::known(crate::Opcode::DraftRefreshRequest, [0, 1, 0, 0, 0, 0]);
        assert_eq!(room.receive(0, 0, malformed.clone()).unwrap(), [malformed]);
    }
}
#[tokio::test]
async fn ranked_three_per_side_from_platform_to_each_draft_seat() {
    use halcyon_platform::{AppState, PlatformConfig, plan_teams_for_mode};
    let dir = tempfile::tempdir().unwrap();
    let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 2, None);
    let app = AppState::open(
        PlatformConfig {
            database_path: dir.path().join("ranked.sqlite"),
            public_address: "localhost:8443".into(),
            seats_per_match: 10,
        },
        Arc::new(rooms.clone()),
    )
    .unwrap();
    assert_eq!(app.seats_for_mode("ranked"), 6);
    assert_eq!(app.seats_for_mode("ranked_5v5"), 10);
    assert_eq!(plan_teams_for_mode("ranked", &[3, 3]), [0, 0, 0, 1, 1, 1]);
    let mut guests = Vec::new();
    for i in 0..6 {
        guests.push(app.create_guest(format!("Seat{i}")).await.unwrap());
    }
    assert!(
        app.join_with_seats(&guests[0].session_token, "ranked".into(), 10)
            .await
            .is_err()
    );
    assert!(
        app.federated_join(&guests[0].session_token, Uuid::new_v4(), "ranked".into(), 5)
            .await
            .is_err()
    );
    for guest in &guests {
        app.join(&guest.session_token, "ranked".into())
            .await
            .unwrap();
    }
    let id = app
        .profile(&guests[0].session_token)
        .await
        .unwrap()
        .lobby
        .unwrap()
        .match_id
        .unwrap();
    for guest in &guests {
        app.accept(&guest.session_token, id).await.unwrap();
    }
    let shared = rooms.room(id).unwrap();
    let mut room = shared.lock().unwrap();
    assert_eq!(room.picks.len(), 6);
    let heroes: Vec<_> = room
        .world
        .entities
        .iter()
        .filter(|e| e.kind == halcyon_sim::Kind::Hero)
        .collect();
    assert_eq!(heroes.len(), 6);
    assert!(heroes.iter().all(|e| !e.bot && e.gold == 0));
    assert_eq!(heroes.iter().filter(|e| e.team == 0).count(), 3);
    assert!(
        heroes
            .iter()
            .filter(|e| e.team == 0)
            .all(|e| e.position.x == heroes[0].position.x)
    );
    assert_ne!(heroes[0].position.x, heroes[1].position.x);
    drop(heroes);
    for (seat, guest) in guests.iter().enumerate() {
        let (tx, _rx) = mpsc::channel(16);
        let (_, _, _, opening) = room
            .attach(guest.player_id, tx, watch::channel(false).0)
            .unwrap();
        assert_eq!(&opening[0].body[16..18], &[3, 3]);
        assert_eq!(opening[0].body[100], room.world.entities[seat].team + 1);
        let roster = opening.last().unwrap();
        assert_eq!(roster.opcode, crate::wire_opcode::DraftRosterFull);
        for (i, row) in roster.body.as_chunks::<161>().0.iter().enumerate() {
            assert_eq!(row[8], u8::from(i < 6));
            if let Some(participant) = guests.get(i) {
                assert_eq!(&row[105..141], participant.player_id.to_string().as_bytes());
                assert_eq!(row[16], room.world.entities[i].team + 1);
            }
        }
        let req = Message::known(crate::Opcode::DraftRefreshRequest, [0; 6]);
        let repeat = room.receive(seat, 0, req).unwrap();
        assert_eq!(&repeat[..repeat.len() - 1], &opening[1..]);
    }
    drop(room);
    let players: Vec<_> = guests.iter().map(|g| g.player_id).collect();
    for (roster, teams) in [
        (players[..5].to_vec(), vec![]),
        (players.clone(), vec![0, 0, 0, 0, 1, 1]),
    ] {
        assert!(
            rooms
                .reserve(Reservation {
                    talent_levels: Vec::new(),
                    match_id: Uuid::new_v4(),
                    mode: "ranked".into(),
                    players: roster,
                    teams,
                    difficulty: None
                })
                .is_err()
        );
    }
    let five = Uuid::new_v4();
    rooms
        .reserve(Reservation {
            talent_levels: Vec::new(),
            match_id: five,
            mode: "ranked_5v5".into(),
            players,
            teams: vec![],
            difficulty: None,
        })
        .unwrap();
    assert_eq!(rooms.room(five).unwrap().lock().unwrap().picks.len(), 10);
}

#[tokio::test]
async fn shop_uses_authenticated_seat_and_authoritative_gold() {
    let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
    let id = Uuid::new_v4();
    rooms
        .reserve(Reservation {
            talent_levels: Vec::new(),
            match_id: id,
            players: vec![Uuid::new_v4()],
            teams: vec![0],
            mode: "solo".into(),
            difficulty: None,
        })
        .unwrap();
    let shared = rooms.room(id).unwrap();
    let mut room = shared.lock().unwrap();
    room.phase(crate::RoomPhase::Running);
    room.world.entities[0].gold = 300;
    let buy = |actor: u32| {
        Message::known(
            crate::Opcode::BuyItem,
            [actor.to_be_bytes(), 458u32.to_be_bytes()].concat(),
        )
    };
    assert!(room.receive(0, 1, buy(1501)).unwrap().is_empty());
    assert!(room.receive(0, 0, buy(1500)).unwrap().is_empty());
    assert_eq!(room.world.entities[0].gold, 300);
    let ack = room.receive(0, 1, buy(1500)).unwrap();
    assert_eq!(
        ack.iter().map(|m| m.opcode).collect::<Vec<_>>(),
        [
            crate::wire_opcode::BuyItem,
            crate::wire_opcode::ModifyActorResource,
            crate::wire_opcode::ItemGrant,
            crate::wire_opcode::ModifyActorAttribute
        ]
    );
    assert_eq!(room.world.entities[0].gold, 0);
    assert!(matches!(
        ack[1].fields().unwrap(),
        Fields::EntityStat {
            entity: 1500,
            value: -300.,
            stat: 6
        }
    ));
    assert!(matches!(
        ack[2].fields().unwrap(),
        Fields::ItemGrant {
            actor: 1500,
            item: 458,
            ..
        }
    ));
    assert!(room.receive(0, 1, buy(1500)).unwrap().is_empty());
    room.world.entities[0].gold = 850;
    let upgrade = Message::known(
        crate::Opcode::BuyItem,
        [1500u32.to_be_bytes(), 505u32.to_be_bytes()].concat(),
    );
    let upgraded = room.receive(0, 1, upgrade).unwrap();
    assert_eq!(
        upgraded.iter().map(|m| m.opcode).collect::<Vec<_>>(),
        [
            crate::wire_opcode::BuyItem,
            crate::wire_opcode::ModifyActorResource,
            crate::wire_opcode::ModifyActorAttribute,
            crate::wire_opcode::ConsumeItem,
            crate::wire_opcode::ItemGrant,
            crate::wire_opcode::ModifyActorAttribute
        ]
    );
    assert_eq!(&upgraded[3].body[4..8], &ack[2].body[8..12]);
    assert_eq!(room.world.entities[0].inventory.len(), 1);
    room.world.entities[0].gold = 300;
    let request = |template: u32| {
        Message::known(
            Opcode::BuyItem,
            [1500u32.to_be_bytes(), template.to_be_bytes()].concat(),
        )
    };
    assert!(room.receive(0, 1, request(549)).unwrap().is_empty());
    assert!(room.receive(0, 1, request(495)).unwrap().is_empty());
    let granted = room.receive(0, 1, request(532)).unwrap();
    assert!(granted.iter().any(|m| matches!(
        m.fields(),
        Ok(Fields::ItemGrant {
            actor: 1500,
            item: 532,
            ..
        })
    )));
    assert_eq!(room.world.entities[0].gold, 0);
}
