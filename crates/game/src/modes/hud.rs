//! Connection-scoped in-match pings and text presets for authenticated seats.
use crate::Room;
use crate::replication::{Batch, messages::wire_id};
use crate::transport;
use halcyon_protocol::{HudQuickMessage, LiveFields, MapPing};
use std::time::{Duration, Instant};

const MAX_MAP_COORDINATE: f32 = 512.;
const MIN_INTERVAL: Duration = Duration::from_millis(750);

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum Quick {
    Missing = 3,
    NotReady,
    Together,
    Careful,
    Teamfight,
    Vision,
    NeedHelp,
    OnMyWay,
    DontGiveUp,
    GoodLuck,
    GoodGame,
}

impl Quick {
    const SUPPORTED: [Self; 11] = [
        Self::Missing,
        Self::NotReady,
        Self::Together,
        Self::Careful,
        Self::Teamfight,
        Self::Vision,
        Self::NeedHelp,
        Self::OnMyWay,
        Self::DontGiveUp,
        Self::GoodLuck,
        Self::GoodGame,
    ];
    fn parse(value: u8) -> Option<Self> {
        Self::SUPPORTED
            .get(usize::from(value.checked_sub(Self::Missing as u8)?))
            .copied()
    }
}

impl Room {
    pub(crate) fn room_hud_connection(&self, index: usize, ch: usize) -> bool {
        let seat = &self.seats[index];
        ch == 0
            && (self.legacy_aral() || self.solo_scene())
            && seat.peers[0].is_some()
            && seat.peers[1].is_none()
            && seat.world_actor_written
            && self.scene_views[index].knows(self.world.entities[index].id)
    }
}

pub(crate) fn submit(room: &mut Room, seat: usize, message: LiveFields<'_>) -> Batch {
    let now = Instant::now();
    if room.seats[seat]
        .last_hud
        .is_some_and(|at| now.duration_since(at) < MIN_INTERVAL)
    {
        return Vec::new();
    }
    let sender = &room.world.entities[seat];
    let actor = u32::from(wire_id(sender.id));
    let team = sender.team;
    let (message, target, everyone) = match message {
        LiveFields::MapPing(ping) if ping.actor == actor => {
            let Some((ping, target)) = check_ping(room, seat, team, ping) else {
                return Vec::new();
            };
            (ping.message(), target, false)
        }
        LiveFields::HudQuickMessage(quick) if quick.actor == actor => {
            let Some((quick, everyone)) = check_quick(quick) else {
                return Vec::new();
            };
            (quick.message(), None, everyone)
        }
        _ => return Vec::new(),
    };
    room.seats[seat].last_hud = Some(now);
    let mut shared = None;
    for (index, other) in room.seats.iter().enumerate() {
        let view = &room.scene_views[index];
        if index != seat
            && (room.world.entities[index].team == team || everyone)
            && view.knows(sender.id)
            && target.is_none_or(|id| view.knows(id))
            && let Some(peer) = other.gameplay_peer()
        {
            peer.send(transport::Delivery::Team(
                shared
                    .get_or_insert_with(|| transport::TeamBatch::new(vec![message.clone()]))
                    .clone(),
            ));
        }
    }
    vec![message]
}

fn check_ping(
    room: &Room,
    seat: usize,
    team: u8,
    mut ping: MapPing,
) -> Option<(MapPing, Option<u32>)> {
    // Observed tactical selectors. Cosmetic pings require an equipped pack.
    if !(2..=5).contains(&ping.selector)
        || !(1..=3).contains(&ping.qualifier)
        || !ping.position.iter().all(|v| v.abs() <= MAX_MAP_COORDINATE)
    {
        return None;
    }
    let Some(target) = ping.target else {
        return Some((ping, None));
    };
    let actor = room
        .world
        .entities
        .iter()
        .find(|e| u32::from(wire_id(e.id)) == target)?;
    if actor.hp <= 0. || !room.scene_views[seat].knows(actor.id) || !room.world.visible(team, actor)
    {
        return None;
    }
    // A forged position never controls the location of a targeted ping.
    ping.position[0] = actor.position.x;
    ping.position[2] = actor.position.y;
    Some((ping, Some(actor.id)))
}

