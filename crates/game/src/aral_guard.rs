use super::{Batch, Message, Opcode, tower_aggro::View};
use halcyon_protocol::{BuffApplication, BuffExtras, BuffPrefix, Fields};
use halcyon_sim::World;

// Local target slot, protecting tower slots, first-side instance. Side two adds 8 slots and 35 instances.
const TARGETS: [(u8, &[u8], u32); 5] = [
    (1, &[3, 4], 2021),
    (3, &[5], 2027),
    (4, &[5], 2033),
    (5, &[6], 2039),
    (6, &[7], 2045),
];

fn profile(slot: u8) -> Option<(&'static [u8], u32)> {
    let side = slot / 8;
    let (_, guards, instance) = TARGETS.iter().find(|x| x.0 == slot % 8 && side < 2)?;
    Some((guards, instance + 35 * u32::from(side)))
}
fn sim_id(slot: u8) -> u32 {
    501 + u32::from(slot)
}
fn wire_id(slot: u8) -> u32 {
    2000 + u32::from(slot)
}

fn application(slot: u8, count: u16) -> Option<Message> {
    let (guards, instance) = profile(slot)?;
    if count == 0 || usize::from(count) > guards.len() {
        return None;
    }
    Some(
        BuffApplication {
            prefix: BuffPrefix {
                target: wire_id(slot),
                source: wire_id(slot),
                instance,
                definition: 205,
                duration_bits: 0xbc00,
            },
            extras: Some(BuffExtras {
                stacks: count,
                words: [0; 4],
            }),
            suffix: &[],
        }
        .to_message()
        .expect("fixed ARAL structure application"),
    )
}

pub(super) fn opening(slot: u8) -> Option<Message> {
    application(slot, profile(slot)?.0.len() as u16)
}

/// Reconstruct counts only if the receiver may see every prerequisite used to compute them.
pub(super) fn restore(world: &World, team: Option<u8>, slot: u8) -> Option<Message> {
    let (guards, _) = profile(slot)?;
    let mut count = 0;
    for &guard in guards {
        let id = sim_id(guard + 8 * (slot / 8));
        let index = world.entities.binary_search_by_key(&id, |e| e.id).ok()?;
        let e = &world.entities[index];
        if team.is_some_and(|side| !world.visible(side, e)) {
            return None;
        }
        count += u16::from(e.hp > 0.);
    }
    application(slot, count)
}

fn decrement(slot: u8) -> Message {
    let (_, instance) = profile(slot).unwrap();
    let mut body = Vec::with_capacity(12);
    body.extend(wire_id(slot).to_be_bytes());
    body.extend(instance.to_be_bytes());
    body.extend(1u32.to_be_bytes());
    Message::known(Opcode::DecrementBuffStack, body)
}

pub(super) fn recordable(m: &Message) -> bool {
    let Some(wire) = m.body.get(..4) else {
        return false;
    };
    let Some(slot) = u32::from_be_bytes(wire.try_into().unwrap())
        .checked_sub(2000)
        .and_then(|n| u8::try_from(n).ok())
    else {
        return false;
    };
    let Some((guards, instance)) = profile(slot) else {
        return false;
    };
    if m.is(Opcode::DecrementBuffStack) {
        return matches!(m.body.get(12..), Some([] | [0, 0]))
            && m.body[4..8] == instance.to_be_bytes()
            && m.body[8..12] == 1_u32.to_be_bytes();
    }
    let Ok(Fields::BuffApplication(effect)) = m.fields() else {
        return false;
    };
    m.is(Opcode::ApplyBuffExtended)
        && effect.prefix.source == wire_id(slot)
        && effect.prefix.instance == instance
        && effect.prefix.definition == 205
        && effect.prefix.duration_bits == 0xbc00
        && matches!(effect.suffix, [] | [0, 0, 0, 0])
        && effect.extras.is_some_and(|e| {
            e.words == [0; 4] && (1..=guards.len()).contains(&usize::from(e.stacks))
        })
}

#[derive(Default)]
pub(super) struct State {
    granted: u16,
    fallen: u16,
}

impl View {
    pub(super) fn accept_guard_links(&mut self, batch: &[Message]) {
        for m in batch
            .iter()
            .filter(|m| m.is(Opcode::ApplyBuffExtended) && recordable(m))
        {
            let slot = (u32::from_be_bytes(m.body[..4].try_into().unwrap()) - 2000) as u8;
            if self.registered(sim_id(slot), slot) {
                self.guard_links.granted |= 1 << slot;
            }
        }
    }

    pub(super) fn guard_death(&mut self, slot: u8, out: &mut Batch) {
        if slot >= 16
            || !self.registered(sim_id(slot), slot)
            || self.guard_links.fallen & (1 << slot) != 0
        {
            return;
        }
        self.guard_links.fallen |= 1 << slot;
        let side = slot / 8;
        for (target, guards, _) in TARGETS {
            let target = target + side * 8;
            if guards.contains(&(slot % 8))
                && self.guard_links.granted & (1 << target) != 0
                && self.registered(sim_id(target), target)
                && self.guard_links.fallen & (1 << target) == 0
            {
                out.push(decrement(target));
            }
        }
    }

    pub(super) fn observer_guard_links(&mut self, world: &World, batch: &mut Batch) {
        let mut links = vec![];
        for slot in (0..16).rev() {
            if !self.registered(sim_id(slot), slot) {
                continue;
            }
            links.extend(restore(world, None, slot));
        }
        self.accept_guard_links(&links);
        batch.extend(links);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MatchHost, Peer, Reservation, Rooms, transport};
    use halcyon_sim::{Command, Event};
    use tokio::sync::{mpsc, watch};
    use uuid::Uuid;

