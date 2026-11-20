//! Each spell family keeps its live entity handles in cast order. Deferred
//! impacts recheck the handle generation after running combat callbacks.
use super::{Event, Room, arcane_fire::Pulse};
use bevy_ecs::component::Mutable;
use bevy_ecs::prelude::{Component, Entity, Mut, Resource};
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
    pub(crate) fn remove_spell_handle<T: TimedSpell>(&mut self, id: Entity) -> Option<T> {
        if let Some(mut order) = self.ecs.get_resource_mut::<CastOrder<T>>() {
            order.handles.retain(|h| *h != id);
        }
        let spell = self.ecs.entity_mut(id).take::<T>()?;
        self.ecs.despawn(id);
        Some(spell)
    }
    pub(super) fn refresh_fire(&mut self, spell: Pulse) {
        self.remove_spell::<Pulse>(|p| p.same_application(&spell));
        self.spawn_spell(spell);
    }
}
impl Room<'_> {
    fn resolve_spells<T: TimedSpell>(&mut self, jobs: &mut Agenda, events: &mut Vec<Event>) {
        let tick = self.state.tick;
        jobs.clear();
        // Freeze cast order before callbacks. A despawned handle can stay in
        // the index until this pass; generation-checked readers skip it.
        self.entities
            .ecs
            .resource_scope(|ecs, mut order: Mut<CastOrder<T>>| {
                order.handles.retain(|&id| {
                    let Some(spell) = ecs.get::<T>(id) else {
                        return false;
                    };
                    if spell.expired_before_step(tick) {
                        ecs.despawn(id);
                        return false;
                    }
                    jobs.push(id);
                    true
                });
            });
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
        self.entities.meta.spell_jobs = jobs;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Component, Clone, Copy)]
    struct OrderedProbe(u32);
    impl TimedSpell for OrderedProbe {
        fn expired_before_step(&self, _: u64) -> bool {
            self.0 == 2
        }
        fn begin_impact(&mut self) -> Self {
            *self
        }
        fn advance(&mut self, room: &mut Room<'_>, _: &mut Vec<Event>) -> bool {
            assert!(room.entities.spells::<Self>().all(|p| p.0 != 2));
            // Advance the decimal sequence to make callback order observable.
            room.next_id = room.next_id * 10 + self.0;
            if self.0 == 1 {
                room.entities.spawn_spell(Self(4));
            }
            false
        }
    }
    #[test]
    fn expiry_precedes_callbacks_and_new_casts_wait_for_the_next_pass() {
        let mut room = crate::World::new(1, 1);
        room.next_id = 0;
        for n in 1..=3 {
            room.entities.spawn_spell(OrderedProbe(n));
        }
        room.advance_spell_family::<OrderedProbe>(&mut Vec::new());
        assert_eq!(room.next_id, 13);
        room.advance_spell_family::<OrderedProbe>(&mut Vec::new());
        assert_eq!(room.next_id, 134);
        room.advance_spell_family::<OrderedProbe>(&mut Vec::new());
        assert!(
            room.entities
                .ecs
                .resource::<CastOrder<OrderedProbe>>()
                .handles
                .is_empty()
        );
    }

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
