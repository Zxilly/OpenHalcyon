use super::*;
use halcyon_protocol::{ACK_FRAME, MatchCipher, parse_hello};
use std::{io, time::Duration};
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
    time::timeout,
};

pub(super) struct Observer {
    viewer: Uuid,
    address: String,
    stop: watch::Sender<bool>,
    live: Option<mpsc::Sender<Batch>>,
    expires: Instant,
}
impl Rooms {
    pub(super) fn observe(&self, id: Uuid, viewer: Uuid) -> Result<(String, Uuid), PlatformError> {
        let room = self.room(id).ok_or(PlatformError::HostUnavailable)?;
        let mut r = room.lock().unwrap();
        if r.phase != "running" || r.players.contains(&viewer) {
            return Err(PlatformError::Conflict("room unavailable for spectating"));
        }
        if let Some((grant, observer)) = r.observers.iter().find(|(_, o)| o.viewer == viewer) {
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
            let completed=room.lock().unwrap().reason==Some("completed");
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
    let handshake = timeout(Duration::from_secs(10), async {
        parse_hello(&transport::frame(&mut socket).await?)?;
        transport::write(&mut socket, &manager.0, &ACK_FRAME).await?;
        let m = cipher.decode_owned(transport::frame(&mut socket).await?)?;
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
        if r.phase != "running" {
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
        b.extend(r.spawn_view(None));
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

            let m = cipher.decode_owned(transport::frame(&mut rd).await?)?;
            // No route from this socket to Room::receive or World::command.
            if !matches!(m.fields()?, Fields::Clock(_) | Fields::Heartbeat) {
                return Err(io::ErrorKind::PermissionDenied.into());
            }
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
    if room.lock().unwrap().reason != Some("completed") {
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
        for ev in events {
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
        if out.is_empty() {
            return;
        }
        self.publish_observers(&out);
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
    async fn revoked_observer_receives_no_further_native_abilities() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                match_id: id,
                players: vec![Uuid::new_v4()],
                teams: vec![0],
                mode: "solo".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        shared.lock().unwrap().phase("running");
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
                match_id,
                players: (0..6).map(|_| Uuid::new_v4()).collect(),
                teams: vec![],
                mode: "casual_aral".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(match_id).unwrap();
        shared.lock().unwrap().phase("running");
        let (_, grant) = rooms.observe(match_id, Uuid::new_v4()).unwrap();
        let mut room = shared.lock().unwrap();
        let (view, mut observer) = mpsc::channel(8);
        room.observers.get_mut(&grant).unwrap().live = Some(view);
        let mut streams = Vec::new();
        for i in 0..2 {
            let (tx, rx) = mpsc::channel(8);
            room.seats[i].peers[1] = Some(Peer {
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
            .position(|m| m.opcode == 0x0430)
            .unwrap();
        let timer_index = visible_enemy.iter().position(|m| m == &expected).unwrap();
        assert!(death_index < timer_index);
        assert_eq!(visible_enemy.iter().filter(|m| **m == expected).count(), 1);
        let friendly = streams[1].try_recv().unwrap().into_records();
        assert!(friendly.iter().all(|m| m.opcode != 0x0430));
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
            assert!(batch.into_records().iter().all(|m| m.opcode != 0x0433));
        }
    }

    #[tokio::test]
    async fn observer_respawn_and_purchase_emit_one_grant_and_actual_currency() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                match_id: id,
                players: vec![Uuid::new_v4()],
                teams: vec![0],
                mode: "5v5".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        shared.lock().unwrap().phase("running");
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
                [0x3f3, 0x43d, 0x41d, 0x41d]
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
