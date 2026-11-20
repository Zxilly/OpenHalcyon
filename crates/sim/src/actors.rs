use super::{Entity, Position};
use bevy_ecs::{component::Component, entity::Entity as Handle, world::World};
use std::ops::{Index, IndexMut};

/// Authoritative room ECS. The ordered index stores only generational handles;
/// gameplay IDs and payloads live on their actor component.
pub struct Actors<'w> {
    pub(crate) ecs: Storage<'w, World>,
    pub(super) meta: Storage<'w, ActorMetadata>,
}
pub(crate) enum Storage<'w, T> {
    Owned(Box<T>),
    Scoped(&'w mut T),
}
impl<T> std::ops::Deref for Storage<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        match self {
            Self::Owned(t) => t,
            Self::Scoped(t) => t,
        }
    }
}
impl<T> std::ops::DerefMut for Storage<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        match self {
            Self::Owned(t) => t,
            Self::Scoped(t) => t,
        }
    }
}
#[derive(Default, bevy_ecs::resource::Resource)]
pub(super) struct ActorMetadata {
    order: Vec<Handle>,
    gameplay_ids: Vec<u32>,
    scan: Option<(usize, bevy_ecs::query::QueryState<&'static Entity>)>,
    pub(super) spells: super::persistent_spells::Spells,
}
impl Default for Actors<'static> {
    fn default() -> Self {
        Self {
            ecs: Storage::Owned(Box::default()),
            meta: Storage::Owned(Box::new(ActorMetadata {
                order: Vec::with_capacity(50),
                gameplay_ids: Vec::with_capacity(50),
                scan: None,
                spells: Default::default(),
            })),
        }
    }
}
impl Actors<'_> {
    /// Move only lightweight index/scratch ownership to the ECS for an exclusive schedule.
    pub(crate) fn with_scoped_schedule<R>(&mut self, f: impl FnOnce(&mut World) -> R) -> R {
        assert!(
            !self.ecs.contains_resource::<ActorMetadata>(),
            "actor schedule is already active"
        );
        self.ecs.insert_resource(std::mem::take(&mut *self.meta));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&mut self.ecs)));
        *self.meta = self
            .ecs
            .remove_resource()
            .expect("actor index restored by scoped systems");
        match result {
            Ok(value) => value,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }
    pub(crate) fn scope<R>(ecs: &mut World, f: impl FnOnce(Actors<'_>) -> R) -> R {
        ecs.resource_scope(
            |ecs, mut meta: bevy_ecs::change_detection::Mut<ActorMetadata>| {
                let mut actors = Actors {
                    ecs: Storage::Scoped(ecs),
                    meta: Storage::Scoped(&mut meta),
                };
                actors.refresh_scan();
                f(actors)
            },
        )
    }
}
pub struct ActorIter<'a> {
    ecs: &'a World,
    handles: std::slice::Iter<'a, Handle>,
}
impl<'a> Iterator for ActorIter<'a> {
    type Item = &'a Entity;
    fn next(&mut self) -> Option<Self::Item> {
        self.handles
            .next()
            .map(|id| self.ecs.get::<Entity>(*id).expect("live actor handle"))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.handles.size_hint()
    }
}
impl DoubleEndedIterator for ActorIter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.handles
            .next_back()
            .map(|id| self.ecs.get::<Entity>(*id).expect("live actor handle"))
    }
}
impl ExactSizeIterator for ActorIter<'_> {}
impl<'a> IntoIterator for &'a Actors<'_> {
    type Item = &'a Entity;
    type IntoIter = ActorIter<'a>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
// Lazily present for any automated actor that has acquired a target. A sparse
// component keeps the actor table layout independent of its presence.
#[derive(Component)]
#[component(storage = "SparseSet")]
struct LastSeenTarget(u32, Position, u64);

impl Actors<'_> {
    pub(super) fn last_seen(&self, index: usize) -> Option<(u32, Position, u64)> {
        let LastSeenTarget(target, position, tick) = self.ecs.get(*self.meta.order.get(index)?)?;
        Some((*target, *position, *tick))
    }

    pub(super) fn remember_target(
        &mut self,
        index: usize,
        target: u32,
        position: Position,
        tick: u64,
    ) {
        let actor = self.meta.order[index];
        let sighting = LastSeenTarget(target, position, tick);
        if let Some(mut last_seen) = self.ecs.get_mut::<LastSeenTarget>(actor) {
            *last_seen = sighting;
        } else {
            self.ecs.entity_mut(actor).insert(sighting);
        }
    }

    pub(super) fn forget_target(&mut self, index: usize) {
        let actor = self.meta.order[index];
        if self.ecs.get::<LastSeenTarget>(actor).is_some() {
            self.ecs.entity_mut(actor).remove::<LastSeenTarget>();
        }
    }
}

