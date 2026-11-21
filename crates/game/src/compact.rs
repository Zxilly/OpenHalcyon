use halcyon_data::EntityTemplate;
use halcyon_protocol::{EntitySpawnBody, EntityStatMask, Message};
use halcyon_sim::{Entity, Kind};

use super::messages::wire_id;

/// Fixed native Solo structure slots. Towers on different lanes share templates.
pub(super) fn structure_slot(e: &Entity) -> Option<u8> {
    let fallback = match (e.kind, EntityTemplate::from_raw(e.type_id)?, e.team) {
        (Kind::Armory, EntityTemplate::VainNode, _) => None,
        (Kind::Crystal, EntityTemplate::VainCrystalHome5v5, 0) => Some(6),
        (Kind::Crystal, EntityTemplate::VainCrystalAway5v5, 1) => Some(7),
        (Kind::Tower, EntityTemplate::OuterTurret5v5, 0) => Some(9),
        (Kind::Tower, EntityTemplate::OuterTurret5v5, 1) => Some(12),
        (Kind::Tower, EntityTemplate::Turret5v5, 0) => Some(26),
        (Kind::Tower, EntityTemplate::ArmoryTurret5v5, 0) => Some(27),
        (Kind::Tower, EntityTemplate::Turret5v5, 1) => Some(32),
        (Kind::Tower, EntityTemplate::ArmoryTurret5v5, 1) => Some(33),
        _ => return None,
    };
    e.id.checked_add(1499)
        .and_then(halcyon_protocol::SoloStructure::at_actor)
        .filter(|p| p.template() == e.type_id && p.team_index() == e.team)
        .map(|p| p.slot())
        .or(fallback)
}

/// All 26 Solo structures (crystals, turrets, and armories) retain their compact
/// `0x03F2` snapshot profile across opening and mid-game visibility/rejoin.
pub(super) fn structure(tick: u64, e: &Entity) -> Option<Message> {
    if !(1..=u32::from(u16::MAX) - 1500).contains(&e.id)
        || !e.hp.is_finite()
        || !e.max_hp.is_finite()
        || e.hp <= 0.
        || e.hp > e.max_hp
        || tick == 0 && e.hp != e.max_hp
    {
        return None;
    }
    let profile = halcyon_protocol::SoloStructure::at_slot(structure_slot(e)?)?;
    let mut body = profile.spawn_body(u32::from(wire_id(e.id)));
    if body.position[0].to_bits() != e.position.x.to_bits()
        || body.position[2].to_bits() != e.position.y.to_bits()
    {
        return None;
    }
    body.hp = e.hp;
    body.max_hp = e.max_hp;
    Some(body.encode())
}

pub(super) fn recordable_structure(body: &[u8]) -> bool {
    let Ok(spawn) = EntitySpawnBody::read_body(body) else {
        return false;
    };
    let Some(profile) = halcyon_protocol::SoloStructure::at_slot(spawn.slot) else {
        return false;
    };
    if !spawn.hp.is_finite()
        || !spawn.max_hp.is_finite()
        || spawn.hp <= 0.
        || spawn.hp > spawn.max_hp
    {
        return false;
    }
    let mut expected = profile.spawn_body(profile.actor());
    expected.hp = spawn.hp;
    expected.max_hp = spawn.max_hp;
    spawn.bit_eq(&expected)
}

/// Fresh births use the native template defaults at the measured route anchor.
fn pristine_minion_body(
    template: u32,
    actor: u32,
    slot: u8,
    team: u8,
    side: i8,
) -> EntitySpawnBody {
    let x = if side == 0 { 70f32 } else { 75f32 };
    let mut body = EntitySpawnBody::new(
        template,
        EntitySpawnBody::MINION_SKIN,
        actor,
        slot,
        team + 1,
    );
    body.position = [
        if team == 0 { -x } else { x },
        f32::from_bits(EntitySpawnBody::GROUND_HEIGHT_BITS),
        5. * f32::from(side),
    ];
    body.forward = [0.0, 0.0, 1.0];
    body.visibility_required = EntitySpawnBody::VISIBILITY_ALL_REQUIRED;
    body.visibility_mask[1 + usize::from(team)] = EntitySpawnBody::VISIBILITY_PRISTINE_MASK;
    body.set_stat_mask(EntityStatMask::Pristine);
    body.lane_index = (3 * team as i8 + 1 + side) as u8;
    body
}