    #[test]
    fn only_declared_instances_follow_same_side_tower_deaths_once() {
        let mut v = View::default();
        let mut out = Batch::new();
        for slot in 0..16 {
            let mut b = vec![0; 122];
            b[116] = slot;
            b[8..12].copy_from_slice(&wire_id(slot).to_be_bytes());
            v.declarations(&[Message::known(Opcode::EntitySpawn, b)]);
        }
        v.guard_death(7, &mut out);
        assert!(out.is_empty());
        let initial: Batch = (0..16).filter_map(opening).collect();
        assert_eq!(initial.len(), 10);
        assert!(initial.iter().all(recordable));
        v.accept_guard_links(&initial);
        for (slot, targets) in [
            (15, &[14][..]),
            (14, &[13]),
            (13, &[11, 12]),
            (11, &[9]),
            (12, &[9]),
            (5, &[3, 4]),
            (7, &[]),
            (1, &[]),
        ] {
            out.clear();
            v.guard_death(slot, &mut out);
            assert_eq!(
                out,
                targets.iter().copied().map(decrement).collect::<Vec<_>>()
            );
            assert!(out.iter().all(recordable));
            v.guard_death(slot, &mut out);
            assert_eq!(out.len(), targets.len());
        }
        let mut bad = decrement(9);
        bad.body[11] = 2;
        assert!(!recordable(&bad));
    }

    #[test]
    fn restored_crystal_count_is_scoped_to_the_receivers_authoritative_view() {
        let mut world = World::new(2, 6);
        world.use_six_human_roster().unwrap();
        world.use_aral_lane().unwrap();
        for (slot, count) in [(3, 1), (4, 0)] {
            let id = sim_id(slot);
            let i = world.entities.binary_search_by_key(&id, |e| e.id).unwrap();
            world.entities[i].hp = 0.;
            let expected = application(1, count);
            assert_eq!(restore(&world, Some(0), 1), expected);
            assert_eq!(restore(&world, None, 1), expected);
            assert!(restore(&world, Some(1), 1).is_none());
        }
    }

    #[test]
    fn dynamic_actor_ids_start_after_all_published_static_attachments() {
        let mut world = World::new(2, 6);
        world.use_six_human_roster().unwrap();
        world.use_aral_lane().unwrap();
        let instances: std::collections::HashSet<_> = (0..16)
            .flat_map(|slot| {
                crate::structure_effects::Opening::aral(slot)
                    .into_iter()
                    .flat_map(crate::structure_effects::Opening::messages)
                    .chain(opening(slot))
            })
            .map(|m| {
                let Ok(Fields::BuffApplication(a)) = m.fields() else {
                    panic!("expected attachment")
                };
                a.prefix.instance
            })
            .collect();
        assert_eq!(instances.len(), 40);
        assert_eq!(instances.iter().max(), Some(&2083));
        for _ in 0..300 {
            world.step();
        }
        let first = world
            .entities
            .iter()
            .filter(|e| e.kind == halcyon_sim::Kind::Minion)
            .map(|e| u32::from(crate::messages::wire_id(e.id)))
            .min()
            .unwrap();
        assert!(first > 2083);
    }

    #[tokio::test]
    async fn a_registered_native_seat_receives_decrement_after_authoritative_tower_kill() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                match_id: id,
                players: (0..6).map(|_| Uuid::new_v4()).collect(),
                teams: vec![],
                mode: "casual_aral".into(),
                talent_levels: vec![],
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut room = shared.lock().unwrap();
        room.world.use_aral_lane().unwrap();
        let opening = crate::aral::opening(&mut room, None);
        let view = &mut room.scene_views[0];
        view.declarations(&opening);
        view.accept_guard_links(&opening);
        let seat = &mut room.seats[0];
        let (tx, mut rx) = mpsc::channel(16);
        seat.peers[0] = Some(Peer {
            generation: 1,
            tx,
            stalled: watch::channel(false).0,
        });
        seat.world_actor_written = true;
        seat.control_world = true;
        seat.preamble
            .observe(&Message::known(Opcode::RequestSceneSnapshot, [0; 6]), 1500);
        let mut owner = [0; 6];
        owner[..4].copy_from_slice(&1500u32.to_be_bytes());
        owner[4] = 1;
        seat.preamble
            .observe(&Message::known(Opcode::ConfirmSceneOwner, owner), 1500);
        seat.preamble
            .observe(&Message::known(Opcode::SceneReadyConfirm, [0; 6]), 1500);

        let tower = sim_id(15);
        let idx = room
            .world
            .entities
            .binary_search_by_key(&tower, |e| e.id)
            .unwrap();
        room.world.entities[0].position = room.world.entities[idx].position;
        room.world.entities[idx].hp = 0.1;
        room.world.command(1, Command::Attack(tower)).unwrap();
        let events = (0..60)
            .find_map(|_| {
                let events = room.world.step();
                events
                    .iter()
                    .any(|e| matches!(e,Event::Death{id,..} if *id==tower))
                    .then_some(events)
            })
            .expect("native outer tower killed by the attacking hero");
        room.visible[0].extend([1, tower]);
        room.replicate(&events);
        let mut all = Vec::new();
        while let Ok(delivery) = rx.try_recv() {
            all.extend(transport::Delivery::into_records(delivery));
        }
        let kill = all.iter().position(|m| m.is(Opcode::ActorDeath)).unwrap();
        let change = all.iter().position(|m| m == &decrement(14)).unwrap();
        assert!(kill < change);
        assert_eq!(
            all.iter()
                .filter(|m| m.is(Opcode::DecrementBuffStack))
                .count(),
            1
        );
    }
}
