//! Authorize native ARAL slot swaps on the current seat's original world stream.
use super::{Message, Room, messages, transport};
use halcyon_protocol::ItemAction;

#[derive(Default)]
pub(super) struct ProjectedSlots([Option<std::num::NonZeroU32>; halcyon_sim::ARAL_ITEM_CAPACITY]);

pub(super) fn affects_owner(message: &Message, owner: u32) -> bool {
    match message.known_opcode() {
        Some(crate::Opcode::HeroSpawn) => {
            halcyon_protocol::HeroSpawnBody::read_unpadded_or_padded(&message.body)
                .ok()
                .is_some_and(|body| body.actor == owner)
        }
        Some(crate::Opcode::DestroyEntity) => {
            halcyon_protocol::DestroyEntityBody::read_exact_or_padded(&message.body)
                .ok()
                .is_some_and(|body| body.actor == owner)
        }
        Some(crate::Opcode::ItemGrant) => {
            halcyon_protocol::ItemGrantBody::read_exact_or_padded(&message.body)
                .ok()
                .is_some_and(|body| body.actor == owner)
        }
        Some(crate::Opcode::ConsumeItem) => {
            halcyon_protocol::ConsumeItemBody::read_exact_or_padded(&message.body)
                .ok()
                .is_some_and(|body| body.actor == owner)
        }
        _ => false,
    }
}

/// Apply only the owner's authored frames that were actually written on this stream.
pub(super) fn record_delivery(view: &mut Option<ProjectedSlots>, owner: u32, batch: &[Message]) {
    for m in batch.iter().filter(|m| affects_owner(m, owner)) {
        match m.known_opcode() {
            Some(crate::Opcode::HeroSpawn) => {
                view.get_or_insert_with(ProjectedSlots::default);
            }
            Some(crate::Opcode::DestroyEntity) => *view = None,
            Some(crate::Opcode::ItemGrant) => {
                if let (Some(view), Ok(grant)) = (
                    view.as_mut(),
                    halcyon_protocol::ItemGrantBody::read_exact_or_padded(&m.body),
                ) && let Some(instance) = std::num::NonZeroU32::new(grant.instance)
                    && let Some(slot) = view.0.iter_mut().find(|slot| slot.is_none())
                {
                    *slot = Some(instance);
                }
            }
            Some(crate::Opcode::ConsumeItem) => {
                if let (Some(view), Ok(consume)) = (
                    view.as_mut(),
                    halcyon_protocol::ConsumeItemBody::read_exact_or_padded(&m.body),
                ) && let Some(instance) = std::num::NonZeroU32::new(consume.instance)
                    && let Some(slot) = view.0.iter_mut().find(|slot| **slot == Some(instance))
                {
                    *slot = None;
                }
            }
            _ => {}
        }
    }
}

/// Native own-hero redeclarations do not replace the client item component.
/// Omit the attached copy of its inventory when that actor has already been declared.
pub(super) fn without_existing_baseline(batch: &[Message], owner: u32) -> Option<Vec<Message>> {
    let mut snapshot = false;
    let mut changed = false;
    let filtered: Vec<_> = batch
        .iter()
        .filter(|m| {
            let old_snapshot = snapshot;
            if m.is(crate::Opcode::HeroSpawn) {
                snapshot = affects_owner(m, owner);
            } else {
                snapshot = snapshot
                    && match m.known_opcode() {
                        Some(crate::Opcode::ItemGrant) => {
                            halcyon_protocol::ItemGrantBody::read_exact_or_padded(&m.body)
                                .ok()
                                .is_some_and(|body| body.actor == owner)
                        }
                        Some(crate::Opcode::ModifyActorAttribute) => {
                            halcyon_protocol::AttributeChange::read(m)
                                .is_some_and(|attr| attr.actor == owner)
                        }
                        _ => false,
                    };
            }
            let drop = old_snapshot
                && snapshot
                && matches!(
                    m.opcode,
                    crate::wire_opcode::ItemGrant | crate::wire_opcode::ModifyActorAttribute
                );
            changed |= drop;
            !drop
        })
        .cloned()
        .collect();
    changed.then_some(filtered)
}

fn authoritative_indices(
    view: &ProjectedSlots,
    inventory: &halcyon_sim::AralInventory,
    indices: [u8; 2],
) -> Option<[u8; 2]> {
    let [i, j] = indices.map(usize::from);
    let left = view.0.get(i).copied().flatten()?;
    let right = view.0.get(j).copied().flatten()?;
    let find = |instance: std::num::NonZeroU32| {
        inventory
            .slots()
            .iter()
            .position(|slot| slot.is_some_and(|held| held.instance == instance.get()))
            .map(|slot| slot as u8)
    };
    Some([find(left)?, find(right)?])
}

