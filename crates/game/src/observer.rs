use super::*;
use halcyon_protocol::{MatchCipher, parse_hello};
use std::{io, time::Duration};
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
    time::timeout,
};

pub(super) struct Observer {
    view: tower_aggro::View,
    scene: Option<tower_aggro::ObserverScene>,
    exit: Option<halcyon_platform::SpectatorExit>,
    viewer: Uuid,
    address: String,
    stop: watch::Sender<bool>,
    live: Option<mpsc::Sender<Batch>>,
    expires: Instant,
}
impl Rooms {
    pub(super) fn observe(&self, id: Uuid, viewer: Uuid) -> Result<(String, Uuid), PlatformError> {
        self.observe_with_exit(id, viewer, None)
    }
    pub(crate) fn observe_with_exit(
        &self,
        id: Uuid,
        viewer: Uuid,
        exit: Option<halcyon_platform::SpectatorExit>,
    ) -> Result<(String, Uuid), PlatformError> {
        let room = self.room(id).ok_or(PlatformError::HostUnavailable)?;
        let mut r = room.lock().unwrap();
        if r.phase != crate::RoomPhase::Running || r.players.contains(&viewer) {
            return Err(PlatformError::Conflict("room unavailable for spectating"));
        }
        if let Some((grant, observer)) = r.observers.iter_mut().find(|(_, o)| o.viewer == viewer) {
            if exit.is_some() {
                observer.exit = exit;
            }
            return Ok((observer.address.clone(), *grant));
        }
        if r.observers.len() >= 16 {
            return Err(PlatformError::HostUnavailable);
        }
        let rt =
            tokio::runtime::Handle::try_current().map_err(|_| PlatformError::HostUnavailable)?;
        let listener = self
            .0
            .ports
            .bind(self.0.bind_ip)
            .map_err(|_| PlatformError::HostUnavailable)?;
        listener
            .set_nonblocking(true)
            .map_err(|_| PlatformError::HostUnavailable)?;
        let port = listener
            .local_addr()
            .map_err(|_| PlatformError::HostUnavailable)?
            .port();
        let address = self.advertise(port);
        let listener =
            TcpListener::from_std(listener).map_err(|_| PlatformError::HostUnavailable)?;
        let grant = Uuid::new_v4();
        let (stop, mut revoked) = watch::channel(false);
        let mut ended = r.stop.subscribe();
        r.observers.insert(
            grant,
            Observer {
                view: Default::default(),
                scene: None,
                exit,
                viewer,
                address: address.clone(),
                stop,
                live: None,
                expires: Instant::now() + Duration::from_secs(60),
            },
        );
        drop(r);
        let m = self.clone();
        rt.spawn(async move {
            // A dedicated endpoint accepts one active read-only stream per grant.
            let mut clients = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    _=ended.changed()=>break,
                    _=revoked.changed()=>break,
                    res=listener.accept()=>match res{
                        Ok((sock,_)) if clients.len()<4=>{clients.spawn(client(sock,room.clone(),m.clone(),grant));},
                        Ok(_)=>{},Err(_)=>break,
                    },
                    Some(_)=clients.join_next(),if !clients.is_empty()=>{},
                }
            }
            drop(listener);
            let completed=room.lock().unwrap().reason==Some(crate::RoomEndReason::Completed);
            tokio::select! {
                biased;
                _=revoked.wait_for(|cancel|*cancel)=>{},
                _=transport::close_clients(&mut clients,completed)=>{},
            }
            // Dropping the JoinSet also cancels clients when explicitly revoked.
        });
        Ok((address, grant))
    }
    pub(super) fn revoke_observer(&self, id: Uuid, grant: Uuid) -> Result<(), PlatformError> {
        if let Some(r) = self.room(id)
            && let Some(o) = r.lock().unwrap().observers.remove(&grant)
        {
            o.stop.send_replace(true);
        }
        Ok(())
    }
}
struct Attached(Shared, Uuid);
impl Drop for Attached {
    fn drop(&mut self) {
        if let Some(o) = self.0.lock().unwrap().observers.get_mut(&self.1) {
            o.live = None;
            o.expires = Instant::now() + Duration::from_secs(60);
        }
    }
}
async fn client(
    mut socket: TcpStream,
    room: Shared,
    manager: Rooms,
    grant: Uuid,
) -> io::Result<()> {
    let cipher = MatchCipher::new(&room.lock().unwrap().id.to_string());
    let mut ended = room.lock().unwrap().stop.subscribe();
    let mut handshake_stop = ended.clone();
    let mut frames = halcyon_protocol::async_frame::FrameReader::new();
    let handshake = timeout(Duration::from_secs(10), async {
        parse_hello(&frames.required(&mut socket).await?)?;
        transport::acknowledge(&mut socket, &manager.0).await?;
        let m = cipher.decode_owned(frames.required(&mut socket).await?.into())?;
        if !matches!(m.fields()?,Fields::PlayerUuid(s) if Uuid::parse_str(s)==Ok(grant)) {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        Ok::<_, io::Error>(())
    });
    tokio::select! {
        biased;
        _=handshake_stop.wait_for(|done|*done)=>return Ok(()),
        result=handshake=>result??,
    }
    let (tx, mut rx) = mpsc::channel(16);
    let (initial, mut revoked) = {
        let mut r = room.lock().unwrap();
        if r.phase != crate::RoomPhase::Running {
            return Err(io::ErrorKind::NotConnected.into());
        }
        let o = r
            .observers
            .get_mut(&grant)
            .ok_or(io::ErrorKind::PermissionDenied)?;
        if o.live.is_some() {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        let revoked = o.stop.subscribe();
        o.live = Some(tx);
        let mut b = vec![r.observer_snapshot()];
        let mut scene = tower_aggro::ObserverScene::new(&r);
        if scene.is_some() {
            // Seat-scoped setters address player-info actors, including defeated heroes.
            b.extend(super::scene::players(&r).map(|mut info| {
                super::messages::redact_player_info(&mut info.body);
                info
            }));
        }
        b.extend(r.spawn_view(None));
        b.extend(r.absent_players());
        let mut view = tower_aggro::View::default();
        if let Some(scene) = &mut scene {
            scene.prepare(
                &r.world,
                &r.picks,
                r.data.as_deref(),
                &mut view,
                &mut b,
                false,
            );
        }
        if matches!(scene, Some(tower_aggro::ObserverScene::Aral(_))) {
            view.observer_guard_links(&r.world, &mut b);
        } else {
            r.solo_structure_snapshot.append(&mut b);
        }
        let o = r.observers.get_mut(&grant).unwrap();
        o.scene = scene;
        o.view = view;
        (b, revoked)
    };
    let _attached = Attached(room.clone(), grant);
    let (mut rd, mut wr) = socket.into_split();
    let outgoing = async {
        let mut next = Some(initial);
        let mut bytes = Vec::new();
        while let Some(batch) = next {
            bytes.clear();
            for m in batch {
                cipher.encode_into(&m, &mut bytes)?;
            }
            transport::write(&mut wr, &manager.0, &bytes).await?;
            next = transport::next_delivery(&mut rx, &mut ended).await;
        }
        wr.shutdown().await
    };
    let incoming = async {
        let mut start = Instant::now();
        let mut count = 0;
        loop {
            if start.elapsed() >= Duration::from_secs(1) {
                start = Instant::now();
                count = 0;
            }
            count += 1;
            if count > 300 {
                return Err(io::ErrorKind::InvalidData.into());
            }

            let Some(mut raw) = frames.next(&mut rd).await? else {
                return Ok(());
            };
            let (opcode, body) = cipher.decrypt_in_place(&mut raw)?;
            // Every valid encrypted frame contains at least six body bytes, enough for
            // the clock float. Neither a heartbeat nor a clock changes observer state.
            if matches!(
                Opcode::try_from(opcode),
                Ok(Opcode::ClockSync | Opcode::Heartbeat)
            ) {
                continue;
            }
            if Message::is_spectator_exit_frame(opcode, body) {
                // Handshake already bound this socket to this grant. Keep the room
                // lock out of the home transaction. A transient write error retains
                // the grant and ordinary reconnect grace.
                let exit = room
                    .lock()
                    .unwrap()
                    .observers
                    .get(&grant)
                    .and_then(|o| o.exit.clone());
                if let Some(exit) = exit {
                    tokio::task::spawn_blocking(move || exit(grant))
                        .await
                        .map_err(io::Error::other)?
                        .map_err(io::Error::other)?;
                }
                let id = room.lock().unwrap().id;
                manager
                    .revoke_observer(id, grant)
                    .map_err(io::Error::other)?;
                return Ok(());
            }
            // No route from this socket to Room::receive or World::command.
            return Err(io::ErrorKind::PermissionDenied.into());
        }
    };
    tokio::pin!(outgoing);
    tokio::select! {
        biased;
        _=revoked.wait_for(|cancel|*cancel)=>return Ok(()),
        _=handshake_stop.wait_for(|done|*done)=>{},
        r=&mut outgoing=>return r,
        r=incoming=>return r,
    }
    if room.lock().unwrap().reason != Some(crate::RoomEndReason::Completed) {
        return Ok(());
    }
    tokio::select! {biased; _=revoked.wait_for(|cancel|*cancel)=>Ok(()),r=&mut outgoing=>r}
}
impl Room {
    fn observer_snapshot(&self) -> Message {
        if self.legacy_aral() {
            return super::aral::roster(self, 0., true);
        }
        // Observer rosters must not disclose the UUIDs used by player admission.
        let mut m = snapshot(&self.world, &self.players, &self.picks);
        for i in 0..self.players.len() {
            m.body[i * 161 + 105..i * 161 + 141].fill(0);
        }
        m
    }
    pub(super) fn tick_observers(&mut self, events: &[Event]) {
        self.observers.retain(|_, o| {
            if o.live.is_none() && Instant::now() >= o.expires {
                o.stop.send_replace(true);
                false
            } else {
                true
            }
        });
        if !self.observers.values().any(|o| o.live.is_some()) {
            return;
        }
        let mut out = vec![];
        let aral = self.legacy_aral();
        let postgame = crate::result_frames::aral_lost_crystal(aral, &self.world);
        let spawned = |actor| {
            events
                .iter()
                .any(|e| matches!(e,Event::Spawn{id} if *id==actor))
        };
        let native = aral || self.solo_scene() && self.seats[0].control_world;
        for ev in events {
            if native && let Some(m) = super::npc_attack::release(&self.world, ev, |_| true) {
                out.push(m);
                continue;
            }
            crate::result_frames::append_postgame(&mut out, ev, postgame);
            if super::hero_progress::append(&self.world, &mut out, ev, |id| !spawned(id)) {
                continue;
            }
            match *ev {
                Event::Move { id, position } => {
                    out.push(Message::position(wire_id(id), position.x, position.y))
                }
                Event::Spawn { id } => {
                    if let Some(e) = self.world.entities.iter().find(|e| e.id == id) {
                        out.push(super::point_move::full_spawn(
                            &self.world,
                            e,
                            &self.picks,
                            self.data.as_deref(),
                            aral,
                        ));
                        super::messages::append_inventory(&mut out, &self.world, e);
                    }
                }
                Event::Purchase(ref receipt) => {
                    let respawned = spawned(receipt.actor);
                    if aral && respawned {
                        continue;
                    }
                    super::messages::append_purchase(&mut out, &self.world, receipt, !respawned);
                }
                Event::GoldAward { id, amount } => {
                    if aral && spawned(id) {
                        continue;
                    }
                    out.push(Message::entity_stat(wire_id(id), amount as f32, 6))
                }
                _ => {
                    super::messages::append_combat(&mut out, ev, |_| true);
                    if aral && let Event::Death { id, .. } = ev {
                        super::messages::append_respawn_timer(&mut out, &self.world, *id);
                    }
                }
            }
        }
        if self.world.tick.is_multiple_of(20) {
            out.push(self.observer_snapshot());
        }
        self.observers.retain(|_, o| {
            let Some(tx) = &o.live else { return true };
            let mut batch = out.clone();
            if let Some(scene) = &mut o.scene {
                scene.prepare(
                    &self.world,
                    &self.picks,
                    self.data.as_deref(),
                    &mut o.view,
                    &mut batch,
                    true,
                );
            }
            batch.retain(|m| {
                super::npc_attack::actors(m).is_none()
                    || o.scene.is_some() && super::npc_attack::registered(&o.view, m)
            });
            if matches!(o.scene, Some(tower_aggro::ObserverScene::Aral(_))) {
                for ev in events {
                    if let Event::Death { id, .. } = ev
                        && let Some(slot) = self.world.aral_structure_slot(*id)
                    {
                        o.view.guard_death(slot, &mut batch);
                    }
                }
            }
            if !batch.is_empty() && tx.try_send(batch).is_err() {
                o.stop.send_replace(true);
                return false;
            }
            true
        });
    }
    pub(super) fn publish_observer_actor(&mut self, actor: u32, batch: &Batch) {
        self.publish_observer_actors(&[actor], batch);
    }
    pub(super) fn publish_observer_actors(&mut self, actors: &[u32], batch: &[Message]) {
        self.observers.retain(|_, o| {
            let Some(tx) = &o.live else { return true };
            if o.scene.is_some()
                && actors.iter().all(|&actor| o.view.knows(actor))
                && tx.try_send(batch.to_vec()).is_err()
            {
                o.stop.send_replace(true);
                return false;
            }
            true
        });
    }
    pub(super) fn publish_observer_approach(&mut self, id: u32, out: &Batch) {
        self.observers.retain(|_, o| {
            if o.scene.is_some()
                && o.view.knows(id)
                && let Some(tx) = &o.live
                && tx.try_send(out.clone()).is_err()
            {
                o.stop.send_replace(true);
                return false;
            }
            true
        });
    }
    pub(super) fn death_observers(&self) -> Vec<mpsc::WeakSender<Batch>> {
        self.observers
            .values()
            .filter_map(|o| o.live.as_ref().map(mpsc::Sender::downgrade))
            .collect()
    }
    pub(super) fn publish_witnessed_death(
        &mut self,
        observed: &[mpsc::WeakSender<Batch>],
        out: &[Message],
    ) {
        self.observers.retain(|_, o| {
            if let Some(tx) = &o.live
                && observed
                    .iter()
                    .any(|prior| prior.upgrade().is_some_and(|old| old.same_channel(tx)))
            {
                if tx.try_send(out.to_vec()).is_err() {
                    o.stop.send_replace(true);
                    return false;
                }
                if let Some(scene) = &mut o.scene {
                    scene.release(out);
                }
            }
            true
        });
    }
    pub(super) fn publish_observer_projectiles(&mut self, shots: &[super::npc_projectile::Shot]) {
        self.observers.retain(|_, observer| {
            let Some(tx) = &observer.live else {
                return true;
            };
            if observer.scene.is_none() {
                return true;
            }
            let out = super::npc_projectile::batch(shots, &observer.view, |_| true);
            if !out.is_empty() && tx.try_send(out).is_err() {
                observer.stop.send_replace(true);
                return false;
            }
            true
        });
    }
    pub(super) fn publish_observers(&mut self, out: &[Message]) {
        self.observers.retain(|_, o| {
            if o.live
                .as_ref()
                .is_some_and(|tx| tx.try_send(out.to_vec()).is_err())
            {
                o.stop.send_replace(true);
                false
            } else {
                true
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bulwark_release_reaches_a_registered_visible_opponent_and_registered_observer() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: id,
                players: (0..6).map(|_| Uuid::new_v4()).collect(),
                mode: "casual_aral".into(),
                teams: vec![0, 0, 1, 1, 0, 1],
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut r = shared.lock().unwrap();
        r.world.entities[0].type_id = halcyon_data::EntityTemplate::Lyra.as_raw();
        let mut caster = crate::messages::spawn(&r.world.entities[0], &r.picks, None);
        caster.body[745] = 16;
        let mut seats = Vec::new();
        for i in 0..4 {
            let (tx, rx) = mpsc::channel(2);
            r.seats[i].peers[1] = Some(Peer {
                generation: 1,
                tx,
                stalled: watch::channel(false).0,
            });
            if i < 3 {
                r.scene_views[i].declarations(std::slice::from_ref(&caster));
            }
            seats.push(rx);
        }
        let mut observers = Vec::new();
        for registered in [true, false] {
            let (tx, rx) = mpsc::channel(2);
            let mut view = tower_aggro::View::default();
            if registered {
                view.declarations(std::slice::from_ref(&caster));
            }
            let scene = tower_aggro::ObserverScene::new(&r);
            r.observers.insert(
                Uuid::new_v4(),
                Observer {
                    view,
                    scene,
                    exit: None,
                    viewer: Uuid::new_v4(),
                    address: String::new(),
                    stop: watch::channel(false).0,
                    live: Some(tx),
                    expires: Instant::now() + Duration::from_secs(60),
                },
            );
            observers.push(rx);
        }
        let cast = Event::Cast {
            source: 1,
            slot: halcyon_sim::AbilitySlot::B,
            target: halcyon_sim::CastTarget::SelfCast,
        };
        r.visible[1].insert(1);
        crate::native_abilities::publish_before_impacts(&mut r, std::slice::from_ref(&cast));
        let expected = vec![Message::ability_target_used(1500, u32::MAX, 2)];
        assert_eq!(seats[2].try_recv().unwrap().into_records(), expected);
        assert_eq!(observers[0].try_recv().unwrap(), expected);
        assert!(observers[1].try_recv().is_err());
        assert!(seats.iter_mut().all(|rx| rx.try_recv().is_err()));
        r.visible[1].remove(&1);
        crate::native_abilities::publish_before_impacts(&mut r, &[cast]);
        assert!(seats[2].try_recv().is_err());
    }

    #[tokio::test]
    async fn chill_winds_observers_need_declarations_for_each_referenced_actor() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: id,
                players: (0..6).map(|_| Uuid::new_v4()).collect(),
                mode: halcyon_protocol::ce::KnownQueue::CasualAral.as_str().into(),
                teams: vec![],
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut room = shared.lock().unwrap();
        room.world.entities[0].type_id = halcyon_data::EntityTemplate::Reim.as_raw();
        let mut caster = crate::messages::spawn(&room.world.entities[0], &room.picks, None);
        caster.body[745] = 16;
        let mut target = crate::messages::spawn(&room.world.entities[1], &room.picks, None);
        target.body[745] = 19;

        let mut viewers = Vec::new();
        for (ready, declared) in [
            (true, vec![caster.clone(), target.clone()]),
            (true, vec![caster.clone()]),
            (true, vec![target.clone()]),
            (false, vec![caster, target]),
        ] {
            let (tx, rx) = mpsc::channel(4);
            let mut view = tower_aggro::View::default();
            view.declarations(&declared);
            let scene = ready
                .then(|| tower_aggro::ObserverScene::new(&room))
                .flatten();
            room.observers.insert(
                Uuid::new_v4(),
                Observer {
                    view,
                    scene,
                    exit: None,
                    viewer: Uuid::new_v4(),
                    address: String::new(),
                    stop: watch::channel(false).0,
                    live: Some(tx),
                    expires: Instant::now() + Duration::from_secs(60),
                },
            );
            viewers.push(rx);
        }

        crate::native_abilities::publish_before_impacts(
            &mut room,
            &[Event::ChillWinds {
                source: 1,
                position: Position { x: 4., y: 2. },
                heading: Some(1.),
            }],
        );
        crate::native_abilities::publish(
            &mut room,
            &[Event::ChillWindsRoot {
                source: 1,
                target: 2,
                instance: 700,
            }],
        );

        for index in [0, 1] {
            let release = viewers[index].try_recv().unwrap();
            assert_eq!(release[0], Message::ability_target_used(1500, u32::MAX, 1));
            assert_eq!(release[1].play_pfx_at_location().unwrap().actor, 1500);
        }
        let root = viewers[0].try_recv().unwrap();
        let halcyon_protocol::Fields::BuffApplication(buff) = root[0].fields().unwrap() else {
            panic!("expected root application");
        };
        assert_eq!((buff.prefix.source, buff.prefix.target), (1500, 1501));
        assert!(viewers.iter_mut().all(|rx| rx.try_recv().is_err()));
    }

    #[tokio::test]
    async fn delayed_aral_lifecycle_is_timed_and_bound_to_original_world_streams() {
        use crate::aral_death;
        use halcyon_sim::Kind;
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: id,
                players: (0..6).map(|_| Uuid::new_v4()).collect(),
                teams: vec![0, 0, 1, 0, 1, 1],
                mode: "casual_aral".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        shared.lock().unwrap().phase(crate::RoomPhase::Running);
        let (_, grant) = rooms.observe(id, Uuid::new_v4()).unwrap();
        let (_, late) = rooms.observe(id, Uuid::new_v4()).unwrap();
        let mut room = shared.lock().unwrap();
        let mut streams = vec![];
        for (i, seat) in room.seats.iter_mut().take(3).enumerate() {
            let (tx, rx) = mpsc::channel(8);
            seat.peers[1] = Some(Peer {
                generation: i as u64 + 1,
                tx,
                stalled: watch::channel(false).0,
            });
            streams.push(rx);
        }
        let (observer, mut watched) = mpsc::channel(8);
        room.observers.get_mut(&grant).unwrap().live = Some(observer);
        for _ in 0..15 * halcyon_sim::TICK_HZ {
            room.world.step();
        }
        let minion = room
            .world
            .entities
            .iter()
            .find(|e| e.kind == Kind::Minion && e.team == 0)
            .unwrap()
            .id;
        let tower = room
            .world
            .entities
            .iter()
            .find(|e| e.kind == Kind::Tower)
            .unwrap()
            .id;
        room.visible[0].extend([1, 2, minion, tower]);
        room.world.entities[1].hp = 0.;
        room.world.entities.retain(|e| e.id != minion);
        let events = [
            Event::Death { id: 2, killer: 1 },
            Event::Death {
                id: minion,
                killer: 1,
            },
            Event::Death {
                id: tower,
                killer: 1,
            },
        ];
        aral_death::schedule(&mut room, &events);
        room.tick_observers(&events);
        room.replicate(&events);
        let field = |m: &Message| u32::from_be_bytes(m.body[..4].try_into().unwrap());
        let deaths = |b: &[Message]| {
            b.iter()
                .filter(|m| m.is(crate::Opcode::ActorDeath))
                .map(field)
                .collect::<Vec<_>>()
        };
        assert!(deaths(&watched.try_recv().unwrap()).contains(&u32::from(wire_id(minion))));
        assert!(deaths(&streams[0].try_recv().unwrap().into_records()).contains(&1501));
        for stream in &mut streams {
            while stream.try_recv().is_ok() {}
        }
        assert_eq!(room.dead_actors.len(), 2);
        let (late_observer, mut unwatched) = mpsc::channel(8);
        room.observers.get_mut(&late).unwrap().live = Some(late_observer);
        let (replacement, mut reconnected) = mpsc::channel(8);
        room.seats[1].peers[1] = Some(Peer {
            generation: 90,
            tx: replacement,
            stalled: watch::channel(false).0,
        });
        let now = room.world.tick;
        aral_death::flush(&mut room, now + 35);
        assert!(streams[0].try_recv().is_err());
        aral_death::flush(&mut room, now + 36);
        let check = |m: &[Message], expected: &[u16], actor: u32| {
            assert_eq!(m.iter().map(|p| p.opcode).collect::<Vec<_>>(), expected);
            assert!(m.iter().all(|p| field(p) == actor && p.body[4..] == [0; 2]));
        };
        check(
            &watched.try_recv().unwrap(),
            &[crate::wire_opcode::ActorDead],
            1501,
        );
        check(
            &streams[0].try_recv().unwrap().into_records(),
            &[crate::wire_opcode::ActorDead],
            1501,
        );
        room.observers.remove(&grant);
        aral_death::flush(&mut room, now + 75);
        assert!(streams[0].try_recv().is_err());
        aral_death::flush(&mut room, now + 76);
        check(
            &streams[0].try_recv().unwrap().into_records(),
            &[
                crate::wire_opcode::ActorDead,
                crate::wire_opcode::DestroyEntity,
            ],
            u32::from(wire_id(minion)),
        );
        assert!(room.dead_actors.is_empty());
        assert!(watched.try_recv().is_err());
        assert!(unwatched.try_recv().is_err());
        assert!(reconnected.try_recv().is_err());
        assert!(streams[2].try_recv().is_err());
    }

    #[tokio::test]
    async fn revoked_observer_receives_no_further_native_abilities() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: id,
                players: vec![Uuid::new_v4()],
                teams: vec![0],
                mode: "solo".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        shared.lock().unwrap().phase(crate::RoomPhase::Running);
        let (_, grant) = rooms.observe(id, Uuid::new_v4()).unwrap();
        let (tx, mut rx) = mpsc::channel(4);
        let cast = Event::Cast {
            source: 2,
            slot: halcyon_sim::AbilitySlot::B,
            target: halcyon_sim::CastTarget::Point(halcyon_sim::Position { x: 2., y: 3. }),
        };
        {
            let mut room = shared.lock().unwrap();
            room.observers.get_mut(&grant).unwrap().live = Some(tx);
            room.world.entities[1].type_id = 273;
            super::super::native_abilities::publish(&mut room, std::slice::from_ref(&cast));
        }
        assert_eq!(
            rx.try_recv().unwrap(),
            [Message::ability_point_used(1501, [2., 0., 3.], 3)]
        );
        rooms.revoke_observer(id, grant).unwrap();
        super::super::native_abilities::publish(&mut shared.lock().unwrap(), &[cast]);
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn aral_respawn_timer_respects_known_actors_and_authorized_observers() {
        use crate::{Peer, Rooms};
        use halcyon_platform::{MatchHost, Reservation};
        use halcyon_sim::{Command, Position};
        use tokio::sync::{mpsc, watch};

        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let match_id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id,
                players: (0..6).map(|_| Uuid::new_v4()).collect(),
                teams: vec![],
                mode: "casual_aral".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(match_id).unwrap();
        shared.lock().unwrap().phase(crate::RoomPhase::Running);
        let (_, grant) = rooms.observe(match_id, Uuid::new_v4()).unwrap();
        let mut room = shared.lock().unwrap();
        let (view, mut observer) = mpsc::channel(8);
        room.observers.get_mut(&grant).unwrap().live = Some(view);
        let mut streams = Vec::new();
        for (i, seat) in room.seats.iter_mut().take(2).enumerate() {
            let (tx, rx) = mpsc::channel(8);
            seat.peers[1] = Some(Peer {
                generation: i as u64 + 1,
                tx,
                stalled: watch::channel(false).0,
            });
            streams.push(rx);
        }
        room.world.entities[0].position = Position { x: 0., y: 0. };
        room.world.entities[1].position = Position { x: 3., y: 0. };
        room.world.entities[1].hp = 0.1;
        room.world.command(1, Command::Attack(2)).unwrap();
        let events = (0..40)
            .find_map(|_| {
                let events = room.world.step();
                events
                    .iter()
                    .any(|event| matches!(event, Event::Death { id: 2, .. }))
                    .then_some(events)
            })
            .expect("hero dies from an authoritative attack");
        let seconds = room.world.respawn_seconds(2).unwrap();
        let expected = Message::actor_respawn_timer(1501, seconds);

        // The victim's side knows its own hero but the killer is now outside its fog.
        // The killer's side had visibility of both references when the victim fell.
        room.visible[0].extend([1, 2]);
        room.visible[1].insert(2);
        room.replicate(&events);
        let visible_enemy = streams[0].try_recv().unwrap().into_records();
        let death_index = visible_enemy
            .iter()
            .position(|m| m.is(crate::Opcode::ActorDeath))
            .unwrap();
        let timer_index = visible_enemy.iter().position(|m| m == &expected).unwrap();
        assert!(death_index < timer_index);
        assert_eq!(visible_enemy.iter().filter(|m| **m == expected).count(), 1);
        let friendly = streams[1].try_recv().unwrap().into_records();
        assert!(friendly.iter().all(|m| !m.is(crate::Opcode::ActorDeath)));
        assert_eq!(friendly.iter().filter(|m| **m == expected).count(), 1);

        room.tick_observers(&events);
        let observed = observer.try_recv().unwrap();
        assert_eq!(observed.iter().filter(|m| **m == expected).count(), 1);
        room.replicate(&[]);
        assert!(streams[0].try_recv().is_err());

        // A team never told about this death gets no timer for the enemy.
        room.visible[0].remove(&2);
        room.replicate(&events);
        while let Ok(batch) = streams[0].try_recv() {
            assert!(
                batch
                    .into_records()
                    .iter()
                    .all(|m| !m.is(crate::Opcode::ActorRespawnTimer))
            );
        }

        // Use the native, confirmed gameplay stream for the relocation presentation.
        for (i, seat) in room.seats.iter_mut().take(2).enumerate() {
            seat.peers[0] = seat.peers[1].take();
            seat.world_actor_written = true;
            let actor = 1500 + i as u32;
            seat.preamble
                .observe(&Message::known(crate::Opcode::RequestScene, [0; 6]), actor);
            let mut request = [0; 6];
            request[..4].copy_from_slice(&actor.to_be_bytes());
            request[4] = 1;
            seat.preamble
                .observe(&Message::known(crate::Opcode::SceneOwner, request), actor);
            seat.preamble.observe(
                &Message::known(crate::Opcode::SceneConfirmed, [0; 6]),
                actor,
            );
        }
        // Advance the same dead hero through its normal countdown.
        let declaration = {
            let mut b = vec![0; 750];
            b[8..12].copy_from_slice(&1501u32.to_be_bytes());
            b[745] = 17;
            Message::known(crate::Opcode::EntitySpawn, b)
        };
        for view in &mut room.scene_views[..2] {
            view.declarations(std::slice::from_ref(&declaration));
        }
        let scene = tower_aggro::ObserverScene::new(&room);
        let o = room.observers.get_mut(&grant).unwrap();
        o.scene = scene;
        o.view.declarations(std::slice::from_ref(&declaration));
        for stream in &mut streams {
            while stream.try_recv().is_ok() {}
        }
        let mut teleported = None;
        while room.world.entities[1].hp <= 0. {
            let events = room.world.step();
            if let Some(Event::RespawnApproach { home, .. }) = events
                .iter()
                .find(|e| matches!(e, Event::RespawnApproach { id: 2, .. }))
            {
                assert!(teleported.replace(room.world.tick).is_none());
                assert_eq!(room.world.entities[1].hp, 0.);
                assert_ne!(room.world.entities[1].position, *home);
                let expected = vec![
                    Message::actor_relocation(1501, [home.x, 1.5, home.y], 1),
                    Message::position(1501, home.x, home.y),
                ];
                crate::aral_death::publish_approaches(&mut room, &events);
                assert_eq!(streams[1].try_recv().unwrap().into_records(), expected);
                assert!(streams[0].try_recv().is_err());
                assert_eq!(observer.try_recv().unwrap(), expected);
                room.world.entities[0].position = *home;
                room.scene_views[1] = Default::default();
                crate::aral_death::publish_approaches(&mut room, &events);
                assert_eq!(streams[0].try_recv().unwrap().into_records(), expected);
                assert!(streams[1].try_recv().is_err());
            }
            if events
                .iter()
                .any(|e| matches!(e, Event::Respawn { id: 2, .. }))
            {
                assert_eq!(teleported.unwrap() + 6, room.world.tick);
            }
        }
        assert!(teleported.is_some());
    }

    #[tokio::test]
    async fn observer_respawn_and_purchase_emit_one_grant_and_actual_currency() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: id,
                players: vec![Uuid::new_v4()],
                teams: vec![0],
                mode: "5v5".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        shared.lock().unwrap().phase(crate::RoomPhase::Running);
        let (_, grant) = rooms.observe(id, Uuid::new_v4()).unwrap();
        let (tx, mut rx) = mpsc::channel(1);
        {
            let mut r = shared.lock().unwrap();
            r.world.tick = 1;
            r.observers.get_mut(&grant).unwrap().live = Some(tx);
            r.world.entities[1].gold = 300;
            let receipt = r
                .world
                .purchase(2, halcyon_sim::BasicItem::WeaponBlade)
                .unwrap();
            r.tick_observers(&[
                Event::Spawn { id: 2 },
                Event::Purchase(receipt.clone()),
                Event::GoldAward { id: 2, amount: 17 },
            ]);
            let batch = rx.try_recv().unwrap();
            assert_eq!(
                batch.iter().map(|m| m.opcode).collect::<Vec<_>>(),
                [
                    crate::wire_opcode::EntitySpawn,
                    crate::wire_opcode::ItemGrant,
                    crate::wire_opcode::EntityStat,
                    crate::wire_opcode::EntityStat
                ]
            );
            assert!(
                matches!(batch[1].fields().unwrap(),Fields::ItemGrant{instance,..} if instance==receipt.granted.instance)
            );
            for (message, delta) in batch[2..].iter().zip([-300., 17.]) {
                assert!(
                    matches!(message.fields().unwrap(),Fields::EntityStat{stat:6,value,..} if value==delta)
                );
            }
        }
        rooms.revoke_observer(id, grant).unwrap();
        assert!(rx.recv().await.is_none());
    }
}
