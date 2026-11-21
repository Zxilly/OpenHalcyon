//! Native match reconnect handshake assembly for Solo and game-channel rejoin.
use super::{Batch, Message, Pick, Room, messages, scene};
use halcyon_sim::Entity;

const DRAFT_TRANSITION_BODY: [u8; 1] = [0];

/// Returns `true` when a single-socket Solo control connection is rejoining an
/// already-locked match in `Loading` or `Running`.
///
/// Sending the direct loading rejoin batch in both phases avoids returning a
/// reconnecting player to the draft UI (where `0x046C` can be dropped if the
/// draft screen has not finished activating) while ensuring the client receives
/// the local seat's hero template and skin hash before entering the loading
/// screen.
pub(super) fn is_solo_rejoin(room: &Room, seat: usize, game: bool) -> bool {
    !game
        && room.solo_scene()
        && (room.seats[seat].control_world
            || (room.seats[seat].locked
                && matches!(
                    room.phase,
                    crate::RoomPhase::Loading | crate::RoomPhase::Running
                )))
}

/// Writes the authenticated seat's hero template, skin hash, and wire team into
/// a `0x03E9 MatchSettings` payload.
///
/// For non-spectator clients (`!is_spectator()`), `MatchSettings` (`0x8292e4`)
/// upserts the local actor into the client player table from `body[0..4]`,
/// `body[8..12]` (hero template), `body[12..16]` (skin hash), and `body[100]`
/// (team), and when `body[4..8] == 0` (`draft == false`) immediately enters the
/// loading screen synchronously. Leaving `body[12..16]` zero clobbers any prior
/// `0x03EE PlayerInfo` skin hash, causing `*KindredSkinManifest*` lookup to
/// return `NULL` and the loading card texture worker to `SIGSEGV` at `0x18a1838`.
pub(super) fn populate_world_settings(message: &mut Message, entity: &Entity, pick: &Pick) {
    if !message.is(crate::Opcode::MatchSettings) {
        return;
    }
    let Ok(mut settings) = halcyon_protocol::MatchSettingsBody::read_body(&message.body) else {
        return;
    };
    settings.hero_template = u32::from(entity.type_id);
    settings.skin_hash = pick.skin;
    settings.team = entity.team + 1;
    let _ = settings.write_body(&mut message.body);
}

/// Populates the `MatchSettings` record inside a game-channel `world_header`
/// batch with the reconnecting seat's hero template, skin hash, and wire team.
pub(super) fn populate_world_header(batch: &mut [Message], entity: &Entity, pick: &Pick) {
    for message in batch {
        if message.is(crate::Opcode::MatchSettings) {
            populate_world_settings(message, entity, pick);
            break;
        }
    }
}

/// Builds the opening reconnect batch for a fresh Solo control stream in
/// `Loading` or `Running`.
pub(super) fn solo_rejoin_batch(room: &Room, seat: usize) -> Batch {
    let mut records: Batch = scene::players(room).collect();
    let mut settings = messages::world_settings(seat, &room.queue);
    populate_world_settings(&mut settings, &room.world.entities[seat], &room.picks[seat]);
    records.push(settings);
    records.push(Message::known(
        crate::Opcode::DraftTransitionToLoading,
        DRAFT_TRANSITION_BODY,
    ));
    records
}

#[cfg(test)]
#[path = "reconnect_tests.rs"]
mod tests;
