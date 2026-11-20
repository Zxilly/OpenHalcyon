use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::ensure;
use halcyon_federation::{
    Federation, OBSERVER_EXIT_BATCH, ObserverExitCandidate, ObserverReply, ObserverRequest,
};
use halcyon_game::Rooms;
use halcyon_matchmaking::Phase;
use halcyon_platform::{HostedSpectator, MatchHost, PlatformError, PooledSpectator, Result};
use uuid::Uuid;

use crate::{Inner, PeerPlay};
use tokio::{
    sync::watch,
    time::{Duration, sleep},
};

#[derive(Clone)]
struct Issued {
    room: Uuid,
    target: Uuid,
    viewer: Uuid,
    revoked: bool,
    exited: Arc<AtomicBool>,
    spectator_id: Option<Uuid>,
}

pub(super) fn attach(federation: &Federation, rooms: Rooms) {
    // Scoped to this host runtime. A restarted host may not recreate a Ready room.
    let registry = Arc::new(Mutex::new(BTreeMap::<(Uuid, Uuid), Issued>::new()));
    let lookup = registry.clone();
    federation.attach_observer_exit_query(move |home, candidates| {
        let registry = lookup
            .lock()
            .map_err(|_| anyhow::anyhow!("observer registry unavailable"))?;
        Ok(candidates
            .iter()
            .filter_map(|candidate| {
                let r = candidate.binding;
                let entry = registry.get(&(home, r.request))?;
                (entry.room == r.room
                    && entry.viewer == r.viewer
                    && entry.target == r.target
                    && entry.spectator_id == Some(candidate.grant)
                    && entry.exited.load(Ordering::Acquire))
                .then_some(r.request)
            })
            .collect())
    });
    federation.attach_observers(move |home, request| {
        let mut registry = registry
            .lock()
            .map_err(|_| anyhow::anyhow!("observer registry unavailable"))?;
        let key = (home, request.request);
        let active: BTreeSet<Uuid> = rooms
            .list()
            .into_iter()
            .filter(|room| room["phase"] != "ended")
            .filter_map(|room| Uuid::parse_str(room["id"].as_str()?).ok())
            .collect();
        registry.retain(|_, record| active.contains(&record.room));

        if let Some(prior) = registry.get_mut(&key) {
            ensure!(
                prior.room == request.room
                    && prior.target == request.target
                    && prior.viewer == request.viewer,
                "observer request identity changed"
            );
            if request.revoke {
                if let Some(id) = prior.spectator_id {
                    rooms.stop_spectating(request.room, id)?;
                }
                prior.revoked = true;
                return Ok(ObserverReply::Revoked);
            }
            ensure!(!prior.exited.load(Ordering::Acquire), "observer exited");
            ensure!(!prior.revoked, "observer request was revoked");
        } else {
            if request.revoke {
                // The home only publishes a capability after its durable claim is
                // confirmed. A grant still in flight cannot be exposed after an
                // early revoke and expires on the host if its response is lost.
                return Ok(ObserverReply::Revoked);
            }
            ensure!(
                registry.values().filter(|r| r.room == request.room).count() < 256,
                "observer request capacity unavailable"
            );
        }
        let mut namespace = [0u8; 32];
        namespace[..16].copy_from_slice(request.viewer.as_bytes());
        namespace[16..].copy_from_slice(request.request.as_bytes());
        let observer = Uuid::new_v5(&home, &namespace);
        let exited = registry
            .get(&key)
            .map(|r| r.exited.clone())
            .unwrap_or_default();
        let marker = exited.clone();
        let hosted = rooms.spectate_with_exit(
            request.room,
            observer,
            Arc::new(move |_| {
                marker.store(true, Ordering::Release);
                Ok(())
            }),
        )?;
        registry.insert(
            key,
            Issued {
                room: request.room,
                target: request.target,
                viewer: request.viewer,
                revoked: false,
                exited,
                spectator_id: Some(hosted.spectator_id),
            },
        );
        Ok(ObserverReply::Granted {
            address: hosted.address,
            spectator_id: hosted.spectator_id,
        })
    });
}

fn request(claim: &PooledSpectator, revoke: bool) -> ObserverRequest {
    ObserverRequest {
        room: claim.room,
        request: claim.request,
        viewer: claim.viewer,
        target: claim.friend,
        revoke,
    }
}

