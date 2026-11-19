use halcyon_matchmaking::{Command, Error as ApplyError, Pool};
use openraft::{BasicNode, LogId, StoredMembership};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{collections::BTreeMap, net::SocketAddr, path::PathBuf};
use uuid::Uuid;
mod capacity;
pub use capacity::Capacity;
mod net;
mod service;
mod store;
pub use service::Federation;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub pool_id: Uuid,
    pub node_id: Uuid,
    pub listen: SocketAddr,
    pub database: PathBuf,
    pub ca: PathBuf,
    pub cert: PathBuf,
    pub key: PathBuf,
    pub peers: BTreeMap<Uuid, Peer>,
    pub bootstrap: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Peer {
    pub address: String,
    pub cert: PathBuf,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Proposal {
    pub caller: Uuid,
    pub request_id: Uuid,
    pub command: Command,
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
mod tests {
    use super::*;
    use crate::Federation;
    use halcyon_matchmaking::{Endpoint, Phase, Queue, Ticket, TicketRef};
    use rcgen::{BasicConstraints, CertificateParams, IsCa, KeyPair};
    use std::{fs, time::Duration};
    use tokio::time::{sleep, timeout};
    fn configs(path: &std::path::Path) -> Vec<Config> {
        let ca_key = KeyPair::generate().unwrap();
        let mut par = CertificateParams::new(Vec::new()).unwrap();
        par.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca = par.self_signed(&ca_key).unwrap();
        let ca_path = path.join("ca.pem");
        fs::write(&ca_path, ca.pem()).unwrap();
        let pool_id = Uuid::new_v4();
        let mut peers = BTreeMap::new();
        let mut configs = Vec::new();
        let mut reservations = Vec::new();
        for i in 0..3 {
            let node_id = Uuid::from_u128(i + 1);
            let key = KeyPair::generate().unwrap();
            let mut par = CertificateParams::new(vec!["127.0.0.1".into()]).unwrap();
            par.distinguished_name
                .push(rcgen::DnType::CommonName, node_id.to_string());
            let leaf = par.signed_by(&key, &ca, &ca_key).unwrap();
            let cert = path.join(format!("{i}.pem"));
            let privkey = path.join(format!("{i}.key"));
            fs::write(&cert, leaf.pem()).unwrap();
            fs::write(&privkey, key.serialize_pem()).unwrap();
            let sock = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let listen = sock.local_addr().unwrap();
            reservations.push(sock);
            peers.insert(
                node_id,
                Peer {
                    address: listen.to_string(),
                    cert: cert.clone(),
                },
            );
            configs.push(Config {
                node_id,
                pool_id,
                listen,
                ca: ca_path.clone(),
                cert,
                key: privkey,
                database: path.join(format!("{i}.db")),
                peers: BTreeMap::new(),
                bootstrap: i == 0,
            });
        }
        for c in &mut configs {
            c.peers = peers.clone()
        }
        configs
    }
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
    async fn three_tls_nodes_failover_restart_and_minority() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = configs(tmp.path());
        let mut nodes = Vec::new();
        for c in &cfg {
            nodes.push(Some(Federation::start(c.clone()).await.unwrap()));
        }
        let leader = elected(&nodes).await;
        let home = (leader + 1) % 3;
        let host = (leader + 2) % 3;
        let t = Ticket {
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
        nodes[leader].take().unwrap().shutdown().await.unwrap();
        elected(&nodes).await;
        let f = nodes[home].as_ref().unwrap();
        f.submit(r, enqueue.clone()).await.unwrap().unwrap();
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
        assert_eq!(
            surviving.read().await.unwrap().allocations()[&room].phase,
            Phase::Ready
        );
        let next = nodes.iter().position(Option::is_some).unwrap();
        nodes[next].take().unwrap().shutdown().await.unwrap();
        let alone = nodes.iter().flatten().next().unwrap();
        assert!(alone.submit(Uuid::new_v4(), enqueue).await.is_err());
        assert!(alone.read().await.is_err());
        alone.shutdown().await.unwrap();
    }
}
