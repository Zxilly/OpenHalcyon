use halcyon_data::EntityTemplate;
use halcyon_sim::{
    AbilitySlot, CastTarget, Command, Event, Kind, Position, native_learn_operand,
    native_learn_request,
};

use super::{Batch, Message, Room, messages::wire_id, transport::Delivery};

mod amael;
pub(super) use amael::recordable as recordable_amael;
mod chakram_motion;
mod chill_winds;
mod projectile;
pub(super) use projectile::recordable as recordable_spell_projectile;
mod reboot;
mod skedaddle;
pub(super) use reboot::recordable as recordable_reboot;
pub(super) fn recordable_skedaddle(m: &Message) -> bool {
    skedaddle::recordable(m)
}
pub(super) fn recordable_chill_winds(m: &Message) -> bool {
    chill_winds::recordable_pfx(m)
}
pub(super) use chakram_motion::recordable_stop;

const AMAEL: u16 = EntityTemplate::Amael.as_raw();
const IDRIS: u16 = EntityTemplate::Idris.as_raw();
const LORELAI: u16 = EntityTemplate::Lorelai.as_raw();

#[derive(Default)]
pub(super) struct NativeAbilities {
    pub(super) learned_at: Option<u64>,
    walked_at: Option<u64>,
    cast: Option<PendingCast>,
    pub(super) catalog_cast: Option<crate::native_abilities_catalog::PendingCatalogCast>,
    pub(super) tripwires: crate::native_abilities_catalog::TripwireWireTracker,
}