pub(super) fn camp_unit(entity: &Entity, slot: u8) -> Option<Message> {
    let template = EntityTemplate::from_raw(entity.type_id)?;
    if entity.kind != Kind::Neutral || !template.is_solo_camp_unit() {
        return None;
    }
    let mut body = EntitySpawnBody::new(
        u32::from(template.as_raw()),
        EntitySpawnBody::JUNGLE_MINION_SKIN,
        u32::from(wire_id(entity.id)),
        slot,
        0,
    );
    body.position = [
        entity.position.x,
        f32::from_bits(EntitySpawnBody::GROUND_HEIGHT_BITS),
        entity.position.y,
    ];
    body.forward = [0.0, 0.0, 1.0];
    body.hp = entity.hp;
    body.max_hp = entity.max_hp;
    body.radius = 3.6;
    body.word64 = 1.0;
    body.word84 = 1.0;
    body.set_team_visibility(0, EntitySpawnBody::VISIBILITY_ACTIVE_MASK);
    is_recordable_camp_unit(&body).then(|| body.encode())
}

fn is_recordable_camp_unit(spawn: &EntitySpawnBody) -> bool {
    (52..=199).contains(&spawn.slot)
        && spawn.flag117 == 0
        && spawn.flag118 == 0
        && spawn.level == 1
        && spawn.lane_index == EntitySpawnBody::NO_LANE_INDEX
        && spawn.team_index == 0
        && u16::try_from(spawn.template)
            .ok()
            .and_then(EntityTemplate::from_raw)
            .is_some_and(EntityTemplate::is_solo_camp_unit)
        && spawn.skin == EntitySpawnBody::JUNGLE_MINION_SKIN
        && (2122..65535).contains(&spawn.actor)
        && spawn.position[0].is_finite()
        && spawn.position[2].is_finite()
        && spawn.hp.is_finite()
        && spawn.max_hp.is_finite()
        && spawn.hp > 0.
        && spawn.hp <= spawn.max_hp
        && spawn.visibility_required == EntitySpawnBody::VISIBILITY_ALL_REQUIRED
        && spawn.visibility_mask == [15, 0, 0, 0, 0, 0, 0, 0]
        && spawn.visibility_aux == [0; 8]
        && spawn.word112 == u32::MAX
        && spawn.position[1].to_bits() == EntitySpawnBody::GROUND_HEIGHT_BITS
        && spawn.forward[0].to_bits() == 0
        && spawn.forward[1].to_bits() == 0
        && spawn.forward[2].to_bits() == 1_f32.to_bits()
        && spawn.radius.to_bits() == 3.6_f32.to_bits()
        && spawn.word64.to_bits() == 1_f32.to_bits()
        && spawn.word84.to_bits() == 1_f32.to_bits()
        && spawn.reserved_44 == [0; 8]
        && spawn.reserved_56 == [0; 8]
        && spawn.reserved_68 == [0; 16]
}

pub(super) fn recordable_camp_unit(body: &[u8]) -> bool {
    EntitySpawnBody::read_body(body)
        .ok()
        .is_some_and(|spawn| is_recordable_camp_unit(&spawn))
}

pub(super) fn recordable_minion_birth(b: &[u8]) -> bool {
    let Ok(spawn) = EntitySpawnBody::read_body(b) else {
        return false;
    };
    if !(52..200).contains(&spawn.slot) || !(1..=2).contains(&spawn.team_index) {
        return false;
    }
    let side = i16::from(spawn.lane_index) - 3 * i16::from(spawn.team_index - 1) - 1;
    (-1..=1).contains(&side)
        && u16::try_from(spawn.template)
            .ok()
            .and_then(EntityTemplate::from_raw)
            .is_some_and(EntityTemplate::is_solo_lane_minion)
        && (2122..65535).contains(&spawn.actor)
        && spawn.bit_eq(&pristine_minion_body(
            spawn.template,
            spawn.actor,
            spawn.slot,
            spawn.team_index - 1,
            side as i8,
        ))
}

