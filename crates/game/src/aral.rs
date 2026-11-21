pub(super) mod announcements;
use super::{Batch, Message, Room, messages};

pub(super) use halcyon_data::LEGACY_ARAL_SEATS as SEATS;
pub(super) const DRAFT_SECONDS: f32 = 20.;
const MODE: &str = halcyon_protocol::ce::NativeMode::Aral.asset();

/// The level belongs to the reserved seat and selected hero. The client supplies only a slot.
pub(super) fn talent_response(
    room: &mut Room,
    seat: usize,
    channel: usize,
    selection: halcyon_protocol::TalentSelection,
) -> Batch {
    use halcyon_protocol::{EquippedTalent, TalentSelection};
    let session = &room.seats[seat];
    if !room.legacy_aral()
        || channel != 0
        || session.peers[0].is_none()
        || !session.world_actor_written
        || !session.preamble.confirmed()
        || room.phase != crate::RoomPhase::Running
    {
        return Vec::new();
    }
    let hero = &room.world.entities[seat];
    let Some(level) = super::authorize_talent_level(room, seat, hero.type_id, selection) else {
        return Vec::new();
    };
    let actor = hero.id;
    let hero_type_id = hero.type_id;
    // One independently identified Mobile Bulwark carried slot 1 and level 10.
    // The reserved level is home-authorized. Other native selections still have only numeric evidence.
    if room.unlock_all_talents
        && let Some(data) = room.data.as_deref()
    {
        let wire_slot = match selection {
            TalentSelection::Slot0 => Some(0u8),
            TalentSelection::Slot1 => Some(1u8),
            TalentSelection::Slot2 => Some(2u8),
            TalentSelection::NoTalent => None,
        };
        let resolved = wire_slot
            .and_then(|slot| halcyon_sim::World::hero_talent_id(hero_type_id, slot, Some(data)))
            .zip(std::num::NonZeroU8::new(level));
        let _ = room.world.set_named_talent(actor, resolved, data);
    } else {
        if hero_type_id == halcyon_data::EntityTemplate::Lyra.as_raw()
            && let Some(data) = room.data.as_deref()
        {
            let talent = (selection == TalentSelection::Slot1 && level == 10).then(|| {
                (
                    halcyon_data::ModeledTalent::MobileBulwark.as_str(),
                    std::num::NonZeroU8::new(10).unwrap(),
                )
            });
            // Older content packages can still serve the numeric equip response.
            let _ = room.world.set_named_talent(actor, talent, data);
        }
        let hero = &room.world.entities[seat];
        if hero.type_id == halcyon_data::EntityTemplate::Gwen.as_raw()
            && let Some(data) = room.data.as_deref()
        {
            let talent = (selection == TalentSelection::Slot0 && level == 1).then(|| {
                (
                    halcyon_data::ModeledTalent::SpeedLoader.as_str(),
                    std::num::NonZeroU8::MIN,
                )
            });
            let _ = room.world.set_named_talent(actor, talent, data);
        }
    }
    let hero = &room.world.entities[seat];
    let equipped = EquippedTalent {
        actor: u32::from(messages::wire_id(hero.id)),
        selection,
        level,
    };
    let batch = vec![
        selection.request(),
        equipped.message().expect("validated numeric equip"),
    ];
    if room.seats[seat].equipped_talent.replace(equipped) != Some(equipped) {
        let shared = super::transport::TeamBatch::new(batch.clone());
        for (i, other) in room.seats.iter().enumerate() {
            if i != seat
                && let Some(peer) = other.gameplay_peer()
            {
                peer.send(super::transport::Delivery::Team(shared.clone()));
            }
        }
        room.publish_observers(&batch);
    }
    batch
}

pub(super) fn talent_snapshot(room: &Room) -> impl Iterator<Item = Message> + '_ {
    room.seats
        .iter()
        .filter_map(|s| s.equipped_talent?.message().ok())
}

pub(super) fn roster(room: &Room, remaining: f32, private: bool) -> Message {
    use halcyon_protocol::{DraftRosterBody, DraftSlotBody, FixedAscii};
    let slots = std::array::from_fn(|index| {
        let mut slot = DraftSlotBody::empty_with_skin(index as u8, DraftSlotBody::ARAL_EMPTY_SKIN);
        if let (Some(player), Some(pick)) = (room.players.get(index), room.picks.get(index)) {
            let actor = &room.world.entities[index];
            slot.occupied = 1;
            slot.set_preferences(pick.preferences.to_wire());
            slot.selected = u8::from(pick.selected);
            slot.locked = u8::from(pick.locked);
            slot.team = actor.team + 1;
            slot.template = actor.type_id;
            slot.entity = messages::wire_id(actor.id);
            slot.skin = pick.skin;
            slot.display_name = FixedAscii::from_str_trunc(messages::GUEST_HANDLE);
            if !private {
                slot.identity = FixedAscii::from_str_trunc(&player.to_string());
            }
        }
        slot
    });
    DraftRosterBody::aral(remaining.clamp(0., DRAFT_SECONDS), DRAFT_SECONDS, slots).encode_aral()
}

pub(super) fn setup(room: &Room, seat: usize, remaining: f32) -> Batch {
    let actor = messages::wire_id(seat as u32 + 1);
    let settings = halcyon_protocol::MatchSettingsBody::draft_three(
        u32::from(actor),
        MODE,
        room.world.entities[seat].team + 1,
    );
    let mut batch = vec![settings.encode()];
    batch.extend(draft_catalog(room, remaining));
    batch
}

