use super::{Message, Opcode, fixed_body::fixed_packet, invalid};
use binrw::{BinRead, BinWrite};
use std::io;

const SLOTS: usize = 16;
const REPORT_BODY_LEN: usize = 1612;
const REPORT_PADDED_LEN: usize = 1614;
const EMPTY_SLOT_ID: u32 = u32::MAX;
const EMPTY_ITEM_ID: u32 = 0x0000ffff;

/// 1612-byte Structure-of-Arrays wire payload of `0x048D` (`Opcode::PostMatchStats`).
#[derive(Clone, Debug, PartialEq, Default, BinRead, BinWrite)]
#[brw(big)]
pub struct StatisticsWireBody {
    pub players: [u32; SLOTS],
    pub teams: [u8; SLOTS],
    pub slot_flags: [u8; SLOTS],
    pub damage_to_heroes: [f32; SLOTS],
    pub weapon_damage_to_heroes: [f32; SLOTS],
    pub crystal_damage_to_heroes: [f32; SLOTS],
    pub true_damage_to_heroes: [f32; SLOTS],
    pub damage_to_structures: [f32; SLOTS],
    pub ambient_gold_input: [f32; SLOTS],
    pub lane_farm_at_five: [u16; SLOTS],
    pub jungle_farm_at_five: [u16; SLOTS],
    pub net_worth_at_five: [u16; SLOTS],
    pub counter_jungle_farm: [u16; SLOTS],
    pub deaths_by_turret: [u8; SLOTS],
    pub scout_cam_uses: [u8; SLOTS],
    pub smurf_score: [f32; SLOTS],
    pub average_interactions_per_minute_alive: [f32; SLOTS],
    pub items: [[u32; 10]; SLOTS],
    pub item_builds: [u8; SLOTS],
    pub auxiliary_words: [u32; 19],
    pub average_net_worth_at_five: [f32; 2],
    pub average_level_at_five: [f32; 2],
    pub average_net_worth_at_ten: [f32; 2],
    pub average_level_at_ten: [f32; 2],
    pub first_blackclaw_capture_time: [f32; 2],
    pub first_ghostwing_capture_time: [f32; 2],
    pub first_outer_turret_kill_time: [f32; 2],
    pub first_mezzanine_turret_kill_time: [f32; 2],
    pub first_inner_turret_kill_time: [f32; 2],
    pub first_armory_kill_time: [f32; 2],
    pub team_ids: [u8; 2],
    pub hero_kills_at_five: [u8; 2],
    pub byte_630: [u8; 2],
    pub hero_kills_at_ten: [u8; 2],
    pub structure_kills_at_ten: [u8; 2],
    pub blackclaw_captures: [u8; 2],
    pub ghostwing_captures: [u8; 2],
    pub structure_kills: [u8; 2],
    pub minion_damage_ratio_inputs: [u32; 4],
}
fixed_packet!(StatisticsWireBody, Opcode::PostMatchStats, 1612);

impl StatisticsWireBody {
    pub fn slot(&self, i: usize) -> Option<PlayerStatistics> {
        if i >= SLOTS {
            return None;
        }
        Some(PlayerStatistics {
            player: self.players[i],
            team: self.teams[i],
            slot_flag: self.slot_flags[i],
            damage_to_heroes: self.damage_to_heroes[i],
            weapon_damage_to_heroes: self.weapon_damage_to_heroes[i],
            crystal_damage_to_heroes: self.crystal_damage_to_heroes[i],
            true_damage_to_heroes: self.true_damage_to_heroes[i],
            damage_to_structures: self.damage_to_structures[i],
            ambient_gold_input: self.ambient_gold_input[i],
            lane_farm_at_five: self.lane_farm_at_five[i],
            jungle_farm_at_five: self.jungle_farm_at_five[i],
            net_worth_at_five: self.net_worth_at_five[i],
            counter_jungle_farm: self.counter_jungle_farm[i],
            deaths_by_turret: self.deaths_by_turret[i],
            scout_cam_uses: self.scout_cam_uses[i],
            smurf_score: self.smurf_score[i],
            average_interactions_per_minute_alive: self.average_interactions_per_minute_alive[i],
            item_build: self.item_builds[i],
            items: self.items[i],
        })
    }