pub(super) fn reorder(room: &mut Room, seat: usize, channel: usize, actor: u32, slots: [u8; 2]) {
    let session = &room.seats[seat];
    let entity = &room.world.entities[seat];
    if !room.legacy_aral()
        || room.phase != crate::RoomPhase::Running
        || channel != 0
        || !session.world_actor_written
        || !session.preamble.confirmed()
        || actor != u32::from(messages::wire_id(entity.id))
    {
        return;
    }
    let (Some(view), Some(inventory)) = (
        session.aral_reorder_view.as_ref(),
        entity.aral_inventory.as_deref(),
    ) else {
        return;
    };
    let Some(authoritative) = authoritative_indices(view, inventory, slots) else {
        return;
    };
    let entity_id = entity.id;
    if !room.world.reorder_aral_items(entity_id, authoritative) {
        return;
    }
    room.seats[seat]
        .aral_reorder_view
        .as_mut()
        .expect("validated owner view")
        .0
        .swap(usize::from(slots[0]), usize::from(slots[1]));
    let mut shared = None;
    for (other, session) in room.seats.iter().enumerate() {
        let team = room.world.entities[other].team;
        if other != seat
            && session.world_actor_written
            && room.visible[usize::from(team)].contains(&entity_id)
            && room.world.visible(team, &room.world.entities[seat])
            && let Some(peer) = session.gameplay_peer()
        {
            let batch = shared.get_or_insert_with(|| {
                let message: Message = ItemAction::Reorder {
                    actor,
                    slots: authoritative,
                }
                .reorder_message()
                .expect("typed reorder");
                transport::TeamBatch::new(vec![message])
            });
            peer.send(transport::Delivery::Team(batch.clone()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Reservation, Rooms};
    use halcyon_types::MatchHost;
    use tokio::sync::{mpsc, watch};
    use uuid::Uuid;

    #[tokio::test]
    async fn own_slots_swap_twice_without_replay_or_hidden_fanout() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        let players: Vec<_> = (0..6).map(|_| Uuid::new_v4()).collect();
        rooms
            .reserve(Reservation {
                match_id: id,
                players: players.clone(),
                teams: Vec::new(),
                mode: "casual_aral".into(),
                difficulty: None,
                talent_levels: Vec::new(),
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut room = shared.lock().unwrap();
        let mut streams = Vec::new();
        for (index, player) in players.iter().take(3).enumerate() {
            let (tx, rx) = mpsc::channel(16);
            room.attach(*player, tx, watch::channel(false).0).unwrap();
            let actor = 1500 + index as u32;
            for frame in [
                Message::known(crate::Opcode::RequestSceneSnapshot, [0; 6]),
                Message::known(
                    crate::Opcode::ConfirmSceneOwner,
                    [actor.to_be_bytes().as_slice(), &[1, 0]].concat(),
                ),
                Message::known(crate::Opcode::SceneReadyConfirm, [0; 6]),
            ] {
                room.seats[index].preamble.observe(&frame, actor);
            }
            room.seats[index].world_actor_written = true;
            streams.push(rx);
        }
        for stream in &mut streams {
            while stream.try_recv().is_ok() {}
        }
        room.phase(crate::RoomPhase::Running);
        let witness_team = usize::from(room.world.entities[2].team);
        room.visible[witness_team].insert(1);
        for item in [
            halcyon_sim::BasicItem::WeaponBlade,
            halcyon_sim::BasicItem::LightArmor,
        ] {
            room.world.purchase(1, item).unwrap();
        }
        let mut own_view = vec![messages::spawn(&room.world.entities[0], &room.picks, None)];
        messages::append_inventory(&mut own_view, &room.world, &room.world.entities[0]);
        record_delivery(&mut room.seats[0].aral_reorder_view, 1500, &own_view);
        let request = |actor, slots| {
            ItemAction::Reorder { actor, slots }
                .reorder_message()
                .unwrap()
        };
        let before = room.world.state_digest();
        for (seat, actor, slots) in [
            (0, 1501, [2, 3]),
            (1, 1500, [2, 3]),
            (0, 1500, [2, 8]),
            (0, 1500, [2, 4]),
        ] {
            assert!(
                room.receive(seat, 0, request(actor, slots))
                    .unwrap()
                    .is_empty()
            );
        }
        assert_eq!(room.world.state_digest(), before);
        for stream in &mut streams {
            assert!(stream.try_recv().is_err());
        }

        let swap = request(1500, [2, 3]);
        for _ in 0..2 {
            assert!(room.receive(0, 0, swap.clone()).unwrap().is_empty());
            assert_eq!(
                streams[2].try_recv().unwrap().into_records().as_slice(),
                std::slice::from_ref(&swap)
            );
            assert!(streams[0].try_recv().is_err());
            assert!(streams[1].try_recv().is_err());
        }
        assert_eq!(room.world.state_digest(), before);

        // A sparse authoritative inventory must not be compacted or replayed on attach.
        use halcyon_sim::BasicItem::*;
        room.world.entities[0].gold = 20_000;
        for item in [
            CrystalBit,
            SwiftShooter,
            SprintBoots,
            BlazingSalvo,
            AlternatingCurrent,
        ] {
            room.world.purchase(1, item).unwrap();
        }
        let inventory = room.world.entities[0].aral_inventory.as_ref().unwrap();
        assert!(!inventory.is_dense());
        let current: Vec<_> = inventory
            .slots()
            .iter()
            .flatten()
            .map(|slot| (slot.template, slot.instance))
            .collect();
        let before_reconnect = room.world.state_digest();
        let generation = room.seats[0].peers[0].as_ref().unwrap().generation;
        room.detach(0, 0, generation);
        let (tx, _replacement) = mpsc::channel(16);
        room.attach(players[0], tx, watch::channel(false).0)
            .unwrap();
        room.receive(
            0,
            0,
            Message::known(crate::Opcode::RequestSceneSnapshot, [0; 6]),
        )
        .unwrap();
        let scene = room
            .receive(
                0,
                0,
                Message::known(crate::Opcode::ConfirmSceneOwner, [0, 0, 5, 220, 1, 0]),
            )
            .unwrap();
        let grants: Vec<_> = scene
            .iter()
            .filter_map(|m| {
                if let halcyon_protocol::Fields::ItemGrant {
                    actor: 1500,
                    item,
                    instance,
                } = m.fields().ok()?
                {
                    Some((item, instance))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(grants, current);
        assert!(scene.iter().all(|m| !matches!(
            m.opcode,
            crate::wire_opcode::ReorderItem | crate::wire_opcode::ConsumeItem
        )));
        assert!(room.seats[0].aral_reorder_view.is_none());
        record_delivery(&mut room.seats[0].aral_reorder_view, 1500, &scene);
        room.receive(
            0,
            0,
            Message::known(crate::Opcode::SceneReadyConfirm, [0; 6]),
        )
        .unwrap();
        room.seats[0].world_actor_written = true;
        assert_eq!(room.world.state_digest(), before_reconnect);

        for stream in &mut streams {
            while stream.try_recv().is_ok() {}
        }
        let moved_after_rejoin = request(1500, [4, 5]);
        let peer_notice = request(1500, [4, 6]);
        for _ in 0..2 {
            assert!(
                room.receive(0, 0, moved_after_rejoin.clone())
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(
                streams[2].try_recv().unwrap().into_records().as_slice(),
                std::slice::from_ref(&peer_notice)
            );
        }
        assert_eq!(room.world.state_digest(), before_reconnect);

        let purchase = room.buy(0, 1500, TravelBoots.template());
        assert!(!purchase.is_empty());
        let after_purchase = room.world.state_digest();
        // Before the purchase records reach this stream, its old slot still
        // points to a consumed instance. It cannot move the replacement.
        room.receive(0, 0, moved_after_rejoin.clone()).unwrap();
        assert_eq!(room.world.state_digest(), after_purchase);
        record_delivery(&mut room.seats[0].aral_reorder_view, 1500, &purchase);
        let prior = room.world.entities[0]
            .aral_inventory
            .as_ref()
            .unwrap()
            .slots()[5]
            .unwrap();
        assert_eq!(prior.template, TravelBoots.template());
        room.receive(0, 0, moved_after_rejoin).unwrap();
        assert_eq!(
            room.world.entities[0]
                .aral_inventory
                .as_ref()
                .unwrap()
                .slots()[4],
            Some(prior)
        );
    }

    #[test]
    fn repeated_own_declaration_leaves_inventory_generation_unchanged() {
        let mut body = [0; 750];
        body[8..12].copy_from_slice(&1500_u32.to_be_bytes());
        let spawn = Message::known(crate::Opcode::HeroSpawn, body);
        let grant = messages::item_grant(1500, 457, 2000);
        let modifier = messages::modifier(1500, halcyon_sim::NativeItemStat::AttackSpeed, 1.);
        let other = messages::item_grant(1501, 457, 2000);
        let mut view = None;
        record_delivery(&mut view, 1500, &[spawn.clone(), grant.clone()]);
        assert_eq!(view.as_ref().unwrap().0[0].unwrap().get(), 2000);
        let authored = [spawn.clone(), grant, modifier, other.clone()];
        let live = without_existing_baseline(&authored, 1500).unwrap();
        assert_eq!(live, [spawn, other]);
        record_delivery(&mut view, 1500, &live);
        assert!(view.unwrap().0[1..].iter().all(Option::is_none));
    }
}
