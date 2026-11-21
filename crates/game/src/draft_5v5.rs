//! Authoritative 5v5 Casual and Ranked Ban/Pick/Swap draft state machine.

use super::{Pick, Seat, messages::wire_id};
use halcyon_data::{EntityTemplate, GameData};
use halcyon_protocol::ce::{KnownQueue, NativeMode};
use halcyon_protocol::draft::{
    DraftAction, RankedDraftSeat, RankedDraftStatePacket, RankedWirePhase,
};
use halcyon_sim::{Kind, World};
use std::time::Instant;

const PHASE_DURATION_SECS: f32 = 30.0;

/// Closed distinction between simultaneous 5v5 Casual pick/lock and turn-based Ranked BP/Swap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Draft5v5Mode {
    Casual,
    RankedBp,
}

/// Closed phase state of a 5v5 Ban/Pick/Swap draft.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Draft5v5Phase {
    Ban { turn: u8 },
    Pick { turn: u8 },
    Swap,
    Complete,
}

impl Draft5v5Phase {
    pub(crate) const fn to_wire(self) -> RankedWirePhase {
        match self {
            Self::Ban { .. } => RankedWirePhase::Ban,
            Self::Pick { .. } => RankedWirePhase::Pick,
            Self::Swap => RankedWirePhase::Swap,
            Self::Complete => RankedWirePhase::Complete,
        }
    }

    pub(crate) const fn active_turn_byte(self) -> u8 {
        match self {
            Self::Ban { turn } | Self::Pick { turn } => turn,
            Self::Swap | Self::Complete => 0,
        }
    }
}

