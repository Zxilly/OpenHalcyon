//! Native scene negotiation on an authenticated control stream.
pub(crate) mod mode_variables;
use super::{Batch, Message, Room, messages};
#[derive(Default)]
pub(super) struct Preamble {
    stage: usize,
    implicit: bool,
}

impl Preamble {
    const OPS: [u16; 3] = [
        crate::wire_opcode::RequestSceneSnapshot,
        crate::wire_opcode::ConfirmSceneOwner,
        crate::wire_opcode::SceneReadyConfirm,
    ];

    pub fn observe(&mut self, message: &Message, actor: u32) {
        let known = match message.known_opcode() {
            Some(crate::Opcode::RequestSceneSnapshot | crate::Opcode::SceneReadyConfirm) => {
                matches!(
                    message.fields(),
                    Ok(halcyon_protocol::Fields::ControlByte {
                        value: 0,
                        remaining: [0, 0, 0, 0, 0],
                    })
                )
            }
            Some(crate::Opcode::ConfirmSceneOwner) => {
                halcyon_protocol::ConfirmSceneOwnerBody::read_padded(&message.body)
                    .is_ok_and(|(owner, pad)| owner.actor == actor && owner.flag == 1 && pad == 0)
            }
            _ => false,
        };
        if known && Self::OPS.get(self.stage) == Some(&message.opcode) {
            self.stage += 1;
        }
    }

    pub fn confirm_world_input(&mut self) {
        debug_assert!(self.scene_requested() && !self.confirmed());
        self.stage = Self::OPS.len();
        self.implicit = true;
    }
    pub fn take_late_confirmation(&mut self, m: &Message) -> bool {
        m.is(crate::Opcode::SceneReadyConfirm)
            && matches!(
                m.fields(),
                Ok(halcyon_protocol::Fields::ControlByte {
                    value: 0,
                    remaining: [0, 0, 0, 0, 0],
                })
            )
            && std::mem::take(&mut self.implicit)
    }
    pub fn scene_requested(&self) -> bool {
        self.stage >= 2
    }

    pub fn confirmed(&self) -> bool {
        self.stage == Self::OPS.len()
    }
}

/// An already connected draft seat can observe a different reserved seat's first registration.
pub(super) fn announce_aral_registration(room: &Room, entering: usize) {
    let mut recipients = room
        .seats
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != entering)
        .filter_map(|(_, seat)| seat.peers[0].as_ref())
        .peekable();
    if recipients.peek().is_none() {
        return;
    }
    let mut uuid_bytes = [0u8; 64];
    room.players[entering]
        .as_hyphenated()
        .encode_lower(&mut uuid_bytes[..36]);
    let notice = super::transport::TeamBatch::new(vec![
        halcyon_protocol::RegisterPlayerUuidBody {
            player_uuid: halcyon_protocol::FixedAscii(uuid_bytes),
        }
        .encode(),
    ]);
    for peer in recipients {
        peer.send(super::transport::Delivery::Team(notice.clone()));
    }
}

/// An ARAL connection already in the scene can observe another seat joining it.
pub(super) fn announce_aral_entry(room: &Room, entering: usize, registration: &Message) {
    let mut recipients = room
        .seats
        .iter()
        .enumerate()
        .filter(|(index, seat)| {
            *index != entering && seat.world_actor_written && seat.preamble.scene_requested()
        })
        .filter_map(|(_, seat)| seat.peers[0].as_ref())
        .peekable();
    if recipients.peek().is_none() {
        return;
    }
    let actor = messages::wire_id(room.world.entities[entering].id);
    let loaded = halcyon_protocol::PlayerLoadedSceneBody::new(
        messages::GUEST_HANDLE,
        entering as u32,
        u32::from(actor),
    )
    .encode_padded();
    let batch = super::transport::TeamBatch::new(vec![
        loaded,
        Message::known(crate::Opcode::RequestSceneSnapshot, [0; 6]),
        registration.clone(),
    ]);
    for peer in recipients {
        peer.send(super::transport::Delivery::Team(batch.clone()));
    }
}

pub(super) fn players(room: &Room) -> impl Iterator<Item = Message> {
    (0..room.picks.len()).map(|i| messages::info(&room.world, &room.players, &room.picks, i, true))
}
pub(super) fn begin_loading(room: &Room) -> Batch {
    players(room)
        .chain(std::iter::once(Message::known(
            crate::Opcode::DraftTransitionToLoading,
            [0],
        )))
        .collect()
}