impl PeerPlay {
    /// Try the authenticated friend's committed pooled match, or return false for the local route.
    pub async fn spectate_friend(&self, token: &str, friend: Uuid) -> Result<bool> {
        let inner = &self.inner;
        let Some(room) = inner
            .platform
            .pooled_spectator_target(token, friend, inner.cfg.pool_id)
            .await?
        else {
            return Ok(false);
        };
        let host = inner
            .fed
            .inspect(|snapshot| {
                snapshot
                    .allocations()
                    .get(&room)
                    .filter(|a| a.phase == Phase::Ready)
                    .map(|a| a.host)
            })
            .await
            .map_err(|_| PlatformError::HostUnavailable)?
            .ok_or(PlatformError::Conflict(
                "friend is unavailable for spectating",
            ))?;
        let claim = inner
            .platform
            .claim_pooled_spectator(token, friend, inner.cfg.pool_id, host, room)
            .await?;
        let reply = inner
            .fed
            .observer_request(host, request(&claim, false))
            .await;
        let reply = match reply {
            Ok(ObserverReply::Granted {
                address,
                spectator_id,
            }) => HostedSpectator {
                address,
                spectator_id,
            },
            _ => {
                inner
                    .platform
                    .cancel_pooled_spectator_request(claim.request)
                    .await?;
                let _ = inner.revoke_observer(&claim).await;
                return Err(PlatformError::HostUnavailable);
            }
        };
        if let Err(error) = inner
            .platform
            .publish_pooled_spectator(claim.request, reply)
            .await
        {
            inner
                .platform
                .cancel_pooled_spectator_request(claim.request)
                .await?;
            let _ = inner.revoke_observer(&claim).await;
            return Err(error);
        }
        Ok(true)
    }

    /// Revoke the actual host-side viewer and release the local exclusive claim.
    pub async fn leave_spectating(&self, token: &str) -> Result<bool> {
        let Some(claim) = self.inner.platform.cancel_pooled_spectator(token).await? else {
            return Ok(false);
        };
        if claim.pool != self.inner.cfg.pool_id {
            return Err(PlatformError::Conflict("viewer belongs to another pool"));
        }
        self.inner
            .revoke_observer(&claim)
            .await
            .map_err(|_| PlatformError::HostUnavailable)?;
        Ok(true)
    }
}

impl Inner {
    async fn revoke_observer(&self, claim: &PooledSpectator) -> anyhow::Result<()> {
        let response = self
            .fed
            .observer_request(claim.host, request(claim, true))
            .await?;
        ensure!(
            matches!(response, ObserverReply::Revoked),
            "host did not revoke observation"
        );
        self.platform.close_pooled_spectator(claim.request).await?;
        Ok(())
    }

    async fn reconcile_observers(self: &Arc<Self>) -> anyhow::Result<()> {
        let claims = self.platform.pooled_spectators(self.cfg.pool_id).await?;
        if claims.is_empty() {
            return Ok(());
        }
        let ended = self
            .fed
            .inspect(|pool| {
                claims
                    .iter()
                    .filter(|claim| {
                        pool.allocations().get(&claim.room).is_some_and(|room| {
                            matches!(room.phase, Phase::Finished | Phase::Aborted)
                        })
                    })
                    .map(|claim| claim.room)
                    .collect::<BTreeSet<_>>()
            })
            .await
            .unwrap_or_default();
        let mut by_host = BTreeMap::<Uuid, Vec<PooledSpectator>>::new();
        for claim in claims {
            if ended.contains(&claim.room) {
                self.platform.close_pooled_spectator(claim.request).await?;
            } else {
                by_host.entry(claim.host).or_default().push(claim);
            }
        }

        // Each host progresses independently. Bound concurrent connections even
        // when several hundred rooms share this home. An unavailable host costs
        // one status timeout for all its claims.
        let mut tasks = tokio::task::JoinSet::new();
        let mut hosts: Vec<_> = by_host.into_iter().collect();
        hosts.sort_by_key(|(_, claims)| !claims.iter().any(|claim| claim.cancelled));
        for (host, claims) in hosts {
            if tasks.len() >= 8 {
                tasks.join_next().await;
            }
            let inner = self.clone();
            tasks.spawn(async move {
                if let Err(error) = inner.reconcile_host_observers(host, claims).await {
                    tracing::debug!(%host,%error,"spectator reconciliation deferred");
                }
            });
        }
        while tasks.join_next().await.is_some() {}
        Ok(())
    }

    async fn reconcile_host_observers(
        &self,
        host: Uuid,
        claims: Vec<PooledSpectator>,
    ) -> anyhow::Result<()> {
        let mut candidates = Vec::new();
        for claim in claims {
            if claim.cancelled {
                self.revoke_observer(&claim).await?;
            } else if let Some(grant) = claim.spectator_id {
                candidates.push(ObserverExitCandidate {
                    binding: request(&claim, false),
                    grant,
                });
            }
        }
        for batch in candidates.chunks(OBSERVER_EXIT_BATCH) {
            let exited = self
                .fed
                .observer_exited(host, batch)
                .await?
                .into_iter()
                .collect::<BTreeSet<_>>();
            for c in batch {
                if exited.contains(&c.binding.request) {
                    self.platform
                        .close_pooled_spectator(c.binding.request)
                        .await?;
                }
            }
        }
        Ok(())
    }
}

pub(super) async fn reconcile(inner: &Arc<Inner>, mut stopped: watch::Receiver<bool>) {
    loop {
        tokio::select! {
            _=stopped.changed()=>break,
            _=sleep(Duration::from_secs(1))=>{},
        }
        tokio::select! {
            _=stopped.changed()=>break,
            result=inner.reconcile_observers()=> {
                if let Err(error)=result {
                    tracing::debug!(%error,"remote spectator reconciliation deferred");
                }
            }
        }
    }
}
