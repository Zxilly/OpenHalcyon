use super::*;
use crate::{Command, Rooms, wire_opcode};
use halcyon_data::{EntityTemplate, TalentLevels};
use halcyon_protocol::{Fields, LiveFields, TalentSelection, ce::NativeMode};
use halcyon_types::{MatchHost, Reservation};
use std::num::NonZeroU8;
use tokio::sync::{mpsc, watch};
use uuid::Uuid;

fn six_players() -> Vec<Uuid> {
    (0..6).map(|_| Uuid::new_v4()).collect()
}

#[tokio::test]
async fn casual_3v3_full_room_lifecycle_draft_opening_jungle_shop_and_victory() {
    let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
    let id = Uuid::new_v4();
    let players = six_players();
    rooms
        .reserve(Reservation {
            talent_levels: Vec::new(),
            match_id: id,
            players: players.clone(),
            teams: vec![0, 1, 0, 1, 0, 1],
            mode: "3v3_pvp_casual".into(),
            difficulty: None,
        })
        .unwrap();
    let shared = rooms.room(id).unwrap();
    let mut rxs = Vec::new();
    for (seat, &p) in players.iter().enumerate() {
        let mut room = shared.lock().unwrap();
        let (tx, rx) = mpsc::channel(32);
        let (_, _, _, initial) = room.attach(p, tx, watch::channel(false).0).unwrap();
        assert_eq!(initial[0].opcode, wire_opcode::MatchSettings);
        assert_eq!(&initial[0].body[16..18], &[3, 3]);
        assert!(
            String::from_utf8_lossy(&initial[0].body[32..96])
                .starts_with(NativeMode::CasualFold.asset())
        );
        assert_eq!(initial.last().unwrap().opcode, wire_opcode::DraftRosterFull);
        rxs.push(rx);
        // Lock hero in draft on channel 0.
        let lock = Message::known(crate::Opcode::DraftLockHero, []);
        room.receive(seat, 0, lock).unwrap();
    }
    {
        let room = shared.lock().unwrap();
        assert_eq!(room.phase, crate::RoomPhase::Loading);
        assert!(room.world.halcyon_fold_state().is_some());
        assert!(room.world.entities.iter().take(6).all(|e| e.gold == 600));
    }
    // Attach game connections (channel 1) for all 6 seats.
    for (seat, &p) in players.iter().enumerate() {
        let mut room = shared.lock().unwrap();
        let (tx, _rx) = mpsc::channel(32);
        let (_, ch, _, spawn_batch) = room.attach(p, tx, watch::channel(false).0).unwrap();
        assert_eq!(ch, 1);
        if seat == 0 {
            assert!(
                spawn_batch
                    .iter()
                    .any(|m| m.opcode == wire_opcode::SceneSettings)
            );
            assert!(
                spawn_batch
                    .iter()
                    .any(|m| m.opcode == wire_opcode::ModifyGameModeVar)
            );
            assert!(
                spawn_batch
                    .iter()
                    .filter(|m| m.opcode == wire_opcode::EntitySpawn)
                    .count()
                    >= 11
            );
            assert!(
                spawn_batch
                    .iter()
                    .any(|m| m.opcode == wire_opcode::HeroSpawn)
            );
        }
    }
    // Damage enemy crystal to shatter and verify reason 0 EndMatch.
    {
        let mut room = shared.lock().unwrap();
        assert_eq!(room.phase, crate::RoomPhase::Running);
        let crystal_idx = room
            .world
            .entities
            .iter()
            .position(|e| e.kind == Kind::Crystal && e.team == 1)
            .unwrap();
        let cid = room.world.entities[crystal_idx].id;
        room.world.entities[crystal_idx].hp = 1.0;
        room.world.entities[0].position = room.world.entities[crystal_idx].position;
        room.world.command(1, Command::Attack(cid)).unwrap();
        for _ in 0..20 {
            room.tick();
            if room.world.winner.is_some() {
                break;
            }
        }
        assert_eq!(room.world.winner, Some(0));
        let end = crate::result_frames::crystal_end("3v3_pvp_casual", &room.world).unwrap();
        assert!(matches!(
            end.fields().unwrap(),
            Fields::MatchRequest {
                team_word: 1,
                reason: 0,
                ..
            }
        ));
    }
}

