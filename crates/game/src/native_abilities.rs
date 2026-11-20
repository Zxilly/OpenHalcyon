use halcyon_data::EntityTemplate;
use halcyon_sim::{
    AbilitySlot, CastTarget, Command, Event, Kind, Position, native_learn_operand,
    native_learn_request,
};

use super::{Batch, Message, Room, messages::wire_id, transport::Delivery};

mod chakram_motion;
mod chill_winds;
mod projectile;
pub(super) use projectile::recordable as recordable_spell_projectile;
mod reboot;
mod skedaddle;
pub(super) fn recordable_skedaddle(m: &Message) -> bool {
    skedaddle::recordable(m)
}
pub(super) fn recordable_chill_winds(m: &Message) -> bool {
    chill_winds::recordable_pfx(m)
}
pub(super) use chakram_motion::recordable_stop;

const IDRIS: u16 = EntityTemplate::Idris.as_raw();
const LORELAI: u16 = EntityTemplate::Lorelai.as_raw();

#[derive(Default)]
pub(super) struct NativeAbilities {
    learned_at: Option<u64>,
    walked_at: Option<u64>,
    cast: Option<PendingCast>,
}

struct PendingCast {
    slot: AbilitySlot,
    target: CastTarget,
    reply: Message,
    generation: u64,
    interrupted_walk: Option<Position>,
}

