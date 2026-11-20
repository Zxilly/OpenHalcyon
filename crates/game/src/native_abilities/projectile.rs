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
    room.project_observer_scenes(|view| spire_message(view, source, id, point).map(|m| vec![m]));
}

pub(crate) fn recordable(m: &Message) -> bool {
    if m.is(Opcode::FireDirectedProjectile) {
        return matches!(ProjectileAction::read_body(Opcode::FireDirectedProjectile,&m.body),
            Some(ProjectileAction::FireDirected(f)) if f.projectile >= 2084 && f.profile_word == PROFILE
                && f.operand == 125 && (16..22).contains(&f.slots[0]) && f.slots[0] == f.slots[1]
                && f.values.iter().all(|v| v.is_finite()) && f.values[1].to_bits() == LAUNCH_HEIGHT.to_bits()
                && f.values[7].to_bits()==0 && f.padding.iter().all(|v| *v==0));
    }
    if m.is(Opcode::DetonateProjectile) {
        return matches!(ProjectileAction::read_body(Opcode::DetonateProjectile,&m.body),
            Some(ProjectileAction::Detonate{projectile, position,flag:0,padding}) if projectile>=2084
                && position.iter().all(|v|v.is_finite()) && position[1].to_bits()==CATCH_HEIGHT.to_bits()
                && padding.iter().all(|v|*v==0));
    }
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

#[derive(Clone, Copy)]
pub(super) enum Chakram {
    Launch {
        id: u32,
        origin: Position,
        aim: Position,
    },
    Catch {
        id: u32,
        position: Position,
    },
}

const LAUNCH_HEIGHT: f32 = f32::from_bits(0x3d192e1f);
const CATCH_HEIGHT: f32 = f32::from_bits(0x3f86630b);

impl Chakram {
    fn message(self, view: &mut View, source: u32) -> Option<Message> {
        let slot = view.slot(source)?;
        let active = view.chakrams.get_mut(usize::from(slot.checked_sub(16)?))?;
        match self {
            Self::Launch { id, origin, aim } => {
                let projectile = id.checked_add(1499)?;
                let dx = aim.x - origin.x;
                let dz = aim.y - origin.y;
                let planar = dx.hypot(dz);
                if !planar.is_finite() || planar == 0. || *active == Some(projectile) {
                    return None;
                }
                // The host's release point is on its zero-height plane.
                let length = planar.hypot(LAUNCH_HEIGHT);
                let message = ProjectileAction::FireDirected(ProjectileFire {
                    projectile,
                    profile_word: PROFILE,
                    operand: 125,
                    slots: [slot; 2],
                    padding: &[],
                    values: [
                        origin.x,
                        LAUNCH_HEIGHT,
                        origin.y,
                        dx / length,
                        dz.hypot(LAUNCH_HEIGHT).copysign(dz) / length,
                        dx / planar,
                        dz / planar,
                        0.,
                    ],
                })
                .message()?;
                *active = Some(projectile);
                Some(message)
            }
            Self::Catch { id, position } => {
                let projectile = id.checked_add(1499)?;
                if *active != Some(projectile) {
                    return None;
                }
                *active = None;
                ProjectileAction::Detonate {
                    projectile,
                    position: [position.x, CATCH_HEIGHT, position.y],
                    flag: 0,
                    padding: &[],
                }
                .message()
            }
        }
    }
}

pub(super) fn chakram(room: &mut Room, source: u32, shot: Chakram) {
    let Some(owner) = source
        .checked_sub(1)
        .and_then(|i| room.world.entities.get(i as usize))
        .filter(|a| a.id == source && a.type_id == EntityTemplate::Idris.as_raw())
    else {
        return;
    };
    let team = owner.team;
    for (i, seat) in room.seats.iter().enumerate() {
        if room.world.entities[i].team != team
            || !room.visible[team as usize].contains(&source)
            || !(seat.world_actor_written || seat.peers[1].is_some())
        {
            continue;
        }
        if let Some(peer) = seat.gameplay_peer()
            && let Some(m) = shot.message(&mut room.scene_views[i], source)
        {
            peer.send(Delivery::Private(vec![m]));
        }
    }
    room.project_observer_scenes(|view| shot.message(view, source).map(|m| vec![m]));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directed_flight_requires_a_declared_source_and_its_own_launch_before_catch() {
        let mut view = View::default();
        let origin = Position {
            x: -38.417,
            y: 0.785,
        };
        let aim = Position {
            x: -40.8939,
            y: 1.4944,
        };
        let fire = Chakram::Launch {
            id: 800,
            origin,
            aim,
        };
        let catch = Chakram::Catch {
            id: 800,
            position: origin,
        };
        assert!(fire.message(&mut view, 1).is_none());
        let mut declaration = Message::new(Opcode::EntitySpawn as u16, vec![0; 746]);
        declaration.body[8..12].copy_from_slice(&1500u32.to_be_bytes());
        declaration.body[745] = 17;
        view.declarations(&[declaration]);
        assert!(catch.message(&mut view, 1).is_none());
        let shot = fire.message(&mut view, 1).unwrap();
        assert!(recordable(&shot));
        assert!(fire.message(&mut view, 1).is_none());
        assert_eq!(&shot.body[0..8], &[0, 0, 8, 251, 0x3e, 0x32, 0x70, 0xa0]);
        assert_eq!(&shot.body[12..16], &0x3d192e1fu32.to_be_bytes());
        assert_eq!(&shot.body[36..], &[0, 0, 0, 0, 0, 125, 17, 17]);
        for (offset, expected) in [(20, -0.96129), (24, 0.27555), (28, -0.96139), (32, 0.27535)] {
            let val = f32::from_be_bytes(shot.body[offset..offset + 4].try_into().unwrap());
            assert!((val - expected).abs() < 0.0005);
        }
        assert!(
            Chakram::Catch {
                id: 799,
                position: origin
            }
            .message(&mut view, 1)
            .is_none()
        );
        let terminal = catch.message(&mut view, 1).unwrap();
        assert!(recordable(&terminal));
        assert_eq!(&terminal.body[0..4], &shot.body[0..4]);
        assert_eq!(&terminal.body[8..12], &0x3f86630bu32.to_be_bytes());
        assert_eq!(terminal.body[16], 0);
        assert!(catch.message(&mut view, 1).is_none());
    }
}