impl serde::Serialize for Actors<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.iter())
    }
}
impl Index<usize> for Actors<'_> {
    type Output = Entity;
    fn index(&self, i: usize) -> &Entity {
        self.get(i).expect("actor index out of bounds")
    }
}
impl IndexMut<usize> for Actors<'_> {
    fn index_mut(&mut self, i: usize) -> &mut Entity {
        self.get_mut(i).expect("actor index out of bounds")
    }
}
impl Actors<'_> {
    /// Only order-independent consumers may traverse Bevy tables directly.
    /// A new archetype with actors can appear during an exclusive system. Until
    /// the query has observed that generation the ordered handles remain exact.
    pub(super) fn any_unordered<'w>(&'w self, predicate: impl FnMut(&'w Entity) -> bool) -> bool {
        if let Some((seen, query)) = &self.meta.scan
            && *seen == self.ecs.archetypes().len()
        {
            query.iter_manual(&self.ecs).any(predicate)
        } else {
            self.iter().any(predicate)
        }
    }
    pub(super) fn for_each_unordered<'w>(&'w self, visit: impl FnMut(&'w Entity)) {
        if let Some((seen, query)) = &self.meta.scan
            && *seen == self.ecs.archetypes().len()
        {
            query.iter_manual(&self.ecs).for_each(visit);
        } else {
            self.iter().for_each(visit);
        }
    }
    pub fn iter(&self) -> ActorIter<'_> {
        ActorIter {
            ecs: &self.ecs,
            handles: self.meta.order.iter(),
        }
    }
    pub fn len(&self) -> usize {
        self.meta.order.len()
    }
    pub fn is_empty(&self) -> bool {
        self.meta.order.is_empty()
    }
    pub fn get(&self, i: usize) -> Option<&Entity> {
        self.ecs.get(*self.meta.order.get(i)?)
    }
    pub fn get_mut(&mut self, i: usize) -> Option<&mut Entity> {
        self.ecs
            .get_mut::<Entity>(*self.meta.order.get(i)?)
            .map(|v| v.into_inner())
    }
    pub fn find_mut(&mut self, predicate: impl FnMut(&Entity) -> bool) -> Option<&mut Entity> {
        let i = self.iter().position(predicate)?;
        self.get_mut(i)
    }
    pub fn last(&self) -> Option<&Entity> {
        self.get(self.len().checked_sub(1)?)
    }
    pub fn last_mut(&mut self) -> Option<&mut Entity> {
        self.get_mut(self.len().checked_sub(1)?)
    }
    pub fn first(&self) -> Option<&Entity> {
        self.get(0)
    }
    pub fn push(&mut self, e: Entity) {
        self.meta.gameplay_ids.push(e.id);
        self.meta.order.push(self.ecs.spawn(e).id());
        self.refresh_scan();
    }
    fn refresh_scan(&mut self) {
        let generation = self.ecs.archetypes().len();
        match &mut self.meta.scan {
            Some((seen, query)) if *seen != generation => {
                query.update_archetypes(&self.ecs);
                *seen = generation;
            }
            None => self.meta.scan = Some((generation, self.ecs.query::<&Entity>())),
            _ => {}
        }
    }
    pub(super) fn gameplay_index(&self, id: u32) -> Option<usize> {
        // Actor IDs are public: a caller can change one without a structural edit.
        // Recheck the live record before accepting an indexed hit. Preserve the
        // sorted search's last duplicate if an adjacent record acquired this ID.
        if let Ok(i) = self.meta.gameplay_ids.binary_search(&id)
            && self[i].id == id
            && self.get(i + 1).is_none_or(|next| next.id != id)
        {
            return Some(i);
        }
        self.binary_search_by_key(&id, |e| e.id).ok()
    }
    fn rebuild_gameplay_ids(&mut self) {
        let meta = &mut *self.meta;
        meta.gameplay_ids.clear();
        meta.gameplay_ids.extend(
            meta.order
                .iter()
                .map(|handle| self.ecs.get::<Entity>(*handle).unwrap().id),
        );
    }
    pub fn binary_search_by_key<B: Ord>(
        &self,
        b: &B,
        mut f: impl FnMut(&Entity) -> B,
    ) -> Result<usize, usize> {
        self.meta
            .order
            .binary_search_by_key(b, |id| f(self.ecs.get(*id).expect("live actor handle")))
    }
    pub fn sort_unstable_by_key<K: Ord>(&mut self, mut f: impl FnMut(&Entity) -> K) {
        self.meta
            .order
            .sort_unstable_by_key(|id| f(self.ecs.get(*id).expect("live actor handle")));
        self.rebuild_gameplay_ids();
    }
    pub fn retain(&mut self, mut keep: impl FnMut(&Entity) -> bool) {
        self.meta.order.retain(|id| {
            if keep(self.ecs.get(*id).expect("live actor handle")) {
                true
            } else {
                self.ecs.despawn(*id);
                false
            }
        });
        self.rebuild_gameplay_ids();
    }
    pub fn truncate(&mut self, len: usize) {
        self.meta.gameplay_ids.truncate(len);
        let start = len.min(self.len());
        for id in self.meta.order.drain(start..) {
            self.ecs.despawn(id);
        }
    }
}

