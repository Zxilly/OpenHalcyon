use halcyon_data::EntityTemplate;
use halcyon_protocol::Message;
use halcyon_sim::{Entity, Kind};

use super::messages::wire_id;

const BODY_LEN: usize = 122;
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

/// Towers acquire dynamic pose and timer fields after the first tick.
/// All fourteen crystal snapshots retain the compact profile.
pub(super) fn structure(tick: u64, e: &Entity) -> Option<Message> {
    if (matches!(e.kind, Kind::Tower | Kind::Armory) && tick != 0)
        || !(1..=u32::from(u16::MAX) - 1500).contains(&e.id)
        || !e.hp.is_finite()
        || !e.max_hp.is_finite()
        || e.hp <= 0.
        || e.hp > e.max_hp
        || tick == 0 && e.hp != e.max_hp
    {
        return None;
    }
    let profile = halcyon_protocol::SoloStructure::at_slot(structure_slot(e)?)?;
    let mut msg = profile.message(u32::from(wire_id(e.id)));
    if msg.body[12..16] != e.position.x.to_be_bytes()
        || msg.body[20..24] != e.position.y.to_be_bytes()
    {
        return None;
    }
    msg.body[36..40].copy_from_slice(&e.hp.to_be_bytes());
    msg.body[40..44].copy_from_slice(&e.max_hp.to_be_bytes());
    Some(msg)
}

pub(super) fn recordable_structure(body: &[u8]) -> bool {
    if body.len() != BODY_LEN {
        return false;
    }
    let Some(profile) = halcyon_protocol::SoloStructure::at_slot(body[116]) else {
        return false;
    };
    let hp = f32::from_be_bytes(body[36..40].try_into().unwrap());
    let max = f32::from_be_bytes(body[40..44].try_into().unwrap());
    if !hp.is_finite()
        || !max.is_finite()
        || hp <= 0.
        || hp > max
        || !matches!(body[116], 6 | 7) && hp != max
    {
        return false;
    }
    let expected = profile.message(profile.actor());
    body[..36] == expected.body[..36] && body[44..] == expected.body[44..]
}

/// Fresh births use the native template defaults at the measured route anchor.
fn pristine_minion(template: u32, actor: u32, slot: u8, team: u8, side: i8) -> Message {
    let mut b = [0; BODY_LEN];
    put_word(&mut b, 0, template);
    put_word(&mut b, 4, 0xeb39ce55);
    put_word(&mut b, 8, actor);
    let x = if side == 0 { 70f32 } else { 75f32 };
    put_word(&mut b, 12, if team == 0 { -x } else { x }.to_bits());
    put_word(&mut b, 20, (5. * f32::from(side)).to_bits());
    put_word(&mut b, 16, 0x3d75_be29);
    put_word(&mut b, 32, 1f32.to_bits());
    b[88..96].fill(1);
    b[97 + usize::from(team)] = 1;
    put_word(&mut b, 112, u32::MAX);
    b[116..122].copy_from_slice(&[slot, 1, 0, 1, (3 * team as i8 + 1 + side) as u8, team + 1]);
    Message::known(crate::Opcode::Snapshot, b)
}

pub(super) fn camp_unit(entity: &Entity, slot: u8) -> Option<Message> {
    let template = EntityTemplate::from_raw(entity.type_id)?;
    if entity.kind != Kind::Neutral || !template.is_solo_camp_unit() {
        return None;
    }
    let mut body = [0; BODY_LEN];
    put_word(&mut body, 0, u32::from(template.as_raw()));
    put_word(&mut body, 4, 0x4dd5b7d0);
    put_word(&mut body, 8, u32::from(wire_id(entity.id)));
    for (offset, value) in [
        (12, entity.position.x),
        (16, f32::from_bits(0x3d75_be29)),
        (20, entity.position.y),
        (32, 1.),
        (36, entity.hp),
        (40, entity.max_hp),
        (52, 3.6),
        (64, 1.),
        (84, 1.),
    ] {
        put_word(&mut body, offset, value.to_bits());
    }
    body[88..96].fill(1);
    body[96] = 15;
    put_word(&mut body, 112, u32::MAX);
    body[116..122].copy_from_slice(&[slot, 0, 0, 1, 255, 0]);
    recordable_camp_unit(&body).then(|| Message::known(crate::Opcode::Snapshot, body))
}

