//! Game-room lifecycle, scene declarations, 0x041F mode variables, Blitz flyouts/talents,
//! and protocol integration for 3v3 Halcyon Fold (`CasualFold` / `RankedFold`) and `Blitz`.

use super::scene::mode_variables::{
    COUNTDOWN_DO_ROUNDING_KEY, COUNTDOWN_DURATION_KEY, COUNTDOWN_PAUSED_KEY, COUNTDOWN_START_KEY,
    TEAM_1_SCORE_KEY, TEAM_2_SCORE_KEY,
};
use super::{Batch, Room, messages, point_move, transport};
use halcyon_data::EntityTemplate;
use halcyon_protocol::{
    BuffApplication, BuffChange, BuffMutation, BuffPrefix, EntitySpawnBody, Flyout, FlyoutText,
    HeroSpawnBody, Message, SoloStructure, ce::KnownQueue,
};
use halcyon_sim::{
    BlitzTalentSlot, Entity, Event, Kind, TICK_HZ, World, map_blitz::BLITZ_DURATION_TICKS,
};

const SCENERY_ACTOR_BASE: u32 = 2000;
const JUNGLE_SHOP_SLOT: u8 = 16;
const FIRST_HERO_SLOT: u8 = 17;
const FIRST_DYNAMIC_SLOT: u8 = 52;
const ACTOR_SLOT_LIMIT: u8 = 200;

const BUFF_INVULNERABLE: u16 = 27;
const BUFF_GLOBALLY_VISIBLE: u16 = 66;
const BUFF_SHOP_GRANT_JUNGLE: u16 = 175;
const BUFF_SHOP_GRANT_BASE_3V3: u16 = 176;
const PERMANENT_DURATION_BITS: u16 = 0xbc00;

pub(crate) const fn non_hero_skin(template: EntityTemplate) -> Option<u32> {
    match template {
        EntityTemplate::RangedMinion | EntityTemplate::LeadMinion | EntityTemplate::TankMinion => {
            Some(EntitySpawnBody::MINION_SKIN)
        }
        EntityTemplate::VainCrystalHome3v3
        | EntityTemplate::VainCrystalAway3v3
        | EntityTemplate::VisionBeacon
        | EntityTemplate::Turret
        | EntityTemplate::VainTurret
        | EntityTemplate::NeutralJungleMinionStore
        | EntityTemplate::RiverShop5v5
        | EntityTemplate::HfBaseTurret
        | EntityTemplate::HfMiddleTurret
        | EntityTemplate::HfOuterTurret
        | EntityTemplate::HfVainTurret => Some(EntitySpawnBody::STRUCTURE_SKIN),
        EntityTemplate::JungleMinionGoldMiner
        | EntityTemplate::JungleMinionCrystalMiner
        | EntityTemplate::BlitzMiddleSentry
        | EntityTemplate::BlitzAllySentry
        | EntityTemplate::JungleShopCenter
        | EntityTemplate::BlitzJungleShopCenter
        | EntityTemplate::KrakenJungle
        | EntityTemplate::KrakenCaptured
        | EntityTemplate::HfTreant
        | EntityTemplate::HfElderTreant
        | EntityTemplate::HfBigBear
        | EntityTemplate::HfSmallBear
        | EntityTemplate::HfCrystalMiner
        | EntityTemplate::HfGoldMiner
        | EntityTemplate::HfKrakenJungle
        | EntityTemplate::HfKrakenCaptured => Some(EntitySpawnBody::JUNGLE_MINION_SKIN),
        _ => None,
    }
}

