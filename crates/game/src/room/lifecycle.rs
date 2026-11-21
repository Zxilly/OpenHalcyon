use super::actions::surrender;
use super::state::{Peer, Room, Seat, Sink};
use crate::host::admin_views::{Channel, EventDetail, EventKind, RoomPhase};
use crate::modes::{aral, fold_blitz, scene};
use crate::replication::{
    self,
    messages::{self, setup, snapshot, world_header},
    result_frames,
    slots::{actor_slots, point_move},
};
use crate::transport::{self, reconnect};
use chrono::Utc;
use halcyon_protocol::Message;
use std::time::Instant;
use tokio::sync::watch;
use uuid::Uuid;

impl Room {
    pub(super) fn solo_loading(&mut self) {
        if self.phase != RoomPhase::Loading
            || self.phase_start.elapsed() < std::time::Duration::from_secs(7)
            || self.seats[0].loading_roster_sent
            || self.seats[0].peers[1].is_some()
        {
            return;
        }
        if let Some(peer) = &self.seats[0].peers[0] {
            peer.send(transport::Delivery::Private(scene::begin_loading(self)));
            self.seats[0].loading_roster_sent = true;
        }
    }

    pub(crate) fn aral_remaining(&self) -> f32 {
        (aral::DRAFT_SECONDS - self.phase_start.elapsed().as_secs_f32()).max(0.)
    }

    pub(super) fn start_control_world(&mut self) {
        if self.phase != RoomPhase::Loading
            || !self
                .seats
                .iter()
                .all(|seat| seat.preamble.confirmed() && seat.peers[0].is_some())
        {
            return;
        }
        // Every control stream has completed the same pristine ARAL opening.
        // Mark only currently visible actors to retain normal team visibility.
        for (team, visible) in self.visible.iter_mut().enumerate() {
            visible.extend(self.world.entities.iter().filter_map(|actor| {
                (actor.hp > 0.0 && self.world.visible(team as u8, actor)).then_some(actor.id)
            }));
        }
        let mut records = self.recommended_paths(None);
        records.push(halcyon_protocol::StartMatchBody.encode());
        let marker = transport::TeamBatch::new(records);
        for peer in self.seats.iter().filter_map(Seat::gameplay_peer) {
            peer.send(transport::Delivery::Team(marker.clone()));
        }
        self.phase(RoomPhase::Running);
    }

    pub(super) fn aral_progress(&mut self) {
        if !self.seats.iter().all(|seat| seat.peers[0].is_some()) {
            return;
        }
        match self.phase {
            RoomPhase::HeroSelect if self.aral_remaining() <= 0. => {
                for (seat, pick) in self.seats.iter_mut().zip(&mut self.picks) {
                    seat.locked = true;
                    pick.selected = true;
                    pick.locked = true;
                }
                let mut opening = vec![aral::roster(self, 0., false)];
                opening.extend(scene::begin_loading(self));
                let opening = transport::TeamBatch::new(opening);
                for peer in self.seats.iter().filter_map(|seat| seat.peers[0].as_ref()) {
                    peer.send(transport::Delivery::Team(opening.clone()));
                }
                self.phase(RoomPhase::Loading);
            }
            RoomPhase::Loading => self.start_control_world(),
            _ => {}
        }
    }

