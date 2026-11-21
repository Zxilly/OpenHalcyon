use crate::Platform;
use axum::Router;
use halcyon_data::BotDifficulty;
use halcyon_platform::{PlatformError, PublicMatches};
use std::{collections::BTreeMap, sync::Arc};
use uuid::Uuid;

impl Platform {
    pub fn public_api(self: &Arc<Self>) -> Router {
        halcyon_platform::router_with_matches(self.app.clone(), self.clone())
    }

    pub(crate) async fn enqueue_with_estimates(
        &self,
        session: &str,
        mode: String,
        difficulty: Option<BotDifficulty>,
        estimates: BTreeMap<Uuid, u32>,
    ) -> halcyon_platform::Result<()> {
        let seats = self.app.seats_for_mode(&mode);
        match &self.peers {
            Some(peers) => peers
                .join_party_with_site_report(
                    session,
                    mode,
                    u8::try_from(seats)
                        .map_err(|_| PlatformError::Invalid("invalid seat count"))?,
                    difficulty,
                    estimates,
                )
                .await
                .map_err(Into::into),
            None => {
                self.app
                    .join_party_with_difficulty(session, mode, seats, difficulty)
                    .await
            }
        }
    }
}

#[async_trait::async_trait]
impl PublicMatches for Platform {
    async fn player_action(
        &self,
        session: &str,
        id: Uuid,
        call: halcyon_platform::player_actions::ActionCall,
    ) -> halcyon_platform::Result<halcyon_platform::player_actions::ActionReply> {
        match &self.peers {
            Some(peers) => peers
                .player_action(session, id, call)
                .await
                .map_err(Into::into),
            None => self.app.player_action(session, id, call).await,
        }
    }

    async fn enqueue(
        &self,
        session: &str,
        mode: String,
        difficulty: Option<BotDifficulty>,
    ) -> halcyon_platform::Result<()> {
        self.enqueue_with_estimates(session, mode, difficulty, BTreeMap::new())
            .await
    }
    async fn cancel_queue(&self, session: &str) -> halcyon_platform::Result<()> {
        match &self.peers {
            Some(peers) => peers.leave(session).await.map_err(Into::into),
            None => self.app.leave(session).await,
        }
    }
    async fn accept_offer(&self, session: &str, id: Uuid) -> halcyon_platform::Result<()> {
        match &self.peers {
            Some(peers) => peers.accept(session, id).await.map_err(Into::into),
            None => self.app.accept(session, id).await,
        }
    }
    async fn match_chat(
        &self,
        session: &str,
        id: Uuid,
        text: String,
    ) -> halcyon_platform::Result<()> {
        match &self.peers {
            Some(peers) => peers.send_chat(session, id, text).await.map_err(Into::into),
            None => self.app.send_chat(session, id, text).await,
        }
    }
}