pub(crate) fn hero_slot(entity: &Entity) -> Option<u8> {
    (entity.kind == Kind::Hero && (1..=6).contains(&entity.id))
        .then(|| FIRST_HERO_SLOT - 1 + entity.id as u8)
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

/// Retains non-hero entities and the first `hero_seats` hero entities (`1` human + AI bots)
/// so 3-per-side solo/bot rooms (`6` heroes total) satisfy Fold/Blitz/ARAL team validation.
pub(crate) fn trim_to_three_per_side_roster(
    world: &mut World,
    hero_seats: usize,
) -> Result<(), String> {
    if world.tick != 0 {
        return Err("roster trim requires an unstarted world".into());
    }
    let mut kept_heroes = 0usize;
    world.entities.retain(|entity| {
        if entity.kind != Kind::Hero {
            return true;
        }
        if kept_heroes < hero_seats {
            kept_heroes += 1;
            true
        } else {
            false
        }
    });
    if kept_heroes != hero_seats {
        return Err("insufficient heroes for three-per-side roster".into());
    }
    Ok(())
}

/// Ensures the authoritative simulation layout (`HalcyonFold`, `Blitz`, `Aral` solo, or 5v5
/// dual-channel `Practice`) and native navmesh / unit data are initialized before Loading / Running.
pub(crate) fn prepare_world(room: &mut Room) -> std::io::Result<()> {
    if room.world.tick != 0 {
        return Ok(());
    }
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
    } else if room.known_queue == Some(KnownQueue::BattleRoyaleBotsSolo) {
        room.world.use_aral_lane().map_err(std::io::Error::other)?;
        if let Some(data) = &room.data {
            room.world
                .install_map_data(data)
                .map_err(std::io::Error::other)?;
        }
    } else if room.known_queue == Some(KnownQueue::Practice)
        && room.seats.iter().any(|s| s.peers[1].is_some())
        && !room.world.is_5v5_enabled()
        && room.world.tick == 0
    {
        room.world
            .use_sovereigns_rise_5v5(room.data.as_deref())
            .map_err(std::io::Error::other)?;
    }
    Ok(())
}

