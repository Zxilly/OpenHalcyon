//! Bevy owns persistent spell components. An ordinal orders impacts and checkpoints
//! independently of archetype moves and handle reuse. One room-local scratch list
//! is reused by both systems and contains only the current phase's scheduled work.
use super::{Event, World, arcane_fire::Pulse, bulwark::Field};
use bevy_ecs::component::Mutable;
use bevy_ecs::prelude::{Commands, Component, Entity, In, InMut, Query};

#[derive(Component)]
struct CastOrder(u64);

pub(super) trait TimedSpell: Component<Mutability = Mutable> {
    fn expired_before_step(&self, tick: u64) -> bool;
    fn begin_impact(&mut self) -> Self;
    fn advance(&mut self, world: &mut World, events: &mut Vec<Event>) -> bool;
}

type Agenda = Vec<(u64, Entity)>;
#[derive(Default)]
pub(super) struct Spells {
    order: u64,
    scratch: Agenda,
    // Keep insertion order across actor archetype changes and handle reuse.
    handles: Vec<Entity>,
}
impl super::Actors<'_> {
    pub(super) fn spells<T: TimedSpell>(&self) -> impl Iterator<Item = &T> {
        self.meta
            .spells
            .handles
            .iter()
            .filter_map(|id| self.ecs.get::<T>(*id))
    }
    pub(super) fn spell_snapshot<T: TimedSpell>(&self) -> Vec<&T> {
        self.spells::<T>().collect()
    }
    pub(super) fn spawn_spell<T: TimedSpell>(&mut self, spell: T) {
        let id = self
            .ecs
            .spawn((CastOrder(self.meta.spells.order), spell))
            .id();
        self.meta.spells.handles.push(id);
        self.meta.spells.order += 1;
    }
    pub(super) fn remove_spell<T: TimedSpell>(
        &mut self,
        mut predicate: impl FnMut(&T) -> bool,
    ) -> Option<T> {
        let i = self
            .meta
            .spells
            .handles
            .iter()
            .position(|id| self.ecs.get::<T>(*id).is_some_and(&mut predicate))?;
        let id = self.meta.spells.handles.remove(i);
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
        self.entities
            .ecs
            .run_system_cached_with(schedule::<T>, (self.tick, &mut *jobs))
            .expect("registered spell scheduling system");
        let mut active = false;
        for &(_, id) in jobs.iter() {
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
                    active = true;
                }
            } else {
                // A replacement may already have removed this generation.
                let _ = ecs.try_despawn(id);
            }
        }
        active
    }
    pub(super) fn advance_spell_family<T: TimedSpell>(&mut self, events: &mut Vec<Event>) {
        if self.entities.spells::<T>().next().is_none() {
            return;
        }
        let mut jobs = std::mem::take(&mut self.entities.meta.spells.scratch);
        self.resolve_spells::<T>(&mut jobs, events);
        let arena = &mut self.entities.meta.spells;
        arena
            .handles
            .retain(|id| self.entities.ecs.get::<CastOrder>(*id).is_some());
        arena.scratch = jobs;
    }
    pub(super) fn advance_persistent_spells(&mut self, events: &mut Vec<Event>) {
        self.advance_spell_family::<Field>(events);
        self.advance_spell_family::<Pulse>(events);
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
        let mut w = World::new(7, 10);
        w.entities.spawn_spell(ReplacementProbe(0));
        let mut jobs = Agenda::new();
        w.resolve_spells::<ReplacementProbe>(&mut jobs, &mut Vec::new());
        assert_eq!(w.entities.spells::<ReplacementProbe>().next().unwrap().0, 1);
        w.resolve_spells::<ReplacementProbe>(&mut jobs, &mut Vec::new());
        assert!(w.entities.spells::<ReplacementProbe>().next().is_none());
    }
}
