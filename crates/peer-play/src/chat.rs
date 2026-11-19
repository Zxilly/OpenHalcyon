use super::{Inner, PeerPlay};
use halcyon_federation::{Federation, MatchChat};
use halcyon_platform::{AppState, PlatformError, Result};
use std::time::Duration;
use tokio::sync::watch;
use uuid::Uuid;

pub(super) fn attach(federation: &Federation, platform: &AppState, pool: Uuid) {
    let platform = platform.clone();
    federation.attach_chat(move |chat, seats| {
        let app = platform.clone();
        async move {
            Ok(app
                .receive_pool_chat(pool, chat.room, chat.id, chat.sender, chat.text, seats)
                .await?)
        }
    });
}

impl PeerPlay {
    pub async fn send_chat(&self, token: &str, room: Uuid, message: String) -> Result<()> {
        let text = message.trim();
        if text.is_empty() || text.len() > 512 || text.chars().any(char::is_control) {
            return Err(PlatformError::Invalid("invalid message"));
        }
        let s = &self.inner;
        let profile = s.platform.profile(token).await?;
        if !profile.lobby.is_some_and(|l| {
            l.match_id == Some(room) && matches!(l.phase.as_str(), "offered" | "playing")
        }) {
            return Err(PlatformError::Conflict("no active seat in this match"));
        }
        let sender = profile.player_id;
        let homes = s
            .fed
            .chat_homes(s.cfg.node_id, room, sender)
            .ok_or(PlatformError::Conflict("no active seat in this match"))?;
        let chat = MatchChat {
            id: Uuid::new_v4(),
            room,
            sender,
            text: text.to_owned(),
        };
        let state = s.clone();
        tokio::task::spawn_blocking(move || {
            state.journal.queue_chat(&chat, homes.into_keys().collect())
        })
        .await
        .map_err(|_| PlatformError::WorkerUnavailable)?
        .map_err(|_| PlatformError::HostUnavailable)?;
        s.chat_wake.notify_one();
        Ok(())
    }
}

/// A stalled peer delays only its own delivery. Bound fan-out by the read batch.
pub(super) async fn reconcile(s: &std::sync::Arc<Inner>, mut stopped: watch::Receiver<bool>) {
    loop {
        if *stopped.borrow() {
            return;
        }
        let state = s.clone();
        let pending = tokio::task::spawn_blocking(move || state.journal.due_chat()).await;
        if let Ok(Ok(batch)) = pending
            && !batch.is_empty()
        {
            let mut homes = std::collections::BTreeMap::<_, Vec<_>>::new();
            for (home, chat) in batch {
                homes.entry(home).or_default().push(chat);
            }
            let mut workers = tokio::task::JoinSet::new();
            for (home, messages) in homes {
                let s = s.clone();
                workers.spawn(async move {
                    let mut reachable = true;
                    for chat in messages {
                        let delivered = reachable && s.fed.deliver_chat(home, &chat).await.is_ok();
                        reachable = delivered;
                        let state = s.clone();
                        if !matches!(
                            tokio::task::spawn_blocking(move || state
                                .journal
                                .chat_attempt(chat.id, home, delivered))
                            .await,
                            Ok(Ok(()))
                        ) {
                            break;
                        }
                    }
                });
            }
            tokio::select! { _=stopped.changed()=>return, _=workers.join_all()=>{} }
            continue;
        }
        tokio::select! {
            _ = stopped.changed() => return,
            _ = s.chat_wake.notified() => {},
            _ = tokio::time::sleep(Duration::from_secs(3)) => {},
        }
    }
}