impl NativeAbilities {
    pub(super) fn take_legacy_cast_slot(&mut self) -> Option<AbilitySlot> {
        self.cast.take().map(|pending| pending.slot)
    }
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
    if actor.kind != Kind::Hero || actor.bot {
        return Vec::new();
    }
    if !room.legacy_aral() {
        let valid_channel =
            channel == 1 || (channel == 0 && room.solo_scene() && connection.world_actor_written);
        if !valid_channel {
            return Vec::new();
        }
        let _ = peer;
        return crate::native_abilities_catalog::receive(room, seat, m);
    }
    if channel != 0 || !connection.world_actor_written {
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
            if aral
                && matches!(
                    halcyon_protocol::RequestCancelAbilityBody::read_padded_zero(&m.body),
                    Ok(req) if req.cancel_flag == 0
                ) =>
        {
            let req = halcyon_protocol::RequestCancelAbilityBody::read_padded_zero(&m.body)
                .expect("validated cancel ability request");
            let Some(pending) = &state.cast else {
                return Vec::new();
            };
            let selector = req.ability_slot;
            if pending.generation != generation
                || NativeCast::for_slot(hero, pending.slot).is_none_or(|c| c.selector() != selector)
                || !room.world.cancel_cast(id, pending.slot, pending.target)
            {
                return Vec::new();
            }
            state.cast = None;
            vec![
                m,
                halcyon_protocol::CancelAbilityBody {
                    actor: u32::from(wire_id(id)),
                    reference: u32::from(selector),
                    is_item: 0,
                    cancel_shared: 0,
                }
                .encode(),
            ]
        }
        Some(crate::Opcode::RequestUpgradeAbility)
            if halcyon_protocol::RequestUpgradeAbilityBody::read_padded_zero(&m.body).is_ok() =>
        {
            let req = halcyon_protocol::RequestUpgradeAbilityBody::read_padded_zero(&m.body)
                .expect("validated upgrade ability request");
            if hero == AMAEL && (!aral || !room.scene_views[seat].knows(id)) {
                return Vec::new();
            }
            let Some((slot, operand)) = native_learn_request(hero, req.ability_slot) else {
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
        Some(crate::Opcode::RequestPointAbility)
            if halcyon_protocol::RequestPointAbilityBody::read_padded_zero(&m.body).is_ok() =>
        {
            let req = halcyon_protocol::RequestPointAbilityBody::read_padded_zero(&m.body)
                .expect("validated point ability request");
            let selector = req.selector;
            let Some(cast) = NativeCast::point_input(hero, selector) else {
                return Vec::new();
            };
            let slot = cast.slot();
            let coordinates = req.coordinates;
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
            if (hero == LORELAI || (aral && hero == AMAEL))
                && matches!(
                    halcyon_protocol::RequestTargetAbilityBody::read_padded_zero(&m.body),
                    Ok(req) if req.selector == NativeCast::LorelaiC.selector()
                ) =>
        {
            let wire_target = halcyon_protocol::RequestTargetAbilityBody::read_padded_zero(&m.body)
                .expect("validated target ability request")
                .target;
            let Some(target_id) = wire_target.checked_sub(1499) else {
                return Vec::new();
            };
            if hero == AMAEL
                && !(room.scene_views[seat].knows(id)
                    && room.scene_views[seat].knows(target_id)
                    && room.visible[actor.team as usize].contains(&target_id))
            {
                return Vec::new();
            }
            if !room.world.entities.iter().any(|e| {
                e.id == target_id
                    && e.id != id
                    && (e.team == actor.team) == (hero == LORELAI)
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
enum NativeCast {
    LorelaiA,
    LorelaiB,
    LorelaiC,
    IdrisB,
    AmaelC,
}
impl NativeCast {
    const fn selector(self) -> u8 {
        match self {
            Self::LorelaiA => 0,
            Self::LorelaiB => 1,
            Self::LorelaiC | Self::AmaelC => 2,
            Self::IdrisB => 3,
        }
    }
    const fn slot(self) -> AbilitySlot {
        match self {
            Self::LorelaiA => AbilitySlot::A,
            Self::LorelaiB | Self::IdrisB => AbilitySlot::B,
            Self::LorelaiC | Self::AmaelC => AbilitySlot::C,
        }
    }
    fn for_slot(hero: u16, slot: AbilitySlot) -> Option<Self> {
        match (hero, slot) {
            (AMAEL, AbilitySlot::C) => Some(Self::AmaelC),
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
pub(super) fn witness(
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

pub(super) fn deliver_registered(
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

fn aral_dash_message(source: u32, goal: Position, speed: f32) -> Message {
    halcyon_protocol::ForcedMove {
        actor: u32::from(wire_id(source)),
        speed,
        destination: [goal.x, f32::from_bits(0x3d192e1f), goal.y],
        mode: halcyon_protocol::ForcedMoveMode::Dash,
    }
    .message()
}

pub(super) fn recordable_dash(m: &Message) -> bool {
    m.is(crate::Opcode::AutoMoveTo)
        && halcyon_protocol::ForcedMove::read_with_padding(&m.body).is_ok_and(|(motion, tail)| {
            (1500..1506).contains(&motion.actor)
                && motion.mode == halcyon_protocol::ForcedMoveMode::Dash
                && motion.speed.is_finite()
                && motion.speed > 0.
                && motion.destination.iter().all(|v| v.is_finite())
                && motion.destination[1].to_bits() == 0x3d192e1f
                && tail.iter().all(|&b| b == 0)
        })
}

pub(super) fn publish_before_impacts(room: &mut Room, events: &[Event]) {
    if !room.legacy_aral() {
        return;
    }
    for event in events {
        if matches!(
            event,
            Event::MobileBulwarkStart { .. } | Event::MobileBulwarkEnd { .. }
        ) {
            super::mobile_bulwark::publish(room, event);
        }
        if let Event::AmaelKick {
            source,
            target,
            instance,
            kind,
        } = *event
        {
            amael::publish(room, source, target, instance, kind, events);
        }
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
            let message = aral_dash_message(source, goal, speed);
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

/// All actor references must belong to the recipient's visible, registered scene.
fn publish_visible(room: &mut Room, actors: &[u32], frames: &[Message]) {
    let mut shared = None;
    for (i, seat) in room.seats.iter().enumerate() {
        let team = usize::from(room.world.entities[i].team);
        if (seat.world_actor_written || seat.peers[1].is_some())
            && actors
                .iter()
                .all(|id| room.visible[team].contains(id) && room.scene_views[i].knows(*id))
            && let Some(peer) = seat.gameplay_peer()
        {
            peer.send(Delivery::Team(
                shared
                    .get_or_insert_with(|| super::transport::TeamBatch::new(frames.to_vec()))
                    .clone(),
            ));
        }
    }
    room.publish_observer_actors(actors, frames);
}

/// Encodes an `0x0425 ModifyBasicAttackTarget` frame, using `u32::MAX` to clear the lock.
pub(super) fn modify_basic_attack_target_message(target: Option<u32>) -> Message {
    halcyon_protocol::ModifyBasicAttackTargetBody {
        target: target.map(|actor| u32::from(wire_id(actor))),
    }
    .encode()
}

pub(super) fn publish(room: &mut Room, events: &[Event]) {
    let aral = room.legacy_aral();
    if !aral {
        crate::native_abilities_catalog::tick_tripwires(room);
    }
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
        if aral
            && let Event::RebootStarted {
                actor, instance, ..
            } = *event
        {
            reboot::start(room, actor, instance);
            continue;
        }
        if let Event::BasicTargetChanged { actor, target } = *event {
            if let Some(i) = actor.checked_sub(1).map(|n| n as usize)
                && let Some(seat) = room.seats.get(i)
                && (seat.peers[1].is_some()
                    || ((aral || room.solo_scene())
                        && seat.world_actor_written
                        && seat.preamble.confirmed()
                        && room.scene_views[i].knows(target)))
                && room.world.entities[i].attack_target() == Some(target)
                && room.visible[usize::from(room.world.entities[i].team)].contains(&target)
                && let Some(peer) = seat.gameplay_peer()
            {
                peer.send(Delivery::Private(vec![modify_basic_attack_target_message(
                    Some(target),
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
        if !aral
            && room
                .world
                .entities
                .iter()
                .position(|e| e.id == source)
                .and_then(|idx| room.seats.get(idx))
                .is_none_or(|seat| seat.native_abilities.cast.is_none())
        {
            crate::native_abilities_catalog::publish_event(room, event);
            continue;
        }
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
            (NativeCast::LorelaiC | NativeCast::AmaelC, CastTarget::Entity(id)) => (
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
        let lunge = match events.get(event_index + 1) {
            Some(Event::AmaelLunge {
                source: caster,
                target: victim,
                goal,
                speed,
            }) if aral
                && cast == NativeCast::AmaelC
                && *caster == source
                && Some(*victim) == target_id =>
            {
                Some(aral_dash_message(source, *goal, *speed))
            }
            _ => None,
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
            batch.extend(lunge.iter().cloned());
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
        if cast == NativeCast::AmaelC
            && let Some(victim) = target_id
        {
            let release: Batch = std::iter::once(message).chain(lunge).collect();
            deliver_registered(room, source, 1 - team, Some(victim), &release);
            room.publish_observer_actors(&[source, victim], &release);
        } else {
            witness(room, source, target_id, &message, ally_release);
        }
    }
}

#[cfg(test)]
mod tests;
