pub(super) mod announcements;
use super::{Batch, Message, Room, messages};

pub(super) use halcyon_data::LEGACY_ARAL_SEATS as SEATS;
pub(super) const DRAFT_SECONDS: f32 = 20.;
const MODE: &str = halcyon_protocol::ce::NativeMode::Aral.asset();

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
        || room.phase != crate::RoomPhase::Running
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
    // One independently identified Mobile Bulwark carried slot 1 and level 10.
    // The reserved level is home-authorized. Other native selections still have only numeric evidence.
    if hero.type_id == halcyon_data::EntityTemplate::Lyra.as_raw()
        && let Some(data) = room.data.as_deref()
    {
        let actor = hero.id;
        let talent = (selection == TalentSelection::Slot1 && level == 10).then(|| {
            (
                halcyon_data::ModeledTalent::MobileBulwark.as_str(),
                std::num::NonZeroU8::new(10).unwrap(),
            )
        });
        // Older content packages can still serve the numeric equip response.
        let _ = room.world.set_named_talent(actor, talent, data);
    }
    let hero = &room.world.entities[seat];
    if hero.type_id == halcyon_data::EntityTemplate::Gwen.as_raw()
        && let Some(data) = room.data.as_deref()
    {
        let talent = (selection == TalentSelection::Slot0 && level == 1).then(|| {
            (
                halcyon_data::ModeledTalent::SpeedLoader.as_str(),
                std::num::NonZeroU8::MIN,
            )
        });
        let actor = hero.id;
        let _ = room.world.set_named_talent(actor, talent, data);
    }
    let hero = &room.world.entities[seat];
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
            pick.preferences.write(row);
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
    Message::known(crate::Opcode::DraftRosterAral, body)
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
    let mut batch = vec![Message::known(crate::Opcode::MatchSettings, settings)];
    batch.extend(draft_catalog(room, remaining));
    batch
}

pub(super) fn draft_catalog(room: &Room, remaining: f32) -> Batch {
    let mut mode = vec![0; 68];
    mode[..4].copy_from_slice(&3u32.to_be_bytes());
    mode[4..4 + MODE.len()].copy_from_slice(MODE.as_bytes());
    let mut batch = Vec::with_capacity(room.catalog.len() + 2);
    batch.push(Message::known(crate::Opcode::DraftModeAnnouncement, mode));
    batch.extend(
        room.catalog
            .iter()
            .map(|entry| Message::known(crate::Opcode::DraftCatalogEntry, *entry)),
    );
    batch.push(roster(room, remaining, false));
    batch
}

pub(super) fn reroll(room: &mut Room, seat: usize) -> Batch {
    if !room.free_aral_reroll
        || !room.legacy_aral()
        || room.phase != crate::RoomPhase::HeroSelect
        || room.aral_remaining() <= 0.
        || room.seats[seat].peers[0].is_none()
        || room.seats[seat].locked
        || room.seats[seat].used_aral_reroll
    {
        return Vec::new();
    }
    let Some(data) = &room.data else {
        return Vec::new();
    };
    let candidates = data.hero_names.len();
    if candidates < 2 {
        return Vec::new();
    }
    let template = room.world.entities[seat].type_id;
    // Room identity is the match seed. This rotates its bits by seat so
    // retries and connection changes cannot rerandomize a draft.
    let seed = room.id.as_u128().rotate_right(7 * seat as u32);
    let start = (seed % candidates as u128) as usize;
    let Some(next) = data
        .hero_names
        .iter()
        .cycle()
        .skip(start)
        .take(candidates)
        .find(|name| {
            data.type_id(name).is_some_and(|id| id != template)
                && room.world.select_hero(seat, name, data).is_ok()
        })
    else {
        return Vec::new();
    };
    let skin = data
        .indexed_skin(next, None)
        .unwrap_or_else(|| halcyon_protocol::skin_hash(&format!("{next}_DefaultSkin")));
    room.picks[seat].name.clone_from(next);
    room.picks[seat].skin = skin;
    room.seats[seat].used_aral_reroll = true;
    let echo = Message::known(crate::Opcode::DraftRerollHero, [0]);
    let roster = room.roster_with_echo(Some(seat), Some(&echo));
    vec![
        roster,
        echo,
        Message::known(crate::Opcode::DraftSelectSkin, skin.to_be_bytes()),
    ]
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
        batch.push(Message::known(crate::Opcode::AralDraftMarker, body));
        batch.push(roster(room, room.aral_remaining(), false));
        room.seats[seat].draft_sync_sent = true;
    }
    batch
}

