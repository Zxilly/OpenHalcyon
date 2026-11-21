use super::state::Room;
use crate::replication::{Batch, messages};
use crate::transport;
use halcyon_sim::BasicItem;

impl Room {
    pub(crate) fn buy(&mut self, i: usize, actor: u32, item: u32) -> Batch {
        let Some(offer) = BasicItem::from_template(item) else {
            return vec![];
        };
        let actor_id = self.world.entities[i].id;
        let Ok(receipt) = self.world.try_purchase(actor_id, offer) else {
            return vec![];
        };
        let mut ack = vec![halcyon_protocol::BuyItemBody { actor, item }.encode()];
        if let Some(receipt) = receipt {
            messages::append_purchase(&mut ack, &self.world, &receipt, true);
        } else if self.legacy_aral() && self.seats[i].world_actor_written {
            ack.push(halcyon_protocol::RejectBuyItemBody.encode());
        } else {
            return vec![];
        }
        let mut shared = None;
        for (j, seat) in self.seats.iter().enumerate() {
            if j != i
                && self.visible[self.world.entities[j].team as usize].contains(&actor_id)
                && self
                    .world
                    .visible(self.world.entities[j].team, &self.world.entities[i])
                && let Some(peer) = seat.gameplay_peer()
            {
                let batch = shared.get_or_insert_with(|| transport::TeamBatch::new(ack.clone()));
                peer.send(transport::Delivery::Team(batch.clone()));
            }
        }
        self.publish_observers(&ack);
        ack
    }
}
