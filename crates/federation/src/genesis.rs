//! Discover a quorum that attests to one immutable initial configuration.
use crate::{BasicNode, Bootstrap, Config, Federation, net::certs};
use anyhow::ensure;
use openraft::error::{InitializeError, RaftError};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, time::Duration};
use tokio::{task::JoinSet, time::sleep};
use uuid::Uuid;

pub(super) fn resolve(cfg: &mut Config) -> anyhow::Result<Option<[u8; 32]>> {
    if cfg.bootstrap != Bootstrap::AUTO {
        ensure!(
            !cfg.pool_id.is_nil(),
            "explicit bootstrap mode requires pool_id"
        );
        return Ok(None);
    }
    ensure!(
        cfg.peers.contains_key(&cfg.node_id),
        "local node must be in the initial peer set"
    );
    let mut hash = Sha256::new();
    hash.update(b"OpenHalcyon automatic pool genesis\0");
    hash.update((cfg.peers.len() as u64).to_be_bytes());
    for (id, peer) in &cfg.peers {
        let authority = peer.address.parse::<hyper::http::uri::Authority>()?;
        ensure!(
            authority.port_u16().is_some(),
            "initial peer address needs a port"
        );
        hash.update(id.as_bytes());
        hash.update((peer.address.len() as u64).to_be_bytes());
        hash.update(peer.address.as_bytes());
        hash.update(Sha256::digest(certs(&peer.cert)?[0].as_ref()));
    }
    let digest: [u8; 32] = hash.finalize().into();
    let mut bytes: [u8; 16] = digest[..16].try_into().unwrap();
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let derived = Uuid::from_bytes(bytes);
    ensure!(
        cfg.pool_id.is_nil() || cfg.pool_id == derived,
        "pool_id does not match the complete automatic genesis configuration"
    );
    cfg.pool_id = derived;
    Ok(Some(digest))
}