pub(super) fn receive(room: &mut Room, seat: usize, channel: usize, m: Message) -> Batch {
    let actor = &room.world.entities[seat];
    let connection = &room.seats[seat];
    let Some(peer) = connection.peers[channel].as_ref() else {
        return Vec::new();
    };
    if channel != 0
        || !(room.solo_scene() || room.legacy_aral())
        || !connection.world_actor_written
        || actor.kind != Kind::Hero
        || actor.bot
    {
        return Vec::new();
    }
    let generation = peer.generation;
    let id = actor.id;
    let hero = actor.type_id;
    let position = actor.position;
    let aral = room.legacy_aral();
    let state = &mut room.seats[seat].native_abilities;
    match m.known_opcode() {
        Some(crate::Opcode::RequestCancelAbility)
            if aral && m.body.len() == 6 && m.body[1..] == [0; 5] =>
        {
            let Some(pending) = &state.cast else {
                return Vec::new();
            };
            let selector = m.body[0];
            if pending.generation != generation
                || NativeCast::for_slot(hero, pending.slot).is_none_or(|c| c.selector() != selector)
                || !room.world.cancel_cast(id, pending.slot, pending.target)
            {
                return Vec::new();
            }
            state.cast = None;
            let mut body = [0; 10];
            body[..4].copy_from_slice(&u32::from(wire_id(id)).to_be_bytes());
            body[4..8].copy_from_slice(&u32::from(selector).to_be_bytes());
            vec![Message::known(crate::Opcode::CancelAbility, body)]
        }
        Some(crate::Opcode::RequestUpgradeAbility)
            if m.body.len() == 6 && m.body[1..] == [0; 5] =>
        {
            let Some((slot, operand)) = native_learn_request(hero, m.body[0]) else {
                return Vec::new();
            };
            if state.learned_at == Some(room.world.tick)
                || room.world.command(id, Command::Learn(slot)).is_err()
            {
                return Vec::new();
            }
            state.learned_at = Some(room.world.tick);
            let grant = Message::ability_learned(u32::from(wire_id(id)), u32::from(operand));
            witness(room, id, None, &grant, None);
            vec![m, grant]
        }
        Some(crate::Opcode::RequestPointAbility) if m.body.len() == 14 && m.body[13] == 0 => {
            let selector = m.body[12];
            let Some(cast) = NativeCast::point_input(hero, selector) else {
                return Vec::new();
            };
            let slot = cast.slot();
            let coordinates: [f32; 3] = std::array::from_fn(|i| {
                f32::from_be_bytes(m.body[i * 4..i * 4 + 4].try_into().unwrap())
            });
            let [x, height, y] = coordinates;
            if height != 0.0 || !x.is_finite() || !y.is_finite() {
                return Vec::new();
            }
            let target = CastTarget::Point(Position { x, y });
            if room
                .world
                .command(id, Command::Cast { slot, target })
                .is_ok()
            {
                state.cast = Some(PendingCast {
                    generation,
                    slot,
                    target,
                    interrupted_walk: (aral
                        && hero == IDRIS
                        && state.walked_at == Some(room.world.tick))
                    .then_some(position),
                    reply: Message::ability_point_used(
                        u32::from(wire_id(id)),
                        coordinates,
                        selector,
                    ),
                });
                return vec![m];
            }
            Vec::new()
        }
        Some(crate::Opcode::RequestTargetAbility)
            if hero == LORELAI
                && m.body.len() == 6
                && m.body[4..] == [NativeCast::LorelaiC.selector(), 0] =>
        {
            let wire_target = u32::from_be_bytes(m.body[..4].try_into().unwrap());
            let Some(target_id) = wire_target.checked_sub(1499) else {
                return Vec::new();
            };
            if !room.world.entities.iter().any(|e| {
                e.id == target_id
                    && e.id != id
                    && e.team == actor.team
                    && e.kind == Kind::Hero
                    && e.hp > 0.0
            }) {
                return Vec::new();
            }
            let slot = AbilitySlot::C;
            let target = CastTarget::Entity(target_id);
            if room
                .world
                .command(id, Command::Cast { slot, target })
                .is_ok()
            {
                state.cast = Some(PendingCast {
                    generation,
                    slot,
                    target,
                    reply: Message::ability_target_used(
                        u32::from(wire_id(id)),
                        wire_target,
                        NativeCast::LorelaiC.selector(),
                    ),
                    interrupted_walk: None,
                });
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

/// Selectors are local to an actor template. The action shape distinguishes
/// a ground point from an allied-actor target.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum NativeCast {
    LorelaiA = 0,
    LorelaiB = 1,
    LorelaiC = 2,
    IdrisB = 3,
}
impl NativeCast {
    const fn selector(self) -> u8 {
        self as u8
    }
    const fn slot(self) -> AbilitySlot {
        match self {
            Self::LorelaiA => AbilitySlot::A,
            Self::LorelaiB | Self::IdrisB => AbilitySlot::B,
            Self::LorelaiC => AbilitySlot::C,
        }
    }
    fn for_slot(hero: u16, slot: AbilitySlot) -> Option<Self> {
        match (hero, slot) {
            (IDRIS, AbilitySlot::B) => Some(Self::IdrisB),
            (LORELAI, AbilitySlot::A) => Some(Self::LorelaiA),
            (LORELAI, AbilitySlot::B) => Some(Self::LorelaiB),
            (LORELAI, AbilitySlot::C) => Some(Self::LorelaiC),
            _ => None,
        }
    }
    fn point_input(hero: u16, selector: u8) -> Option<Self> {
        [AbilitySlot::A, AbilitySlot::B]
            .into_iter()
            .filter_map(|slot| Self::for_slot(hero, slot))
            .find(|cast| cast.selector() == selector)
    }
}

// The origin has a generation-bound confirmation on its issuing control socket.
// Every other seat receives one release on its current world stream.
fn witness(
    room: &mut Room,
    source: u32,
    target: Option<u32>,
    message: &Message,
    ally_release: Option<(u8, std::sync::Arc<super::transport::TeamBatch>)>,
) {
    let needs_spire_registration = source
        .checked_sub(1)
        .and_then(|seat| room.world.entities.get(seat as usize))
        .is_some_and(|e| e.id == source && e.type_id == EntityTemplate::Reim.as_raw());
    let mut shared = None;
    for (index, (seat, actor)) in room.seats.iter().zip(&room.world.entities).enumerate() {
        let visible = &room.visible[actor.team as usize];
        if actor.id != source
            && (seat.world_actor_written || seat.peers[1].is_some())
            && visible.contains(&source)
            && (!needs_spire_registration || room.scene_views[index].knows(source))
            && target.is_none_or(|id| visible.contains(&id))
            && let Some(peer) = seat.gameplay_peer()
        {
            let batch = if let Some((team, release)) = &ally_release
                && *team == actor.team
                && room.scene_views[index].knows(source)
            {
                release
            } else {
                shared
                    .get_or_insert_with(|| super::transport::TeamBatch::new(vec![message.clone()]))
            };
            peer.send(Delivery::Team(batch.clone()));
        }
    }
    room.publish_observers(std::slice::from_ref(message));
}

fn deliver_registered(
    room: &mut Room,
    source: u32,
    team: u8,
    target: Option<u32>,
    out: &[Message],
) {
    let mut shared = None;
    for (index, seat) in room.seats.iter().enumerate() {
        let viewer = &room.world.entities[index];
        let visible = &room.visible[usize::from(team)];
        if viewer.id != source
            && viewer.team == team
            && visible.contains(&source)
            && room.scene_views[index].knows(source)
            && target.is_none_or(|id| visible.contains(&id) && room.scene_views[index].knows(id))
            && (seat.world_actor_written || seat.peers[1].is_some())
            && let Some(peer) = seat.gameplay_peer()
        {
            peer.send(Delivery::Team(
                shared
                    .get_or_insert_with(|| super::transport::TeamBatch::new(out.to_vec()))
                    .clone(),
            ));
        }
    }
}

fn twister_message(source: u32, goal: Position, speed: f32) -> Message {
    let mut body = Vec::with_capacity(21);
    body.extend(u32::from(wire_id(source)).to_be_bytes());
    for v in [speed, goal.x, f32::from_bits(0x3d192e1f), goal.y] {
        body.extend(v.to_be_bytes());
    }
    body.push(0);
    Message::known(crate::Opcode::AutoMoveTo, body)
}

pub(super) fn recordable_twister(m: &Message) -> bool {
    let b = &m.body;
    if !m.is(crate::Opcode::AutoMoveTo) || !matches!(b.len(), 21 | 22) {
        return false;
    }
    let word = |k| u32::from_be_bytes(b[k..k + 4].try_into().unwrap());
    (1500..1506).contains(&word(0))
        && f32::from_bits(word(4)) > 0.
        && [4, 8, 16]
            .iter()
            .all(|k| f32::from_bits(word(*k)).is_finite())
        && word(12) == 0x3d192e1f
        && b[20..].iter().all(|b| *b == 0)
}

pub(super) fn publish_before_impacts(room: &mut Room, events: &[Event]) {
    if !room.legacy_aral() {
        return;
    }
    for event in events {
        if let Event::TwisterDash {
            source,
            goal,
            speed,
        } = *event
            && let Some(i) = source.checked_sub(1)
            && let Some(actor) = room.world.entities.get(i as usize)
            && actor.id == source
            && actor.type_id == EntityTemplate::Karas.as_raw()
        {
            let message = twister_message(source, goal, speed);
            let team = 1 - actor.team;
            deliver_registered(room, source, team, None, std::slice::from_ref(&message));
            room.publish_observer_actors(&[source], std::slice::from_ref(&message));
        }
        if let Event::ChakramLaunch {
            source,
            id,
            origin,
            aim,
        } = *event
        {
            projectile::chakram(
                room,
                source,
                projectile::Chakram::Launch { id, origin, aim },
            );
        }
        if let Event::Cast {
            source,
            slot: AbilitySlot::B,
            target: CastTarget::SelfCast,
        } = *event
            && let Some(index) = source.checked_sub(1)
            && let Some(caster) = room.world.entities.get(index as usize)
            && caster.id == source
            && caster.type_id == EntityTemplate::Lyra.as_raw()
        {
            let batch = vec![Message::ability_target_used(
                u32::from(wire_id(source)),
                u32::MAX,
                2,
            )];
            let enemy_team = 1 - caster.team;
            deliver_registered(room, source, enemy_team, None, &batch);
            room.publish_observer_actor(source, &batch);
        }
        if let Event::ChillWinds {
            source,
            position,
            heading,
        } = *event
        {
            chill_winds::publish(room, source, position, heading);
        }
        skedaddle::publish(room, event);
    }
}

pub(super) fn publish(room: &mut Room, events: &[Event]) {
    if !(room.solo_scene() || room.legacy_aral()) {
        return;
    }
    let aral = room.legacy_aral();
    for (event_index, event) in events.iter().enumerate() {
        if aral
            && let Event::RebootRecovered {
                actor,
                instance,
                cooldown_seconds,
                hp,
            } = *event
        {
            reboot::publish(room, actor, instance, cooldown_seconds, hp);
            continue;
        }
        if aral && let Event::BasicTargetChanged { actor, target } = *event {
            if let Some(i) = actor.checked_sub(1).map(|n| n as usize)
                && let Some(seat) = room.seats.get(i)
                && seat.world_actor_written
                && seat.preamble.confirmed()
                && room.world.entities[i].attack_target() == Some(target)
                && room.visible[usize::from(room.world.entities[i].team)].contains(&target)
                && room.scene_views[i].knows(target)
                && let Some(peer) = seat.gameplay_peer()
            {
                peer.send(Delivery::Private(vec![Message::known(
                    crate::Opcode::ModifyBasicAttackTarget,
                    u32::from(wire_id(target)).to_be_bytes(),
                )]));
            }
            continue;
        }
        if aral
            && let Event::ChakramCatch {
                source,
                id,
                position,
            } = *event
        {
            projectile::chakram(room, source, projectile::Chakram::Catch { id, position });
            continue;
        }
        if aral && let Event::WinterSpireProjectile { source, id, point } = *event {
            projectile::spire(room, source, id, point);
            continue;
        }
        if aral
            && let Event::Move { id, .. } = *event
            && let Some(index) = id.checked_sub(1).map(|v| v as usize)
            && let Some(seat) = room.seats.get_mut(index)
            && room.world.entities[index].type_id == IDRIS
        {
            seat.native_abilities.walked_at = Some(room.world.tick);
        }
        if let Event::ChillWindsRoot {
            source,
            target,
            instance,
        } = *event
        {
            chill_winds::root(room, source, target, instance);
            continue;
        }
        let (source, slot) = match *event {
            Event::Cast { source, slot, .. } | Event::Learned { source, slot } => (source, slot),
            _ => continue,
        };
        let Some(index) = source.checked_sub(1).map(|v| v as usize) else {
            continue;
        };
        let Some(actor) = room
            .world
            .entities
            .get(index)
            .filter(|e| e.id == source && e.kind == Kind::Hero)
        else {
            continue;
        };
        let hero = actor.type_id;
        let actor_wire = u32::from(wire_id(source));
        // Reim is evidenced only on another ARAL seat. There is no verified originating input.
        if aral
            && hero == EntityTemplate::Reim.as_raw()
            && slot == AbilitySlot::B
            && matches!(event, Event::Learned { .. })
        {
            witness(
                room,
                source,
                None,
                &Message::ability_learned(actor_wire, 1),
                None,
            );
            continue;
        }
        if aral && hero == EntityTemplate::Reim.as_raw() && slot == AbilitySlot::A {
            let msg = match *event {
                Event::Learned { .. } => Message::ability_learned(actor_wire, 0),
                Event::Cast {
                    target: CastTarget::Point(p),
                    ..
                } => Message::ability_point_used(actor_wire, [p.x, 0., p.y], 0),
                _ => continue,
            };
            witness(room, source, None, &msg, None);
            continue;
        }
        let Event::Cast { target, .. } = *event else {
            if let Some(operand) = native_learn_operand(hero, slot) {
                witness(
                    room,
                    source,
                    None,
                    &Message::ability_learned(actor_wire, operand.into()),
                    None,
                );
            }
            continue;
        };
        let Some(cast) = NativeCast::for_slot(hero, slot) else {
            continue;
        };
        let (mut message, target_id) = match (cast, target) {
            (
                NativeCast::IdrisB | NativeCast::LorelaiA | NativeCast::LorelaiB,
                CastTarget::Point(p),
            ) => (
                Message::ability_point_used(actor_wire, [p.x, 0., p.y], cast.selector()),
                None,
            ),
            (NativeCast::LorelaiC, CastTarget::Entity(id)) => (
                Message::ability_target_used(actor_wire, u32::from(wire_id(id)), cast.selector()),
                Some(id),
            ),
            _ => continue,
        };
        let chakram = if aral
            && hero == IDRIS
            && slot == AbilitySlot::B
            && let Some(Event::ChakramApplied { source: caster, id }) = events.get(event_index + 1)
            && *caster == source
            && let Some(first) = id.checked_add(1499)
            && let Some(second) = first.checked_add(1)
        {
            let [first, second] = [(61, first), (820, second)].map(|(definition, instance)| {
                halcyon_protocol::BuffApplication {
                    prefix: halcyon_protocol::BuffPrefix {
                        target: actor_wire,
                        source: actor_wire,
                        duration_bits: 0x4066,
                        instance,
                        definition,
                    },
                    extras: None,
                    suffix: &[0; 6],
                }
                .to_message()
                .expect("fixed Chakram effect pair")
            });
            Some([
                Message::position(wire_id(source), actor.position.x, actor.position.y),
                first,
                second,
            ])
        } else {
            None
        };
        let team = actor.team;
        let position = actor.position;
        let mut stop = None;
        if let Some(seat) = room.seats.get_mut(index)
            && let Some(pending) = seat.native_abilities.cast.take()
            && pending.slot == slot
            && pending.target == target
            && seat.world_actor_written
            && let Some(peer) = seat.peers[0]
                .as_ref()
                .filter(|p| p.generation == pending.generation)
        {
            if chakram.is_some() && pending.interrupted_walk == Some(position) {
                stop = chakram_motion::stop(actor_wire, position);
            }
            message = pending.reply;
            let mut batch = vec![message.clone()];
            if aral
                && hero == LORELAI
                && slot == AbilitySlot::C
                && let Some(target) = target_id
                && let Some(instance) = events.iter().find_map(|event| match *event {
                    Event::WaterwallApplied {
                        source: caster,
                        target: ally,
                        id,
                    } if caster == source && ally == target => id.checked_add(1499),
                    _ => None,
                })
            {
                let apply = halcyon_protocol::BuffApplication {
                    prefix: halcyon_protocol::BuffPrefix {
                        target: u32::from(wire_id(target)),
                        source: actor_wire,
                        duration_bits: 0x4400,
                        instance,
                        definition: 954,
                    },
                    extras: None,
                    suffix: &[0; 6],
                };
                batch.push(apply.to_message().expect("fixed Waterwall format"));
            }
            batch.extend(chakram.iter().flatten().cloned());
            peer.send(Delivery::Private(batch));
        }
        let ally_release = chakram.map(|tail| {
            let mut out = Vec::with_capacity(if stop.is_some() { 5 } else { 4 });
            if let Some(stop) = stop {
                out.push(stop);
            }
            out.push(message.clone());
            out.extend(tail);
            (team, super::transport::TeamBatch::new(out))
        });
        witness(room, source, target_id, &message, ally_release);
    }
}

#[cfg(test)]
mod tests {
    use crate::{Peer, Rooms};
    use halcyon_platform::{MatchHost, Reservation};
    use tokio::sync::{mpsc, watch};
    use uuid::Uuid;

    use super::*;

    #[test]
    fn twister_writes_speed_before_destination_and_only_accepts_the_flat_aral_profile() {
        let mut m = twister_message(
            3,
            Position {
                x: -5.930279,
                y: 4.824222,
            },
            21.875313,
        );
        assert!(recordable_twister(&m));
        assert_eq!(u32::from_be_bytes(m.body[0..4].try_into().unwrap()), 1502);
        assert_eq!(&m.body[4..8], &21.875313f32.to_be_bytes());
        assert_eq!(&m.body[8..12], &(-5.930279f32).to_be_bytes());
        assert_eq!(&m.body[12..16], &0x3d192e1fu32.to_be_bytes());
        assert_eq!(&m.body[16..20], &4.824222f32.to_be_bytes());
        m.body[20] = 1;
        assert!(!recordable_twister(&m));
    }
    #[tokio::test]
    async fn learns_once_and_only_confirms_the_registered_caster_after_release() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let match_id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id,
                players: (0..6).map(|_| Uuid::new_v4()).collect(),
                teams: vec![0, 1, 0, 1, 0, 1],
                mode: "casual_aral".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(match_id).unwrap();
        let mut room = shared.lock().unwrap();
        let mut recipients = Vec::new();
        for (i, seat) in room.seats.iter_mut().enumerate().take(3) {
            let (tx, rx) = mpsc::channel(64);
            seat.peers[0] = Some(Peer {
                generation: 1 + i as u64,
                tx,
                stalled: watch::channel(false).0,
            });
            for (opcode, body) in [
                (crate::wire_opcode::RequestScene, [0; 6]),
                (
                    crate::wire_opcode::SceneOwner,
                    [0, 0, 5, 220 + i as u8, 1, 0],
                ),
                (crate::wire_opcode::SceneConfirmed, [0; 6]),
            ] {
                seat.preamble
                    .observe(&Message::new(opcode, body), 1500 + i as u32);
            }
            recipients.push(rx);
        }
        room.phase(crate::RoomPhase::Running);
        room.world.entities[0].type_id = IDRIS;
        room.world.entities[0].skill_points = 2;
        room.world.entities[0].position = Position { x: 8.0, y: 0.0 };

        let learn = Message::known(crate::Opcode::RequestUpgradeAbility, [1, 0, 0, 0, 0, 0]);
        let mut body = [0; 14];
        body[..4].copy_from_slice(&12f32.to_be_bytes());
        body[4..8].copy_from_slice(&(-0.0_f32).to_be_bytes());
        body[12] = NativeCast::IdrisB.selector();
        let cast = Message::known(crate::Opcode::RequestPointAbility, body);

        assert!(room.receive(0, 0, learn.clone()).unwrap().is_empty());
        room.seats[0].world_actor_written = true;
        room.seats[1].world_actor_written = true;
        room.seats[2].world_actor_written = true;
        let mut declared = super::super::messages::spawn(&room.world.entities[0], &[], None);
        declared.body[745] = 16;
        for view in &mut room.scene_views[..3] {
            view.declarations(std::slice::from_ref(&declared));
        }
        assert!(room.receive(1, 0, learn.clone()).unwrap().is_empty());
        assert!(room.receive(0, 0, cast.clone()).unwrap().is_empty());
        room.tick();
        assert!(drain(&mut recipients[0]).is_empty());

        let learned = room.receive(0, 0, learn.clone()).unwrap();
        assert_eq!(learned, [learn.clone(), Message::ability_learned(1500, 3)]);
        assert!(room.receive(0, 0, learn).unwrap().is_empty());
        assert_eq!(room.world.entities[0].skill_points, 1);
        assert_eq!(room.world.entities[0].ability_ranks, [0, 1, 0]);
        let mut invalid = cast.clone();
        invalid.body[..4].copy_from_slice(&1000f32.to_be_bytes());
        assert!(room.receive(0, 0, invalid).unwrap().is_empty());
        room.tick();
        assert!(drain(&mut recipients[0]).is_empty());

        room.world
            .command(1, Command::MoveTo(Position { x: 9., y: 0. }))
            .unwrap();
        room.tick();
        assert_eq!(
            room.receive(0, 0, cast.clone()).unwrap().as_slice(),
            std::slice::from_ref(&cast)
        );
        assert!(
            room.seats[0]
                .native_abilities
                .cast
                .as_ref()
                .unwrap()
                .interrupted_walk
                .is_some()
        );
        let cancel = |selector, shared| {
            Message::known(
                crate::Opcode::RequestCancelAbility,
                [selector, shared, 0, 0, 0, 0],
            )
        };
        assert!(room.receive(0, 0, cancel(2, 0)).unwrap().is_empty());
        assert!(room.receive(0, 0, cancel(3, 1)).unwrap().is_empty());
        assert!(room.receive(1, 0, cancel(3, 0)).unwrap().is_empty());
        assert_eq!(
            room.receive(0, 0, cancel(3, 0)).unwrap(),
            [Message::known(
                crate::Opcode::CancelAbility,
                [0, 0, 5, 220, 0, 0, 0, 3, 0, 0]
            )]
        );
        assert!(room.receive(0, 0, cancel(3, 0)).unwrap().is_empty());
        for _ in 0..12 {
            room.tick();
        }
        assert!(drain(&mut recipients[0]).is_empty());
        assert!(drain(&mut recipients[1]).is_empty());
        assert!(drain(&mut recipients[2]).is_empty());
        room.world
            .command(1, Command::MoveTo(Position { x: 9., y: 0. }))
            .unwrap();
        room.tick();
        let stopped = room.world.entities[0].position;
        assert!(stopped.x > 8.);
        assert_eq!(
            room.receive(0, 0, cast.clone()).unwrap().as_slice(),
            std::slice::from_ref(&cast)
        );
        assert!(room.receive(0, 0, cast.clone()).unwrap().is_empty());
        assert!(drain(&mut recipients[0]).is_empty());
        for _ in 0..12 {
            room.tick();
        }
        let issuer = drain(&mut recipients[0]);
        assert_eq!(issuer.len(), 4);
        assert_eq!(
            issuer[0],
            Message::ability_point_used(1500, [12., -0.0, 0.], 3)
        );
        assert_eq!(issuer[1], Message::position(1500, stopped.x, stopped.y));
        let buffs = [&issuer[2], &issuer[3]].map(|message| {
            let Ok(halcyon_protocol::Fields::BuffApplication(buff)) = message.fields() else {
                panic!("missing released Chakram application")
            };
            assert_eq!(
                (
                    buff.prefix.source,
                    buff.prefix.target,
                    buff.prefix.duration_bits
                ),
                (1500, 1500, 0x4066)
            );
            assert!(buff.prefix.instance > 1505);
            assert_eq!(buff.suffix, [0; 6]);
            buff.prefix
        });
        assert_eq!(buffs.map(|b| b.definition), [61, 820]);
        assert_eq!(buffs[0].instance + 1, buffs[1].instance);
        assert_eq!(drain(&mut recipients[1]), issuer[..1]);
        let teammate = drain(&mut recipients[2]);
        let stop = chakram_motion::stop(1500, stopped).unwrap();
        assert!(recordable_stop(&stop));
        assert_eq!(
            teammate,
            [
                stop,
                issuer[0].clone(),
                issuer[1].clone(),
                issuer[2].clone(),
                issuer[3].clone()
            ]
        );
        assert!(room.receive(0, 0, cast).unwrap().is_empty());
        for _ in 0..12 {
            room.tick();
        }
        assert!(drain(&mut recipients[0]).is_empty());

        let point = room.world.entities[0].position;
        room.world.entities[1].position = point;
        room.world.command(1, Command::Attack(2)).unwrap();
        let change = [Event::BasicTargetChanged {
            actor: 1,
            target: 2,
        }];
        room.visible[0].remove(&2);
        publish(&mut room, &change);
        assert!(drain(&mut recipients[0]).is_empty());
        let mut target = super::super::messages::spawn(&room.world.entities[1], &[], None);
        target.body[745] = 17;
        room.scene_views[0].declarations(&[target]);
        room.visible[0].insert(2);
        publish(&mut room, &change);
        assert_eq!(
            drain(&mut recipients[0]),
            [Message::known(
                crate::Opcode::ModifyBasicAttackTarget,
                1501u32.to_be_bytes()
            )]
        );
        assert!(drain(&mut recipients[1]).is_empty());
        assert!(drain(&mut recipients[2]).is_empty());
    }

    #[tokio::test]
    async fn lorelai_learns_three_selectors_and_confirms_only_released_ally_or_point_casts() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let match_id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id,
                players: (0..6).map(|_| Uuid::new_v4()).collect(),
                teams: vec![],
                mode: "casual_aral".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(match_id).unwrap();
        let mut room = shared.lock().unwrap();
        let (tx, mut rx) = mpsc::channel(128);
        room.seats[0].peers[0] = Some(Peer {
            generation: 1,
            tx,
            stalled: watch::channel(false).0,
        });
        for (op, body) in [
            (crate::wire_opcode::RequestScene, [0; 6]),
            (crate::wire_opcode::SceneOwner, [0, 0, 5, 220, 1, 0]),
            (crate::wire_opcode::SceneConfirmed, [0; 6]),
        ] {
            room.seats[0]
                .preamble
                .observe(&Message::new(op, body), 1500);
        }
        room.phase(crate::RoomPhase::Running);
        room.seats[0].world_actor_written = true;
        room.world.entities[0].type_id = LORELAI;
        room.world.entities[0].level = 10;
        room.world.entities[0].max_energy = 1000.;
        room.world.entities[0].energy = 1000.;
        room.world.entities[0].skill_points = 3;
        let ally = room
            .world
            .entities
            .iter()
            .skip(1)
            .find(|e| e.kind == Kind::Hero && e.team == room.world.entities[0].team)
            .unwrap()
            .id;
        let position = Position { x: 1., y: 0. };
        room.world.entities[0].position = position;
        room.world.entities[(ally - 1) as usize].position = position;
        for selector in 0..3 {
            let request = Message::known(
                crate::Opcode::RequestUpgradeAbility,
                [selector, 0, 0, 0, 0, 0],
            );
            assert_eq!(
                room.receive(0, 0, request.clone()).unwrap(),
                [request, Message::ability_learned(1500, selector.into())]
            );
            room.world.tick += 1;
            let packet = if selector < 2 {
                let mut body = [0; 14];
                body[..4].copy_from_slice(&position.x.to_be_bytes());
                body[12] = selector;
                Message::known(crate::Opcode::RequestPointAbility, body)
            } else {
                let mut body = [0; 6];
                body[..4].copy_from_slice(&u32::from(wire_id(ally)).to_be_bytes());
                body[4] = 2;
                Message::known(crate::Opcode::RequestTargetAbility, body)
            };
            if selector == 2 {
                // The native report witnessed a base veil away from the earlier pools.
                let dry = Position { x: 9., y: 1. };
                room.world.entities[0].position = dry;
                room.world.entities[(ally - 1) as usize].position = dry;
            }
            let response = room.receive(0, 0, packet.clone()).unwrap();
            if selector < 2 {
                assert_eq!(response, [packet]);
            } else {
                assert!(response.is_empty());
            }
            assert!(
                room.seats[0].native_abilities.cast.is_some(),
                "selector {selector}"
            );
            assert!(rx.try_recv().is_err());
            let expected = if selector < 2 {
                Message::ability_point_used(1500, [position.x, 0., position.y], selector)
            } else {
                Message::ability_target_used(1500, u32::from(wire_id(ally)), 2)
            };
            for _ in 0..60 {
                let events = room.world.step();
                publish(&mut room, &events);
                if room.seats[0].native_abilities.cast.is_none() {
                    break;
                }
            }
            let batch = rx.try_recv().unwrap().into_records();
            assert_eq!(batch[0], expected);
            if selector == 2 {
                assert_eq!(batch.len(), 2);
                let Ok(halcyon_protocol::Fields::BuffApplication(effect)) = batch[1].fields()
                else {
                    panic!("expected the targeted base Waterwall after its release");
                };
                assert_eq!(effect.prefix.source, 1500);
                assert_eq!(effect.prefix.target, u32::from(wire_id(ally)));
                assert_eq!(effect.prefix.definition, 954);
                assert_eq!(effect.prefix.duration(), 4.);
                assert!(effect.prefix.instance > 1505);
                assert_eq!(effect.suffix, [0; 6]);
            } else {
                assert_eq!(batch.len(), 1);
            }
        }
    }

    #[tokio::test]
    async fn witnessed_releases_require_visible_source_and_target_and_skip_replaced_origin() {
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
        let mut streams = vec![];
        for seat in &mut room.seats {
            let (tx, rx) = mpsc::channel(8);
            seat.peers[1] = Some(Peer {
                generation: 2,
                tx: tx.clone(),
                stalled: watch::channel(false).0,
            });
            seat.peers[0] = Some(Peer {
                generation: 2,
                tx,
                stalled: watch::channel(false).0,
            });
            seat.world_actor_written = true;
            streams.push(rx);
        }
        room.world.entities[0].type_id = LORELAI;
        let target = CastTarget::Entity(2);
        room.seats[0].native_abilities.cast = Some(PendingCast {
            interrupted_walk: None,
            slot: AbilitySlot::C,
            target,
            reply: Message::ability_target_used(1500, 1501, 2),
            generation: 1,
        });
        room.visible[0].extend([1, 2]);
        room.visible[1].insert(1);
        let cast = Event::Cast {
            source: 1,
            slot: AbilitySlot::C,
            target,
        };
        publish(
            &mut room,
            &[
                cast.clone(),
                Event::WaterwallApplied {
                    source: 1,
                    target: 2,
                    id: 90,
                },
            ],
        );
        assert!(streams[0].try_recv().is_err());
        assert_eq!(
            streams[1].try_recv().unwrap().into_records(),
            [Message::ability_target_used(1500, 1501, 2)]
        );
        assert!(streams[2].try_recv().is_err());
        room.visible[1].clear();
        room.visible[1].insert(2);
        publish(&mut room, std::slice::from_ref(&cast));
        assert!(streams[2].try_recv().is_err());
        room.visible[1].insert(1);
        publish(&mut room, &[cast]);
        assert_eq!(
            streams[2].try_recv().unwrap().into_records(),
            [Message::ability_target_used(1500, 1501, 2)]
        );
    }

    #[tokio::test]
    async fn winter_spire_publishes_only_the_registered_visible_witness_form() {
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
        room.world.entities[0].type_id = EntityTemplate::Reim.as_raw();
        let (tx, mut rx) = mpsc::channel(8);
        room.seats[1].peers[1] = Some(Peer {
            generation: 1,
            tx,
            stalled: watch::channel(false).0,
        });
        room.visible[0].insert(1);
        let p = Position {
            x: -3.9156992,
            y: 7.9015503,
        };
        let event = Event::Cast {
            source: 1,
            slot: AbilitySlot::A,
            target: CastTarget::Point(p),
        };
        let events = [
            event,
            Event::WinterSpireProjectile {
                source: 1,
                id: 800,
                point: p,
            },
        ];
        publish(&mut room, &events);
        assert!(rx.try_recv().is_err());
        let mut hero = crate::messages::spawn(&room.world.entities[0], &room.picks, None);
        hero.body[745] = 16;
        room.scene_views[1].declarations(&[hero]);
        publish(&mut room, &events);
        assert_eq!(
            rx.try_recv().unwrap().into_records(),
            [Message::ability_point_used(1500, [p.x, 0., p.y], 0)]
        );
        let batch = rx.try_recv().unwrap().into_records();
        let shot = &batch[0];
        assert!(recordable_spell_projectile(shot));
        assert_eq!(&shot.body[0..4], &2299u32.to_be_bytes());
        assert_eq!(&shot.body[4..8], &1043493024u32.to_be_bytes());
        assert_eq!(&shot.body[8..12], &p.x.to_be_bytes());
        assert_eq!(&shot.body[12..16], &[0; 4]);
        assert_eq!(&shot.body[16..20], &p.y.to_be_bytes());
        assert_eq!(&shot.body[20..], &[0, 0, 0, 0, 0, 86, 16, 16]);
        room.visible[0].clear();
        publish(&mut room, &events);
        assert!(rx.try_recv().is_err());
        assert!(NativeCast::point_input(EntityTemplate::Reim.as_raw(), 0).is_none());
        assert!(native_learn_request(EntityTemplate::Reim.as_raw(), 0).is_none());
    }

    #[tokio::test]
    async fn chill_winds_sends_the_witnessed_pair_and_a_registered_one_second_root() {
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
        room.world.entities[0].type_id = EntityTemplate::Reim.as_raw();
        let mut streams = Vec::new();
        let mut caster = crate::messages::spawn(&room.world.entities[0], &room.picks, None);
        caster.body[745] = 16;
        let mut target = crate::messages::spawn(&room.world.entities[2], &room.picks, None);
        target.body[745] = 18;
        for i in 0..3 {
            let (tx, rx) = mpsc::channel(8);
            room.seats[i].peers[1] = Some(Peer {
                generation: 1,
                tx,
                stalled: watch::channel(false).0,
            });
            streams.push(rx);
            room.scene_views[i].declarations(std::slice::from_ref(&caster));
        }
        for v in &mut room.visible {
            v.extend([1, 3]);
        }
        let p = Position { x: 4., y: 2. };
        let angle = Some(-2.4);
        let start = Event::ChillWinds {
            source: 1,
            position: p,
            heading: angle,
        };
        let root = Event::ChillWindsRoot {
            source: 1,
            target: 3,
            instance: 700,
        };
        publish_before_impacts(&mut room, &[start]);
        publish(&mut room, std::slice::from_ref(&root));
        let batch = streams[1].try_recv().unwrap().into_records();
        assert_eq!(batch[0], Message::ability_target_used(1500, u32::MAX, 1));
        let pfx = batch[1].play_pfx_at_location().unwrap();
        assert_eq!(pfx.position, [4., f32::from_bits(0x3d192e1f), 2.]);
        assert_eq!(pfx.unknown_word, (-2.4f32).to_bits());
        assert!(recordable_chill_winds(&batch[1]));
        for rx in &mut streams {
            assert!(rx.try_recv().is_err());
        }
        room.scene_views[1].declarations(&[target]);
        publish(&mut room, &[root]);
        let buff = streams[1].try_recv().unwrap().into_records();
        let halcyon_protocol::Fields::BuffApplication(application) = buff[0].fields().unwrap()
        else {
            panic!("expected root")
        };
        assert_eq!(
            (
                application.prefix.target,
                application.prefix.source,
                application.prefix.definition,
                application.prefix.duration()
            ),
            (1502, 1500, 654, 1.)
        );
        publish_before_impacts(
            &mut room,
            &[Event::ChillWinds {
                source: 1,
                position: p,
                heading: None,
            }],
        );
        assert_eq!(
            streams[1].try_recv().unwrap().into_records(),
            [Message::ability_target_used(1500, u32::MAX, 1)]
        );
    }

    #[tokio::test]
    async fn solo_hero_bot_successful_learn_reaches_registered_world_peer() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: id,
                players: vec![Uuid::new_v4()],
                teams: vec![0],
                mode: "solo".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut room = shared.lock().unwrap();
        let (tx, mut bot_view) = mpsc::channel(16);
        room.seats[0].peers[1] = Some(Peer {
            generation: 1,
            tx,
            stalled: watch::channel(false).0,
        });
        let bot = &mut room.world.entities[3];
        assert!(bot.bot);
        bot.type_id = IDRIS;
        bot.skill_points = 1;
        bot.ability_ranks = [0; 3];
        let bot_id = bot.id;
        room.visible[0].insert(bot_id);
        for _ in 0..5 {
            let events = room.world.step();
            publish(&mut room, &events);
        }
        assert_eq!(
            bot_view.try_recv().unwrap().into_records(),
            [Message::ability_learned(u32::from(wire_id(bot_id)), 3)]
        );
    }

    #[tokio::test]
    async fn progression_only_grants_use_rank_gates_and_allow_a_later_retry() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let match_id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id,
                players: (0..6).map(|_| Uuid::new_v4()).collect(),
                teams: vec![],
                mode: "casual_aral".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(match_id).unwrap();
        let mut room = shared.lock().unwrap();
        let mut witnesses = Vec::new();
        for (index, seat) in room.seats.iter_mut().enumerate().take(2) {
            let (tx, rx) = mpsc::channel(16);
            seat.peers[0] = Some(Peer {
                generation: (index + 1) as u64,
                tx,
                stalled: watch::channel(false).0,
            });
            let actor = 1500 + index as u32;
            for (op, body) in [
                (crate::wire_opcode::RequestScene, [0; 6]),
                (
                    crate::wire_opcode::SceneOwner,
                    [0, 0, 5, 220 + index as u8, 1, 0],
                ),
                (crate::wire_opcode::SceneConfirmed, [0; 6]),
            ] {
                seat.preamble.observe(&Message::new(op, body), actor);
            }
            seat.world_actor_written = true;
            witnesses.push(rx);
        }
        room.phase(crate::RoomPhase::Running);
        room.world.entities[0].type_id = IDRIS;
        room.world.entities[0].level = 5;
        room.world.entities[0].skill_points = 2;

        let learn =
            |index| Message::known(crate::Opcode::RequestUpgradeAbility, [index, 0, 0, 0, 0, 0]);
        assert!(room.receive(0, 0, learn(2)).unwrap().is_empty());
        assert_eq!(room.world.entities[0].skill_points, 2);
        room.world.tick += 1;
        room.world.entities[0].level = 6;
        room.visible[1].insert(1);
        assert_eq!(
            room.receive(0, 0, learn(2)).unwrap(),
            [learn(2), Message::ability_learned(1500, 4)]
        );
        assert!(room.receive(0, 0, learn(2)).unwrap().is_empty());
        assert!(room.receive(0, 0, learn(0)).unwrap().is_empty());
        assert_eq!(room.world.entities[0].skill_points, 1);
        assert!(witnesses[0].try_recv().is_err());
        assert_eq!(
            witnesses[1].try_recv().unwrap().into_records(),
            [Message::ability_learned(1500, 4)]
        );
        assert!(witnesses[1].try_recv().is_err());

        let mut unsupported_cast = [0; 14];
        unsupported_cast[12] = 4;
        assert!(
            room.receive(
                0,
                0,
                Message::known(crate::Opcode::RequestPointAbility, unsupported_cast)
            )
            .unwrap()
            .is_empty()
        );
        room.world.tick += 1;
        room.visible[1].clear();
        assert_eq!(
            room.receive(0, 0, learn(0)).unwrap(),
            [learn(0), Message::ability_learned(1500, 0)]
        );
        assert!(witnesses[1].try_recv().is_err());
        assert_eq!(room.world.entities[0].skill_points, 0);
        assert_eq!(room.world.entities[0].ability_ranks, [1, 0, 1]);
    }

    fn drain(receiver: &mut mpsc::Receiver<Delivery>) -> Vec<Message> {
        let mut replies = vec![];
        while let Ok(batch) = receiver.try_recv() {
            let records = batch.into_records();
            let release_batch = records
                .iter()
                .any(|m| m.is(crate::Opcode::PlayPointAbility));
            replies.extend(records.into_iter().filter(|m| {
                matches!(
                    m.opcode,
                    crate::wire_opcode::ModifyBasicAttackTarget
                        | crate::wire_opcode::StopActor
                        | crate::wire_opcode::PlayPointAbility
                        | crate::wire_opcode::EntityProp
                ) || (release_batch && m.is(crate::Opcode::Position))
            }));
        }
        replies
    }
}
