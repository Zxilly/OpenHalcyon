use halcyon_protocol::Message;
use halcyon_sim::{Command, Entity, Kind, Position, World};

use super::{Pick, messages};

/// Submit the observed padded point request for the authenticated seat and
/// acknowledge its ordered destination. Actual motion still comes from the sim.
pub(super) fn submit(
    world: &mut World,
    actor: u32,
    slot: u8,
    position: Position,
    body: &[u8],
) -> Option<Message> {
    if body.len() != 14 || body[8..] != [0; 6] {
        return None;
    }
    world.command(actor, Command::MoveTo(position)).ok()?;
    let mut reply = [0; 9];
    reply[0] = slot;
    reply[1..5].copy_from_slice(&position.x.to_be_bytes());
    reply[5..9].copy_from_slice(&position.y.to_be_bytes());
    Some(Message::new(0x03f8, reply))
}

pub(super) fn aral_slot(actor: &Entity) -> Option<u8> {
    (actor.kind == Kind::Hero && (1..=super::aral::SEATS as u32).contains(&actor.id))
        .then(|| 15 + actor.id as u8)
}

/// ARAL heroes use actor-order registration, independent of team assignment.
pub(super) fn full_spawn(
    actor: &Entity,
    picks: &[Pick],
    data: Option<&halcyon_data::GameData>,
    legacy_aral: bool,
) -> Message {
    let mut declaration = messages::spawn(actor, picks, data);
    if legacy_aral && let Some(slot) = aral_slot(actor) {
        declaration.body[745] = slot;
    }
    declaration
}
