use crate::{EntityTemplate, GameData};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{io, path::Path};

const TEMPLATES: [(EntityTemplate, &str); 4] = [
    (EntityTemplate::MinionRange5v5, "5v5_Minion_Range"),
    (EntityTemplate::MinionMelee5v5, "5v5_Minion_Melee"),
    (EntityTemplate::MinionSiege5v5, "5v5_Minion_Siege"),
    (EntityTemplate::MinionCaptain5v5, "5v5_Minion_Captain"),
];

#[derive(Deserialize)]
struct Unit {
    name: String,
    entity_type_id: Option<u16>,
    gold_bounty: f64,
    xp_bounty: f32,
}

/// Base gold and XP before the room distributes the kill reward.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct SoloLaneRewards([(u32, f32); 4]);

impl SoloLaneRewards {
    pub(crate) fn load(dir: &Path, hash: &mut Sha256) -> io::Result<Self> {
        let rows: Vec<Unit> = crate::read(dir, "units_full.json", hash)?;
        Self::from_rows(&rows)
    }

    fn from_rows(rows: &[Unit]) -> io::Result<Self> {
        let mut rewards = [(0, 0.); 4];
        for (index, (template, name)) in TEMPLATES.iter().enumerate() {
            let invalid = || {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid or ambiguous {name} bounty in units_full.json"),
                )
            };
            let mut matches = rows
                .iter()
                .filter(|r| r.entity_type_id == Some(template.as_raw()));
            let row = matches.next().ok_or_else(invalid)?;
            if matches.next().is_some()
                || row.name != *name
                || !(0. ..=f64::from(u16::MAX)).contains(&row.gold_bounty)
                || row.gold_bounty.fract() != 0.
                || !row.xp_bounty.is_finite()
                || row.xp_bounty < 0.
            {
                return Err(invalid());
            }
            rewards[index] = (row.gold_bounty as u32, row.xp_bounty);
        }
        Ok(Self(rewards))
    }

    pub fn for_template(self, template: EntityTemplate) -> Option<(u32, f32)> {
        let index = TEMPLATES
            .iter()
            .position(|(expected, _)| template == *expected)?;
        Some(self.0[index])
    }
}

impl GameData {
    pub fn solo_lane_rewards(&self) -> Option<SoloLaneRewards> {
        self.extra.solo_lane_rewards
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounty_requires_the_named_template_once_and_integral_gold() {
        let mut rows: Vec<_> = TEMPLATES
            .iter()
            .map(|(t, n)| Unit {
                name: n.to_string(),
                entity_type_id: Some(t.as_raw()),
                gold_bounty: 35.,
                xp_bounty: 8.,
            })
            .collect();
        assert_eq!(
            SoloLaneRewards::from_rows(&rows)
                .unwrap()
                .for_template(EntityTemplate::MinionMelee5v5),
            Some((35, 8.))
        );
        assert_eq!(
            SoloLaneRewards::from_rows(&rows)
                .unwrap()
                .for_template(EntityTemplate::Ringo),
            None
        );
        rows[0].gold_bounty = 0.5;
        assert!(SoloLaneRewards::from_rows(&rows).is_err());
        rows[0].gold_bounty = 12.;
        rows[0].entity_type_id = rows[1].entity_type_id;
        assert!(SoloLaneRewards::from_rows(&rows).is_err());
    }
}