    pub(crate) fn attach(
        &mut self,
        uuid: Uuid,
        tx: Sink,
        stalled: watch::Sender<bool>,
    ) -> Option<(usize, usize, u64, replication::Batch)> {
        let idx = self.players.iter().position(|p| *p == uuid)?;
        if matches!(self.phase, RoomPhase::Ending | RoomPhase::Ended) || self.events.len() >= 4096 {
            return None;
        }
        // A locked seat can open its data connection while other humans are still selecting.
        let legacy_aral = self.legacy_aral();
        let game = !legacy_aral
            && !self.seats[idx].control_world
            && self.seats[idx].locked
            && self.seats[idx].peers[1].is_none()
            && (self.seats[idx].peers[0].is_some() || self.phase == RoomPhase::Running);
        let ch = usize::from(game);
        if let Some(existing) = &self.seats[idx].peers[ch] {
            if ch == 0 && (*existing.stalled.borrow() || existing.tx.is_closed()) {
                let generation = existing.generation;
                self.detach(idx, ch, generation);
            } else {
                return None;
            }
        }
        self.last_active = Instant::now();
        self.generation += 1;
        if game {
            let _ = fold_blitz::prepare_world(self);
            // The reconnect snapshot represents precisely this simulation tick.
            let team = self.world.entities[idx].team;
            let visible = &mut self.visible[usize::from(team)];
            visible.clear();
            visible.extend(
                self.world
                    .entities
                    .iter()
                    .filter(|e| e.hp > 0. && self.world.visible(team, e))
                    .map(|e| e.id),
            );
        } else {
            self.seats[idx].loading_roster_sent = false;
            self.seats[idx].scene_written = false;
            self.seats[idx].world_actor_written = false;
            self.seats[idx].aral_reorder_view = None;
            self.scene_views[idx] = Default::default();
            self.seats[idx].draft_sync_sent = false;
            self.seats[idx].preamble = Default::default();
        }
        self.seats[idx].seen[ch].clear();
        self.seats[idx].peers[ch] = Some(Peer {
            generation: self.generation,
            tx,
            stalled,
        });
        let first_registration = self.seats[idx].last.is_none();
        self.seats[idx].last = Some(Utc::now());
        if self.phase == RoomPhase::Allocated {
            self.phase(RoomPhase::HeroSelect);
        }
        self.event(
            EventKind::Connection,
            "Authenticated",
            EventDetail::Connection {
                player_id: uuid,
                channel: Channel::for_game(game),
                generation: Some(self.generation),
                opcode: None,
            },
        );
        let solo_rejoin = reconnect::is_solo_rejoin(self, idx, game);
        let mut batch = if solo_rejoin {
            // A fresh control stream has never received the seat and world mode.
            self.seats[idx].loading_roster_sent = true;
            reconnect::solo_rejoin_batch(self, idx)
        } else if legacy_aral {
            aral::setup(
                self,
                idx,
                if self.phase == RoomPhase::Running {
                    0.
                } else {
                    self.aral_remaining()
                },
            )
        } else if game {
            let mut header =
                world_header(&self.world, &self.players, &self.picks, &self.queue, idx);
            reconnect::populate_world_header(
                &mut header,
                &self.world.entities[idx],
                &self.picks[idx],
            );
            header
        } else {
            let mut out = setup(
                &self.world,
                &self.players,
                &self.catalog,
                idx,
                &self.queue,
                &self.picks,
            );
            if let Some(d) = self.draft_5v5.as_ref().filter(|d| d.is_ranked()) {
                out.push(d.ranked_packet(&self.world, &self.picks).encode());
            }
            out
        };
        if !game
            && !self.solo_scene()
            && matches!(self.phase, RoomPhase::Loading | RoomPhase::Running)
            && (legacy_aral || self.draft_5v5.is_some())
        {
            self.seats[idx].loading_roster_sent = true;
            batch.extend(scene::begin_loading(self));
        }
        if game {
            let team = self.world.entities[idx].team;
            if fold_blitz::is_managed(self.known_queue) {
                fold_blitz::append_opening(self, Some(team), false, &mut batch);
                for e in &self.world.entities {
                    if e.hp > 0.0 && self.world.visible(team, e) {
                        self.visible[usize::from(team)].insert(e.id);
                        if replication::uses_visibility_toggle(team, e) {
                            batch.push(replication::actor_visibility_message(e.id, team, true));
                        }
                    }
                }
            } else {
                batch.extend(self.spawn_view(Some(team)));
            }
            let ready_for_5v5 =
                self.world.is_5v5_enabled()
                    && self.phase == RoomPhase::Loading
                    && self.seats.iter().enumerate().all(|(seat_idx, s)| {
                        self.world.entities[seat_idx].bot || s.peers[1].is_some()
                    });
            if self.seats.iter().all(|s| s.peers[1].is_some()) || ready_for_5v5 {
                self.phase(RoomPhase::Running);
            }
        }
        if legacy_aral && first_registration && self.phase == RoomPhase::HeroSelect {
            scene::announce_aral_registration(self, idx);
        }
        Some((idx, ch, self.generation, batch))
    }

    pub(crate) fn spawn_view(&self, team: Option<u8>) -> replication::Batch {
        if fold_blitz::is_managed(self.known_queue) {
            let mut out = Vec::new();
            fold_blitz::append_opening_snapshot(self, team, false, &mut out);
            return out;
        }
        let mut out = vec![];
        for e in &self.world.entities {
            if e.hp <= 0. || team.is_some_and(|t| !self.world.visible(t, e)) {
                continue;
            }
            out.push(point_move::full_spawn(
                &self.world,
                e,
                &self.picks,
                self.data.as_deref(),
                self.legacy_aral(),
            ));
            messages::append_inventory(&mut out, &self.world, e);
            if let Some(viewer_team) = team
                && replication::uses_visibility_toggle(viewer_team, e)
            {
                out.push(replication::actor_visibility_message(
                    e.id,
                    viewer_team,
                    true,
                ));
            }
        }
        out.extend(self.recommended_paths(team));
        out.extend(aral::talent_snapshot(self));
        messages::append_5v5_camp_markers(&mut out, &self.world, team.map_or(0, |t| t + 1));
        out
    }

