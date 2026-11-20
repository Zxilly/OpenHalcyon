use crate::config::Peers;
use anyhow::{Result, ensure};
use halcyon_federation::Federation;
use halcyon_game::Rooms;
use halcyon_peer_play::PeerPlay;
use halcyon_platform::AppState;
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::Arc, time::Duration};
use uuid::Uuid;

pub struct Network {
    pub worker: PeerPlay,
    node: Federation,
    pub view: halcyon_admin::PoolStats,
    observer: tokio::task::JoinHandle<()>,
}

impl Network {
    pub async fn start(
        settings: Peers,
        id: Uuid,
        digest: String,
        platform: AppState,
        rooms: Rooms,
    ) -> Result<Self> {
        let cfg = halcyon_federation::Config::read(&settings.cluster_file)?;
        ensure!(
            cfg.node_id == id,
            "federation node_id must match server instance_id"
        );
        let pool_id = cfg.pool_id;
        let node = Federation::start(cfg).await?;
        let worker = PeerPlay::start(
            node.clone(),
            platform,
            rooms,
            halcyon_peer_play::Config {
                pool_id,
                node_id: id,
                database: settings.journal,
                rules_digest: digest,
                peer_latency_ms: settings.latency_ms,
                max_latency_ms: settings.max_latency_ms,
            },
        )?;
        let (tx, rx) = tokio::sync::watch::channel(Vec::<Value>::new());
        let source = node.clone();
        let observer = tokio::spawn(async move {
            let mut last = None;
            let mut local_tickets = 0;
            loop {
                let read = tokio::time::timeout(
                    Duration::from_secs(2),
                    source.inspect(|pool| pool.active_ticket_count(id)),
                )
                .await;
                let healthy = if let Ok(Ok(count)) = read {
                    local_tickets = count;
                    last = Some(chrono::Utc::now());
                    true
                } else {
                    false
                };
                let metrics = source.metrics();
                let hosts = source
                    .host_offers()
                    .iter()
                    .filter(|h| h.free_seats > 0)
                    .map(|h| h.id)
                    .collect::<BTreeSet<_>>()
                    .len();
                let role = format!("{:?}", metrics.state).to_lowercase();
                tx.send_replace(vec![json!({"poolId":pool_id,"state":if healthy{"online"}else{"no_quorum"},
                    "localRole":role,"leaderId":metrics.current_leader,"voterCount":metrics.membership_config.voter_ids().count(),
                    "lastContactAt":last,"eligibleHosts":if healthy{Some(hosts)}else{None},"localTickets":local_tickets})]);
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });
        Ok(Self {
            worker,
            node,
            view: Arc::new(move || rx.borrow().clone()),
            observer,
        })
    }
    pub async fn shutdown(self) -> Result<()> {
        self.observer.abort();
        self.worker.shutdown().await;
        self.node.shutdown().await
    }
}
