use halcyon_protocol::Message;
use halcyon_sim::{Entity, Kind};

use super::messages::wire_id;

const BODY_LEN: usize = 122;
/// Native Solo mid-lane slots. Other lanes share the template but have
/// different registrations and are not part of the authoritative layout.
pub(super) fn structure_slot(e: &Entity) -> Option<u8> {
    match (e.kind, e.type_id, e.team) {
        (Kind::Crystal, 295, 0) => Some(6),
        (Kind::Crystal, 296, 1) => Some(7),
        (Kind::Tower, 340, 0) => Some(9),
        (Kind::Tower, 340, 1) => Some(12),
        (Kind::Tower, 339, 0) => Some(26),
        (Kind::Tower, 341, 0) => Some(27),
        (Kind::Tower, 339, 1) => Some(32),
        (Kind::Tower, 341, 1) => Some(33),
        _ => None,
    }
}

/// Towers acquire dynamic pose and timer fields after the first tick.
/// All fourteen crystal snapshots retain the compact profile.
pub(super) fn structure(tick: u64, e: &Entity) -> Option<Message> {
    if (e.kind == Kind::Tower && tick != 0)
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

pub(super) fn recordable_mid_structure(body: &[u8]) -> bool {
    if body.len() != BODY_LEN {
        return false;
    }
    let wire = match body[116] {
        6 => 2006,
        7 => 2007,
        9 => 2009,
        12 => 2012,
        26 => 2060,
        27 => 2065,
        32 => 2102,
        33 => 2107,
        _ => return false,
    };
    let hp = f32::from_be_bytes(body[36..40].try_into().unwrap());
    let max = f32::from_be_bytes(body[40..44].try_into().unwrap());
    if !hp.is_finite() || !max.is_finite() || hp <= 0. || hp > max || body[116] >= 8 && hp != max {
        return false;
    }
    let expected = halcyon_protocol::SoloStructure::at_slot(body[116])
        .unwrap()
        .message(wire);
    body[..36] == expected.body[..36] && body[44..] == expected.body[44..]
}

/// Fresh births use the native template defaults at the measured route anchor.
fn pristine_minion(template: u32, actor: u32, slot: u8, team: u8) -> Message {
    let mut b = [0; BODY_LEN];
    put_word(&mut b, 0, template);
    put_word(&mut b, 4, 0xeb39ce55);
    put_word(&mut b, 8, actor);
    put_word(&mut b, 12, if team == 0 { -70f32 } else { 70f32 }.to_bits());
    put_word(&mut b, 16, 0x3d75_be29);
    put_word(&mut b, 32, 1f32.to_bits());
    b[88..96].fill(1);
    b[97 + usize::from(team)] = 1;
    put_word(&mut b, 112, u32::MAX);
    b[116..122].copy_from_slice(&[slot, 1, 0, 1, if team == 0 { 1 } else { 4 }, team + 1]);
    Message::new(0x3f2, b)
}

pub(super) fn recordable_minion_birth(b: &[u8]) -> bool {
    if b.len() != BODY_LEN || !(52..200).contains(&b[116]) || !(1..=2).contains(&b[121]) {
        return false;
    }
    let template = u32::from_be_bytes(b[..4].try_into().unwrap());
    let actor = u32::from_be_bytes(b[8..12].try_into().unwrap());
    matches!(template, 335..=338)
        && (2122..65535).contains(&actor)
        && pristine_minion(template, actor, b[116], b[121] - 1).body == b
}

/// Subsequent visibility entries and reconnects carry current numeric state.
pub(super) fn minion(entity: &Entity, slot: u8, fresh: bool) -> Option<Message> {
    if entity.kind != Kind::Minion
        || !matches!(entity.type_id, 335..=338)
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
    let default_hp = [475., 575., 880., 950.][usize::from(entity.type_id - 335)];
    if fresh && entity.level == 1 && entity.hp == default_hp && entity.max_hp == default_hp {
        return Some(pristine_minion(
            u32::from(entity.type_id),
            u32::from(wire_id(entity.id)),
            slot,
            entity.team,
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
    body[120] = if entity.team == 0 { 1 } else { 4 };
    body[121] = entity.team + 1;
    Some(Message::new(0x03f2, body))
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
            assert_eq!((message.opcode, b.len()), (0x03f2, 122));
            assert_eq!(word(b, 0), u32::from(entity.type_id));
            assert_eq!(word(b, 4), 0xc10b41da);
            assert_eq!(word(b, 8), u32::from(wire_id(entity.id)));
            assert_eq!(word(b, 12), entity.position.x.to_bits());
            assert_eq!(word(b, 20), entity.position.y.to_bits());
            assert_eq!(
                word(b, 24),
                if entity.team == 0 { 1.0_f32 } else { -1.0 }.to_bits()
            );
            assert_eq!(word(b, 36), entity.hp.to_bits());
            assert_eq!(word(b, 40), entity.max_hp.to_bits());
            assert_eq!(b[117..122], [0, 0, 1, 255, entity.team + 1]);
            assert!(recordable_mid_structure(b));
            slots.push(b[116]);
        }
        slots.sort_unstable();
        assert_eq!(slots, [6, 7, 9, 12, 26, 27, 32, 33]);
    }

    #[test]
    fn recording_accepts_only_the_registered_mid_lane_opening_profile() {
        let encoded = halcyon_protocol::SoloStructure::at_slot(27)
            .unwrap()
            .message(2065);
        assert!(recordable_mid_structure(&encoded.body));
        for offset in [0, 4, 8, 16, 24, 28, 64, 97, 116, 121] {
            let mut other = encoded.body.clone();
            other[offset] ^= 1;
            assert!(!recordable_mid_structure(&other), "changed byte {offset}");
        }
        let side_lane = halcyon_protocol::SoloStructure::at_slot(25)
            .unwrap()
            .message(2051);
        assert!(!recordable_mid_structure(&side_lane.body));
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
