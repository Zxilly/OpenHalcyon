use bevy_ecs::{entity::Entity, prelude::Component, query::QueryState, world::World};
use std::collections::HashMap;
use uuid::Uuid;

use super::Shared;

/// Owns the room's shared handle. The room task and authenticated connections
/// hold clones of the same handle across registry entity generations.
#[derive(Component)]
struct HostedRoom {
    id: Uuid,
    room: Shared,
}

/// Match UUIDs are externally durable identities. Bevy entities are process-local.
pub(super) struct RoomRegistry {
    world: World,
    by_match: HashMap<Uuid, Entity>,
    hosted_rooms: QueryState<&'static HostedRoom>,
}

impl Default for RoomRegistry {
    fn default() -> Self {
        let mut world = World::new();
        let hosted_rooms = world.query();
        Self {
            world,
            by_match: HashMap::new(),
            hosted_rooms,
        }
    }
}

impl RoomRegistry {
    pub(super) fn get(&self, id: &Uuid) -> Option<&Shared> {
        let entity = *self.by_match.get(id)?;
        self.world
            .get::<HostedRoom>(entity)
            .map(|hosted| &hosted.room)
    }

    pub(super) fn values(&mut self) -> impl Iterator<Item = &Shared> {
        self.hosted_rooms
            .iter(&self.world)
            .map(|hosted| &hosted.room)
    }

    pub(super) fn handles(&mut self) -> Vec<Shared> {
        self.values().cloned().collect()
    }

    pub(super) fn insert(&mut self, id: Uuid, room: Shared) {
        assert!(!self.by_match.contains_key(&id), "duplicate room identity");
        let entity = self.world.spawn(HostedRoom { id, room }).id();
        self.by_match.insert(id, entity);
    }

    pub(super) fn remove(&mut self, id: &Uuid) {
        if let Some(entity) = self.by_match.remove(id) {
            let existed = self.world.despawn(entity);
            debug_assert!(existed);
        }
    }

    pub(super) fn retain(&mut self, mut keep: impl FnMut(&Shared) -> bool) {
        let expired: Vec<_> = self
            .hosted_rooms
            .iter(&self.world)
            .filter(|hosted| !keep(&hosted.room))
            .map(|hosted| hosted.id)
            .collect();
        for id in expired {
            self.remove(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rooms;
    use halcyon_types::{HostError, MatchHost, Reservation};
    use std::sync::Arc;

    fn solo(id: Uuid, player: Uuid) -> Reservation {
        Reservation {
            match_id: id,
            players: vec![player],
            mode: halcyon_protocol::ce::KnownQueue::SoloBots.as_str().into(),
            teams: vec![],
            talent_levels: vec![],
            difficulty: None,
        }
    }

    #[tokio::test]
    async fn capacity_and_stable_room_handles_survive_entity_reuse() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let (first, second, player) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let staged = rooms.prepare(solo(first, player)).unwrap();
        let handle = rooms.room(first).unwrap();
        assert_eq!(
            rooms.prepare(solo(first, player)).unwrap().address,
            staged.address
        );
        assert_eq!(rooms.hosting_counts(), (1, 0, 0, 10));
        assert!(matches!(
            rooms.prepare(solo(second, player)),
            Err(HostError::HostUnavailable)
        ));
        rooms.discard(first).unwrap();
        assert!(*handle.lock().unwrap().stop.borrow());
        assert!(rooms.room(first).is_none());
        assert_eq!(rooms.hosting_counts(), (0, 0, 0, 0));

        let hosted = rooms.reserve(solo(second, player)).unwrap();
        let replacement = rooms.room(second).unwrap();
        assert!(!Arc::ptr_eq(&handle, &replacement));
        assert_eq!(
            rooms.reserve(solo(second, player)).unwrap().address,
            hosted.address
        );
        assert!(rooms.room(first).is_none());
        assert_eq!(rooms.hosting_counts(), (1, 1, 9, 0));
    }
}
