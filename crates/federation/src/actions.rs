use crate::Federation;
use halcyon_matchmaking::{Phase, TicketPhase};
use halcyon_types::player_actions::{ActionCall, ActionDecision, ActionReply};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use uuid::Uuid;

#[derive(Clone, Copy, Serialize, Deserialize)]
pub(crate) struct Request {
    room: Uuid,
    player: Uuid,
    call: ActionCall,
}
type Handler = dyn Fn(Uuid, Uuid, Uuid, ActionCall) -> ActionReply + Send + Sync;
#[derive(Default, Clone)]
pub(crate) struct Actions(Arc<RwLock<Option<Arc<Handler>>>>);
impl Federation {
    pub fn attach_actions(
        &self,
        handler: impl Fn(Uuid, Uuid, Uuid, ActionCall) -> ActionReply + Send + Sync + 'static,
    ) {
        *self.actions.0.write().expect("action handler lock") = Some(Arc::new(handler));
    }
    fn action_host(&self, home: Uuid, room: Uuid, player: Uuid) -> Option<Uuid> {
        if !self.voter(home) {
            return None;
        }
        self.store.inspect(|pool| {
            let alloc = pool.allocations().get(&room)?;
            if alloc.phase != Phase::Ready {
                return None;
            }
            alloc
                .tickets
                .iter()
                .any(|seat| {
                    pool.tickets().get(&seat.id).is_some_and(|entry| {
                        entry.ticket.generation == seat.generation
                            && entry.phase == TicketPhase::Assigned(room)
                            && entry.ticket.home == home
                            && entry.ticket.players.contains(&player)
                    })
                })
                .then_some(alloc.host)
        })
    }
    pub async fn player_action(
        &self,
        room: Uuid,
        player: Uuid,
        call: ActionCall,
    ) -> anyhow::Result<ActionReply> {
        let local = self.net.cfg.node_id;
        let request = Request { room, player, call };
        let Some(host) = self.action_host(local, room, player) else {
            return Ok(ActionReply::Decision(ActionDecision::Unavailable));
        };
        if host == local {
            self.action_here(local, request)
        } else {
            self.net
                .post(
                    host,
                    "/internal/player-action",
                    &request,
                    Duration::from_secs(3),
                )
                .await
        }
    }
    pub(crate) fn action_here(&self, home: Uuid, r: Request) -> anyhow::Result<ActionReply> {
        if self.action_host(home, r.room, r.player) != Some(self.net.cfg.node_id) {
            return Ok(ActionReply::Decision(ActionDecision::Unavailable));
        }
        let handler = self
            .actions
            .0
            .read()
            .expect("action handler lock")
            .clone()
            .ok_or_else(|| anyhow::anyhow!("action host offline"))?;
        Ok(handler(home, r.room, r.player, r.call))
    }
}
