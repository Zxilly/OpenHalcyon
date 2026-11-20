//! Game-room lifecycle, scene declarations, 0x041F mode variables, Blitz flyouts/talents,
//! and protocol integration for 3v3 Halcyon Fold (`CasualFold` / `RankedFold`) and `Blitz`.

use super::{Batch, Room, messages, point_move, transport};
use halcyon_data::EntityTemplate;
use halcyon_protocol::{Flyout, FlyoutText, Message, ce::KnownQueue};
use halcyon_sim::{BlitzTalentSlot, Entity, Event, Kind, TICK_HZ, World};

const STRUCTURE_SKIN: u32 = 0xc10b41da;
const SCENERY_ACTOR_BASE: u32 = 2000;
const JUNGLE_SHOP_SLOT: u8 = 16;

pub(crate) const BLITZ_SCORE_LEFT_KEY: u32 = fnv1a_32(b"blitz_score_left");
pub(crate) const BLITZ_SCORE_RIGHT_KEY: u32 = fnv1a_32(b"blitz_score_right");
pub(crate) const BLITZ_TIMER_KEY: u32 = fnv1a_32(b"blitz_remaining_seconds");
pub(crate) const BLITZ_SUDDEN_DEATH_KEY: u32 = fnv1a_32(b"blitz_sudden_death");

const fn fnv1a_32(bytes: &[u8]) -> u32 {
    let mut hash = 0x811c_9dc5_u32;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u32;
        hash = hash.wrapping_mul(0x0100_0193);
        i += 1;
    }
    hash
}

pub(crate) fn is_halcyon_fold(queue: Option<KnownQueue>) -> bool {
    queue.is_some_and(|q| q.mode().is_halcyon_fold())
}

pub(crate) fn is_blitz(queue: Option<KnownQueue>) -> bool {
    queue.is_some_and(|q| q.mode().is_blitz())
}

pub(crate) fn is_managed(queue: Option<KnownQueue>) -> bool {
    is_halcyon_fold(queue) || is_blitz(queue)
}

/// Ensures the authoritative simulation layout (`HalcyonFold` or `Blitz`) and native
/// A001 navmesh / unit data are initialized before entering Loading / Running.
pub(crate) fn prepare_world(room: &mut Room) -> std::io::Result<()> {
    if is_halcyon_fold(room.known_queue) {
        room.world
            .use_halcyon_fold()
            .map_err(std::io::Error::other)?;
        if let Some(data) = &room.data {
            room.world
                .install_map_data(data)
                .map_err(std::io::Error::other)?;
        }
    } else if is_blitz(room.known_queue) {
        room.world.use_blitz().map_err(std::io::Error::other)?;
        if let Some(data) = &room.data {
            room.world
                .install_map_data(data)
                .map_err(std::io::Error::other)?;
        }
    }
    Ok(())
}

