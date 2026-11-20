use super::*;
use crate::tower_aggro::View;
use halcyon_protocol::{Opcode, ProjectileAction, ProjectileFire};

const PROFILE: u32 = 1_043_493_024;
const SPIRE: u16 = 86;

fn spire_message(view: &View, source: u32, id: u32, point: Position) -> Option<Message> {
    let slot = view.slot(source)?;
    if !(16..22).contains(&slot) {
        return None;
    }
    ProjectileAction::FirePoint(ProjectileFire {
        projectile: id.checked_add(1499)?,
        profile_word: PROFILE,
        values: [point.x, 0., point.y, 0.],
        operand: SPIRE,
        slots: [slot; 2],
        padding: &[],
    })
    .message()
}

pub(super) fn spire(room: &mut Room, source: u32, id: u32, point: Position) {
    let Some(actor) = source
        .checked_sub(1)
        .and_then(|i| room.world.entities.get(i as usize))
        .filter(|a| a.id == source && a.type_id == EntityTemplate::Reim.as_raw())
    else {
        return;
    };
    let team = actor.team;
    if room.visible[team as usize].contains(&source) {
        for (i, seat) in room.seats.iter().enumerate() {
            let viewer = &room.world.entities[i];
            if viewer.id == source
                || viewer.team != team
                || !(seat.world_actor_written || seat.peers[1].is_some())
            {
                continue;
            }
            if let Some(peer) = seat.gameplay_peer()
                && let Some(m) = spire_message(&room.scene_views[i], source, id, point)
            {
                peer.send(Delivery::Private(vec![m]));
            }
        }
    }
    room.publish_observer_spell_projectile(|view| spire_message(view, source, id, point));
}

pub(crate) fn recordable(m: &Message) -> bool {
    let Some(ProjectileAction::FirePoint(f)) =
        ProjectileAction::read_body(Opcode::FirePointProjectile, &m.body)
    else {
        return false;
    };
    m.is(Opcode::FirePointProjectile)
        && f.projectile >= 2084
        && f.profile_word == PROFILE
        && f.operand == SPIRE
        && f.values.iter().all(|v| v.is_finite())
        && f.values[3].to_bits() == 0
        && (16..22).contains(&f.slots[0])
        && f.slots[0] == f.slots[1]
        && f.padding.iter().all(|v| *v == 0)
}
