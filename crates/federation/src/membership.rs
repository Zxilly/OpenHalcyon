use crate::{Config, Federation, net::Transport};
use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};
use tokio::time::timeout;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct Members {
    pub pool_id: Uuid,
    pub leader: Uuid,
    /// A joint configuration contains the outgoing and incoming voter sets.
    pub configurations: Vec<BTreeSet<Uuid>>,
    pub addresses: BTreeMap<Uuid, String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberChange {
    pub expected: BTreeSet<Uuid>,
    pub voters: BTreeSet<Uuid>,
}

const READ: Duration = Duration::from_secs(4);
const CHANGE: Duration = Duration::from_secs(40);
pub(crate) const CHANGE_REQUEST: Duration = Duration::from_secs(46);

/// A CLI client uses the operator's existing pool certificate and pinned peers.
pub struct PoolAdmin(Transport);
impl PoolAdmin {
    pub fn new(config: Config) -> anyhow::Result<Self> {
        Ok(Self(Transport::new(config)?))
    }
    pub async fn members(&self) -> anyhow::Result<Members> {
        let mut last = None;
        for seed in self.0.cfg.peers.keys() {
            match self.0.post(*seed, "/internal/members", &(), READ).await {
                Ok(m) => return Ok(m),
                Err(e) => last = Some(e),
            }
        }
        Err(last.unwrap_or_else(|| anyhow::anyhow!("no pool peers configured")))
    }
    pub async fn change(&self, request: &MemberChange) -> anyhow::Result<()> {
        let leader = self.members().await?.leader;
        let applied: Result<(), String> = self
            .0
            .post(
                leader,
                "/internal/reconfigure",
                request,
                CHANGE_REQUEST + READ,
            )
            .await?;
        applied.map_err(anyhow::Error::msg)
    }
}

impl Federation {
    pub async fn members(&self) -> anyhow::Result<Members> {
        let leader = self
            .raft
            .current_leader()
            .await
            .context("no elected leader")?;
        if leader != self.net.cfg.node_id {
            return self.net.post(leader, "/internal/members", &(), READ).await;
        }
        timeout(READ, self.raft.ensure_linearizable()).await??;
        let membership = self.store.machine()?.membership;
        Ok(Members {
            pool_id: self.net.cfg.pool_id,
            leader,
            configurations: membership.membership().get_joint_config().clone(),
            addresses: membership
                .nodes()
                .map(|(id, node)| (*id, node.addr.clone()))
                .collect(),
        })
    }
    pub(crate) async fn reconfigure(&self, request: MemberChange) -> Result<(), String> {
        async fn apply(f: &Federation, r: MemberChange) -> anyhow::Result<()> {
            let _guard = f.membership_lock.lock().await;
            ensure!(
                f.raft.current_leader().await == Some(f.net.cfg.node_id),
                "leadership changed; read the pool and retry"
            );
            let current = f.members().await?.configurations;
            ensure!(
                !r.voters.is_empty() && r.voters.iter().all(|id| f.net.cfg.peers.contains_key(id)),
                "all voters need trusted peer identities"
            );
            if current.len() == 1 && current[0] == r.voters {
                return Ok(());
            }
            ensure!(
                current.len() == 1 && current[0] == r.expected,
                "membership differs from expected configuration"
            );
            timeout(CHANGE, f.change_membership(r.voters)).await??;
            Ok(())
        }
        apply(self, request).await.map_err(|e| e.to_string())
    }
}
