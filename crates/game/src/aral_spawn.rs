use halcyon_data::EntityTemplate;
use halcyon_protocol::Message;
use halcyon_sim::{Entity, Kind, World};

use super::{Pick, messages, messages::wire_id, point_move};

const STRUCTURE_SKIN: u32 = 0xc10b41da;
const MINION_SKIN: u32 = 0xeb39ce55;

/// One actual ARAL tower or crystal, registered at its observed map slot.
pub(super) fn structure(world: &World, entity: &Entity) -> Option<Message> {
    let slot = world.aral_structure_slot(entity.id)?;
    let (height, forward, profile_word84) =
        match (entity.kind, EntityTemplate::from_raw(entity.type_id)?, slot) {
            (Kind::Crystal, EntityTemplate::VainCrystalHome3v3, 1) => (0.0, [0.0, 0.0, 1.0], 12.0),
            (Kind::Crystal, EntityTemplate::VainCrystalAway3v3, 9) => {
                (0.1, [0.104_528_464, 0.0, 0.994_521_9], 12.0)
            }
            (Kind::Tower, EntityTemplate::Turret, 5) => {
                (0.0, [0.965_925_8, 0.0, 0.258_819_07], 1.0)
            }
            (Kind::Tower, EntityTemplate::Turret, 13) => {
                (0.0, [-0.965_925_8, 0.0, 0.258_819_07], 1.0)
            }
            (Kind::Tower, EntityTemplate::Turret, 6 | 7) => (0.0, [1.0, 0.0, 0.0], 1.0),
            (Kind::Tower, EntityTemplate::Turret, 14 | 15) => (0.0, [-1.0, 0.0, 0.0], 1.0),
            (Kind::Tower, EntityTemplate::VainTurret, 3 | 4 | 11 | 12) => {
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
    Some(Message::known(crate::Opcode::Snapshot, body))
}

/// Fixed map scenery. These actors have no simulation, vision, or economic effect.
pub(super) fn scenery() -> impl Iterator<Item = Message> {
    [0, 2, 8, 10]
        .into_iter()
        .filter_map(scenery_body)
        .map(|body| Message::known(crate::Opcode::Snapshot, body))
}

fn scenery_body(slot: u8) -> Option<[u8; 122]> {
    let (template, position, direction, hp) = match slot {
        0 => (
            EntityTemplate::NeutralJungleMinionStore,
            [-88.5, 0.89, 2.0],
            [0.766_044_44, 0.0, -0.642_787_64],
            868.0,
        ),
        8 => (
            EntityTemplate::NeutralJungleMinionStore,
            [88.565, 1.8, 0.51],
            [0.0, 0.0, 1.0],
            868.0,
        ),
        2 => (
            EntityTemplate::VisionBeacon,
            [-76.12, 0.0, 19.9],
            [0.0, 0.0, 1.0],
            9999.0,
        ),
        10 => (
            EntityTemplate::VisionBeacon,
            [76.12, 0.1, 19.9],
            [0.104_528_464, -0.0, 0.994_521_9],
            9999.0,
        ),
        _ => return None,
    };
    let team = slot / 8;
    let mut body = [0; 122];
    body[..4].copy_from_slice(&u32::from(template.as_raw()).to_be_bytes());
    body[4..8].copy_from_slice(&STRUCTURE_SKIN.to_be_bytes());
    body[8..12].copy_from_slice(&(2000 + u32::from(slot)).to_be_bytes());
    floats(&mut body, 12, position);
    floats(&mut body, 24, direction);
    floats(&mut body, 36, [hp, hp]);
    floats(&mut body, 84, [12.0]);
    body[88..96].fill(1);
    if template == EntityTemplate::NeutralJungleMinionStore {
        floats(&mut body, 52, [2.5]);
        floats(&mut body, 64, [1.0]);
        body[96..104].fill(3);
    }
    body[97 + usize::from(team)] = 15;
    body[112..116].fill(255);
    body[116] = slot;
    body[119] = 1;
    body[120] = 255;
    body[121] = team + 1;
    Some(body)
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
            return Some(point_move::full_spawn(world, entity, picks, data, true));
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
        let native_minion = EntityTemplate::from_raw(entity.type_id)
            .is_some_and(EntityTemplate::is_aral_lane_minion);
        if fresh_wave
            && native_minion
            && (2..=4).contains(&entity.level)
            && entity.hp == entity.max_hp
        {
            Some(pristine_minion(entity, slot))
        } else {
            let mut full = messages::spawn(entity, picks, data);
            if native_minion {
                full.body[4..8].copy_from_slice(&MINION_SKIN.to_be_bytes());
            }
            full.body[745] = slot;
            Some(full)
        }
    }

    /// Reuse only after the delayed destroy for every team that saw the actor.
    pub fn destroy_sent(&mut self, id: u32, team: u8) {
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
    Message::known(crate::Opcode::Snapshot, body)
}

/// Established visibility deltas for a newly declared, pristine ARAL wave actor.
/// The opposing index varies even for identical declarations.
#[derive(Clone, Copy)]
pub(super) struct InitialVisibility {
    actor: u32,
    team: u8,
    changed: u8,
}

impl InitialVisibility {
    pub(super) fn read(declaration: &Message) -> Option<Self> {
        let b = &declaration.body;
        if !declaration.is(crate::Opcode::Snapshot)
            || b.len() != 122
            || b[117] != 1
            || !recordable(b)
        {
            return None;
        }
        let actor = u32::from_be_bytes(b[8..12].try_into().unwrap());
        let team = b[121];
        let mut changed = 0;
        for index in 0..8u8 {
            if index == 3 - team {
                continue;
            }
            let i = usize::from(index);
            let mask = if index == team { 15 } else { 0 };
            if [b[88 + i], b[96 + i], b[104 + i]] != [1, mask, 0] {
                changed |= 1 << index;
            }
        }
        (changed != 0).then_some(Self {
            actor,
            team,
            changed,
        })
    }
    pub(super) fn rows(self) -> impl Iterator<Item = Message> {
        (0..8)
            .filter(move |&index| self.changed & (1 << index) != 0)
            .map(move |index| {
                let mask = if index == self.team { 15 } else { 0 };
                Message::actor_visibility(self.actor, index, 1, [mask, 0])
            })
    }
}

pub(super) fn recordable(body: &[u8]) -> bool {
    if body.len() != 122 || body[112..116] != [255; 4] {
        return false;
    }
    let template = u32::from_be_bytes(body[..4].try_into().unwrap());
    let Some(template) = u16::try_from(template)
        .ok()
        .and_then(EntityTemplate::from_raw)
    else {
        return false;
    };
    if matches!(
        template,
        EntityTemplate::VisionBeacon | EntityTemplate::NeutralJungleMinionStore
    ) {
        return scenery_body(body[116]).is_some_and(|scenery| body == scenery);
    }
    match (template, &body[116..122]) {
        (EntityTemplate::VainCrystalHome3v3, [1, 0, 0, 1, 255, 1])
        | (EntityTemplate::VainCrystalAway3v3, [9, 0, 0, 1, 255, 2])
        | (EntityTemplate::Turret, [5..=7, 0, 0, 1, 255, 1] | [13..=15, 0, 0, 1, 255, 2])
        | (EntityTemplate::VainTurret, [3..=4, 0, 0, 1, 255, 1] | [11..=12, 0, 0, 1, 255, 2]) => {
            body[4..8] == STRUCTURE_SKIN.to_be_bytes()
                && body[8..12] == (2000 + u32::from(body[116])).to_be_bytes()
        }
        (kind, [22..=199, 1, 0, 2..=4, 0, 1] | [22..=199, 1, 0, 2..=4, 1, 2])
            if kind.is_aral_lane_minion() =>
        {
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
    for (word, value) in body[offset..offset + N * 4]
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(values)
    {
        word.copy_from_slice(&value.to_be_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_scenery_encodes_only_the_observed_profiles() {
        let messages: Vec<_> = scenery().collect();
        assert_eq!(messages.len(), 4);
        for (message, (slot, template, position, hp)) in messages.into_iter().zip([
            (0, 314u32, [-88.5_f32, 0.89, 2.0], 868.0_f32),
            (2, 298, [-76.12, 0.0, 19.9], 9999.0),
            (8, 314, [88.565, 1.8, 0.51], 868.0),
            (10, 298, [76.12, 0.1, 19.9], 9999.0),
        ]) {
            let mut b = message.body;
            assert_eq!(message.opcode, crate::wire_opcode::Snapshot);
            assert!(recordable(&b));
            assert_eq!(b[..4], template.to_be_bytes());
            assert_eq!(b[8..12], (2000 + u32::from(slot)).to_be_bytes());
            assert_eq!(
                b[12..24],
                position
                    .into_iter()
                    .flat_map(f32::to_be_bytes)
                    .collect::<Vec<_>>()
            );
            assert_eq!(b[36..40], hp.to_be_bytes());
            assert_eq!(b[40..44], hp.to_be_bytes());
            assert_eq!(b[116..122], [slot, 0, 0, 1, 255, slot / 8 + 1]);
            b[11] ^= 1;
            assert!(!recordable(&b));
        }
    }

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
            assert_eq!(fresh.opcode, crate::wire_opcode::Snapshot);
            assert!(recordable(&fresh.body));
            let team = fresh.body[121];
            let mut prefilled = fresh.clone();
            prefilled.body[88 + 6] = 1;
            let rows: Vec<_> = InitialVisibility::read(&prefilled)
                .unwrap()
                .rows()
                .collect();
            assert_eq!(rows.len(), 6);
            assert!(rows.iter().all(|m| m.body[4] != 6 && m.body[4] != 3 - team));
            assert!(rows.iter().any(|m| m.body[4..8] == [team, 1, 15, 0]));

            let mut wide_template = fresh.body.clone();
            wide_template[0] = 1;
            assert!(!recordable(&wide_template));
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
        assert_eq!(restored.opcode, crate::wire_opcode::EntitySpawn);
        assert!(InitialVisibility::read(&restored).is_none());
        assert_eq!(restored.body[745], 22);
        assert_eq!(restored.body[42..46], damaged.hp.to_be_bytes());
        assert_eq!(restored.body[46..50], damaged.max_hp.to_be_bytes());

        let mut custom = first.clone();
        custom.type_id = u16::MAX;
        let custom_actor = slots
            .declaration(&world, &custom, &[], None, 0, true)
            .unwrap();
        assert!(custom_actor.is(crate::Opcode::EntitySpawn));
        assert_eq!(&custom_actor.body[2..4], &custom.type_id.to_be_bytes());
        slots.destroy_sent(first.id, 0);
        let mut later = first.clone();
        later.id += 100;
        let before_other_team = slots
            .declaration(&world, &later, &[], None, 0, true)
            .unwrap();
        assert_eq!(before_other_team.body[116], 24);
        slots.destroy_sent(first.id, 1);
        later.id += 1;
        let after_both_teams = slots
            .declaration(&world, &later, &[], None, 0, true)
            .unwrap();
        assert_eq!(after_both_teams.body[116], 22);
    }
}