/// Generates a 122-byte `EntitySpawn` (`0x03F2`) frame for a Halcyon Fold or Blitz structure.
pub(crate) fn structure_spawn(world: &World, entity: &Entity) -> Option<Message> {
    let slot = world
        .halcyon_fold_structure_slot(entity.id)
        .or_else(|| world.blitz_structure_slot(entity.id))?;
    let template = EntityTemplate::from_raw(entity.type_id)?;
    let (height, forward, profile_word84) = match (entity.kind, template, slot % 8) {
        (Kind::Crystal, EntityTemplate::VainCrystalHome3v3, 1) => (0.0, [0.0, 0.0, 1.0], 12.0),
        (Kind::Crystal, EntityTemplate::VainCrystalAway3v3, 1) => {
            (0.1, [0.104_528_464, 0.0, 0.994_521_9], 12.0)
        }
        (Kind::Tower, _, 5) => {
            let x = if entity.team == 0 {
                0.965_925_8
            } else {
                -0.965_925_8
            };
            (0.0, [x, 0.0, 0.258_819_07], 1.0)
        }
        (Kind::Tower, _, 6 | 7) => {
            let x = if entity.team == 0 { 1.0 } else { -1.0 };
            (0.0, [x, 0.0, 0.0], 1.0)
        }
        (Kind::Tower, _, 3 | 4) => {
            let x = if entity.team == 0 {
                0.766_044_44
            } else {
                -0.766_044_44
            };
            (0.0, [x, 0.0, -0.642_787_64], 1.0)
        }
        _ => return None,
    };
    if !entity.hp.is_finite() || !entity.max_hp.is_finite() || entity.hp <= 0.0 {
        return None;
    }
    let mut body = [0u8; 122];
    body[..4].copy_from_slice(&u32::from(entity.type_id).to_be_bytes());
    body[4..8].copy_from_slice(&STRUCTURE_SKIN.to_be_bytes());
    body[8..12].copy_from_slice(&u32::from(messages::wire_id(entity.id)).to_be_bytes());
    put_floats(
        &mut body,
        12,
        [entity.position.x, height, entity.position.y],
    );
    put_floats(&mut body, 24, forward);
    put_floats(&mut body, 36, [entity.hp, entity.max_hp]);
    if entity.kind == Kind::Tower {
        put_floats(&mut body, 64, [1.0]);
    }
    put_floats(&mut body, 84, [profile_word84]);
    body[88..104].fill(1);
    body[97 + usize::from(entity.team)] = 15;
    body[112..116].fill(u8::MAX);
    body[116] = slot;
    body[119] = 1;
    body[120] = u8::MAX;
    body[121] = entity.team + 1;
    Some(Message::known(crate::Opcode::EntitySpawn, body))
}

struct ScenerySpec {
    slot: u8,
    template: EntityTemplate,
    position: [f32; 3],
    direction: [f32; 3],
    hp: f32,
}

/// Generates static scenery `0x03F2` declarations (base shops, vision beacons, Jungle Shop).
pub(crate) fn scenery(blitz: bool) -> Vec<Message> {
    let mut out = Vec::with_capacity(5);
    let specs = [
        ScenerySpec {
            slot: 0,
            template: EntityTemplate::NeutralJungleMinionStore,
            position: [-88.5, 0.89, 2.0],
            direction: [0.766_044_44, 0.0, -0.642_787_64],
            hp: 868.0,
        },
        ScenerySpec {
            slot: 8,
            template: EntityTemplate::NeutralJungleMinionStore,
            position: [88.565, 1.8, 0.51],
            direction: [0.0, 0.0, 1.0],
            hp: 868.0,
        },
        ScenerySpec {
            slot: 2,
            template: EntityTemplate::VisionBeacon,
            position: [-76.12, 0.0, 19.9],
            direction: [0.0, 0.0, 1.0],
            hp: 9999.0,
        },
        ScenerySpec {
            slot: 10,
            template: EntityTemplate::VisionBeacon,
            position: [76.12, 0.1, 19.9],
            direction: [0.104_528_464, 0.0, 0.994_521_9],
            hp: 9999.0,
        },
        ScenerySpec {
            slot: JUNGLE_SHOP_SLOT,
            template: if blitz {
                EntityTemplate::BlitzJungleShopCenter
            } else {
                EntityTemplate::JungleShopCenter
            },
            position: [0.2, 0.0, 42.0],
            direction: [0.0, 0.0, -1.0],
            hp: 450.0,
        },
    ];
    for spec in specs {
        let ScenerySpec {
            slot,
            template,
            position,
            direction,
            hp,
        } = spec;
        let team = if slot == JUNGLE_SHOP_SLOT {
            2
        } else {
            slot / 8
        };
        let mut body = [0u8; 122];
        body[..4].copy_from_slice(&u32::from(template.as_raw()).to_be_bytes());
        body[4..8].copy_from_slice(&STRUCTURE_SKIN.to_be_bytes());
        body[8..12].copy_from_slice(&(SCENERY_ACTOR_BASE + u32::from(slot)).to_be_bytes());
        put_floats(&mut body, 12, position);
        put_floats(&mut body, 24, direction);
        put_floats(&mut body, 36, [hp, hp]);
        put_floats(&mut body, 84, [12.0]);
        body[88..96].fill(1);
        if matches!(
            template,
            EntityTemplate::NeutralJungleMinionStore
                | EntityTemplate::JungleShopCenter
                | EntityTemplate::BlitzJungleShopCenter
        ) {
            put_floats(&mut body, 52, [2.5]);
            put_floats(&mut body, 64, [1.0]);
            body[96..104].fill(3);
        }
        if team < 2 {
            body[97 + usize::from(team)] = 15;
            body[121] = team + 1;
        } else {
            body[96] = 15;
            body[121] = 0;
        }
        body[112..116].fill(u8::MAX);
        body[116] = slot;
        body[119] = 1;
        body[120] = u8::MAX;
        out.push(Message::known(crate::Opcode::EntitySpawn, body));
    }
    out
}