/// Subsequent visibility entries and reconnects carry current numeric state.
pub(super) fn minion(entity: &Entity, slot: u8, fresh: bool) -> Option<Message> {
    if entity.kind != Kind::Minion
        || entity.team > 1
        || !(52..200).contains(&slot)
        || !(1..=u32::from(u16::MAX) - 1500).contains(&entity.id)
        || !entity.position.x.is_finite()
        || !entity.position.y.is_finite()
        || !entity.hp.is_finite()
        || !entity.max_hp.is_finite()
        || entity.hp <= 0.0
        || entity.hp > entity.max_hp
    {
        return None;
    }
    let default_hp = match EntityTemplate::from_raw(entity.type_id)? {
        EntityTemplate::MinionRange5v5 => 475.,
        EntityTemplate::MinionMelee5v5 => 575.,
        EntityTemplate::MinionSiege5v5 => 880.,
        EntityTemplate::MinionCaptain5v5 => 950.,
        _ => return None,
    };
    if fresh && entity.level == 1 && entity.hp == default_hp && entity.max_hp == default_hp {
        return Some(
            pristine_minion_body(
                u32::from(entity.type_id),
                u32::from(wire_id(entity.id)),
                slot,
                entity.team,
                entity.solo_side,
            )
            .encode(),
        );
    }
    let forward_x = if entity.team == 0 { 1.0_f32 } else { -1.0 };
    let mut body = EntitySpawnBody::new(
        u32::from(entity.type_id),
        EntitySpawnBody::MINION_SKIN,
        u32::from(wire_id(entity.id)),
        slot,
        entity.team + 1,
    );
    body.position = [
        entity.position.x,
        f32::from_bits(EntitySpawnBody::GROUND_HEIGHT_BITS),
        entity.position.y,
    ];
    body.forward = [forward_x, 0.0, 0.0];
    body.hp = entity.hp;
    body.max_hp = entity.max_hp;
    body.radius = 3.5;
    body.word64 = 1.0;
    body.word84 = 1.0;
    body.lane_index = (3 * entity.team as i8 + 1 + entity.solo_side) as u8;
    body.set_team_visibility(0, EntitySpawnBody::VISIBILITY_ACTIVE_MASK);
    Some(body.encode())
}

#[cfg(test)]
mod tests {
    use halcyon_sim::World;

    use super::*;

    fn word(body: &[u8], offset: usize) -> u32 {
        u32::from_be_bytes(body[offset..offset + 4].try_into().unwrap())
    }

    #[test]
    #[ignore = "requires the local base delivery and supplemental map assets"]
    fn solo_camp_snapshot_uses_current_health_neutral_slot_and_recorder_shape() {
        let base = std::env::var_os("HALCYON_DELIVERY").unwrap();
        let extra = std::path::PathBuf::from(std::env::var_os("HALCYON_DATA_EXTRA").unwrap());
        let data = halcyon_data::GameData::load_with_extra(base, Some(&extra)).unwrap();
        let mut world = World::from_data(0, 10, &data).unwrap();
        world.use_solo_mid_lane().unwrap();
        world
            .install_native_navigation(data.native_maps().unwrap())
            .unwrap();
        world.install_native_camps(&data).unwrap();
        for _ in 0..786 {
            world.step();
        }
        let i = world
            .entities
            .iter()
            .position(|e| e.kind == Kind::Neutral)
            .unwrap();
        world.entities[i].hp -= 20.;
        let e = &world.entities[i];
        let mut slots = crate::actor_slots::ActorSlots::from_roster(&world.entities);
        let frame = slots.declaration(&world, e, &[], Some(&data)).unwrap();
        let body = &frame.body;
        assert!(recordable_camp_unit(body));
        assert_eq!(word(body, 8), u32::from(wire_id(e.id)));
        assert_eq!(word(body, 36), e.hp.to_bits());
        assert_eq!(word(body, 40), e.max_hp.to_bits());
        assert_eq!(&body[117..], &[0, 0, 1, 255, 0]);
        assert_eq!(
            slots.declaration(&world, e, &[], Some(&data)).unwrap().body,
            *body
        );
    }

