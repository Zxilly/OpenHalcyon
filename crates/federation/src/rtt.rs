use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

use uuid::Uuid;

use crate::Federation;

impl Federation {
    /// Authenticated addresses from the current committed voting configuration.
    pub async fn configured_voters(&self) -> anyhow::Result<BTreeSet<Uuid>> {
        Ok(self
            .members()
            .await?
            .configurations
            .into_iter()
            .flatten()
            .filter(|id| self.net.cfg.peers.contains_key(id))
            .collect())
    }

    /// Measure small authenticated request round trips for one home.
    pub async fn measure_home_rtt(&self, voters: &BTreeSet<Uuid>) -> BTreeMap<Uuid, u32> {
        let home = self.net.cfg.node_id;
        let mut samples = BTreeMap::new();
        let mut probes = tokio::task::JoinSet::new();
        for &peer in voters
            .iter()
            .filter(|id| self.net.cfg.peers.contains_key(id))
        {
            if peer == home {
                samples.insert(home, 0);
                continue;
            }
            if !self.voter(peer) {
                continue;
            }
            let net = self.net.clone();
            probes.spawn(async move {
                let started = Instant::now();
                net.post::<_, ()>(peer, "/internal/rtt", &(), Duration::from_secs(1))
                    .await
                    .ok()?;
                let elapsed = started.elapsed().as_nanos().div_ceil(1_000_000);
                Some((peer, u32::try_from(elapsed).unwrap_or(u32::MAX)))
            });
        }
        while let Some(probe) = probes.join_next().await {
            if let Ok(Some((peer, elapsed))) = probe {
                samples.insert(peer, elapsed);
            }
        }
        samples
    }
}
