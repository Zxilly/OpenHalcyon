//! End request measured in live ARAL and recorded 5v5 PvP crystal endings.
pub(super) mod statistics;
use halcyon_protocol::Message;
use halcyon_sim::{Kind, World};

pub(super) fn statistics_snapshot(world: &World) -> std::io::Result<Message> {
    statistics::snapshot(world)
}

/// Builds the observed reason-zero request only for an authoritative crystal loss.
/// The local ARAL captures place the statistics report and this request about six seconds after death.
pub(super) fn crystal_end(queue: &str, world: &World) -> Option<Message> {
    if !matches!(
        queue,
        "casual_aral"
            | "aral_casual"
            | "aral"
            | "5v5_pvp_casual"
            | "5v5_casual"
            | "casual_5v5"
            | "5v5_bots_solo"
            | "solo"
            | "5v5_pvp_ranked"
            | "5v5_ranked"
            | "ranked_5v5"
    ) {
        return None;
    }
    let winner = world.winner?;
    if winner > 1
        || !world
            .entities
            .iter()
            .any(|e| e.kind == Kind::Crystal && e.team == 1 - winner && e.hp <= 0.)
    {
        return None;
    }
    let mut body = [0; 6];
    body[..4].copy_from_slice(&(u32::from(winner) + 1).to_be_bytes());
    Some(Message::new(0x3f1, body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use halcyon_protocol::Fields;

    #[test]
    fn only_supported_worlds_with_crystal_loss_produce_a_winning_team_request() {
        for winner in [0, 1] {
            let mut w = World::new(10, 1);
            assert!(crystal_end("aral", &w).is_none());
            w.winner = Some(winner);
            assert!(crystal_end("casual_aral", &w).is_none());
            let crystal = w
                .entities
                .iter_mut()
                .find(|e| e.kind == Kind::Crystal && e.team == 1 - winner)
                .unwrap();
            crystal.hp = 0.;
            assert!(crystal_end("5v5_bots_solo", &w).is_some());
            assert!(crystal_end("5v5_practice", &w).is_none());
            assert!(crystal_end("5v5_pvp_casual", &w).is_some());
            assert!(crystal_end("5v5_pvp_ranked", &w).is_some());
            let message = crystal_end("casual_aral", &w).unwrap();
            assert!(
                matches!(message.fields().unwrap(), Fields::MatchRequest {team_word,reason:0,remaining:[0]} if team_word == 1+u32::from(winner))
            );
        }
    }
}