/// Appends declarations for every visible actor (heroes `0x03F3`, structures `0x03F2`,
/// neutrals & minions), scenery, `0x041F` mode variables, and equipped talents.
pub(crate) fn append_opening(room: &Room, team: Option<u8>, batch: &mut Batch) {
    let blitz = is_blitz(room.known_queue);
    batch.extend(scenery(blitz));
    for e in &room.world.entities {
        if e.hp <= 0.0 || team.is_some_and(|side| !room.world.visible(side, e)) {
            continue;
        }
        if let Some(structure) = structure_spawn(&room.world, e) {
            batch.push(structure);
            continue;
        }
        if e.kind == Kind::Hero {
            batch.push(point_move::full_spawn(
                &room.world,
                e,
                &room.picks,
                room.data.as_deref(),
                false,
            ));
            messages::append_inventory(batch, &room.world, e);
        } else {
            batch.push(messages::spawn(e, &room.picks, room.data.as_deref()));
        }
    }
    if blitz {
        batch.extend(super::scene::mode_variables::Opening::Blitz.messages());
        batch.extend(blitz_mode_var_snapshot(&room.world));
        batch.extend(super::aral::talent_snapshot(room));
    } else if is_halcyon_fold(room.known_queue) {
        batch.extend(super::scene::mode_variables::Opening::HalcyonFold.messages());
    }
}

pub(crate) fn mode_var_message(key: u32, word: u32) -> Message {
    let mut body = [0u8; 14];
    body[..4].copy_from_slice(&key.to_be_bytes());
    body[4..8].copy_from_slice(&word.to_be_bytes());
    Message::known(crate::Opcode::ModifyGameModeVar, body)
}

pub(crate) fn blitz_mode_var_snapshot(world: &World) -> Vec<Message> {
    let Some(blitz) = world.blitz_state() else {
        return Vec::new();
    };
    vec![
        mode_var_message(BLITZ_SCORE_LEFT_KEY, blitz.team_points[0]),
        mode_var_message(BLITZ_SCORE_RIGHT_KEY, blitz.team_points[1]),
        mode_var_message(BLITZ_TIMER_KEY, blitz.remaining_seconds().to_bits()),
        mode_var_message(BLITZ_SUDDEN_DEATH_KEY, u32::from(blitz.sudden_death)),
    ]
}

