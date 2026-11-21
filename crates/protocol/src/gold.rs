use super::{Message, fixed_body, invalid};
use binrw::{BinRead, BinWrite};
use std::io;

const GOLD_OFFSET: usize = 286;
const RESOURCE_MASK_OFFSET: usize = 326;
const FULL_SPAWN_COMPACT_LEN: usize = 746;
const FULL_SPAWN_PADDED_LEN: usize = 750;

#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
struct SpawnGoldFields {
    gold: f32,
    cumulative: f32,
}

impl Message {
    fn has_spawn_resources(&self) -> bool {
        self.is(crate::Opcode::HeroSpawn)
            && matches!(
                self.body.len(),
                FULL_SPAWN_COMPACT_LEN | FULL_SPAWN_PADDED_LEN
            )
            && self.body[RESOURCE_MASK_OFFSET..RESOURCE_MASK_OFFSET + 4] == [0; 4]
    }

    /// Resources 6 and 7 in a full actor declaration: spendable gold and cumulative gold.
    pub fn spawn_gold(&self) -> Option<(f32, f32)> {
        self.has_spawn_resources().then(|| {
            let fields: SpawnGoldFields =
                fixed_body::read(&self.body[GOLD_OFFSET..GOLD_OFFSET + 8])
                    .expect("verified spawn gold slice");
            (fields.gold, fields.cumulative)
        })
    }

    /// Patch just the two resource baselines. The alternate stat mask omits these fields.
    pub fn set_spawn_gold(&mut self, gold: f32, cumulative: f32) -> io::Result<()> {
        if !self.has_spawn_resources()
            || !gold.is_finite()
            || !cumulative.is_finite()
            || gold < 0.
            || cumulative < 0.
        {
            return Err(invalid("invalid spawn gold baseline"));
        }
        fixed_body::write(
            &SpawnGoldFields { gold, cumulative },
            &mut self.body[GOLD_OFFSET..GOLD_OFFSET + 8],
            8,
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resource_mask_and_bounds_preserve_the_other_fields() {
        for length in [746, 750] {
            let mut m = Message::known(crate::Opcode::HeroSpawn, vec![0x31; length]);
            assert!(m.spawn_gold().is_none());
            assert!(m.set_spawn_gold(250., 2000.).is_err());
            m.body[326] = 0;
            assert!(m.spawn_gold().is_none());
            m.body[326..330].fill(0);
            let before = m.clone();
            m.set_spawn_gold(250., 2000.).unwrap();
            assert_eq!(m.spawn_gold(), Some((250., 2000.)));
            assert_eq!(m.body[..286], before.body[..286]);
            assert_eq!(m.body[294..], before.body[294..]);
        }
    }
}
