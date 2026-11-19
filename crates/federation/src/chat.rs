use anyhow::{Context, ensure};
use halcyon_matchmaking::{Phase, TicketPhase};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    sync::{Arc, RwLock},
    time::Duration,
};
use uuid::Uuid;

use crate::Federation;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MatchChat {
    pub id: Uuid,
    pub room: Uuid,
    pub sender: Uuid,
    pub text: String,
}

type Delivery = Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>>;
type Receiver = dyn Fn(MatchChat, usize) -> Delivery + Send + Sync;
#[derive(Default, Clone)]
pub(crate) struct Chats(Arc<RwLock<Option<Arc<Receiver>>>>);

impl Federation {
    pub fn attach_chat<F, Fut>(&self, receiver: F)
    where
        F: Fn(MatchChat, usize) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        *self.chats.0.write().expect("chat receiver lock") =
            Some(Arc::new(move |chat, seats| Box::pin(receiver(chat, seats))));
    }

    /// Project a committed allocation locally. A lagging home can retry after replication.
    pub fn chat_homes(
        &self,
        origin: Uuid,
        room: Uuid,
        sender: Uuid,
    ) -> Option<BTreeMap<Uuid, usize>> {
        if !self.voter(origin) {
            return None;
        }
        self.store.inspect(|pool| {
            let allocation = pool.allocations().get(&room)?;
            if !matches!(allocation.phase, Phase::Preparing | Phase::Ready) {
                return None;
            }
            let mut homes = BTreeMap::new();
            let mut authorized = false;
            for seat in &allocation.tickets {
                let entry = pool.tickets().get(&seat.id)?;
                if entry.ticket.generation != seat.generation
                    || entry.phase != TicketPhase::Assigned(room)
                {
                    return None;
                }
                *homes.entry(entry.ticket.home).or_default() += entry.ticket.players.len();
                authorized |= entry.ticket.home == origin && entry.ticket.players.contains(&sender);
            }
            authorized.then_some(homes)
        })
    }

    pub async fn deliver_chat(&self, target: Uuid, chat: &MatchChat) -> anyhow::Result<()> {
        if target == self.net.cfg.node_id {
            self.chat_here(target, chat.clone()).await
        } else {
            self.net
                .post(target, "/internal/chat", chat, Duration::from_secs(3))
                .await
        }
    }

    pub(crate) async fn chat_here(&self, origin: Uuid, chat: MatchChat) -> anyhow::Result<()> {
        ensure!(
            !chat.text.is_empty()
                && chat.text.len() <= 512
                && !chat.text.chars().any(char::is_control),
            "invalid message"
        );
        let seats = self
            .chat_homes(origin, chat.room, chat.sender)
            .and_then(|homes| homes.get(&self.net.cfg.node_id).copied())
            .context("no active roster for this chat")?;
        let receive = self
            .chats
            .0
            .read()
            .expect("chat receiver lock")
            .clone()
            .context("chat receiver offline")?;
        receive(chat, seats).await
    }
}
