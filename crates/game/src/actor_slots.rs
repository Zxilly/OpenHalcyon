use halcyon_protocol::{EntitySpawnBody, HeroSpawnBody, Message};
use halcyon_sim::{Entity, Kind, World};

use super::{Pick, compact, messages};

const FIRST_MINION_SLOT: u8 = 52;
const ACTOR_SLOTS: u8 = 200;

const RIVER_LEFT_SLOT_KEY: u32 = u32::MAX - 1;
const RIVER_RIGHT_SLOT_KEY: u32 = u32::MAX;

pub(super) struct ActorSlots {
    heroes: [Option<u8>; 10],
    minions: [Option<u32>; (ACTOR_SLOTS - FIRST_MINION_SLOT) as usize],
    awaiting_destroy: [bool; (ACTOR_SLOTS - FIRST_MINION_SLOT) as usize],
    reported_full: bool,
}

fn spawn_with_slot(message: &Message, slot: u8) -> Message {
    let mut body =
        HeroSpawnBody::read_body(&message.body).expect("messages::spawn emits 750B HeroSpawn");
    body.slot = slot;
    body.encode()
}

impl ActorSlots {
    pub fn from_roster(entities: &halcyon_sim::Actors<'_>) -> Self {
        let mut heroes = [None; 10];
        let mut next_slot = [14, 19];
        for hero in entities.iter().filter(|entity| entity.kind == Kind::Hero) {
            let team = usize::from(hero.team);
            if (1..=10).contains(&hero.id) && team < 2 && next_slot[team] < [19, 24][team] {
                heroes[(hero.id - 1) as usize] = Some(next_slot[team]);
                next_slot[team] += 1;
            }
        }
        Self {
            heroes,
            minions: [None; (ACTOR_SLOTS - FIRST_MINION_SLOT) as usize],
            awaiting_destroy: [false; (ACTOR_SLOTS - FIRST_MINION_SLOT) as usize],
            reported_full: false,
        }
    }

    pub(super) fn fixed(&self, entity: &Entity) -> Option<u8> {
        match (entity.kind, entity.type_id, entity.team) {
            (Kind::Hero, _, _) if (1..=10).contains(&entity.id) => {
                self.heroes[(entity.id - 1) as usize]
            }
            _ => compact::structure_slot(entity),
        }
    }

    pub fn declaration(
        &mut self,
        world: &World,
        entity: &Entity,
        picks: &[Pick],
        data: Option<&halcyon_data::GameData>,
    ) -> Option<Message> {
        self.declaration_at(world, entity, picks, data, false)
    }

    pub fn declaration_at(
        &mut self,
        world: &World,
        entity: &Entity,
        picks: &[Pick],
        data: Option<&halcyon_data::GameData>,
        fresh: bool,
    ) -> Option<Message> {
        if !(1..=u32::from(u16::MAX) - 1500).contains(&entity.id) {
            return None;
        }
        if matches!(entity.kind, Kind::Minion | Kind::Neutral) {
            if let Some(slot) = self.minion(entity.id) {
                if entity.kind == Kind::Neutral {
                    return compact::camp_unit(entity, slot);
                }
                return Some(compact::minion(entity, slot, fresh).unwrap_or_else(|| {
                    let message = messages::spawn(entity, picks, data);
                    spawn_with_slot(&message, slot)
                }));
            }
            if !std::mem::replace(&mut self.reported_full, true) {
                tracing::warn!("native Solo actor slots exhausted; deferring minion declaration");
            }
            return None;
        }
        let slot = self.fixed(entity)?;
        Some(compact::structure(world.tick, entity).unwrap_or_else(|| {
            let raw = messages::spawn(entity, picks, data);
            let mut body =
                HeroSpawnBody::read_body(&raw.body).expect("messages::spawn emits 750B HeroSpawn");
            if entity.kind == Kind::Tower {
                // Use the stationary tower skin and terrain height. Dynamic aim
                // and timer words from replay snapshots are not authoritative here.
                body.skin = EntitySpawnBody::STRUCTURE_SKIN;
                body.position[1] = f32::from_bits(EntitySpawnBody::GROUND_HEIGHT_BITS);
                body.forward[0] = if entity.team == 0 { 1. } else { -1. };
            }
            if entity.kind == Kind::Armory {
                let profile = halcyon_protocol::SoloStructure::at_slot(slot)
                    .expect("fixed Solo armory slot")
                    .spawn_body(u32::from(messages::wire_id(entity.id)));
                body.skin = profile.skin;
                body.position[1] = profile.position[1];
                body.forward = profile.forward;
            }
            if entity.kind == Kind::Hero {
                body.apply_solo_profile(entity.bot, entity.team, world.tick);
                super::hero_progress::set_spawn(world, &mut body, entity);
            }
            body.slot = slot;
            body.encode()
        }))
    }

    pub(super) fn alloc_dynamic(&mut self, id: u32) -> Option<u8> {
        if !(1..=u32::from(u16::MAX) - 1500).contains(&id) {
            return None;
        }
        let slot = self.minion(id);
        if slot.is_none() && !std::mem::replace(&mut self.reported_full, true) {
            tracing::warn!("native Solo actor slots exhausted; deferring minion declaration");
        }
        slot
    }

    pub(super) fn assigned_dynamic(&self, id: u32) -> Option<u8> {
        self.minions
            .iter()
            .position(|slot| *slot == Some(id))
            .map(|index| FIRST_MINION_SLOT + index as u8)
    }

    fn minion(&mut self, id: u32) -> Option<u8> {
        let index = self
            .minions
            .iter()
            .position(|slot| *slot == Some(id))
            .or_else(|| self.minions.iter().position(Option::is_none))?;
        self.minions[index] = Some(id);
        Some(FIRST_MINION_SLOT + index as u8)
    }

    pub fn river_shop_slot(&mut self, right: bool) -> Option<u8> {
        let key = if right {
            RIVER_RIGHT_SLOT_KEY
        } else {
            RIVER_LEFT_SLOT_KEY
        };
        self.minion(key)
    }

    pub fn defer_destroy(&mut self, id: u32) {
        if let Some(index) = self.minions.iter().position(|slot| *slot == Some(id)) {
            self.awaiting_destroy[index] = true;
        }
    }

    pub fn destroy_sent(&mut self, id: u32) {
        if let Some(index) = self.minions.iter().position(|slot| *slot == Some(id)) {
            self.minions[index] = None;
            self.awaiting_destroy[index] = false;
        }
    }

    pub fn released(&mut self, id: u32) {
        if let Some(index) = self.minions.iter().position(|slot| *slot == Some(id))
            && !self.awaiting_destroy[index]
        {
            self.minions[index] = None;
        }
    }

    /// A new scene gets a fresh client table. Preserve indices of living actors.
    pub fn retain_living(&mut self, entities: &halcyon_sim::Actors<'_>) {
        for (slot, awaiting) in self.minions.iter_mut().zip(&mut self.awaiting_destroy) {
            if slot.is_some_and(|id| !entities.iter().any(|e| e.id == id && e.hp > 0.0)) {
                *slot = None;
                *awaiting = false;
            }
        }
    }
}

#[cfg(test)]
#[path = "actor_slots_tests.rs"]
mod tests;