    pub(crate) fn detach(&mut self, i: usize, ch: usize, generation: u64) {
        if self.seats[i].peers[ch]
            .as_ref()
            .is_some_and(|p| p.generation == generation)
        {
            self.seats[i].peers[ch] = None;
            self.world_disconnected(i, ch);
            // The empty-room grace starts when the last current connection leaves.
            // A connected player may have received updates without sending input.
            self.last_active = Instant::now();
            if ch == 0 {
                self.seats[i].aral_reorder_view = None;
            }
            self.seats[i].last = Some(Utc::now());
            self.event(
                EventKind::Connection,
                "Disconnected",
                EventDetail::Connection {
                    player_id: self.players[i],
                    channel: Channel::for_game(ch != 0),
                    generation: Some(generation),
                    opcode: None,
                },
            );
        }
    }

    pub(crate) fn roster_update(&self, except: Option<usize>) -> Message {
        self.roster_with_echo(except, None)
    }

    pub(crate) fn roster_with_echo(
        &self,
        except: Option<usize>,
        echo: Option<&Message>,
    ) -> Message {
        let legacy = self.legacy_aral();
        let roster = if legacy {
            aral::roster(self, self.aral_remaining(), false)
        } else if self.phase == RoomPhase::HeroSelect {
            messages::snapshot_with_timers(
                &self.world,
                &self.players,
                &self.picks,
                (300. - self.phase_start.elapsed().as_secs_f32()).max(0.),
                300.,
            )
        } else if self.phase == RoomPhase::Loading {
            messages::snapshot_with_timers(
                &self.world,
                &self.players,
                &self.picks,
                (7. - self.phase_start.elapsed().as_secs_f32()).max(0.),
                7.,
            )
        } else {
            snapshot(&self.world, &self.players, &self.picks)
        };
        let ranked_045b = (!legacy)
            .then(|| {
                self.draft_5v5
                    .as_ref()
                    .filter(|d| d.is_ranked())
                    .map(|d| d.ranked_packet(&self.world, &self.picks).encode())
            })
            .flatten();
        for (i, s) in self.seats.iter().enumerate() {
            if except != Some(i)
                && let Some(p) = &s.peers[0]
            {
                let mut batch = vec![roster.clone()];
                if let Some(pkt) = &ranked_045b {
                    batch.push(pkt.clone());
                }
                batch.extend(echo.cloned());
                p.send(transport::Delivery::Private(batch));
            }
        }
        roster
    }

    pub(super) fn enter_5v5_loading(&mut self, actor_seat: usize, out: &mut Vec<Message>) {
        if !self.world.is_5v5_enabled()
            && !self.solo_scene()
            && !self.legacy_aral()
            && !halcyon_data::three_per_side(&self.queue)
            && self.world.tick == 0
        {
            if let Err(err) = self.world.use_sovereigns_rise_5v5(self.data.as_deref()) {
                tracing::warn!(%err, "failed to install 5v5 Sovereign's Rise state");
            } else {
                self.solo_slots = actor_slots::ActorSlots::from_roster(&self.world.entities);
            }
        }
        self.phase(RoomPhase::Loading);
        let loading_batch = scene::begin_loading(self);
        for (idx, seat) in self.seats.iter_mut().enumerate() {
            if seat.peers[0].is_some() {
                seat.loading_roster_sent = true;
            }
            if idx != actor_seat
                && let Some(peer) = &seat.peers[0]
            {
                peer.send(transport::Delivery::Private(loading_batch.clone()));
            }
        }
        out.extend(loading_batch);
    }

    pub(super) fn validated_skin(&self, hero: &str, requested: u32) -> u32 {
        if let Some(skin) = self
            .data
            .as_ref()
            .and_then(|data| data.indexed_skin(hero, Some(requested)))
        {
            return skin;
        }
        let default = halcyon_data::skins::default_skin_hash(hero);
        if requested == default {
            return default;
        }
        let prefix = format!("{hero}_");
        let valid = self.data.as_ref().is_some_and(|data| {
            data.skin_names.iter().any(|name| {
                name.starts_with(&prefix) && halcyon_protocol::skin_hash(name) == requested
            })
        });
        if valid { requested } else { default }
    }

    pub(crate) fn finish_world(&mut self) {
        if *self.stop.borrow() {
            return;
        }
        let result = match self.surrender.iter().position(surrender::Ballot::passed) {
            Some(loser) => Message::aral_surrender_result(2 - loser as u8).ok(),
            None => result_frames::crystal_end(&self.queue, &self.world)
                .or_else(|| result_frames::score_or_time_end(&self.queue, &self.world)),
        };
        if let Some(result) = result {
            let mut final_batch = Vec::with_capacity(2);
            match result_frames::statistics_snapshot(&self.world) {
                Ok(report) => final_batch.push(report),
                Err(error) => {
                    tracing::error!(room=%self.id, %error, "cannot encode final match statistics");
                }
            }
            final_batch.push(result);
            self.publish_observers(&final_batch);
            let delivery = transport::TeamBatch::new(final_batch);
            for peer in self.seats.iter().filter_map(Seat::gameplay_peer) {
                peer.send(transport::Delivery::Team(delivery.clone()));
            }
        }
        self.stop.send_replace(true);
    }
}
