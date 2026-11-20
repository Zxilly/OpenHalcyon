use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, ensure};
use halcyon_matchmaking::Phase;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::Federation;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ObserverRequest {
    pub room: Uuid,
    pub request: Uuid,
    pub viewer: Uuid,
    pub target: Uuid,
    pub revoke: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ObserverReply {
    Granted { address: String, spectator_id: Uuid },
    Revoked,
}

type Handler = dyn Fn(Uuid, ObserverRequest) -> anyhow::Result<ObserverReply> + Send + Sync;

#[derive(Clone, Default)]
pub(crate) struct Observers {
    manage: Arc<Mutex<Option<Arc<Handler>>>>,
    exited: Arc<Mutex<Option<Arc<ExitQuery>>>>,
}
pub const OBSERVER_EXIT_BATCH: usize = 128;
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ObserverExitCandidate {
    pub binding: ObserverRequest,
    pub grant: Uuid,
}
type ExitQuery = dyn Fn(Uuid, &[ObserverExitCandidate]) -> anyhow::Result<Vec<Uuid>> + Send + Sync;

impl Federation {
    /// Attach the room server without changing it when the reconciliation worker restarts.
    pub fn attach_observers(
        &self,
        handler: impl Fn(Uuid, ObserverRequest) -> anyhow::Result<ObserverReply> + Send + Sync + 'static,
    ) {
        let mut current = self
            .observers
            .manage
            .lock()
            .expect("observer handler register");
        if current.is_none() {
            *current = Some(Arc::new(handler));
        }
    }

    pub async fn observer_request(
        &self,
        host: Uuid,
        request: ObserverRequest,
    ) -> anyhow::Result<ObserverReply> {
        let caller = self.net.cfg.node_id;
        ensure!(
            request.revoke || self.voter(caller),
            "viewer home is not a pool voter"
        );
        if host == caller {
            return self.observer_here(caller, request).await;
        }
        self.net
            .post(host, "/internal/observe", &request, Duration::from_secs(9))
            .await
    }

    pub fn attach_observer_exit_query(
        &self,
        query: impl Fn(Uuid, &[ObserverExitCandidate]) -> anyhow::Result<Vec<Uuid>>
        + Send
        + Sync
        + 'static,
    ) {
        let mut current = self
            .observers
            .exited
            .lock()
            .expect("observer exit query register");
        if current.is_none() {
            *current = Some(Arc::new(query));
        }
    }

    /// Read the host's explicit native-exit tombstone. Older peers return an
    /// unsupported route, which the home retries without changing the claim.
    pub async fn observer_exited(
        &self,
        host: Uuid,
        candidates: &[ObserverExitCandidate],
    ) -> anyhow::Result<Vec<Uuid>> {
        ensure!(
            candidates.len() <= OBSERVER_EXIT_BATCH,
            "too many spectator claims"
        );
        if host == self.net.cfg.node_id {
            return self.observer_exit_here(host, candidates);
        }
        self.net
            .post(
                host,
                "/internal/observe/exited",
                &candidates,
                Duration::from_secs(2),
            )
            .await
    }

    pub(crate) fn observer_exit_here(
        &self,
        home: Uuid,
        candidates: &[ObserverExitCandidate],
    ) -> anyhow::Result<Vec<Uuid>> {
        ensure!(
            candidates.len() <= OBSERVER_EXIT_BATCH,
            "too many spectator claims"
        );
        let handler = self
            .observers
            .exited
            .lock()
            .map_err(|_| anyhow::anyhow!("observer exit query unavailable"))?
            .clone()
            .context("observation unsupported")?;
        handler(home, candidates)
    }

    pub(crate) async fn observer_here(
        &self,
        home: Uuid,
        request: ObserverRequest,
    ) -> anyhow::Result<ObserverReply> {
        if !request.revoke {
            ensure!(
                self.members()
                    .await?
                    .configurations
                    .iter()
                    .flatten()
                    .any(|id| *id == home),
                "viewer home is not a pool voter"
            );
            self.inspect(|pool| -> anyhow::Result<()> {
                let room = pool
                    .allocations()
                    .get(&request.room)
                    .context("room is unavailable")?;
                ensure!(
                    room.phase == Phase::Ready && room.host == self.net.cfg.node_id,
                    "room is unavailable"
                );
                ensure!(
                    room.tickets.iter().any(|ticket| {
                        pool.tickets().get(&ticket.id).is_some_and(|entry| {
                            entry.ticket.home == home
                                && entry.ticket.players.contains(&request.target)
                        })
                    }),
                    "target is unavailable"
                );
                ensure!(
                    pool.active_ticket(home, request.viewer).is_none(),
                    "viewer is already participating"
                );
                Ok(())
            })
            .await??;
        }
        let handler = self
            .observers
            .manage
            .lock()
            .map_err(|_| anyhow::anyhow!("observer handler unavailable"))?
            .clone()
            .context("room server does not support remote observation")?;
        handler(home, request)
    }
}
