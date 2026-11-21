use crate::AbilitySlot;
use halcyon_data::EntityTemplate;

// The three columns correspond to native learn indices 0, 1 and 2.
const MAPPINGS: [(EntityTemplate, [Option<u8>; 3]); 6] = [
    (EntityTemplate::Amael, [None, None, Some(2)]),
    (EntityTemplate::Alpha, [None, None, Some(2)]),
    (EntityTemplate::Hero009, [Some(0), Some(1), None]),
    (EntityTemplate::Yates, [Some(0), Some(2), Some(3)]),
    (EntityTemplate::Idris, [Some(0), Some(3), Some(4)]),
    (EntityTemplate::Lorelai, [Some(0), Some(1), Some(2)]),
];

pub fn native_learn_operand(hero: u16, slot: AbilitySlot) -> Option<u8> {
    let hero = EntityTemplate::from_raw(hero)?;
    MAPPINGS.iter().find(|(id, _)| *id == hero)?.1[slot as usize]
}

pub fn native_learn_request(hero: u16, index: u8) -> Option<(AbilitySlot, u8)> {
    let slot = *[AbilitySlot::A, AbilitySlot::B, AbilitySlot::C].get(usize::from(index))?;
    Some((slot, native_learn_operand(hero, slot)?))
}

pub fn known_native_learn_operand(operand: u8) -> bool {
    MAPPINGS.iter().any(|(_, row)| row.contains(&Some(operand)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CastTarget, Command, Position, World};

    #[test]
    fn captured_human_slots_consume_points_and_obey_rank_gates() {
        for (template, expected) in MAPPINGS {
            let hero = template.as_raw();
            let mut w = World::new(1, 1);
            w.entities[0].type_id = hero;
            w.entities[0].level = 5;
            w.entities[0].skill_points = 10;
            assert!(w.command(1, Command::Learn(AbilitySlot::C)).is_err());
            w.entities[0].level = 6;
            for (index, slot) in [AbilitySlot::A, AbilitySlot::B, AbilitySlot::C]
                .into_iter()
                .enumerate()
            {
                assert_eq!(
                    native_learn_request(hero, index as u8),
                    expected[index].map(|operand| (slot, operand))
                );
                let before = w.entities[0].skill_points;
                assert_eq!(
                    w.command(1, Command::Learn(slot)).is_ok(),
                    expected[index].is_some()
                );
                let learned = u8::from(expected[index].is_some());
                assert_eq!(w.entities[0].skill_points, before - learned);
                assert_eq!(w.entities[0].ability_ranks[index], learned);
            }
            assert!(w.command(1, Command::Learn(AbilitySlot::C)).is_err());
        }
        let mut w = World::new(1, 1);
        w.entities[0].type_id = 273;
        w.entities[0].level = 6;
        w.command(1, Command::Learn(AbilitySlot::C)).unwrap();
        assert!(w.command(1, Command::Learn(AbilitySlot::A)).is_err());
        assert!(
            w.command(
                1,
                Command::Cast {
                    slot: AbilitySlot::C,
                    target: CastTarget::Point(Position { x: 1.0, y: 0.0 }),
                }
            )
            .is_err()
        );
    }

    #[test]
    fn projection_only_basic_skill_has_the_same_rank_limits() {
        let mut w = World::new(2, 1);
        w.entities[0].type_id = 254;
        w.entities[0].skill_points = 6;
        for level in [1, 3, 5, 7, 9] {
            w.entities[0].level = level;
            assert!(w.command(1, Command::Learn(AbilitySlot::A)).is_ok());
            assert!(w.command(1, Command::Learn(AbilitySlot::A)).is_err());
        }
        w.entities[0].level = 12;
        assert!(w.command(1, Command::Learn(AbilitySlot::A)).is_err());
        assert_eq!(w.entities[0].ability_ranks, [5, 0, 0]);
        assert_eq!(w.entities[0].skill_points, 1);
    }

    #[test]
    fn bots_only_spend_points_on_playable_combat() {
        let mut w = World::new(1, 1);
        let bot = &mut w.entities[1];
        bot.type_id = 273;
        bot.level = 6;
        bot.skill_points = 2;
        for _ in 0..6 {
            w.step();
        }
        assert_eq!(w.entities[1].ability_ranks, [0, 1, 0]);
        assert_eq!(w.entities[1].skill_points, 1);
    }
}
