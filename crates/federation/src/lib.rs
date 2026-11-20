use halcyon_matchmaking::{Command, Error as ApplyError, Pool};
use openraft::{BasicNode, LogId, StoredMembership};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{collections::BTreeMap, net::SocketAddr, path::PathBuf};
use uuid::Uuid;
mod capacity;
mod membership;
pub use capacity::Capacity;
pub use membership::{MemberChange, Members, PoolAdmin};
mod chat;
mod net;
pub use chat::MatchChat;
mod observation;
pub use observation::{OBSERVER_EXIT_BATCH, ObserverExitCandidate, ObserverReply, ObserverRequest};
mod genesis;
mod rtt;
mod service;
mod store;
pub use service::Federation;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Uuid::is_nil")]
    pub pool_id: Uuid,
    pub node_id: Uuid,
    pub listen: SocketAddr,
    pub database: PathBuf,
    pub ca: PathBuf,
    pub cert: PathBuf,
    pub key: PathBuf,
    pub peers: BTreeMap<Uuid, Peer>,
    pub bootstrap: Bootstrap,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Bootstrap {
    Manual(bool),
    Automatic(AutomaticBootstrap),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AutomaticBootstrap {
    Auto,
}

impl Bootstrap {
    pub const AUTO: Self = Self::Automatic(AutomaticBootstrap::Auto);
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Peer {
    pub address: String,
    pub cert: PathBuf,
}
impl Config {
    pub fn read(path: &std::path::Path) -> anyhow::Result<Self> {
        let mut cfg: Self = serde_json::from_reader(std::fs::File::open(path)?)?;
        let base = path.parent().unwrap_or(std::path::Path::new("."));
        for dest in [&mut cfg.database, &mut cfg.ca, &mut cfg.cert, &mut cfg.key]
            .into_iter()
            .chain(cfg.peers.values_mut().map(|p| &mut p.cert))
        {
            *dest = base.join(&*dest);
        }
        genesis::resolve(&mut cfg)?;
        Ok(cfg)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Proposal {
    pub caller: Uuid,
    pub request_id: Uuid,
    pub command: Command,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epoch: Option<i64>,
}
type Outcome = Result<(), ApplyError>;
openraft::declare_raft_types!(pub Types: D=Proposal,R=Outcome,NodeId=Uuid);
type Raft = openraft::Raft<Types>;
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Machine {
    applied: Option<LogId<Uuid>>,
    membership: StoredMembership<Uuid, BasicNode>,
    pool: Pool,
    results: BTreeMap<(Uuid, Uuid), (Command, Outcome)>,
}
fn encode(v: &impl Serialize) -> anyhow::Result<Vec<u8>> {
    let mut b = Vec::new();
    ciborium::into_writer(v, &mut b)?;
    Ok(b)
}
fn decode<T: DeserializeOwned>(b: &[u8]) -> anyhow::Result<T> {
    Ok(ciborium::from_reader(b)?)
}
use std::io::Cursor;

#[cfg(test)]
extern crate self as halcyon_federation;
#[cfg(test)]
#[path = "../tests/support/pool.rs"]
mod test_pool;
#[cfg(test)]
mod tests {
    use super::test_pool::configs;
    use super::*;
    use crate::Federation;
    use halcyon_matchmaking::{Endpoint, Phase, Queue, Ticket, TicketRef};
    use std::{collections::BTreeSet, time::Duration};
    use tokio::time::{sleep, timeout};
    async fn elected(nodes: &[Option<Federation>]) -> usize {
        timeout(Duration::from_secs(20), async {
            loop {
                let views: Vec<_> = nodes
                    .iter()
                    .flatten()
                    .map(|f| f.metrics().current_leader)
                    .collect();
                if let Some(Some(id)) = views.first()
                    && views.iter().all(|v| *v == Some(*id))
                    && let Some(i) = nodes
                        .iter()
                        .position(|f| f.as_ref().is_some_and(|f| f.metrics().id == *id))
                    && nodes[i].as_ref().unwrap().read().await.is_ok()
                {
                    break i;
                }
                sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .expect("majority elects a leader")
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn voter_can_discover_and_change_committed_membership() {
        let dir = tempfile::tempdir().unwrap();
        let (cfg, listeners) = configs(dir.path());
        let mut nodes = Vec::new();
        for (c, listener) in cfg.iter().zip(listeners) {
            nodes.push(Some(
                Federation::start_prebound(c.clone(), listener)
                    .await
                    .unwrap(),
            ));
        }
        let leader = elected(&nodes).await;
        let caller = (leader + 1) % 3;
        let removed = (leader + 2) % 3;
        let admin = PoolAdmin::new(cfg[caller].clone()).unwrap();
        let members = admin.members().await.unwrap();
        let all = members.configurations[0].clone();
        assert_eq!(all.len(), 3);
        let mut two = all.clone();
        two.remove(&cfg[removed].node_id);
        assert!(
            admin
                .change(&MemberChange {
                    expected: BTreeSet::new(),
                    voters: two.clone()
                })
                .await
                .is_err()
        );
        admin
            .change(&MemberChange {
                expected: all.clone(),
                voters: two.clone(),
            })
            .await
            .unwrap();
        assert_eq!(
            admin.members().await.unwrap().configurations,
            vec![two.clone()]
        );
        let outsider = PoolAdmin::new(cfg[removed].clone()).unwrap();
        assert!(
            outsider
                .change(&MemberChange {
                    expected: two.clone(),
                    voters: all.clone()
                })
                .await
                .is_err()
        );
        admin
            .change(&MemberChange {
                expected: two,
                voters: all.clone(),
            })
            .await
            .unwrap();
        assert_eq!(admin.members().await.unwrap().configurations, vec![all]);
        for node in nodes.into_iter().flatten() {
            node.shutdown().await.unwrap();
        }
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn three_tls_nodes_failover_restart_and_minority() {
        let tmp = tempfile::tempdir().unwrap();
        let (cfg, listeners) = configs(tmp.path());
        let mut nodes = Vec::new();
        for (c, listener) in cfg.iter().zip(listeners) {
            nodes.push(Some(
                Federation::start_prebound(c.clone(), listener)
                    .await
                    .unwrap(),
            ));
        }
        let leader = elected(&nodes).await;
        let home = (leader + 1) % 3;
        let host = (leader + 2) % 3;
        let t = Ticket {
            talent_levels: Vec::new(),
            id: Uuid::new_v4(),
            generation: 1,
            home: cfg[home].node_id,
            players: vec![Uuid::new_v4()],
            queue: Queue {
                name: "lane".into(),
                build: 147219,
                rules_digest: "r".into(),
                seats: 1,
            },
            latency_ms: BTreeMap::from([(cfg[host].node_id, 20)]),
            max_latency_ms: 50,
        };
        let r = Uuid::new_v4();
        let f = nodes[home].as_ref().unwrap();
        let enqueue = Command::Enqueue(t.clone());
        f.submit(r, enqueue.clone()).await.unwrap().unwrap();
        let key = TicketRef {
            id: t.id,
            generation: 1,
        };
        let room = Uuid::new_v4();
        f.submit(
            Uuid::new_v4(),
            Command::Allocate {
                id: room,
                host: cfg[host].node_id,
                tickets: vec![key],
            },
        )
        .await
        .unwrap()
        .unwrap();
        let deadline = f.read().await.unwrap().allocations()[&room]
            .expires_at
            .unwrap();
        nodes[leader].take().unwrap().shutdown().await.unwrap();
        elected(&nodes).await;
        let f = nodes[home].as_ref().unwrap();
        f.submit(r, enqueue.clone()).await.unwrap().unwrap();
        assert_eq!(
            f.read().await.unwrap().allocations()[&room].expires_at,
            Some(deadline)
        );
        assert_eq!(
            f.submit(r, Command::Cancel(key)).await.unwrap(),
            Err(ApplyError::Conflict)
        );
        f.submit(
            Uuid::new_v4(),
            Command::Accept {
                id: room,
                ticket: key,
            },
        )
        .await
        .unwrap()
        .unwrap();
        nodes[host]
            .as_ref()
            .unwrap()
            .submit(
                Uuid::new_v4(),
                Command::HostReady {
                    id: room,
                    endpoint: Endpoint {
                        host: "localhost".into(),
                        port: 9333,
                    },
                },
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            f.read().await.unwrap().allocations()[&room].phase,
            Phase::Ready
        );
        let primary = elected(&nodes).await;
        nodes[primary].as_ref().unwrap().snapshot_then_purge().await;
        nodes[leader] = Some(Federation::start(cfg[leader].clone()).await.unwrap());
        timeout(Duration::from_secs(10), async {
            while nodes[leader].as_ref().unwrap().metrics().snapshot.is_none() {
                sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("restarted follower installs remote snapshot");
        elected(&nodes).await;
        let second = elected(&nodes).await;
        nodes[second].take().unwrap().shutdown().await.unwrap();
        elected(&nodes).await;

        let surviving = nodes.iter().flatten().next().unwrap();
        let restored = surviving.read().await.unwrap();
        assert_eq!(restored.allocations()[&room].phase, Phase::Ready);
        assert_eq!(restored.allocations()[&room].expires_at, Some(deadline));
        let next = nodes.iter().position(Option::is_some).unwrap();
        nodes[next].take().unwrap().shutdown().await.unwrap();
        let alone = nodes.iter().flatten().next().unwrap();
        assert!(alone.submit(Uuid::new_v4(), enqueue).await.is_err());
        assert!(alone.read().await.is_err());
        let result: anyhow::Result<()> = alone
            .inspect(|_| panic!("a minority must not expose a working set"))
            .await;
        assert!(result.is_err());
        alone.shutdown().await.unwrap();
    }
}
