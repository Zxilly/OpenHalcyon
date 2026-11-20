use super::{Batch, Message, Room, RoomPhase, Seat, messages::wire_id, transport::Delivery};
use halcyon_protocol::{ActorFlagChange, ActorFlags};

#[derive(Default)]
pub(super) struct Presence {
    world_channels: [bool; 2],
    afk: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Peer, Rooms};
    use halcyon_platform::{MatchHost, Reservation};
    use tokio::sync::{mpsc, watch};
    use uuid::Uuid;

    #[tokio::test]
    async fn last_world_close_sets_afk_and_only_a_restored_world_clears_it() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                match_id: id,
                players: (0..6).map(|_| Uuid::new_v4()).collect(),
                teams: Vec::new(),
                mode: "casual_aral".into(),
                talent_levels: Vec::new(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut room = shared.lock().unwrap();
        room.phase = RoomPhase::Running;
        let attach = |room: &mut Room, seat: usize, ch: usize, generation: u64| {
            let (tx, rx) = mpsc::channel(8);
            room.seats[seat].peers[ch] = Some(Peer {
                generation,
                tx,
                stalled: watch::channel(false).0,
            });
            rx
        };
        let mut witness = attach(&mut room, 1, 0, 1);
        room.world_connected(1, 0);
        let _control = attach(&mut room, 0, 0, 2);
        let _data = attach(&mut room, 0, 1, 3);
        room.world_connected(0, 0);
        room.world_connected(0, 1);
        room.detach(0, 1, 3);
        room.detach(0, 0, 0);
        assert!(witness.try_recv().is_err());
        room.detach(0, 0, 2);
        let read = |rx: &mut mpsc::Receiver<Delivery>| {
            let Delivery::Private(batch) = rx.try_recv().unwrap() else {
                panic!()
            };
            assert_eq!(batch.len(), 1);
            let flags = batch[0].actor_flags().unwrap();
            assert_eq!(flags.actor, 1500);
            flags.change
        };
        assert_eq!(read(&mut witness), ActorFlagChange::Afk);
        let mut late = attach(&mut room, 2, 0, 4);
        room.world_connected(2, 0);
        assert_eq!(read(&mut late), ActorFlagChange::Afk);
        let _fresh = attach(&mut room, 0, 0, 5);
        room.detach(0, 0, 2);
        assert!(witness.try_recv().is_err());
        room.world_connected(0, 0);
        assert_eq!(read(&mut witness), ActorFlagChange::Reconnected);
        assert_eq!(read(&mut late), ActorFlagChange::Reconnected);
        assert!(room.absent_players().is_empty());
        room.world_connected(0, 0);
        assert!(witness.try_recv().is_err());
    }
}

impl Seat {
    fn presence_peer(&self) -> Option<&super::Peer> {
        (0..2).rev().find_map(|ch| {
            self.peers[ch]
                .as_ref()
                .filter(|_| self.presence.world_channels[ch])
        })
    }
}

impl Room {
    fn presence_message(&self, seat: usize, change: ActorFlagChange) -> Message {
        ActorFlags::new(wire_id(self.world.entities[seat].id).into(), change).to_message()
    }

    pub(super) fn absent_players(&self) -> Batch {
        self.seats
            .iter()
            .enumerate()
            .filter(|(_, seat)| seat.presence.afk)
            .map(|(i, _)| self.presence_message(i, ActorFlagChange::Afk))
            .collect()
    }

    /// The scoreboard addresses roster seats, including dead or out-of-sight heroes.
    fn announce_presence(&mut self, seat: usize, change: ActorFlagChange) {
        let message = self.presence_message(seat, change);
        for (i, recipient) in self.seats.iter().enumerate() {
            if i != seat
                && let Some(peer) = recipient.presence_peer()
            {
                peer.send(Delivery::Private(vec![message.clone()]));
            }
        }
        self.publish_observers(std::slice::from_ref(&message));
    }

    pub(super) fn world_connected(&mut self, seat: usize, channel: usize) {
        let presence = &mut self.seats[seat].presence;
        presence.world_channels[channel] = true;
        if std::mem::take(&mut presence.afk) && self.phase == RoomPhase::Running {
            self.announce_presence(seat, ActorFlagChange::Reconnected);
        }
        let snapshot = self.absent_players();
        if !snapshot.is_empty()
            && let Some(peer) = &self.seats[seat].peers[channel]
        {
            peer.send(Delivery::Private(snapshot));
        }
    }

    pub(super) fn world_disconnected(&mut self, seat: usize, channel: usize) {
        let presence = &mut self.seats[seat].presence;
        if std::mem::take(&mut presence.world_channels[channel])
            && self.phase == RoomPhase::Running
            && !presence.world_channels.contains(&true)
        {
            presence.afk = true;
            self.announce_presence(seat, ActorFlagChange::Afk);
        }
    }
}
