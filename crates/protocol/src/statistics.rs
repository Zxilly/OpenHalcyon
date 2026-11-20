use super::{Message, invalid};
use std::io;

/// Structure-of-arrays record. Unknown columns and trailing bytes stay in Message::body.
#[derive(Debug, PartialEq)]
pub struct MatchStatistics<'a>(pub(crate) &'a [u8]);
#[derive(Debug, Clone, PartialEq)]
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
    pub counter_jungle_farm: u16,
    pub deaths_by_turret: u8,
    pub scout_cam_uses: u8,
    pub smurf_score: f32,
    pub average_interactions_per_minute_alive: f32,
    /// Android analytics maps 0..=4 to unknown, weapon, crystal, support and hybrid.
    pub item_build: u8,
    /// Empty item positions use 0x0000ffff.
    pub items: [u32; 10],
}
const TEAM_IDS: usize = 0x62c;
const RATIOS: usize = 0x63c;
const FLOAT_COLUMNS: [usize; 8] = [0x60, 0xa0, 0xe0, 0x120, 0x160, 0x1a0, 0x280, 0x2c0];
const HALF_COLUMNS: [usize; 4] = [0x1e0, 0x200, 0x220, 0x240];
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
    /// In each pair the second value is the minion contribution and the first is the remainder.
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
            counter_jungle_farm: half(HALF_COLUMNS[3]),
            deaths_by_turret: b[0x260 + i],
            scout_cam_uses: b[0x270 + i],
            smurf_score: float(FLOAT_COLUMNS[6]),
            average_interactions_per_minute_alive: float(FLOAT_COLUMNS[7]),
            item_build: b[0x580 + i],
            items: std::array::from_fn(|j| word(item_offset(i, j))),
        })
    }
}

/// In-place editor for a complete statistics report supplied by the caller.
/// Encode the same message after this borrow ends to retain the unknown columns and padding.
pub struct MatchStatisticsMut<'a>(&'a mut [u8]);

