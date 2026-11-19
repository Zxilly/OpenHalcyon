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
pub(crate) struct Observers(Arc<Mutex<Option<Arc<Handler>>>>);

impl Federation {
    /// Attach the room server without changing it when the reconciliation worker restarts.
    pub fn attach_observers(
        &self,
        handler: impl Fn(Uuid, ObserverRequest) -> anyhow::Result<ObserverReply> + Send + Sync + 'static,
    ) {
        let mut current = self.observers.0.lock().expect("observer handler register");
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
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("observer handler unavailable"))?
            .clone()
            .context("room server does not support remote observation")?;
        handler(home, request)
    }
}