pub(super) fn opening(room: &mut Room, team: Option<u8>) -> Batch {
    let mut batch: Batch = Vec::new();
    batch.push(messages::scene_mode(&room.queue, 0));
    batch.extend(crate::scene::players(room));
    batch.push(messages::world_clock(room.world.tick));
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
                    if room.world.tick == 0 && team.is_none() {
                        super::native_cooldown::opening(
                            &mut batch,
                            actor,
                            halcyon_protocol::ce::NativeMode::Aral,
                        );
                    }
                    messages::append_inventory(&mut batch, &room.world, actor);
                }
                halcyon_sim::Kind::Minion => lane_minions.push(declaration),
                _ => map_objects.push(declaration),
            }
        }
    }
    map_objects.sort_unstable_by_key(|m| std::cmp::Reverse(m.body[116]));
    let initial = room.world.tick == 0 && team.is_none();
    let slots = map_objects
        .iter()
        .filter_map(|message| 1_u16.checked_shl(u32::from(message.body[116])))
        .fold(0, |map, mask| map | mask);
    batch.extend(map_objects);
    batch.extend(lane_minions);
    if initial {
        batch.extend(
            (0..16)
                .rev()
                .filter(|slot| slots & (1 << slot) != 0)
                .flat_map(|slot| {
                    crate::structure_effects::Opening::aral(slot)
                        .into_iter()
                        .flat_map(crate::structure_effects::Opening::messages)
                        .chain(crate::aral_guard::opening(slot))
                }),
        );
        batch.extend(super::scene::mode_variables::Opening::Aral.messages());
    } else {
        batch.extend(
            (0..16)
                .rev()
                .filter(|slot| slots & (1 << slot) != 0)
                .filter_map(|slot| crate::aral_guard::restore(&room.world, team, slot)),
        );
    }
    batch.extend(room.recommended_paths(team));
    batch.extend(talent_snapshot(room));
    if let Some(team) = team {
        batch.extend(room.surrender_snapshot(team));
    }
    batch
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
        let probe = Message::known(crate::Opcode::RequestSceneSnapshot, [0; 6]);
        socket
            .write_all(&cipher.encode(&probe).unwrap())
            .await
            .unwrap();
        let mut words = [0u8; 6];
        words[..4].copy_from_slice(&actor.to_be_bytes());
        words[4] = 1;
        let request = Message::known(crate::Opcode::ConfirmSceneOwner, words);
        socket
            .write_all(&cipher.encode(&request).unwrap())
            .await
            .unwrap();
        let name = next(socket, cipher).await;
        assert_eq!(name.opcode, crate::wire_opcode::SceneSettings);
        assert_eq!(name.body[68], 0);
        for _ in 0..SEATS {
            assert_eq!(
                next(socket, cipher).await.opcode,
                crate::wire_opcode::PlayerInfo
            );
        }
        let clock = next(socket, cipher).await;
        assert_eq!(clock.opcode, crate::wire_opcode::SyncWorldClock);
        if initial {
            assert_eq!(clock.body, [0; 6]);
        }
        let mut declared = false;
        let mut structures = Vec::new();
        let mut talents = Vec::new();
        let mut variables = Vec::new();
        let mut tower_effects = Vec::new();
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
            if !initial {
                assert!(
                    !frame.is(crate::Opcode::InitCooldown),
                    "rejoin reset an active cooldown"
                );
            }
            if frame.is(crate::Opcode::EquipTalent) {
                talents.push(frame.body.clone());
            }
            if frame.is(crate::Opcode::ModifyGameModeVar) {
                variables.push(frame.clone());
            }
            if frame.is(crate::Opcode::ApplyBuffExtended) {
                assert!(
                    crate::structure_effects::recordable(&frame)
                        || crate::aral_guard::recordable(&frame)
                );
                assert!(variables.is_empty(), "tower effects precede mode variables");
                let Ok(Fields::BuffApplication(effect)) = frame.fields() else {
                    panic!("missing typed ARAL tower effect");
                };
                assert!(structures.contains(&((effect.prefix.target - 2000) as u8)));
                tower_effects.push((
                    effect.prefix.target,
                    effect.prefix.definition,
                    effect.prefix.instance,
                ));
            }
            if frame.is(crate::Opcode::HeroSpawn) {
                let id = u32::from_be_bytes(frame.body[8..12].try_into().unwrap());
                assert!((1500..1506).contains(&id));
                assert_eq!(frame.body[745], 16 + (id - 1500) as u8);
                declared |= id == actor;
                if initial
                    && frame.body[..4]
                        == u32::from(halcyon_data::EntityTemplate::Ringo.as_raw()).to_be_bytes()
                {
                    // The two recorded Ringo declarations had this exact adjacent key order.
                    for expected in [
                        0xb855d752, 0x1e275dc1, 0x022982b5, 0xcad52059, 0xc9d51ec6, 0xc8d51d33,
                        0xd60c580b,
                    ] {
                        let next_frame = next(socket, cipher).await;
                        let Fields::CooldownSync { owner, key, .. } = next_frame.fields().unwrap()
                        else {
                            panic!("Ringo declaration lacked its opening cooldown block")
                        };
                        assert_eq!((owner, key), (id, expected));
                    }
                }
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
            if frame.is(crate::Opcode::EntitySpawn) {
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
            assert_eq!(tower_effects.iter().filter(|e| e.1 == 205).count(), 10);
            tower_effects.retain(|e| e.1 != 205);
            assert_eq!(tower_effects.len(), 30);
            assert_eq!(
                tower_effects
                    .iter()
                    .map(|effect| effect.2)
                    .collect::<std::collections::HashSet<_>>()
                    .len(),
                30
            );
            for (rows, slot) in tower_effects
                .as_chunks::<3>()
                .0
                .iter()
                .zip([15, 14, 13, 12, 11, 7, 6, 5, 4, 3])
            {
                assert_eq!(
                    rows.iter().map(|e| e.0).collect::<Vec<_>>(),
                    [2000 + slot; 3]
                );
                assert_eq!(
                    rows.iter().map(|e| e.1).collect::<Vec<_>>(),
                    [194, 198, 204]
                );
            }
            assert_eq!(
                variables,
                crate::scene::mode_variables::Opening::Aral
                    .messages()
                    .collect::<Vec<_>>()
            );
            assert!(talents.is_empty());
        } else {
            assert!(variables.is_empty());
            assert!(
                tower_effects
                    .iter()
                    .all(|&(_, definition, _)| definition == 205)
            );
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
        let done = Message::known(crate::Opcode::SceneReadyConfirm, [0; 6]);
        socket
            .write_all(&cipher.encode(&done).unwrap())
            .await
            .unwrap();
        until(socket, cipher, done.opcode).await;
    }

    #[tokio::test]
    #[ignore = "requires original delivery and supplemental combat data"]
    async fn lyra_native_mobile_bulwark_requires_the_seats_named_level_evidence() {
        use halcyon_protocol::TalentSelection::*;
        let data = std::sync::Arc::new(
            halcyon_data::GameData::load_with_extra(
                std::env::var_os("HALCYON_DELIVERY").unwrap(),
                Some(std::path::Path::new(
                    &std::env::var_os("HALCYON_DATA_EXTRA").unwrap(),
                )),
            )
            .unwrap(),
        );
        let rooms = Rooms::new(
            "127.0.0.1".parse().unwrap(),
            "127.0.0.1".into(),
            1,
            Some(data.clone()),
        );
        let id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: id,
                players: (0..6).map(|_| Uuid::new_v4()).collect(),
                mode: "casual_aral".into(),
                teams: vec![0, 1, 0, 1, 0, 1],
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut r = shared.lock().unwrap();
        r.world.select_hero(0, "Lyra", &data).unwrap();
        r.phase = crate::RoomPhase::Running;
        r.seats[0].world_actor_written = true;
        let (tx, _rx) = tokio::sync::mpsc::channel(2);
        r.seats[0].peers[0] = Some(crate::Peer {
            generation: 1,
            tx,
            stalled: tokio::sync::watch::channel(false).0,
        });
        for m in [
            Message::known(crate::Opcode::RequestSceneSnapshot, [0; 6]),
            Message::known(crate::Opcode::ConfirmSceneOwner, [0, 0, 5, 220, 1, 0]),
            Message::known(crate::Opcode::SceneReadyConfirm, [0; 6]),
        ] {
            r.seats[0].preamble.observe(&m, 1500);
        }
        r.talent_levels = vec![Default::default(); 6];
        let original = r.world.state_digest();
        assert!(talent_response(&mut r, 0, 0, Slot1).is_empty());
        let template = halcyon_data::EntityTemplate::Lyra.as_raw();
        r.talent_levels[1].insert(template, std::num::NonZeroU8::new(10).unwrap());
        assert!(talent_response(&mut r, 0, 0, Slot1).is_empty());
        r.talent_levels[0].insert(template, std::num::NonZeroU8::new(9).unwrap());
        assert_eq!(talent_response(&mut r, 0, 0, Slot1).len(), 2);
        assert_eq!(r.world.state_digest(), original);
        r.talent_levels[0].insert(template, std::num::NonZeroU8::new(10).unwrap());
        assert_eq!(talent_response(&mut r, 0, 0, Slot1).len(), 2);
        let equipped = r.world.state_digest();
        assert_ne!(equipped, original);
        assert_eq!(talent_response(&mut r, 0, 0, Slot1).len(), 2);
        assert_eq!(r.world.state_digest(), equipped);
        assert_eq!(talent_response(&mut r, 0, 0, NoTalent).len(), 2);
        assert_eq!(r.world.state_digest(), original);
    }

    #[test]
    fn readiness_requires_order_and_authenticated_actor_id() {
        let mut preamble = crate::scene::Preamble::default();
        preamble.observe(
            &Message::known(crate::Opcode::SceneReadyConfirm, [0; 6]),
            1505,
        );
        preamble.observe(
            &Message::known(crate::Opcode::RequestSceneSnapshot, [0; 6]),
            1505,
        );
        preamble.observe(
            &Message::known(crate::Opcode::ConfirmSceneOwner, [0, 0, 5, 224, 1, 0]),
            1505,
        );
        assert!(!preamble.scene_requested());
        preamble.observe(
            &Message::known(crate::Opcode::ConfirmSceneOwner, [0, 0, 5, 225, 1, 0]),
            1505,
        );
        assert!(preamble.scene_requested());
        assert!(!preamble.confirmed());
        preamble.observe(
            &Message::known(crate::Opcode::SceneReadyConfirm, [0; 6]),
            1505,
        );
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

    #[tokio::test]
    async fn encrypted_aral_witnesses_receive_successful_flask_and_sale_once() {
        play_six_seats(Scenario::ItemActions).await;
    }
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Scenario {
        ItemActions,
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
                .write_all(
                    &cipher
                        .encode(&Message::known(crate::Opcode::RegisterPlayerUuid, auth))
                        .unwrap(),
                )
                .await
                .unwrap();
            let setting = next(&mut socket, &cipher).await;
            assert_eq!(setting.opcode, crate::wire_opcode::MatchSettings);
            assert_eq!(setting.body[16..18], [3, 3]);
            assert_eq!(setting.body[100], (index % 2 + 1) as u8);
            let mode = next(&mut socket, &cipher).await;
            assert_eq!(mode.opcode, crate::wire_opcode::DraftModeAnnouncement);
            assert_eq!(mode.body[..4], 3u32.to_be_bytes());
            let snapshot = until(&mut socket, &cipher, crate::wire_opcode::DraftRosterAral).await;
            let Fields::AralRoster(view) = snapshot.fields().unwrap() else {
                panic!("expected ARAL roster")
            };
            assert_eq!(view.allotted_seconds(), 20.);
            assert_eq!(view.identity(index).unwrap(), player.to_string().as_bytes());
            assert!(!view.slot(6).unwrap().occupied);
            clients.push(socket);
        }

        for player in &players[1..] {
            let notice = until(
                &mut clients[0],
                &cipher,
                crate::wire_opcode::RegisterPlayerUuid,
            )
            .await;
            assert_eq!(
                notice.fields().unwrap(),
                Fields::PlayerUuid(&player.to_string())
            );
        }
        for refresh in 0..3 {
            let request = Message::known(crate::Opcode::DraftRefreshRequest, [0; 6]);
            clients[0]
                .write_all(&cipher.encode(&request).unwrap())
                .await
                .unwrap();
            assert_eq!(
                next(&mut clients[0], &cipher).await.opcode,
                crate::wire_opcode::DraftModeAnnouncement
            );
            let catalog_len = rooms.room(room_id).unwrap().lock().unwrap().catalog.len();
            for _ in 0..catalog_len {
                assert_eq!(
                    next(&mut clients[0], &cipher).await.opcode,
                    crate::wire_opcode::DraftCatalogEntry
                );
            }
            let first = next(&mut clients[0], &cipher).await;
            assert_eq!(first.opcode, crate::wire_opcode::DraftRosterAral);
            if refresh == 0 {
                let sync = next(&mut clients[0], &cipher).await;
                let mut expected = [0; 86];
                for (i, value) in [(3, 1), (7, 5), (76, 1), (80, 3)] {
                    expected[i] = value;
                }
                assert_eq!(
                    sync,
                    Message::known(crate::Opcode::AralDraftMarker, expected)
                );
                let second = next(&mut clients[0], &cipher).await;
                assert_eq!(second.opcode, crate::wire_opcode::DraftRosterAral);
                assert_eq!(first.body[8..], second.body[8..]);
            }
            assert_eq!(next(&mut clients[0], &cipher).await, request);
        }
        let room = rooms.room(room_id).unwrap();
        {
            let mut room = room.lock().unwrap();
            room.phase_start = Instant::now() - Duration::from_secs(21);
            room.tick();
            assert_eq!(room.phase, crate::RoomPhase::Loading);
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
                match frame.known_opcode() {
                    Some(crate::Opcode::RegisterPlayerUuid | crate::Opcode::DraftRosterAral) => {}
                    Some(crate::Opcode::PlayerInfo) => roster_count += 1,
                    Some(crate::Opcode::DraftTransitionToLoading) => break,
                    _ => panic!("unexpected frame {:#x} before world request", frame.opcode),
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
            let handle = until(
                &mut clients[0],
                &cipher,
                crate::wire_opcode::PlayerLoadedScene,
            )
            .await;
            assert!(
                matches!(handle.fields().unwrap(), Fields::HandleRecord {text:b"Guest", field64,lookup, ..}
                if field64 == seat as u32 && lookup == actor)
            );
            assert_eq!(
                next(&mut clients[0], &cipher).await,
                Message::known(crate::Opcode::RequestSceneSnapshot, [0; 6])
            );
            let confirmation = next(&mut clients[0], &cipher).await;
            assert_eq!(confirmation.opcode, crate::wire_opcode::ConfirmSceneOwner);
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
            assert_eq!(
                until(socket, &cipher, crate::wire_opcode::StartMatch)
                    .await
                    .body,
                [0; 6]
            );
        }
        assert!(
            room.lock()
                .unwrap()
                .seats
                .iter()
                .all(|seat| seat.world_actor_written)
        );
        let original_home = room.lock().unwrap().world.entities[0].position;
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
            let learn = Message::known(crate::Opcode::RequestUpgradeAbility, [1, 0, 0, 0, 0, 0]);
            socket
                .write_all(&cipher.encode(&learn).unwrap())
                .await
                .unwrap();
            until(socket, &cipher, crate::wire_opcode::GrantAbility).await;
            let mut body = [0; 14];
            body[4..8].copy_from_slice(&(-0.0_f32).to_be_bytes());
            body[8..12].copy_from_slice(&0.88_f32.to_be_bytes());
            body[12] = 3;
            body[..4].copy_from_slice(&1000_f32.to_be_bytes());
            let invalid = Message::known(crate::Opcode::RequestPointAbility, body);
            socket
                .write_all(&cipher.encode(&invalid).unwrap())
                .await
                .unwrap();
            let barrier = Message::known(crate::Opcode::ClockSync, [0; 6]);
            socket
                .write_all(&cipher.encode(&barrier).unwrap())
                .await
                .unwrap();
            loop {
                let frame = next(socket, &cipher).await;
                assert!(!matches!(
                    frame.opcode,
                    crate::wire_opcode::RequestPointAbility | crate::wire_opcode::PlayPointAbility
                ));
                if frame == barrier {
                    break;
                }
            }
            body[..4].copy_from_slice(&(-74.0_f32).to_be_bytes());
            let cast = Message::known(crate::Opcode::RequestPointAbility, body);
            let cancel = Message::known(crate::Opcode::RequestCancelAbility, [3, 0, 0, 0, 0, 0]);
            let packet = [
                cipher.encode(&cast).unwrap(),
                cipher.encode(&cancel).unwrap(),
            ]
            .concat();
            socket.write_all(&packet).await.unwrap();
            assert_eq!(
                until(socket, &cipher, crate::wire_opcode::RequestCancelAbility).await,
                cancel
            );
            let confirmed = next(socket, &cipher).await;
            assert!(confirmed.is(crate::Opcode::CancelAbility));
            assert_eq!(&confirmed.body[..10], &[0, 0, 5, 220, 0, 0, 0, 3, 0, 0]);
            socket
                .write_all(&cipher.encode(&cast).unwrap())
                .await
                .unwrap();
            loop {
                let frame = next(socket, &cipher).await;
                assert_ne!(frame.opcode, crate::wire_opcode::PlayPointAbility);
                if frame.is(crate::Opcode::RequestPointAbility) {
                    assert_eq!(frame, cast);
                    break;
                }
            }
            let release = until(socket, &cipher, crate::wire_opcode::PlayPointAbility).await;
            assert_eq!(release.body[..4], 1500_u32.to_be_bytes());
            assert_eq!(release.body[4..17], cast.body[..13]);
        }
        let point = |x: f32, z: f32| {
            Message::known(
                crate::Opcode::RequestMovePoint,
                [x.to_be_bytes(), z.to_be_bytes()].concat(),
            )
        };
        for (seat, opcode, x, z) in [
            (0, crate::wire_opcode::RequestMovePoint, -74.0, 0.88),
            (3, crate::wire_opcode::RequestMoveJoystick, 75.0, -5.06),
            (0, crate::wire_opcode::RequestMoveJoystick, -74.0, 0.88),
        ] {
            let mut request = point(x, z);
            request.opcode = opcode;
            clients[seat]
                .write_all(&cipher.encode(&request).unwrap())
                .await
                .unwrap();
            let reply = until(&mut clients[seat], &cipher, crate::wire_opcode::MoveTo).await;
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
        let barrier = Message::known(crate::Opcode::ClockSync, [0; 6]);
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
                if response.is(crate::Opcode::MoveTo) {
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
        assert_eq!(
            until(&mut clients[0], &cipher, crate::wire_opcode::MoveTo)
                .await
                .body[0],
            16
        );
        assert_eq!(
            until(&mut clients[1], &cipher, crate::wire_opcode::MoveTo)
                .await
                .body[0],
            16
        );
        for target in [1500_u32, 19999, 1501] {
            let mut body = target.to_be_bytes().to_vec();
            body.push(0);
            clients[0]
                .write_all(
                    &cipher
                        .encode(&Message::known(
                            crate::Opcode::RequestModifyBasicAttackTarget,
                            body,
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
        }
        let ack = until(
            &mut clients[0],
            &cipher,
            crate::wire_opcode::RequestModifyBasicAttackTarget,
        )
        .await;
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
            let announced = until(socket, &cipher, crate::wire_opcode::RequestEquipTalent).await;
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
        until(&mut clients[0], &cipher, crate::wire_opcode::EquipTalent).await;
        clients[1]
            .write_all(&cipher.encode(&barrier).unwrap())
            .await
            .unwrap();
        loop {
            let frame = next(&mut clients[1], &cipher).await;
            if frame == barrier {
                break;
            }
            assert!(!matches!(
                frame.opcode,
                crate::wire_opcode::RequestEquipTalent | crate::wire_opcode::EquipTalent
            ));
        }
        let selection = Message::known(crate::Opcode::RequestRecommendedBuildPath, [2]);
        clients[0]
            .write_all(&cipher.encode(&selection).unwrap())
            .await
            .unwrap();
        until(
            &mut clients[0],
            &cipher,
            crate::wire_opcode::SetRecommendedBuildPath,
        )
        .await;
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
                    .encode(&Message::known(
                        crate::Opcode::RegisterPlayerUuid,
                        grant.to_string().as_bytes(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let scoreboard = next(&mut observer, &cipher).await;
        assert!(scoreboard.is(crate::Opcode::DraftRosterAral));
        for actor in 1500_u32..1506 {
            let frame = next(&mut observer, &cipher).await;
            assert!(frame.is(crate::Opcode::PlayerInfo));
            assert_eq!(&frame.body[..128], &[0; 128]);
            assert_eq!(&frame.body[160..164], &actor.to_be_bytes());
        }
        let path = until(
            &mut observer,
            &cipher,
            crate::wire_opcode::SetRecommendedBuildPath,
        )
        .await;
        assert_eq!(&path.body[..5], &[0, 0, 5, 220, 2]);
        let restored = until(&mut observer, &cipher, crate::wire_opcode::EquipTalent).await;
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
            let cleared = until(socket, &cipher, crate::wire_opcode::EquipTalent).await;
            assert_eq!(cleared.body, [0, 0, 5, 220, 255, 0]);
        }
        if scenario == Scenario::ItemActions {
            use halcyon_protocol::Opcode;
            {
                let mut room = room.lock().unwrap();
                let far = room.world.entities[1].position;
                for idx in 0..room.world.entities.len().min(SEATS) {
                    let e = &mut room.world.entities[idx];
                    e.position = if e.team == 0 {
                        original_home
                    } else {
                        halcyon_sim::Position {
                            x: far.x.max(20.),
                            y: far.y,
                        }
                    };
                }
                let id = room.world.entities[0].id;
                room.world.command(id, halcyon_sim::Command::Stop).unwrap();
                room.tick();
            }
            room.lock().unwrap().world.entities[0].gold += 500;
            let buy_infusion = Message::known(
                Opcode::BuyItem,
                [1500u32.to_be_bytes(), 493u32.to_be_bytes()].concat(),
            );
            clients[0]
                .write_all(&cipher.encode(&buy_infusion).unwrap())
                .await
                .unwrap();
            let held = loop {
                let grant = until(&mut clients[0], &cipher, crate::wire_opcode::ItemGrant).await;
                if grant.body[4..8] == 493u32.to_be_bytes() {
                    break u32::from_be_bytes(grant.body[8..12].try_into().unwrap());
                }
            };
            let press = cipher
                .encode(&Message::known(
                    Opcode::RequestActivateItemSelf,
                    held.to_be_bytes(),
                ))
                .unwrap();
            clients[0]
                .write_all(&[press.as_slice(), press.as_slice()].concat())
                .await
                .unwrap();
            let [owner, ally] = clients.get_disjoint_mut([0, 2]).unwrap();
            let mut shared = None;
            for socket in [owner, ally, &mut observer] {
                let effect = loop {
                    let m = until(socket, &cipher, crate::wire_opcode::ApplyBuffCompact).await;
                    if let Fields::BuffApplication(b) = m.fields().unwrap()
                        && b.prefix.definition
                            == halcyon_protocol::BuffDefinition::WeaponInfusion as u16
                    {
                        assert_eq!(
                            (b.prefix.source, b.prefix.target, b.prefix.duration_bits),
                            (1500, 1500, 0x58b0)
                        );
                        break b.prefix.instance;
                    }
                };
                assert!(shared.replace(effect).is_none_or(|old| old == effect));
                let m = next(socket, &cipher).await;
                let stack = m.buff_stack().unwrap();
                assert_eq!(
                    (stack.target, stack.instance, stack.count),
                    (
                        1500,
                        effect,
                        u32::from(room.lock().unwrap().world.entities[0].level)
                    )
                );
            }
            let buy_block = Message::known(
                Opcode::BuyItem,
                [1500u32.to_be_bytes(), 485u32.to_be_bytes()].concat(),
            );
            clients[0]
                .write_all(&cipher.encode(&buy_block).unwrap())
                .await
                .unwrap();
            let instance = loop {
                let m = until(&mut clients[0], &cipher, crate::wire_opcode::ItemGrant).await;
                if m.body[4..8] == 485u32.to_be_bytes() {
                    break u32::from_be_bytes(m.body[8..12].try_into().unwrap());
                }
            };
            let press = Message::known(Opcode::RequestActivateItemSelf, instance.to_be_bytes());
            let packet = cipher.encode(&press).unwrap();
            clients[0]
                .write_all(&[packet.as_slice(), packet.as_slice()].concat())
                .await
                .unwrap();
            let [owner, ally] = clients.get_disjoint_mut([0, 2]).unwrap();
            for socket in [owner, ally, &mut observer] {
                until(socket, &cipher, crate::wire_opcode::ItemActivated).await;
                let mut key = 0;
                for opcode in [
                    Opcode::ApplyBuffCompact,
                    Opcode::ApplyBuffCompact,
                    Opcode::ModifyActorResource,
                    Opcode::ApplyBuffCompact,
                ] {
                    let m = next(socket, &cipher).await;
                    assert!(m.is(opcode));
                    if let Fields::BuffApplication(effect) = m.fields().unwrap()
                        && effect.prefix.definition == 268
                    {
                        key = effect.prefix.instance;
                    }
                }
                assert!(key > 1505);
                let end = until(socket, &cipher, crate::wire_opcode::TimeoutBuff).await;
                assert_eq!(end.buff_mutation().unwrap().instance, key);
            }
            let buy = Message::known(
                Opcode::BuyItem,
                [1500u32.to_be_bytes(), 473u32.to_be_bytes()].concat(),
            );
            clients[0]
                .write_all(&cipher.encode(&buy).unwrap())
                .await
                .unwrap();
            let mut instance = None;
            for socket in [&mut clients[2], &mut observer] {
                loop {
                    let grant = until(socket, &cipher, crate::wire_opcode::ItemGrant).await;
                    if grant.body[0..4] == 1500u32.to_be_bytes()
                        && grant.body[4..8] == 473u32.to_be_bytes()
                    {
                        let id = u32::from_be_bytes(grant.body[8..12].try_into().unwrap());
                        assert!(instance.replace(id).is_none_or(|was| was == id));
                        break;
                    }
                }
            }
            let instance = instance.unwrap();
            let balance = {
                let mut state = room.lock().unwrap();
                let id = state.world.entities[0].id;
                let sale = state.world.sell_item(id, instance).unwrap();
                assert!(state.world.sell_item(id, instance).is_err());
                state
                    .world
                    .command(
                        id,
                        halcyon_sim::Command::ActivateHealingFlask { instance: 2000 },
                    )
                    .unwrap();
                assert!(
                    state
                        .world
                        .command(
                            id,
                            halcyon_sim::Command::ActivateHealingFlask { instance: 2000 }
                        )
                        .is_err()
                );
                let point = state.world.entities[0].position;
                state
                    .world
                    .command(
                        id,
                        halcyon_sim::Command::DeployVisionTotem {
                            instance: 2001,
                            point,
                        },
                    )
                    .unwrap();
                state.tick();
                sale.balance
            };
            for socket in [&mut clients[2], &mut observer] {
                let sale = until(socket, &cipher, crate::wire_opcode::SellItem).await;
                assert_eq!(
                    &sale.body[..8],
                    &[1500u32.to_be_bytes(), instance.to_be_bytes()].concat()
                );
                for _ in 0..2 {
                    assert!(next(socket, &cipher).await.is(Opcode::ModifyActorAttribute));
                }
                let remove = next(socket, &cipher).await;
                assert!(remove.is(Opcode::ConsumeItem));
                assert_eq!(
                    &remove.body[..10],
                    &[sale.body[..8].to_vec(), vec![0, 1]].concat()
                );
                let money =
                    halcyon_protocol::ResourceChange::read(&next(socket, &cipher).await).unwrap();
                assert_eq!(
                    (money.actor, money.index, money.set_mode, money.value),
                    (1500, 6, 1, balance as f32)
                );
                let request = next(socket, &cipher).await;
                assert!(request.is(Opcode::RequestActivateItemSelf));
                assert_eq!(&request.body[..4], &2000u32.to_be_bytes());
                let active = next(socket, &cipher).await;
                assert!(active.is(Opcode::ItemActivated));
                assert_eq!(
                    &active.body[..12],
                    &[
                        1500u32.to_be_bytes(),
                        2000u32.to_be_bytes(),
                        u32::MAX.to_be_bytes()
                    ]
                    .concat()
                );
                assert_eq!(&active.body[12..21], &[0, 0, 0, 0, 0, 0, 0, 0, 2]);
                let point = next(socket, &cipher).await;
                assert!(point.is(Opcode::RequestActivateItemPoint));
                assert_eq!(&point.body[12..16], &2001_u32.to_be_bytes());
                let cam = next(socket, &cipher).await;
                assert!(cam.is(Opcode::ItemActivated));
                assert_eq!(&cam.body[..4], &1500_u32.to_be_bytes());
                assert_eq!(&cam.body[12..16], &point.body[..4]);
                assert_eq!(&cam.body[16..20], &point.body[8..12]);
                assert_eq!(cam.body[20], 1);
            }
            for seat in [0, 1] {
                clients[seat]
                    .write_all(&cipher.encode(&barrier).unwrap())
                    .await
                    .unwrap();
                loop {
                    let m = next(&mut clients[seat], &cipher).await;
                    if m == barrier {
                        break;
                    }
                    assert!(!matches!(
                        m.known_opcode(),
                        Some(
                            Opcode::SellItem
                                | Opcode::RequestActivateItemSelf
                                | Opcode::RequestActivateItemPoint
                                | Opcode::ItemActivated
                        )
                    ));
                }
            }
            rooms.revoke_observer(room_id, grant).unwrap();
            return;
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
                u32::from(crate::messages::wire_id(tower_id))
            };
            let mut shared_id = None;
            for socket in clients
                .iter_mut()
                .take(2)
                .chain(std::iter::once(&mut observer))
            {
                tokio::time::timeout(Duration::from_secs(2), async {
                    let mut releases = 0;
                    let mut fires = 0;
                    loop {
                        let msg = next(socket, &cipher).await;
                        if msg.is(crate::Opcode::PlayTargetAbility)
                            && msg.body[..4] == wire_tower.to_be_bytes()
                            && msg.body[4..8] == 1501u32.to_be_bytes()
                        {
                            assert_eq!(msg.body[8], 0);
                            releases += 1;
                        }
                        if let Fields::Projectile(halcyon_protocol::ProjectileAction::FireTracked(
                            f,
                        )) = msg.fields().unwrap()
                            && f.slots == [7, 7, 17]
                        {
                            assert_eq!(releases, 1);
                            assert_eq!(
                                (f.profile_word, f.values, f.operand),
                                (403962522, [0.], 112)
                            );
                            assert_eq!(*shared_id.get_or_insert(f.projectile), f.projectile);
                            fires += 1;
                        }
                        if msg.is(crate::Opcode::HealthImpact)
                            && msg.body[..4] == 1501u32.to_be_bytes()
                            && msg.body[4..8] == wire_tower.to_be_bytes()
                        {
                            assert_eq!((releases, fires), (1, 1));
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
                        if matches!(
                            m.opcode,
                            crate::wire_opcode::EntitySpawn | crate::wire_opcode::HeroSpawn
                        ) && m.body[8..12] == wire_actor.to_be_bytes()
                        {
                            registered = Some(
                                m.body[if m.is(crate::Opcode::EntitySpawn) {
                                    116
                                } else {
                                    745
                                }],
                            );
                        }
                        if m.is(crate::Opcode::MoveTo) && Some(m.body[0]) == registered {
                            let expected =
                                crate::point_move::destination(registered.unwrap(), destination);
                            assert_eq!(m.body[..9], expected.body);
                            saw_path = true;
                        }
                        if m.is(crate::Opcode::ModifyVisibility)
                            && m.body[..4] == wire_actor.to_be_bytes()
                        {
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
                if m.is(crate::Opcode::ModifyVisibility) {
                    assert_ne!(m.body[..4], hidden);
                }
                if matches!(
                    m.opcode,
                    crate::wire_opcode::EntitySpawn | crate::wire_opcode::HeroSpawn
                ) {
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
            until(socket, &cipher, crate::wire_opcode::EquipTalent).await;
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
        let talent = until(&mut clients[3], &cipher, crate::wire_opcode::EquipTalent).await;
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
        until(
            &mut clients[0],
            &cipher,
            crate::wire_opcode::MoveToCorrection,
        )
        .await;
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
                    .encode(&Message::known(
                        crate::Opcode::RegisterPlayerUuid,
                        players[3].to_string().as_bytes(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        until(
            &mut returned,
            &cipher,
            crate::wire_opcode::DraftTransitionToLoading,
        )
        .await;
        request_scene(&mut returned, &cipher, 1503, false).await;
        ready(&mut returned, &cipher).await;
        returned
            .write_all(&cipher.encode(&point(75.0, -5.06)).unwrap())
            .await
            .unwrap();
        assert_eq!(
            until(&mut returned, &cipher, crate::wire_opcode::MoveTo)
                .await
                .body[0],
            19
        );
        clients[3] = returned;

        if surrender {
            let yes = cipher
                .encode(&Message::known(
                    crate::Opcode::SetSurrenderStateRequest,
                    [1],
                ))
                .unwrap();
            for (votes, seat) in [1, 3, 5].into_iter().enumerate() {
                clients[seat].write_all(&yes).await.unwrap();
                for teammate in [1, 3, 5] {
                    let board = until(
                        &mut clients[teammate],
                        &cipher,
                        crate::wire_opcode::TeamSurrenderStateChanged,
                    )
                    .await;
                    assert_eq!(board.body[148..151], [2, if votes == 2 { 2 } else { 1 }, 3]);
                    assert_eq!(
                        board.body[68..71].iter().filter(|v| **v == 1).count(),
                        votes + 1
                    );
                }
            }
            let postgame_clock = Message::clock(123.5);
            clients[1]
                .write_all(&cipher.encode(&point(75.0, -5.06)).unwrap())
                .await
                .unwrap();
            clients[1]
                .write_all(&cipher.encode(&postgame_clock).unwrap())
                .await
                .unwrap();
            loop {
                let response = next(&mut clients[1], &cipher).await;
                assert_ne!(response.opcode, crate::wire_opcode::MoveTo);
                if response.is(crate::Opcode::ClockSync) {
                    assert_eq!(response.body[..4], 123.5_f32.to_be_bytes());
                    break;
                }
            }
            let mut room = room.lock().unwrap();
            assert_eq!(room.phase, crate::RoomPhase::Ending);
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
                .find_mut(|e| e.kind == Kind::Crystal && e.team == 1)
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
            assert_eq!(room.phase, crate::RoomPhase::Ending);
            assert!(!*room.stop.borrow());
            room.phase_start = Instant::now() - Duration::from_secs(6);
            room.tick();
            assert!(*room.stop.borrow());
        }
        for socket in &mut clients {
            let message = if surrender {
                loop {
                    let message = next(socket, &cipher).await;
                    assert!(!matches!(
                        message.opcode,
                        crate::wire_opcode::CrystalShattered
                            | crate::wire_opcode::TeamSurrenderStateChanged
                    ));
                    if message.is(crate::Opcode::PostMatchStats) {
                        break message;
                    }
                }
            } else {
                let postgame = until(socket, &cipher, crate::wire_opcode::CrystalShattered).await;
                assert_eq!(postgame.body, [0; 6]);
                let death = until(socket, &cipher, crate::wire_opcode::ActorDeath).await;
                assert_eq!(
                    u32::from_be_bytes(death.body[..4].try_into().unwrap()),
                    2009
                );
                until(socket, &cipher, crate::wire_opcode::PostMatchStats).await
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
                (
                    crate::wire_opcode::EndMatch,
                    &[0, 0, 0, 1, if surrender { 2 } else { 0 }, 0][..]
                )
            );
        }
    }

    #[tokio::test]
    async fn dead_hero_aral_reconnect_clears_afk_and_sets_world_actor_written() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        let players: Vec<_> = (0..SEATS).map(|_| Uuid::new_v4()).collect();
        let address = rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: id,
                players: players.clone(),
                teams: vec![0, 1, 0, 1, 0, 1],
                mode: "casual_aral".into(),
                difficulty: None,
            })
            .unwrap()
            .address;
        let room = rooms.room(id).unwrap();
        let cipher = MatchCipher::new(&id.to_string());
        let mut clients = Vec::with_capacity(SEATS);
        for player in &players {
            let mut socket = TcpStream::connect(&address).await.unwrap();
            socket
                .write_all(&hello("localhost").unwrap())
                .await
                .unwrap();
            let mut ack = [0; 5];
            socket.read_exact(&mut ack).await.unwrap();
            assert_eq!(ack, ACK_FRAME);
            socket
                .write_all(
                    &cipher
                        .encode(&Message::known(
                            crate::Opcode::RegisterPlayerUuid,
                            player.to_string().as_bytes(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            until(&mut socket, &cipher, crate::wire_opcode::DraftRosterAral).await;
            clients.push(socket);
        }
        room.lock().unwrap().phase_start =
            Instant::now() - Duration::from_secs(DRAFT_SECONDS as u64);
        for (seat, socket) in clients.iter_mut().enumerate() {
            until(
                socket,
                &cipher,
                crate::wire_opcode::DraftTransitionToLoading,
            )
            .await;
            request_scene(socket, &cipher, 1500 + seat as u32, true).await;
            ready(socket, &cipher).await;
        }
        for socket in &mut clients {
            until(socket, &cipher, crate::wire_opcode::StartMatch).await;
        }

        // Disconnect seat 0 and verify witness seat 1 observes AFK.
        clients[0].shutdown().await.unwrap();
        timeout(Duration::from_secs(2), async {
            while room.lock().unwrap().seats[0].peers[0].is_some() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let afk = until(&mut clients[1], &cipher, crate::wire_opcode::PlayerAfk).await;
        assert_eq!(afk.actor_flags().unwrap().actor, 1500);

        // Kill hero 0 via authoritative combat while disconnected.
        {
            let mut r = room.lock().unwrap();
            let pos = r.world.entities[0].position;
            r.world.entities[1].position = pos;
            r.world.entities[0].hp = 0.1;
            r.world.command(2, halcyon_sim::Command::Attack(1)).unwrap();
            while r.world.entities[0].hp > 0.0 {
                r.tick();
            }
        }

        // Reconnect seat 0 while dead: opening omits 1500 EntitySpawn, yet write completion
        // marks world_actor_written and clears AFK immediately.
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
                    .encode(&Message::known(
                        crate::Opcode::RegisterPlayerUuid,
                        players[0].to_string().as_bytes(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        until(
            &mut returned,
            &cipher,
            crate::wire_opcode::DraftTransitionToLoading,
        )
        .await;
        returned
            .write_all(
                &cipher
                    .encode(&Message::known(crate::Opcode::RequestSceneSnapshot, [0; 6]))
                    .unwrap(),
            )
            .await
            .unwrap();
        returned
            .write_all(
                &cipher
                    .encode(&Message::known(
                        crate::Opcode::ConfirmSceneOwner,
                        [0, 0, 5, 220, 1, 0],
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        loop {
            let msg = next(&mut returned, &cipher).await;
            if msg.is(crate::Opcode::HeroSpawn) {
                let actor = u32::from_be_bytes(msg.body[8..12].try_into().unwrap());
                assert_ne!(actor, 1500, "dead hero must not spawn in aral::opening");
            }
            if msg.is(crate::Opcode::ConfirmSceneOwner) {
                break;
            }
        }
        timeout(Duration::from_secs(2), async {
            while !room.lock().unwrap().seats[0].world_actor_written {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        let reconnected = until(
            &mut clients[1],
            &cipher,
            crate::wire_opcode::PlayerReconnected,
        )
        .await;
        assert_eq!(reconnected.actor_flags().unwrap().actor, 1500);
        ready(&mut returned, &cipher).await;
        until(&mut returned, &cipher, crate::wire_opcode::StartMatch).await;

        // Advance simulation until hero 0 respawns; verify 0x03F3 + 0x0432 arrive on reconnect stream.
        {
            let mut r = room.lock().unwrap();
            while r.world.entities[0].hp <= 0.0 {
                r.tick();
            }
        }
        let mut saw_spawn = false;
        let mut saw_respawn = false;
        timeout(Duration::from_secs(4), async {
            while !(saw_spawn && saw_respawn) {
                let msg = next(&mut returned, &cipher).await;
                if msg.is(crate::Opcode::HeroSpawn) && msg.body[8..12] == 1500u32.to_be_bytes() {
                    saw_spawn = true;
                }
                if msg.is(crate::Opcode::ActorRespawn) && msg.body[..4] == 1500u32.to_be_bytes() {
                    assert!(saw_spawn);
                    saw_respawn = true;
                }
            }
        })
        .await
        .unwrap();
    }
}

#[cfg(test)]
mod reroll_tests {
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
}
