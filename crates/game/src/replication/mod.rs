pub(crate) mod compact;
pub(crate) mod hero_progress;
pub(crate) mod messages;
pub(crate) mod npc_attack;
pub(crate) mod npc_projectile;
pub(crate) mod result_frames;
pub(crate) mod slots;
pub(crate) mod structure_effects;

use crate::abilities::legacy::ringo;
use crate::modes::{aral, fold_blitz};
use crate::room::Room;
use crate::transport;
use halcyon_protocol::{Message, ce::KnownQueue};
use halcyon_sim::{Entity, Event};
use messages::{snapshot, spawn, wire_id};
use std::collections::HashSet;

pub(crate) type Batch = Vec<Message>;

const VISIBILITY_STATE_NORMAL: u8 = 1;
const VISIBILITY_MASK_HIDDEN: u8 = 0;
const VISIBILITY_MASK_VISIBLE: u8 = 15;

/// Returns true for non-own-team mobile and neutral actors whose client visibility
/// is toggled via `0x042B` (`Opcode::ModifyVisibility`). Own-team units and static
/// lane structures (`Tower`, `Crystal`, `Armory`) remain visible via their spawn masks.
pub(crate) fn uses_visibility_toggle(viewer_team: u8, entity: &Entity) -> bool {
    entity.team != viewer_team
        && matches!(
            entity.kind,
            halcyon_sim::Kind::Hero | halcyon_sim::Kind::Minion | halcyon_sim::Kind::Neutral
        )
}

pub(crate) fn actor_visibility_message(actor_id: u32, viewer_team: u8, visible: bool) -> Message {
    let mask = if visible {
        VISIBILITY_MASK_VISIBLE
    } else {
        VISIBILITY_MASK_HIDDEN
    };
    Message::actor_visibility(
        u32::from(wire_id(actor_id)),
        viewer_team + 1,
        VISIBILITY_STATE_NORMAL,
        [mask, 0],
    )
}

struct ReplicationFrame<'a> {
    events: &'a [Event],
    fallen: HashSet<u32>,
    native_solo: bool,
    postgame: Option<u32>,
    legacy_aral: bool,
    facing_targets: [Option<u32>; aral::SEATS],
    control_stream: bool,
    born: HashSet<u32>,
}

impl Room {
    pub(crate) fn allocate_entity_declaration(
        &mut self,
        index: usize,
        team: u8,
        native_solo: bool,
        legacy_aral: bool,
        born: bool,
    ) -> Option<Message> {
        if fold_blitz::is_managed(self.known_queue) {
            return fold_blitz::allocate_declaration(self, index, native_solo);
        }
        if self.known_queue == Some(KnownQueue::BattleRoyaleBotsSolo) {
            return fold_blitz::allocate_aral_solo_declaration(self, index);
        }
        let e = &self.world.entities[index];
        if native_solo {
            self.solo_slots
                .declaration_at(&self.world, e, &self.picks, self.data.as_deref(), born)
        } else if legacy_aral {
            self.aral_slots.declaration(
                &self.world,
                e,
                &self.picks,
                self.data.as_deref(),
                team,
                born,
            )
        } else {
            Some(spawn(e, &self.picks, self.data.as_deref()))
        }
    }

    pub(crate) fn drain_5v5_camp_messages(&mut self) -> (Batch, [Batch; 2]) {
        let mut obs = Vec::new();
        let mut teams: [Batch; 2] = [Vec::new(), Vec::new()];
        if !self.world.is_5v5_enabled() {
            return (obs, teams);
        }
        for t in self.world.take_5v5_camp_transitions() {
            let msg = Message::camp_visibility(
                t.actor_id,
                t.viewer_index,
                halcyon_protocol::CampVisibilityState::from_wire(t.state as u8),
            );
            match t.viewer_index {
                0 => obs.push(msg),
                1 => teams[0].push(msg),
                2 => teams[1].push(msg),
                _ => {}
            }
        }
        (obs, teams)
    }

    #[cfg(test)]
    pub(crate) fn replicate(&mut self, events: &[Event]) {
        self.replicate_with_camps(events, &[Vec::new(), Vec::new()]);
    }

    pub(crate) fn replicate_with_camps(&mut self, events: &[Event], team_camps: &[Batch; 2]) {
        let fallen: HashSet<u32> = events
            .iter()
            .filter_map(|event| match event {
                Event::Death { id, .. } => Some(*id),
                _ => None,
            })
            .collect();
        let native_solo = self.solo_scene() && self.seats[0].control_world;
        let postgame = result_frames::aral_lost_crystal(self.legacy_aral(), &self.world);
        let legacy_aral = self.legacy_aral();
        let facing_targets = if legacy_aral {
            self.ringo_attacks.observe(events, &self.world)
        } else {
            [None; aral::SEATS]
        };
        let control_stream = legacy_aral || native_solo;
        let born: HashSet<u32> = if control_stream {
            events
                .iter()
                .filter_map(|event| match event {
                    Event::Spawn { id } => Some(*id),
                    _ => None,
                })
                .collect()
        } else {
            HashSet::new()
        };
        let frame = ReplicationFrame {
            events,
            fallen,
            native_solo,
            postgame,
            legacy_aral,
            facing_targets,
            control_stream,
            born,
        };
        for (team, camps) in team_camps.iter().enumerate() {
            self.replicate_team(team, &frame, camps);
        }
        if self.solo_scene() || fold_blitz::is_managed(self.known_queue) {
            for ev in events {
                if let Event::Death { id, .. } = *ev {
                    self.solo_slots.released(id);
                }
            }
        }
    }

