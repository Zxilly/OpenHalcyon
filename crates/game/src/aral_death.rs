//! Finish a witnessed ARAL death after the measured presentation delay.
use super::*;
use halcyon_sim::Kind;

pub(super) struct Pending {
    id: u32,
    minion: bool,
    due: u64,
    teams: u8,
    // An attached but unready socket must not inherit another socket's declaration.
    generations: [Option<u64>; aral::SEATS],
    observers: Vec<mpsc::WeakSender<Batch>>,
}

pub(super) fn schedule(room: &mut Room, events: &[Event]) {
    for event in events {
        let Event::Death { id, .. } = *event else {
            continue;
        };
        let minion = match room.world.entities.binary_search_by_key(&id, |e| e.id) {
            Ok(i) if room.world.entities[i].kind == Kind::Hero => false,
            // World::step removes only dead minions. ARAL's wave consists solely of 287..289.
            Err(_) => true,
            _ => continue,
        };
        room.dead_actors.push(Pending {
            id,
            minion,
            due: room.world.tick + halcyon_sim::TICK_HZ * if minion { 38 } else { 18 } / 10,
            generations: [None; aral::SEATS],
            teams: 0,
            observers: room.death_observers(),
        });
    }
}

pub(super) fn witness(pending: &mut [Pending], id: u32, team: u8, seats: &[Seat], world: &World) {
    if let Some(death) = pending.iter_mut().rev().find(|d| d.id == id) {
        death.teams |= 1 << team;
        for (index, (seat, actor)) in seats.iter().zip(&world.entities).enumerate() {
            if actor.team == team
                && let Some(peer) = seat.gameplay_peer()
            {
                death.generations[index] = Some(peer.generation);
            }
        }
    }
}

pub(super) fn flush(room: &mut Room, now: u64) {
    // The window contains only the last 3.8 seconds of actual deaths.
    let mut i = 0;
    while i < room.dead_actors.len() {
        if room.dead_actors[i].due > now {
            i += 1;
            continue;
        }
        let death = room.dead_actors.remove(i);
        if !death.minion
            && room
                .world
                .entities
                .iter()
                .any(|e| e.id == death.id && e.hp > 0.)
        {
            continue;
        }
        let mut body = [0; 6];
        body[..4].copy_from_slice(&u32::from(wire_id(death.id)).to_be_bytes());
        let mut out = vec![Message::known(crate::Opcode::ActorDead, body)];
        if death.minion {
            out.push(Message::known(crate::Opcode::DestroyEntity, body));
            for team in 0..2 {
                if death.teams & (1 << team) != 0 {
                    room.aral_slots.destroy_sent(death.id, team);
                }
            }
        }
        if !death.observers.is_empty() {
            room.publish_witnessed_death(&death.observers, &out);
        }
        let batch = transport::TeamBatch::new(out);
        for (seat, gen_at_death) in room.seats.iter().zip(death.generations) {
            if let Some(peer) = seat.gameplay_peer()
                && Some(peer.generation) == gen_at_death
            {
                peer.send(transport::Delivery::Team(batch.clone()));
            }
        }
    }
}

pub(super) fn publish_approaches(room: &mut Room, events: &[Event]) {
    for ev in events {
        let Event::RespawnApproach { id, team, home } = *ev else {
            continue;
        };
        let height = if team == 0 { 1.3 } else { 1.5 };
        let actor = wire_id(id);
        let out = vec![
            Message::actor_relocation(u32::from(actor), [home.x, height, home.y], 1),
            Message::position(actor, home.x, home.y),
        ];
        room.publish_observer_approach(id, &out);
        let batch = transport::TeamBatch::new(out);
        let enemy_visible = room.world.sees_position(1 - team, home);
        for (i, seat) in room.seats.iter().enumerate() {
            if (room.world.entities[i].team == team || enemy_visible)
                && seat.world_actor_written
                && seat.preamble.confirmed()
                && seat.peers[1].is_none()
                && room.scene_views[i].knows(id)
                && let Some(peer) = &seat.peers[0]
            {
                peer.send(transport::Delivery::Team(batch.clone()));
            }
        }
    }
}