pub(super) async fn discover(fed: &Federation) -> anyhow::Result<()> {
    let Some(genesis) = fed.net.genesis else {
        return Ok(());
    };
    let config = &fed.net.cfg;
    let quorum = config.peers.len() / 2 + 1;
    while !fed.raft.is_initialized().await? {
        let mut reachable = 1; // The local configured certificate was already checked.
        let mut probes = JoinSet::new();
        for &peer in config.peers.keys().filter(|&&peer| peer != config.node_id) {
            let net = fed.net.clone();
            probes.spawn(async move {
                net.post::<_, [u8; 32]>(
                    peer,
                    "/internal/genesis",
                    &genesis,
                    Duration::from_millis(850),
                )
                .await
                .is_ok_and(|reply| reply == genesis)
            });
        }
        while reachable < quorum {
            let Some(result) = probes.join_next().await else {
                break;
            };
            reachable += usize::from(result.unwrap_or(false));
        }
        drop(probes);
        if reachable >= quorum {
            let members = config
                .peers
                .iter()
                .map(|(id, peer)| (*id, BasicNode::new(&peer.address)))
                .collect::<BTreeMap<_, _>>();
            match fed.raft.initialize(members).await {
                Ok(()) | Err(RaftError::APIError(InitializeError::NotAllowed(_))) => return Ok(()),
                Err(error) => return Err(error.into()),
            }
        }
        sleep(Duration::from_millis(400)).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_pool::configs;
    use halcyon_matchmaking::{Command, Queue, Ticket};
    use std::net::TcpListener;
    use tokio::time::{sleep, timeout};

    fn automatic(path: &std::path::Path) -> (Vec<Config>, Vec<TcpListener>) {
        let (mut cfg, listeners) = configs(path);
        for node in &mut cfg {
            node.bootstrap = Bootstrap::AUTO;
            node.pool_id = Uuid::nil();
        }
        (cfg, listeners)
    }

    async fn leader(nodes: &[&Federation]) -> Uuid {
        timeout(Duration::from_secs(10), async {
            loop {
                for &node in nodes {
                    if node.metrics().state.is_leader() && node.read().await.is_ok() {
                        return node.net.cfg.node_id;
                    }
                }
                sleep(Duration::from_millis(30)).await;
            }
        })
        .await
        .expect("no elected automatic pool leader")
    }

    async fn wait_members(nodes: &[&Federation], expected: &BTreeMap<Uuid, crate::Peer>) {
        timeout(Duration::from_secs(5), async {
            loop {
                if nodes.iter().all(|node| {
                    node.store
                        .membership()
                        .nodes()
                        .map(|(id, node)| (*id, node.addr.as_str()))
                        .eq(expected
                            .iter()
                            .map(|(&id, peer)| (id, peer.address.as_str())))
                }) {
                    break;
                }
                sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("complete initial membership failed to replicate");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 3)]
    async fn all_three_can_start_automatically_and_share_one_full_genesis() {
        let dir = tempfile::tempdir().unwrap();
        let (cfg, sockets) = automatic(dir.path());
        let [a, b, c]: [_; 3] = sockets.try_into().unwrap();
        let (a, b, c) = tokio::join!(
            Federation::start_prebound(cfg[0].clone(), a),
            Federation::start_prebound(cfg[1].clone(), b),
            Federation::start_prebound(cfg[2].clone(), c)
        );
        let (a, b, c) = (a.unwrap(), b.unwrap(), c.unwrap());
        assert_eq!(a.net.cfg.pool_id, b.net.cfg.pool_id);
        assert_eq!(b.net.cfg.pool_id, c.net.cfg.pool_id);
        assert_eq!(a.net.cfg.pool_id.get_version_num(), 8);
        let signature = leader(&[&a, &b, &c]).await;
        wait_members(&[&a, &b, &c], &cfg[0].peers).await;
        assert!(cfg[0].peers.contains_key(&signature));
        let (a, b, c) = tokio::join!(a.shutdown(), b.shutdown(), c.shutdown());
        a.unwrap();
        b.unwrap();
        c.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 3)]
    async fn missing_lowest_voter_still_bootstraps_and_restart_cannot_form_smaller_pool() {
        let dir = tempfile::tempdir().unwrap();
        let (cfg, sockets) = automatic(dir.path());
        let [_absent, b, c]: [_; 3] = sockets.try_into().unwrap();
        let (b, c) = tokio::join!(
            Federation::start_prebound(cfg[1].clone(), b),
            Federation::start_prebound(cfg[2].clone(), c)
        );
        let (b, c) = (b.unwrap(), c.unwrap());
        let elected = leader(&[&b, &c]).await;
        assert_ne!(elected, cfg[0].node_id);
        wait_members(&[&b, &c], &cfg[0].peers).await;

        let ticket = Ticket {
            id: Uuid::new_v4(),
            generation: 1,
            home: cfg[1].node_id,
            players: vec![Uuid::new_v4()],
            talent_levels: vec![],
            queue: Queue {
                name: "test".into(),
                build: 147219,
                rules_digest: "test".into(),
                seats: 1,
            },
            latency_ms: BTreeMap::new(),
            max_latency_ms: 100,
        };
        b.submit(Uuid::new_v4(), Command::Enqueue(ticket.clone()))
            .await
            .unwrap()
            .unwrap();
        assert!(b.read().await.unwrap().tickets().contains_key(&ticket.id));
        let pool = b.net.cfg.pool_id;
        c.shutdown().await.unwrap();
        b.shutdown().await.unwrap();

        let lone = Federation::start(cfg[1].clone()).await.unwrap();
        assert_eq!(lone.net.cfg.pool_id, pool);
        assert!(lone.inspect_applied(|p| p.tickets().contains_key(&ticket.id)));
        assert!(lone.read().await.is_err());
        let genesis = lone.net.genesis.unwrap();
        let error = lone
            .net
            .post::<_, [u8; 32]>(
                cfg[1].node_id,
                "/internal/genesis",
                &genesis,
                Duration::from_secs(2),
            )
            .await
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<crate::net::PeerStatus>().unwrap().0,
            axum::http::StatusCode::CONFLICT
        );
        wait_members(&[&lone], &cfg[0].peers).await;
        lone.shutdown().await.unwrap();

        let mut changed = cfg[1].clone();
        changed.peers.get_mut(&cfg[0].node_id).unwrap().address = "localhost:39999".into();
        let error = Federation::start(changed.clone())
            .await
            .err()
            .expect("changed peer address must reject stored identity");
        assert!(
            error
                .to_string()
                .contains("database belongs to another pool")
        );
        changed.bootstrap = Bootstrap::Manual(false);
        changed.pool_id = pool;
        let migrated = Federation::start(changed).await.unwrap();
        assert!(migrated.inspect_applied(|p| p.tickets().contains_key(&ticket.id)));
        wait_members(&[&migrated], &cfg[0].peers).await;
        assert!(migrated.read().await.is_err());
        migrated.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 3)]
    async fn disagreement_of_pinned_peers_cannot_start_pristine_pools() {
        let dir = tempfile::tempdir().unwrap();
        let (mut cfg, sockets) = automatic(dir.path());
        let [a, b, _absent]: [_; 3] = sockets.try_into().unwrap();
        let third = cfg[2].node_id;
        cfg[1].peers.get_mut(&third).unwrap().address = "localhost:39999".into();
        let (a, b) = tokio::join!(
            Federation::start_prebound(cfg[0].clone(), a),
            Federation::start_prebound(cfg[1].clone(), b)
        );
        let (a, b) = (a.unwrap(), b.unwrap());
        assert_ne!(a.net.cfg.pool_id, b.net.cfg.pool_id);
        let response = a
            .net
            .post::<_, [u8; 32]>(
                cfg[1].node_id,
                "/internal/genesis",
                &a.net.genesis.unwrap(),
                Duration::from_secs(2),
            )
            .await;
        assert_eq!(
            response
                .unwrap_err()
                .downcast_ref::<crate::net::PeerStatus>()
                .unwrap()
                .0,
            axum::http::StatusCode::FORBIDDEN
        );
        for node in [&a, &b] {
            assert!(node.read().await.is_err());
        }
        // Both agents retry through more than one discovery probe window.
        sleep(Duration::from_millis(1400)).await;
        for node in [&a, &b] {
            assert!(!node.raft.is_initialized().await.unwrap());
            assert_eq!(node.metrics().membership_config.voter_ids().count(), 0);
        }
        let (a, b) = tokio::join!(a.shutdown(), b.shutdown());
        a.unwrap();
        b.unwrap();
    }

    #[test]
    fn bootstrap_config_remains_compatible_and_cert_rotation_requires_explicit_identity() {
        let dir = tempfile::tempdir().unwrap();
        let (mut cfg, _) = automatic(dir.path());
        assert_eq!(
            serde_json::from_str::<Bootstrap>("true").unwrap(),
            Bootstrap::Manual(true)
        );
        assert_eq!(
            serde_json::from_str::<Bootstrap>("false").unwrap(),
            Bootstrap::Manual(false)
        );
        assert_eq!(
            serde_json::from_str::<Bootstrap>(r#""auto""#).unwrap(),
            Bootstrap::AUTO
        );
        let id = cfg[0].node_id;
        let config_path = dir.path().join("pool.json");
        std::fs::write(&config_path, serde_json::to_vec(&cfg[0]).unwrap()).unwrap();
        let parsed = Config::read(&config_path).unwrap();
        assert_ne!(parsed.pool_id, Uuid::nil());
        let other = cfg[1].cert.clone();
        cfg[0].peers.get_mut(&id).unwrap().cert = other;
        let changed = resolve(&mut cfg[0]).unwrap().unwrap();
        assert_ne!(changed, resolve(&mut parsed.clone()).unwrap().unwrap());
        cfg[0].pool_id = parsed.pool_id;
        assert!(resolve(&mut cfg[0]).is_err());
        cfg[0].bootstrap = Bootstrap::Manual(false);
        assert_eq!(resolve(&mut cfg[0]).unwrap(), None);
        assert_eq!(cfg[0].pool_id, parsed.pool_id);
    }
}