    fn replicate_team(&mut self, team: usize, frame: &ReplicationFrame<'_>, team_camps: &Batch) {
        let has_peers = self
            .seats
            .iter()
            .zip(&self.world.entities)
            .any(|(seat, actor)| usize::from(actor.team) == team && seat.gameplay_peer().is_some());
        if !has_peers {
            return;
        }
        let control_seats: Vec<usize> = if frame.control_stream {
            self.seats
                .iter()
                .zip(&self.world.entities)
                .enumerate()
                .filter(|(_, (seat, actor))| {
                    usize::from(actor.team) == team
                        && seat.gameplay_peer().is_some()
                        && seat.peers[1].is_none()
                        && seat.preamble.confirmed()
                })
                .map(|(i, _)| i)
                .collect()
        } else {
            Vec::new()
        };
        let mut out = vec![];
        let mut per_seat_decls: Vec<(usize, Batch)> = control_seats
            .iter()
            .map(|&seat| (seat, Vec::new()))
            .collect();
        let mut newly_declared = HashSet::new();
        let mut guard_deaths = Vec::new();
        // Compute new team visibility into `self.next_visible[team]` without aliasing `self`.
        let mut current = std::mem::take(&mut self.next_visible[team]);
        current.clear();
        for idx in 0..self.world.entities.len() {
            let (id, hp, is_visible) = {
                let e = &self.world.entities[idx];
                (e.id, e.hp, e.hp > 0. && self.world.visible(team as u8, e))
            };
            if !is_visible || hp <= 0. {
                continue;
            }
            if !control_seats.is_empty() {
                let missing = control_seats
                    .iter()
                    .filter(|&&seat| !self.scene_views[seat].knows(id))
                    .count();
                if missing == 0 {
                    current.insert(id);
                    continue;
                }
                let Some(message) = self.allocate_entity_declaration(
                    idx,
                    team as u8,
                    frame.native_solo,
                    frame.legacy_aral,
                    frame.born.contains(&id),
                ) else {
                    continue;
                };
                current.insert(id);
                let e = &self.world.entities[idx];
                if missing == control_seats.len() {
                    newly_declared.insert(id);
                    out.push(message);
                    messages::append_inventory(&mut out, &self.world, e);
                } else {
                    for (seat, decls) in &mut per_seat_decls {
                        if !self.scene_views[*seat].knows(id) {
                            decls.push(message.clone());
                            messages::append_inventory(decls, &self.world, e);
                            if uses_visibility_toggle(team as u8, e)
                                && self.visible[team].contains(&id)
                            {
                                decls.push(actor_visibility_message(id, team as u8, true));
                            }
                        }
                    }
                }
            } else {
                if !self.visible[team].contains(&id) {
                    let Some(message) = self.allocate_entity_declaration(
                        idx,
                        team as u8,
                        frame.native_solo,
                        frame.legacy_aral,
                        frame.born.contains(&id),
                    ) else {
                        continue;
                    };
                    newly_declared.insert(id);
                    out.push(message);
                    messages::append_inventory(&mut out, &self.world, &self.world.entities[idx]);
                }
                current.insert(id);
            }
        }
        self.solo_structure_snapshot.append(&mut out);
        for (seat, decls) in &mut per_seat_decls {
            if !decls.is_empty() {
                self.solo_structure_snapshot.append(decls);
                self.scene_views[*seat].declarations(decls);
            }
        }
        if !out.is_empty() {
            for &seat in &control_seats {
                self.scene_views[seat].declarations(&out);
            }
        }
        if frame.native_solo
            && usize::from(self.world.entities[0].team) == team
            && self.seats[0].world_actor_written
            && self.seats[0].preamble.confirmed()
            && self.seats[0].peers[1].is_none()
        {
            self.scene_views[0].append_solo_bot_paths(
                &self.world,
                &self.world.entities[0],
                &current,
                frame.events,
                &mut out,
            );
        }
        let previous = &mut self.visible[team];
        let viewer_team = team as u8;
        for e in &self.world.entities {
            if !uses_visibility_toggle(viewer_team, e) {
                continue;
            }
            let was_visible = previous.contains(&e.id);
            let now_visible = current.contains(&e.id);
            if !was_visible && now_visible {
                out.push(actor_visibility_message(e.id, viewer_team, true));
                if !newly_declared.contains(&e.id)
                    && !frame
                        .events
                        .iter()
                        .any(|ev| matches!(ev, Event::Move { id, .. } if *id == e.id))
                {
                    out.push(Message::position(wire_id(e.id), e.position.x, e.position.y));
                }
            } else if was_visible && !now_visible && e.hp > 0.0 && !frame.fallen.contains(&e.id) {
                out.push(actor_visibility_message(e.id, viewer_team, false));
            }
        }
        let mut has_attack_release = false;
        for ev in frame.events {
            if frame.legacy_aral
                && ringo::append(&mut out, ev, |id| {
                    current.contains(&id) || frame.fallen.contains(&id) && previous.contains(&id)
                })
            {
                has_attack_release = true;
                continue;
            }
            if frame.control_stream
                && let Some(m) = npc_attack::release(&self.world, ev, |id| {
                    current.contains(&id) || frame.fallen.contains(&id) && previous.contains(&id)
                })
            {
                out.push(m);
                has_attack_release = true;
                continue;
            }
            result_frames::append_postgame(&mut out, ev, frame.postgame);
            if hero_progress::append(&self.world, &mut out, ev, |id| {
                !newly_declared.contains(&id)
                    && (current.contains(&id)
                        || (frame.fallen.contains(&id) && previous.contains(&id)))
            }) {
                continue;
            }
            if let Event::Purchase(receipt) = ev {
                if frame.legacy_aral && newly_declared.contains(&receipt.actor) {
                    continue;
                }
                if current.contains(&receipt.actor)
                    || frame.fallen.contains(&receipt.actor) && previous.contains(&receipt.actor)
                {
                    messages::append_purchase(
                        &mut out,
                        &self.world,
                        receipt,
                        !newly_declared.contains(&receipt.actor),
                    );
                }
                continue;
            }
            if let Event::GoldAward { id, amount } = ev {
                if frame.legacy_aral && newly_declared.contains(id) {
                    continue;
                }
                if current.contains(id) || frame.fallen.contains(id) && previous.contains(id) {
                    out.push(Message::entity_stat(wire_id(*id), *amount as f32, 6));
                }
                continue;
            }
            if let Event::Move { id, position } = ev
                && current.contains(id)
            {
                out.push(Message::position(wire_id(*id), position.x, position.y));
            } else {
                let before = out.len();
                messages::append_combat(&mut out, ev, |actor| {
                    current.contains(&actor)
                        || (frame.fallen.contains(&actor) && previous.contains(&actor))
                });
                if frame.legacy_aral
                    && let Event::Death { id, .. } = ev
                {
                    let death_sent = out.len() != before;
                    if death_sent {
                        guard_deaths.extend(self.world.aral_structure_slot(*id));
                        aral::death::witness(
                            &mut self.dead_actors,
                            *id,
                            team as u8,
                            &self.seats,
                            &self.world,
                        );
                    }
                    let known_ally = previous.contains(id)
                        && self.world.entities.iter().any(|actor| {
                            actor.id == *id
                                && actor.kind == halcyon_sim::Kind::Hero
                                && usize::from(actor.team) == team
                        });
                    if death_sent || known_ally {
                        messages::append_respawn_timer(&mut out, &self.world, *id);
                    }
                }
            }
        }
        *previous = current;
        self.next_visible[team] = HashSet::new();
        if frame.control_stream {
            if self.world.tick.is_multiple_of(10 * halcyon_sim::TICK_HZ) {
                out.push(messages::world_clock(self.world.tick));
            }
        } else if self.world.tick.is_multiple_of(20) {
            out.push(snapshot(&self.world, &self.players, &self.picks));
        }
        out.extend(team_camps.iter().cloned());
        for (seat, decls) in per_seat_decls {
            if !decls.is_empty()
                && let Some(peer) = self.seats[seat].gameplay_peer()
            {
                peer.send(transport::Delivery::Private(decls));
            }
        }
        if out.is_empty() {
            return;
        }
        let mut peers = self
            .seats
            .iter()
            .zip(&self.world.entities)
            .filter(|(_, actor)| usize::from(actor.team) == team)
            .filter_map(|(seat, _)| seat.gameplay_peer())
            .peekable();
        let Some(first) = peers.next() else {
            return;
        };
        if has_attack_release {
            npc_attack::deliver(
                out,
                &self.seats,
                &self.world,
                &mut self.scene_views,
                team,
                &frame.facing_targets,
            );
        } else if peers.peek().is_none() {
            first.send(transport::Delivery::Private(out));
        } else {
            let batch = transport::TeamBatch::new(out);
            for peer in std::iter::once(first).chain(peers) {
                peer.send(transport::Delivery::Team(batch.clone()));
            }
        }
        if !guard_deaths.is_empty() {
            for ((seat, hero), view) in self
                .seats
                .iter()
                .zip(&self.world.entities)
                .zip(&mut self.scene_views)
            {
                if usize::from(hero.team) != team
                    || !seat.world_actor_written
                    || !seat.preamble.confirmed()
                    || seat.peers[1].is_some()
                {
                    continue;
                }
                let Some(peer) = seat.gameplay_peer() else {
                    continue;
                };
                let mut updates = Batch::new();
                for &slot in &guard_deaths {
                    view.guard_death(slot, &mut updates);
                }
                if !updates.is_empty() {
                    peer.send(transport::Delivery::Private(updates));
                }
            }
        }
    }
}
