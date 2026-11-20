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
            .filter(|l| l.phase_kind() != Some(LobbyPhase::Spectating))
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
    ) -> Result<StateSnapshot, PlatformError> {
        let mut rv = StateSnapshot::from_profile(p);
        if rv.state == LobbyState::PendingAuto {
            rv.num_players = self.app.native_queue_party_size(key).await? as usize;
        }
        if rv.state == LobbyState::PostMatch
            && let Some(settled) = p.lobby.as_ref().and_then(|l| l.settled_at_ms)
            && let Some(stamp) = chrono::DateTime::from_timestamp_millis(settled)
        {
            let expiry_ms = settled + 100_000;
            let remaining_ms = (expiry_ms - chrono::Utc::now().timestamp_millis()).max(0);
            rv.ttl = Seconds::Whole(((remaining_ms + 999) / 1000).min(100));
            rv.expiry = Seconds::Fraction(expiry_ms as f64 / 1000.0);
            rv.timestamp = Some(stamp.format("%m/%d/%y %H:%M:%S").to_string());
            rv.site = Some("");
            rv.notify = true;
        }
        if let Some(o) = self.native_offer(key, p).await? {
            let l = p.lobby.as_ref().expect("bound offer");
            rv.num_players = o.seats.len();
            rv.num_queued_entries = Some(o.entries as usize);
            rv.prev_state = Some(LobbyState::PendingAuto);
            if l.phase_kind() == Some(LobbyPhase::Finished) {
                rv.notify = true;
            }
            if l.phase_kind() == Some(LobbyPhase::Offered) {
                if l.mode == "casual_aral"
                    && o.seats
                        .iter()
                        .any(|s| s.player == p.player_id && !s.accepted)
                {
                    rv.state = LobbyState::MatchedPartners;
                }
                if let Some(exp) = o.expires {
                    rv.ttl = Seconds::Whole((exp - now() as i64).max(0));
                    rv.expiry = Seconds::Whole(exp);
                }
            }
            rv.timestamp.get_or_insert_with(wire::timestamp);
        }
        Ok(rv)
    }

    pub(super) async fn pending_match(&self, key: &str, p: &PlayerProfile) -> Result<Value, Error> {
        let response = if p.lobby.as_ref().is_some_and(|l| {
            matches!(
                l.phase_kind(),
                Some(LobbyPhase::Offered | LobbyPhase::Reserving)
            )
        }) && let Some(o) = self.native_offer(key, p).await?
        {
            let responses = o
                .seats
                .iter()
                .map(|seat| OfferSeat {
                    player_uuid: seat.player,
                    response: u8::from(seat.accepted),
                    accept_delay: 0.0,
                })
                .collect();
            let remaining = o.expires.map(|exp| (exp - now() as i64).max(0));
            PendingMatch {
                match_id: o.id.to_string(),
                is_valid: remaining != Some(0) && o.seats.iter().any(|s| s.accepted),
                ttl: remaining.unwrap_or(1),
                responses,
            }
        } else {
            PendingMatch {
                match_id: String::new(),
                is_valid: false,
                ttl: 0,
                responses: Vec::new(),
            }
        };
        Ok(serde_json::to_value(response)?)
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PendingMatch {
    is_valid: bool,
    match_id: String,
    responses: Vec<OfferSeat>,
    ttl: i64,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct OfferSeat {
    #[serde(rename = "playerUUID")]
    player_uuid: Uuid,
    response: u8,
    accept_delay: f64,
}