/// Dead heroes and buildings remain addressable. Minions and neutral units
/// leave the ordered index before their ECS entities are despawned.
pub(super) fn remove_defeated_units(
    actors: bevy_ecs::prelude::Query<&Entity>,
    mut meta: bevy_ecs::prelude::ResMut<ActorMetadata>,
    mut commands: bevy_ecs::prelude::Commands,
) {
    let survives = |actor: &Entity| {
        !matches!(actor.kind, super::Kind::Minion | super::Kind::Neutral) || actor.hp > 0.
    };
    if actors.iter().all(survives) {
        return;
    }
    let ActorMetadata {
        order,
        gameplay_ids,
        ..
    } = &mut *meta;
    gameplay_ids.clear();
    order.retain(|&id| {
        let actor = actors.get(id).expect("live actor handle");
        if survives(actor) {
            gameplay_ids.push(actor.id);
            true
        } else {
            commands.entity(id).despawn();
            false
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::prelude::{Component, Query};

    #[derive(Component)]
    struct Marker;
    #[test]
    fn cleanup_keeps_dead_heroes_and_removes_defeated_units_from_both_indexes() {
        use bevy_ecs::system::RunSystemOnce;
        let mut world = crate::World::new(2, 10);
        let actors = &mut world.entities;
        actors.truncate(1);
        actors[0].hp = 0.;
        let hero = actors[0].id;
        for (id, kind, hp) in [
            (hero + 1, crate::Kind::Minion, 0.),
            (hero + 2, crate::Kind::Neutral, -1.),
            (hero + 3, crate::Kind::Minion, 1.),
        ] {
            let mut unit = actors[0].clone();
            (unit.id, unit.kind, unit.hp) = (id, kind, hp);
            actors.push(unit);
        }
        let removed = [actors.meta.order[1], actors.meta.order[2]];
        actors.with_scoped_schedule(|ecs| ecs.run_system_once(remove_defeated_units).unwrap());
        assert_eq!(actors.meta.gameplay_ids, [hero, hero + 3]);
        assert_eq!(
            actors.iter().map(|e| e.id).collect::<Vec<_>>(),
            [hero, hero + 3]
        );
        assert!(
            removed
                .into_iter()
                .all(|handle| actors.ecs.get::<Entity>(handle).is_none())
        );
    }

    #[test]
    fn gameplay_index_checks_public_edits_sorted_rebuild_and_generation_reuse() {
        let mut world = crate::World::new(7, 2);
        let actors = &mut world.entities;
        actors.truncate(4);
        actors[1].id = 3;
        assert_eq!(actors.gameplay_index(2), None);
        assert_eq!(actors.gameplay_index(3), Some(2));
        actors[3].id = 9;
        assert_eq!(actors.gameplay_index(9), Some(3));
        actors[0].id = 7;
        actors.sort_unstable_by_key(|e| e.id);
        assert_eq!(actors.gameplay_index(7), Some(2));
        actors.retain(|e| e.id != 3);
        let old = actors.meta.order[1];
        let e = actors[1].clone();
        actors.truncate(1);
        actors.push(e);
        assert_ne!(actors.meta.order[1], old);
        assert_eq!(actors.gameplay_index(9), Some(1));
    }
    #[test]
    fn unordered_scan_observes_archetype_changes_and_current_actor_values() {
        let mut room = crate::World::new(7, 4);
        let handle = room.entities.meta.order[1];
        let actor_id = room.entities[1].id;
        fn compare(actors: &Actors<'_>) {
            let mut actual = Vec::new();
            actors.any_unordered(|a| {
                actual.push((a.id, a.hp.to_bits()));
                false
            });
            actual.sort_unstable_by_key(|a| a.0);
            let mut expected: Vec<_> = actors.iter().map(|a| (a.id, a.hp.to_bits())).collect();
            expected.sort_unstable_by_key(|a| a.0);
            assert_eq!(actual, expected);
        }
        compare(&room.entities);
        room.entities.ecs.entity_mut(handle).insert(Marker);
        room.entities[1].hp = 37.;
        compare(&room.entities);
        let mut next = room.entities[1].clone();
        next.id = u32::MAX;
        room.entities.push(next);
        room.entities[1].hp = 0.;
        compare(&room.entities);
        room.entities.retain(|e| e.id != actor_id);
        compare(&room.entities);
    }
    #[test]
    fn scoped_schedule_updates_authoritative_actor_and_preserves_external_order() {
        let mut room = crate::World::new(7, 4);
        let ids: Vec<_> = room.entities.iter().map(|e| e.id).collect();
        let old = room.entities.meta.order[1];
        room.entities.with_scoped_schedule(|ecs| {
            let mut q = ecs.query::<&Entity>();
            assert_eq!(q.iter(ecs).count(), ids.len());
            ecs.entity_mut(old).insert(Marker);
            ecs.run_system_cached(
                |mut q: Query<&mut Entity, bevy_ecs::prelude::With<Marker>>| {
                    q.single_mut().unwrap().hp = 37.;
                },
            )
            .unwrap();
            Actors::scope(ecs, |mut actors| {
                assert_eq!(actors[1].hp, 37.);
                actors[1].hp = 38.;
                assert_eq!(actors.iter().map(|e| e.id).collect::<Vec<_>>(), ids);
                actors.retain(|e| e.id != ids[0]);
            });
        });
        assert_eq!(room.entities[0].hp, 38.);
        assert_eq!(room.entities.meta.order[0], old);
        assert_eq!(
            room.entities.iter().map(|e| e.id).collect::<Vec<_>>(),
            ids[1..]
        );
    }

    #[test]
    fn targeting_memory_is_private_and_bound_to_the_living_actor() {
        use crate::{Command, DamageSchool, ImpactEffect, Kind, TICK_HZ, World, base};

        let mut world = World::new(3, 0);
        world.entities.truncate(2);
        let visible_point = Position { x: 6., y: 0. };
        world.entities[1].position = visible_point;
        world.tick = 3;
        world.step();
        let observer = world.entities.meta.order[0];
        let sighting = Some((2, visible_point, world.tick));
        assert_eq!(world.entities.last_seen(0), sighting);
        let mut observed = world.entities.ecs.query::<(&Entity, &LastSeenTarget)>();
        let (actor, memory) = observed.get(&world.entities.ecs, observer).unwrap();
        assert_eq!(
            (actor.id, memory.0, memory.1, memory.2),
            (1, 2, visible_point, world.tick)
        );
        let snapshot = serde_json::to_value(&world.entities).unwrap();
        assert!(snapshot[0].get("last_seen").is_none());

        world.entities[1].position = base(1);
        assert!(!world.visible(0, &world.entities[1]));
        assert_eq!(world.decide(0), Command::MoveTo(visible_point));
        world.tick += 2 * TICK_HZ + 1;
        assert_ne!(world.decide(0), Command::MoveTo(visible_point));
        // Expiration only affects decisions. Checkpoints retain the last observation.
        assert_eq!(world.entities.last_seen(0), sighting);

        let hp = world.entities[0].hp;
        world.apply_damage(
            2,
            1,
            0,
            hp,
            (DamageSchool::Weapon, ImpactEffect::Standard),
            &mut Vec::new(),
        );
        assert_eq!(world.entities.last_seen(0), None);
        assert!(observed.get(&world.entities.ecs, observer).is_err());

        // A reused ECS slot and gameplay ID cannot inherit an old component.
        world.entities.remember_target(1, 1, base(0), world.tick);
        let old = world.entities.meta.order[1];
        world.entities.truncate(1);
        world.next_id = 2;
        assert_eq!(world.spawn(1, Kind::Hero, 0, base(1), true), 2);
        assert_ne!(world.entities.meta.order[1], old);
        assert_eq!(world.entities.last_seen(1), None);
        assert!(world.entities.ecs.get::<LastSeenTarget>(old).is_none());
    }

    #[test]
    fn automated_room_keeps_pre_migration_checkpoint() {
        let mut world = crate::World::new(0, 0);
        for _ in 0..8_000 {
            world.step();
        }
        assert!((0..world.entities.len()).any(|index| world.entities.last_seen(index).is_some()));
        assert_eq!(
            world.state_digest(),
            "5e6afe4771a219ccf220b3f33df28b565ddba8c4354a0f3b860be18a11550706"
        );
    }

    #[test]
    fn scoped_schedule_restores_index_after_unwind() {
        let mut room = crate::World::new(2, 4);
        let id = room.entities[0].id;
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            room.entities
                .with_scoped_schedule(|ecs| Actors::scope(ecs, |_| panic!("injected")))
        }));
        assert!(panic.is_err());
        assert_eq!(room.entities[0].id, id);
    }
}