impl<'a> MatchStatisticsMut<'a> {
    pub fn new(message: &'a mut Message) -> io::Result<Self> {
        if !message.is(crate::Opcode::PostMatchStats) || !complete(&message.body) {
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
        self.write_slot(i, &stats)
    }

    fn write_slot(&mut self, i: usize, stats: &PlayerStatistics) -> io::Result<()> {
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
            stats.smurf_score,
            stats.average_interactions_per_minute_alive,
        ]) {
            let offset = column + 4 * i;
            self.0[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
        for (column, value) in HALF_COLUMNS.into_iter().zip([
            stats.lane_farm_at_five,
            stats.jungle_farm_at_five,
            stats.net_worth_at_five,
            stats.counter_jungle_farm,
        ]) {
            let offset = column + 2 * i;
            self.0[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
        }
        self.0[0x260 + i] = stats.deaths_by_turret;
        self.0[0x270 + i] = stats.scout_cam_uses;
        self.0[0x580 + i] = stats.item_build;
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

/// Both teams have the same columns. Times are preserved in native units.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct TeamStatistics {
    pub team: u8,
    pub average_net_worth_at_five: f32,
    pub average_level_at_five: f32,
    pub average_net_worth_at_ten: f32,
    pub average_level_at_ten: f32,
    pub first_blackclaw_capture_time: f32,
    pub first_ghostwing_capture_time: f32,
    pub first_outer_turret_kill_time: f32,
    pub first_mezzanine_turret_kill_time: f32,
    pub first_inner_turret_kill_time: f32,
    pub first_armory_kill_time: f32,
    pub hero_kills_at_five: u8,
    pub byte_630: u8,
    pub hero_kills_at_ten: u8,
    pub structure_kills_at_ten: u8,
    pub blackclaw_captures: u8,
    pub ghostwing_captures: u8,
    pub structure_kills: u8,
}

impl MatchStatistics<'_> {
    pub fn team(&self, i: usize) -> Option<TeamStatistics> {
        if i >= 2 {
            return None;
        }
        let word = |column: usize| {
            let off = column + 4 * i;
            f32::from_be_bytes(self.0[off..off + 4].try_into().unwrap())
        };
        Some(TeamStatistics {
            team: self.0[TEAM_IDS + i],
            average_net_worth_at_five: word(0x5dc),
            average_level_at_five: word(0x5e4),
            average_net_worth_at_ten: word(0x5ec),
            average_level_at_ten: word(0x5f4),
            first_blackclaw_capture_time: word(0x5fc),
            first_ghostwing_capture_time: word(0x604),
            first_outer_turret_kill_time: word(0x60c),
            first_mezzanine_turret_kill_time: word(0x614),
            first_inner_turret_kill_time: word(0x61c),
            first_armory_kill_time: word(0x624),
            hero_kills_at_five: self.0[0x62e + i],
            byte_630: self.0[0x630 + i],
            hero_kills_at_ten: self.0[0x632 + i],
            structure_kills_at_ten: self.0[0x634 + i],
            blackclaw_captures: self.0[0x636 + i],
            ghostwing_captures: self.0[0x638 + i],
            structure_kills: self.0[0x63a + i],
        })
    }

    /// Nineteen BE32 values forwarded by the reader as a separate block.
    pub fn auxiliary_words(&self) -> [u32; 19] {
        std::array::from_fn(|i| {
            let off = 0x590 + 4 * i;
            u32::from_be_bytes(self.0[off..off + 4].try_into().unwrap())
        })
    }
}

/// Original report from an authoritative producer. Auxiliary words require an explicit source.
pub struct StatisticsReport {
    pub players: Vec<PlayerStatistics>,
    pub teams: [TeamStatistics; 2],
    pub auxiliary_words: [u32; 19],
    pub minion_damage_ratio_inputs: [u32; 4],
}

impl StatisticsReport {
    pub fn encode(&self) -> io::Result<Message> {
        if self.players.is_empty() || self.players.len() > SLOTS {
            return Err(invalid("invalid statistics roster length"));
        }
        let mut body = vec![0; 1612];
        let mut view = MatchStatisticsMut(&mut body);
        for (i, p) in self.players.iter().enumerate() {
            if self.players[..i]
                .iter()
                .any(|other| other.player == p.player)
            {
                return Err(invalid("duplicate statistics player"));
            }
            view.write_slot(i, p)?;
        }
        for i in self.players.len()..SLOTS {
            view.0[4 * i..4 * i + 4].fill(255);
            view.0[TEAMS + i] = 255;
            for j in 0..10 {
                let offset = item_offset(i, j);
                view.0[offset..offset + 4].copy_from_slice(&0xffffu32.to_be_bytes());
            }
        }
        view.set_team_ids([self.teams[0].team, self.teams[1].team])?;
        for (i, t) in self.teams.iter().enumerate() {
            let floats = [
                t.average_net_worth_at_five,
                t.average_level_at_five,
                t.average_net_worth_at_ten,
                t.average_level_at_ten,
                t.first_blackclaw_capture_time,
                t.first_ghostwing_capture_time,
                t.first_outer_turret_kill_time,
                t.first_mezzanine_turret_kill_time,
                t.first_inner_turret_kill_time,
                t.first_armory_kill_time,
            ];
            for (j, val) in floats.into_iter().enumerate() {
                let off = 0x5dc + 8 * j + 4 * i;
                view.0[off..off + 4].copy_from_slice(&val.to_be_bytes());
            }
            let bytes = [
                t.hero_kills_at_five,
                t.byte_630,
                t.hero_kills_at_ten,
                t.structure_kills_at_ten,
                t.blackclaw_captures,
                t.ghostwing_captures,
                t.structure_kills,
            ];
            for (j, val) in bytes.into_iter().enumerate() {
                view.0[0x62e + 2 * j + i] = val;
            }
        }
        for (i, val) in self.auxiliary_words.iter().enumerate() {
            let off = 0x590 + 4 * i;
            view.0[off..off + 4].copy_from_slice(&val.to_be_bytes());
        }
        view.set_minion_damage_ratio_inputs(self.minion_damage_ratio_inputs);
        Ok(Message::known(crate::Opcode::PostMatchStats, body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Fields, MatchCipher};

    #[test]
    fn new_report_reconstructs_both_teams_and_uses_observed_empty_slots() {
        let mut source = vec![0; 1612];
        for i in 0..16 {
            source[4 * i..4 * i + 4].fill(255);
            source[0x40 + i] = 255;
            for j in 0..10 {
                let offset = 0x300 + 40 * i + 4 * j;
                source[offset..offset + 4].copy_from_slice(&0xffffu32.to_be_bytes());
            }
        }
        for i in 0..2 {
            source[4 * i..4 * i + 4].copy_from_slice(&(1500 + i as u32).to_be_bytes());
            source[0x40 + i] = i as u8 + 1;
            source[0x50 + i] = 255;
            source[0x240 + i * 2..0x242 + i * 2].copy_from_slice(&(13 + i as u16).to_be_bytes());
            source[0x260 + i] = 4;
            source[0x270 + i] = 2;
            source[0x580 + i] = 3;
        }
        for (i, b) in source[0x590..].iter_mut().enumerate() {
            *b = (i.wrapping_mul(31) % 127) as u8;
        }
        source[0x62c..0x62e].copy_from_slice(&[1, 2]);
        let view = MatchStatistics(&source);
        let mut report = StatisticsReport {
            players: vec![view.slot(0).unwrap(), view.slot(1).unwrap()],
            teams: [view.team(0).unwrap(), view.team(1).unwrap()],
            auxiliary_words: view.auxiliary_words(),
            minion_damage_ratio_inputs: view.minion_damage_ratio_inputs(),
        };
        assert_eq!(report.encode().unwrap().body, source);
        assert_eq!(report.players[1].counter_jungle_farm, 14);
        assert_eq!(report.teams[1].structure_kills, source[0x63b]);
        assert_eq!(
            report.teams[1].first_outer_turret_kill_time.to_bits(),
            u32::from_be_bytes(source[0x610..0x614].try_into().unwrap())
        );
        report.players[1].player = report.players[0].player;
        assert!(report.encode().is_err());
    }

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
            let mut message = Message::known(crate::Opcode::PostMatchStats, original);
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
        for (op, len) in [
            (crate::wire_opcode::PostMatchStats, 1611),
            (crate::wire_opcode::PostMatchStats, 1613),
            (crate::wire_opcode::EndMatch, 1612),
        ] {
            assert!(MatchStatisticsMut::new(&mut Message::new(op, vec![0; len])).is_err());
        }
    }
}