#[tokio::test]
async fn blitz_room_roster_0459_talents_0440_flyouts_and_score_endmatch() {
    let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
    let id = Uuid::new_v4();
    let players = six_players();
    let mut grants: Vec<TalentLevels> = vec![Default::default(); 6];
    grants[0].insert(EntityTemplate::Ringo.as_raw(), NonZeroU8::new(10).unwrap());
    rooms
        .reserve(Reservation {
            talent_levels: grants,
            match_id: id,
            players: players.clone(),
            teams: vec![0, 1, 0, 1, 0, 1],
            mode: "blitz_3v3".into(),
            difficulty: None,
        })
        .unwrap();
    let shared = rooms.room(id).unwrap();
    for (seat, &p) in players.iter().enumerate() {
        let mut room = shared.lock().unwrap();
        let (tx, _rx) = mpsc::channel(32);
        let (_, _, _, initial) = room.attach(p, tx, watch::channel(false).0).unwrap();
        assert_eq!(initial[0].opcode, wire_opcode::MatchSettings);
        assert!(
            String::from_utf8_lossy(&initial[0].body[32..96])
                .starts_with(NativeMode::Blitz.asset())
        );
        let roster = initial.last().unwrap();
        assert_eq!(roster.opcode, wire_opcode::DraftRosterFull);
        let total = f32::from_be_bytes(roster.body[4..8].try_into().unwrap());
        assert!((total - 300.0).abs() < 0.01);
        room.receive(seat, 0, Message::known(crate::Opcode::DraftLockHero, []))
            .unwrap();
    }
    let mut game_rxs = Vec::new();
    for &p in &players {
        let mut room = shared.lock().unwrap();
        let (tx, rx) = mpsc::channel(32);
        let (_, ch, _, _) = room.attach(p, tx, watch::channel(false).0).unwrap();
        assert_eq!(ch, 1);
        game_rxs.push(rx);
    }
    while game_rxs[1].try_recv().is_ok() {}
    // Equip Blitz Talent Slot0 (Lv 10) for seat 0 and verify 0x0484 broadcast + weapon increase.
    {
        let mut room = shared.lock().unwrap();
        let before_wp = room.world.entities[0].weapon_power();
        let ack = room
            .receive(0, 1, TalentSelection::Slot0.request())
            .unwrap();
        assert_eq!(ack.len(), 2);
        assert!(matches!(
            ack[1].fields().unwrap(),
            Fields::Live(LiveFields::TalentEquipped(eq))
                if eq.actor == 1500 && eq.selection == TalentSelection::Slot0 && eq.level == 10
        ));
        assert!(room.world.entities[0].weapon_power() > before_wp);
    }
    let _talent_broadcast = game_rxs[1].try_recv().unwrap();

    // Trigger 15 Blitz points via 2 enemy turrets (+6), 3 enemy hero kills (+3), and 2 sentries/miner (+6).
    {
        let mut room = shared.lock().unwrap();
        for obj_idx in 0..5 {
            if obj_idx < 2 {
                let tid = room
                    .world
                    .entities
                    .iter()
                    .filter(|e| e.kind == Kind::Tower && e.team == 1)
                    .nth(obj_idx)
                    .unwrap()
                    .id;
                let i = room
                    .world
                    .entities
                    .iter()
                    .position(|e| e.id == tid)
                    .unwrap();
                room.world.entities[i].hp = 1.0;
                room.world.entities[0].position = room.world.entities[i].position;
                room.world.command(1, Command::Attack(tid)).unwrap();
            } else {
                let hero_idx = [1, 3, 5][obj_idx - 2];
                room.world.entities[hero_idx].hp = 1.0;
                room.world.entities[hero_idx].position = room.world.entities[0].position;
                let hid = room.world.entities[hero_idx].id;
                room.world.command(1, Command::Attack(hid)).unwrap();
            }
            for _ in 0..20 {
                room.tick();
                if room.world.entities[0].attack_target().is_none() {
                    break;
                }
            }
        }
        while room.world.winner.is_none() {
            let target_id = room
                .world
                .blitz_state()
                .unwrap()
                .objectives
                .iter()
                .find_map(|o| (o.team != 0).then_some(o.entity_id).flatten())
                .expect("living blitz objective");
            let idx = room
                .world
                .entities
                .iter()
                .position(|e| e.id == target_id)
                .unwrap();
            room.world.entities[idx].hp = 1.0;
            room.world.entities[0].position = room.world.entities[idx].position;
            room.world.command(1, Command::Attack(target_id)).unwrap();
            for _ in 0..20 {
                room.tick();
                if room.world.winner.is_some() {
                    break;
                }
            }
        }
        assert_eq!(room.world.winner, Some(0));
        assert!(room.world.blitz_state().unwrap().team_points[0] >= 15);
        let end = crate::result_frames::score_or_time_end("blitz_3v3", &room.world).unwrap();
        assert!(matches!(
            end.fields().unwrap(),
            Fields::MatchRequest {
                team_word: 1,
                reason: 3,
                ..
            }
        ));
    }
    let mut saw_flyout = false;
    let mut saw_mode_var = false;
    while let Ok(delivery) = game_rxs[1].try_recv() {
        for msg in delivery.into_records() {
            if msg.opcode == wire_opcode::ShowFlyoutText {
                saw_flyout = true;
            }
            if msg.opcode == wire_opcode::ModifyGameModeVar {
                saw_mode_var = true;
            }
        }
    }
    assert!(saw_flyout);
    assert!(saw_mode_var);
}

#[tokio::test]
async fn five_v_five_pvp_surrender_broadcasts_five_slot_board_and_settles() {
    let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
    let id = Uuid::new_v4();
    let players: Vec<Uuid> = (0..10).map(|_| Uuid::new_v4()).collect();
    rooms
        .reserve(Reservation {
            talent_levels: Vec::new(),
            match_id: id,
            players: players.clone(),
            teams: vec![0, 1, 0, 1, 0, 1, 0, 1, 0, 1],
            mode: "5v5_pvp_casual".into(),
            difficulty: None,
        })
        .unwrap();
    let shared = rooms.room(id).unwrap();
    for (seat, &p) in players.iter().enumerate() {
        let mut room = shared.lock().unwrap();
        let (tx, _rx) = mpsc::channel(32);
        room.attach(p, tx, watch::channel(false).0).unwrap();
        let lock = halcyon_protocol::draft::DraftAction::LockHero { confirm: 1 }.encode();
        room.receive(seat, 0, lock).unwrap();
    }
    let mut rxs = Vec::with_capacity(10);
    for &p in &players {
        let mut room = shared.lock().unwrap();
        let (tx, rx) = mpsc::channel(32);
        let (_, ch, _, _) = room.attach(p, tx, watch::channel(false).0).unwrap();
        assert_eq!(ch, 1);
        rxs.push(rx);
    }
    for rx in &mut rxs {
        while rx.try_recv().is_ok() {}
    }

    // Red team (seats 1, 3, 5, 7, 9) unanimously votes YES across ch=1 using padded and 1-byte votes.
    let yes_padded = Message::known(crate::Opcode::SetSurrenderStateRequest, [1, 0, 0, 0, 0, 0]);
    let yes_compact = Message::aral_surrender_request();
    let red_seats = [1usize, 3, 5, 7, 9];
    for (idx, &seat) in red_seats.iter().enumerate() {
        let mut room = shared.lock().unwrap();
        let vote = if idx % 2 == 0 {
            yes_padded.clone()
        } else {
            yes_compact.clone()
        };
        room.receive(seat, 1, vote).unwrap();
    }

    let boards: Vec<_> = rxs[1]
        .try_recv()
        .unwrap()
        .into_records()
        .into_iter()
        .chain(
            std::iter::from_fn(|| rxs[1].try_recv().ok())
                .flat_map(transport::Delivery::into_records),
        )
        .filter(|m| m.opcode == wire_opcode::TeamSurrenderStateChanged)
        .collect();
    assert_eq!(boards.len(), 5);
    let Fields::Live(LiveFields::TeamSurrenderBoard(final_board)) =
        boards.last().unwrap().fields().unwrap()
    else {
        panic!("expected TeamSurrenderBoard for 5v5");
    };
    assert_eq!(final_board.wire_team, 2);
    assert_eq!(final_board.count, 5);
    assert_eq!(final_board.phase, 2);
    assert_eq!(&final_board.actors[..5], &[1501, 1503, 1505, 1507, 1509]);
    assert_eq!(&final_board.votes[..5], &[1, 1, 1, 1, 1]);

    {
        let mut room = shared.lock().unwrap();
        assert_eq!(room.phase, crate::RoomPhase::Ending);
        assert_eq!(room.world.winner, Some(0));
        room.finish_world();
    }
    let mut saw_surrender_end = false;
    while let Ok(delivery) = rxs[0].try_recv() {
        for msg in delivery.into_records() {
            if msg.opcode == wire_opcode::EndMatch {
                assert_eq!(msg.body, [0, 0, 0, 1, 2, 0]);
                saw_surrender_end = true;
            }
        }
    }
    assert!(saw_surrender_end);
}

