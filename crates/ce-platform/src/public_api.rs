use super::*;
use std::collections::BTreeMap;

impl Platform {
    pub fn public_api(self: &Arc<Self>) -> Router {
        if self.peers.is_some() {
            halcyon_platform::router_with_matches(self.app.clone(), self.clone())
        } else {
            halcyon_platform::router(self.app.clone())
        }
    }

    pub(super) async fn enqueue_with_estimates(
        &self,
        session: &str,
        mode: String,
        difficulty: Option<BotDifficulty>,
        estimates: BTreeMap<Uuid, u32>,
    ) -> halcyon_platform::Result<()> {
        let seats = match mode.as_str() {
            "5v5_bots_solo" => 1,
            "solo" if difficulty.is_some() => 1,
            LEGACY_ARAL_QUEUE => LEGACY_ARAL_SEATS,
            _ => self.app.seats_per_match(),
        };
        match &self.peers {
            Some(peers) => {
                peers
                    .join_party_with_site_report(
                        session,
                        mode,
                        u8::try_from(seats)
                            .map_err(|_| PlatformError::Invalid("invalid seat count"))?,
                        difficulty,
                        estimates,
                    )
                    .await
            }
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
            Some(peers) => peers.leave(session).await,
            None => self.app.leave(session).await,
        }
    }
    async fn accept_offer(&self, session: &str, id: Uuid) -> halcyon_platform::Result<()> {
        match &self.peers {
            Some(peers) => peers.accept(session, id).await,
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
            Some(peers) => peers.send_chat(session, id, text).await,
            None => self.app.send_chat(session, id, text).await,
        }
    }
}
