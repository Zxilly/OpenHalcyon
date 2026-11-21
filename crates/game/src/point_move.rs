use halcyon_protocol::{HeroSpawnBody, Message};
use halcyon_sim::{Command, Entity, Kind, Position, World};

use super::{Pick, Room, messages, transport};

pub(super) fn publish_aral(room: &mut Room, origin: usize, path: &Message) {
    let actor = &room.world.entities[origin];
    let visible: [bool; 2] = std::array::from_fn(|team| room.world.visible(team as u8, actor));
    let slot = halcyon_protocol::MoveToBody::read_body(&path.body).map_or(0, |body| body.selector);
    let mut shared = None;
    for (i, seat) in room.seats.iter().enumerate() {
        if i != origin
            && visible[usize::from(room.world.entities[i].team)]
            && seat.world_actor_written
            && seat.preamble.confirmed()
            && seat.peers[1].is_none()
            && room.scene_views[i].registered(actor.id, slot)
            && let Some(peer) = &seat.peers[0]
        {
            let batch = shared.get_or_insert_with(|| transport::TeamBatch::new(vec![path.clone()]));
            peer.send(transport::Delivery::Team(batch.clone()));
        }
    }
    room.publish_observers(std::slice::from_ref(path));
}

/// Submit the observed padded point request for the authenticated seat and
/// acknowledge its ordered destination. Actual motion still comes from the sim.
pub(super) fn submit(
    world: &mut World,
    actor: u32,
    slot: u8,
    position: Position,
    body: &[u8],
) -> Option<Message> {
    halcyon_protocol::MovePointRequestBody::read_padded_zero(body).ok()?;
    world.command(actor, Command::MoveTo(position)).ok()?;
    Some(destination(slot, position))
}

/// The native path action addresses the actor by the recipient's scene slot.
pub(super) fn destination(slot: u8, point: Position) -> Message {
    halcyon_protocol::MoveToBody {
        selector: slot,
        x: point.x,
        y: point.y,
    }
    .encode()
}

pub(super) fn aral_slot(actor: &Entity) -> Option<u8> {
    (actor.kind == Kind::Hero && (1..=super::aral::SEATS as u32).contains(&actor.id))
        .then(|| 15 + actor.id as u8)
}

/// ARAL heroes use actor-order registration, independent of team assignment.
pub(super) fn full_spawn(
    world: &World,
    actor: &Entity,
    picks: &[Pick],
    data: Option<&halcyon_data::GameData>,
    legacy_aral: bool,
) -> Message {
    let raw = messages::spawn(actor, picks, data);
    let mut body =
        HeroSpawnBody::read_body(&raw.body).expect("messages::spawn emits 750B HeroSpawn");
    if legacy_aral && let Some(slot) = aral_slot(actor) {
        body.slot = slot;
        let (height, heading) = if actor.team == 0 {
            (1.297_397_4_f32, 1.0_f32)
        } else {
            (1.777_397_4, -1.0)
        };
        body.position[1] = height;
        body.forward[0] = heading;
    }
    super::hero_progress::set_spawn(world, &mut body, actor);
    body.encode()
}