#[tokio::test]
async fn solo_and_three_per_side_surrender_pass_without_bot_stalls() {
    let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 4, None);
    // 1) 5v5 Solo Bots (`ch == 0`, 1 human + 9 bots) passes immediately on single human YES.
    let solo_id = Uuid::new_v4();
    let solo_player = Uuid::new_v4();
    rooms
        .reserve(Reservation {
            talent_levels: Vec::new(),
            match_id: solo_id,
            players: vec![solo_player],
            teams: Vec::new(),
            mode: "5v5_bots_solo".into(),
            difficulty: Some(halcyon_data::BotDifficulty::Easy),
        })
        .unwrap();
    let solo_room = rooms.room(solo_id).unwrap();
    let (tx, mut rx) = mpsc::channel(32);
    {
        let mut room = solo_room.lock().unwrap();
        room.attach(solo_player, tx, watch::channel(false).0)
            .unwrap();
        room.receive(0, 0, Message::known(crate::Opcode::DraftLockHero, []))
            .unwrap();
        room.phase(crate::RoomPhase::Running);
        room.seats[0].loading_roster_sent = true;
        room.seats[0].world_actor_written = true;
        // Drive ch=0 scene preamble confirmation to confirm gameplay stream.
        let snap = Message::known(crate::Opcode::RequestSceneSnapshot, [0; 6]);
        let owner = Message::known(crate::Opcode::ConfirmSceneOwner, [0, 0, 5, 220, 1, 0]);
        let ready = Message::known(crate::Opcode::SceneReadyConfirm, [0; 6]);
        room.receive(0, 0, snap).unwrap();
        room.receive(0, 0, owner).unwrap();
        room.receive(0, 0, ready).unwrap();
        assert!(room.seats[0].preamble.confirmed());
        room.receive(0, 0, Message::aral_surrender_request())
            .unwrap();
        assert_eq!(room.phase, crate::RoomPhase::Ending);
        assert_eq!(room.world.winner, Some(1));
    }
    let mut boards = Vec::new();
    while let Ok(delivery) = rx.try_recv() {
        for m in delivery.into_records() {
            if m.opcode == wire_opcode::TeamSurrenderStateChanged {
                boards.push(m);
            }
        }
    }
    assert_eq!(boards.len(), 1);
    let Fields::Live(LiveFields::TeamSurrenderBoard(solo_board)) = boards[0].fields().unwrap()
    else {
        panic!("expected 5-slot board for 5v5 solo surrender");
    };
    assert_eq!(solo_board.count, 5);
    assert_eq!(solo_board.phase, 2);
    assert_eq!(&solo_board.votes[..5], &[1; 5]);

    // 2) 3v3 PvP Casual (`ch == 1`, 3 per side) uses 3-slot AralSurrenderBoard and passes on 3rd YES.
    let pvp3_id = Uuid::new_v4();
    let pvp3_players = six_players();
    rooms
        .reserve(Reservation {
            talent_levels: Vec::new(),
            match_id: pvp3_id,
            players: pvp3_players.clone(),
            teams: vec![0, 1, 0, 1, 0, 1],
            mode: "3v3_pvp_casual".into(),
            difficulty: None,
        })
        .unwrap();
    let pvp3_room = rooms.room(pvp3_id).unwrap();
    for (seat, &p) in pvp3_players.iter().enumerate() {
        let mut room = pvp3_room.lock().unwrap();
        let (tx, _) = mpsc::channel(16);
        room.attach(p, tx, watch::channel(false).0).unwrap();
        room.receive(seat, 0, Message::known(crate::Opcode::DraftLockHero, []))
            .unwrap();
    }
    let mut rxs3 = Vec::new();
    for &p in &pvp3_players {
        let mut room = pvp3_room.lock().unwrap();
        let (tx, rx) = mpsc::channel(16);
        let (_, ch, _, _) = room.attach(p, tx, watch::channel(false).0).unwrap();
        assert_eq!(ch, 1);
        rxs3.push(rx);
    }
    while rxs3[0].try_recv().is_ok() {}
    for seat in [0usize, 2, 4] {
        pvp3_room
            .lock()
            .unwrap()
            .receive(seat, 1, Message::aral_surrender_request())
            .unwrap();
    }
    let msg3 = std::iter::from_fn(|| rxs3[0].try_recv().ok())
        .flat_map(transport::Delivery::into_records)
        .filter(|m| m.opcode == wire_opcode::TeamSurrenderStateChanged)
        .last()
        .unwrap();
    assert!(matches!(
        msg3.fields().unwrap(),
        Fields::Live(LiveFields::AralSurrenderBoard(b)) if b.wire_team == 1 && b.phase == 2 && b.votes == [1, 1, 1]
    ));

    // 3) 3v3 Solo Bots (`solo_bots`, 1 human + 5 bots) passes immediately on single human YES with 3-slot board.
    let solo3_id = Uuid::new_v4();
    let solo3_player = Uuid::new_v4();
    rooms
        .reserve(Reservation {
            talent_levels: Vec::new(),
            match_id: solo3_id,
            players: vec![solo3_player],
            teams: Vec::new(),
            mode: "solo_bots".into(),
            difficulty: Some(halcyon_data::BotDifficulty::Hard),
        })
        .unwrap();
    let solo3_room = rooms.room(solo3_id).unwrap();
    let (tx0, _) = mpsc::channel(16);
    let (tx1, mut rx1) = mpsc::channel(16);
    {
        let mut room = solo3_room.lock().unwrap();
        room.attach(solo3_player, tx0, watch::channel(false).0)
            .unwrap();
        room.receive(0, 0, Message::known(crate::Opcode::DraftLockHero, []))
            .unwrap();
        room.attach(solo3_player, tx1, watch::channel(false).0)
            .unwrap();
        while rx1.try_recv().is_ok() {}
        room.receive(0, 1, Message::aral_surrender_request())
            .unwrap();
        assert_eq!(room.phase, crate::RoomPhase::Ending);
        assert_eq!(room.world.winner, Some(1));
    }
    let msg_solo3 = std::iter::from_fn(|| rx1.try_recv().ok())
        .flat_map(transport::Delivery::into_records)
        .filter(|m| m.opcode == wire_opcode::TeamSurrenderStateChanged)
        .last()
        .unwrap();
    assert!(matches!(
        msg_solo3.fields().unwrap(),
        Fields::Live(LiveFields::AralSurrenderBoard(b)) if b.wire_team == 1 && b.phase == 2 && b.votes == [1, 1, 1]
    ));
}

