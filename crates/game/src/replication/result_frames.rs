//! End request measured in live ARAL and recorded 5v5 PvP crystal endings.
pub(crate) mod statistics;
use halcyon_protocol::{
    Message,
    ce::{KnownQueue, NativeMode},
};
use halcyon_sim::{Kind, World};

pub(crate) fn statistics_snapshot(world: &World) -> std::io::Result<Message> {
    statistics::snapshot(world)
}

/// Five archived ARAL crystal defeats announce postgame at the crystal's death.
pub(crate) fn aral_lost_crystal(legacy_aral: bool, world: &World) -> Option<u32> {
    let winner = world.winner?;
    if !legacy_aral || winner > 1 {
        return None;
    }
    world
        .entities
        .iter()
        .find(|entity| entity.kind == Kind::Crystal && entity.team == 1 - winner && entity.hp <= 0.)
        .map(|crystal| crystal.id)
}

pub(crate) fn append_postgame(
    out: &mut Vec<Message>,
    ev: &halcyon_sim::Event,
    crystal: Option<u32>,
) {
    if let halcyon_sim::Event::Death { id, .. } = ev
        && Some(*id) == crystal
    {
        out.push(halcyon_protocol::CrystalShatteredBody.encode());
    }
}

/// Builds the observed reason-zero request only for an authoritative crystal loss.
/// The local ARAL captures place the statistics report and this request about six seconds after death.
pub(crate) fn crystal_end(queue: &str, world: &World) -> Option<Message> {
    if !matches!(
        KnownQueue::parse(queue).map(KnownQueue::mode),
        Some(
            NativeMode::Aral
                | NativeMode::CasualFive
                | NativeMode::SoloBots
                | NativeMode::RankedFive
                | NativeMode::CasualFold
                | NativeMode::RankedFold
                | NativeMode::Blitz
        )
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
    Some(
        halcyon_protocol::EndMatchBody {
            team_word: u32::from(winner) + 1,
            reason: halcyon_protocol::EndMatchReason::CrystalShattered,
        }
        .encode_padded(),
    )
}

/// Builds a reason-3 (Score/Time Limit) end frame when a Blitz match ends without crystal shatter.
pub(crate) fn score_or_time_end(queue: &str, world: &World) -> Option<Message> {
    if KnownQueue::parse(queue).map(KnownQueue::mode) != Some(NativeMode::Blitz) {
        return None;
    }
    let winner = world.winner?;
    if winner > 1 {
        return None;
    }
    Some(
        halcyon_protocol::EndMatchBody {
            team_word: u32::from(winner) + 1,
            reason: halcyon_protocol::EndMatchReason::ScoreOrTimeLimit,
        }
        .encode_padded(),
    )
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
                .find_mut(|e| e.kind == Kind::Crystal && e.team == 1 - winner)
                .unwrap();
            crystal.hp = 0.;
            assert!(crystal_end("5v5_bots_solo", &w).is_some());
            assert!(crystal_end("5v5_practice", &w).is_none());
            assert!(crystal_end("5v5_pvp_casual", &w).is_some());
            assert!(crystal_end("5v5_pvp_ranked", &w).is_some());
            assert!(crystal_end("3v3_pvp_casual", &w).is_some());
            assert!(crystal_end("3v3_pvp_ranked", &w).is_some());
            assert!(crystal_end("solo_bots", &w).is_some());
            assert!(crystal_end("standard_bots_solo", &w).is_some());
            assert!(crystal_end("battle_royale_bots_solo", &w).is_some());
            assert!(crystal_end("blitz_3v3", &w).is_some());
            assert!(crystal_end("blitz_bots_solo", &w).is_some());
            assert!(crystal_end("blitz_pvp_revamp", &w).is_some());
            assert!(crystal_end("blitz_pvp_pure", &w).is_some());
            for blitz_queue in [
                "blitz_3v3",
                "blitz_bots_solo",
                "blitz_pvp_revamp",
                "blitz_pvp_pure",
            ] {
                let blitz_score = score_or_time_end(blitz_queue, &w).unwrap();
                assert!(matches!(
                    blitz_score.fields().unwrap(),
                    Fields::MatchRequest {
                        team_word,
                        reason: 3,
                        remaining: [0]
                    } if team_word == 1 + u32::from(winner)
                ));
            }
            let message = crystal_end("casual_aral", &w).unwrap();
            assert!(
                matches!(message.fields().unwrap(), Fields::MatchRequest {team_word,reason:0,remaining:[0]} if team_word == 1+u32::from(winner))
            );
        }
    }
}
