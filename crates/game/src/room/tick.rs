use super::actions::surrender;
use super::state::Room;
use crate::abilities;
use crate::host::admin_views::{RoomEndReason, RoomPhase};
use crate::modes::{aral, fold_blitz, scene};
use crate::replication::npc_projectile;
use crate::transport;
use halcyon_protocol::ce::{KnownQueue, NativeMode};
use halcyon_sim::Event;
use std::time::{Duration, Instant};

impl Room {
    pub(crate) fn tick(&mut self) {
        if !self.admitted {
            return;
        }
        if self.phase == RoomPhase::Ending && self.reason == Some(RoomEndReason::Completed) {
            self.advance_ending();
            return;
        }
        if self.last_active.elapsed().as_secs() > 300
            && self
                .seats
                .iter()
                .all(|s| s.peers.iter().all(Option::is_none))
        {
            self.reason = Some(RoomEndReason::Error);
            self.phase(RoomPhase::Ending);
            self.stop.send_replace(true);
            return;
        }
        let legacy = self.legacy_aral();
        let roster_interval = if legacy {
            Duration::from_millis(100)
        } else {
            Duration::from_secs(1)
        };
        if self.solo_scene() && self.phase == RoomPhase::Loading {
            self.solo_loading();
        }
        if (self.phase == RoomPhase::HeroSelect
            || !legacy && self.phase == RoomPhase::Loading && !self.seats[0].loading_roster_sent)
            && self.last_roster.elapsed() >= roster_interval
        {
            self.last_roster = Instant::now();
            self.roster_update(None);
        }

        if legacy && matches!(self.phase, RoomPhase::HeroSelect | RoomPhase::Loading) {
            self.aral_progress();
            return;
        }

        // The world advances once the selected transport has confirmed admission.
        if self.phase != RoomPhase::Running {
            return;
        }
        if self.world.tick == 0 && self.solo_scene() && self.seats[0].control_world {
            self.solo_structure_snapshot.start();
            scene::initial_structures(self);
        }
        let events = self.world.step();
        fold_blitz::publish_tick(self, &events);
        let (observer_camps, team_camps) = self.drain_5v5_camp_messages();
        self.solo_structure_snapshot.observe(&events);
        if legacy && self.world.winner.is_none() {
            self.advance_absences();
        }
        aral::death::flush(self, self.world.tick);
        aral::death::schedule(self, &events);
        if legacy {
            aral::death::publish_approaches(self, &events);
        }
        self.publish_aral_items(&events);
        self.tick_observers(&events);
        if !observer_camps.is_empty() {
            self.publish_observers(&observer_camps);
        }
        self.record_combat_metrics(&events);
        abilities::publish_before_impacts(self, &events);
        self.replicate_with_camps(&events, &team_camps);
        if legacy || self.solo_scene() {
            npc_projectile::publish(self, &events);
        }
        if legacy {
            aral::announcements::publish(self, &events);
        }
        scene::sync_5v5_shops(self);
        fold_blitz::sync_shops(self);
        if legacy || self.solo_scene() {
            self.flush_scene_views();
        }
        abilities::publish(self, &events);
        if self.world.winner.is_none() {
            self.advance_ballots();
        }
        if self.world.winner.is_some() {
            self.reason = Some(RoomEndReason::Completed);
            self.phase(RoomPhase::Ending);
            if self.known_queue.map(KnownQueue::mode) != Some(NativeMode::Aral) {
                self.finish_world();
            }
        }
    }

    fn advance_ending(&mut self) {
        if !self.dead_actors.is_empty() {
            let elapsed =
                self.phase_start.elapsed().as_millis() * u128::from(halcyon_sim::TICK_HZ) / 1000;
            aral::death::flush(self, self.world.tick.saturating_add(elapsed as u64));
        }
        let delay = if self.surrender.iter().any(surrender::Ballot::passed) {
            15
        } else {
            6
        };
        if self.phase_start.elapsed() >= Duration::from_secs(delay) {
            self.finish_world();
        }
    }

    fn record_combat_metrics(&mut self, events: &[Event]) {
        for event in events {
            match *event {
                Event::Death { id, killer } if (1..=10).contains(&id) => {
                    self.combat[(id - 1) as usize].deaths += 1;
                    if (1..=10).contains(&killer) {
                        self.combat[(killer - 1) as usize].kills += 1;
                    }
                }
                Event::Damage { source, amount, .. } if (1..=10).contains(&source) => {
                    self.combat[(source - 1) as usize].damage += f64::from(amount);
                }
                Event::Heal { source, amount, .. } if (1..=10).contains(&source) => {
                    self.combat[(source - 1) as usize].healing += f64::from(amount);
                }
                _ => {}
            }
        }
    }

    fn flush_scene_views(&mut self) {
        for (i, (seat, actor)) in self.seats.iter().zip(&self.world.entities).enumerate() {
            if !seat.world_actor_written || !seat.preamble.confirmed() || seat.peers[1].is_some() {
                continue;
            }
            if let Some(peer) = &seat.peers[0] {
                let mut output = Vec::new();
                let visible = &self.visible[usize::from(actor.team)];
                self.scene_views[i].append(&self.world, |id| visible.contains(&id), &mut output);
                if !output.is_empty() {
                    peer.send(transport::Delivery::Private(output));
                }
            }
        }
    }
}