#[tokio::test]
async fn missing_solo_practice_and_blitz_aliases_reserve_backfill_bots_and_enter_running() {
    let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 8, None);
    for (mode, total_heroes, asset, is_blitz_mode) in [
        ("blitz_bots_solo", 6usize, "*GameMode_Blitz_SoloBots*", true),
        ("solo_bots", 6, "*GameMode_HF_SoloBots*", false),
        ("standard_bots_solo", 6, "*GameMode_HF_SoloBots*", false),
        (
            "battle_royale_bots_solo",
            6,
            "*GameMode_Aral_SoloBots*",
            false,
        ),
        ("5v5_practice", 10, "*GameMode_5v5_Practice*", false),
    ] {
        let id = Uuid::new_v4();
        let player = Uuid::new_v4();
        let grants = if is_blitz_mode {
            let mut map = TalentLevels::default();
            map.insert(EntityTemplate::Ringo.as_raw(), NonZeroU8::new(10).unwrap());
            vec![map]
        } else {
            Vec::new()
        };
        rooms
            .reserve(Reservation {
                talent_levels: grants,
                match_id: id,
                players: vec![player],
                teams: Vec::new(),
                mode: mode.into(),
                difficulty: (mode != "5v5_practice").then_some(halcyon_data::BotDifficulty::Medium),
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut room = shared.lock().unwrap();
        assert_eq!(room.picks.len(), total_heroes);
        assert_eq!(
            room.world
                .entities
                .iter()
                .filter(|e| e.kind == Kind::Hero)
                .count(),
            total_heroes
        );
        assert_eq!(
            room.world
                .entities
                .iter()
                .filter(|e| e.kind == Kind::Hero && e.bot)
                .count(),
            total_heroes - 1
        );
        assert!(room.solo_scene());
        let (tx0, _rx0) = mpsc::channel(32);
        let (_, ch0, _, _) = room.attach(player, tx0, watch::channel(false).0).unwrap();
        assert_eq!(ch0, 0);
        if mode == "5v5_practice" {
            let lock = halcyon_protocol::draft::DraftAction::LockHero { confirm: 1 }.encode();
            room.receive(0, 0, lock).unwrap();
        } else {
            room.receive(0, 0, Message::known(crate::Opcode::DraftLockHero, []))
                .unwrap();
        }
        assert_eq!(room.phase, crate::RoomPhase::Loading);

        // Solo-scene opening builds native map frames for Fold/Blitz/ARAL before ch=0 RequestSceneSnapshot.
        if total_heroes == 6 {
            let opening = solo_opening(&mut room, 0)
                .unwrap()
                .expect("expected solo_opening batch for 6-hero bot queue");
            assert!(
                opening
                    .iter()
                    .any(|m| m.opcode == wire_opcode::SceneSettings)
            );
        }

        // Dual-channel attach works seamlessly as well.
        let (tx1, _rx1) = mpsc::channel(32);
        let (_, ch1, _, _) = room.attach(player, tx1, watch::channel(false).0).unwrap();
        assert_eq!(ch1, 1);
        assert_eq!(room.phase, crate::RoomPhase::Running);
        if is_blitz_mode {
            let ack = room
                .receive(0, 1, TalentSelection::Slot0.request())
                .unwrap();
            assert_eq!(ack.len(), 2);
            assert_eq!(room.world.entities[0].gold, 6000);
        }
        assert_eq!(KnownQueue::parse(mode).unwrap().asset(), asset);
    }

    for pvp_blitz in ["blitz_pvp_revamp", "blitz_pvp_pure"] {
        let id = Uuid::new_v4();
        let players = six_players();
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: id,
                players: players.clone(),
                teams: vec![0, 1, 0, 1, 0, 1],
                mode: pvp_blitz.into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        for (seat, &p) in players.iter().enumerate() {
            let mut room = shared.lock().unwrap();
            let (tx, _) = mpsc::channel(16);
            room.attach(p, tx, watch::channel(false).0).unwrap();
            room.receive(seat, 0, Message::known(crate::Opcode::DraftLockHero, []))
                .unwrap();
        }
        for &p in &players {
            let mut room = shared.lock().unwrap();
            let (tx, _) = mpsc::channel(16);
            let (_, ch, _, _) = room.attach(p, tx, watch::channel(false).0).unwrap();
            assert_eq!(ch, 1);
        }
        assert_eq!(shared.lock().unwrap().phase, crate::RoomPhase::Running);
    }
}

fn assert_no_non_hero_03f3_and_valid_cff0_skins(batch: &[Message]) {
    for msg in batch {
        if msg.opcode == wire_opcode::HeroSpawn {
            let raw = u16::from_be_bytes(msg.body[2..4].try_into().unwrap());
            assert!(
                raw != 329 && non_hero_skin(EntityTemplate::from_raw(raw).unwrap()).is_none(),
                "non-hero template {raw} leaked onto 0x03F3 HeroSpawn"
            );
            assert_ne!(
                msg.body[745], 0,
                "HeroSpawn for template {raw} missing slot index"
            );
        } else if msg.opcode == wire_opcode::ModifyGameModeVar {
            assert_eq!(msg.body.len(), 14);
            let key = u32::from_be_bytes(msg.body[..4].try_into().unwrap());
            assert!(
                super::super::scene::mode_variables::CLIENT_REGISTERED_KEYS.contains(&key),
                "unregistered 0x041F GameModeVar key {key:#010x}"
            );
        } else if msg.opcode == wire_opcode::EntitySpawn {
            assert_eq!(msg.body.len(), 122);
            let raw = u16::try_from(u32::from_be_bytes(msg.body[..4].try_into().unwrap()))
                .expect("valid template raw");
            assert_ne!(raw, 329, "5v5_BaseShop leaked into 3v3/Blitz/ARAL scene");
            let skin = u32::from_be_bytes(msg.body[4..8].try_into().unwrap());
            let template = EntityTemplate::from_raw(raw).expect("known non-hero template");
            let expected = non_hero_skin(template)
                .unwrap_or_else(|| panic!("missing CFF0 skin mapping for {template:?}"));
            assert_eq!(
                skin, expected,
                "wrong CFF0 skin for {template:?} ({raw}): got {skin:#010x}, expected {expected:#010x}"
            );
        }
    }
}

#[tokio::test]
async fn blitz_draft_0459_and_refresh_include_300s_timer_and_all_six_seats() {
    let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
    let id = Uuid::new_v4();
    let player = Uuid::new_v4();
    rooms
        .reserve(Reservation {
            talent_levels: Vec::new(),
            match_id: id,
            players: vec![player],
            teams: Vec::new(),
            mode: "blitz_bots_solo".into(),
            difficulty: Some(halcyon_data::BotDifficulty::Medium),
        })
        .unwrap();
    let shared = rooms.room(id).unwrap();
    let mut room = shared.lock().unwrap();
    let (tx, _rx) = mpsc::channel(32);
    let (_, _, _, initial) = room.attach(player, tx, watch::channel(false).0).unwrap();
    let roster = initial.last().unwrap();
    assert_eq!(roster.opcode, wire_opcode::DraftRosterFull);
    let rem = f32::from_be_bytes(roster.body[..4].try_into().unwrap());
    let total = f32::from_be_bytes(roster.body[4..8].try_into().unwrap());
    assert!(rem > 290.0 && (total - 300.0).abs() < 0.01);
    for seat in 0..6 {
        let row = &roster.body[seat * 161..][..161];
        assert_eq!(row[8], 1, "seat {seat} missing in Blitz 0x0459 roster");
        if seat > 0 {
            assert_eq!(row[10], 1, "bot seat {seat} unselected");
            assert_eq!(row[11], 1, "bot seat {seat} unlocked");
        }
    }
    let refresh = room
        .receive(
            0,
            0,
            Message::known(crate::Opcode::DraftRefreshRequest, [0; 6]),
        )
        .unwrap();
    let refreshed_roster = refresh
        .iter()
        .find(|m| m.opcode == wire_opcode::DraftRosterFull)
        .expect("DraftRefreshRequest must emit 0x0459 in Blitz");
    assert!(f32::from_be_bytes(refreshed_roster.body[..4].try_into().unwrap()) > 290.0);
    room.receive(0, 0, Message::known(crate::Opcode::DraftLockHero, []))
        .unwrap();
    assert_eq!(room.phase, crate::RoomPhase::Loading);
}

#[tokio::test]
async fn round3_cff0_skins_zero_non_hero_03f3_and_shopkeeper_buffs_across_ticks() {
    let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 8, None);
    for (queue, expected_jungle_shop, expected_store_operand) in [
        ("solo_bots", 315u16, Some(SoloStructure::STORE_3V3_OPERAND)),
        (
            "blitz_bots_solo",
            316u16,
            Some(SoloStructure::STORE_BLITZ_OPERAND),
        ),
        ("battle_royale_bots_solo", 0u16, None),
    ] {
        let id = Uuid::new_v4();
        let player = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: id,
                players: vec![player],
                teams: Vec::new(),
                mode: queue.into(),
                difficulty: Some(halcyon_data::BotDifficulty::Medium),
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut room = shared.lock().unwrap();
        let (tx, mut rx) = mpsc::channel(256);
        room.attach(player, tx, watch::channel(false).0).unwrap();
        room.receive(0, 0, Message::known(crate::Opcode::DraftLockHero, []))
            .unwrap();
        room.seats[0].loading_roster_sent = true;
        room.receive(
            0,
            0,
            Message::known(crate::Opcode::RequestSceneSnapshot, [0; 6]),
        )
        .unwrap();
        let wire_hero = u32::from(messages::wire_id(room.world.entities[0].id));
        let mut confirm = [0u8; 6];
        confirm[..4].copy_from_slice(&wire_hero.to_be_bytes());
        confirm[4..].copy_from_slice(&[1, 0]);
        let opening = room
            .receive(
                0,
                0,
                Message::known(crate::Opcode::ConfirmSceneOwner, confirm),
            )
            .unwrap();
        assert_no_non_hero_03f3_and_valid_cff0_skins(&opening);
        if expected_jungle_shop != 0 {
            let shop = opening
                .iter()
                .find(|m| {
                    m.opcode == wire_opcode::EntitySpawn
                        && u32::from_be_bytes(m.body[8..12].try_into().unwrap()) == 2016
                })
                .expect("Jungle Shop 2016 declared at opening");
            assert_eq!(
                u32::from_be_bytes(shop.body[..4].try_into().unwrap()),
                u32::from(expected_jungle_shop)
            );
            assert_eq!(
                u32::from_be_bytes(shop.body[4..8].try_into().unwrap()),
                EntitySpawnBody::JUNGLE_MINION_SKIN
            );
            let mut buff_pairs = Vec::new();
            for msg in &opening {
                if let Ok(Fields::BuffApplication(b)) = msg.fields() {
                    buff_pairs.push((b.prefix.target, b.prefix.definition));
                }
            }
            for keeper in [2000, 2008] {
                for def in [176, 27, 66] {
                    assert!(buff_pairs.contains(&(keeper, def)));
                }
            }
            for def in [175, 27, 66] {
                assert!(buff_pairs.contains(&(2016, def)));
            }
        }
        let expected_slot = room.hero_slot(&room.world.entities[0]).unwrap();
        assert!(room.scene_views[0].registered(room.world.entities[0].id, expected_slot));
        room.receive(
            0,
            0,
            Message::known(crate::Opcode::SceneReadyConfirm, [0; 6]),
        )
        .unwrap();
        room.seats[0].world_actor_written = true;
        assert_eq!(room.phase, crate::RoomPhase::Running);
        while rx.try_recv().is_ok() {}
        let mut live = Vec::new();
        let mut drain = |rx: &mut mpsc::Receiver<transport::Delivery>| {
            while let Ok(d) = rx.try_recv() {
                live.extend(d.into_records());
            }
        };
        // Step forward across wave spawns, camp spawns, Kraken transition (in 3v3), and deaths.
        for _ in 0..800 {
            room.tick();
            drain(&mut rx);
        }
        if queue == "solo_bots" {
            room.world.tick = 15 * 60 * TICK_HZ - 1;
            room.tick();
            drain(&mut rx);
            // Verify captured Kraken declaration also uses 0x03F2 + JUNGLE_MINION_SKIN.
            let k_id = room
                .world
                .entities
                .iter()
                .find(|e| e.type_id == EntityTemplate::HfKrakenJungle.as_raw())
                .map(|e| e.id)
                .expect("15:00 Kraken spawned");
            let ki = room
                .world
                .entities
                .iter()
                .position(|e| e.id == k_id)
                .unwrap();
            room.world.entities[ki].hp = 1.0;
            let h_pos = room.world.entities[ki].position;
            room.world.entities[0].position = h_pos;
            room.world.command(1, Command::Attack(k_id)).unwrap();
            for _ in 0..10 {
                room.tick();
                drain(&mut rx);
            }
            assert!(
                live.iter().any(|m| {
                    m.opcode == wire_opcode::EntitySpawn
                        && u32::from_be_bytes(m.body[..4].try_into().unwrap())
                            == u32::from(EntityTemplate::HfKrakenCaptured.as_raw())
                        && u32::from_be_bytes(m.body[4..8].try_into().unwrap())
                            == EntitySpawnBody::JUNGLE_MINION_SKIN
                }),
                "expected captured Kraken 364 on 0x03F2 with JUNGLE_MINION_SKIN"
            );
        }
        assert_no_non_hero_03f3_and_valid_cff0_skins(&live);
        if expected_store_operand.is_some() {
            assert!(
                live.iter().any(|m| m.opcode == wire_opcode::EntitySpawn),
                "expected live 0x03F2 EntitySpawn declarations for waves/camps in {queue}"
            );
        }
    }
}