/// Builds the single-channel `solo_opening` (`0x046E`) batch when a 3v3 Fold, Blitz, or ARAL
/// solo-bots queue uses `ch == 0` solo control loading instead of opening a second stream.
pub(crate) fn solo_opening(room: &mut Room, seat: usize) -> std::io::Result<Option<Batch>> {
    let aral_solo = room.known_queue == Some(KnownQueue::BattleRoyaleBotsSolo);
    if !is_managed(room.known_queue) && !aral_solo {
        return Ok(None);
    }
    prepare_world(room)?;
    let team = room.world.entities[seat].team;
    let initial = room.phase == crate::RoomPhase::Loading && room.world.tick == 0;
    let mut out = vec![
        messages::scene_mode(&room.queue, 1),
        Message::known(crate::Opcode::SceneLoadedDismiss, [0]),
    ];
    out.extend(super::scene::players(room));
    out.push(messages::world_clock(room.world.tick));
    room.visible[usize::from(team)].clear();
    if !initial {
        room.solo_slots.retain_living(&room.world.entities);
    }
    if is_managed(room.known_queue) {
        let view_team = (!initial).then_some(team);
        append_opening(room, view_team, true, &mut out);
        for e in &room.world.entities {
            if e.hp > 0.0 && room.world.visible(team, e) {
                room.visible[usize::from(team)].insert(e.id);
                if crate::uses_visibility_toggle(team, e) {
                    out.push(crate::actor_visibility_message(e.id, team, true));
                }
            }
        }
        out.extend(room.recommended_paths(Some(team)));
    } else {
        out.extend(super::aral_spawn::scenery());
        for i in 0..room.world.entities.len() {
            let e = &room.world.entities[i];
            if e.hp <= 0.0 {
                continue;
            }
            let visible = room.world.visible(team, e);
            let show = if initial {
                matches!(e.kind, Kind::Hero | Kind::Crystal | Kind::Tower) || visible
            } else {
                visible
            };
            if !show {
                continue;
            }
            if let Some(msg) = super::aral_spawn::structure(&room.world, e) {
                out.push(msg);
            } else if e.kind == Kind::Hero {
                let Some(slot) = point_move::aral_slot(e) else {
                    continue;
                };
                out.push(hero_spawn(room, e, slot, true));
                if initial {
                    super::native_cooldown::opening(
                        &mut out,
                        e,
                        halcyon_protocol::ce::NativeMode::Aral,
                    );
                }
                messages::append_inventory(&mut out, &room.world, e);
            } else if let Some(slot) = room.solo_slots.alloc_dynamic(e.id)
                && let Some(msg) = entity_spawn(slot, e)
            {
                out.push(msg);
            }
            if visible {
                room.visible[usize::from(team)].insert(e.id);
                if crate::uses_visibility_toggle(team, e) {
                    out.push(crate::actor_visibility_message(e.id, team, true));
                }
            }
        }
        out.extend(super::scene::mode_variables::Opening::Aral.messages());
        out.extend(super::aral::talent_snapshot(room));
        out.extend(room.recommended_paths(Some(team)));
    }
    Ok(Some(out))
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
    let mut body = EntitySpawnBody::new(
        u32::from(entity.type_id),
        EntitySpawnBody::STRUCTURE_SKIN,
        u32::from(messages::wire_id(entity.id)),
        slot,
        entity.team + 1,
    );
    body.position = [entity.position.x, height, entity.position.y];
    body.forward = forward;
    body.hp = entity.hp;
    body.max_hp = entity.max_hp;
    if entity.kind == Kind::Tower {
        body.word64 = 1.0;
    }
    body.word84 = profile_word84;
    body.set_team_visibility(1, EntitySpawnBody::VISIBILITY_ACTIVE_MASK);
    Some(body.encode())
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
        let team_index = if slot == JUNGLE_SHOP_SLOT {
            0
        } else {
            slot / 8 + 1
        };
        let skin = non_hero_skin(template).unwrap_or(EntitySpawnBody::STRUCTURE_SKIN);
        let mut body = EntitySpawnBody::new(
            u32::from(template.as_raw()),
            skin,
            SCENERY_ACTOR_BASE + u32::from(slot),
            slot,
            team_index,
        );
        body.position = position;
        body.forward = direction;
        body.hp = hp;
        body.max_hp = hp;
        body.word84 = 12.0;
        let mask_fill = if matches!(
            template,
            EntityTemplate::NeutralJungleMinionStore
                | EntityTemplate::JungleShopCenter
                | EntityTemplate::BlitzJungleShopCenter
        ) {
            body.radius = 2.5;
            body.word64 = 1.0;
            3
        } else {
            0
        };
        body.set_team_visibility(mask_fill, EntitySpawnBody::VISIBILITY_ACTIVE_MASK);
        out.push(body.encode());
    }
    out
}

/// Opening shopkeeper access, invulnerability, and visibility buffs for base shops (`2000`, `2008`)
/// and center Jungle Shop (`2016`).
pub(crate) fn shopkeeper_opening_buffs() -> Vec<Message> {
    const SPECS: [(u32, u32, [u16; 3]); 3] = [
        (
            2000,
            2200,
            [
                BUFF_SHOP_GRANT_BASE_3V3,
                BUFF_INVULNERABLE,
                BUFF_GLOBALLY_VISIBLE,
            ],
        ),
        (
            2008,
            2208,
            [
                BUFF_SHOP_GRANT_BASE_3V3,
                BUFF_INVULNERABLE,
                BUFF_GLOBALLY_VISIBLE,
            ],
        ),
        (
            2016,
            2216,
            [
                BUFF_SHOP_GRANT_JUNGLE,
                BUFF_INVULNERABLE,
                BUFF_GLOBALLY_VISIBLE,
            ],
        ),
    ];
    let mut out = Vec::with_capacity(9);
    for (actor, first_instance, defs) in SPECS {
        for (offset, definition) in defs.into_iter().enumerate() {
            out.push(
                BuffApplication {
                    prefix: BuffPrefix {
                        target: actor,
                        source: actor,
                        duration_bits: PERMANENT_DURATION_BITS,
                        instance: first_instance + offset as u32,
                        definition,
                    },
                    extras: None,
                    suffix: &[],
                }
                .to_message()
                .expect("verified 3v3/Blitz shopkeeper buff"),
            );
        }
    }
    out
}

