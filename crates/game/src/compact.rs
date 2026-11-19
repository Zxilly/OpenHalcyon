use halcyon_protocol::Message;
use halcyon_sim::{Entity, Kind};

use super::messages::wire_id;

const BODY_LEN: usize = 122;
const STRUCTURE_SKIN_WORD: u32 = 0xc10b41da;

/// An opening outer mid-lane tower or a living home crystal in the Android 5v5 Solo
/// opening. The caller selects that opening; other map families use different
/// placement indices. Only one instance of each of these four slots is valid.
pub(super) fn structure(tick: u64, entity: &Entity) -> Option<Message> {
    let placement = match (entity.kind, entity.type_id, entity.team) {
        (Kind::Crystal, 295, 0) => 6,
        (Kind::Crystal, 296, 1) => 7,
        (Kind::Tower, 340, 0) if tick == 0 => 9,
        (Kind::Tower, 340, 1) if tick == 0 => 12,
        _ => return None,
    };
    if !(1..=u32::from(u16::MAX) - 1500).contains(&entity.id)
        || !entity.position.x.is_finite()
        || !entity.position.y.is_finite()
        || !entity.hp.is_finite()
        || entity.hp <= 0.0
        || !entity.max_hp.is_finite()
        || entity.hp > entity.max_hp
        || (tick == 0 && entity.hp != entity.max_hp)
    {
        return None;
    }

    let mut body = [0; BODY_LEN];
    put_word(&mut body, 0, u32::from(entity.type_id));
    put_word(&mut body, 4, STRUCTURE_SKIN_WORD);
    put_word(&mut body, 8, u32::from(wire_id(entity.id)));
    let forward = if entity.team == 0 { 1.0_f32 } else { -1.0 };
    for (offset, value) in [
        (12, entity.position.x),
        (16, 0.06),
        (20, entity.position.y),
        (24, forward),
        (36, entity.hp),
        (40, entity.max_hp),
        (84, 1.0),
    ] {
        put_word(&mut body, offset, value.to_bits());
    }
    if entity.kind == Kind::Tower {
        put_word(&mut body, 64, 1.0_f32.to_bits());
    }
    body[88..104].fill(1);
    body[97 + usize::from(entity.team)] = 15;
    put_word(&mut body, 112, u32::MAX);
    body[116] = placement;
    body[119] = 1;
    body[120] = u8::MAX;
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
        let world = World::new(0, 1);
        let mut slots = Vec::new();
        for entity in &world.entities {
            let Some(message) = structure(world.tick, entity) else {
                assert!(matches!(entity.kind, Kind::Hero | Kind::Minion));
                continue;
            };
            let b = &message.body;
            assert_eq!((message.opcode, b.len()), (0x03f2, 122));
            assert_eq!(word(b, 0), u32::from(entity.type_id));
            assert_eq!(word(b, 4), STRUCTURE_SKIN_WORD);
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
            slots.push(b[116]);
        }
        slots.sort_unstable();
        assert_eq!(slots, [6, 7, 9, 12]);
    }

    #[test]
    fn later_crystal_uses_current_health_but_tower_requires_opening_state() {
        let world = World::new(0, 1);
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