#[tokio::test]
async fn round3_shop_3v3_and_blitz_emit_173_174_with_mode_store_and_cancel_on_leave() {
    let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 4, None);
    for (mode, operand, jungle_ready_tick) in [
        (
            "solo_bots",
            SoloStructure::STORE_3V3_OPERAND,
            halcyon_sim::map_3v3::JUNGLE_SHOP_UNLOCK_TICK,
        ),
        ("blitz_bots_solo", SoloStructure::STORE_BLITZ_OPERAND, 0),
    ] {
        let id = Uuid::new_v4();
        let player = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: id,
                players: vec![player],
                teams: Vec::new(),
                mode: mode.into(),
                difficulty: Some(halcyon_data::BotDifficulty::Easy),
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut room = shared.lock().unwrap();
        let (tx, mut rx) = mpsc::channel(128);
        room.attach(player, tx, watch::channel(false).0).unwrap();
        room.receive(0, 0, Message::known(crate::Opcode::DraftLockHero, []))
            .unwrap();
        room.seats[0].loading_roster_sent = true;
        room.receive(
            0,
            0,
            Message::known(crate::Opcode::RequestSceneSnapshot, [0; 6]),
        )
        .unwrap();
        let wire_hero = u32::from(messages::wire_id(room.world.entities[0].id));
        let mut confirm = [0u8; 6];
        confirm[..4].copy_from_slice(&wire_hero.to_be_bytes());
        confirm[4..].copy_from_slice(&[1, 0]);
        room.receive(
            0,
            0,
            Message::known(crate::Opcode::ConfirmSceneOwner, confirm),
        )
        .unwrap();
        room.receive(
            0,
            0,
            Message::known(crate::Opcode::SceneReadyConfirm, [0; 6]),
        )
        .unwrap();
        room.seats[0].world_actor_written = true;
        while rx.try_recv().is_ok() {}

        // 1) At base shop at opening -> Buff 173 + Buff 174 (source = 2000, operand = 452/453).
        room.tick();
        let at_base: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok())
            .flat_map(transport::Delivery::into_records)
            .collect();
        let mut got_173 = false;
        let mut base_174_instance = None;
        for msg in &at_base {
            if let Ok(Fields::BuffApplication(b)) = msg.fields() {
                if b.prefix.definition == 173 && b.prefix.target == wire_hero {
                    got_173 = true;
                }
                if b.prefix.definition == 174
                    && b.prefix.target == wire_hero
                    && b.prefix.source == 2000
                    && b.extras.as_ref().is_some_and(|e| e.words[2] == operand)
                {
                    base_174_instance = Some(b.prefix.instance);
                }
            }
        }
        assert!(got_173, "missing Buff 173 for {mode}");
        let base_instance = base_174_instance.expect("missing Base Buff 174");

        // 2) Move outside shop range -> BuffMutation Cancel (0x0445) on active instance.
        room.world.entities[0].position = halcyon_sim::Position { x: 0.0, y: 0.0 };
        room.tick();
        let left_shop: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok())
            .flat_map(transport::Delivery::into_records)
            .collect();
        assert!(
            left_shop.iter().any(|m| {
                matches!(
                    m.buff_mutation(),
                    Some(bm)
                        if bm.target == wire_hero
                            && bm.instance == base_instance
                            && bm.change == BuffChange::Cancel
                )
            }),
            "missing 0x0445 CancelBuff when leaving shop in {mode}"
        );

        // 3) Move to Jungle Shop (0.2, 42.0): 3v3 denies before 4:00, both grant after unlock.
        room.world.entities[0].position = halcyon_sim::map_3v3::JUNGLE_SHOP_POS;
        if jungle_ready_tick > 0 {
            room.tick();
            let before_unlock: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok())
                .flat_map(transport::Delivery::into_records)
                .collect();
            assert!(!before_unlock.iter().any(|m| {
                matches!(
                    m.fields(),
                    Ok(Fields::BuffApplication(b))
                        if b.prefix.definition == 174
                )
            }));
            room.world.tick = jungle_ready_tick;
        }
        room.tick();
        let at_jungle: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok())
            .flat_map(transport::Delivery::into_records)
            .collect();
        assert!(
            at_jungle.iter().any(|m| {
                matches!(
                    m.fields(),
                    Ok(Fields::BuffApplication(b))
                        if b.prefix.definition == 174
                            && b.prefix.target == wire_hero
                            && b.prefix.source == 2016
                            && b.extras.as_ref().is_some_and(|e| e.words[2] == operand)
                )
            }),
            "missing Jungle Shop Buff 174 (source 2016, operand {operand}) in {mode}"
        );
    }
}