/// Emits live `0x041F` score/timer updates and `0x0440` Blitz score/gameover flyouts.
pub(crate) fn publish_tick(room: &mut Room, events: &[Event]) {
    if !is_blitz(room.known_queue) {
        return;
    }
    let Some(blitz) = room.world.blitz_state() else {
        return;
    };
    let remaining_bits = blitz.remaining_seconds().to_bits();
    let mut frames = Vec::new();
    for ev in events {
        match *ev {
            Event::BlitzPoints {
                team,
                points,
                source,
                total,
                ..
            } => {
                let text = match points {
                    1 => Some(FlyoutText::OnePoint),
                    2 => Some(FlyoutText::TwoPoints),
                    3 => Some(FlyoutText::ThreePoints),
                    _ => None,
                };
                if let Some(text) = text {
                    let anchor = if source > 0 { source } else { 1 };
                    frames.push(
                        Flyout::new(u32::from(messages::wire_id(anchor)), text, total).to_message(),
                    );
                }
                let key = if team == 0 {
                    BLITZ_SCORE_LEFT_KEY
                } else {
                    BLITZ_SCORE_RIGHT_KEY
                };
                frames.push(mode_var_message(key, total));
            }
            Event::BlitzSuddenDeath => {
                frames.push(mode_var_message(BLITZ_SUDDEN_DEATH_KEY, 1));
            }
            Event::End { winner } => {
                frames.push(
                    Flyout::new(
                        u32::from(messages::wire_id(1)),
                        FlyoutText::GameOver,
                        u32::from(winner) + 1,
                    )
                    .to_message(),
                );
            }
            _ => {}
        }
    }
    if room.world.tick.is_multiple_of(TICK_HZ) {
        frames.push(mode_var_message(BLITZ_TIMER_KEY, remaining_bits));
    }
    if frames.is_empty() {
        return;
    }
    let batch = transport::TeamBatch::new(frames.clone());
    for peer in room.seats.iter().filter_map(super::Seat::gameplay_peer) {
        peer.send(transport::Delivery::Team(batch.clone()));
    }
    room.publish_observers(&frames);
}

/// Authorizes and applies a Blitz `0x0481` -> `0x0484` talent equip on either channel.
pub(crate) fn talent_response(
    room: &mut Room,
    seat: usize,
    selection: halcyon_protocol::TalentSelection,
) -> Batch {
    use halcyon_protocol::{EquippedTalent, TalentSelection};
    if !is_blitz(room.known_queue)
        || room.phase != crate::RoomPhase::Running
        || room.seats[seat].gameplay_peer().is_none()
    {
        return Vec::new();
    }
    let hero = &room.world.entities[seat];
    let level = if selection == TalentSelection::NoTalent {
        0
    } else {
        let Some(lvl) = room
            .talent_levels
            .get(seat)
            .and_then(|levels| levels.get(&hero.type_id))
        else {
            return Vec::new();
        };
        lvl.get()
    };
    let slot = match selection {
        TalentSelection::Slot0 => BlitzTalentSlot::Slot0,
        TalentSelection::Slot1 => BlitzTalentSlot::Slot1,
        TalentSelection::Slot2 => BlitzTalentSlot::Slot2,
        TalentSelection::NoTalent => BlitzTalentSlot::NoTalent,
    };
    let actor = hero.id;
    if room
        .world
        .equip_blitz_talent(actor, slot, level, room.data.as_deref())
        .is_err()
    {
        return Vec::new();
    }
    let equipped = EquippedTalent {
        actor: u32::from(messages::wire_id(actor)),
        selection,
        level,
    };
    let batch = vec![
        selection.request(),
        equipped.message().expect("validated Blitz talent equip"),
    ];
    if room.seats[seat].equipped_talent.replace(equipped) != Some(equipped) {
        let shared = transport::TeamBatch::new(batch.clone());
        for (i, other) in room.seats.iter().enumerate() {
            if i != seat
                && let Some(peer) = other.gameplay_peer()
            {
                peer.send(transport::Delivery::Team(shared.clone()));
            }
        }
        room.publish_observers(&batch);
    }
    batch
}

fn put_floats<const N: usize>(body: &mut [u8], offset: usize, values: [f32; N]) {
    for (word, value) in body[offset..offset + N * 4]
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(values)
    {
        word.copy_from_slice(&value.to_be_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Rooms, wire_opcode};
    use halcyon_data::{EntityTemplate, TalentLevels};
    use halcyon_platform::{MatchHost, Reservation};
    use halcyon_protocol::{Fields, LiveFields, TalentSelection, ce::NativeMode};
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
    async fn blitz_room_roster_045a_talents_0440_flyouts_and_score_endmatch() {
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
            assert_eq!(initial.last().unwrap().opcode, wire_opcode::DraftRosterAral);
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
}
