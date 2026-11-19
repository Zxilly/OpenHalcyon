use super::{Message, invalid};
use std::io;

/// Structure-of-arrays record. Unknown columns and trailing bytes stay in Message::body.
#[derive(Debug, PartialEq)]
pub struct MatchStatistics<'a>(pub(crate) &'a [u8]);
#[derive(Debug, PartialEq)]
pub struct PlayerStatistics {
    pub player: u32,
    pub team: u8,
    /// Raw +0x50 column. All eight observed reports used 255 for occupied slots and zero for unused slots.
    pub slot_flag: u8,
    pub damage_to_heroes: f32,
    pub weapon_damage_to_heroes: f32,
    pub crystal_damage_to_heroes: f32,
    pub true_damage_to_heroes: f32,
    pub damage_to_structures: f32,
    pub ambient_gold_input: f32,
    pub lane_farm_at_five: u16,
    pub jungle_farm_at_five: u16,
    pub net_worth_at_five: u16,
    pub items: [u32; 10],
}
const TEAM_IDS: usize = 0x62c;
const RATIOS: usize = 0x63c;
const FLOAT_COLUMNS: [usize; 6] = [0x60, 0xa0, 0xe0, 0x120, 0x160, 0x1a0];
const HALF_COLUMNS: [usize; 3] = [0x1e0, 0x200, 0x220];
const TEAMS: usize = 0x40;
const SLOT_FLAGS: usize = 0x50;
const SLOTS: usize = 16;

fn item_offset(slot: usize, item: usize) -> usize {
    0x300 + 40 * slot + 4 * item
}

pub(super) fn complete(body: &[u8]) -> bool {
    matches!(body.len(), 1612 | 1614)
}

impl MatchStatistics<'_> {
    /// Two wire team IDs associated with the report's team records.
    pub fn team_ids(&self) -> [u8; 2] {
        [self.0[TEAM_IDS], self.0[TEAM_IDS + 1]]
    }

    /// Four original BE32 float bit patterns feeding two minion damage ratios.
    /// The original numerator and denominator names remain unresolved.
    pub fn minion_damage_ratio_inputs(&self) -> [u32; 4] {
        std::array::from_fn(|i| {
            let start = RATIOS + 4 * i;
            u32::from_be_bytes(self.0[start..start + 4].try_into().unwrap())
        })
    }

    /// Returns the raw slot, including any unused/sentinel player identifier.
    pub fn slot(&self, i: usize) -> Option<PlayerStatistics> {
        if i >= SLOTS {
            return None;
        }
        let b = self.0;
        let word = |p: usize| u32::from_be_bytes(b[p..p + 4].try_into().unwrap());
        let float = |p: usize| f32::from_bits(word(p + 4 * i));
        let half = |p: usize| u16::from_be_bytes(b[p + 2 * i..p + 2 * i + 2].try_into().unwrap());
        Some(PlayerStatistics {
            player: word(4 * i),
            team: b[TEAMS + i],
            slot_flag: b[SLOT_FLAGS + i],
            damage_to_heroes: float(FLOAT_COLUMNS[0]),
            weapon_damage_to_heroes: float(FLOAT_COLUMNS[1]),
            crystal_damage_to_heroes: float(FLOAT_COLUMNS[2]),
            true_damage_to_heroes: float(FLOAT_COLUMNS[3]),
            damage_to_structures: float(FLOAT_COLUMNS[4]),
            ambient_gold_input: float(FLOAT_COLUMNS[5]),
            lane_farm_at_five: half(HALF_COLUMNS[0]),
            jungle_farm_at_five: half(HALF_COLUMNS[1]),
            net_worth_at_five: half(HALF_COLUMNS[2]),
            items: std::array::from_fn(|j| word(item_offset(i, j))),
        })
    }
}

/// In-place editor for a complete statistics report supplied by the caller.
/// Encode the same message after this borrow ends to retain the unknown columns and padding.
pub struct MatchStatisticsMut<'a>(&'a mut [u8]);

impl<'a> MatchStatisticsMut<'a> {
    pub fn new(message: &'a mut Message) -> io::Result<Self> {
        if message.opcode != 0x048d || !complete(&message.body) {
            return Err(invalid("expected a complete 0x048d report"));
        }
        Ok(Self(&mut message.body))
    }

