use halcyon_protocol::Message;
use halcyon_sim::{Entity, Kind, World};

use super::{Pick, messages, messages::wire_id, point_move};

const STRUCTURE_SKIN: u32 = 0xc10b41da;
const MINION_SKIN: u32 = 0xeb39ce55;

/// One actual ARAL tower or crystal, registered at its observed map slot.
pub(super) fn structure(world: &World, entity: &Entity) -> Option<Message> {
    let slot = world.aral_structure_slot(entity.id)?;
    let (height, forward, profile_word84) = match (entity.kind, entity.type_id, slot) {
        (Kind::Crystal, 292, 1) => (0.0, [0.0, 0.0, 1.0], 12.0),
        (Kind::Crystal, 293, 9) => (0.1, [0.104_528_464, 0.0, 0.994_521_9], 12.0),
        (Kind::Tower, 300, 5) => (0.0, [0.965_925_8, 0.0, 0.258_819_07], 1.0),
        (Kind::Tower, 300, 13) => (0.0, [-0.965_925_8, 0.0, 0.258_819_07], 1.0),
        (Kind::Tower, 300, 6 | 7) => (0.0, [1.0, 0.0, 0.0], 1.0),
        (Kind::Tower, 300, 14 | 15) => (0.0, [-1.0, 0.0, 0.0], 1.0),
        (Kind::Tower, 301, 3 | 4 | 11 | 12) => {
            let x = if entity.team == 0 {
                0.766_044_44
            } else {
                -0.766_044_44
            };
            (0.0, [x, 0.0, -0.642_787_64], 1.0)
        }
        _ => return None,
    };
    if !sane(entity) {
        return None;
    }
    let mut body = base(entity, STRUCTURE_SKIN, slot);
    floats(
        &mut body,
        12,
        [entity.position.x, height, entity.position.y],
    );
    floats(&mut body, 24, forward);
    floats(&mut body, 36, [entity.hp, entity.max_hp]);
    floats(&mut body, 84, [profile_word84]);
    if entity.kind == Kind::Tower {
        floats(&mut body, 64, [1.0]);
    }
    body[88..104].fill(1);
    body[97 + usize::from(entity.team)] = 15;
    body[119] = 1;
    body[120] = u8::MAX;
    Some(Message::new(0x03f2, body))
}

const FIRST_WAVE_SLOT: usize = 22;
const WAVE_SLOTS: usize = 200 - FIRST_WAVE_SLOT;

pub(super) struct WaveSlots {
    slots: [Option<WaveActor>; WAVE_SLOTS],
    reported_full: bool,
}

#[derive(Clone, Copy)]
struct WaveActor {
    id: u32,
    exposed: u8,
}

impl Default for WaveSlots {
    fn default() -> Self {
        Self {
            slots: [None; WAVE_SLOTS],
            reported_full: false,
        }
    }
}

impl WaveSlots {
    pub fn declaration(
        &mut self,
        world: &World,
        entity: &Entity,
        picks: &[Pick],
        data: Option<&halcyon_data::GameData>,
        team: u8,
        fresh_wave: bool,
    ) -> Option<Message> {
        if !sane(entity) {
            return None;
        }
        if entity.kind == Kind::Hero {
            return Some(point_move::full_spawn(entity, picks, data, true));
        }
        if entity.kind != Kind::Minion {
            return structure(world, entity);
        }
        let existing = self
            .slots
            .iter()
            .position(|slot| slot.is_some_and(|old| old.id == entity.id));
        let Some(index) = existing.or_else(|| self.slots.iter().position(Option::is_none)) else {
            if !std::mem::replace(&mut self.reported_full, true) {
                tracing::warn!("ARAL actor slots exhausted; deferring minion declaration");
            }
            return None;
        };
        let actor = self.slots[index].get_or_insert(WaveActor {
            id: entity.id,
            exposed: 0,
        });
        actor.exposed |= 1 << team;
        let slot = (FIRST_WAVE_SLOT + index) as u8;
        match (
            fresh_wave,
            entity.type_id,
            entity.level,
            entity.hp == entity.max_hp,
        ) {
            (true, 287..=289, 2..=4, true) => Some(pristine_minion(entity, slot)),
            _ => {
                let mut full = messages::spawn(entity, picks, data);
                if matches!(entity.type_id, 287..=289) {
                    full.body[4..8].copy_from_slice(&MINION_SKIN.to_be_bytes());
                }
                full.body[745] = slot;
                Some(full)
            }
        }
    }

    /// Reuse only after every team to which this actor was sent got its death.
    pub fn death_sent(&mut self, id: u32, team: u8) {
        if let Some(slot) = self
            .slots
            .iter_mut()
            .find(|slot| slot.is_some_and(|actor| actor.id == id))
            && let Some(actor) = slot.as_mut()
        {
            actor.exposed &= !(1 << team);
            if actor.exposed == 0 {
                *slot = None;
            }
        }
    }
}

