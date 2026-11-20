//! Each spell family keeps its live entity handles in cast order. Deferred
//! impacts recheck the handle generation after running combat callbacks.
use super::{Event, Room, arcane_fire::Pulse};
use bevy_ecs::component::Mutable;
use bevy_ecs::prelude::{Commands, Component, Entity, In, InMut, Query, Res, Resource};
use std::marker::PhantomData;

#[derive(Resource)]
struct CastOrder<T: TimedSpell> {
    handles: Vec<Entity>,
    spell: PhantomData<T>,
}
impl<T: TimedSpell> Default for CastOrder<T> {
    fn default() -> Self {
        Self {
            handles: Vec::new(),
            spell: PhantomData,
        }
    }
}

pub(super) trait TimedSpell: Component<Mutability = Mutable> {
    fn expired_before_step(&self, tick: u64) -> bool;
    fn begin_impact(&mut self) -> Self;
    fn advance(&mut self, world: &mut Room<'_>, events: &mut Vec<Event>) -> bool;
}

type Agenda = Vec<Entity>;
impl super::Actors<'_> {
    pub(super) fn spells<T: TimedSpell>(&self) -> impl Iterator<Item = &T> {
        self.ecs
            .get_resource::<CastOrder<T>>()
            .into_iter()
            .flat_map(|order| order.handles.iter())
            .filter_map(|id| self.ecs.get::<T>(*id))
    }
    pub(super) fn spell_snapshot<T: TimedSpell>(&self) -> Vec<&T> {
        self.spells::<T>().collect()
    }
    pub(super) fn spawn_spell<T: TimedSpell>(&mut self, spell: T) {
        let handle = self.ecs.spawn(spell).id();
        self.ecs
            .get_resource_or_insert_with(CastOrder::<T>::default)
            .handles
            .push(handle);
    }
    pub(super) fn remove_spell<T: TimedSpell>(
        &mut self,
        mut predicate: impl FnMut(&T) -> bool,
    ) -> Option<T> {
        let i = self
            .ecs
            .get_resource::<CastOrder<T>>()?
            .handles
            .iter()
            .position(|id| self.ecs.get::<T>(*id).is_some_and(&mut predicate))?;
        let id = self.ecs.resource_mut::<CastOrder<T>>().handles.remove(i);
        let spell = self.ecs.entity_mut(id).take::<T>()?;
        self.ecs.despawn(id);
        Some(spell)
    }
    pub(super) fn refresh_fire(&mut self, spell: Pulse) {
        self.remove_spell::<Pulse>(|p| p.same_application(&spell));
        self.spawn_spell(spell);
    }
}
fn schedule<T: TimedSpell>(
    (In(tick), InMut(jobs)): (In<u64>, InMut<Agenda>),
    order: Res<CastOrder<T>>,
    spells: Query<&T>,
    mut commands: Commands,
) {
    jobs.clear();
    for &id in &order.handles {
        let Ok(spell) = spells.get(id) else {
            continue;
        };
        if spell.expired_before_step(tick) {
            commands.entity(id).despawn();
        } else {
            jobs.push(id);
        }
    }
}
impl Room<'_> {
    fn resolve_spells<T: TimedSpell>(&mut self, jobs: &mut Agenda, events: &mut Vec<Event>) {
        self.entities
            .ecs
            .run_system_cached_with(schedule::<T>, (self.state.tick, &mut *jobs))
            .expect("registered spell scheduling system");
        for &id in jobs.iter() {
            let Some(mut spell) = self
                .entities
                .ecs
                .get_mut::<T>(id)
                .map(|mut component| component.begin_impact())
            else {
                continue;
            };
            // Combat callbacks can read other spells and refresh or remove the
            // original. Write back only if this exact generational handle survives.
            let keep = spell.advance(self, events);
            let ecs = &mut self.entities.ecs;
            if keep {
                if let Some(mut original) = ecs.get_mut::<T>(id) {
                    *original = spell;
                }
            } else {
                // A replacement may already have removed this generation.
                let _ = ecs.try_despawn(id);
            }
        }
    }
    pub(super) fn advance_spell_family<T: TimedSpell>(&mut self, events: &mut Vec<Event>) {
        if self
            .entities
            .ecs
            .get_resource::<CastOrder<T>>()
            .is_none_or(|order| order.handles.is_empty())
        {
            return;
        }
        let mut jobs = std::mem::take(&mut self.entities.meta.spell_jobs);
        self.resolve_spells::<T>(&mut jobs, events);
        self.entities
            .ecs
            .resource_scope(|ecs, mut order: bevy_ecs::prelude::Mut<CastOrder<T>>| {
                order.handles.retain(|id| ecs.get::<T>(*id).is_some());
            });
        self.entities.meta.spell_jobs = jobs;
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
        fn advance(&mut self, world: &mut Room<'_>, _: &mut Vec<Event>) -> bool {
            assert_eq!(world.entities.spells::<Self>().count(), 1);
            if self.0 == 0 {
                let ecs = &mut world.entities.ecs;
                let id = ecs
                    .iter_entities()
                    .find(|e| e.contains::<Self>())
                    .unwrap()
                    .id();
                ecs.despawn(id);
                world.entities.spawn_spell(Self(1));
            }
            false
        }
    }
    #[test]
    fn impact_can_see_and_replace_itself_without_overwriting_the_new_generation() {
        let mut w = crate::World::new(7, 10);
        w.entities.spawn_spell(ReplacementProbe(0));
        let mut jobs = Agenda::new();
        w.resolve_spells::<ReplacementProbe>(&mut jobs, &mut Vec::new());
        assert_eq!(w.entities.spells::<ReplacementProbe>().next().unwrap().0, 1);
        w.resolve_spells::<ReplacementProbe>(&mut jobs, &mut Vec::new());
        assert!(w.entities.spells::<ReplacementProbe>().next().is_none());
    }
}