pub(super) fn solo_table(room: &Room) -> Message {
    let mut table = halcyon_protocol::SceneReadinessRowsBody::default();
    for (row, actor) in table
        .rows
        .iter_mut()
        .zip(room.world.entities.iter().take(room.picks.len()))
    {
        *row = halcyon_protocol::SceneReadinessRow {
            reference: u32::from(messages::wire_id(actor.id)),
            gate: 1,
            flag: u8::from(actor.bot),
        };
    }
    table.encode()
}
pub(super) fn solo_opening(room: &mut Room, seat: usize) -> std::io::Result<Batch> {
    if let Some(batch) = super::modes_3v3_blitz::solo_opening(room, seat)? {
        return Ok(batch);
    }
    let team = room.world.entities[seat].team;
    let initial = room.phase == crate::RoomPhase::Loading && room.world.tick == 0;
    if initial {
        room.world
            .use_solo_mid_lane()
            .map_err(std::io::Error::other)?;
        if let Some(data) = &room.data {
            room.world
                .install_map_data(data)
                .map_err(std::io::Error::other)?;
        }
    }
    let mut out = vec![
        messages::scene_mode(&room.queue, 1),
        Message::known(crate::Opcode::SceneLoadedDismiss, [0]),
    ];
    out.extend(players(room));
    out.push(messages::world_clock(room.world.tick));
    room.visible[team as usize].clear();
    if !initial {
        room.solo_slots.retain_living(&room.world.entities);
    }
    for e in &room.world.entities {
        if e.hp <= 0. {
            continue;
        }
        let visible = room.world.visible(team, e);
        // The recorded initial scene declares all ten starting heroes. On reconnect
        // live opposing positions are subject to the team's usual visibility.
        if (initial && e.kind == halcyon_sim::Kind::Hero) || (!initial && visible) {
            let Some(message) =
                room.solo_slots
                    .declaration(&room.world, e, &room.picks, room.data.as_deref())
            else {
                continue;
            };
            out.push(message);
            if initial {
                super::native_cooldown::opening(
                    &mut out,
                    e,
                    halcyon_protocol::ce::NativeMode::SoloBots,
                );
            }
            messages::append_inventory(&mut out, &room.world, e);
            if visible {
                room.visible[team as usize].insert(e.id);
                if crate::uses_visibility_toggle(team, e) {
                    out.push(crate::actor_visibility_message(e.id, team, true));
                }
            }
        }
    }
    if initial {
        out.extend(mode_variables::Opening::Solo.messages());
    } else {
        room.solo_structure_snapshot.append(&mut out);
    }
    out.extend(room.recommended_paths(Some(team)));
    Ok(out)
}

/// First authoritative tick, before stepping the pristine world. Mark only structures
/// actually sent to this team; normal replication handles everything else.
pub(super) fn initial_structures(room: &mut Room) {
    let Some(peer) = room.seats[0].gameplay_peer() else {
        return;
    };
    let team = room.world.entities[0].team;
    let known = &mut room.visible[usize::from(team)];
    let mut batch = Vec::new();
    let mut effects = Vec::new();
    for entity in &room.world.entities {
        if !known.contains(&entity.id)
            && room.world.visible(team, entity)
            && let Some(message) = super::compact::structure(room.world.tick, entity)
        {
            known.insert(entity.id);
            if let Ok(spawn) = halcyon_protocol::EntitySpawnBody::read_body(&message.body)
                && let Some(opening) = super::structure_effects::Opening::solo(spawn.slot)
                && opening.actor() == u32::from(messages::wire_id(entity.id))
            {
                effects.push(opening);
            }
            batch.push(message);
        }
    }
    if !batch.is_empty() {
        super::structure_effects::append_solo(&mut batch, &mut effects);
        super::structure_effects::append_crystal_start(&mut batch, &effects);
        if let Some((actor, _, message)) = halcyon_protocol::SoloStructure::base_shopkeeper(team) {
            batch.push(message);
            batch.extend(halcyon_protocol::SoloStructure::shopkeeper_opening_buffs(
                actor,
            ));
            room.scene_views[0].shop_base_declared = true;
        }
        room.scene_views[0].declarations(&batch);
        peer.send(super::transport::Delivery::Private(batch));
    }
}

