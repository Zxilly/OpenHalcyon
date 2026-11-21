//! ARAL Ringo's Double Down basic-attack override.
use crate::Message;
use crate::modes::aral;
use crate::replication::{Batch, messages::wire_id, slots::tower_aggro::View};
use halcyon_sim::Event;

/// Last released attack per ARAL seat. Interrupted windups emit no attack event.
#[derive(Default)]
pub(crate) struct AttackHistory([Option<(u32, halcyon_sim::Position)>; aral::SEATS]);

impl AttackHistory {
    pub(crate) fn observe(
        &mut self,
        events: &[Event],
        world: &halcyon_sim::World,
    ) -> [Option<u32>; aral::SEATS] {
        let mut changed = [None; aral::SEATS];
        for event in events {
            let (source, target) = match *event {
                Event::Attack { source, target } => (source, Some(target)),
                Event::Cast { source, .. } | Event::Death { id: source, .. } => (source, None),
                _ => continue,
            };
            let Some(seat) = source.checked_sub(1).map(|v| v as usize) else {
                continue;
            };
            if let Some(previous) = self.0.get_mut(seat) {
                if world.entities[seat].type_id != halcyon_data::EntityTemplate::Ringo.as_raw() {
                    continue;
                }
                let attack = target.map(|id| (id, world.entities[seat].position));
                if previous.is_some() && *previous != attack {
                    changed[seat] = target;
                }
                *previous = attack;
            }
        }
        changed
    }
}

const LANE_HEIGHT: f32 = f32::from_bits(0x3d19_2e1f);

fn observed_ground(x: f32, z: f32) -> bool {
    (-69.44..=52.15).contains(&x) && (-2.13..=14.30).contains(&z)
}

/// Called on the already filtered stream immediately before an empowered release.
pub(crate) fn face_attack_target(
    batch: &mut Batch,
    release: &Message,
    world: &halcyon_sim::World,
    changed: &[Option<u32>; aral::SEATS],
    view: &View,
) {
    if !release.is(crate::Opcode::PlayTargetAbility) {
        return;
    }
    let Some((source, Some(target))) = references(release) else {
        return;
    };
    let seat = (source - 1) as usize;
    let actor = &world.entities[seat];
    if changed[seat] != Some(target)
        || view.ringo_effects[seat].is_none()
        || actor.type_id != halcyon_data::EntityTemplate::Ringo.as_raw()
        || !observed_ground(actor.position.x, actor.position.y)
    {
        return;
    }
    let caster = wire_id(source);
    let position = actor.position;
    batch.push(
        halcyon_protocol::Fields::MoveAndFace {
            actor: u32::from(caster),
            reference: u32::from(wire_id(target)),
            values: [position.x, LANE_HEIGHT, position.y],
            remaining: &[],
        }
        .movement_message()
        .expect("fixed move-and-face shape"),
    );
    batch.push(Message::position(caster, position.x, position.y));
}

pub(crate) fn recordable_face(m: &Message) -> bool {
    matches!(m.fields(), Ok(halcyon_protocol::Fields::MoveAndFace {
        actor: actor @ 1500..=1505, reference: target @ 1500..=65534,
        values: [x, height, z], remaining: [],
    }) if actor != target && height.to_bits() == LANE_HEIGHT.to_bits() && observed_ground(x,z))
}

pub(crate) fn append(out: &mut Batch, event: &Event, visible: impl Fn(u32) -> bool) -> bool {
    let Event::DoubleDown {
        source,
        instance,
        attack_target,
    } = *event
    else {
        return false;
    };
    if !visible(source) {
        return true;
    }
    let actor = u32::from(wire_id(source));
    let Some(instance) = instance.checked_add(1499) else {
        return true;
    };
    if let Some(target) = attack_target {
        if visible(target) && wire_id(target) != u16::MAX {
            out.push(Message::ability_target_used(
                actor,
                u32::from(wire_id(target)),
                3,
            ));
        }
        out.push(
            halcyon_protocol::BuffMutation {
                target: actor,
                instance,
                change: halcyon_protocol::BuffChange::Cancel,
                padding: &[0; 6],
            }
            .to_message()
            .expect("fixed Double Down cancel"),
        );
        for key in [3u32, 8, 9, 10] {
            out.push(halcyon_protocol::ClearAbilityOverrideBody { actor, key }.encode_padded());
        }
    } else {
        out.push(
            halcyon_protocol::BuffApplication {
                prefix: halcyon_protocol::BuffPrefix {
                    target: actor,
                    source: actor,
                    instance,
                    definition: 379,
                    duration_bits: 0xbc00,
                },
                extras: None,
                suffix: &[0; 6],
            }
            .to_message()
            .expect("fixed Double Down application"),
        );
        for key in [8u32, 9, 10] {
            out.push(
                halcyon_protocol::OverrideAbilityBody {
                    actor,
                    key,
                    replacement: 3,
                    flag: 0,
                }
                .encode_padded(),
            );
        }
    }
    true
}

