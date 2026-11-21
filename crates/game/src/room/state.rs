use super::actions::{player_actions, player_presence, surrender};
use super::tick_latency::TickLatency;
use crate::abilities;
use crate::abilities::legacy::ringo;
use crate::host::admin_views::{
    self, EventDetail, EventKind, RoomEndReason, RoomParticipant, RoomPhase, RoomSummary,
};
use crate::host::manager::now;
use crate::modes::{aral, draft_5v5, draft_preferences, fold_blitz, scene};
use crate::replication::slots::{actor_slots, point_move, tower_aggro};
use crate::replication::structure_effects;
use crate::transport::{self, observer};
use chrono::{DateTime, Utc};
use halcyon_data::GameData;
use halcyon_protocol::ce::KnownQueue;
use halcyon_sim::World;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, atomic::AtomicU64},
    time::Instant,
};
use tokio::sync::{mpsc, watch};
use uuid::Uuid;

pub(crate) type Sink = mpsc::Sender<transport::Delivery>;

pub(crate) struct Peer {
    pub(crate) generation: u64,
    pub(crate) tx: Sink,
    pub(crate) stalled: watch::Sender<bool>,
}

impl Peer {
    pub(crate) fn send(&self, batch: transport::Delivery) {
        if self.tx.try_send(batch).is_err() {
            self.stalled.send_replace(true);
        }
    }
}

#[derive(Default)]
pub(crate) struct Seat {
    pub(crate) last_hud: Option<Instant>,
    pub(crate) used_aral_reroll: bool,
    pub(crate) presence: player_presence::Presence,
    pub(crate) action_receipts: player_actions::Receipts,
    pub(crate) locked: bool,
    pub(crate) draft_sync_sent: bool,
    pub(crate) recommended_path: Option<halcyon_protocol::RecommendedBuildPath>,
    pub(crate) equipped_talent: Option<halcyon_protocol::EquippedTalent>,
    pub(crate) aral_reorder_view: Option<aral::inventory::ProjectedSlots>,
    pub(crate) preamble: scene::Preamble,
    pub(crate) loading_roster_sent: bool,
    pub(crate) control_world: bool,
    pub(crate) scene_written: bool,
    pub(crate) world_actor_written: bool,
    pub(crate) native_abilities: abilities::NativeAbilities,
    pub(crate) peers: [Option<Peer>; 2],
    pub(crate) last: Option<DateTime<Utc>>,
    pub(crate) seen: [HashSet<u16>; 2],
}

impl Seat {
    pub(crate) fn gameplay_peer(&self) -> Option<&Peer> {
        self.peers[1]
            .as_ref()
            .or_else(|| self.peers[0].as_ref().filter(|_| self.preamble.confirmed()))
    }
}

pub(crate) fn authorize_talent_level(
    room: &Room,
    seat: usize,
    hero_type_id: u16,
    selection: halcyon_protocol::TalentSelection,
) -> Option<u8> {
    use halcyon_protocol::TalentSelection;
    if selection == TalentSelection::NoTalent {
        return Some(0);
    }
    if let Some(level) = room
        .talent_levels
        .get(seat)
        .and_then(|levels| levels.get(&hero_type_id))
    {
        return Some(level.get());
    }
    room.unlock_all_talents.then_some(match selection {
        TalentSelection::Slot0 | TalentSelection::Slot1 => 10,
        TalentSelection::Slot2 => 5,
        TalentSelection::NoTalent => 0,
    })
}

pub(crate) struct Pick {
    pub(crate) preferences: draft_preferences::Preferences,
    pub(crate) name: String,
    pub(crate) skin: u32,
    pub(crate) selected: bool,
    pub(crate) locked: bool,
}

#[derive(Clone, Copy, Default, serde::Serialize)]
pub(crate) struct Combat {
    pub(crate) kills: u32,
    pub(crate) deaths: u32,
    pub(crate) damage: f64,
    pub(crate) healing: f64,
}

#[derive(PartialEq, Eq)]
pub(crate) struct FederationOrigin {
    pub(crate) pool: Uuid,
    pub(crate) homes: Vec<Uuid>,
}