fn pristine_minion(entity: &Entity, slot: u8) -> Message {
    let mut body = base(entity, MINION_SKIN, slot);
    floats(&mut body, 12, [entity.position.x, 0.0, entity.position.y]);
    floats(&mut body, 24, [0.0, 0.0, 1.0]);
    body[97 + usize::from(entity.team)] = 1;
    body[117] = 1;
    body[119] = entity.level;
    body[120] = entity.team;
    Message::new(0x03f2, body)
}

pub(super) fn recordable(body: &[u8]) -> bool {
    if body.len() != 122 || body[112..116] != [255; 4] {
        return false;
    }
    let template = u32::from_be_bytes(body[..4].try_into().unwrap());
    match (template, &body[116..122]) {
        (292, [1, 0, 0, 1, 255, 1])
        | (293, [9, 0, 0, 1, 255, 2])
        | (300, [5..=7, 0, 0, 1, 255, 1] | [13..=15, 0, 0, 1, 255, 2])
        | (301, [3..=4, 0, 0, 1, 255, 1] | [11..=12, 0, 0, 1, 255, 2]) => {
            body[4..8] == STRUCTURE_SKIN.to_be_bytes()
        }
        (287..=289, [22..=199, 1, 0, 2..=4, 0, 1] | [22..=199, 1, 0, 2..=4, 1, 2]) => {
            body[4..8] == MINION_SKIN.to_be_bytes() && body[36..88] == [0; 52]
        }
        _ => false,
    }
}

fn sane(entity: &Entity) -> bool {
    (1..=u32::from(u16::MAX) - 1500).contains(&entity.id)
        && entity.team < 2
        && entity.position.x.is_finite()
        && entity.position.y.is_finite()
        && entity.hp.is_finite()
        && entity.max_hp.is_finite()
        && entity.hp > 0.0
        && entity.hp <= entity.max_hp
}

fn base(entity: &Entity, skin: u32, slot: u8) -> [u8; 122] {
    let mut body = [0; 122];
    body[..4].copy_from_slice(&u32::from(entity.type_id).to_be_bytes());
    body[4..8].copy_from_slice(&skin.to_be_bytes());
    body[8..12].copy_from_slice(&u32::from(wire_id(entity.id)).to_be_bytes());
    body[112..116].fill(u8::MAX);
    body[116] = slot;
    body[121] = entity.team + 1;
    body
}

fn floats<const N: usize>(body: &mut [u8], offset: usize, values: [f32; N]) {
    for (word, value) in body[offset..offset + N * 4].chunks_exact_mut(4).zip(values) {
        word.copy_from_slice(&value.to_be_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pristine_wave_uses_compact_and_damaged_snapshot_keeps_the_same_slot() {
        let mut world = World::new(1, 6);
        world.use_six_human_roster().unwrap();
        world.assign_teams(&[]).unwrap();
        world.use_aral_lane().unwrap();
        for _ in 0..15 * halcyon_sim::TICK_HZ {
            world.step();
        }
        let mut wave = world.entities.iter().filter(|e| e.kind == Kind::Minion);
        let first = wave.next().unwrap().clone();
        let second = wave.next().unwrap().clone();
        assert_eq!(first.type_id, 289);
        assert_eq!(second.type_id, 289);
        let mut slots = WaveSlots::default();
        for entity in [&first, &second] {
            let fresh = slots
                .declaration(&world, entity, &[], None, 0, true)
                .unwrap();
            assert_eq!(fresh.opcode, 0x03f2);
            assert!(recordable(&fresh.body));
            let mut malformed = fresh.body.clone();
            malformed[120] ^= 1;
            assert!(!recordable(&malformed));
            assert_eq!(fresh.body[116], 22 + entity.team);
            assert_eq!(fresh.body[117], 1);
            assert_eq!(fresh.body[119], 2);
            assert_eq!(fresh.body[120], entity.team);
            assert_eq!(fresh.body[12..16], entity.position.x.to_be_bytes());
            assert_eq!(fresh.body[20..24], entity.position.y.to_be_bytes());
        }
        let mut damaged = first.clone();
        damaged.hp -= 14.0;
        let restored = slots
            .declaration(&world, &damaged, &[], None, 1, false)
            .unwrap();
        assert_eq!(restored.opcode, 0x03f3);
        assert_eq!(restored.body[745], 22);
        assert_eq!(restored.body[42..46], damaged.hp.to_be_bytes());
        assert_eq!(restored.body[46..50], damaged.max_hp.to_be_bytes());

        slots.death_sent(first.id, 0);
        let mut later = first.clone();
        later.id += 100;
        let before_other_team = slots
            .declaration(&world, &later, &[], None, 0, true)
            .unwrap();
        assert_eq!(before_other_team.body[116], 24);
        slots.death_sent(first.id, 1);
        later.id += 1;
        let after_both_teams = slots
            .declaration(&world, &later, &[], None, 0, true)
            .unwrap();
        assert_eq!(after_both_teams.body[116], 22);
    }
}
