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
        let mut out = vec![Message::new(0x0431, body)];
        if death.minion {
            out.push(Message::new(0x040b, body));
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