pub(crate) struct Room {
    pub(crate) free_aral_reroll: bool,
    pub(crate) unlock_all_talents: bool,
    pub(crate) federation: Option<FederationOrigin>,
    pub(crate) draft_5v5: Option<draft_5v5::Draft5v5>,
    pub(crate) scene_views: Vec<tower_aggro::View>,
    pub(crate) ringo_attacks: ringo::AttackHistory,
    pub(crate) solo_slots: actor_slots::ActorSlots,
    pub(crate) solo_structure_snapshot: structure_effects::SoloSnapshot,
    pub(crate) aral_slots: aral::spawn::WaveSlots,
    pub(crate) dead_actors: Vec<aral::death::Pending>,
    pub(crate) aral_announcements: aral::announcements::Announcements,
    pub(crate) surrender: [surrender::Ballot; 2],
    pub(crate) visible: [HashSet<u32>; 2],
    pub(crate) next_visible: [HashSet<u32>; 2],
    pub(crate) replay_bytes: Arc<AtomicU64>,
    pub(crate) observers: HashMap<Uuid, observer::Observer>,
    pub(crate) latency: TickLatency,
    pub(crate) admitted: bool,
    pub(crate) admission_ready: watch::Sender<bool>,
    pub(crate) data: Option<Arc<GameData>>,
    pub(crate) picks: Vec<Pick>,
    pub(crate) id: Uuid,
    pub(crate) address: String,
    pub(crate) queue: String,
    pub(crate) known_queue: Option<KnownQueue>,
    pub(crate) difficulty: Option<halcyon_data::BotDifficulty>,
    pub(crate) players: Vec<Uuid>,
    pub(crate) requested_teams: Vec<u8>,
    pub(crate) talent_levels: Vec<halcyon_data::TalentLevels>,
    pub(crate) combat: [Combat; 10],
    pub(crate) catalog: Arc<[[u8; 32]]>,
    pub(crate) seats: Vec<Seat>,
    pub(crate) world: World,
    pub(crate) phase: RoomPhase,
    pub(crate) created: DateTime<Utc>,
    pub(crate) since: DateTime<Utc>,
    pub(crate) ended: Option<DateTime<Utc>>,
    pub(crate) reason: Option<RoomEndReason>,
    pub(crate) termination: Option<admin_views::Termination>,
    pub(crate) events: Vec<admin_views::RoomEvent>,
    pub(crate) stop: watch::Sender<bool>,
    pub(crate) settled: watch::Sender<bool>,
    pub(crate) generation: u64,
    pub(crate) last_active: Instant,
    pub(crate) phase_start: Instant,
    pub(crate) last_roster: Instant,
}

impl Room {
    pub(crate) fn event(&mut self, kind: EventKind, message: &str, detail: EventDetail) {
        match &detail {
            EventDetail::Connection {
                channel,
                generation,
                opcode,
                ..
            } => {
                tracing::debug!(room=%self.id,?kind,message,?channel,generation,?opcode,"room event")
            }
            EventDetail::Action { opcode, .. } => {
                tracing::debug!(room=%self.id,?kind,message,?opcode,"room event")
            }
            _ => tracing::debug!(room=%self.id,?kind,message,"room event"),
        }
        self.events.push(admin_views::RoomEvent {
            seq: self.events.len() + 1,
            kind,
            message: message.into(),
            at: now(),
            detail,
        });
    }

    pub(crate) fn phase(&mut self, phase: RoomPhase) {
        if self.phase != phase {
            self.phase = phase;
            self.phase_start = Instant::now();
            self.since = Utc::now();
            self.event(
                EventKind::PhaseChanged,
                phase.as_str(),
                EventDetail::Phase {},
            );
        }
    }

    pub(crate) fn view(&self, detail: bool) -> RoomSummary {
        let participants = detail.then(|| {
            (0..self.picks.len())
                .map(|slot| {
                    let s = self.seats.get(slot);
                    RoomParticipant {
                        slot,
                        team: self.world.entities[slot].team,
                        player_id: self.players.get(slot).copied(),
                        home_instance_id: self
                            .federation
                            .as_ref()
                            .and_then(|f| f.homes.get(slot))
                            .copied(),
                        bot_profile: s.is_none().then_some("lane-v1"),
                        display_name: if s.is_some() {
                            "Guest".into()
                        } else {
                            format!("{} Bot", self.picks[slot].name)
                        },
                        hero: self.picks[slot].name.clone(),
                        locked: s.is_none_or(|s| s.locked),
                        control_connected: s.is_some_and(|s| s.peers[0].is_some()),
                        game_connected: (!self.legacy_aral() || self.phase == RoomPhase::Running)
                            && s.is_some_and(|s| s.gameplay_peer().is_some()),
                        last_event_at: s.and_then(|s| s.last).map(|x| x.to_rfc3339()),
                    }
                })
                .collect()
        });
        RoomSummary {
            id: self.id,
            queue: self.queue.clone(),
            phase: self.phase,
            created_at: self.created.to_rfc3339(),
            phase_since: self.since.to_rfc3339(),
            ended_at: self.ended.map(|x| x.to_rfc3339()),
            end_reason: self.reason.map(RoomEndReason::as_str),
            human_players: self.players.len(),
            bot_players: self.picks.len() - self.players.len(),
            pool_id: self.federation.as_ref().map(|f| f.pool),
            tick_latency: self.latency.report(),
            difficulty: self.difficulty,
            participants,
        }
    }

    pub(crate) fn legacy_aral(&self) -> bool {
        self.known_queue == Some(KnownQueue::CasualAral)
    }

    pub(crate) fn solo_scene(&self) -> bool {
        self.players.len() == 1 && self.known_queue.is_some_and(KnownQueue::is_single_human)
    }

    pub(crate) fn hero_slot(&self, actor: &halcyon_sim::Entity) -> Option<u8> {
        if self.legacy_aral() || self.known_queue == Some(KnownQueue::BattleRoyaleBotsSolo) {
            point_move::aral_slot(actor)
        } else if fold_blitz::is_managed(self.known_queue) {
            fold_blitz::hero_slot(actor)
        } else {
            self.solo_slots.fixed(actor)
        }
    }
}