    pub fn set_slot(&mut self, i: usize, stats: &PlayerStatistics) -> io::Result<()> {
        if i >= SLOTS || stats.player == EMPTY_SLOT_ID || !matches!(stats.team, 1 | 2) {
            return Err(invalid("invalid occupied statistics slot"));
        }
        self.players[i] = stats.player;
        self.teams[i] = stats.team;
        self.slot_flags[i] = stats.slot_flag;
        self.damage_to_heroes[i] = stats.damage_to_heroes;
        self.weapon_damage_to_heroes[i] = stats.weapon_damage_to_heroes;
        self.crystal_damage_to_heroes[i] = stats.crystal_damage_to_heroes;
        self.true_damage_to_heroes[i] = stats.true_damage_to_heroes;
        self.damage_to_structures[i] = stats.damage_to_structures;
        self.ambient_gold_input[i] = stats.ambient_gold_input;
        self.lane_farm_at_five[i] = stats.lane_farm_at_five;
        self.jungle_farm_at_five[i] = stats.jungle_farm_at_five;
        self.net_worth_at_five[i] = stats.net_worth_at_five;
        self.counter_jungle_farm[i] = stats.counter_jungle_farm;
        self.deaths_by_turret[i] = stats.deaths_by_turret;
        self.scout_cam_uses[i] = stats.scout_cam_uses;
        self.smurf_score[i] = stats.smurf_score;
        self.average_interactions_per_minute_alive[i] = stats.average_interactions_per_minute_alive;
        self.item_builds[i] = stats.item_build;
        self.items[i] = stats.items;
        Ok(())
    }

    pub fn team(&self, i: usize) -> Option<TeamStatistics> {
        if i >= 2 {
            return None;
        }
        Some(TeamStatistics {
            team: self.team_ids[i],
            average_net_worth_at_five: self.average_net_worth_at_five[i],
            average_level_at_five: self.average_level_at_five[i],
            average_net_worth_at_ten: self.average_net_worth_at_ten[i],
            average_level_at_ten: self.average_level_at_ten[i],
            first_blackclaw_capture_time: self.first_blackclaw_capture_time[i],
            first_ghostwing_capture_time: self.first_ghostwing_capture_time[i],
            first_outer_turret_kill_time: self.first_outer_turret_kill_time[i],
            first_mezzanine_turret_kill_time: self.first_mezzanine_turret_kill_time[i],
            first_inner_turret_kill_time: self.first_inner_turret_kill_time[i],
            first_armory_kill_time: self.first_armory_kill_time[i],
            hero_kills_at_five: self.hero_kills_at_five[i],
            byte_630: self.byte_630[i],
            hero_kills_at_ten: self.hero_kills_at_ten[i],
            structure_kills_at_ten: self.structure_kills_at_ten[i],
            blackclaw_captures: self.blackclaw_captures[i],
            ghostwing_captures: self.ghostwing_captures[i],
            structure_kills: self.structure_kills[i],
        })
    }

    pub fn set_team(&mut self, i: usize, team: &TeamStatistics) {
        self.team_ids[i] = team.team;
        self.average_net_worth_at_five[i] = team.average_net_worth_at_five;
        self.average_level_at_five[i] = team.average_level_at_five;
        self.average_net_worth_at_ten[i] = team.average_net_worth_at_ten;
        self.average_level_at_ten[i] = team.average_level_at_ten;
        self.first_blackclaw_capture_time[i] = team.first_blackclaw_capture_time;
        self.first_ghostwing_capture_time[i] = team.first_ghostwing_capture_time;
        self.first_outer_turret_kill_time[i] = team.first_outer_turret_kill_time;
        self.first_mezzanine_turret_kill_time[i] = team.first_mezzanine_turret_kill_time;
        self.first_inner_turret_kill_time[i] = team.first_inner_turret_kill_time;
        self.first_armory_kill_time[i] = team.first_armory_kill_time;
        self.hero_kills_at_five[i] = team.hero_kills_at_five;
        self.byte_630[i] = team.byte_630;
        self.hero_kills_at_ten[i] = team.hero_kills_at_ten;
        self.structure_kills_at_ten[i] = team.structure_kills_at_ten;
        self.blackclaw_captures[i] = team.blackclaw_captures;
        self.ghostwing_captures[i] = team.ghostwing_captures;
        self.structure_kills[i] = team.structure_kills;
    }
}

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