/// Builds a compact 122-byte `EntitySpawn` (`0x03F2`) frame with verified CFF0 skin hashes for
/// every live non-hero unit (minions, jungle camps, miners, Blitz sentries, captured Kraken).
pub(crate) fn entity_spawn(slot: u8, entity: &Entity) -> Option<Message> {
    if entity.kind == Kind::Hero
        || !(FIRST_DYNAMIC_SLOT..ACTOR_SLOT_LIMIT).contains(&slot)
        || !(1..=u32::from(u16::MAX) - 1500).contains(&entity.id)
        || !entity.position.x.is_finite()
        || !entity.position.y.is_finite()
        || !entity.hp.is_finite()
        || !entity.max_hp.is_finite()
        || entity.hp <= 0.0
        || entity.max_hp <= 0.0
    {
        return None;
    }
    let template = EntityTemplate::from_raw(entity.type_id)?;
    let skin = non_hero_skin(template)?;
    let hp = entity.hp.min(entity.max_hp);
    let forward = match entity.team {
        0 => [1.0, 0.0, 0.0],
        1 => [-1.0, 0.0, 0.0],
        _ => [0.0, 0.0, 1.0],
    };
    let radius = if entity.kind == Kind::Minion {
        3.5
    } else {
        3.6
    };
    let team_index = if entity.team < 2 { entity.team + 1 } else { 0 };
    let mut body = EntitySpawnBody::new(
        u32::from(template.as_raw()),
        skin,
        u32::from(messages::wire_id(entity.id)),
        slot,
        team_index,
    );
    body.position = [
        entity.position.x,
        f32::from_bits(EntitySpawnBody::GROUND_HEIGHT_BITS),
        entity.position.y,
    ];
    body.forward = forward;
    body.hp = hp;
    body.max_hp = entity.max_hp;
    body.radius = radius;
    body.word64 = 1.0;
    body.word84 = 1.0;
    body.lane_index = if entity.team < 2 && entity.kind == Kind::Minion {
        1 + 3 * entity.team
    } else {
        EntitySpawnBody::NO_LANE_INDEX
    };
    body.set_team_visibility(0, EntitySpawnBody::VISIBILITY_ACTIVE_MASK);
    Some(body.encode())
}

fn hero_spawn(room: &Room, entity: &Entity, slot: u8, solo: bool) -> Message {
    let mut decl = point_move::full_spawn(
        &room.world,
        entity,
        &room.picks,
        room.data.as_deref(),
        false,
    );
    if solo {
        messages::solo_hero_profile(&mut decl, entity, room.world.tick);
    }
    let mut body =
        HeroSpawnBody::read_body(&decl.body).expect("full_spawn produces 750B HeroSpawn");
    body.slot = slot;
    body.encode()
}

