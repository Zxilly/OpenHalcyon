//! Authorize native ARAL slot swaps on the current seat's original world stream.
use super::{Message, Room, messages, transport};
use halcyon_protocol::ItemAction;

pub(super) fn reorder(room: &mut Room, seat: usize, channel: usize, actor: u32, slots: [u8; 2]) {
    let session = &room.seats[seat];
    let entity = &room.world.entities[seat];
    if !room.legacy_aral()
        || room.phase != "running"
        || channel != 0
        || !session.aral_reorder_view
        || !session.world_actor_written
        || !session.preamble.confirmed()
        || actor != u32::from(messages::wire_id(entity.id))
    {
        return;
    }
    let entity_id = entity.id;
    if !room.world.reorder_aral_items(entity_id, slots) {
        return;
    }
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
                let message: Message = ItemAction::Reorder { actor, slots }
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
    use halcyon_platform::MatchHost;
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
                Message::new(0x046e, [0; 6]),
                Message::new(0x0471, [actor.to_be_bytes().as_slice(), &[1, 0]].concat()),
                Message::new(0x046d, [0; 6]),
            ] {
                room.seats[index].preamble.observe(&frame, actor);
            }
            room.seats[index].world_actor_written = true;
            room.seats[index].aral_reorder_view = true;
            streams.push(rx);
        }
        for stream in &mut streams {
            while stream.try_recv().is_ok() {}
        }
        room.phase("running");
        let witness_team = usize::from(room.world.entities[2].team);
        room.visible[witness_team].insert(1);
        for item in [
            halcyon_sim::BasicItem::WeaponBlade,
            halcyon_sim::BasicItem::LightArmor,
        ] {
            room.world.purchase(1, item).unwrap();
        }
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
        room.receive(0, 0, Message::new(0x046e, [0; 6])).unwrap();
        let scene = room
            .receive(0, 0, Message::new(0x0471, [0, 0, 5, 220, 1, 0]))
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
        assert!(scene.iter().all(|m| !matches!(m.opcode, 0x438 | 0x44b)));
        room.receive(0, 0, Message::new(0x046d, [0; 6])).unwrap();
        room.seats[0].world_actor_written = true;
        assert!(!room.seats[0].aral_reorder_view);
        assert!(room.receive(0, 0, swap).unwrap().is_empty());
        assert_eq!(room.world.state_digest(), before_reconnect);
    }
}
