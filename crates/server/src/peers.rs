use crate::config::Peers;
use anyhow::{Result, ensure};
use halcyon_admin::{PoolHealth, PoolRole, PoolStatus};
use halcyon_federation::Federation;
use halcyon_federation::ServerState;
use halcyon_game::Rooms;
use halcyon_peer_play::PeerPlay;
use halcyon_platform::AppState;
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
        let (tx, rx) = tokio::sync::watch::channel(Vec::<PoolStatus>::new());
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
                let role = match metrics.state {
                    ServerState::Learner => PoolRole::Learner,
                    ServerState::Follower => PoolRole::Follower,
                    ServerState::Candidate => PoolRole::Candidate,
                    ServerState::Leader => PoolRole::Leader,
                    ServerState::Shutdown => PoolRole::Shutdown,
                };
                let status = PoolStatus {
                    pool_id,
                    state: if healthy {
                        PoolHealth::Online
                    } else {
                        PoolHealth::NoQuorum
                    },
                    local_role: role,
                    leader_id: metrics.current_leader,
                    voter_count: metrics.membership_config.voter_ids().count(),
                    last_contact_at: last,
                    eligible_hosts: healthy.then_some(hosts),
                    local_tickets,
                };
                tx.send_replace(vec![status]);
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
