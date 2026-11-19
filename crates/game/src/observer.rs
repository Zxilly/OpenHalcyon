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
    live: Option<Sink>,
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
            clients.shutdown().await;
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
    timeout(Duration::from_secs(10), async {
        parse_hello(&transport::frame(&mut socket).await?)?;
        transport::write(&mut socket, &manager.0, &ACK_FRAME).await?;
        let m = cipher.decode_owned(transport::frame(&mut socket).await?)?;
        if !matches!(m.fields()?,Fields::PlayerUuid(s) if Uuid::parse_str(s)==Ok(grant)) {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        Ok::<_, io::Error>(())
    })
    .await??;
    let (tx, mut rx) = mpsc::channel(16);
    let initial = {
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
        o.live = Some(tx);
        let mut b = vec![r.observer_snapshot()];
        b.extend(r.spawn_view(None));
        b
    };
    let _attached = Attached(room, grant);
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
            next = rx.recv().await;
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
    tokio::select! {r=outgoing=>r,r=incoming=>r}
}
impl Room {
    fn observer_snapshot(&self) -> Message {
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
        for ev in events {
            match *ev {
                Event::Move { id, position } => {
                    out.push(Message::position(wire_id(id), position.x, position.y))
                }
                Event::Spawn { id } => {
                    if let Some(e) = self.world.entities.iter().find(|e| e.id == id) {
                        out.push(spawn(e, &self.picks, self.data.as_deref()));
                    }
                }
                _ => {}
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
