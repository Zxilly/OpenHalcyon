use super::{Actors, BotDiagnostics, Event, Room, RoomState, TICK_HZ};
use bevy_ecs::prelude::{
    IntoScheduleConfigs, Mut, Res, Resource, Schedule, Schedules, World as Ecs,
};
use bevy_ecs::schedule::ScheduleLabel;
use std::ops::{Deref, DerefMut};

pub(super) enum StateSlot<'a> {
    Owned(Option<Box<RoomState>>),
    Scoped(&'a mut RoomState),
}
impl StateSlot<'_> {
    pub fn owned(state: RoomState) -> Self {
        Self::Owned(Some(Box::new(state)))
    }
}
impl Deref for StateSlot<'_> {
    type Target = RoomState;
    fn deref(&self) -> &RoomState {
        match self {
            Self::Owned(s) => s.as_deref().expect("room state outside schedule"),
            Self::Scoped(s) => s,
        }
    }
}
impl DerefMut for StateSlot<'_> {
    fn deref_mut(&mut self) -> &mut RoomState {
        match self {
            Self::Owned(s) => s.as_deref_mut().expect("room state outside schedule"),
            Self::Scoped(s) => s,
        }
    }
}

#[derive(ScheduleLabel, Clone, Debug, Hash, PartialEq, Eq)]
struct FixedTick;

#[derive(Resource)]
struct Frame {
    state: Box<RoomState>,
    events: Vec<Event>,
    diagnostics: Option<BotDiagnostics>,
    after_effects: bool,
}

impl Room<'static> {
    pub fn step(&mut self) -> Vec<Event> {
        self.run_fixed_tick(None)
    }
    pub fn step_diagnosed(&mut self, diagnostics: &mut BotDiagnostics) -> Vec<Event> {
        self.run_fixed_tick(Some(diagnostics))
    }
    fn run_fixed_tick(&mut self, mut diagnostics: Option<&mut BotDiagnostics>) -> Vec<Event> {
        let events = std::mem::take(&mut self.state.pending_item_events);
        if self.state.winner.is_some() {
            return events;
        }
        let StateSlot::Owned(slot) = &mut self.state else {
            unreachable!()
        };
        let state = slot.take().expect("owned scalar state");
        let (frame, outcome) = self.entities.with_scoped_schedule(|ecs| {
            install(ecs);
            ecs.insert_resource(Frame {
                state,
                events,
                diagnostics: diagnostics.as_deref_mut().map(std::mem::take),
                after_effects: false,
            });
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                ecs.run_schedule(FixedTick)
            }));
            (
                ecs.remove_resource::<Frame>().expect("restored tick state"),
                outcome,
            )
        });
        *slot = Some(frame.state);
        if let Some(output) = diagnostics {
            *output = frame.diagnostics.expect("diagnosed tick")
        }
        if let Err(panic) = outcome {
            std::panic::resume_unwind(panic)
        }
        frame.events
    }
}

/// One serial schedule per physical room. A turn commits movement, purchases,
/// damage and deaths before the next actor sees the battlefield.
fn install(ecs: &mut Ecs) {
    if ecs
        .get_resource::<Schedules>()
        .is_some_and(|s| s.contains(FixedTick))
    {
        return;
    }
    let mut schedule = Schedule::new(FixedTick);
    schedule.set_executor(bevy_ecs::schedule::SingleThreadedExecutor::new());
    schedule.add_systems(
        (
            advance,
            super::actors::remove_defeated_units.run_if(contested),
            milestones.run_if(milestone_due),
        )
            .chain(),
    );
    ecs.add_schedule(schedule);
}
fn contested(frame: Res<Frame>) -> bool {
    frame.after_effects
}
fn milestone_due(frame: Res<Frame>) -> bool {
    matches!(frame.state.tick,t if t==300*TICK_HZ || t==600*TICK_HZ)
}

fn with_room<R>(
    ecs: &mut Ecs,
    f: impl FnOnce(&mut Room<'_>, &mut Vec<Event>, Option<&mut BotDiagnostics>) -> R,
) -> R {
    ecs.resource_scope(|ecs, mut frame: Mut<Frame>| {
        let Frame {
            state,
            events,
            diagnostics,
            ..
        } = frame.as_mut();
        Actors::scope(ecs, |entities| {
            let mut room = Room {
                entities,
                state: StateSlot::Scoped(state),
            };
            f(&mut room, events, diagnostics.as_mut())
        })
    })
}
/// These phases share exclusive access and commit their writes immediately.
fn advance(ecs: &mut Ecs) {
    let contested = with_room(ecs, |room, events, diagnostics| {
        room.state.tick += 1;
        room.advance_devices();
        room.state.nav_budget = 6;
        room.advance_spell_family::<crate::combat::SpellEffect>(events);
        room.advance_spell_family::<crate::chakram::Chakram>(events);
        room.advance_spell_family::<crate::lorelai::Water>(events);
        room.advance_spell_family::<crate::bulwark::Field>(events);
        room.advance_spell_family::<crate::arcane_fire::Pulse>(events);
        room.advance_spell_family::<crate::twister::Dash>(events);
        room.advance_spell_family::<crate::sigil::Field>(events);
        room.advance_spell_family::<crate::hero_kit::KitEffect>(events);
        if room.winner.is_some() {
            return false;
        }
        room.spawn_tick_waves(events);
        room.team_upkeep(events);
        match diagnostics {
            Some(d) => room.resolve_actor_turns::<true>(events, Some(d)),
            None => room.resolve_actor_turns::<false>(events, None),
        }
        true
    });
    ecs.resource_mut::<Frame>().after_effects = contested;
}
fn milestones(ecs: &mut Ecs) {
    with_room(ecs, |room, _, _| {
        room.record_milestone(room.tick == 300 * TICK_HZ)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Position, World};

    #[test]
    fn simultaneous_lethal_basic_attacks_resolve_in_seeded_turn_order() {
        for (seed, winner) in [(0, 2), (1, 1)] {
            let mut room = World::new(seed, 10);
            room.entities.retain(|actor| actor.id <= 2);
            for i in 0..2 {
                let actor = &mut room.entities[i];
                actor.position = Position {
                    x: 29. + i as f32,
                    y: 0.,
                };
                actor.stats.range = 2.;
                actor.hp = 1.;
                actor.next_attack = 0;
                let target = 2 - i as u32;
                actor.order = Command::Attack(target);
                actor.winding = Some((1, target));
            }
            let events = room.step();
            let deaths: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    Event::Death { id, killer } => Some((*id, *killer)),
                    _ => None,
                })
                .collect();
            assert_eq!(deaths, [(3 - winner, winner)]);
        }
    }

    #[test]
    fn exclusive_system_panic_restores_scalar_and_actor_ownership() {
        let mut room = World::new(15, 10);
        room.step();
        let before = room.entities.len();
        room.entities.with_scoped_schedule(|ecs| {
            ecs.resource_mut::<Schedules>()
                .get_mut(FixedTick)
                .unwrap()
                .add_systems(
                    (|ecs: &mut Ecs| with_room(ecs, |_, _, _| panic!("injected phase failure")))
                        .after(advance),
                );
        });
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| room.step()));
        assert!(result.is_err());
        assert_eq!(room.entities.len(), before);
        assert_eq!(room.tick, 2);
    }
}
