use anyhow::{Context, ensure};
use halcyon_federation::{Capacity, Federation};
use halcyon_game::Rooms;
use halcyon_matchmaking::{
    Allocation, Command, Endpoint, Phase, Pool, Queue, Ticket, TicketPhase, TicketRef, plan,
    result_digest,
};
use halcyon_platform::{AppState, FederatedIntent, PlatformError, Reservation, plan_teams};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::watch;
use uuid::Uuid;
mod journal;
mod observation;
use journal::Journal;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub pool_id: Uuid,
    pub node_id: Uuid,
    pub database: PathBuf,
    pub rules_digest: String,
    /// Operator approved RTT estimates in milliseconds, keyed by candidate host.
    pub peer_latency_ms: BTreeMap<Uuid, u32>,
    pub max_latency_ms: u32,
}
struct Inner {
    fed: Federation,
    platform: AppState,
    rooms: Rooms,
    cfg: Config,
    journal: Journal,
}
#[derive(Clone)]
pub struct PeerPlay {
    inner: Arc<Inner>,
    stop: watch::Sender<bool>,
    done: watch::Receiver<bool>,
}
impl PeerPlay {
    pub fn start(
        federation: Federation,
        platform: AppState,
        rooms: Rooms,
        config: Config,
    ) -> anyhow::Result<Self> {
        ensure!(
            config.node_id == federation.metrics().id,
            "node identity mismatch"
        );
        ensure!(!config.rules_digest.is_empty(), "empty rules digest");
        observation::attach(&federation, rooms.clone());
        let journal = Journal::open(&config.database, config.pool_id, config.node_id)?;
        let inner = Arc::new(Inner {
            fed: federation,
            platform,
            rooms,
            cfg: config,
            journal,
        });
        let (stop, mut stopped) = watch::channel(false);
        let (done_tx, done) = watch::channel(false);
        let worker = inner.clone();
        tokio::spawn(async move {
            let observer_stop = stopped.clone();
            tokio::join!(
                async {
                    let mut state = Worker::default();
                    loop {
                        tokio::select! {_=stopped.changed()=>break,_=tokio::time::sleep(Duration::from_millis(300))=>{}}
                        if let Err(e) = state.round(&worker).await {
                            tracing::debug!(%e,"peer matchmaking round deferred");
                        }
                    }
                },
                observation::reconcile(&worker, observer_stop),
            );
            done_tx.send_replace(true);
        });
        Ok(Self { inner, stop, done })
    }
    pub async fn shutdown(&self) {
        self.stop.send_replace(true);
        let mut done = self.done.clone();
        let _ = done.wait_for(|v| *v).await;
    }
    pub async fn join(&self, token: &str, mode: String, seats: u8) -> halcyon_platform::Result<()> {
        self.join_roster(token, mode, seats, false).await
    }
    pub async fn join_party_or_solo(
        &self,
        token: &str,
        mode: String,
        seats: u8,
    ) -> halcyon_platform::Result<()> {
        self.join_roster(token, mode, seats, true).await
    }
    async fn join_roster(
        &self,
        token: &str,
        mode: String,
        seats: u8,
        with_party: bool,
    ) -> halcyon_platform::Result<()> {
        let s = &self.inner;
        if seats == 0 || seats > 10 {
            return Err(PlatformError::Invalid("invalid seat count"));
        }
        let intent = if with_party {
            s.platform
                .federated_join_party_or_solo(token, s.cfg.pool_id, mode, seats)
                .await?
        } else {
            s.platform
                .federated_join(token, s.cfg.pool_id, mode, seats)
                .await?
        };
        s.enqueue(&intent)
            .await
            .map_err(|_| PlatformError::HostUnavailable)?;
        Ok(())
    }
    pub async fn accept(&self, token: &str, room: Uuid) -> halcyon_platform::Result<()> {
        let i = self.intent(token).await?;
        if i.match_id != Some(room) || !matches!(i.phase.as_str(), "offered" | "playing") {
            return Err(PlatformError::Conflict("no offer for this match"));
        }
        let i = self
            .inner
            .platform
            .federated_action_as(token, i.ticket, Some(room), false)
            .await?;
        if i.accepted_match != Some(room) || i.phase == "playing" {
            return Ok(());
        }
        self.inner
            .accept(&i, room)
            .await
            .map_err(|_| PlatformError::HostUnavailable)
    }
    pub async fn leave(&self, token: &str) -> halcyon_platform::Result<()> {
        let i = self.intent(token).await?;
        if i.phase == "finished" {
            return self
                .inner
                .platform
                .federated_acknowledge(token, i.ticket)
                .await;
        }
        if i.phase == "playing" {
            return Err(PlatformError::Conflict("match is already playing"));
        }
        let i = self
            .inner
            .platform
            .federated_action_as(token, i.ticket, None, true)
            .await?;
        self.inner
            .cancel(&i)
            .await
            .map_err(|_| PlatformError::HostUnavailable)?;
        self.inner
            .platform
            .federated_project(i.ticket, "closed", None, None, None)
            .await?;
        self.inner.platform.federated_release(i.ticket).await
    }
    async fn intent(&self, token: &str) -> halcyon_platform::Result<FederatedIntent> {
        self.inner
            .platform
            .federated_intent(token)
            .await?
            .filter(|i| i.pool == self.inner.cfg.pool_id)
            .ok_or(PlatformError::Conflict("no ticket in this pool"))
    }
    pub fn finish(&self, id: Uuid, result: Value) -> anyhow::Result<bool> {
        self.inner.journal.finish(id, &result)
    }
}
impl Inner {
    fn ticket(&self, i: &FederatedIntent) -> Ticket {
        Ticket {
            id: i.ticket,
            generation: 1,
            home: self.cfg.node_id,
            players: i.players.clone(),
            queue: Queue {
                name: i.mode.clone(),
                build: 147219,
                rules_digest: self.cfg.rules_digest.clone(),
                seats: i.seats,
            },
            latency_ms: self.cfg.peer_latency_ms.clone(),
            max_latency_ms: self.cfg.max_latency_ms,
        }
    }
    async fn commit(&self, key: Uuid, tag: &str, cmd: Command) -> anyhow::Result<()> {
        let req = Uuid::new_v5(&key, tag.as_bytes());
        self.fed.submit(req, cmd).await??;
        Ok(())
    }
    async fn enqueue(&self, i: &FederatedIntent) -> anyhow::Result<()> {
        self.commit(i.ticket, "enqueue", Command::Enqueue(self.ticket(i)))
            .await
    }
    async fn cancel(&self, i: &FederatedIntent) -> anyhow::Result<()> {
        // An enqueue whose response timed out may still be committed later. Serialize
        // cancellation after that same durable intent before releasing the home claim.
        self.enqueue(i).await?;
        self.commit(
            i.ticket,
            "cancel",
            Command::Cancel(TicketRef {
                id: i.ticket,
                generation: 1,
            }),
        )
        .await
    }
    async fn accept(&self, i: &FederatedIntent, id: Uuid) -> anyhow::Result<()> {
        self.commit(
            i.ticket,
            &format!("accept/{id}"),
            Command::Accept {
                id,
                ticket: TicketRef {
                    id: i.ticket,
                    generation: 1,
                },
            },
        )
        .await
    }
    async fn project(
        &self,
        i: &FederatedIntent,
        phase: &str,
        id: Option<Uuid>,
        address: Option<String>,
        result: Option<Value>,
    ) -> anyhow::Result<()> {
        self.platform
            .federated_project(i.ticket, phase, id, address, result)
            .await?;
        Ok(())
    }
    async fn sync_home(&self, p: &Pool, i: &FederatedIntent) -> anyhow::Result<()> {
        let Some(entry) = p.tickets().get(&i.ticket) else {
            return self.enqueue(i).await;
        };
        if i.phase == "finished" {
            return Ok(());
        }
        match &entry.phase {
            TicketPhase::Closed => {
                let prior = p.allocations().values().find(|a| {
                    a.phase == Phase::Finished && a.tickets.iter().any(|t| t.id == i.ticket)
                });
                if let Some(room) = prior {
                    self.project(i, "finished", Some(room.id), None, room.result.clone())
                        .await
                } else {
                    self.project(i, "closed", None, None, None).await?;
                    self.platform.federated_release(i.ticket).await?;
                    Ok(())
                }
            }
            TicketPhase::Queued => {
                if i.cancel {
                    return self.cancel(i).await;
                }
                self.project(i, "queued", None, None, None).await
            }
            TicketPhase::Assigned(id) => {
                let a = &p.allocations()[id];
                if a.phase == Phase::Ready {
                    let ep = a
                        .endpoint
                        .as_ref()
                        .context("ready allocation without endpoint")?;
                    return self
                        .project(i, "playing", Some(*id), Some(ep.address()), None)
                        .await;
                }
                if i.cancel {
                    return self.cancel(i).await;
                }
                self.project(i, "offered", Some(*id), None, None).await?;
                if i.accepted_match == Some(*id) && !a.accepted.contains(&i.ticket) {
                    self.accept(i, *id).await?
                }
                Ok(())
            }
        }
    }
}
trait Address {
    fn address(&self) -> String;
}
impl Address for Endpoint {
    fn address(&self) -> String {
        if self.host.contains(':') && !self.host.starts_with('[') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}
#[derive(Default)]
struct Worker {
    revision: Option<u64>,
    pool: Arc<Pool>,
    anchors: BTreeMap<Uuid, Instant>,
    preparing: BTreeMap<Uuid, Instant>,
    active: BTreeSet<Uuid>,
    advertised: BTreeMap<Queue, Instant>,
}
impl Worker {
    async fn round(&mut self, s: &Inner) -> anyhow::Result<()> {
        let (revision, changed) = s.fed.changes(self.revision).await?;
        if let Some(pool) = changed {
            self.pool = Arc::new(pool);
        }
        self.revision = Some(revision);
        let pool = self.pool.clone();
        for i in s.platform.federated_intents(s.cfg.pool_id).await? {
            if let Err(e) = s.sync_home(&pool, &i).await {
                tracing::debug!(%e,ticket=%i.ticket,"home reconciliation deferred")
            }
        }
        for (id, result) in s.journal.outbox()? {
            let digest = result_digest(&result)?;
            s.commit(id, "settle", Command::Settle { id, digest, result })
                .await?;
            s.journal.delivered(id)?;
        }
        for a in pool.allocations().values() {
            if a.host == s.cfg.node_id
                && let Err(e) = self.host(s, &pool, a).await
            {
                tracing::debug!(%e,room=%a.id,"host reconciliation deferred");
            }
        }
        self.preparing.retain(|id, _| {
            pool.allocations()
                .get(id)
                .is_some_and(|a| a.phase == Phase::Preparing)
        });
        for a in pool
            .allocations()
            .values()
            .filter(|a| a.phase == Phase::Preparing)
        {
            if self
                .preparing
                .entry(a.id)
                .or_insert_with(Instant::now)
                .elapsed()
                > Duration::from_secs(35)
            {
                s.commit(a.id, "timeout", Command::Abort { id: a.id })
                    .await?;
            }
        }
        self.plan(s, &pool).await
    }
    async fn host(&mut self, s: &Inner, p: &Pool, a: &Allocation) -> anyhow::Result<()> {
        match a.phase {
            Phase::Preparing => {
                let mut players = Vec::new();
                let mut sizes = Vec::new();
                let mut mode = None;
                for key in &a.tickets {
                    let t = &p.tickets()[&key.id].ticket;
                    players.extend_from_slice(&t.players);
                    sizes.push(t.players.len());
                    mode = Some(t.queue.name.clone());
                }
                let prepared = s.rooms.prepare(Reservation {
                    match_id: a.id,
                    players,
                    teams: plan_teams(&sizes),
                    mode: mode.context("empty roster")?,
                });
                let Ok(hosted) = prepared else {
                    return s
                        .commit(a.id, "no-capacity", Command::Abort { id: a.id })
                        .await;
                };
                let addr = hosted.address.parse::<http::uri::Authority>()?;
                let endpoint = Endpoint {
                    host: addr.host().trim_matches(['[', ']']).into(),
                    port: addr.port_u16().context("missing match port")?,
                };
                if let Some(old) = &a.endpoint
                    && old != &endpoint
                {
                    return s
                        .commit(a.id, "endpoint-lost", Command::Abort { id: a.id })
                        .await;
                }
                s.journal.prepared(a.id, &hosted.address)?;
                if a.endpoint.is_none() {
                    s.commit(
                        a.id,
                        "host-ready",
                        Command::HostReady { id: a.id, endpoint },
                    )
                    .await?;
                }
            }
            Phase::Ready => {
                // Lost rooms require explicit operational isolation/termination. A
                // restarted host cannot recreate a second world for a committed game.
                if !self.active.contains(&a.id) && s.rooms.detail(a.id).is_some() {
                    s.rooms.activate(a.id)?;
                    self.active.insert(a.id);
                }
            }
            Phase::Aborted => {
                s.rooms.discard(a.id)?;
                self.active.remove(&a.id);
            }
            Phase::Finished => {
                self.active.remove(&a.id);
            }
        }
        Ok(())
    }
    async fn plan(&mut self, s: &Inner, p: &Pool) -> anyhow::Result<()> {
        let mut anchors: BTreeMap<Queue, (u64, Uuid)> = BTreeMap::new();
        for e in p
            .tickets()
            .values()
            .filter(|e| e.phase == TicketPhase::Queued)
        {
            let key = (e.sequence, e.ticket.id);
            let earliest = anchors.entry(e.ticket.queue.clone()).or_insert(key);
            *earliest = (*earliest).min(key);
        }
        self.advertised
            .retain(|queue, _| anchors.contains_key(queue));
        self.anchors.retain(|id, _| {
            p.tickets()
                .get(id)
                .is_some_and(|e| e.phase == TicketPhase::Queued)
        });
        let voters: Vec<_> = s.fed.metrics().membership_config.voter_ids().collect();
        for (queue, (_, anchor)) in anchors {
            if queue.rules_digest != s.cfg.rules_digest || queue.build != 147219 {
                continue;
            }
            if self
                .advertised
                .get(&queue)
                .is_none_or(|t| t.elapsed() > Duration::from_secs(2))
            {
                let total = s.rooms.capacity();
                let free = s.rooms.available_rooms();
                s.fed
                    .advertise(Capacity {
                        sequence: s.journal.sequence()?,
                        ttl_ms: 6500,
                        queue: queue.clone(),
                        total_seats: total * usize::from(queue.seats),
                        free_seats: free * usize::from(queue.seats),
                    })
                    .await?;
                self.advertised.insert(queue.clone(), Instant::now());
            }
            let mut ranked = voters.clone();
            ranked.sort_by_key(|v| Uuid::new_v5(&anchor, v.as_bytes()));
            let Some(rank) = ranked.iter().position(|id| *id == s.cfg.node_id) else {
                continue;
            };
            if self
                .anchors
                .entry(anchor)
                .or_insert_with(Instant::now)
                .elapsed()
                < Duration::from_millis(350 + rank as u64 * 500)
            {
                continue;
            }
            let Some(group) = plan(p, anchor, &s.fed.host_offers()) else {
                continue;
            };
            let id = Uuid::new_v4();
            s.commit(
                id,
                "allocate",
                Command::Allocate {
                    id,
                    host: group.host,
                    tickets: group.tickets,
                },
            )
            .await?;
        }
        Ok(())
    }
}
