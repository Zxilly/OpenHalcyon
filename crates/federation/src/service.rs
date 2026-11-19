use super::*;
use crate::{
    net::{MAX_BODY, Transport, cbor},
    store::Store,
};
use anyhow::{Context, ensure};
use axum::{
    body::{Body, to_bytes},
    http::{Method, Request, Response, StatusCode},
    response::IntoResponse,
};
use hyper_util::rt::TokioIo;
use openraft::{
    RaftMetrics,
    raft::{AppendEntriesRequest, InstallSnapshotRequest, VoteRequest},
    storage::Adaptor,
};
use std::{collections::BTreeSet, convert::Infallible, sync::Arc, time::Duration};
use tokio::{
    net::TcpListener,
    sync::{Semaphore, watch},
    time::timeout,
};

const DEADLINE: Duration = Duration::from_secs(4);
#[derive(Clone)]
pub struct Federation {
    raft: Raft,
    store: Store,
    pub(crate) net: Transport,
    pub(crate) offers: crate::capacity::Offers,
    stop: watch::Sender<bool>,
}
impl Federation {
    pub async fn start(cfg: Config) -> anyhow::Result<Self> {
        let listener = TcpListener::bind(cfg.listen).await?;
        let net = Transport::new(cfg)?;
        let cfg = &net.cfg;
        let store = Store::open(&cfg.database, cfg.pool_id, cfg.node_id)?;
        let settings = openraft::Config {
            cluster_name: cfg.pool_id.to_string(),
            heartbeat_interval: 180,
            election_timeout_min: 650,
            election_timeout_max: 1200,
            snapshot_policy: openraft::SnapshotPolicy::LogsSinceLast(128),
            max_in_snapshot_log_to_keep: 32,
            ..Default::default()
        }
        .validate()?;
        let (logs, machine) = Adaptor::new(store.clone());
        let raft = Raft::new(cfg.node_id, Arc::new(settings), net.clone(), logs, machine).await?;
        if cfg.bootstrap && !raft.is_initialized().await? {
            let members = cfg
                .peers
                .iter()
                .map(|(id, p)| (*id, BasicNode::new(&p.address)))
                .collect::<BTreeMap<_, _>>();
            raft.initialize(members).await?;
        }
        let (stop, mut stopped) = watch::channel(false);
        let fed = Self {
            raft,
            store,
            net,
            offers: Default::default(),
            stop,
        };
        let server = fed.clone();
        tokio::spawn(async move {
            let slots = Arc::new(Semaphore::new(128));
            loop {
                let socket = tokio::select! {
                    _=stopped.changed()=>break,
                    result=listener.accept()=>match result {Ok((socket,_))=>socket,Err(_)=>break}
                };
                let Ok(slot) = slots.clone().try_acquire_owned() else {
                    continue;
                };
                let f = server.clone();
                let mut closing = stopped.clone();
                tokio::spawn(async move {
                    let _slot = slot;
                    tokio::select! {
                        _ = closing.changed() => {},
                        _ = f.serve_peer(socket) => {},
                    }
                });
            }
        });
        Ok(fed)
    }
    async fn serve_peer(&self, socket: tokio::net::TcpStream) -> anyhow::Result<()> {
        socket.set_nodelay(true)?;
        let tls = timeout(Duration::from_secs(5), self.net.accept.accept(socket)).await??;
        let peer = self.net.identity(tls.get_ref().1.peer_certificates())?;
        let service =
            hyper::service::service_fn(|request: Request<hyper::body::Incoming>| async move {
                let response = match timeout(
                    Duration::from_secs(12),
                    self.route(peer, request.map(Body::new)),
                )
                .await
                {
                    Ok(Ok(response)) => response,
                    Ok(Err(e)) => {
                        tracing::debug!(%e,"peer request failed");
                        StatusCode::SERVICE_UNAVAILABLE.into_response()
                    }
                    Err(_) => StatusCode::REQUEST_TIMEOUT.into_response(),
                };
                Ok::<_, Infallible>(response)
            });
        hyper::server::conn::http1::Builder::new()
            .timer(hyper_util::rt::TokioTimer::new())
            .header_read_timeout(Duration::from_secs(12))
            .keep_alive(true)
            .serve_connection(TokioIo::new(tls), service)
            .await?;
        Ok(())
    }
    #[cfg(test)]
    pub(crate) async fn snapshot_then_purge(&self) {
        let applied = self.metrics().last_applied.unwrap();
        self.raft.trigger().snapshot().await.unwrap();
        self.raft
            .wait(Some(Duration::from_secs(5)))
            .snapshot(applied, "snapshot built")
            .await
            .unwrap();
        self.raft.trigger().purge_log(applied.index).await.unwrap();
        self.raft
            .wait(Some(Duration::from_secs(5)))
            .metrics(|m| m.purged == Some(applied), "prefix purged")
            .await
            .unwrap();
    }
    pub fn metrics(&self) -> RaftMetrics<Uuid, BasicNode> {
        self.raft.metrics().borrow().clone()
    }
    pub async fn shutdown(&self) -> anyhow::Result<()> {
        self.stop.send_replace(true);
        self.raft.shutdown().await?;
        Ok(())
    }
    pub async fn submit(&self, request_id: Uuid, command: Command) -> anyhow::Result<Outcome> {
        self.submit_as(Proposal {
            caller: self.net.cfg.node_id,
            request_id,
            command,
        })
        .await
    }
    pub(crate) fn voter(&self, id: Uuid) -> bool {
        self.metrics()
            .membership_config
            .voter_ids()
            .any(|v| v == id)
    }
    async fn submit_as(&self, p: Proposal) -> anyhow::Result<Outcome> {
        ensure!(self.voter(p.caller), "caller is not a voter");
        let id = self
            .raft
            .current_leader()
            .await
            .context("no elected leader")?;
        if id == self.net.cfg.node_id {
            return self.write_here(p).await;
        }
        self.net.post(id, "/internal/forward", &p, DEADLINE).await
    }
    async fn write_here(&self, p: Proposal) -> anyhow::Result<Outcome> {
        ensure!(self.voter(p.caller), "caller is not a voter");
        if let Command::Allocate { host, .. } = &p.command
            && !self.voter(*host)
        {
            return Ok(Err(ApplyError::Invalid));
        }
        Ok(timeout(DEADLINE, self.raft.client_write(p)).await??.data)
    }
    pub async fn read(&self) -> anyhow::Result<Pool> {
        let id = self
            .raft
            .current_leader()
            .await
            .context("no elected leader")?;
        if id == self.net.cfg.node_id {
            return self.read_here().await;
        }
        self.net.post(id, "/internal/read", &(), DEADLINE).await
    }
    async fn read_here(&self) -> anyhow::Result<Pool> {
        timeout(DEADLINE, self.raft.ensure_linearizable()).await??;
        Ok(self.store.machine()?.pool)
    }
    /// A linearizable read that sends the pool only when the applied index changed.
    pub async fn changes(&self, known: Option<u64>) -> anyhow::Result<(u64, Option<Pool>)> {
        let id = self
            .raft
            .current_leader()
            .await
            .context("no elected leader")?;
        if id == self.net.cfg.node_id {
            return self.changes_here(known).await;
        }
        self.net
            .post(id, "/internal/changes", &known, DEADLINE)
            .await
    }
    async fn changes_here(&self, known: Option<u64>) -> anyhow::Result<(u64, Option<Pool>)> {
        timeout(DEADLINE, self.raft.ensure_linearizable()).await??;
        Ok(self.store.pool_changes(known)?)
    }
    /// Run on the elected leader. All transports must already pin each proposed member.
    pub async fn change_membership(&self, voters: BTreeSet<Uuid>) -> anyhow::Result<()> {
        ensure!(!voters.is_empty(), "empty voter set");
        for id in &voters {
            let p = self.net.cfg.peers.get(id).context("unconfigured peer")?;
            if !self.voter(*id) {
                self.raft
                    .add_learner(*id, BasicNode::new(&p.address), true)
                    .await?;
            }
        }
        self.raft.change_membership(voters, false).await?;
        Ok(())
    }
    async fn route(&self, peer: Uuid, req: Request<Body>) -> anyhow::Result<Response<Body>> {
        if req
            .headers()
            .get("x-halcyon-pool")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok())
            != Some(self.net.cfg.pool_id)
        {
            return Ok(StatusCode::FORBIDDEN.into_response());
        }
        let (parts, body) = req.into_parts();
        let path = parts.uri.path();
        let body = to_bytes(body, MAX_BODY).await?;
        if parts.method == Method::POST {
            match path {
                "/internal/append" => {
                    let r: AppendEntriesRequest<Types> = decode(&body)?;
                    ensure!(r.vote.leader_id.node_id == peer, "leader identity mismatch");
                    return cbor(&self.raft.append_entries(r).await);
                }
                "/internal/vote" => {
                    let r: VoteRequest<Uuid> = decode(&body)?;
                    ensure!(
                        r.vote.leader_id.node_id == peer,
                        "candidate identity mismatch"
                    );
                    return cbor(&self.raft.vote(r).await);
                }
                "/internal/snapshot" => {
                    let r: InstallSnapshotRequest<Types> = decode(&body)?;
                    ensure!(r.vote.leader_id.node_id == peer, "leader identity mismatch");
                    return cbor(&self.raft.install_snapshot(r).await);
                }
                _ => {}
            }
        }
        if !self.voter(peer) {
            return Ok(StatusCode::FORBIDDEN.into_response());
        }
        if parts.method == Method::POST {
            match path {
                "/internal/forward" => {
                    let p: Proposal = decode(&body)?;
                    if p.caller != peer {
                        return Ok(StatusCode::FORBIDDEN.into_response());
                    }
                    return cbor(&self.write_here(p).await?);
                }
                "/internal/capacity" => {
                    self.offers.update(peer, decode(&body)?)?;
                    return cbor(&());
                }
                "/internal/read" => return cbor(&self.read_here().await?),
                "/internal/changes" => return cbor(&self.changes_here(decode(&body)?).await?),
                _ => {}
            }
        }
        let Some(resource) = path.strip_prefix("/federation/v1/") else {
            return Ok(StatusCode::NOT_FOUND.into_response());
        };
        let seg: Vec<_> = resource.split('/').collect();
        if let ["hosts", key, "capacity"] = seg.as_slice()
            && parts.method == Method::PUT
        {
            if peer.to_string() != *key {
                return Ok(StatusCode::FORBIDDEN.into_response());
            }
            let Ok(msg) = serde_json::from_slice(&body) else {
                return Ok(StatusCode::BAD_REQUEST.into_response());
            };
            return Ok(if self.offers.update(peer, msg).is_ok() {
                StatusCode::NO_CONTENT
            } else {
                StatusCode::CONFLICT
            }
            .into_response());
        }
        if parts.method == Method::GET {
            let pool = self.read().await?;
            let data = match seg.as_slice() {
                ["tickets", id] => Uuid::parse_str(id)
                    .ok()
                    .and_then(|id| pool.tickets().get(&id))
                    .map(serde_json::to_value),
                ["allocations", id] => Uuid::parse_str(id)
                    .ok()
                    .and_then(|id| pool.allocations().get(&id))
                    .map(serde_json::to_value),
                _ => None,
            };
            return Ok(match data {
                Some(v) => axum::Json(v?).into_response(),
                None => StatusCode::NOT_FOUND.into_response(),
            });
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Write {
            request_id: Uuid,
            command: Command,
        }
        let Ok(w) = serde_json::from_slice::<Write>(&body) else {
            return Ok(StatusCode::BAD_REQUEST.into_response());
        };
        let matches = match (&w.command, seg.as_slice()) {
            (Command::Enqueue(t), ["tickets", id]) => t.id.to_string() == *id,
            (Command::Cancel(t), ["tickets", id]) => t.id.to_string() == *id,
            (Command::Allocate { id, .. }, ["allocations", key]) => id.to_string() == *key,
            (Command::Accept { id, ticket }, ["allocations", key, "acceptances", t]) => {
                id.to_string() == *key && ticket.id.to_string() == *t
            }
            (Command::HostReady { id, .. }, ["allocations", key, "readiness"]) => {
                id.to_string() == *key
            }
            (Command::Abort { id }, ["allocations", key, "abort"]) => id.to_string() == *key,
            (Command::Finish { id, .. } | Command::Settle { id, .. }, ["results", key]) => {
                id.to_string() == *key
            }
            _ => false,
        };
        if !matches {
            return Ok(StatusCode::BAD_REQUEST.into_response());
        }
        let method = if matches!(w.command, Command::Cancel(_)) {
            Method::DELETE
        } else {
            Method::PUT
        };
        if method != parts.method {
            return Ok(StatusCode::METHOD_NOT_ALLOWED.into_response());
        }
        // The originating certificate must reach the leader to authorize a REST write.
        if let Some(leader) = self.raft.current_leader().await
            && leader != self.net.cfg.node_id
        {
            let address = &self
                .net
                .cfg
                .peers
                .get(&leader)
                .context("unknown leader")?
                .address;
            return Ok(
                axum::response::Redirect::temporary(&format!("https://{address}{path}"))
                    .into_response(),
            );
        }
        let result = self
            .submit_as(Proposal {
                caller: peer,
                request_id: w.request_id,
                command: w.command,
            })
            .await?;
        Ok(match result {
            Ok(()) => StatusCode::NO_CONTENT,
            Err(ApplyError::Forbidden) => StatusCode::FORBIDDEN,
            Err(ApplyError::Missing) => StatusCode::NOT_FOUND,
            Err(ApplyError::Invalid) => StatusCode::BAD_REQUEST,
            Err(ApplyError::Conflict) => StatusCode::CONFLICT,
        }
        .into_response())
    }
}