pub(super) fn draft_catalog(room: &Room, remaining: f32) -> Batch {
    use halcyon_protocol::{DraftCatalogEntryBody, DraftModeAnnouncementBody, FixedAscii};
    let mut batch = Vec::with_capacity(room.catalog.len() + 2);
    batch.push(DraftModeAnnouncementBody::new(3, MODE).encode());
    batch.extend(room.catalog.iter().map(|entry| {
        DraftCatalogEntryBody {
            name: FixedAscii(*entry),
        }
        .encode()
    }));
    batch.push(roster(room, remaining, false));
    batch
}

pub(super) fn reroll(room: &mut Room, seat: usize) -> Batch {
    if !room.free_aral_reroll
        || !room.legacy_aral()
        || room.phase != crate::RoomPhase::HeroSelect
        || room.aral_remaining() <= 0.
        || room.seats[seat].peers[0].is_none()
        || room.seats[seat].locked
        || room.seats[seat].used_aral_reroll
    {
        return Vec::new();
    }
    let Some(data) = &room.data else {
        return Vec::new();
    };
    let candidates = data.hero_names.len();
    if candidates < 2 {
        return Vec::new();
    }
    let template = room.world.entities[seat].type_id;
    // Room identity is the match seed. This rotates its bits by seat so
    // retries and connection changes cannot rerandomize a draft.
    let seed = room.id.as_u128().rotate_right(7 * seat as u32);
    let start = (seed % candidates as u128) as usize;
    let Some(next) = data
        .hero_names
        .iter()
        .cycle()
        .skip(start)
        .take(candidates)
        .find(|name| {
            data.type_id(name).is_some_and(|id| id != template)
                && room.world.select_hero(seat, name, data).is_ok()
        })
    else {
        return Vec::new();
    };
    let skin = data
        .indexed_skin(next, None)
        .unwrap_or_else(|| halcyon_data::skins::default_skin_hash(next));
    room.picks[seat].name.clone_from(next);
    room.picks[seat].skin = skin;
    room.seats[seat].used_aral_reroll = true;
    let echo = Message::known(crate::Opcode::DraftRerollHero, [0]);
    let roster = room.roster_with_echo(Some(seat), Some(&echo));
    vec![
        roster,
        echo,
        halcyon_protocol::DraftAction::SelectSkin {
            skin_id: skin,
            flags: 0,
        }
        .encode(),
    ]
}

pub(super) fn refresh(room: &mut Room, seat: usize) -> Batch {
    let mut batch = draft_catalog(room, room.aral_remaining());
    if !room.seats[seat].draft_sync_sent {
        // The first refresh of all eight recorded ARAL connections sent this reroll state
        // (verified at 0xad4250): cost=1 (+0), limit=5 (+4), used=0 (+8),
        // allow_override=1 (+76), balance=3 (+77), locked=0 (+81).
        batch.push(halcyon_protocol::AralRerollStateBody::new(1, 5, 0, 1, 3, 0).encode());
        batch.push(roster(room, room.aral_remaining(), false));
        room.seats[seat].draft_sync_sent = true;
    }
    batch
}

pub(super) fn opening(room: &mut Room, team: Option<u8>) -> Batch {
    let mut batch: Batch = Vec::new();
    batch.push(messages::scene_mode(&room.queue, 0));
    batch.extend(crate::scene::players(room));
    batch.push(messages::world_clock(room.world.tick));
    let mut map_objects: Vec<_> = super::aral_spawn::scenery().collect();
    let mut lane_minions = Vec::new();
    for actor in &room.world.entities {
        if actor.hp <= 0.0 || team.is_some_and(|side| !room.world.visible(side, actor)) {
            continue;
        }
        if let Some(declaration) = room.aral_slots.declaration(
            &room.world,
            actor,
            &room.picks,
            room.data.as_deref(),
            team.unwrap_or(actor.team),
            false,
        ) {
            match actor.kind {
                halcyon_sim::Kind::Hero => {
                    batch.push(declaration);
                    if room.world.tick == 0 && team.is_none() {
                        super::native_cooldown::opening(
                            &mut batch,
                            actor,
                            halcyon_protocol::ce::NativeMode::Aral,
                        );
                    }
                    messages::append_inventory(&mut batch, &room.world, actor);
                }
                halcyon_sim::Kind::Minion => lane_minions.push(declaration),
                _ => map_objects.push(declaration),
            }
        }
    }
    let spawn_slot = |message: &Message| {
        halcyon_protocol::EntitySpawnBody::read_body(&message.body).map_or(0, |body| body.slot)
    };
    map_objects.sort_unstable_by_key(|m| std::cmp::Reverse(spawn_slot(m)));
    let initial = room.world.tick == 0 && team.is_none();
    let slots = map_objects
        .iter()
        .filter_map(|message| 1_u16.checked_shl(u32::from(spawn_slot(message))))
        .fold(0, |map, mask| map | mask);
    batch.extend(map_objects);
    batch.extend(lane_minions);
    if initial {
        batch.extend(
            (0..16)
                .rev()
                .filter(|slot| slots & (1 << slot) != 0)
                .flat_map(|slot| {
                    crate::structure_effects::Opening::aral(slot)
                        .into_iter()
                        .flat_map(crate::structure_effects::Opening::messages)
                        .chain(crate::aral_guard::opening(slot))
                }),
        );
        batch.extend(super::scene::mode_variables::Opening::Aral.messages());
    } else {
        batch.extend(
            (0..16)
                .rev()
                .filter(|slot| slots & (1 << slot) != 0)
                .filter_map(|slot| crate::aral_guard::restore(&room.world, team, slot)),
        );
    }
    batch.extend(room.recommended_paths(team));
    batch.extend(talent_snapshot(room));
    if let Some(team) = team {
        batch.extend(room.surrender_snapshot(team));
    }
    batch
}

#[cfg(test)]
mod reroll_tests;
#[cfg(test)]
mod shop_tests;
#[cfg(test)]
mod tests;
