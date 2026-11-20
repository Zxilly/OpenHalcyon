//! Bevy owns persistent spell components. An ordinal orders impacts and checkpoints
//! independently of archetype moves and handle reuse. One room-local scratch list
//! is reused by both systems and contains only the current phase's scheduled work.
use super::{Event, World, arcane_fire::Pulse, bulwark::Field};
use bevy_ecs::component::Mutable;
use bevy_ecs::prelude::{Commands, Component, Entity, In, InMut, Query, World as EffectWorld};

#[derive(Component)]
struct CastOrder(u64);

pub(super) trait TimedSpell: Component<Mutability = Mutable> {
    fn expired_before_step(&self, tick: u64) -> bool;
    fn begin_impact(&mut self) -> Self;
    fn advance(&mut self, world: &mut World, events: &mut Vec<Event>) -> bool;
}

type Agenda = Vec<(u64, Entity)>;
#[derive(Default)]
struct Arena {
    ecs: EffectWorld,
    order: u64,
    active: bool,
    scratch: Agenda,
}
#[derive(Default)]
pub(super) struct Spells(Option<Box<Arena>>);

impl Spells {
    fn arena_mut(&mut self) -> &mut Arena {
        self.0.get_or_insert_with(Box::default)
    }

    pub fn iter<T: TimedSpell>(&self) -> impl Iterator<Item = &T> {
        self.0
            .iter()
            .filter(|a| a.active)
            .flat_map(|a| a.ecs.iter_entities().filter_map(|e| e.get::<T>()))
    }
    pub fn snapshot<T: TimedSpell>(&self) -> Vec<&T> {
        let mut rows: Vec<_> = self
            .0
            .iter()
            .filter(|a| a.active)
            .flat_map(|a| {
                a.ecs
                    .iter_entities()
                    .filter_map(|e| Some((e.get::<CastOrder>()?.0, e.get::<T>()?)))
            })
            .collect();
        rows.sort_unstable_by_key(|r| r.0);
        rows.into_iter().map(|(_, spell)| spell).collect()
    }
    pub fn spawn<T: TimedSpell>(&mut self, spell: T) {
        let arena = self.arena_mut();
        arena.ecs.spawn((CastOrder(arena.order), spell));
        arena.order += 1;
        arena.active = true;
    }
    pub fn refresh_fire(&mut self, spell: Pulse) {
        if let Some(arena) = &mut self.0 {
            let previous = arena
                .ecs
                .iter_entities()
                .find(|e| e.get::<Pulse>().is_some_and(|p| p.same_application(&spell)))
                .map(|e| e.id());
            if let Some(id) = previous {
                arena.ecs.despawn(id);
            }
        }
        self.spawn(spell);
    }
}

fn schedule<T: TimedSpell>(
    (In(tick), InMut(jobs)): (In<u64>, InMut<Agenda>),
    spells: Query<(Entity, &CastOrder, &T)>,
    mut commands: Commands,
) {
    jobs.clear();
    for (id, order, spell) in &spells {
        if spell.expired_before_step(tick) {
            commands.entity(id).despawn();
        } else {
            jobs.push((order.0, id));
        }
    }
    jobs.sort_unstable_by_key(|(order, _)| *order);
}
impl World {
    fn resolve_spells<T: TimedSpell>(
        &mut self,
        jobs: &mut Agenda,
        events: &mut Vec<Event>,
    ) -> bool {
        self.spells
            .arena_mut()
            .ecs
            .run_system_cached_with(schedule::<T>, (self.tick, &mut *jobs))
            .expect("registered spell scheduling system");
        let mut active = false;
        for &(_, id) in jobs.iter() {
            let Some(mut spell) = self
                .spells
                .arena_mut()
                .ecs
                .get_mut::<T>(id)
                .map(|mut component| component.begin_impact())
            else {
                continue;
            };
            // Combat callbacks can read other spells and refresh or remove the
            // original. Write back only if this exact generational handle survives.
            let keep = spell.advance(self, events);
            let ecs = &mut self.spells.arena_mut().ecs;
            if keep {
                if let Some(mut original) = ecs.get_mut::<T>(id) {
                    *original = spell;
                    active = true;
                }
            } else {
                // A replacement may already have removed this generation.
                let _ = ecs.try_despawn(id);
            }
        }
        active
    }
    pub(super) fn advance_persistent_spells(&mut self, events: &mut Vec<Event>) {
        let Some(arena) = self.spells.0.as_mut().filter(|a| a.active) else {
            return;
        };
        let start_order = arena.order;
        let mut jobs = std::mem::take(&mut arena.scratch);
        let fields = self.resolve_spells::<Field>(&mut jobs, events);
        let fire = self.resolve_spells::<Pulse>(&mut jobs, events);
        let arena = self.spells.arena_mut();
        arena.active = fields || fire || arena.order != start_order;
        arena.scratch = jobs;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Component, Clone, Copy)]
    struct ReplacementProbe(u8);
    impl TimedSpell for ReplacementProbe {
        fn expired_before_step(&self, _: u64) -> bool {
            false
        }
        fn begin_impact(&mut self) -> Self {
            *self
        }
        fn advance(&mut self, world: &mut World, _: &mut Vec<Event>) -> bool {
            assert_eq!(world.spells.iter::<Self>().count(), 1);
            if self.0 == 0 {
                let ecs = &mut world.spells.arena_mut().ecs;
                let id = ecs
                    .iter_entities()
                    .find(|e| e.contains::<Self>())
                    .unwrap()
                    .id();
                ecs.despawn(id);
                world.spells.spawn(Self(1));
            }
            false
        }
    }
    #[test]
    fn impact_can_see_and_replace_itself_without_overwriting_the_new_generation() {
        let mut w = World::new(7, 10);
        w.spells.spawn(ReplacementProbe(0));
        let mut jobs = Agenda::new();
        w.resolve_spells::<ReplacementProbe>(&mut jobs, &mut Vec::new());
        assert_eq!(w.spells.iter::<ReplacementProbe>().next().unwrap().0, 1);
        w.resolve_spells::<ReplacementProbe>(&mut jobs, &mut Vec::new());
        assert!(w.spells.iter::<ReplacementProbe>().next().is_none());
    }
}
