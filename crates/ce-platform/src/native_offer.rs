use super::*;
use halcyon_platform::NativeOffer;

impl Platform {
    async fn native_offer(
        &self,
        key: &str,
        p: &PlayerProfile,
    ) -> Result<Option<NativeOffer>, PlatformError> {
        let Some(id) = p
            .lobby
            .as_ref()
            .filter(|l| l.phase != "spectating")
            .and_then(|l| l.match_id)
        else {
            return Ok(None);
        };
        let mut offer = self.app.native_offer(key).await?;
        if offer.is_none()
            && let Some(peers) = &self.peers
        {
            offer = peers.native_offer(key).await?;
        }
        Ok(offer.filter(|o| o.id == id && o.seats.iter().any(|s| s.player == p.player_id)))
    }

    pub(super) async fn projected_state(
        &self,
        key: &str,
        p: &PlayerProfile,
    ) -> Result<Value, PlatformError> {
        let mut rv = state(p);
        if rv["state"] == "pending_auto" {
            rv["numPlayers"] = json!(self.app.native_queue_party_size(key).await?);
        }
        if rv["state"] == "post_match"
            && let Some(settled) = p.lobby.as_ref().and_then(|l| l.settled_at_ms)
            && let Some(stamp) = chrono::DateTime::from_timestamp_millis(settled)
        {
            let expiry_ms = settled + 100_000;
            let remaining_ms = (expiry_ms - chrono::Utc::now().timestamp_millis()).max(0);
            rv["ttl"] = json!(((remaining_ms + 999) / 1000).min(100));
            rv["expiry"] = json!(expiry_ms as f64 / 1000.0);
            rv["timestamp"] = json!(stamp.format("%m/%d/%y %H:%M:%S").to_string());
            rv["site"] = json!("");
            rv["notify"] = json!(true);
        }
        if let Some(o) = self.native_offer(key, p).await? {
            let l = p.lobby.as_ref().expect("bound offer");
            rv["numPlayers"] = json!(o.seats.len());
            rv["numQueuedEntries"] = json!(o.entries);
            rv["prevState"] = json!("pending_auto");
            if l.phase == "finished" {
                rv["notify"] = json!(true);
            }
            if l.phase == "offered" {
                if l.mode == "casual_aral"
                    && o.seats
                        .iter()
                        .any(|s| s.player == p.player_id && !s.accepted)
                {
                    rv["state"] = json!("matched_partners");
                }
                if let Some(exp) = o.expires {
                    rv["ttl"] = json!((exp - now() as i64).max(0));
                    rv["expiry"] = json!(exp);
                }
            }
            if rv.get("timestamp").is_none() {
                rv["timestamp"] = json!(chrono::Utc::now().format("%m/%d/%y %H:%M:%S").to_string());
            }
        }
        Ok(rv)
    }

    pub(super) async fn pending_match(
        &self,
        key: &str,
        p: &PlayerProfile,
    ) -> Result<Value, PlatformError> {
        if p.lobby
            .as_ref()
            .is_some_and(|l| matches!(l.phase.as_str(), "offered" | "reserving"))
            && let Some(o) = self.native_offer(key, p).await?
        {
            let responses: Vec<_> = o
                .seats
                .iter()
                .map(|seat| {
                    json!({
                        "playerUUID": seat.player,
                        "response": u8::from(seat.accepted),
                        "acceptDelay": 0.0,
                    })
                })
                .collect();
            let remaining = o.expires.map(|exp| (exp - now() as i64).max(0));
            // Acknowledged seats have response=1. All 150 seat rows in the 25 captures have zero acceptDelay.
            return Ok(
                json!({"matchId":o.id,"isValid":remaining!=Some(0) && o.seats.iter().any(|s|s.accepted),"ttl":remaining.unwrap_or(1),"responses":responses}),
            );
        }
        Ok(json!({"isValid":false,"matchId":"","responses":[],"ttl":0}))
    }
}
