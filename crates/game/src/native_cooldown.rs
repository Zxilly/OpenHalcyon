use super::{Batch, Message};
use halcyon_sim::{Entity, Kind};

/// FNV-1a over the exact internal ability definition name.
const fn key(name: &[u8]) -> u32 {
    let mut hash = 0x811c9dc5u32;
    let mut i = 0;
    while i < name.len() {
        hash = (hash ^ name[i] as u32).wrapping_mul(0x01000193);
        i += 1;
    }
    hash
}

const IDRIS_B: u32 = key(b"Ability__Idris__B");
const LORELAI_A: u32 = key(b"Ability__Lorelai__A");
const LORELAI_B: u32 = key(b"Ability__Lorelai__B");
const LORELAI_C: u32 = key(b"Ability__Lorelai__C");

fn initial_tail(ability: u32) -> Option<[u8; 6]> {
    match ability {
        IDRIS_B | LORELAI_A | LORELAI_B => Some([0, 0, 1, 0, 1, 0]),
        LORELAI_C => Some([0, 0, 1, 1, 1, 0]),
        _ => None,
    }
}

/// Called only after sending this hero's declaration in the initial scene.
pub(super) fn opening(batch: &mut Batch, actor: &Entity, aral: bool) {
    if actor.kind != Kind::Hero {
        return;
    }
    let abilities: &[u32] = match actor.type_id {
        273 => &[IDRIS_B],
        409 if aral => &[LORELAI_C, LORELAI_B, LORELAI_A],
        _ => return,
    };
    for ability in abilities {
        let mut body = [0; 22];
        body[..4].copy_from_slice(&u32::from(super::messages::wire_id(actor.id)).to_be_bytes());
        body[4..8].copy_from_slice(&ability.to_be_bytes());
        body[16..].copy_from_slice(&initial_tail(*ability).expect("verified opening ability"));
        batch.push(Message::known(crate::Opcode::CooldownSync, body));
    }
}

pub(super) fn recordable(msg: &Message) -> bool {
    let b = &msg.body;
    b.len() == 22
        && matches!(b[..4], [0, 0, 5, 220..=229])
        && b[8..16] == [0; 8]
        && initial_tail(u32::from_be_bytes(b[4..8].try_into().unwrap()))
            .is_some_and(|v| b[16..] == v)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn verified_opening_keys_are_hero_and_mode_specific() {
        let mut w = halcyon_sim::World::new(1, 10);
        let hero = &mut w.entities[0];
        let mut out = vec![];
        for (template, aral, wanted) in [
            (409, true, &[0x72761226, 0x737613b9, 0x70760f00][..]),
            (409, false, &[]),
            (273, true, &[0x5cb5e744]),
            (273, false, &[0x5cb5e744]),
            (243, true, &[]),
        ] {
            hero.type_id = template;
            opening(&mut out, hero, aral);
            assert_eq!(
                out.iter()
                    .map(|m| u32::from_be_bytes(m.body[4..8].try_into().unwrap()))
                    .collect::<Vec<_>>(),
                wanted
            );
            for msg in &mut out {
                assert!(recordable(msg));
                msg.body[8] = 1;
                assert!(!recordable(msg));
            }
            out.clear();
        }
    }
}