fn check_quick(mut quick: HudQuickMessage) -> Option<(HudQuickMessage, bool)> {
    let selector = Quick::parse(quick.selector)?;
    if quick.target.is_some()
        || quick.value != 0.
        || quick.operand != 0
        || quick.padding.is_some_and(|pad| pad != [0; 5])
    {
        return None;
    }
    quick.padding = None;
    // The GG table's visibility bit conflicts with the text label. Keep it team-only.
    Some((quick, selector == Quick::GoodLuck))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::manager::Rooms;
    use crate::replication::messages;
    use crate::{Message, Peer};
    use halcyon_types::{MatchHost, Reservation};
    use tokio::sync::{mpsc, watch};
    use uuid::Uuid;

    #[tokio::test]
    async fn hud_enforces_actor_binding_visibility_audience_and_pacing() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: id,
                players: (0..6).map(|_| Uuid::new_v4()).collect(),
                mode: "casual_aral".into(),
                teams: vec![0, 0, 1, 0, 1, 1],
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut room = shared.lock().unwrap();
        let mut streams = Vec::new();
        let mut sender_spawn = messages::spawn(&room.world.entities[0], &room.picks, None);
        sender_spawn.body[745] = 16;
        for i in 0..3 {
            let (tx, rx) = mpsc::channel(8);
            room.seats[i].peers[0] = Some(Peer {
                tx,
                generation: 1,
                stalled: watch::channel(false).0,
            });
            let mut own_spawn = messages::spawn(&room.world.entities[i], &room.picks, None);
            own_spawn.body[745] = 16 + i as u8;
            room.scene_views[i].declarations(std::slice::from_ref(&sender_spawn));
            room.scene_views[i].declarations(std::slice::from_ref(&own_spawn));
            room.seats[i].world_actor_written = true;
            let mut owner = u32::from(wire_id(room.world.entities[i].id))
                .to_be_bytes()
                .to_vec();
            owner.extend([1, 0]);
            let preamble = &mut room.seats[i].preamble;
            for message in [
                Message::known(crate::Opcode::RequestSceneSnapshot, [0; 6]),
                Message::known(crate::Opcode::ConfirmSceneOwner, owner),
                Message::known(crate::Opcode::SceneReadyConfirm, [0; 6]),
            ] {
                preamble.observe(&message, 1500 + i as u32);
            }
            streams.push(rx);
        }
        room.phase(crate::RoomPhase::Running);
        let mut ping = MapPing {
            actor: 1500,
            target: None,
            position: [3., 0., 2.],
            selector: 2,
            qualifier: 1,
        };
        assert!(room.receive(1, 0, ping.message()).unwrap().is_empty());
        ping.target = Some(1502);
        assert!(room.receive(0, 0, ping.message()).unwrap().is_empty());
        ping.target = None;
        ping.position[0] = f32::INFINITY;
        assert!(room.receive(0, 0, ping.message()).unwrap().is_empty());
        ping.position[0] = 3.;
        assert_eq!(
            room.receive(0, 0, ping.message()).unwrap(),
            [ping.message()]
        );
        assert_eq!(
            streams[1].try_recv().unwrap().into_records(),
            [ping.message()]
        );
        assert!(room.receive(0, 0, ping.message()).unwrap().is_empty());
        assert!(streams.iter_mut().all(|stream| stream.try_recv().is_err()));
        room.seats[0].last_hud = None;
        let mut teammate = messages::spawn(&room.world.entities[1], &room.picks, None);
        teammate.body[745] = 17;
        room.scene_views[0].declarations(&[teammate]);
        ping.target = Some(1501);
        let authoritative = room.world.entities[1].position;
        let mut canonical = ping;
        canonical.position[0] = authoritative.x;
        canonical.position[2] = authoritative.y;
        assert_eq!(
            room.receive(0, 0, ping.message()).unwrap(),
            [canonical.message()]
        );
        assert_eq!(
            streams[1].try_recv().unwrap().into_records(),
            [canonical.message()]
        );
        assert!(streams[2].try_recv().is_err());
        for (selector, cross_team) in [(Quick::GoodGame, false), (Quick::GoodLuck, true)] {
            room.seats[0].last_hud = None;
            let quick = HudQuickMessage {
                actor: 1500,
                selector: selector as u8,
                target: None,
                value: 0.,
                operand: 0,
                padding: None,
            };
            assert_eq!(
                room.receive(0, 0, quick.message()).unwrap(),
                [quick.message()]
            );
            assert_eq!(
                streams[1].try_recv().unwrap().into_records(),
                [quick.message()]
            );
            if cross_team {
                assert_eq!(
                    streams[2].try_recv().unwrap().into_records(),
                    [quick.message()]
                );
            }
            assert!(streams.iter_mut().all(|stream| stream.try_recv().is_err()));
        }
    }
}
