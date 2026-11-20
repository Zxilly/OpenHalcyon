//! Self-sourced structure effects observed on pristine native maps.
use crate::{Batch, Message, Opcode};
use halcyon_protocol::{BuffApplication, BuffExtras, BuffPrefix, Fields, SoloStructure};

const DURATION: u16 = 0xbc00;
const EXTRAS: BuffExtras = BuffExtras {
    stacks: 1,
    words: [0; 4],
};

#[derive(Clone, Copy)]
pub(super) struct Opening {
    actor: u32,
    first_instance: u32,
    definitions: &'static [u16],
}

impl Opening {
    pub(super) fn aral(slot: u8) -> Option<Self> {
        let (side, local) = (slot / 8, slot % 8);
        if side > 1 || !(3..=7).contains(&local) {
            return None;
        }
        let actor = 2000 + u32::from(slot);
        Some(Self {
            actor,
            first_instance: actor + 19 + 5 * u32::from(local - 3) + 27 * u32::from(side),
            definitions: &[194, 198, 204],
        })
    }

    pub(super) fn solo(slot: u8) -> Option<Self> {
        let actor = SoloStructure::at_slot(slot)?.actor();
        let (first_instance, definitions): (_, &'static [u16]) = match slot {
            0..=5 => (2014 + 4 * u32::from(slot), &[207, 208, 209]),
            6..=7 => (2038 + 2 * u32::from(slot - 6), &[178, 179]),
            8..=13 => (2042 + 14 * u32::from(slot - 8), &[194, 199, 204]),
            24..=35 => (actor + 1, &[194, 199, 204]),
            _ => return None,
        };
        Some(Self {
            actor,
            first_instance,
            definitions,
        })
    }

    pub(super) fn actor(self) -> u32 {
        self.actor
    }

    fn prefixes(self) -> impl Iterator<Item = BuffPrefix> {
        self.definitions
            .iter()
            .enumerate()
            .map(move |(ordinal, &definition)| BuffPrefix {
                target: self.actor,
                source: self.actor,
                duration_bits: DURATION,
                instance: self.first_instance + ordinal as u32,
                definition,
            })
    }

    pub(super) fn messages(self) -> impl Iterator<Item = Message> {
        self.prefixes().map(|prefix| {
            BuffApplication {
                prefix,
                extras: Some(EXTRAS),
                suffix: &[],
            }
            .to_message()
            .expect("verified opening effect")
        })
    }
}

/// The Solo recording groups effects after declarations, in descending order
/// of each structure's first effect instance. Only declared actors are passed in.
pub(super) fn append_solo(batch: &mut Batch, visible: &mut [Opening]) {
    visible.sort_unstable_by_key(|group| std::cmp::Reverse(group.first_instance));
    batch.extend(visible.iter().flat_map(|group| group.messages()));
}

pub(super) fn recordable(message: &Message) -> bool {
    if !message.is(Opcode::EntityData) {
        return false;
    }
    let Ok(Fields::BuffApplication(effect)) = message.fields() else {
        return false;
    };
    let p = effect.prefix;
    if p.target != p.source
        || p.duration_bits != DURATION
        || effect.extras != Some(EXTRAS)
        || !matches!(effect.suffix, [] | [0, 0, 0, 0])
    {
        return false;
    }
    let aral = p
        .target
        .checked_sub(2000)
        .and_then(|s| u8::try_from(s).ok())
        .and_then(Opening::aral);
    aral.into_iter()
        .chain(SoloStructure::at_actor(p.target).and_then(|s| Opening::solo(s.slot())))
        .any(|group| group.actor == p.target && group.prefixes().any(|row| row == p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn solo_opening_reproduces_all_self_sourced_structure_prefixes() {
        let mut groups: Vec<_> = (0..36).filter_map(Opening::solo).collect();
        let mut messages = Vec::new();
        append_solo(&mut messages, &mut groups);
        assert_eq!(groups.len(), 26);
        assert_eq!(messages.len(), 76);
        let mut digest = Sha256::new();
        for mut m in messages {
            assert!(recordable(&m));
            digest.update(&m.body);
            m.body[9] ^= 1;
            assert!(!recordable(&m));
        }
        assert_eq!(
            format!("{:x}", digest.finalize()),
            "6f0364b0d4320ee94eafb96c44e3ebb97b3cc5520d6a4ff5a374a7a1e6cc06d0"
        );
    }
}
