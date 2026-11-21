use crate::{MatchHost, Reservation, Rooms};
use halcyon_protocol::{Message, Opcode};
use std::{path::Path, sync::Arc};
use tokio::sync::{mpsc, watch};
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires HALCYON_DELIVERY"]
async fn aral_reroll_needs_an_unlocked_authenticated_seat_and_is_once_per_draft() {
    let data =
        halcyon_data::GameData::load(Path::new(&std::env::var_os("HALCYON_DELIVERY").unwrap()))
            .unwrap();
    let rooms = Rooms::new(
        "127.0.0.1".parse().unwrap(),
        "127.0.0.1".into(),
        2,
        Some(Arc::new(data)),
    );
    for enabled in [false, true] {
        rooms.set_free_aral_reroll(enabled);
        let id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: id,
                players: (0..6).map(|_| Uuid::new_v4()).collect(),
                teams: vec![0, 0, 0, 1, 1, 1],
                mode: "casual_aral".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut room = shared.lock().unwrap();
        let mut receivers = Vec::new();
        for i in 0..2 {
            let (tx, rx) = mpsc::channel(8);
            let player = room.players[i];
            room.attach(player, tx, watch::channel(false).0).unwrap();
            receivers.push(rx);
        }
        for rx in &mut receivers {
            while rx.try_recv().is_ok() {}
        }
        let original = room.world.entities[0].type_id;
        let other = room.world.entities[1].type_id;
        let ask = Message::known(Opcode::DraftRerollHero, [0; 6]);
        assert!(room.receive(0, 1, ask.clone()).unwrap().is_empty());
        assert!(
            room.receive(
                0,
                0,
                Message::known(Opcode::DraftRerollHero, [1, 0, 0, 0, 0, 0])
            )
            .unwrap()
            .is_empty()
        );
        room.seats[0].locked = true;
        assert!(room.receive(0, 0, ask.clone()).unwrap().is_empty());
        room.seats[0].locked = false;
        let result = room.receive(0, 0, ask.clone()).unwrap();
        if enabled {
            assert_eq!(
                result.iter().map(Message::known_opcode).collect::<Vec<_>>(),
                [
                    Some(Opcode::DraftRosterAral),
                    Some(Opcode::DraftRerollHero),
                    Some(Opcode::DraftSelectSkin)
                ]
            );
            assert_ne!(room.world.entities[0].type_id, original);
            assert_eq!(room.world.entities[1].type_id, other);
            assert_eq!(receivers[1].try_recv().unwrap().into_records(), result[..2]);
            let generation = room.seats[0].peers[0].as_ref().unwrap().generation;
            let player = room.players[0];
            room.detach(0, 0, generation);
            let (tx, _rx) = mpsc::channel(8);
            room.attach(player, tx, watch::channel(false).0).unwrap();
            assert!(room.receive(0, 0, ask).unwrap().is_empty());
            assert!(receivers[1].try_recv().is_err());
        } else {
            assert!(result.is_empty());
            assert_eq!(room.world.entities[0].type_id, original);
            assert!(receivers[1].try_recv().is_err());
        }
    }
}
