//! Finish a witnessed death across ARAL and 5v5/3v3/Blitz after the presentation delay.
use super::*;
use halcyon_sim::Kind;

const MAX_SEATS: usize = 10;

pub(super) struct Pending {
    id: u32,
    minion: bool,
    due: u64,
    teams: u8,
    // An attached but unready socket must not inherit another socket's declaration.
    generations: [Option<u64>; MAX_SEATS],
    observers: Vec<mpsc::WeakSender<Batch>>,
}

pub(super) fn schedule(room: &mut Room, events: &[Event]) {
    let legacy = room.legacy_aral();
    let solo_scene = !legacy && room.solo_scene() && room.seats[0].control_world;
    for event in events {
        let Event::Death { id, .. } = *event else {
            continue;
        };
        let minion = match room.world.entities.binary_search_by_key(&id, |e| e.id) {
            Ok(i) if room.world.entities[i].kind == Kind::Hero => false,
            // World::step removes dead Minion and Neutral actors; keep structures in place.
            Err(_) => true,
            _ => continue,
        };
        let observers = room.death_observers();
        let mut generations = [None; MAX_SEATS];
        let mut teams = 0u8;
        if !legacy {
            for (index, (seat, actor)) in room
                .seats
                .iter()
                .zip(&room.world.entities)
                .enumerate()
                .take(MAX_SEATS)
            {
                let known = if solo_scene {
                    room.scene_views
                        .get(index)
                        .is_some_and(|view| view.knows(id))
                } else {
                    room.visible[usize::from(actor.team)].contains(&id)
                };
                if known {
                    teams |= 1 << actor.team;
                    if let Some(peer) = seat.gameplay_peer() {
                        generations[index] = Some(peer.generation);
                    }
                }
            }
            if solo_scene && minion && room.scene_views.first().is_some_and(|view| view.knows(id)) {
                room.solo_slots.defer_destroy(id);
            }
            if generations.iter().all(Option::is_none) && observers.is_empty() {
                continue;
            }
        }
        room.dead_actors.push(Pending {
            id,
            minion,
            due: room.world.tick + halcyon_sim::TICK_HZ * if minion { 38 } else { 18 } / 10,
            generations,
            teams,
            observers,
        });
    }
}

pub(super) fn witness(pending: &mut [Pending], id: u32, team: u8, seats: &[Seat], world: &World) {
    if let Some(death) = pending.iter_mut().rev().find(|d| d.id == id) {
        death.teams |= 1 << team;
        for (index, (seat, actor)) in seats
            .iter()
            .zip(&world.entities)
            .enumerate()
            .take(MAX_SEATS)
        {
            if actor.team == team
                && let Some(peer) = seat.gameplay_peer()
            {
                death.generations[index] = Some(peer.generation);
            }
        }
    }
}

pub(super) fn flush(room: &mut Room, now: u64) {
    let legacy = room.legacy_aral();
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
        let actor = u32::from(wire_id(death.id));
        let mut out = vec![halcyon_protocol::ActorDeadBody { actor }.encode_padded()];
        if death.minion {
            out.push(halcyon_protocol::DestroyEntityBody { actor }.encode_padded());
            if legacy {
                for team in 0..2 {
                    if death.teams & (1 << team) != 0 {
                        room.aral_slots.destroy_sent(death.id, team);
                    }
                }
            } else {
                room.solo_slots.destroy_sent(death.id);
            }
        }
        if !death.observers.is_empty() {
            room.publish_witnessed_death(&death.observers, &out);
        }
        let batch = transport::TeamBatch::new(out);
        for (index, (seat, gen_at_death)) in room.seats.iter().zip(death.generations).enumerate() {
            if let Some(peer) = seat.gameplay_peer()
                && Some(peer.generation) == gen_at_death
            {
                peer.send(transport::Delivery::Team(batch.clone()));
                if !legacy
                    && death.minion
                    && let Some(view) = room.scene_views.get_mut(index)
                {
                    view.release_effect(death.id);
                }
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
        room.publish_observer_actor(id, &out);
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
