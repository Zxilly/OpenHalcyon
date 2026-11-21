//! Scene declarations and `0x046E` solo opening snapshot assembly for 3v3 Halcyon Fold,
//! `Blitz`, and `BattleRoyaleBotsSolo`.

use super::setup::{
    ACTOR_SLOT_LIMIT, BUFF_GLOBALLY_VISIBLE, BUFF_INVULNERABLE, BUFF_SHOP_GRANT_BASE_3V3,
    BUFF_SHOP_GRANT_JUNGLE, FIRST_DYNAMIC_SLOT, JUNGLE_SHOP_SLOT, PERMANENT_DURATION_BITS,
    SCENERY_ACTOR_BASE, hero_slot, is_blitz, is_halcyon_fold, is_managed, non_hero_skin,
    prepare_world,
};
use super::tick_sync::blitz_mode_var_snapshot;
use crate::abilities::cooldowns as native_cooldown;
use crate::modes::{aral, scene};
use crate::replication::{
    Batch, actor_visibility_message, messages, slots::point_move, uses_visibility_toggle,
};
use crate::{Opcode, Room, RoomPhase};
use halcyon_data::EntityTemplate;
use halcyon_protocol::{
    BuffApplication, BuffPrefix, EntitySpawnBody, HeroSpawnBody, Message, ce::KnownQueue,
};
use halcyon_sim::{Entity, Kind, World};

/// Builds the single-channel `solo_opening` (`0x046E`) batch when a 3v3 Fold, Blitz, or ARAL
/// solo-bots queue uses `ch == 0` solo control loading instead of opening a second stream.
pub(crate) fn solo_opening(room: &mut Room, seat: usize) -> std::io::Result<Option<Batch>> {
    let aral_solo = room.known_queue == Some(KnownQueue::BattleRoyaleBotsSolo);
    if !is_managed(room.known_queue) && !aral_solo {
        return Ok(None);
    }
    prepare_world(room)?;
    let team = room.world.entities[seat].team;
    let initial = room.phase == RoomPhase::Loading && room.world.tick == 0;
    let mut out = vec![
        messages::scene_mode(&room.queue, 1),
        Message::known(Opcode::SceneLoadedDismiss, [0]),
    ];
    out.extend(scene::players(room));
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
                if uses_visibility_toggle(team, e) {
                    out.push(actor_visibility_message(e.id, team, true));
                }
            }
        }
        out.extend(room.recommended_paths(Some(team)));
    } else {
        out.extend(aral::spawn::scenery());
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
            if let Some(msg) = aral::spawn::structure(&room.world, e) {
                out.push(msg);
            } else if e.kind == Kind::Hero {
                let Some(slot) = point_move::aral_slot(e) else {
                    continue;
                };
                out.push(hero_spawn(room, e, slot, true));
                if initial {
                    native_cooldown::opening(&mut out, e, halcyon_protocol::ce::NativeMode::Aral);
                }
                messages::append_inventory(&mut out, &room.world, e);
            } else if let Some(slot) = room.solo_slots.alloc_dynamic(e.id)
                && let Some(msg) = entity_spawn(slot, e)
            {
                out.push(msg);
            }
            if visible {
                room.visible[usize::from(team)].insert(e.id);
                if uses_visibility_toggle(team, e) {
                    out.push(actor_visibility_message(e.id, team, true));
                }
            }
        }
        out.extend(scene::mode_variables::Opening::Aral.messages());
        out.extend(aral::talent_snapshot(room));
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
        batch.extend(scene::mode_variables::Opening::Blitz.messages());
        batch.extend(blitz_mode_var_snapshot(&room.world));
        batch.extend(aral::talent_snapshot(room));
    } else if is_halcyon_fold(room.known_queue) {
        batch.extend(scene::mode_variables::Opening::HalcyonFold.messages());
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
    if let Some(structure) = aral::spawn::structure(&room.world, e) {
        return Some(structure);
    }
    if e.kind == Kind::Hero {
        return point_move::aral_slot(e).map(|slot| hero_spawn(room, e, slot, true));
    }
    let slot = room.solo_slots.alloc_dynamic(e.id)?;
    entity_spawn(slot, e)
}
