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
#[path = "draft_5v5_tests.rs"]
mod tests;