/// Closed rejection reasons for invalid 5v5 draft actions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Draft5v5Error {
    WrongPhase,
    WrongTurn,
    AlreadyLocked,
    HeroBanned,
    HeroAlreadyTaken,
    InvalidHero,
    SameTeamRequired,
    SeatNotLocked,
    NoPendingSwap,
    InvalidSwapTarget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PendingSwap {
    pub(crate) requester: usize,
    pub(crate) target: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DraftOutcome {
    pub(crate) state_changed: bool,
    pub(crate) start_loading: bool,
    pub(crate) echo: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Draft5v5 {
    pub(crate) mode: Draft5v5Mode,
    pub(crate) phase: Draft5v5Phase,
    pub(crate) bans: [u32; 4],
    pub(crate) hovered_ban: Option<u32>,
    pub(crate) ban_order: Vec<usize>,
    pub(crate) pick_blocks: Vec<Vec<usize>>,
    pub(crate) pick_order: Vec<usize>,
    pub(crate) pending_swap: Option<PendingSwap>,
    pub(crate) swap_ready: [bool; 10],
    pub(crate) role_selectors: [Option<u8>; 10],
    pub(crate) step_started: Instant,
}

impl Draft5v5 {
    pub(crate) fn for_room(known_queue: Option<KnownQueue>, world: &World) -> Option<Self> {
        let mode = match known_queue.map(KnownQueue::mode) {
            Some(NativeMode::RankedFive | NativeMode::PrivateDraft) => Draft5v5Mode::RankedBp,
            Some(
                NativeMode::CasualFive
                | NativeMode::Private
                | NativeMode::PrivateBots
                | NativeMode::CoopBots
                | NativeMode::Practice,
            ) => Draft5v5Mode::Casual,
            Some(
                NativeMode::Aral
                | NativeMode::RankedFold
                | NativeMode::CasualFold
                | NativeMode::Blitz
                | NativeMode::SoloBots,
            ) => {
                return None;
            }
            None => Draft5v5Mode::Casual,
        };

        let mut teams: [Vec<usize>; 2] = [Vec::new(), Vec::new()];
        for (idx, e) in world
            .entities
            .iter()
            .take(10)
            .enumerate()
            .filter(|(_, e)| e.kind == Kind::Hero)
        {
            if let Some(bucket) = teams.get_mut(usize::from(e.team)) {
                bucket.push(idx);
            }
        }
        if teams[0].is_empty() && teams[1].is_empty() {
            return None;
        }

        // 5v5 Ranked Ban/Pick/Swap schedule:
        // Bans: alternating [Blue captain, Red captain, Blue captain, Red captain] (4 bans).
        // Picks: 1-2-2-2-2-1 snake across team seats.
        let mut ban_order = Vec::with_capacity(4);
        if !teams[0].is_empty() && !teams[1].is_empty() {
            for turn in 0..4 {
                let team = turn % 2;
                ban_order.push(teams[team][0]);
            }
        }

        let pick_blocks = build_snake_blocks(&teams[0], &teams[1]);
        let pick_order: Vec<usize> = pick_blocks.iter().flatten().copied().collect();

        let phase = match mode {
            Draft5v5Mode::RankedBp => {
                if !ban_order.is_empty() {
                    Draft5v5Phase::Ban { turn: 0 }
                } else {
                    Draft5v5Phase::Pick { turn: 0 }
                }
            }
            Draft5v5Mode::Casual => Draft5v5Phase::Pick { turn: 0 },
        };

        Some(Self {
            mode,
            phase,
            bans: [0; 4],
            hovered_ban: None,
            ban_order,
            pick_blocks,
            pick_order,
            pending_swap: None,
            swap_ready: [false; 10],
            role_selectors: [None; 10],
            step_started: Instant::now(),
        })
    }

    pub(crate) fn is_ranked(&self) -> bool {
        self.mode == Draft5v5Mode::RankedBp
    }

    pub(crate) fn active_ban_seat(&self) -> Option<usize> {
        match self.phase {
            Draft5v5Phase::Ban { turn } => self.ban_order.get(usize::from(turn)).copied(),
            _ => None,
        }
    }

    pub(crate) fn active_pick_block(&self) -> &[usize] {
        let Draft5v5Phase::Pick { turn } = self.phase else {
            return &[];
        };
        let mut offset = 0usize;
        for block in &self.pick_blocks {
            let end = offset + block.len();
            if usize::from(turn) < end {
                return block;
            }
            offset = end;
        }
        &[]
    }

    pub(crate) fn is_banned(&self, hero_id: u32) -> bool {
        hero_id != 0 && self.bans.contains(&hero_id)
    }

    pub(crate) fn handle_action(
        &mut self,
        seat: usize,
        action: DraftAction,
        world: &mut World,
        seats: &mut [Seat],
        picks: &mut [Pick],
        data: Option<&GameData>,
    ) -> Result<DraftOutcome, Draft5v5Error> {
        match action {
            DraftAction::SelectSlotOrTeam { slot } => {
                if seat < self.role_selectors.len() {
                    self.role_selectors[seat] = Some(slot);
                }
                Ok(DraftOutcome {
                    state_changed: true,
                    start_loading: false,
                    echo: true,
                })
            }
            DraftAction::SelectRole { selector } => {
                if seat < self.role_selectors.len() {
                    self.role_selectors[seat] = Some(selector);
                }
                Ok(DraftOutcome {
                    state_changed: true,
                    start_loading: false,
                    echo: true,
                })
            }
            DraftAction::SelectPosition { position } => {
                if let Some(pick) = picks.get_mut(seat) {
                    pick.preferences.set_position(position);
                }
                Ok(DraftOutcome {
                    state_changed: true,
                    start_loading: false,
                    echo: true,
                })
            }
            DraftAction::SelectBuildPath { build_path } => {
                if let Some(pick) = picks.get_mut(seat) {
                    pick.preferences.set_build(build_path);
                }
                Ok(DraftOutcome {
                    state_changed: true,
                    start_loading: false,
                    echo: true,
                })
            }
            DraftAction::SelectBan { hero_id } => {
                if self.mode != Draft5v5Mode::RankedBp {
                    return Err(Draft5v5Error::WrongPhase);
                }
                if self.active_ban_seat() != Some(seat) {
                    return Err(Draft5v5Error::WrongTurn);
                }
                if hero_id == 0 {
                    self.hovered_ban = None;
                } else {
                    if self.is_banned(hero_id) {
                        return Err(Draft5v5Error::HeroBanned);
                    }
                    if hero_name_for_template(data, hero_id).is_none() {
                        return Err(Draft5v5Error::InvalidHero);
                    }
                    self.hovered_ban = Some(hero_id);
                }
                Ok(DraftOutcome {
                    state_changed: true,
                    start_loading: false,
                    echo: true,
                })
            }
            DraftAction::ConfirmBan { .. } => {
                let Draft5v5Phase::Ban { turn } = self.phase else {
                    return Err(Draft5v5Error::WrongPhase);
                };
                if self.mode != Draft5v5Mode::RankedBp || self.active_ban_seat() != Some(seat) {
                    return Err(Draft5v5Error::WrongTurn);
                }
                let hero_id = self.hovered_ban.ok_or(Draft5v5Error::InvalidHero)?;
                if self.is_banned(hero_id) {
                    return Err(Draft5v5Error::HeroBanned);
                }
                self.bans[usize::from(turn)] = hero_id;
                self.hovered_ban = None;
                self.advance_ban(turn);
                Ok(DraftOutcome {
                    state_changed: true,
                    start_loading: false,
                    echo: true,
                })
            }
            DraftAction::SkipBan { .. } => {
                let Draft5v5Phase::Ban { turn } = self.phase else {
                    return Err(Draft5v5Error::WrongPhase);
                };
                if self.mode != Draft5v5Mode::RankedBp || self.active_ban_seat() != Some(seat) {
                    return Err(Draft5v5Error::WrongTurn);
                }
                self.bans[usize::from(turn)] = 0;
                self.hovered_ban = None;
                self.advance_ban(turn);
                Ok(DraftOutcome {
                    state_changed: true,
                    start_loading: false,
                    echo: true,
                })
            }
            DraftAction::SelectHero { hero_id, flags } => {
                if !matches!(self.phase, Draft5v5Phase::Pick { .. }) {
                    return Err(Draft5v5Error::WrongPhase);
                }
                if seats[seat].locked {
                    return Err(Draft5v5Error::AlreadyLocked);
                }
                if self.mode == Draft5v5Mode::RankedBp {
                    if !self.active_pick_block().contains(&seat) {
                        return Err(Draft5v5Error::WrongTurn);
                    }
                    if self.is_banned(hero_id) {
                        return Err(Draft5v5Error::HeroBanned);
                    }
                    if is_hero_locked_elsewhere(hero_id, seat, world, seats, picks) {
                        return Err(Draft5v5Error::HeroAlreadyTaken);
                    }
                }
                let name =
                    hero_name_for_template(data, hero_id).ok_or(Draft5v5Error::InvalidHero)?;
                apply_hero_selection(seat, &name, hero_id, flags, world, picks, data)?;
                Ok(DraftOutcome {
                    state_changed: true,
                    start_loading: false,
                    echo: true,
                })
            }
            DraftAction::LockHero { .. } => {
                let Draft5v5Phase::Pick { turn } = self.phase else {
                    return Err(Draft5v5Error::WrongPhase);
                };
                if seats[seat].locked {
                    return Ok(DraftOutcome {
                        state_changed: false,
                        start_loading: false,
                        echo: true,
                    });
                }
                if self.mode == Draft5v5Mode::RankedBp {
                    if !self.active_pick_block().contains(&seat) {
                        return Err(Draft5v5Error::WrongTurn);
                    }
                    let template_id = u32::from(world.entities[seat].type_id);
                    if self.is_banned(template_id) {
                        return Err(Draft5v5Error::HeroBanned);
                    }
                    if is_hero_locked_elsewhere(template_id, seat, world, seats, picks) {
                        return Err(Draft5v5Error::HeroAlreadyTaken);
                    }
                }
                seats[seat].locked = true;
                picks[seat].selected = true;
                picks[seat].locked = true;
                let mut start_loading = false;
                if self.mode == Draft5v5Mode::RankedBp {
                    if self
                        .pick_order
                        .iter()
                        .all(|&s| is_seat_locked(s, seats, picks))
                    {
                        self.phase = Draft5v5Phase::Swap;
                        self.step_started = Instant::now();
                    } else {
                        let next_turn = self.next_unlocked_pick_turn(turn, seats, picks);
                        self.phase = Draft5v5Phase::Pick { turn: next_turn };
                        self.step_started = Instant::now();
                    }
                } else if seats.iter().all(|s| s.locked) {
                    self.phase = Draft5v5Phase::Complete;
                    start_loading = true;
                }
                Ok(DraftOutcome {
                    state_changed: true,
                    start_loading,
                    echo: true,
                })
            }
            DraftAction::SwapRequest { first, second } => {
                if self.mode != Draft5v5Mode::RankedBp || self.phase != Draft5v5Phase::Swap {
                    return Err(Draft5v5Error::WrongPhase);
                }
                let target = resolve_swap_partner(seat, first, second, picks.len())
                    .ok_or(Draft5v5Error::InvalidSwapTarget)?;
                if !is_seat_locked(seat, seats, picks) || !is_seat_locked(target, seats, picks) {
                    return Err(Draft5v5Error::SeatNotLocked);
                }
                if world.entities[seat].team != world.entities[target].team {
                    return Err(Draft5v5Error::SameTeamRequired);
                }
                self.pending_swap = Some(PendingSwap {
                    requester: seat,
                    target,
                });
                Ok(DraftOutcome {
                    state_changed: true,
                    start_loading: false,
                    echo: true,
                })
            }
            DraftAction::SwapResponse { first, second } => {
                if self.mode != Draft5v5Mode::RankedBp || self.phase != Draft5v5Phase::Swap {
                    return Err(Draft5v5Error::WrongPhase);
                }
                let swap = self.pending_swap.ok_or(Draft5v5Error::NoPendingSwap)?;
                if seat != swap.target {
                    return Err(Draft5v5Error::WrongTurn);
                }
                let a = resolve_seat(first, seats.len());
                let b = resolve_seat(second, seats.len());
                // Match Android `sub_8e0880`: `[requester, responder]` indicates rejection.
                let rejected = a == Some(swap.requester) && b == Some(swap.target);
                self.pending_swap = None;
                if !rejected {
                    swap_seat_heroes(swap.requester, swap.target, world, picks, data);
                }
                Ok(DraftOutcome {
                    state_changed: true,
                    start_loading: false,
                    echo: true,
                })
            }
            DraftAction::SwapCancel { .. } | DraftAction::SwapDecline { .. } => {
                if self.mode != Draft5v5Mode::RankedBp || self.phase != Draft5v5Phase::Swap {
                    return Err(Draft5v5Error::WrongPhase);
                }
                let swap = self.pending_swap.ok_or(Draft5v5Error::NoPendingSwap)?;
                if seat != swap.requester && seat != swap.target {
                    return Err(Draft5v5Error::WrongTurn);
                }
                self.pending_swap = None;
                Ok(DraftOutcome {
                    state_changed: true,
                    start_loading: false,
                    echo: true,
                })
            }
            DraftAction::ReadyControl { control } => {
                if self.mode != Draft5v5Mode::RankedBp || self.phase != Draft5v5Phase::Swap {
                    return Ok(DraftOutcome {
                        state_changed: false,
                        start_loading: false,
                        echo: true,
                    });
                }
                if seat < self.swap_ready.len() {
                    self.swap_ready[seat] = control != 0;
                }
                let all_humans_ready = (0..seats.len())
                    .filter(|&i| !world.entities[i].bot)
                    .all(|i| self.swap_ready[i]);
                let start_loading = all_humans_ready;
                if start_loading {
                    self.pending_swap = None;
                    self.phase = Draft5v5Phase::Complete;
                }
                Ok(DraftOutcome {
                    state_changed: true,
                    start_loading,
                    echo: true,
                })
            }
            DraftAction::SelectSkin { .. } => Ok(DraftOutcome {
                state_changed: true,
                start_loading: false,
                echo: true,
            }),
        }
    }

    pub(crate) fn ranked_packet(&self, world: &World, picks: &[Pick]) -> RankedDraftStatePacket {
        let elapsed = self.step_started.elapsed().as_secs_f32();
        let remaining = (PHASE_DURATION_SECS - elapsed).max(0.0);
        let seats = std::array::from_fn(|idx| {
            let Some(actor) = world.entities.get(idx).filter(|e| e.kind == Kind::Hero) else {
                return RankedDraftSeat::default();
            };
            let Some(pick) = picks.get(idx) else {
                return RankedDraftSeat::default();
            };
            RankedDraftSeat {
                present: true,
                seat_index: idx as u8,
                selected: pick.selected,
                locked: pick.locked,
                position: pick.preferences.position_raw(),
                build_path: pick.preferences.build_raw(),
                team: actor.team + 1,
                actor_wire_id: wire_id(actor.id),
                hero_template: if pick.selected {
                    actor.type_id
                } else {
                    u16::MAX
                },
            }
        });
        RankedDraftStatePacket {
            phase: self.phase.to_wire(),
            active_turn: self.phase.active_turn_byte(),
            bans: self.bans,
            seats,
            timer_remaining_seconds: remaining,
            timer_total_seconds: PHASE_DURATION_SECS,
        }
    }

    fn advance_ban(&mut self, current_turn: u8) {
        let next = usize::from(current_turn) + 1;
        if next < self.ban_order.len() {
            self.phase = Draft5v5Phase::Ban { turn: next as u8 };
        } else {
            self.phase = Draft5v5Phase::Pick { turn: 0 };
        }
        self.step_started = Instant::now();
    }

    fn next_unlocked_pick_turn(&self, current_turn: u8, seats: &[Seat], picks: &[Pick]) -> u8 {
        let mut offset = 0usize;
        for block in &self.pick_blocks {
            let end = offset + block.len();
            if usize::from(current_turn) < end {
                // Keep advancing inside the active snake block as long as any seat is unlocked.
                let any_unlocked = block.iter().any(|&s| !is_seat_locked(s, seats, picks));
                if any_unlocked {
                    return current_turn;
                }
                return end.min(self.pick_order.len().saturating_sub(1)) as u8;
            }
            offset = end;
        }
        current_turn
    }
}

fn is_seat_locked(seat: usize, seats: &[Seat], picks: &[Pick]) -> bool {
    seats
        .get(seat)
        .map_or_else(|| picks.get(seat).is_some_and(|p| p.locked), |s| s.locked)
}

fn build_snake_blocks(t0: &[usize], t1: &[usize]) -> Vec<Vec<usize>> {
    let mut blocks = Vec::new();
    let specs: [(u8, usize, usize); 6] = [
        (0, 0, 1),
        (1, 0, 2),
        (0, 1, 3),
        (1, 2, 4),
        (0, 3, 5),
        (1, 4, 5),
    ];
    for (team, start, end) in specs {
        let src = if team == 0 { t0 } else { t1 };
        if start < src.len() {
            blocks.push(src[start..end.min(src.len())].to_vec());
        }
    }
    blocks
}

fn resolve_seat(raw: u8, seat_count: usize) -> Option<usize> {
    let idx = usize::from(raw);
    (idx < seat_count).then_some(idx)
}

fn resolve_swap_partner(actor: usize, first: u8, second: u8, seat_count: usize) -> Option<usize> {
    let a = resolve_seat(first, seat_count);
    let b = resolve_seat(second, seat_count);
    match (a, b) {
        (Some(x), Some(y)) if x == actor && y != actor => Some(y),
        (Some(x), Some(y)) if y == actor && x != actor => Some(x),
        (_, Some(y)) if y != actor => Some(y),
        (Some(x), _) if x != actor => Some(x),
        _ => None,
    }
}

pub(crate) fn hero_name_for_template(data: Option<&GameData>, template: u32) -> Option<String> {
    if template == u32::from(EntityTemplate::Ringo.as_raw()) {
        return Some("Ringo".to_owned());
    }
    data.and_then(|d| {
        d.manifest
            .iter()
            .find(|e| {
                u32::from(e.entity_type_id) == template && e.path.starts_with("build://Characters/")
            })
            .map(|e| e.name.trim_matches('*').to_owned())
    })
}

fn is_hero_locked_elsewhere(
    hero_id: u32,
    self_seat: usize,
    world: &World,
    seats: &[Seat],
    picks: &[Pick],
) -> bool {
    picks.iter().enumerate().any(|(i, p)| {
        i != self_seat
            && p.selected
            && is_seat_locked(i, seats, picks)
            && world
                .entities
                .get(i)
                .is_some_and(|e| u32::from(e.type_id) == hero_id)
    })
}

fn apply_hero_selection(
    seat: usize,
    name: &str,
    hero_id: u32,
    flags: u16,
    world: &mut World,
    picks: &mut [Pick],
    data: Option<&GameData>,
) -> Result<(), Draft5v5Error> {
    if let Some(d) = data {
        world
            .select_hero(seat, name, d)
            .map_err(|_| Draft5v5Error::InvalidHero)?;
    } else if name != "Ringo" {
        return Err(Draft5v5Error::InvalidHero);
    } else {
        world.entities[seat].type_id = hero_id as u16;
    }
    let skin = data
        .and_then(|d| d.indexed_skin(name, None))
        .unwrap_or_else(|| halcyon_data::skins::default_skin_hash(name));
    picks[seat] = Pick {
        preferences: picks[seat].preferences,
        name: name.to_owned(),
        skin: if picks[seat].name == name && flags != 0 {
            picks[seat].skin
        } else {
            skin
        },
        selected: true,
        locked: false,
    };
    Ok(())
}

fn swap_seat_heroes(
    a: usize,
    b: usize,
    world: &mut World,
    picks: &mut [Pick],
    data: Option<&GameData>,
) {
    let name_a = picks[a].name.clone();
    let name_b = picks[b].name.clone();
    let skin_a = picks[a].skin;
    let skin_b = picks[b].skin;
    let type_a = world.entities[a].type_id;
    let type_b = world.entities[b].type_id;
    if let Some(d) = data {
        let _ = world.select_hero(a, &name_b, d);
        let _ = world.select_hero(b, &name_a, d);
    } else {
        world.entities[a].type_id = type_b;
        world.entities[b].type_id = type_a;
    }
    picks[a].name = name_b;
    picks[a].skin = skin_b;
    picks[b].name = name_a;
    picks[b].skin = skin_a;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Message, Opcode, RoomPhase, Rooms, transport::Delivery};
    use halcyon_platform::{MatchHost, Reservation};
    use halcyon_protocol::{CampVisibilityState, Fields};
    use halcyon_sim::{BasicItem, Command, Event, map_5v5};
    use std::sync::Arc;
    use tokio::sync::{mpsc, watch};
    use uuid::Uuid;

    fn arm_hero_for_fast_clear(world: &mut World) {
        for seat in 0..2 {
            let id = world.entities[seat].id;
            world.entities[seat].gold = 50_000;
            for _ in 0..4 {
                world.purchase(id, BasicItem::Sorrowblade).unwrap();
            }
            for _ in 0..2 {
                world.purchase(id, BasicItem::TyrantsMonocle).unwrap();
            }
            world.entities[seat].max_hp = 50_000.;
            world.entities[seat].hp = 50_000.;
        }
    }

    fn test_data() -> Option<Arc<GameData>> {
        let delivery = std::env::var_os("HALCYON_DELIVERY")?;
        let extra = std::env::var_os("HALCYON_DATA_EXTRA");
        GameData::load_with_extra(
            std::path::Path::new(&delivery),
            extra.as_deref().map(std::path::Path::new),
        )
        .ok()
        .map(Arc::new)
    }

    fn drain_delivery(rx: &mut mpsc::Receiver<Delivery>) -> Vec<Message> {
        let mut out = Vec::new();
        while let Ok(d) = rx.try_recv() {
            out.extend(d.into_records());
        }
        out
    }

    fn drain_observer(rx: &mut mpsc::Receiver<Vec<Message>>) -> Vec<Message> {
        let mut out = Vec::new();
        while let Ok(b) = rx.try_recv() {
            out.extend(b);
        }
        out
    }

    #[tokio::test]
    async fn casual_5v5_ten_human_lifecycle_and_live_042c_camp_visibility() {
        let data = test_data();
        let rooms = Rooms::new(
            "127.0.0.1".parse().unwrap(),
            "127.0.0.1".into(),
            1,
            data.clone(),
        );
        let room_id = Uuid::new_v4();
        let players: Vec<Uuid> = (0..10).map(|_| Uuid::new_v4()).collect();
        let teams = vec![0, 0, 0, 0, 0, 1, 1, 1, 1, 1];
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: room_id,
                players: players.clone(),
                teams,
                mode: "5v5_pvp_casual".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(room_id).unwrap();

        // Attach all 10 draft connections cleanly.
        let mut draft_rxs = Vec::with_capacity(10);
        for &pid in &players {
            let mut r = shared.lock().unwrap();
            assert!(r.world.is_5v5_enabled());
            let (tx, rx) = mpsc::channel(32);
            let (_, ch, _, initial) = r.attach(pid, tx, watch::channel(false).0).unwrap();
            assert_eq!(ch, 0);
            assert!(initial.iter().any(|m| m.is(Opcode::DraftRosterFull)));
            draft_rxs.push(rx);
        }

        // Hover & lock Ringo across all 10 casual seats -> transitions HeroSelect -> Loading + 0x046C.
        let ringo_tpl = u32::from(EntityTemplate::Ringo.as_raw());
        for (seat, _) in players.iter().enumerate() {
            let mut r = shared.lock().unwrap();
            let select = DraftAction::SelectHero {
                hero_id: ringo_tpl,
                flags: 0,
            }
            .encode();
            let _ = r.receive(seat, 0, select).unwrap();
            let lock = DraftAction::LockHero { confirm: 1 }.encode();
            let reply = r.receive(seat, 0, lock).unwrap();
            if seat == 9 {
                assert_eq!(r.phase, RoomPhase::Loading);
                assert!(reply.iter().any(|m| m.is(Opcode::DraftTransitionToLoading)));
            }
        }
        // Other seats receive 0x046C broadcast upon final lock.
        let peer0_msgs = drain_delivery(&mut draft_rxs[0]);
        assert!(
            peer0_msgs
                .iter()
                .any(|m| m.is(Opcode::DraftTransitionToLoading))
        );

        // Attach 10 game connections -> transitions to Running and emits 0x03F2 camp markers.
        let mut game_rxs = Vec::with_capacity(10);
        for (seat, &pid) in players.iter().enumerate() {
            let mut r = shared.lock().unwrap();
            let (tx, rx) = mpsc::channel(4096);
            let (_, ch, _, initial) = r.attach(pid, tx, watch::channel(false).0).unwrap();
            assert_eq!(ch, 1);
            let spawned_markers: Vec<u32> = initial
                .iter()
                .filter(|m| m.is(Opcode::EntitySpawn))
                .map(|m| u32::from_be_bytes(m.body[8..12].try_into().unwrap()))
                .collect();
            for actor in 6580..=6591 {
                assert!(
                    spawned_markers.contains(&actor),
                    "seat {seat} missing 0x03F2 camp marker {actor}"
                );
            }
            assert!(spawned_markers.contains(&6566));
            assert!(spawned_markers.contains(&6567));
            game_rxs.push(rx);
        }
        assert_eq!(shared.lock().unwrap().phase, RoomPhase::Running);

        // Connect live observer (index 0) & drain initial setup frames.
        let (_, grant) = rooms.observe(room_id, Uuid::new_v4()).unwrap();
        let (obs_tx, mut obs_rx) = mpsc::channel(4096);
        {
            let mut r = shared.lock().unwrap();
            r.observers.get_mut(&grant).unwrap().live = Some(obs_tx);
        }

        // Advance past camp spawn (35s) and confirm 0x042C emissions reach observer (0), team 0 (1), team 1 (2).
        let mut team0_spawn_msgs = Vec::new();
        let mut team1_spawn_msgs = Vec::new();
        let mut obs_spawn_msgs = Vec::new();
        for _ in 0..map_5v5::CAMP_SPAWN_TICK + 2 {
            shared.lock().unwrap().tick();
            team0_spawn_msgs.extend(drain_delivery(&mut game_rxs[0]));
            team1_spawn_msgs.extend(drain_delivery(&mut game_rxs[5]));
            obs_spawn_msgs.extend(drain_observer(&mut obs_rx));
        }
        let extract_042c = |msgs: &[Message]| -> Vec<(u32, u8, CampVisibilityState)> {
            msgs.iter()
                .filter_map(|m| match m.fields().ok()? {
                    Fields::CampVisibility {
                        actor,
                        index,
                        state,
                    } => Some((actor, index, state)),
                    _ => None,
                })
                .collect()
        };
        let t0_camps = extract_042c(&team0_spawn_msgs);
        let t1_camps = extract_042c(&team1_spawn_msgs);
        let obs_camps = extract_042c(&obs_spawn_msgs);
        assert!(
            obs_camps
                .iter()
                .any(|&(_, idx, st)| idx == 0 && st == CampVisibilityState::PopulatedVisible)
        );
        assert!(
            t0_camps
                .iter()
                .any(|&(_, idx, st)| idx == 1 && st == CampVisibilityState::PopulatedHidden)
        );
        assert!(
            t1_camps
                .iter()
                .any(|&(_, idx, st)| idx == 2 && st == CampVisibilityState::PopulatedHidden)
        );

        // Move Team 0 Hero 1 to Weapon Treant on Team 0 side & clear it -> team 0 (idx=1) and
        // observer (idx=0) get EmptyVisible (1), while unvisited team 1 (idx=2) does NOT.
        let (wp_id, wp_pos, marker_actor) = {
            let r = shared.lock().unwrap();
            let u = r
                .world
                .entities
                .iter()
                .find(|e| {
                    e.kind == halcyon_sim::Kind::Neutral
                        && e.hp > 0.
                        && e.type_id == EntityTemplate::WeaponTreant.as_raw()
                        && e.position.x < 0.
                })
                .unwrap();
            let actor = r
                .world
                .camp_5v5_markers()
                .into_iter()
                .find(|(_, k, p, _)| {
                    *k == halcyon_sim::CampKind5v5::WeaponTreant
                        && ((p.x - u.position.x).powi(2) + (p.y - u.position.y).powi(2)).sqrt()
                            < 5.0
                })
                .unwrap()
                .0;
            (u.id, u.position, actor)
        };
        {
            let mut r = shared.lock().unwrap();
            arm_hero_for_fast_clear(&mut r.world);
            r.world.entities[0].position = wp_pos;
            r.world.command(1, Command::Attack(wp_id)).unwrap();
        }
        for _ in 0..100 {
            shared.lock().unwrap().tick();
        }
        let t0_clear = extract_042c(&drain_delivery(&mut game_rxs[0]));
        let t1_clear = extract_042c(&drain_delivery(&mut game_rxs[5]));
        let obs_clear = extract_042c(&drain_observer(&mut obs_rx));
        assert!(
            t0_clear.contains(&(marker_actor, 1, CampVisibilityState::EmptyVisible)),
            "clearing team viewer 1 must emit EmptyVisible for {marker_actor}"
        );
        assert!(
            obs_clear.contains(&(marker_actor, 0, CampVisibilityState::EmptyVisible)),
            "observer viewer 0 must emit EmptyVisible for {marker_actor}"
        );
        assert!(
            !t1_clear.contains(&(marker_actor, 2, CampVisibilityState::EmptyVisible)),
            "unvisited enemy viewer 2 must retain fog instead of prematurely learning EmptyVisible"
        );
    }

    #[tokio::test]
    async fn ranked_5v5_bp_swap_draft_lifecycle_045d_to_046a_and_045b() {
        let data = test_data();
        let rooms = Rooms::new(
            "127.0.0.1".parse().unwrap(),
            "127.0.0.1".into(),
            1,
            data.clone(),
        );
        let room_id = Uuid::new_v4();
        let players: Vec<Uuid> = (0..10).map(|_| Uuid::new_v4()).collect();
        let teams = vec![0, 0, 0, 0, 0, 1, 1, 1, 1, 1];
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: room_id,
                players: players.clone(),
                teams,
                mode: "5v5_pvp_ranked".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(room_id).unwrap();

        let mut draft_rxs = Vec::with_capacity(10);
        for (seat, &pid) in players.iter().enumerate() {
            let mut r = shared.lock().unwrap();
            let (tx, rx) = mpsc::channel(512);
            let (_, ch, _, initial) = r.attach(pid, tx, watch::channel(false).0).unwrap();
            assert_eq!(ch, 0);
            let pkt_msg = initial
                .iter()
                .find(|m| m.opcode == crate::wire_opcode::DraftRosterRanked)
                .expect("ranked attach must include 0x045B packet");
            let pkt = RankedDraftStatePacket::decode(&pkt_msg.body).unwrap();
            assert_eq!(pkt.phase, RankedWirePhase::Ban);
            assert!(pkt.seats[seat].present);
            draft_rxs.push(rx);
        }

        let catalog_ids: Vec<u32> = if let Some(ref d) = data {
            d.hero_names
                .iter()
                .filter_map(|name| d.type_id(name).map(u32::from))
                .take(14)
                .collect()
        } else {
            vec![u32::from(EntityTemplate::Ringo.as_raw())]
        };
        let ban_a = catalog_ids[0];
        let ban_b = *catalog_ids.get(1).unwrap_or(&ban_a);
        let ban_c = *catalog_ids.get(2).unwrap_or(&ban_a);
        let unique_heroes = catalog_ids.len() >= 14;

        // 1. Out-of-turn player (seat 1) cannot ban; Blue captain (seat 0) -> Red captain (seat 5) -> etc.
        {
            let mut r = shared.lock().unwrap();
            let bad_ban = r
                .receive(1, 0, DraftAction::SelectBan { hero_id: ban_a }.encode())
                .unwrap();
            assert!(
                bad_ban.is_empty(),
                "out-of-turn ban must not mutate or echo draft state"
            );
            // Turn 0: seat 0 bans ban_a.
            r.receive(0, 0, DraftAction::SelectBan { hero_id: ban_a }.encode())
                .unwrap();
            r.receive(0, 0, DraftAction::ConfirmBan { confirm: 1 }.encode())
                .unwrap();
            // Turn 1: seat 5 skips ban.
            r.receive(5, 0, DraftAction::SkipBan { flags: 1 }.encode())
                .unwrap();
            // Turn 2: seat 0 bans ban_b (or skips if fallback Ringo-only data).
            if unique_heroes {
                r.receive(0, 0, DraftAction::SelectBan { hero_id: ban_b }.encode())
                    .unwrap();
                r.receive(0, 0, DraftAction::ConfirmBan { confirm: 1 }.encode())
                    .unwrap();
                // Turn 3: seat 5 bans ban_c.
                r.receive(5, 0, DraftAction::SelectBan { hero_id: ban_c }.encode())
                    .unwrap();
                r.receive(5, 0, DraftAction::ConfirmBan { confirm: 1 }.encode())
                    .unwrap();
            } else {
                r.receive(0, 0, DraftAction::SkipBan { flags: 1 }.encode())
                    .unwrap();
                r.receive(5, 0, DraftAction::SkipBan { flags: 1 }.encode())
                    .unwrap();
            }
            let draft = r.draft_5v5.as_ref().unwrap();
            assert_eq!(draft.phase, Draft5v5Phase::Pick { turn: 0 });
            if unique_heroes {
                assert_eq!(draft.bans, [ban_a, 0, ban_b, ban_c]);
            }
        }

        // 2. Enforce banned hero & out-of-turn rejections during 1-2-2-2-2-1 snake picks.
        let pick_pool: Vec<u32> = if unique_heroes {
            catalog_ids[3..13].to_vec()
        } else {
            vec![catalog_ids[0]; 10]
        };
        {
            let mut r = shared.lock().unwrap();
            if unique_heroes {
                // Team 1 seat 5 trying to pick during Team 0 turn 0 must fail.
                let wrong_turn = r
                    .receive(
                        5,
                        0,
                        DraftAction::SelectHero {
                            hero_id: pick_pool[0],
                            flags: 0,
                        }
                        .encode(),
                    )
                    .unwrap();
                assert!(wrong_turn.is_empty());
                // Picking banned hero ban_a must fail cleanly.
                let banned_pick = r
                    .receive(
                        0,
                        0,
                        DraftAction::SelectHero {
                            hero_id: ban_a,
                            flags: 0,
                        }
                        .encode(),
                    )
                    .unwrap();
                assert!(banned_pick.is_empty());
                assert!(!r.picks[0].selected);
            }
            // Walk exact 1-2-2-2-2-1 snake seats: [0], [5, 6], [1, 2], [7, 8], [3, 4], [9].
            let snake = [0, 5, 6, 1, 2, 7, 8, 3, 4, 9];
            for (step, &seat) in snake.iter().enumerate() {
                let hero_id = pick_pool[step];
                if unique_heroes && step > 0 {
                    // Already locked hero pick_pool[0] must be rejected on another seat.
                    let dup = r
                        .receive(
                            seat,
                            0,
                            DraftAction::SelectHero {
                                hero_id: pick_pool[0],
                                flags: 0,
                            }
                            .encode(),
                        )
                        .unwrap();
                    assert!(dup.is_empty(), "duplicate hero selection must fail");
                }
                let sel = r
                    .receive(
                        seat,
                        0,
                        DraftAction::SelectHero { hero_id, flags: 0 }.encode(),
                    )
                    .unwrap();
                assert!(
                    sel.iter()
                        .any(|m| m.opcode == crate::wire_opcode::DraftRosterRanked)
                );
                let lock = r
                    .receive(seat, 0, DraftAction::LockHero { confirm: 1 }.encode())
                    .unwrap();
                assert!(lock.iter().any(|m| m.is(Opcode::DraftLockHero)));
            }
            assert_eq!(r.draft_5v5.as_ref().unwrap().phase, Draft5v5Phase::Swap);
        }

        // 3. Swap phase: cross-team rejected, decline clears, accept swaps heroes keeping seats/teams.
        {
            let mut r = shared.lock().unwrap();
            let before_0 = r.picks[0].name.clone();
            let before_1 = r.picks[1].name.clone();
            // Cross-team swap request (0 -> 5) rejected.
            let cross = r
                .receive(
                    0,
                    0,
                    DraftAction::SwapRequest {
                        first: 0,
                        second: 5,
                    }
                    .encode(),
                )
                .unwrap();
            assert!(cross.is_empty());
            // Same-team request (0 -> 1) + decline (0x046A) keeps heroes intact.
            r.receive(
                0,
                0,
                DraftAction::SwapRequest {
                    first: 0,
                    second: 1,
                }
                .encode(),
            )
            .unwrap();
            assert_eq!(
                r.draft_5v5.as_ref().unwrap().pending_swap,
                Some(PendingSwap {
                    requester: 0,
                    target: 1
                })
            );
            r.receive(
                1,
                0,
                DraftAction::SwapDecline {
                    first: 1,
                    second: 0,
                }
                .encode(),
            )
            .unwrap();
            assert!(r.draft_5v5.as_ref().unwrap().pending_swap.is_none());
            assert_eq!(r.picks[0].name, before_0);
            assert_eq!(r.picks[1].name, before_1);

            // Request (0 -> 1) + accept ([responder, requester] = [1, 0]) swaps picks cleanly.
            r.receive(
                0,
                0,
                DraftAction::SwapRequest {
                    first: 0,
                    second: 1,
                }
                .encode(),
            )
            .unwrap();
            r.receive(
                1,
                0,
                DraftAction::SwapResponse {
                    first: 1,
                    second: 0,
                }
                .encode(),
            )
            .unwrap();
            assert_eq!(r.picks[0].name, before_1);
            assert_eq!(r.picks[1].name, before_0);
            assert_eq!(r.world.entities[0].team, 0);
            assert_eq!(r.world.entities[1].team, 0);

            // Position & build preferences sync into 0x045B.
            r.receive(
                0,
                0,
                DraftAction::SelectPosition {
                    position: Some(halcyon_protocol::draft::DraftPosition::Mid),
                }
                .encode(),
            )
            .unwrap();
            let pref_reply = r
                .receive(
                    0,
                    0,
                    DraftAction::SelectBuildPath {
                        build_path: Some(halcyon_protocol::draft::DraftBuildPath::Crystal),
                    }
                    .encode(),
                )
                .unwrap();
            let pkt_msg = pref_reply
                .iter()
                .find(|m| m.opcode == crate::wire_opcode::DraftRosterRanked)
                .unwrap();
            let decoded = RankedDraftStatePacket::decode(&pkt_msg.body).unwrap();
            assert_eq!(decoded.seats[0].position, 2);
            assert_eq!(decoded.seats[0].build_path, 1);

            // All 10 humans ready up (0x046B) -> transitions into Loading + 0x046C.
            for seat in 0..10 {
                let reply = r
                    .receive(seat, 0, DraftAction::ReadyControl { control: 1 }.encode())
                    .unwrap();
                if seat == 9 {
                    assert_eq!(r.phase, RoomPhase::Loading);
                    assert!(reply.iter().any(|m| m.is(Opcode::DraftTransitionToLoading)));
                }
            }
        }
        let seat0_msgs = drain_delivery(&mut draft_rxs[0]);
        assert!(
            seat0_msgs
                .iter()
                .any(|m| m.is(Opcode::DraftTransitionToLoading))
        );
    }

    #[test]
    #[ignore = "slow soak; run with cargo nextest run --run-ignored all"]
    fn postgame_statistics_048d_populates_blackclaw_and_ghostwing_captures() {
        let data = test_data();
        let mut world = World::new(7, 10);
        world.assign_teams(&[0, 0, 0, 0, 0, 1, 1, 1, 1, 1]).unwrap();
        world.use_sovereigns_rise_5v5(data.as_deref()).unwrap();
        arm_hero_for_fast_clear(&mut world);

        // Advance to Ghostwing (9:00) and kill it as Team 0 Hero 1.
        while world.tick < map_5v5::GHOSTWING_SPAWN_TICK {
            world.step();
        }
        let gw_id = world.ghostwing_status().unwrap().0.unwrap();
        let gw_pos = world
            .entities
            .iter()
            .find(|e| e.id == gw_id)
            .unwrap()
            .position;
        world.entities[0].position = gw_pos;
        world.entities[1].position = gw_pos;
        world.entities[0].hp = 50_000.;
        world.entities[1].hp = 50_000.;
        world.command(1, Command::Attack(gw_id)).unwrap();
        world.command(2, Command::Attack(gw_id)).unwrap();
        let mut captured_gw = false;
        for _ in 0..600 {
            if world
                .step()
                .iter()
                .any(|e| matches!(e, Event::GhostwingCaptured { team: 0 }))
            {
                captured_gw = true;
                break;
            }
        }
        assert!(captured_gw);

        // Advance to Blackclaw (15:00) and kill it as Team 0 Heroes 1 & 2.
        while world.tick < map_5v5::BLACKCLAW_SPAWN_TICK {
            world.step();
        }
        let bc_id = world.blackclaw_status().unwrap().0.unwrap();
        let bc_pos = world
            .entities
            .iter()
            .find(|e| e.id == bc_id)
            .unwrap()
            .position;
        world.entities[0].position = bc_pos;
        world.entities[1].position = bc_pos;
        world.entities[0].hp = 50_000.;
        world.entities[1].hp = 50_000.;
        world.command(1, Command::Attack(bc_id)).unwrap();
        world.command(2, Command::Attack(bc_id)).unwrap();
        let mut captured_bc = false;
        for _ in 0..1200 {
            if world
                .step()
                .iter()
                .any(|e| matches!(e, Event::BlackclawCaptured { team: 0 }))
            {
                captured_bc = true;
                break;
            }
        }
        assert!(captured_bc);

        let msg = crate::result_frames::statistics::snapshot(&world).unwrap();
        let Fields::MatchStatistics(report) = msg.fields().unwrap() else {
            panic!("expected MatchStatistics frame");
        };
        let team0 = report.team(0).unwrap();
        let team1 = report.team(1).unwrap();
        assert_eq!(team0.ghostwing_captures, 1);
        assert_eq!(team0.blackclaw_captures, 1);
        assert!(team0.first_ghostwing_capture_time >= 540.0);
        assert!(team0.first_blackclaw_capture_time >= 900.0);
        assert_eq!(team1.ghostwing_captures, 0);
        assert_eq!(team1.blackclaw_captures, 0);
    }

    #[test]
    fn draft_selection_resolves_client_lookup2_default_skins_for_all_playable_heroes() {
        let Some(data) = test_data() else {
            return;
        };
        let mut world = World::new(7, 10);
        let mut picks: Vec<Pick> = (0..10)
            .map(|_| Pick {
                preferences: Default::default(),
                name: "Ringo".into(),
                skin: halcyon_data::skins::default_skin_hash("Ringo"),
                selected: false,
                locked: false,
            })
            .collect();
        for hero in &data.hero_names {
            let tid = u32::from(data.type_id(hero).expect("manifest entry"));
            apply_hero_selection(0, hero, tid, 0, &mut world, &mut picks, Some(&data)).unwrap();
            assert_eq!(
                picks[0].skin,
                halcyon_data::skins::default_skin_hash(hero),
                "default skin hash mismatch for {hero}"
            );
        }
        // Regression check for `2fff17f3` (`Churnwalker Bot` + `Catherine`/`Silvernail` tail 9..11).
        for (hero, expected) in [
            ("Catherine", 0x62db_dc96),
            ("Silvernail", 0xf4ec_ac3f),
            ("Churnwalker", 0xc73e_ab15),
            ("Amael", 0x2fd7_245d),
            ("Hero016", 0x7bd2_186c),
        ] {
            let tid = u32::from(data.type_id(hero).expect("manifest entry"));
            apply_hero_selection(0, hero, tid, 0, &mut world, &mut picks, Some(&data)).unwrap();
            assert_eq!(picks[0].skin, expected, "{hero}");
        }
    }
}
