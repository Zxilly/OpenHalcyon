use super::{Message, invalid};
use std::io;

impl Message {
    fn has_spawn_resources(&self) -> bool {
        self.opcode == 0x03f3 && matches!(self.body.len(), 746 | 750) && self.body[326] == 0
    }

    /// Resources 6 and 7 in a full actor declaration: spendable gold and cumulative gold.
    pub fn spawn_gold(&self) -> Option<(f32, f32)> {
        self.has_spawn_resources().then(|| {
            let read = |i| f32::from_be_bytes(self.body[i..i + 4].try_into().unwrap());
            (read(286), read(290))
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
        self.body[286..290].copy_from_slice(&gold.to_be_bytes());
        self.body[290..294].copy_from_slice(&cumulative.to_be_bytes());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resource_mask_and_bounds_preserve_the_other_fields() {
        for length in [746, 750] {
            let mut m = Message::new(0x03f3, vec![0x31; length]);
            assert!(m.spawn_gold().is_none());
            assert!(m.set_spawn_gold(250., 2000.).is_err());
            m.body[326] = 0;
            let before = m.clone();
            m.set_spawn_gold(250., 2000.).unwrap();
            assert_eq!(m.spawn_gold(), Some((250., 2000.)));
            assert_eq!(m.body[..286], before.body[..286]);
            assert_eq!(m.body[294..], before.body[294..]);
        }
    }
}
