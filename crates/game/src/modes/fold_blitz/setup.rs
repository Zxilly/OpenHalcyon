//! Mode classification, static scene slot/skin tables, and world initialization
//! for 3v3 Halcyon Fold (`CasualFold` / `RankedFold`), `Blitz`, and `BattleRoyaleBotsSolo`.

use crate::Room;
use halcyon_data::EntityTemplate;
use halcyon_protocol::{EntitySpawnBody, ce::KnownQueue};
use halcyon_sim::{Entity, Kind, World};

pub(super) const SCENERY_ACTOR_BASE: u32 = 2000;
pub(super) const JUNGLE_SHOP_SLOT: u8 = 16;
pub(super) const FIRST_HERO_SLOT: u8 = 17;
pub(super) const FIRST_DYNAMIC_SLOT: u8 = 52;
pub(super) const ACTOR_SLOT_LIMIT: u8 = 200;

pub(super) const BUFF_INVULNERABLE: u16 = 27;
pub(super) const BUFF_GLOBALLY_VISIBLE: u16 = 66;
pub(super) const BUFF_SHOP_GRANT_JUNGLE: u16 = 175;
pub(super) const BUFF_SHOP_GRANT_BASE_3V3: u16 = 176;
pub(super) const PERMANENT_DURATION_BITS: u16 = 0xbc00;

pub(crate) const fn non_hero_skin(template: EntityTemplate) -> Option<u32> {
    match template {
        EntityTemplate::RangedMinion | EntityTemplate::LeadMinion | EntityTemplate::TankMinion => {
            Some(EntitySpawnBody::MINION_SKIN)
        }
        EntityTemplate::VainCrystalHome3v3
        | EntityTemplate::VainCrystalAway3v3
        | EntityTemplate::VisionBeacon
        | EntityTemplate::Turret
        | EntityTemplate::VainTurret
        | EntityTemplate::NeutralJungleMinionStore
        | EntityTemplate::RiverShop5v5
        | EntityTemplate::HfBaseTurret
        | EntityTemplate::HfMiddleTurret
        | EntityTemplate::HfOuterTurret
        | EntityTemplate::HfVainTurret => Some(EntitySpawnBody::STRUCTURE_SKIN),
        EntityTemplate::JungleMinionGoldMiner
        | EntityTemplate::JungleMinionCrystalMiner
        | EntityTemplate::BlitzMiddleSentry
        | EntityTemplate::BlitzAllySentry
        | EntityTemplate::JungleShopCenter
        | EntityTemplate::BlitzJungleShopCenter
        | EntityTemplate::KrakenJungle
        | EntityTemplate::KrakenCaptured
        | EntityTemplate::HfTreant
        | EntityTemplate::HfElderTreant
        | EntityTemplate::HfBigBear
        | EntityTemplate::HfSmallBear
        | EntityTemplate::HfCrystalMiner
        | EntityTemplate::HfGoldMiner
        | EntityTemplate::HfKrakenJungle
        | EntityTemplate::HfKrakenCaptured => Some(EntitySpawnBody::JUNGLE_MINION_SKIN),
        _ => None,
    }
}

pub(crate) fn hero_slot(entity: &Entity) -> Option<u8> {
    (entity.kind == Kind::Hero && (1..=6).contains(&entity.id))
        .then(|| FIRST_HERO_SLOT - 1 + entity.id as u8)
}

pub(crate) fn is_halcyon_fold(queue: Option<KnownQueue>) -> bool {
    queue.is_some_and(|q| q.mode().is_halcyon_fold())
}

pub(crate) fn is_blitz(queue: Option<KnownQueue>) -> bool {
    queue.is_some_and(|q| q.mode().is_blitz())
}

pub(crate) fn is_managed(queue: Option<KnownQueue>) -> bool {
    is_halcyon_fold(queue) || is_blitz(queue)
}

/// Retains non-hero entities and the first `hero_seats` hero entities (`1` human + AI bots)
/// so 3-per-side solo/bot rooms (`6` heroes total) satisfy Fold/Blitz/ARAL team validation.
pub(crate) fn trim_to_three_per_side_roster(
    world: &mut World,
    hero_seats: usize,
) -> Result<(), String> {
    if world.tick != 0 {
        return Err("roster trim requires an unstarted world".into());
    }
    let mut kept_heroes = 0usize;
    world.entities.retain(|entity| {
        if entity.kind != Kind::Hero {
            return true;
        }
        if kept_heroes < hero_seats {
            kept_heroes += 1;
            true
        } else {
            false
        }
    });
    if kept_heroes != hero_seats {
        return Err("insufficient heroes for three-per-side roster".into());
    }
    Ok(())
}

/// Ensures the authoritative simulation layout (`HalcyonFold`, `Blitz`, `Aral` solo, or 5v5
/// dual-channel `Practice`) and native navmesh / unit data are initialized before Loading / Running.
pub(crate) fn prepare_world(room: &mut Room) -> std::io::Result<()> {
    if room.world.tick != 0 {
        return Ok(());
    }
    if is_halcyon_fold(room.known_queue) {
        room.world
            .use_halcyon_fold()
            .map_err(std::io::Error::other)?;
        if let Some(data) = &room.data {
            room.world
                .install_map_data(data)
                .map_err(std::io::Error::other)?;
        }
    } else if is_blitz(room.known_queue) {
        room.world.use_blitz().map_err(std::io::Error::other)?;
        if let Some(data) = &room.data {
            room.world
                .install_map_data(data)
                .map_err(std::io::Error::other)?;
        }
    } else if room.known_queue == Some(KnownQueue::BattleRoyaleBotsSolo) {
        room.world.use_aral_lane().map_err(std::io::Error::other)?;
        if let Some(data) = &room.data {
            room.world
                .install_map_data(data)
                .map_err(std::io::Error::other)?;
        }
    } else if room.known_queue == Some(KnownQueue::Practice)
        && room.seats.iter().any(|s| s.peers[1].is_some())
        && !room.world.is_5v5_enabled()
        && room.world.tick == 0
    {
        room.world
            .use_sovereigns_rise_5v5(room.data.as_deref())
            .map_err(std::io::Error::other)?;
    }
    Ok(())
}