pub(crate) fn references(m: &Message) -> Option<(u32, Option<u32>)> {
    let body = &m.body;
    let (wire, target) = match m.known_opcode() {
        Some(crate::Opcode::ApplyBuffCompact) => {
            let Ok(halcyon_protocol::Fields::BuffApplication(v)) = m.fields() else {
                return None;
            };
            if v.prefix.source != v.prefix.target
                || v.prefix.definition != 379
                || v.prefix.duration_bits != 0xbc00
                || v.prefix.instance <= 1505
                || v.suffix != [0; 6]
            {
                return None;
            }
            (v.prefix.target, None)
        }
        Some(crate::Opcode::CancelBuff) => {
            let bm = m.buff_mutation()?;
            if bm.change != halcyon_protocol::BuffChange::Cancel
                || bm.instance <= 1505
                || bm.padding != [0; 6]
            {
                return None;
            }
            (bm.target, None)
        }
        Some(crate::Opcode::PlayTargetAbility) => {
            let pt = halcyon_protocol::PlayTargetAbilityBody::read_exact(body).ok()?;
            if pt.selector != 3 || !(1500..65535).contains(&pt.target) {
                return None;
            }
            (pt.actor, Some(pt.target - 1499))
        }
        Some(crate::Opcode::OverrideAbility) => {
            let ov = halcyon_protocol::OverrideAbilityBody::read_padded_zero(body).ok()?;
            if !matches!(ov.key, 8..=10) || ov.replacement != 3 || ov.flag != 0 {
                return None;
            }
            (ov.actor, None)
        }
        Some(crate::Opcode::ClearAbilityOverride) => {
            let clr = halcyon_protocol::ClearAbilityOverrideBody::read_padded_zero(body).ok()?;
            if !matches!(clr.key, 3 | 8..=10) {
                return None;
            }
            (clr.actor, None)
        }
        _ => return None,
    };
    (1500..=1505).contains(&wire).then(|| (wire - 1499, target))
}

