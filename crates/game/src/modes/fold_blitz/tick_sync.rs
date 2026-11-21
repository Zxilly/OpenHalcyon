//! Shopkeeper authorization sync, 0x041F mode variables, Blitz tick updates,
//! and Blitz talent equip handling for 3v3 Halcyon Fold and `Blitz`.

use super::setup::{is_blitz, is_managed};
use crate::RoomPhase;
use crate::modes::scene::mode_variables::{
    COUNTDOWN_DO_ROUNDING_KEY, COUNTDOWN_DURATION_KEY, COUNTDOWN_PAUSED_KEY, COUNTDOWN_START_KEY,
    TEAM_1_SCORE_KEY, TEAM_2_SCORE_KEY,
};
use crate::replication::{Batch, messages};
use crate::room::{Room, Seat, authorize_talent_level};
use crate::transport;
use halcyon_protocol::{BuffChange, BuffMutation, Flyout, FlyoutText, Message, SoloStructure};
use halcyon_sim::{BlitzTalentSlot, Event, Kind, TICK_HZ, World, map_blitz::BLITZ_DURATION_TICKS};

/// Keeps `Buff_Shop_CanShopIcon` (`173`) and `Buff_Shop_AllowStorePurchase` (`174`, operand `452`
/// for 3v3 HF or `453` for Blitz) synchronized across human seats on both `ch == 0` solo and
/// `ch == 1` PvP connections.
pub(crate) fn sync_shops(room: &mut Room) {
    if !is_managed(room.known_queue) {
        return;
    }
    let store_operand = if is_blitz(room.known_queue) {
        SoloStructure::STORE_BLITZ_OPERAND
    } else {
        SoloStructure::STORE_3V3_OPERAND
    };
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
        let Some(actor_id) = room
            .world
            .entities
            .get(seat_idx)
            .filter(|e| e.kind == Kind::Hero)
            .map(|e| e.id)
        else {
            continue;
        };
        let mut batch = Vec::new();
        let wire_actor = u32::from(messages::wire_id(actor_id));
        if !room.scene_views[seat_idx].shop_can_icon_sent {
            batch.push(SoloStructure::shop_can_icon_message(
                wire_actor,
                0x10_0000 + seat_idx as u32,
            ));
            room.scene_views[seat_idx].shop_can_icon_sent = true;
        }
        if let Some(keeper_actor) = room.world.shop_3v3_blitz_keeper_for_actor(actor_id) {
            let due = room.scene_views[seat_idx].shop_allow_next_tick;
            if due.is_none_or(|tick| room.world.tick >= tick) {
                let seq = room.scene_views[seat_idx].shop_allow_seq;
                room.scene_views[seat_idx].shop_allow_seq = seq.wrapping_add(1);
                let instance = 0x20_0000 + (seat_idx as u32) * 0x1_0000 + (seq & 0xFFFF);
                batch.push(SoloStructure::shop_allow_purchase_for_store(
                    wire_actor,
                    keeper_actor,
                    instance,
                    store_operand,
                ));
                room.scene_views[seat_idx].shop_allow_active_instance = Some(instance);
                room.scene_views[seat_idx].shop_allow_next_tick =
                    Some(room.world.tick + halcyon_sim::TICK_HZ);
            }
        } else if let Some(active_instance) =
            room.scene_views[seat_idx].shop_allow_active_instance.take()
        {
            room.scene_views[seat_idx].shop_allow_next_tick = None;
            if let Ok(cancel) = (BuffMutation {
                target: wire_actor,
                instance: active_instance,
                change: BuffChange::Cancel,
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
            peer.send(transport::Delivery::Private(batch));
        }
    }
}

pub(crate) fn mode_var_message(key: u32, word: u32) -> Message {
    halcyon_protocol::ModifyGameModeVarBody { key, value: word }.encode_padded()
}

pub(crate) fn blitz_mode_var_snapshot(world: &World) -> Vec<Message> {
    let Some(blitz) = world.blitz_state() else {
        return Vec::new();
    };
    let duration_seconds = (BLITZ_DURATION_TICKS / TICK_HZ) as u32;
    vec![
        mode_var_message(COUNTDOWN_START_KEY, 0),
        mode_var_message(COUNTDOWN_DURATION_KEY, duration_seconds),
        mode_var_message(COUNTDOWN_PAUSED_KEY, u32::from(blitz.sudden_death)),
        mode_var_message(COUNTDOWN_DO_ROUNDING_KEY, 0),
        mode_var_message(TEAM_1_SCORE_KEY, blitz.team_points[0]),
        mode_var_message(TEAM_2_SCORE_KEY, blitz.team_points[1]),
    ]
}

/// Emits live `0x041F` team score / pause updates and `0x0440` Blitz score/gameover flyouts.
pub(crate) fn publish_tick(room: &mut Room, events: &[Event]) {
    if !is_blitz(room.known_queue) || room.world.blitz_state().is_none() {
        return;
    }
    let mut frames = Vec::new();
    for ev in events {
        match *ev {
            Event::BlitzPoints {
                team,
                points,
                source,
                total,
                ..
            } => {
                let text = match points {
                    1 => Some(FlyoutText::OnePoint),
                    2 => Some(FlyoutText::TwoPoints),
                    3 => Some(FlyoutText::ThreePoints),
                    _ => None,
                };
                if let Some(text) = text {
                    let anchor = if source > 0 { source } else { 1 };
                    frames.push(
                        Flyout::new(u32::from(messages::wire_id(anchor)), text, total).to_message(),
                    );
                }
                let key = if team == 0 {
                    TEAM_1_SCORE_KEY
                } else {
                    TEAM_2_SCORE_KEY
                };
                frames.push(mode_var_message(key, total));
            }
            Event::BlitzSuddenDeath => {
                frames.push(mode_var_message(COUNTDOWN_PAUSED_KEY, 1));
            }
            Event::End { winner } => {
                frames.push(
                    Flyout::new(
                        u32::from(messages::wire_id(1)),
                        FlyoutText::GameOver,
                        u32::from(winner) + 1,
                    )
                    .to_message(),
                );
            }
            _ => {}
        }
    }
    if frames.is_empty() {
        return;
    }
    let batch = transport::TeamBatch::new(frames.clone());
    for peer in room.seats.iter().filter_map(Seat::gameplay_peer) {
        peer.send(transport::Delivery::Team(batch.clone()));
    }
    room.publish_observers(&frames);
}

/// Authorizes and applies a Blitz `0x0481` -> `0x0484` talent equip on either channel.
pub(crate) fn talent_response(
    room: &mut Room,
    seat: usize,
    selection: halcyon_protocol::TalentSelection,
) -> Batch {
    use halcyon_protocol::{EquippedTalent, TalentSelection};
    if !is_blitz(room.known_queue)
        || room.phase != RoomPhase::Running
        || room.seats[seat].gameplay_peer().is_none()
    {
        return Vec::new();
    }
    let hero = &room.world.entities[seat];
    let Some(level) = authorize_talent_level(room, seat, hero.type_id, selection) else {
        return Vec::new();
    };
    let slot = match selection {
        TalentSelection::Slot0 => BlitzTalentSlot::Slot0,
        TalentSelection::Slot1 => BlitzTalentSlot::Slot1,
        TalentSelection::Slot2 => BlitzTalentSlot::Slot2,
        TalentSelection::NoTalent => BlitzTalentSlot::NoTalent,
    };
    let actor = hero.id;
    if room
        .world
        .equip_blitz_talent(actor, slot, level, room.data.as_deref())
        .is_err()
    {
        return Vec::new();
    }
    let equipped = EquippedTalent {
        actor: u32::from(messages::wire_id(actor)),
        selection,
        level,
    };
    let batch = vec![
        selection.request(),
        equipped.message().expect("validated Blitz talent equip"),
    ];
    if room.seats[seat].equipped_talent.replace(equipped) != Some(equipped) {
        let shared = transport::TeamBatch::new(batch.clone());
        for (i, other) in room.seats.iter().enumerate() {
            if i != seat
                && let Some(peer) = other.gameplay_peer()
            {
                peer.send(transport::Delivery::Team(shared.clone()));
            }
        }
        room.publish_observers(&batch);
    }
    batch
}
