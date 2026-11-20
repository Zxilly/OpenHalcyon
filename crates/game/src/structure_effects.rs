//! Initial structure effects observed on pristine native maps.
use crate::{Batch, Message, Opcode};
use halcyon_protocol::{BuffApplication, BuffExtras, BuffPrefix, Fields, SoloStructure};

const MAP_SOURCE: u32 = u32::MAX;
const MAP_STRUCTURE_EFFECT: u16 = 205;
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
    map_instance: Option<u32>,
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
            map_instance: None,
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
            map_instance: match slot {
                0..=5 => Some(2169 - u32::from(slot)),
                24..=35 => Some(2152 + 2 * u32::from((35 - slot) / 2) + u32::from(slot % 2)),
                _ => None,
            },
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
            .chain(self.map_instance.map(|instance| BuffPrefix {
                target: self.actor,
                source: MAP_SOURCE,
                duration_bits: DURATION,
                instance,
                definition: MAP_STRUCTURE_EFFECT,
            }))
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

fn crystal_start_prefixes() -> impl Iterator<Item = BuffPrefix> {
    halcyon_sim::SOLO_CRYSTAL_START_INSTANCES
        .enumerate()
        .map(|(i, instance)| BuffPrefix {
            target: if i < 6 { 2007 } else { 2006 },
            source: MAP_SOURCE,
            duration_bits: DURATION,
            instance,
            definition: if i % 2 == 0 {
                MAP_STRUCTURE_EFFECT
            } else {
                180
            },
        })
}

/// Called only after the initial start marker and declarations for this stream.
pub(super) fn append_crystal_start(batch: &mut Batch, declared: &[Opening]) {
    for prefix in crystal_start_prefixes().filter(|p| declared.iter().any(|g| g.actor == p.target))
    {
        batch.push(
            BuffApplication {
                prefix,
                extras: None,
                suffix: &[],
            }
            .to_message()
            .expect("verified Solo crystal startup effect"),
        );
    }
}

#[derive(Default, Clone, Copy)]
pub(super) struct SoloSnapshot {
    started: bool,
    // Fixed slots survive fog and are reused when a VainNode rebuilds.
    lost_slots: u64,
}

impl SoloSnapshot {
    pub fn start(&mut self) {
        self.started = true;
    }

    pub fn observe(&mut self, events: &[halcyon_sim::Event]) {
        if self.started {
            for event in events {
                if let halcyon_sim::Event::Death { id, .. } = event
                    && let Some(structure) = id.checked_add(1499).and_then(SoloStructure::at_actor)
                {
                    self.lost_slots |= 1_u64 << structure.slot();
                }
            }
        }
    }

    pub fn append(self, batch: &mut Batch) {
        if !self.started {
            return;
        }
        let mut groups = Vec::new();
        for message in batch.iter() {
            let slot = match message.known_opcode() {
                Some(Opcode::Snapshot) => message.body.get(116),
                Some(Opcode::EntitySpawn) => message.body.get(745),
                _ => continue,
            };
            let Some(profile) = slot.copied().and_then(SoloStructure::at_slot) else {
                continue;
            };
            if self.lost_slots & (1_u64 << profile.slot()) == 0
                && message.body[8..12] == profile.actor().to_be_bytes()
                && message.body[..4] == u32::from(profile.template()).to_be_bytes()
            {
                groups.push(Opening::solo(profile.slot()).expect("known Solo structure"));
            }
        }
        groups.sort_unstable_by_key(|g| g.first_instance);
        for mut group in groups {
            if self.lost_slots != 0 {
                // No archive shows which map-owned instances survive a structure loss.
                group.map_instance = None;
            }
            batch.extend(group.messages());
            if self.lost_slots == 0 && matches!(group.actor, 2006 | 2007) {
                for prefix in crystal_start_prefixes()
                    .filter(|p| p.target == group.actor)
                    .take(2)
                {
                    batch.push(
                        BuffApplication {
                            prefix,
                            extras: Some(BuffExtras {
                                stacks: 3,
                                ..EXTRAS
                            }),
                            suffix: &[],
                        }
                        .to_message()
                        .expect("verified crystal snapshot"),
                    );
                }
            }
        }
    }
}