#[tokio::test]
async fn round3_pvp_3v3_and_blitz_ch1_zero_non_hero_03f3_and_040b_destroy() {
    let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 4, None);
    for mode in ["3v3_pvp_casual", "blitz_3v3"] {
        let id = Uuid::new_v4();
        let players = six_players();
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: id,
                players: players.clone(),
                teams: vec![0, 1, 0, 1, 0, 1],
                mode: mode.into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        for (seat, &p) in players.iter().enumerate() {
            let mut room = shared.lock().unwrap();
            let (tx, _) = mpsc::channel(16);
            room.attach(p, tx, watch::channel(false).0).unwrap();
            room.receive(seat, 0, Message::known(crate::Opcode::DraftLockHero, []))
                .unwrap();
        }
        let mut rxs = Vec::new();
        let mut openings = Vec::new();
        for &p in &players {
            let mut room = shared.lock().unwrap();
            let (tx, rx) = mpsc::channel(512);
            let (_, ch, _, open) = room.attach(p, tx, watch::channel(false).0).unwrap();
            assert_eq!(ch, 1);
            assert_no_non_hero_03f3_and_valid_cff0_skins(&open);
            openings.push(open);
            rxs.push(rx);
        }
        assert!(openings[0].iter().any(|m| {
            m.opcode == wire_opcode::EntitySpawn
                && u32::from_be_bytes(m.body[8..12].try_into().unwrap()) == 2016
        }));
        let mut live = Vec::new();
        for _ in 0..1600 {
            let mut room = shared.lock().unwrap();
            room.tick();
            while let Ok(d) = rxs[0].try_recv() {
                live.extend(d.into_records());
            }
        }
        assert_no_non_hero_03f3_and_valid_cff0_skins(&live);
        assert!(
            live.iter().any(|m| m.opcode == wire_opcode::EntitySpawn),
            "expected PvP ch=1 live 0x03F2 spawns in {mode}"
        );
        assert!(
            live.iter().any(|m| m.opcode == wire_opcode::DestroyEntity),
            "expected PvP ch=1 0x040B DestroyEntity on wave/sentry deaths in {mode}"
        );
    }
}