pub(super) fn recordable_camp_unit(body: &[u8]) -> bool {
    if body.len() != BODY_LEN || !matches!(body[116..], [52..=199, 0, 0, 1, 255, 0]) {
        return false;
    }
    let word = |i| u32::from_be_bytes(body[i..i + 4].try_into().unwrap());
    let f = |i| f32::from_bits(word(i));
    u16::try_from(word(0))
        .ok()
        .and_then(EntityTemplate::from_raw)
        .is_some_and(EntityTemplate::is_solo_camp_unit)
        && word(4) == 0x4dd5b7d0
        && (2122..65535).contains(&word(8))
        && [12, 20, 36, 40].into_iter().all(|i| f(i).is_finite())
        && f(36) > 0.
        && f(36) <= f(40)
        && body[88..97] == [1, 1, 1, 1, 1, 1, 1, 1, 15]
        && body[97..112] == [0; 15]
        && word(112) == u32::MAX
        && [
            (16, 0x3d75_be29),
            (32, 1_f32.to_bits()),
            (52, 3.6_f32.to_bits()),
            (64, 1_f32.to_bits()),
            (84, 1_f32.to_bits()),
        ]
        .into_iter()
        .all(|(i, value)| word(i) == value)
        && [24, 28, 44, 48, 56, 60, 68, 72, 76, 80]
            .into_iter()
            .all(|i| word(i) == 0)
}

pub(super) fn recordable_minion_birth(b: &[u8]) -> bool {
    if b.len() != BODY_LEN || !(52..200).contains(&b[116]) || !(1..=2).contains(&b[121]) {
        return false;
    }
    let template = u32::from_be_bytes(b[..4].try_into().unwrap());
    let actor = u32::from_be_bytes(b[8..12].try_into().unwrap());
    let side = i16::from(b[120]) - 3 * i16::from(b[121] - 1) - 1;
    (-1..=1).contains(&side)
        && u16::try_from(template)
            .ok()
            .and_then(EntityTemplate::from_raw)
            .is_some_and(EntityTemplate::is_solo_lane_minion)
        && (2122..65535).contains(&actor)
        && pristine_minion(template, actor, b[116], b[121] - 1, side as i8).body == b
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
        return Some(pristine_minion(
            u32::from(entity.type_id),
            u32::from(wire_id(entity.id)),
            slot,
            entity.team,
            entity.solo_side,
        ));
    }
    let mut body = [0; BODY_LEN];
    put_word(&mut body, 0, u32::from(entity.type_id));
    put_word(&mut body, 4, 0xeb39ce55);
    put_word(&mut body, 8, u32::from(wire_id(entity.id)));
    let forward = if entity.team == 0 { 1.0_f32 } else { -1.0 };
    for (offset, value) in [
        (12, entity.position.x),
        (16, f32::from_bits(0x3d75_be29)),
        (20, entity.position.y),
        (24, forward),
        (36, entity.hp),
        (40, entity.max_hp),
        (52, 3.5),
        (64, 1.0),
        (84, 1.0),
    ] {
        put_word(&mut body, offset, value.to_bits());
    }
    body[88..96].fill(1);
    body[97 + usize::from(entity.team)] = 15;
    put_word(&mut body, 112, u32::MAX);
    body[116] = slot;
    body[119] = 1;
    body[120] = (3 * entity.team as i8 + 1 + entity.solo_side) as u8;
    body[121] = entity.team + 1;
    Some(Message::known(crate::Opcode::Snapshot, body))
}

fn put_word(body: &mut [u8], offset: usize, value: u32) {
    body[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
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
                (crate::wire_opcode::Snapshot, 122)
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
    fn later_crystal_uses_current_health_but_tower_requires_opening_state() {
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
        let tower = world
            .entities
            .iter()
            .find(|e| e.kind == Kind::Tower)
            .unwrap();
        assert!(structure(1800, tower).is_none());
        crystal.hp = crystal.max_hp;
        crystal.team = 1;
        assert!(structure(0, &crystal).is_none());
        crystal.team = 0;
        crystal.id = u32::MAX;
        assert!(structure(0, &crystal).is_none());
    }
}