pub(super) fn recordable(message: &Message) -> bool {
    if message.is(Opcode::EntityProp) {
        return matches!(message.fields(), Ok(Fields::BuffApplication(effect))
            if effect.extras.is_none() && matches!(effect.suffix, [] | [0,0,0,0,0,0])
            && crystal_start_prefixes().any(|prefix| prefix == effect.prefix));
    }
    if !message.is(Opcode::EntityData) {
        return false;
    }
    let Ok(Fields::BuffApplication(effect)) = message.fields() else {
        return false;
    };
    let p = effect.prefix;
    if effect.extras
        == Some(BuffExtras {
            stacks: 3,
            ..EXTRAS
        })
    {
        return matches!(effect.suffix, [] | [0, 0, 0, 0])
            && crystal_start_prefixes()
                .filter(|row| row.target == p.target)
                .take(2)
                .any(|row| row == p);
    }
    if p.duration_bits != DURATION
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
    fn intact_solo_restore_matches_the_thirteen_late_sections_and_stops_after_loss() {
        let declarations: Vec<_> = (0..36)
            .rev()
            .filter_map(SoloStructure::at_slot)
            .map(|s| s.message(s.actor()))
            .collect();
        let mut current = SoloSnapshot::default();
        let mut b = declarations.clone();
        current.append(&mut b);
        assert_eq!(b, declarations);
        current.start();
        current.observe(&[halcyon_sim::Event::Death { id: 1, killer: 2 }]);
        current.append(&mut b);
        let effects = &b[declarations.len()..];
        assert_eq!(effects.len(), 98);
        let mut digest = Sha256::new();
        for m in effects {
            assert!(recordable(m));
            digest.update(&m.body);
        }
        assert_eq!(
            format!("{:x}", digest.finalize()),
            "4760604432928414b12617ade328441d2631620326f06d4adfb5359cac0603e3"
        );
        for actor in [2005_u32, 2008] {
            let mut after_loss = current;
            after_loss.observe(&[halcyon_sim::Event::Death {
                id: actor - 1499,
                killer: 1,
            }]);
            after_loss.observe(&[]);
            // Even a later declaration for a rebuilt node cannot revive its old instances.
            let mut b = declarations.clone();
            after_loss.append(&mut b);
            let effects = &b[declarations.len()..];
            assert_eq!(effects.len(), 73);
            for effect in effects {
                let Fields::BuffApplication(effect) = effect.fields().unwrap() else {
                    panic!()
                };
                assert_eq!(effect.prefix.target, effect.prefix.source);
                assert_ne!(effect.prefix.target, actor);
            }
        }
    }

    #[test]
    fn crystal_start_only_addresses_newly_declared_crystals_in_capture_order() {
        for slots in [vec![], vec![6], vec![7], vec![6, 7]] {
            let mut batch = Vec::new();
            let declared: Vec<_> = slots
                .iter()
                .map(|&slot| Opening::solo(slot).unwrap())
                .collect();
            append_crystal_start(&mut batch, &declared);
            let expected: Vec<_> = [(7, 2007_u32, 2170_u32), (6, 2006, 2176)]
                .into_iter()
                .filter(|(slot, _, _)| slots.contains(slot))
                .flat_map(|(_, actor, first)| {
                    (0..6).map(move |i| (actor, first + i, [205, 180][i as usize % 2]))
                })
                .collect();
            assert_eq!(batch.len(), expected.len());
            for (mut m, (actor, instance, definition)) in batch.into_iter().zip(expected) {
                assert!(m.is(Opcode::EntityProp));
                assert_eq!(
                    m.body,
                    [
                        actor.to_be_bytes().as_slice(),
                        &[255; 4],
                        &[0xbc, 0],
                        &instance.to_be_bytes(),
                        &u16::to_be_bytes(definition)
                    ]
                    .concat()
                );
                assert!(recordable(&m));
                m.body[3] ^= 1;
                assert!(!recordable(&m));
            }
        }
    }

    #[test]
    fn solo_opening_reproduces_all_initial_structure_prefixes() {
        let mut groups: Vec<_> = (0..36).filter_map(Opening::solo).collect();
        let mut messages = Vec::new();
        append_solo(&mut messages, &mut groups);
        assert_eq!(groups.len(), 26);
        assert_eq!(messages.len(), 94);
        let mut digest = Sha256::new();
        for mut m in messages {
            assert!(recordable(&m));
            digest.update(&m.body);
            m.body[9] ^= 1;
            assert!(!recordable(&m));
        }
        assert_eq!(
            format!("{:x}", digest.finalize()),
            "bf7618cbe20157483de2b0ad6978cd534ffff64514c710aa604981dbbbf3e350"
        );
    }
}