/// A cancellation references the application delivered to this scene generation.
pub(crate) fn track(view: &mut View, message: &Message) -> bool {
    if !matches!(
        message.opcode,
        crate::wire_opcode::ApplyBuffCompact | crate::wire_opcode::CancelBuff
    ) {
        return true;
    }
    let Some((actor, _)) = references(message) else {
        return true;
    };
    let active = &mut view.ringo_effects[(actor - 1) as usize];
    if message.is(crate::Opcode::ApplyBuffCompact) {
        let Ok(halcyon_protocol::Fields::BuffApplication(v)) = message.fields() else {
            return false;
        };
        *active = Some(v.prefix.instance);
        true
    } else {
        let Some(bm) = message.buff_mutation() else {
            return false;
        };
        if *active != Some(bm.instance) {
            return false;
        }
        *active = None;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn same_target_requires_displacement_and_history_does_not_survive_death() {
        let mut history = AttackHistory::default();
        let mut world = halcyon_sim::World::new(7, 6);
        world.entities[0].type_id = halcyon_data::EntityTemplate::Ringo.as_raw();
        let shot = [Event::Attack {
            source: 1,
            target: 2,
        }];
        assert_eq!(history.observe(&shot, &world)[0], None);
        assert_eq!(history.observe(&shot, &world)[0], None);
        world.entities[0].position.x += 1.;
        assert_eq!(history.observe(&shot, &world)[0], Some(2));
        history.observe(&[Event::Death { id: 1, killer: 3 }], &world);
        assert_eq!(history.observe(&shot, &world)[0], None);
    }

    #[tokio::test]
    async fn empowered_attack_turns_toward_a_new_opponent_on_the_registered_stream() {
        use crate::{Peer, Rooms, transport::Delivery};
        use halcyon_sim::{Command, Position};
        use halcyon_types::{MatchHost, Reservation};
        use tokio::sync::{mpsc, watch};
        use uuid::Uuid;

        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "localhost".into(), 1, None);
        let id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                match_id: id,
                players: (0..6).map(|_| Uuid::new_v4()).collect(),
                teams: vec![0, 1, 0, 1, 0, 1],
                talent_levels: vec![],
                difficulty: None,
                mode: halcyon_protocol::ce::KnownQueue::CasualAral.as_str().into(),
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut room = shared.lock().unwrap();
        let (tx, mut rx) = mpsc::channel(256);
        let seat = &mut room.seats[2];
        seat.peers[0] = Some(Peer {
            generation: 1,
            tx,
            stalled: watch::channel(false).0,
        });
        seat.world_actor_written = true;
        seat.preamble.observe(
            &Message::known(crate::Opcode::RequestSceneSnapshot, [0; 6]),
            1502,
        );
        seat.preamble.observe(
            &Message::known(crate::Opcode::ConfirmSceneOwner, [0, 0, 5, 222, 1, 0]),
            1502,
        );
        seat.preamble.confirm_world_input();
        for (seat, x) in [0., 1., -1., 2.].into_iter().enumerate() {
            room.world.entities[seat].position = Position { x, y: 2. };
        }
        room.world.entities[0].type_id = halcyon_data::EntityTemplate::Ringo.as_raw();
        room.world.entities[1].hp = 0.1;
        for target in [2, 4] {
            room.world.command(1, Command::Attack(target)).unwrap();
            let mut released = false;
            for _ in 0..160 {
                let events = room.world.step();
                room.replicate(&events);
                if events.iter().any(|ev| matches!(ev,Event::DoubleDown{source:1,attack_target,..} if (target==2 && attack_target.is_none()) || *attack_target==Some(target))) {
                    released = true; break;
                }
            }
            assert!(released, "missing perk transition against {target}");
        }
        let mut seen = Vec::new();
        while let Ok(batch) = rx.try_recv() {
            match batch {
                Delivery::Private(batch) => seen.extend(batch),
                Delivery::Team(batch) => seen.extend_from_slice(batch.messages()),
            }
        }
        let facing = seen
            .iter()
            .position(|m| m.is(crate::Opcode::MoveAndFace))
            .unwrap();
        assert_eq!(
            seen.iter()
                .filter(|m| m.is(crate::Opcode::MoveAndFace))
                .count(),
            1
        );
        assert!(recordable_face(&seen[facing]));
        assert_eq!(seen[facing].body[..8], [0, 0, 5, 220, 0, 0, 5, 223]);
        assert_eq!(seen[facing + 1], Message::position(1500, 0., 2.));
        assert_eq!(
            seen[facing + 2],
            Message::ability_target_used(1500, 1503, 3)
        );
        assert!(seen[facing + 3].is(crate::Opcode::CancelBuff));
    }

    #[test]
    fn only_registered_visibles_are_referenced_in_release_order() {
        let mut out = Batch::new();
        let grant = Event::DoubleDown {
            source: 1,
            instance: 990,
            attack_target: None,
        };
        append(&mut out, &grant, |_| false);
        assert!(out.is_empty());
        append(&mut out, &grant, |_| true);
        assert_eq!(
            out.iter()
                .skip(1)
                .map(|m| (m.opcode, m.body[7]))
                .collect::<Vec<_>>(),
            [
                (crate::wire_opcode::OverrideAbility, 8),
                (crate::wire_opcode::OverrideAbility, 9),
                (crate::wire_opcode::OverrideAbility, 10)
            ]
        );
        let Ok(halcyon_protocol::Fields::BuffApplication(effect)) = out[0].fields() else {
            panic!("expected grant")
        };
        let instance = effect.prefix.instance;
        let mut view = View::default();
        assert!(track(&mut view, &out[0]));
        let use_perk = Event::DoubleDown {
            source: 1,
            instance: 990,
            attack_target: Some(6),
        };
        out.clear();
        append(&mut out, &use_perk, |_| true);
        assert_eq!(
            out.iter().map(|m| m.opcode).collect::<Vec<_>>(),
            [
                crate::wire_opcode::PlayTargetAbility,
                crate::wire_opcode::CancelBuff,
                crate::wire_opcode::ClearAbilityOverride,
                crate::wire_opcode::ClearAbilityOverride,
                crate::wire_opcode::ClearAbilityOverride,
                crate::wire_opcode::ClearAbilityOverride
            ]
        );
        assert!(!track(&mut View::default(), &out[1]));
        assert!(track(&mut view, &out[1]));
        assert!(!track(&mut view, &out[1]));
        assert_eq!(out[1].buff_mutation().unwrap().instance, instance);
        assert_eq!(references(&out[0]), Some((1, Some(6))));
        out.clear();
        append(&mut out, &use_perk, |id| id == 1);
        assert_eq!(
            out.iter().skip(1).map(|m| m.body[7]).collect::<Vec<_>>(),
            [3, 8, 9, 10]
        );
        assert!(out.iter().all(|m| references(m) == Some((1, None))));
    }
}