    /// Updates only evidenced columns of an occupied slot. Invalid changes leave the body intact.
    pub fn edit_slot(
        &mut self,
        i: usize,
        edit: impl FnOnce(&mut PlayerStatistics),
    ) -> io::Result<()> {
        let mut stats = MatchStatistics(self.0)
            .slot(i)
            .ok_or_else(|| invalid("slot out of range"))?;
        if stats.player == u32::MAX {
            return Err(invalid("cannot edit an unused statistics slot"));
        }
        edit(&mut stats);
        if stats.player == u32::MAX || !matches!(stats.team, 1 | 2) {
            return Err(invalid("invalid occupied statistics slot"));
        }
        self.0[4 * i..4 * i + 4].copy_from_slice(&stats.player.to_be_bytes());
        self.0[TEAMS + i] = stats.team;
        self.0[SLOT_FLAGS + i] = stats.slot_flag;
        for (column, value) in FLOAT_COLUMNS.into_iter().zip([
            stats.damage_to_heroes,
            stats.weapon_damage_to_heroes,
            stats.crystal_damage_to_heroes,
            stats.true_damage_to_heroes,
            stats.damage_to_structures,
            stats.ambient_gold_input,
        ]) {
            let offset = column + 4 * i;
            self.0[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
        for (column, value) in HALF_COLUMNS.into_iter().zip([
            stats.lane_farm_at_five,
            stats.jungle_farm_at_five,
            stats.net_worth_at_five,
        ]) {
            let offset = column + 2 * i;
            self.0[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
        }
        for (j, value) in stats.items.into_iter().enumerate() {
            let offset = item_offset(i, j);
            self.0[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
        Ok(())
    }

    pub fn set_team_ids(&mut self, ids: [u8; 2]) -> io::Result<()> {
        if !matches!(ids, [1, 2] | [2, 1]) {
            return Err(invalid("expected both statistics teams"));
        }
        self.0[TEAM_IDS..TEAM_IDS + 2].copy_from_slice(&ids);
        Ok(())
    }

    pub fn set_minion_damage_ratio_inputs(&mut self, bits: [u32; 4]) {
        for (i, value) in bits.into_iter().enumerate() {
            let offset = RATIOS + 4 * i;
            self.0[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Fields, MatchCipher};

    #[test]
    fn patching_existing_report_preserves_all_other_slots_columns_and_padding() {
        let cipher = MatchCipher::new("synthetic");
        for size in [1612, 1614] {
            let mut original = vec![0xa5; size];
            let i = 3;
            original[..4].copy_from_slice(&u32::MAX.to_be_bytes());
            original[4 * i..4 * i + 4].copy_from_slice(&1503u32.to_be_bytes());
            original[0x40 + i] = 2;
            let mut expected = original.clone();
            expected[0xa0 + 4 * i..0xa4 + 4 * i].copy_from_slice(&17.25f32.to_be_bytes());
            expected[0x220 + 2 * i..0x222 + 2 * i].copy_from_slice(&271u16.to_be_bytes());
            expected[0x300 + 40 * i + 36..0x300 + 40 * i + 40]
                .copy_from_slice(&123u32.to_be_bytes());
            expected[0x62c..0x62e].copy_from_slice(&[2, 1]);
            let ratios = [0, 0x80000000, 0x7fc01234, 0x3f800000u32];
            for (i, n) in ratios.into_iter().enumerate() {
                expected[0x63c + 4 * i..0x640 + 4 * i].copy_from_slice(&n.to_be_bytes());
            }
            let mut message = Message::new(0x048d, original);
            let mut report = MatchStatisticsMut::new(&mut message).unwrap();
            assert!(report.edit_slot(16, |_| panic!()).is_err());
            assert!(report.edit_slot(0, |_| panic!()).is_err());
            assert!(
                report
                    .edit_slot(i, |s| {
                        s.damage_to_structures = 1.;
                        s.player = u32::MAX;
                    })
                    .is_err()
            );
            assert!(report.set_team_ids([1, 1]).is_err());
            report
                .edit_slot(i, |s| {
                    s.weapon_damage_to_heroes = 17.25;
                    s.net_worth_at_five = 271;
                    s.items[9] = 123;
                })
                .unwrap();
            report.set_team_ids([2, 1]).unwrap();
            report.set_minion_damage_ratio_inputs(ratios);
            assert_eq!(message.body, expected);
            let frame = cipher.encode(&message).unwrap();
            let mut decoded = cipher.decode(&frame[2..]).unwrap();
            let Fields::MatchStatistics(report) = decoded.fields().unwrap() else {
                panic!()
            };
            assert_eq!(report.slot(i).unwrap().items[9], 123);
            assert_eq!(report.team_ids(), [2, 1]);
            assert_eq!(report.minion_damage_ratio_inputs(), ratios);
            MatchStatisticsMut::new(&mut decoded)
                .unwrap()
                .edit_slot(i, |_| {})
                .unwrap();
            assert_eq!(cipher.encode(&decoded).unwrap(), frame);
        }
        for (op, len) in [(0x48d, 1611), (0x48d, 1613), (0x3f1, 1612)] {
            assert!(MatchStatisticsMut::new(&mut Message::new(op, vec![0; len])).is_err());
        }
    }
}
