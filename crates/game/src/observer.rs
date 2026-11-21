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
    exit: Option<SpectatorExit>,
    viewer: Uuid,
    address: String,
    stop: watch::Sender<bool>,
    pub(super) live: Option<mpsc::Sender<Batch>>,
    expires: Instant,
}
impl Rooms {
    pub(super) fn observe(&self, id: Uuid, viewer: Uuid) -> HostResult<(String, Uuid)> {
        self.observe_with_exit(id, viewer, None)
    }
    pub(crate) fn observe_with_exit(
        &self,
        id: Uuid,
        viewer: Uuid,
        exit: Option<SpectatorExit>,
    ) -> HostResult<(String, Uuid)> {
        let room = self.room(id).ok_or(HostError::HostUnavailable)?;
        let mut r = room.lock().unwrap();
        if r.phase != crate::RoomPhase::Running || r.players.contains(&viewer) {
            return Err(HostError::Conflict("room unavailable for spectating"));
        }
        if let Some((grant, observer)) = r.observers.iter_mut().find(|(_, o)| o.viewer == viewer) {
            if exit.is_some() {
                observer.exit = exit;
            }
            return Ok((observer.address.clone(), *grant));
        }
        if r.observers.len() >= 16 {
            return Err(HostError::HostUnavailable);
        }
        let rt = tokio::runtime::Handle::try_current().map_err(|_| HostError::HostUnavailable)?;
        let listener = self
            .0
            .ports
            .bind(self.0.bind_ip)
            .map_err(|_| HostError::HostUnavailable)?;
        listener
            .set_nonblocking(true)
            .map_err(|_| HostError::HostUnavailable)?;
        let port = listener
            .local_addr()
            .map_err(|_| HostError::HostUnavailable)?
            .port();
        let address = self.advertise(port);
        let listener = TcpListener::from_std(listener).map_err(|_| HostError::HostUnavailable)?;
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
            let completed = room.lock().unwrap().reason == Some(crate::RoomEndReason::Completed);
            let mut settled = room.lock().unwrap().settled.subscribe();
            tokio::select! {
                biased;
                _=revoked.wait_for(|cancel|*cancel)=>{},
                _=async {
                    if completed {
                        let _ = settled.wait_for(|done| *done).await;
                    }
                    transport::close_clients(&mut clients,completed).await;
                }=>{},
            }
            // Dropping the JoinSet also cancels clients when explicitly revoked.
        });
        Ok((address, grant))
    }
    pub(super) fn revoke_observer(&self, id: Uuid, grant: Uuid) -> HostResult<()> {
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
    let (cipher, mut handshake_stop, mut ended) = {
        let room = room.lock().unwrap();
        (
            MatchCipher::new(&room.id.to_string()),
            room.stop.subscribe(),
            room.settled.subscribe(),
        )
    };
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
                Ok(Opcode::ClockSync | Opcode::ServerHelloAck)
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
        if let Ok(mut roster) = halcyon_protocol::DraftRosterBody::read_body(&m.body) {
            roster.redact_identities(self.players.len());
            let _ = roster.write_body(&mut m.body);
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
    /// Project a native batch against each connected observer's own active scene.
    /// Projectors may update registration state only after those checks succeed.
    pub(super) fn project_observer_scenes(
        &mut self,
        project: impl Fn(&mut tower_aggro::View) -> Option<Batch>,
    ) {
        self.observers.retain(|_, observer| {
            if let Some(tx) = &observer.live
                && observer.scene.is_some()
                && let Some(batch) = project(&mut observer.view)
                && tx.try_send(batch).is_err()
            {
                observer.stop.send_replace(true);
                return false;
            }
            true
        });
    }
    pub(super) fn publish_observer_actor_if(
        &mut self,
        actor: u32,
        batch: &[Message],
        eligible: impl Fn(&mut tower_aggro::View) -> bool,
    ) {
        self.project_observer_scenes(|view| {
            (view.knows(actor) && eligible(view)).then(|| batch.to_vec())
        });
    }
    pub(super) fn publish_observer_actor(&mut self, actor: u32, batch: &[Message]) {
        self.publish_observer_actors(&[actor], batch);
    }
    pub(super) fn publish_observer_actors(&mut self, actors: &[u32], batch: &[Message]) {
        self.project_observer_scenes(|view| {
            actors
                .iter()
                .all(|&actor| view.knows(actor))
                .then(|| batch.to_vec())
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
        self.project_observer_scenes(|view| {
            let batch = super::npc_projectile::batch(shots, view, |_| true);
            (!batch.is_empty()).then_some(batch)
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
#[path = "observer_tests.rs"]
mod tests;