#[tokio::test]
async fn round4_all_041f_mode_vars_are_client_registered_across_blitz_3v3_and_ticks() {
    let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 4, None);
    let id = Uuid::new_v4();
    let player = Uuid::new_v4();
    rooms
        .reserve(Reservation {
            talent_levels: Vec::new(),
            match_id: id,
            players: vec![player],
            teams: Vec::new(),
            mode: "blitz_bots_solo".into(),
            difficulty: Some(halcyon_data::BotDifficulty::Medium),
        })
        .unwrap();
    let shared = rooms.room(id).unwrap();
    let mut room = shared.lock().unwrap();
    let (tx, mut rx) = mpsc::channel(256);
    room.attach(player, tx, watch::channel(false).0).unwrap();
    room.receive(0, 0, Message::known(crate::Opcode::DraftLockHero, []))
        .unwrap();
    room.seats[0].loading_roster_sent = true;
    room.receive(
        0,
        0,
        Message::known(crate::Opcode::RequestSceneSnapshot, [0; 6]),
    )
    .unwrap();
    let wire_hero = u32::from(messages::wire_id(room.world.entities[0].id));
    let mut confirm = [0u8; 6];
    confirm[..4].copy_from_slice(&wire_hero.to_be_bytes());
    confirm[4..].copy_from_slice(&[1, 0]);
    let opening = room
        .receive(
            0,
            0,
            Message::known(crate::Opcode::ConfirmSceneOwner, confirm),
        )
        .unwrap();
    assert_no_non_hero_03f3_and_valid_cff0_skins(&opening);
    let open_vars: std::collections::HashMap<u32, u32> = opening
        .iter()
        .filter(|m| m.opcode == wire_opcode::ModifyGameModeVar)
        .map(|m| {
            (
                u32::from_be_bytes(m.body[..4].try_into().unwrap()),
                u32::from_be_bytes(m.body[4..8].try_into().unwrap()),
            )
        })
        .collect();
    assert_eq!(open_vars.get(&COUNTDOWN_START_KEY), Some(&0));
    assert_eq!(open_vars.get(&COUNTDOWN_DURATION_KEY), Some(&300));
    assert_eq!(open_vars.get(&COUNTDOWN_PAUSED_KEY), Some(&0));
    assert_eq!(open_vars.get(&TEAM_1_SCORE_KEY), Some(&0));
    assert_eq!(open_vars.get(&TEAM_2_SCORE_KEY), Some(&0));

    room.receive(
        0,
        0,
        Message::known(crate::Opcode::SceneReadyConfirm, [0; 6]),
    )
    .unwrap();
    room.seats[0].world_actor_written = true;
    while rx.try_recv().is_ok() {}

    // Trigger a hero kill point (+1 on Team1Score = 0x7c9ddf52) and Sudden Death (PAUSED = 1).
    room.world.entities[1].hp = 1.0;
    room.world.entities[1].position = room.world.entities[0].position;
    let target_id = room.world.entities[1].id;
    room.world.command(1, Command::Attack(target_id)).unwrap();
    let mut live = Vec::new();
    for _ in 0..20 {
        room.tick();
        while let Ok(d) = rx.try_recv() {
            live.extend(d.into_records());
        }
    }
    publish_tick(&mut room, &[Event::BlitzSuddenDeath]);
    while let Ok(d) = rx.try_recv() {
        live.extend(d.into_records());
    }
    assert_no_non_hero_03f3_and_valid_cff0_skins(&live);
    assert!(live.iter().any(|m| {
        m.opcode == wire_opcode::ModifyGameModeVar
            && u32::from_be_bytes(m.body[..4].try_into().unwrap()) == TEAM_1_SCORE_KEY
            && u32::from_be_bytes(m.body[4..8].try_into().unwrap()) >= 1
    }));
    assert!(live.iter().any(|m| {
        m.opcode == wire_opcode::ModifyGameModeVar
            && u32::from_be_bytes(m.body[..4].try_into().unwrap()) == COUNTDOWN_PAUSED_KEY
            && u32::from_be_bytes(m.body[4..8].try_into().unwrap()) == 1
    }));
}

#[tokio::test]
async fn unlock_all_talents_authorizes_baron_talents_at_ce_levels_in_blitz() {
    let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
    rooms.set_unlock_all_talents(true);
    let id = Uuid::new_v4();
    let players = six_players();
    rooms
        .reserve(Reservation {
            talent_levels: Vec::new(),
            match_id: id,
            players: players.clone(),
            teams: vec![0, 1, 0, 1, 0, 1],
            mode: "blitz_pvp_revamp".into(),
            difficulty: None,
        })
        .unwrap();
    let shared = rooms.room(id).unwrap();
    for (seat, &p) in players.iter().enumerate() {
        let mut room = shared.lock().unwrap();
        let (tx, _) = mpsc::channel(16);
        room.attach(p, tx, watch::channel(false).0).unwrap();
        room.receive(seat, 0, Message::known(crate::Opcode::DraftLockHero, []))
            .unwrap();
    }
    for &p in &players {
        let mut room = shared.lock().unwrap();
        let (tx, _) = mpsc::channel(16);
        room.attach(p, tx, watch::channel(false).0).unwrap();
    }
    let mut room = shared.lock().unwrap();
    room.world.entities[0].type_id = EntityTemplate::Baron.as_raw();
    for (sel, expected_level) in [
        (TalentSelection::Slot0, 10u8),
        (TalentSelection::Slot1, 10u8),
        (TalentSelection::Slot2, 5u8),
        (TalentSelection::NoTalent, 0u8),
    ] {
        let ack = room.receive(0, 1, sel.request()).unwrap();
        assert_eq!(ack.len(), 2);
        assert!(matches!(
            ack[1].fields().unwrap(),
            Fields::Live(LiveFields::TalentEquipped(eq))
                if eq.actor == 1500 && eq.selection == sel && eq.level == expected_level
        ));
    }
}

