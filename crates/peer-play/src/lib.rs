use anyhow::{Context, ensure};
use halcyon_federation::{Capacity, Federation};
use halcyon_game::Rooms;
use halcyon_matchmaking::{
    Allocation, Command, Endpoint, Phase, Queue, Ticket, TicketPhase, TicketRef, WorkingSet,
    result_digest,
};
use halcyon_platform::{AbortReason, LobbyPhase};
use halcyon_platform::{
    AppState, BotDifficulty, FederatedIntent, PlatformError, Reservation, plan_teams_for_mode,
};
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
mod chat;
mod journal;
mod latency;
mod observation;
use journal::Journal;

#[derive(serde::Serialize, serde::Deserialize)]
struct AbortedRoom {
    reason: AbortReason,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub pool_id: Uuid,
    pub node_id: Uuid,
    pub database: PathBuf,
    pub rules_digest: String,
    /// An explicit host allowlist of RTT estimates in milliseconds. Empty measures voters on enqueue.
    pub peer_latency_ms: BTreeMap<Uuid, u32>,
    pub max_latency_ms: u32,
}
struct Inner {
    fed: Federation,
    platform: AppState,
    rooms: Rooms,
    cfg: Config,
    journal: Journal,
    chat_wake: tokio::sync::Notify,
    rtt: latency::HomeRtt,
    difficulty_rules: BTreeMap<BotDifficulty, String>,
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
        chat::attach(&federation, &platform, config.pool_id);
        let journal = Journal::open(&config.database, config.pool_id, config.node_id)?;
        // A reader of the home journal may restart without the game service.
        // Only a locally recorded room missing from this host can be lost.
        let hosted: BTreeSet<_> = journal.unsettled_rooms()?.into_iter().collect();
        let lost_rooms = hosted
            .iter()
            .copied()
            .filter(|id| !rooms.contains(*id))
            .collect();
        let difficulty_rules = BotDifficulty::ALL
            .into_iter()
            .map(|level| (level, level.rules_digest(&config.rules_digest)))
            .collect();
        let inner = Arc::new(Inner {
            fed: federation,
            platform,
            rooms,
            cfg: config,
            journal,
            chat_wake: Default::default(),
            rtt: Default::default(),
            difficulty_rules,
        });
        let (stop, mut stopped) = watch::channel(false);
        let (done_tx, done) = watch::channel(false);
        let worker = inner.clone();
        tokio::spawn(async move {
            let observer_stop = stopped.clone();
            let chat_stop = stopped.clone();
            tokio::join!(
                async {
                    let mut state = Worker {
                        lost_rooms,
                        hosted,
                        ..Worker::default()
                    };
                    loop {
                        tokio::select! {_=stopped.changed()=>break,_=tokio::time::sleep(Duration::from_millis(300))=>{}}
                        if let Err(e) = state.round(&worker).await {
                            tracing::debug!(%e,"peer matchmaking round deferred");
                        }
                    }
                },
                observation::reconcile(&worker, observer_stop),
                chat::reconcile(&worker, chat_stop),
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
        self.join_roster(token, mode, seats, None, false, BTreeMap::new())
            .await
    }
    pub async fn join_party_or_solo(
        &self,
        token: &str,
        mode: String,
        seats: u8,
    ) -> halcyon_platform::Result<()> {
        self.join_roster(token, mode, seats, None, true, BTreeMap::new())
            .await
    }
    pub async fn join_party_with_difficulty(
        &self,
        token: &str,
        mode: String,
        seats: u8,
        difficulty: Option<BotDifficulty>,
    ) -> halcyon_platform::Result<()> {
        self.join_roster(token, mode, seats, difficulty, true, BTreeMap::new())
            .await
    }

    pub async fn join_party_with_site_report(
        &self,
        token: &str,
        mode: String,
        seats: u8,
        difficulty: Option<BotDifficulty>,
        estimates: BTreeMap<Uuid, u32>,
    ) -> halcyon_platform::Result<()> {
        self.join_roster(token, mode, seats, difficulty, true, estimates)
            .await
    }
    async fn join_roster(
        &self,
        token: &str,
        mode: String,
        seats: u8,
        difficulty: Option<BotDifficulty>,
        with_party: bool,
        estimates: BTreeMap<Uuid, u32>,
    ) -> halcyon_platform::Result<()> {
        let s = &self.inner;
        if seats == 0 || seats > 10 {
            return Err(PlatformError::Invalid("invalid seat count"));
        }
        let intent = if with_party {
            s.platform
                .federated_join_party_with_site_report(
                    token,
                    s.cfg.pool_id,
                    mode,
                    seats,
                    difficulty,
                    estimates,
                )
                .await?
        } else {
            s.platform
                .federated_join(token, s.cfg.pool_id, mode, seats)
                .await?
        };
        s.enqueue(&intent, None)
            .await
            .map_err(|_| PlatformError::HostUnavailable)?;
        Ok(())
    }
    /// Return only the committed allocation associated with this session's home ticket.
    pub async fn native_offer(
        &self,
        session: &str,
    ) -> halcyon_platform::Result<Option<halcyon_platform::NativeOffer>> {
        use halcyon_platform::{NativeOffer, NativeSeat};
        let home = &self.inner;
        let Some(binding) = home
            .platform
            .federated_offer_binding(session, home.cfg.pool_id)
            .await?
        else {
            return Ok(None);
        };
        Ok(home.fed.inspect_applied(|pool| {
            let a = pool.allocations().get(&binding.room)?;
            if a.phase == Phase::Aborted {
                return None;
            }
            let mut authorized = false;
            let mut seats = Vec::new();
            for key in &a.tickets {
                let entry = pool.tickets().get(&key.id)?;
                if entry.ticket.generation != key.generation {
                    return None;
                }
                let own = key.id == binding.ticket && entry.ticket.home == home.cfg.node_id;
                authorized |= own && entry.ticket.players.contains(&binding.player);
                let accepted = a.accepted.contains(&key.id);
                seats.extend(entry.ticket.players.iter().map(|&player| NativeSeat {
                    player,
                    accepted: accepted || (own && binding.accepted.contains(&player)),
                }));
            }
            authorized.then_some(NativeOffer {
                id: a.id,
                seats,
                entries: a.tickets.len() as u32,
                expires: a.expires_at,
            })
        }))
    }

    pub async fn accept(&self, token: &str, room: Uuid) -> halcyon_platform::Result<()> {
        let i = self.intent(token).await?;
        if i.match_id != Some(room) || !matches!(i.phase.as_str(), "offered" | "playing") {
            return Err(PlatformError::Conflict("no offer for this match"));
        }
        let bound_room = || async {
            self.inner
                .fed
                .inspect(|pool| {
                    pool.allocations().get(&room).and_then(|a| {
                        a.tickets
                            .iter()
                            .any(|ticket| ticket.id == i.ticket)
                            .then_some((
                                a.phase.clone(),
                                a.expires_at,
                                a.accepted.contains(&i.ticket),
                            ))
                    })
                })
                .await
                .map_err(|_| PlatformError::HostUnavailable)
        };
        let mut binding = bound_room().await?;
        if matches!(binding, Some((Phase::Preparing, None, false))) {
            self.inner
                .expire(room)
                .await
                .map_err(|_| PlatformError::HostUnavailable)?;
            binding = bound_room().await?;
        }
        let deadline = match binding {
            Some((Phase::Ready, _, true) | (Phase::Preparing, _, true)) => return Ok(()),
            Some((Phase::Preparing, Some(deadline), false)) => deadline,
            _ => return Err(PlatformError::Conflict("no offer for this match")),
        };
        let i = self
            .inner
            .platform
            .federated_accept_in_window(token, i.ticket, room, deadline)
            .await?;
        if i.accepted_match != Some(room) || i.phase == "playing" {
            return Ok(());
        }
        self.inner.accept(&i, room).await.map_err(|error| {
            if error.downcast_ref::<halcyon_matchmaking::Error>()
                == Some(&halcyon_matchmaking::Error::Conflict)
            {
                PlatformError::Conflict("federated offer is no longer open")
            } else {
                PlatformError::HostUnavailable
            }
        })
    }
    pub async fn reject_offer(&self, token: &str, room: Uuid) -> halcyon_platform::Result<bool> {
        let Some(ticket) = self
            .inner
            .platform
            .native_reject_ticket(token, room, self.inner.cfg.pool_id)
            .await?
        else {
            return Ok(false);
        };
        let request = Uuid::new_v5(&ticket, format!("reject/{room}").as_bytes());
        match self
            .inner
            .fed
            .submit(
                request,
                Command::Reject {
                    id: room,
                    ticket: TicketRef {
                        id: ticket,
                        generation: 1,
                    },
                },
            )
            .await
            .map_err(|_| PlatformError::HostUnavailable)?
        {
            Ok(()) => Ok(true),
            Err(halcyon_matchmaking::Error::Conflict | halcyon_matchmaking::Error::Missing) => {
                Ok(false)
            }
            Err(_) => Err(PlatformError::HostUnavailable),
        }
    }
    pub async fn leave(&self, token: &str) -> halcyon_platform::Result<()> {
        let Some(i) = self
            .inner
            .platform
            .federated_begin_leave(token, self.inner.cfg.pool_id)
            .await?
        else {
            return Ok(());
        };
        self.inner.cancel(&i).await.map_err(|error| {
            if error.downcast_ref::<halcyon_matchmaking::Error>()
                == Some(&halcyon_matchmaking::Error::Conflict)
            {
                PlatformError::Conflict("federated match has already started")
            } else {
                PlatformError::HostUnavailable
            }
        })?;
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
    fn difficulty_for_queue(&self, queue: &Queue) -> anyhow::Result<Option<BotDifficulty>> {
        if queue.build == 147219 {
            if queue.rules_digest == self.cfg.rules_digest
                || (queue.name == halcyon_platform::LEGACY_ARAL_QUEUE
                    && queue.rules_digest == self.aral_talent_rules())
            {
                return Ok(None);
            }
            if BotDifficulty::supports_queue(&queue.name)
                && let Some((&difficulty, _)) = self
                    .difficulty_rules
                    .iter()
                    .find(|(_, digest)| **digest == queue.rules_digest)
            {
                return Ok(Some(difficulty));
            }
        }
        anyhow::bail!("incompatible queue rules")
    }

    fn aral_talent_rules(&self) -> String {
        format!(
            "{}{}",
            halcyon_platform::ARAL_TALENT_RULES_PREFIX,
            self.cfg.rules_digest
        )
    }

    async fn ticket(
        &self,
        i: &FederatedIntent,
        known_pool: Option<&WorkingSet>,
    ) -> anyhow::Result<Ticket> {
        let frozen = self.journal.ticket(i.ticket)?;
        if i.pre_freeze {
            let committed = if let Some(pool) = known_pool {
                pool.tickets
                    .get(&i.ticket)
                    .map(|entry| entry.ticket.clone())
            } else {
                self.fed
                    .inspect(|pool| {
                        pool.tickets()
                            .get(&i.ticket)
                            .map(|entry| entry.ticket.clone())
                    })
                    .await?
            };
            if let Some(ticket) = committed {
                ensure!(
                    ticket.home == self.cfg.node_id
                        && ticket.id == i.ticket
                        && ticket.generation == 1
                        && ticket.players == i.players
                        && ticket.queue.name == i.mode
                        && ticket.queue.seats == i.seats
                        && i.difficulty
                            .is_none_or(|level| self.difficulty_for_queue(&ticket.queue).ok()
                                == Some(Some(level))),
                    "legacy committed ticket does not match the durable home intent",
                );
                if frozen.as_ref() != Some(&ticket) {
                    self.journal.adopt_legacy_ticket(&ticket)?;
                }
                return Ok(ticket);
            }
        }
        if let Some(ticket) = frozen {
            return Ok(ticket);
        }
        let mut latency_ms = if self.cfg.peer_latency_ms.is_empty() {
            self.rtt.sample(&self.fed).await?
        } else {
            self.cfg.peer_latency_ms.clone()
        };
        // The reporting member can raise the estimate for the entire roster.
        // Only trusted, eligible hosts already in the baseline participate.
        for (node, ms) in &i.reported_ms {
            if let Some(baseline) = latency_ms.get_mut(node) {
                *baseline = (*baseline).max(*ms);
            }
        }
        let talent_levels = self
            .platform
            .pooled_talent_levels(self.cfg.pool_id, i.ticket)
            .await?;
        let rules_digest = if i.mode == halcyon_platform::LEGACY_ARAL_QUEUE {
            self.aral_talent_rules()
        } else {
            i.difficulty.map_or_else(
                || self.cfg.rules_digest.clone(),
                |level| self.difficulty_rules[&level].clone(),
            )
        };
        self.journal.freeze_ticket(Ticket {
            talent_levels,
            id: i.ticket,
            generation: 1,
            home: self.cfg.node_id,
            players: i.players.clone(),
            queue: Queue {
                name: i.mode.clone(),
                build: 147219,
                rules_digest,
                seats: i.seats,
            },
            latency_ms,
            max_latency_ms: self.cfg.max_latency_ms,
        })
    }
    async fn commit(&self, key: Uuid, tag: &str, cmd: Command) -> anyhow::Result<()> {
        let req = Uuid::new_v5(&key, tag.as_bytes());
        self.fed.submit(req, cmd).await??;
        Ok(())
    }
    async fn enqueue(
        &self,
        i: &FederatedIntent,
        known_pool: Option<&WorkingSet>,
    ) -> anyhow::Result<()> {
        let ticket = self.ticket(i, known_pool).await?;
        self.commit(i.ticket, "enqueue", Command::Enqueue(ticket))
            .await
    }
    async fn cancel(&self, i: &FederatedIntent) -> anyhow::Result<()> {
        // An enqueue whose response timed out may still be committed later. Serialize
        // cancellation after that same durable intent before releasing the home claim.
        self.enqueue(i, None).await?;
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
    async fn expire(&self, id: Uuid) -> anyhow::Result<()> {
        // An early attempt can race a leader whose clock is behind. The state
        // machine gates expiry, and the same room can be checked again later.
        let attempt = format!("expire/{}", unix_epoch());
        self.commit(id, &attempt, Command::Expire { id }).await?;
        Ok(())
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
        phase: LobbyPhase,
        id: Option<Uuid>,
        address: Option<String>,
        result: Option<Value>,
    ) -> anyhow::Result<()> {
        self.platform
            .federated_project(i.ticket, phase.as_str(), id, address, result)
            .await?;
        Ok(())
    }
    async fn sync_home(&self, p: &WorkingSet, i: &FederatedIntent) -> anyhow::Result<()> {
        let Some(entry) = p.tickets.get(&i.ticket) else {
            return self.enqueue(i, Some(p)).await;
        };
        if i.phase == "finished" {
            return Ok(());
        }
        match &entry.phase {
            TicketPhase::Closed => {
                let prior = p.finished(TicketRef {
                    id: i.ticket,
                    generation: entry.ticket.generation,
                });
                if let Some(room) = prior.filter(|room| {
                    !room.result.as_ref().is_some_and(|value| {
                        <AbortedRoom as serde::Deserialize>::deserialize(value)
                            .is_ok_and(|room| room.reason == AbortReason::HostRestarted)
                    })
                }) {
                    self.project(
                        i,
                        LobbyPhase::Finished,
                        Some(room.id),
                        None,
                        room.result.clone(),
                    )
                    .await
                } else {
                    self.project(i, LobbyPhase::Closed, None, None, None)
                        .await?;
                    self.platform.federated_release(i.ticket).await?;
                    Ok(())
                }
            }
            TicketPhase::Queued => {
                if i.cancel {
                    return self.cancel(i).await;
                }
                self.project(i, LobbyPhase::Queued, None, None, None).await
            }
            TicketPhase::Assigned(id) => {
                let a = &p.allocations[id];
                if a.phase == Phase::Ready {
                    let ep = a
                        .endpoint
                        .as_ref()
                        .context("ready allocation without endpoint")?;
                    return self
                        .project(i, LobbyPhase::Playing, Some(*id), Some(ep.address()), None)
                        .await;
                }
                if i.cancel {
                    return self.cancel(i).await;
                }
                self.project(i, LobbyPhase::Offered, Some(*id), None, None)
                    .await?;
                if i.accepted_match == Some(*id) && !a.accepted.contains(&i.ticket) {
                    self.accept(i, *id).await?
                }
                Ok(())
            }
        }
    }
}
fn unix_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
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
    lost_rooms: BTreeSet<Uuid>,
    hosted: BTreeSet<Uuid>,
    anchors: BTreeMap<Uuid, Instant>,
    active: BTreeSet<Uuid>,
    advertised: BTreeMap<Queue, Instant>,
}
impl Worker {
    async fn round(&mut self, s: &Inner) -> anyhow::Result<()> {
        let intents = s.platform.pending_federated_intents(s.cfg.pool_id).await?;
        let pool = s
            .fed
            .inspect(|pool| {
                pool.working_set(
                    intents.iter().map(|intent| intent.ticket),
                    self.hosted.iter().copied(),
                )
            })
            .await?;
        for i in intents {
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
        let now = unix_epoch();
        for a in pool.allocations.values() {
            if a.phase == Phase::Preparing && a.expires_at.is_none_or(|deadline| now >= deadline) {
                s.expire(a.id).await?;
                continue;
            }
            if a.host != s.cfg.node_id {
                continue;
            }
            let active = matches!(a.phase, Phase::Preparing | Phase::Ready);
            if active {
                self.hosted.insert(a.id);
            } else if !self.hosted.contains(&a.id) {
                continue;
            }
            match self.host(s, &pool, a).await {
                Ok(()) if !active => {
                    self.hosted.remove(&a.id);
                }
                Err(e) => tracing::debug!(%e,room=%a.id,"host reconciliation deferred"),
                _ => {}
            }
        }
        self.plan(s, &pool).await
    }
    async fn host(&mut self, s: &Inner, p: &WorkingSet, a: &Allocation) -> anyhow::Result<()> {
        match a.phase {
            Phase::Preparing => {
                let mut players = Vec::new();
                let mut sizes = Vec::new();
                let mut homes = Vec::new();
                let mut talent_levels = Vec::new();
                let mut queue = None;
                for key in &a.tickets {
                    let t = &p.tickets[&key.id].ticket;
                    players.extend_from_slice(&t.players);
                    homes.extend(std::iter::repeat_n(t.home, t.players.len()));
                    sizes.push(t.players.len());
                    if t.talent_levels.is_empty() {
                        talent_levels.resize_with(players.len(), Default::default);
                    } else {
                        talent_levels.extend_from_slice(&t.talent_levels);
                    }
                    queue = Some(&t.queue);
                }
                let queue = queue.context("empty roster")?;
                let Ok(difficulty) = s.difficulty_for_queue(queue) else {
                    return s
                        .commit(a.id, "incompatible-rules", Command::Abort { id: a.id })
                        .await;
                };
                if talent_levels
                    .iter()
                    .all(halcyon_platform::TalentLevels::is_empty)
                    || queue.rules_digest != s.aral_talent_rules()
                {
                    talent_levels.clear();
                }
                let prepared = s.rooms.prepare_federated(
                    Reservation {
                        talent_levels,
                        match_id: a.id,
                        players,
                        teams: plan_teams_for_mode(&queue.name, &sizes),
                        mode: queue.name.clone(),
                        difficulty,
                    },
                    s.cfg.pool_id,
                    homes,
                );
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
                if self.lost_rooms.contains(&a.id) && !s.rooms.contains(a.id) {
                    // Only the assigned host can sign this terminal outcome. The
                    // normal outbox retries it across quorum loss or restart.
                    ensure!(
                        s.journal.finish(
                            a.id,
                            &serde_json::to_value(AbortedRoom {
                                reason: AbortReason::HostRestarted
                            })?
                        )?,
                        "missing recorded hosted room"
                    );
                    self.lost_rooms.remove(&a.id);
                    return Ok(());
                }
                if !self.active.contains(&a.id) && s.rooms.detail(a.id).is_some() {
                    s.rooms.activate(a.id)?;
                    self.active.insert(a.id);
                }
            }
            Phase::Aborted => {
                s.rooms.discard(a.id)?;
                s.journal.discard_preparation(a.id)?;
                self.lost_rooms.remove(&a.id);
                self.active.remove(&a.id);
            }
            Phase::Finished => {
                self.lost_rooms.remove(&a.id);
                self.active.remove(&a.id);
            }
        }
        Ok(())
    }
    async fn plan(&mut self, s: &Inner, p: &WorkingSet) -> anyhow::Result<()> {
        let mut anchors: BTreeMap<Queue, (u64, Uuid)> = BTreeMap::new();
        for e in p
            .tickets
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
            p.tickets
                .get(id)
                .is_some_and(|e| e.phase == TicketPhase::Queued)
        });
        let voters: Vec<_> = s.fed.metrics().membership_config.voter_ids().collect();
        for (queue, (_, anchor)) in anchors {
            if s.difficulty_for_queue(&queue).is_err() {
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
            let Some(group) = p.plan(anchor, &s.fed.host_offers()) else {
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
