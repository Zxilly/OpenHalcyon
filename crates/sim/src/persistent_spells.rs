//! Private spell entities are delivered in cast order, independent of Bevy's archetype order.
use super::{Event, World, arcane_fire::Pulse, bulwark::Field};
use bevy_ecs::component::Mutable;
use bevy_ecs::prelude::{
    Commands, Component, Entity, InMut, Mut, Query, ResMut, Resource, World as EffectWorld,
};
use std::marker::PhantomData;

pub(super) trait TimedSpell: Component<Mutability = Mutable> {
    fn advance(&mut self, world: &mut World, events: &mut Vec<Event>) -> bool;
}
#[derive(Resource)]
struct CastOrder<T: TimedSpell> {
    entities: Vec<Entity>,
    kind: PhantomData<T>,
}
impl<T: TimedSpell> Default for CastOrder<T> {
    fn default() -> Self {
        Self {
            entities: Vec::new(),
            kind: PhantomData,
        }
    }
}
#[derive(Default)]
pub(super) struct Spells(Option<Box<EffectWorld>>);
/// Writes the same sequence as the pre-ECS checkpoint without materializing a second list.
struct Snapshot<'a, T>(&'a Spells, PhantomData<T>);
impl<T: TimedSpell + serde::Serialize> serde::Serialize for Snapshot<'_, T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter::<T>())
    }
}

impl Spells {
    pub fn snapshot<T: TimedSpell + serde::Serialize>(&self) -> impl serde::Serialize + '_ {
        Snapshot::<T>(self, PhantomData)
    }

    pub fn iter<T: TimedSpell>(&self) -> impl Iterator<Item = &T> {
        self.0.iter().flat_map(|ecs| {
            ecs.resource::<CastOrder<T>>()
                .entities
                .iter()
                .map(|id| ecs.get::<T>(*id).expect("live spell in cast order"))
        })
    }
    fn storage(&mut self) -> &mut EffectWorld {
        self.0.get_or_insert_with(|| {
            let mut ecs = EffectWorld::new();
            ecs.init_resource::<CastOrder<Field>>();
            ecs.init_resource::<CastOrder<Pulse>>();
            Box::new(ecs)
        })
    }
    pub fn spawn<T: TimedSpell>(&mut self, spell: T) {
        let ecs = self.storage();
        let id = ecs.spawn(spell).id();
        ecs.resource_mut::<CastOrder<T>>().entities.push(id);
    }
    pub fn refresh_fire(&mut self, spell: Pulse) {
        let ecs = self.storage();
        ecs.resource_scope(|ecs, mut order: Mut<CastOrder<Pulse>>| {
            order.entities.retain(|id| {
                if ecs
                    .get::<Pulse>(*id)
                    .expect("live pulse")
                    .same_application(&spell)
                {
                    ecs.despawn(*id);
                    false
                } else {
                    true
                }
            });
        });
        self.spawn(spell);
    }
}
fn advance<T: TimedSpell>(
    (InMut(world), InMut(events)): (InMut<World>, InMut<Vec<Event>>),
    mut order: ResMut<CastOrder<T>>,
    mut spells: Query<&mut T>,
    mut commands: Commands,
) {
    order.entities.retain(|id| {
        let keep = spells
            .get_mut(*id)
            .expect("live spell in cast order")
            .advance(world, events);
        if !keep {
            commands.entity(*id).despawn();
        }
        keep
    });
}
impl World {
    pub(super) fn advance_persistent_spells(&mut self, events: &mut Vec<Event>) {
        let Some(mut ecs) = self.spells.0.take() else {
            return;
        };
        // Complete contact damage and deaths before healing or burning on this tick.
        if !ecs.resource::<CastOrder<Field>>().entities.is_empty() {
            ecs.run_system_cached_with(advance::<Field>, (&mut *self, &mut *events))
                .expect("bulwark system");
        }
        if !ecs.resource::<CastOrder<Pulse>>().entities.is_empty() {
            ecs.run_system_cached_with(advance::<Pulse>, (&mut *self, events))
                .expect("fire system");
        }
        self.spells.0 = Some(ecs);
    }
}