#[tokio::test]
async fn blitz_visibility_transitions_emit_042b_for_enemies_minions_and_neutrals() {
    let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
    let id = Uuid::new_v4();
    let player = Uuid::new_v4();
    rooms
        .reserve(Reservation {
            talent_levels: Vec::new(),
            match_id: id,
            players: vec![player],
            teams: Vec::new(),
            mode: "blitz_bots_solo".into(),
            difficulty: Some(halcyon_data::BotDifficulty::Medium),
        })
        .unwrap();
    let shared = rooms.room(id).unwrap();
    let mut room = shared.lock().unwrap();
    let (tx, mut rx) = mpsc::channel(256);
    room.attach(player, tx, watch::channel(false).0).unwrap();
    room.receive(0, 0, Message::known(crate::Opcode::DraftLockHero, []))
        .unwrap();
    room.seats[0].loading_roster_sent = true;
    room.receive(
        0,
        0,
        Message::known(crate::Opcode::RequestSceneSnapshot, [0; 6]),
    )
    .unwrap();
    let wire_hero = u32::from(messages::wire_id(room.world.entities[0].id));
    let mut confirm = [0u8; 6];
    confirm[..4].copy_from_slice(&wire_hero.to_be_bytes());
    confirm[4..].copy_from_slice(&[1, 0]);
    room.receive(
        0,
        0,
        Message::known(crate::Opcode::ConfirmSceneOwner, confirm),
    )
    .unwrap();
    room.receive(
        0,
        0,
        Message::known(crate::Opcode::SceneReadyConfirm, [0; 6]),
    )
    .unwrap();
    room.seats[0].world_actor_written = true;
    while rx.try_recv().is_ok() {}

    let middle_sentry = room
        .world
        .entities
        .iter()
        .find(|e| e.type_id == EntityTemplate::BlitzMiddleSentry.as_raw())
        .expect("BlitzMiddleSentry present")
        .clone();
    let ally_sentry_id = room
        .world
        .entities
        .iter()
        .find(|e| e.type_id == EntityTemplate::BlitzAllySentry.as_raw() && e.team == 0)
        .expect("team 0 AllySentry present")
        .id;
    let enemy_hero_id = room.world.entities[1].id;
    assert_eq!(room.world.entities[1].team, 1);

    // Step 2: Enemy hero 2 and MiddleSentry start in fog; own AllySentry is visible.
    assert!(!room.visible[0].contains(&enemy_hero_id));
    assert!(!room.visible[0].contains(&middle_sentry.id));
    assert!(room.visible[0].contains(&ally_sentry_id));

    // Step 3: Move stationary enemy hero 2 and human hero 0 next to MiddleSentry, replicate(&[]).
    let home_pos = room.world.entities[0].position;
    let sentry_wire = u32::from(messages::wire_id(middle_sentry.id));
    let enemy_hero_wire = u32::from(messages::wire_id(enemy_hero_id));
    room.world.entities[0].position = middle_sentry.position;
    room.world.entities[1].position = middle_sentry.position;
    room.replicate(&[]);

    let enter = rx.try_recv().unwrap().into_records();
    let has_vis = |batch: &[Message], actor_wire: u32, mask: u8| {
        batch.iter().any(|m| {
            m.is(crate::Opcode::ModifyVisibility)
                && m.body[..8]
                    == [
                        actor_wire.to_be_bytes()[0],
                        actor_wire.to_be_bytes()[1],
                        actor_wire.to_be_bytes()[2],
                        actor_wire.to_be_bytes()[3],
                        1,
                        1,
                        mask,
                        0,
                    ]
        })
    };
    assert!(has_vis(&enter, enemy_hero_wire, 15));
    assert!(has_vis(&enter, sentry_wire, 15));
    assert!(enter.iter().any(|m| {
        m.is(crate::Opcode::MoveToCorrection) && m.body[..4] == enemy_hero_wire.to_be_bytes()
    }));
    assert!(enter.iter().all(|m| {
        !(m.is(crate::Opcode::HeroSpawn)
            && m.body[10..12] == (enemy_hero_wire as u16).to_be_bytes())
            && !(m.is(crate::Opcode::EntitySpawn) && m.body[8..12] == sentry_wire.to_be_bytes())
    }));
    for m in &enter {
        if m.is(crate::Opcode::ModifyVisibility) {
            assert!(matches!(
                m.body.as_slice(),
                [0, 0, _, _, 0..=7, 1, 0 | 15, 0, 0, 0, 0, 0, 0, 0]
            ));
        }
    }

    // Step 4: Return hero 0 to home fountain (enemy hero & MiddleSentry leave vision).
    room.world.entities[0].position = home_pos;
    room.replicate(&[]);
    let leave = rx.try_recv().unwrap().into_records();
    assert!(has_vis(&leave, enemy_hero_wire, 0));
    assert!(has_vis(&leave, sentry_wire, 0));
    assert!(leave.iter().all(|m| {
        !m.is(crate::Opcode::ModifyVisibility)
            || room
                .world
                .entities
                .iter()
                .find(|e| u32::from(messages::wire_id(e.id)).to_be_bytes() == m.body[..4])
                .is_some_and(|e| !matches!(e.kind, Kind::Tower | Kind::Crystal))
    }));

    // Step 5: Step to Blitz wave start, bring a fresh enemy minion into vision, verify 0x03F2 precedes 0x042B.
    while !room
        .world
        .entities
        .iter()
        .any(|e| e.kind == Kind::Minion && e.team == 1)
    {
        room.world.step();
    }
    room.world.entities[0].position = home_pos;
    room.replicate(&[]);
    while rx.try_recv().is_ok() {}

    let enemy_minion_id = room
        .world
        .entities
        .iter()
        .find(|e| e.kind == Kind::Minion && e.team == 1 && !room.visible[0].contains(&e.id))
        .expect("unseen enemy minion present")
        .id;
    let minion_wire = u32::from(messages::wire_id(enemy_minion_id));
    room.world
        .entities
        .find_mut(|e| e.id == enemy_minion_id)
        .unwrap()
        .position = home_pos;
    room.replicate(&[]);
    let minion_enter = rx.try_recv().unwrap().into_records();
    let spawn_pos = minion_enter
        .iter()
        .position(|m| {
            m.is(crate::Opcode::EntitySpawn) && m.body[8..12] == minion_wire.to_be_bytes()
        })
        .expect("enemy minion 0x03F2 EntitySpawn emitted");
    let vis_pos = minion_enter
        .iter()
        .position(|m| {
            m.is(crate::Opcode::ModifyVisibility)
                && m.body[..8]
                    == [
                        minion_wire.to_be_bytes()[0],
                        minion_wire.to_be_bytes()[1],
                        minion_wire.to_be_bytes()[2],
                        minion_wire.to_be_bytes()[3],
                        1,
                        1,
                        15,
                        0,
                    ]
        })
        .expect("enemy minion 0x042B ModifyVisibility emitted");
    assert!(spawn_pos < vis_pos, "0x03F2 must precede 0x042B");

    // Step 6: Mid-match solo_opening reconnect snapshot reveals visible enemy & neutral actors.
    room.world.tick = 40;
    room.world.entities[0].position = middle_sentry.position;
    room.world.entities[1].position = middle_sentry.position;
    let reconnect = solo_opening(&mut room, 0).unwrap().unwrap();
    assert!(has_vis(&reconnect, enemy_hero_wire, 15));
    assert!(has_vis(&reconnect, sentry_wire, 15));
}
