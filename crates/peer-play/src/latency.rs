use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

use halcyon_federation::Federation;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Default)]
pub(crate) struct HomeRtt(Mutex<Option<Samples>>);

struct Samples {
    voters: BTreeSet<Uuid>,
    values: BTreeMap<Uuid, u32>,
    expires: Instant,
}

impl HomeRtt {
    /// Only pending enqueues call this method. A cold batch has a single owner;
    /// other enqueues share its measurements for the same committed voter set.
    pub async fn sample(&self, federation: &Federation) -> anyhow::Result<BTreeMap<Uuid, u32>> {
        let mut cached = self.0.lock().await;
        let voters = federation.configured_voters().await?;
        if let Some(samples) = cached.as_ref()
            && samples.voters == voters
            && samples.expires > Instant::now()
        {
            return Ok(samples.values.clone());
        }
        let values = federation.measure_home_rtt(&voters).await;
        let lifetime = if values.len() == voters.len() { 20 } else { 5 };
        *cached = Some(Samples {
            voters,
            values: values.clone(),
            expires: Instant::now() + Duration::from_secs(lifetime),
        });
        Ok(values)
    }
}