    #[test]
    fn initial_slots_encode_only_simulated_structures() {
        let mut world = World::new(0, 1);
        world.use_solo_mid_lane().unwrap();
        let mut slots = Vec::new();
        for entity in &world.entities {
            let Some(message) = structure(world.tick, entity) else {
                assert!(matches!(entity.kind, Kind::Hero | Kind::Minion));
                continue;
            };
            let b = &message.body;
            assert_eq!(
                (message.opcode, b.len()),
                (crate::wire_opcode::EntitySpawn, 122)
            );
            assert_eq!(word(b, 0), u32::from(entity.type_id));
            assert_eq!(
                word(b, 4),
                if entity.kind == Kind::Armory {
                    0x4a2eed42
                } else {
                    0xc10b41da
                }
            );
            assert_eq!(word(b, 8), u32::from(wire_id(entity.id)));
            assert_eq!(word(b, 12), entity.position.x.to_bits());
            assert_eq!(word(b, 20), entity.position.y.to_bits());
            assert_eq!(
                word(b, 24),
                word(
                    &halcyon_protocol::SoloStructure::at_slot(b[116])
                        .unwrap()
                        .message(word(b, 8))
                        .body,
                    24
                )
            );
            assert_eq!(word(b, 36), entity.hp.to_bits());
            assert_eq!(word(b, 40), entity.max_hp.to_bits());
            assert_eq!(b[117..122], [0, 0, 1, 255, entity.team + 1]);
            assert!(recordable_structure(b));
            slots.push(b[116]);
        }
        slots.sort_unstable();
        let expected: Vec<_> = (0..14).chain(24..36).collect();
        assert_eq!(slots, expected);
    }

    #[test]
    fn recording_accepts_only_the_registered_mid_lane_opening_profile() {
        let encoded = halcyon_protocol::SoloStructure::at_slot(27)
            .unwrap()
            .message(2065);
        assert!(recordable_structure(&encoded.body));
        for offset in [0, 4, 8, 16, 24, 28, 64, 97, 116, 121] {
            let mut other = encoded.body.clone();
            other[offset] ^= 1;
            assert!(!recordable_structure(&other), "changed byte {offset}");
        }
        let side_lane = halcyon_protocol::SoloStructure::at_slot(25)
            .unwrap()
            .message(2051);
        assert!(recordable_structure(&side_lane.body));
    }

    #[test]
    fn later_crystal_and_tower_use_compact_current_health_snapshot() {
        let mut world = World::new(0, 1);
        world.use_solo_mid_lane().unwrap();
        let mut crystal = world
            .entities
            .iter()
            .find(|e| e.type_id == 295)
            .unwrap()
            .clone();
        crystal.hp -= 375.0;
        let resumed = structure(1800, &crystal).unwrap();
        assert_eq!(word(&resumed.body, 36), crystal.hp.to_bits());
        assert_eq!(word(&resumed.body, 40), crystal.max_hp.to_bits());
        assert!(recordable_structure(&resumed.body));
        let mut tower = world
            .entities
            .iter()
            .find(|e| e.id == 2011 - 1499)
            .unwrap()
            .clone();
        tower.hp -= 412.5;
        let late_tower = structure(1800, &tower).unwrap();
        assert_eq!(late_tower.opcode, crate::wire_opcode::EntitySpawn);
        assert_eq!(word(&late_tower.body, 0), 340);
        assert_eq!(word(&late_tower.body, 8), 2011);
        assert_eq!(word(&late_tower.body, 36), tower.hp.to_bits());
        assert_eq!(word(&late_tower.body, 40), tower.max_hp.to_bits());
        assert_eq!(late_tower.body[116], 11);
        assert!(recordable_structure(&late_tower.body));
        crystal.hp = crystal.max_hp;
        crystal.team = 1;
        assert!(structure(0, &crystal).is_none());
        crystal.team = 0;
        crystal.id = u32::MAX;
        assert!(structure(0, &crystal).is_none());
    }
}