fn append_opening_with_slot(
    room: &Room,
    team: Option<u8>,
    solo: bool,
    batch: &mut Batch,
    mut alloc_dynamic: impl FnMut(u32) -> Option<u8>,
) {
    let blitz = is_blitz(room.known_queue);
    batch.extend(scenery(blitz));
    batch.extend(shopkeeper_opening_buffs());
    for e in &room.world.entities {
        if e.hp <= 0.0 || team.is_some_and(|side| !room.world.visible(side, e)) {
            continue;
        }
        if let Some(structure) = structure_spawn(&room.world, e) {
            batch.push(structure);
            continue;
        }
        if e.kind == Kind::Hero {
            if let Some(slot) = hero_slot(e) {
                batch.push(hero_spawn(room, e, slot, solo));
                messages::append_inventory(batch, &room.world, e);
            }
            continue;
        }
        if let Some(slot) = alloc_dynamic(e.id)
            && let Some(msg) = entity_spawn(slot, e)
        {
            batch.push(msg);
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

/// Appends opening declarations into `room.solo_slots` for connected solo and PvP peers.
pub(crate) fn append_opening(room: &mut Room, team: Option<u8>, solo: bool, batch: &mut Batch) {
    let mut allocated = Vec::new();
    for i in 0..room.world.entities.len() {
        let e = &room.world.entities[i];
        if e.hp <= 0.0 || team.is_some_and(|side| !room.world.visible(side, e)) {
            continue;
        }
        if !matches!(e.kind, Kind::Hero | Kind::Crystal | Kind::Tower)
            && let Some(slot) = room.solo_slots.alloc_dynamic(e.id)
        {
            allocated.push((e.id, slot));
        }
    }
    append_opening_with_slot(room, team, solo, batch, |id| {
        allocated
            .iter()
            .find_map(|&(eid, slot)| (eid == id).then_some(slot))
    });
}

/// Read-only opening snapshot variant used by observers and read-only view callers.
pub(crate) fn append_opening_snapshot(
    room: &Room,
    team: Option<u8>,
    solo: bool,
    batch: &mut Batch,
) {
    let mut next_fallback = FIRST_DYNAMIC_SLOT;
    append_opening_with_slot(room, team, solo, batch, |id| {
        room.solo_slots.assigned_dynamic(id).or_else(|| {
            while next_fallback < ACTOR_SLOT_LIMIT {
                let candidate = next_fallback;
                next_fallback += 1;
                if room
                    .world
                    .entities
                    .iter()
                    .all(|other| room.solo_slots.assigned_dynamic(other.id) != Some(candidate))
                {
                    return Some(candidate);
                }
            }
            None
        })
    });
}

/// Allocates a single live declaration (`0x03F3` for heroes, `0x03F2` for everything else)
/// on 3v3 Halcyon Fold and Blitz rooms. Never emits `0x03F3` for non-hero actors.
pub(crate) fn allocate_declaration(room: &mut Room, index: usize, solo: bool) -> Option<Message> {
    let e = &room.world.entities[index];
    if e.hp <= 0.0 {
        return None;
    }
    if let Some(structure) = structure_spawn(&room.world, e) {
        return Some(structure);
    }
    if e.kind == Kind::Hero {
        return hero_slot(e).map(|slot| hero_spawn(room, e, slot, solo));
    }
    let slot = room.solo_slots.alloc_dynamic(e.id)?;
    entity_spawn(slot, e)
}

/// Allocates a single live declaration on `BattleRoyaleBotsSolo` (`use_aral_lane`), keeping all
/// structures and minions on `0x03F2` and heroes on `0x03F3` slots `16..=21`.
pub(crate) fn allocate_aral_solo_declaration(room: &mut Room, index: usize) -> Option<Message> {
    let e = &room.world.entities[index];
    if e.hp <= 0.0 {
        return None;
    }
    if let Some(structure) = super::aral_spawn::structure(&room.world, e) {
        return Some(structure);
    }
    if e.kind == Kind::Hero {
        return point_move::aral_slot(e).map(|slot| hero_spawn(room, e, slot, true));
    }
    let slot = room.solo_slots.alloc_dynamic(e.id)?;
    entity_spawn(slot, e)
}

/// Keeps `Buff_Shop_CanShopIcon` (`173`) and `Buff_Shop_AllowStorePurchase` (`174`, operand `452`
/// for 3v3 HF or `453` for Blitz) synchronized across human seats on both `ch == 0` solo and
/// `ch == 1` PvP connections.
pub(crate) fn sync_shops(room: &mut Room) {
    if !is_managed(room.known_queue) {
        return;
    }
    let store_operand = if is_blitz(room.known_queue) {
        SoloStructure::STORE_BLITZ_OPERAND
    } else {
        SoloStructure::STORE_3V3_OPERAND
    };
    for seat_idx in 0..room.seats.len() {
        let peer_ch = if room.seats[seat_idx].peers[1].is_some() {
            Some(1usize)
        } else if room.seats[seat_idx].peers[0].is_some()
            && room.seats[seat_idx].world_actor_written
            && room.seats[seat_idx].preamble.confirmed()
        {
            Some(0usize)
        } else {
            None
        };
        let Some(ch) = peer_ch else {
            continue;
        };
        let Some(actor_id) = room
            .world
            .entities
            .get(seat_idx)
            .filter(|e| e.kind == Kind::Hero)
            .map(|e| e.id)
        else {
            continue;
        };
        let mut batch = Vec::new();
        let wire_actor = u32::from(messages::wire_id(actor_id));
        if !room.scene_views[seat_idx].shop_can_icon_sent {
            batch.push(SoloStructure::shop_can_icon_message(
                wire_actor,
                0x10_0000 + seat_idx as u32,
            ));
            room.scene_views[seat_idx].shop_can_icon_sent = true;
        }
        if let Some(keeper_actor) = room.world.shop_3v3_blitz_keeper_for_actor(actor_id) {
            let due = room.scene_views[seat_idx].shop_allow_next_tick;
            if due.is_none_or(|tick| room.world.tick >= tick) {
                let seq = room.scene_views[seat_idx].shop_allow_seq;
                room.scene_views[seat_idx].shop_allow_seq = seq.wrapping_add(1);
                let instance = 0x20_0000 + (seat_idx as u32) * 0x1_0000 + (seq & 0xFFFF);
                batch.push(SoloStructure::shop_allow_purchase_for_store(
                    wire_actor,
                    keeper_actor,
                    instance,
                    store_operand,
                ));
                room.scene_views[seat_idx].shop_allow_active_instance = Some(instance);
                room.scene_views[seat_idx].shop_allow_next_tick =
                    Some(room.world.tick + halcyon_sim::TICK_HZ);
            }
        } else if let Some(active_instance) =
            room.scene_views[seat_idx].shop_allow_active_instance.take()
        {
            room.scene_views[seat_idx].shop_allow_next_tick = None;
            if let Ok(cancel) = (BuffMutation {
                target: wire_actor,
                instance: active_instance,
                change: BuffChange::Cancel,
                padding: &[],
            })
            .to_message()
            {
                batch.push(cancel);
            }
        }
        if !batch.is_empty()
            && let Some(peer) = &room.seats[seat_idx].peers[ch]
        {
            peer.send(transport::Delivery::Private(batch));
        }
    }
}

pub(crate) fn mode_var_message(key: u32, word: u32) -> Message {
    halcyon_protocol::ModifyGameModeVarBody { key, value: word }.encode_padded()
}

pub(crate) fn blitz_mode_var_snapshot(world: &World) -> Vec<Message> {
    let Some(blitz) = world.blitz_state() else {
        return Vec::new();
    };
    let duration_seconds = (BLITZ_DURATION_TICKS / TICK_HZ) as u32;
    vec![
        mode_var_message(COUNTDOWN_START_KEY, 0),
        mode_var_message(COUNTDOWN_DURATION_KEY, duration_seconds),
        mode_var_message(COUNTDOWN_PAUSED_KEY, u32::from(blitz.sudden_death)),
        mode_var_message(COUNTDOWN_DO_ROUNDING_KEY, 0),
        mode_var_message(TEAM_1_SCORE_KEY, blitz.team_points[0]),
        mode_var_message(TEAM_2_SCORE_KEY, blitz.team_points[1]),
    ]
}

/// Emits live `0x041F` team score / pause updates and `0x0440` Blitz score/gameover flyouts.
pub(crate) fn publish_tick(room: &mut Room, events: &[Event]) {
    if !is_blitz(room.known_queue) || room.world.blitz_state().is_none() {
        return;
    }
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
                    TEAM_1_SCORE_KEY
                } else {
                    TEAM_2_SCORE_KEY
                };
                frames.push(mode_var_message(key, total));
            }
            Event::BlitzSuddenDeath => {
                frames.push(mode_var_message(COUNTDOWN_PAUSED_KEY, 1));
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
    let Some(level) = super::authorize_talent_level(room, seat, hero.type_id, selection) else {
        return Vec::new();
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

#[cfg(test)]
#[path = "modes_3v3_blitz_tests.rs"]
mod tests;