pub(super) fn complete(body: &[u8]) -> bool {
    matches!(body.len(), REPORT_BODY_LEN | REPORT_PADDED_LEN)
}

impl MatchStatistics<'_> {
    fn wire(&self) -> StatisticsWireBody {
        StatisticsWireBody::read_body(self.0).expect("complete 0x048d statistics body")
    }

    /// Two wire team IDs associated with the report's team records.
    pub fn team_ids(&self) -> [u8; 2] {
        self.wire().team_ids
    }

    /// Four original BE32 float bit patterns feeding two minion damage ratios.
    /// In each pair the second value is the minion contribution and the first is the remainder.
    pub fn minion_damage_ratio_inputs(&self) -> [u32; 4] {
        self.wire().minion_damage_ratio_inputs
    }

    /// Returns the raw slot, including any unused/sentinel player identifier.
    pub fn slot(&self, i: usize) -> Option<PlayerStatistics> {
        self.wire().slot(i)
    }
}

/// In-place editor for a complete statistics report supplied by the caller.
/// Encode the same message after this borrow ends to retain the unknown columns and padding.
pub struct MatchStatisticsMut<'a>(&'a mut [u8]);

impl<'a> MatchStatisticsMut<'a> {
    pub fn new(message: &'a mut Message) -> io::Result<Self> {
        if !message.is(Opcode::PostMatchStats) || !complete(&message.body) {
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
        let mut wire = StatisticsWireBody::read_body(self.0)?;
        let mut stats = wire.slot(i).ok_or_else(|| invalid("slot out of range"))?;
        if stats.player == EMPTY_SLOT_ID {
            return Err(invalid("cannot edit an unused statistics slot"));
        }
        edit(&mut stats);
        wire.set_slot(i, &stats)?;
        wire.write_body(self.0)?;
        Ok(())
    }

    pub fn set_team_ids(&mut self, ids: [u8; 2]) -> io::Result<()> {
        if !matches!(ids, [1, 2] | [2, 1]) {
            return Err(invalid("expected both statistics teams"));
        }
        let mut wire = StatisticsWireBody::read_body(self.0)?;
        wire.team_ids = ids;
        wire.write_body(self.0)?;
        Ok(())
    }

    pub fn set_minion_damage_ratio_inputs(&mut self, bits: [u32; 4]) {
        let mut wire = StatisticsWireBody::read_body(self.0).expect("complete 0x048d report");
        wire.minion_damage_ratio_inputs = bits;
        wire.write_body(self.0)
            .expect("complete 0x048d report output");
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
        self.wire().team(i)
    }

    /// Nineteen BE32 values forwarded by the reader as a separate block.
    pub fn auxiliary_words(&self) -> [u32; 19] {
        self.wire().auxiliary_words
    }
}

/// Original report from an authoritative producer. Auxiliary words require an explicit source.
#[derive(Clone, Debug, PartialEq)]
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
        let mut wire = StatisticsWireBody::default();
        for (i, p) in self.players.iter().enumerate() {
            if self.players[..i]
                .iter()
                .any(|other| other.player == p.player)
            {
                return Err(invalid("duplicate statistics player"));
            }
            wire.set_slot(i, p)?;
        }
        for i in self.players.len()..SLOTS {
            wire.players[i] = EMPTY_SLOT_ID;
            wire.teams[i] = 255;
            wire.items[i] = [EMPTY_ITEM_ID; 10];
        }
        let ids = [self.teams[0].team, self.teams[1].team];
        if !matches!(ids, [1, 2] | [2, 1]) {
            return Err(invalid("expected both statistics teams"));
        }
        for (i, t) in self.teams.iter().enumerate() {
            wire.set_team(i, t);
        }
        wire.auxiliary_words = self.auxiliary_words;
        wire.minion_damage_ratio_inputs = self.minion_damage_ratio_inputs;
        Ok(wire.encode())
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