/// Keeps 5v5 base/river shopkeepers (`329`/`330`) and `Buff_Shop_AllowStorePurchase` (`174`)
/// synchronized on confirmed human seats across both `5v5_bots_solo` and 5v5 PvP.
pub(super) fn sync_5v5_shops(room: &mut Room) {
    if super::modes_3v3_blitz::is_managed(room.known_queue)
        || room.known_queue == Some(halcyon_protocol::ce::KnownQueue::BattleRoyaleBotsSolo)
    {
        return;
    }
    let solo_active = room.solo_scene() && room.seats[0].control_world;
    if !solo_active && !room.world.is_5v5_enabled() {
        return;
    }
    let river_ready = room.world.is_5v5_enabled()
        && room.world.tick >= halcyon_sim::map_5v5::RIVER_SHOP_SPAWN_TICK;
    for seat_idx in 0..room.seats.len() {
        let peer_ch = if room.seats[seat_idx].peers[1].is_some() {
            Some(1usize)
        } else if room.seats[seat_idx].peers[0].is_some()
            && room.seats[seat_idx].world_actor_written
            && room.seats[seat_idx].preamble.confirmed()
        {
            Some(0usize)
        } else {
            None
        };
        let Some(ch) = peer_ch else {
            continue;
        };
        let Some((actor_id, team)) = room
            .world
            .entities
            .get(seat_idx)
            .filter(|e| e.kind == halcyon_sim::Kind::Hero)
            .map(|e| (e.id, e.team))
        else {
            continue;
        };
        let mut batch = Vec::new();
        if !room.scene_views[seat_idx].shop_base_declared
            && let Some((actor, _, message)) =
                halcyon_protocol::SoloStructure::base_shopkeeper(team)
        {
            let mut decl = vec![message];
            decl.extend(halcyon_protocol::SoloStructure::shopkeeper_opening_buffs(
                actor,
            ));
            room.scene_views[seat_idx].declarations(&decl);
            room.scene_views[seat_idx].shop_base_declared = true;
            batch.extend(decl);
        }
        if river_ready && !room.scene_views[seat_idx].shop_river_declared {
            let mut decl = Vec::new();
            for right in [false, true] {
                if let Some(slot) = room.solo_slots.river_shop_slot(right) {
                    let (actor, message) =
                        halcyon_protocol::SoloStructure::river_shopkeeper(right, slot);
                    decl.push(message);
                    decl.extend(halcyon_protocol::SoloStructure::shopkeeper_opening_buffs(
                        actor,
                    ));
                }
            }
            if !decl.is_empty() {
                room.scene_views[seat_idx].declarations(&decl);
                room.scene_views[seat_idx].shop_river_declared = true;
                batch.extend(decl);
            }
        }
        let wire_actor = u32::from(messages::wire_id(actor_id));
        if !room.scene_views[seat_idx].shop_can_icon_sent {
            batch.push(halcyon_protocol::SoloStructure::shop_can_icon_message(
                wire_actor,
                0x10_0000 + seat_idx as u32,
            ));
            room.scene_views[seat_idx].shop_can_icon_sent = true;
        }
        if let Some(keeper_actor) = room.world.shop_5v5_keeper_for_actor(actor_id) {
            let due = room.scene_views[seat_idx].shop_allow_next_tick;
            if due.is_none_or(|tick| room.world.tick >= tick) {
                let seq = room.scene_views[seat_idx].shop_allow_seq;
                room.scene_views[seat_idx].shop_allow_seq = seq.wrapping_add(1);
                let instance = 0x20_0000 + (seat_idx as u32) * 0x1_0000 + (seq & 0xFFFF);
                batch.push(
                    halcyon_protocol::SoloStructure::shop_allow_purchase_message(
                        wire_actor,
                        keeper_actor,
                        instance,
                    ),
                );
                room.scene_views[seat_idx].shop_allow_active_instance = Some(instance);
                room.scene_views[seat_idx].shop_allow_next_tick =
                    Some(room.world.tick + halcyon_sim::TICK_HZ);
            }
        } else if let Some(active_instance) =
            room.scene_views[seat_idx].shop_allow_active_instance.take()
        {
            room.scene_views[seat_idx].shop_allow_next_tick = None;
            if let Ok(cancel) = (halcyon_protocol::BuffMutation {
                target: wire_actor,
                instance: active_instance,
                change: halcyon_protocol::BuffChange::Cancel,
                padding: &[],
            })
            .to_message()
            {
                batch.push(cancel);
            }
        }
        if !batch.is_empty()
            && let Some(peer) = &room.seats[seat_idx].peers[ch]
        {
            peer.send(super::transport::Delivery::Private(batch));
        }
    }
}

#[cfg(test)]
mod tests;
