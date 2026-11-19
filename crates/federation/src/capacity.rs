use crate::{Federation, Uuid};
use anyhow::ensure;
use halcyon_matchmaking::{HostOffer, Queue};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capacity {
    pub sequence: u64,
    pub ttl_ms: u64,
    pub queue: Queue,
    pub free_seats: usize,
    pub total_seats: usize,
}
#[derive(Clone, Default)]
pub(crate) struct Offers(Arc<Mutex<BTreeMap<Uuid, (Capacity, Instant)>>>);
impl Offers {
    pub fn update(&self, id: Uuid, offer: Capacity) -> anyhow::Result<()> {
        ensure!(
            offer.free_seats <= offer.total_seats && (1..=60_000).contains(&offer.ttl_ms),
            "invalid capacity"
        );
        let mut seen = self.0.lock().unwrap();
        if let Some((old, _)) = seen.get(&id) {
            if old.sequence == offer.sequence && old == &offer {
                return Ok(());
            }
            ensure!(old.sequence < offer.sequence, "stale capacity");
        }
        let expires = Instant::now() + Duration::from_millis(offer.ttl_ms);
        seen.insert(id, (offer, expires));
        Ok(())
    }
    pub fn live(&self) -> Vec<HostOffer> {
        let now = Instant::now();
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, (_, expires))| *expires > now)
            .map(|(id, (v, _))| HostOffer {
                id: *id,
                queue: v.queue.clone(),
                free_seats: v.free_seats,
                total_seats: v.total_seats,
            })
            .collect()
    }
}
impl Federation {
    /// The sender supplies a monotonically increasing sequence, including across restarts.
    pub async fn advertise(&self, offer: Capacity) -> anyhow::Result<()> {
        let id = self.net.cfg.node_id;
        self.offers.update(id, offer.clone())?;
        let mut sends = tokio::task::JoinSet::new();
        for peer in self
            .metrics()
            .membership_config
            .voter_ids()
            .filter(|p| *p != id)
        {
            let net = self.net.clone();
            let msg = offer.clone();
            sends.spawn(async move {
                net.post::<_, ()>(peer, "/internal/capacity", &msg, Duration::from_secs(1))
                    .await
            });
        }
        while let Some(r) = sends.join_next().await {
            if let Ok(Err(e)) = r {
                tracing::debug!(%e,"capacity advertisement undelivered")
            }
        }
        Ok(())
    }
    pub fn host_offers(&self) -> Vec<HostOffer> {
        self.offers
            .live()
            .into_iter()
            .filter(|h| self.voter(h.id))
            .collect()
    }
}
