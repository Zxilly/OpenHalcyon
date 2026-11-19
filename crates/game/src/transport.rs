use super::*;
use halcyon_protocol::{ACK_FRAME, MAX_PAYLOAD, MatchCipher, parse_hello};
use std::{io, sync::OnceLock, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinSet,
    time::{self, timeout},
};

fn bad(msg: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

pub(super) enum Delivery {
    Private(Batch),
    Team(Arc<TeamBatch>),
}

/// CE encrypts each frame independently with the room key. Teammates admitted
/// to the same room can share the ciphertext for their common visibility set.
pub(super) struct TeamBatch {
    messages: Batch,
    bytes: OnceLock<Result<Vec<u8>, io::ErrorKind>>,
}

impl TeamBatch {
    pub(super) fn new(messages: Batch) -> Arc<Self> {
        Arc::new(Self {
            messages,
            bytes: OnceLock::new(),
        })
    }
}

impl Delivery {
    fn bytes<'a>(&'a self, cipher: &MatchCipher, scratch: &'a mut Vec<u8>) -> io::Result<&'a [u8]> {
        match self {
            Self::Private(messages) => {
                scratch.clear();
                encode(cipher, messages, scratch)?;
                Ok(scratch)
            }
            Self::Team(team) => team
                .bytes
                .get_or_init(|| {
                    let mut bytes = Vec::new();
                    encode(cipher, &team.messages, &mut bytes).map_err(|error| error.kind())?;
                    Ok(bytes)
                })
                .as_deref()
                .map_err(|kind| io::Error::from(*kind)),
        }
    }

    fn into_records(self) -> Batch {
        match self {
            Self::Private(messages) => messages,
            Self::Team(team) => Arc::try_unwrap(team).map_or_else(
                |shared| shared.messages.clone(),
                |exclusive| exclusive.messages,
            ),
        }
    }
}

fn encode(cipher: &MatchCipher, messages: &[Message], buffer: &mut Vec<u8>) -> io::Result<()> {
    for message in messages {
        cipher.encode_into(message, buffer)?;
    }
    Ok(())
}
pub(super) async fn frame<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Vec<u8>> {
    let size = reader.read_u16().await? as usize;
    if !(2..=MAX_PAYLOAD).contains(&size) {
        return Err(bad("invalid frame size"));
    }
    let mut bytes = vec![0; size];
    reader.read_exact(&mut bytes).await?;
    Ok(bytes)
}
pub(super) async fn write<W: AsyncWrite + Unpin>(
    writer: &mut W,
    manager: &Manager,
    bytes: &[u8],
) -> io::Result<()> {
    timeout(Duration::from_secs(5), writer.write_all(bytes)).await??;
    manager
        .bytes
        .fetch_add(bytes.len() as u64, Ordering::Relaxed);
    Ok(())
}
/// Natural completion seals the queue after the last tick. A writer already in
/// write_all finishes that batch before reading the remainder in FIFO order.
pub(super) async fn next_delivery<T>(
    rx: &mut mpsc::Receiver<T>,
    ended: &mut watch::Receiver<bool>,
) -> Option<T> {
    if !*ended.borrow() {
        tokio::select! {
            biased;
            _=ended.changed()=>{},
            value=rx.recv()=>return value,
        }
    }
    rx.close();
    rx.recv().await
}

pub(super) async fn close_clients<T: Send + 'static>(tasks: &mut JoinSet<T>, completed: bool) {
    if completed {
        let _ = timeout(Duration::from_secs(1), async {
            while tasks.join_next().await.is_some() {}
        })
        .await;
    }
    tasks.shutdown().await;
}

struct Connected(Shared, usize, usize, u64);
impl Drop for Connected {
    fn drop(&mut self) {
        self.0.lock().unwrap().detach(self.1, self.2, self.3);
    }
}
async fn connection(mut sock: TcpStream, room: Shared, manager: Rooms) -> io::Result<()> {
    let (mut admitted, mut ended) = {
        let room = room.lock().unwrap();
        (room.admission_ready.subscribe(), room.stop.subscribe())
    };
    let mut handshake_stop = ended.clone();
    let handshake = async {
        timeout(Duration::from_secs(10), admitted.wait_for(|ready| *ready))
            .await?
            .map_err(|_| bad("reservation closed"))?;
        sock.set_nodelay(true)?;
        let cipher = MatchCipher::new(&room.lock().unwrap().id.to_string());
        let uuid = timeout(Duration::from_secs(10), async {
            parse_hello(&frame(&mut sock).await?)?;
            write(&mut sock, &manager.0, &ACK_FRAME).await?;
            let msg = cipher.decode_owned(frame(&mut sock).await?)?;
            let Fields::PlayerUuid(s) = msg.fields()? else {
                return Err(bad("expected roster identification"));
            };
            Uuid::parse_str(s).map_err(|_| bad("invalid roster identification"))
        })
        .await??;
        Ok::<_, io::Error>((cipher, uuid))
    };
    let (cipher, uuid) = tokio::select! {
        biased;
        _=handshake_stop.wait_for(|done|*done)=>return Ok(()),
        result=handshake=>result?,
    };
    let (stalled, mut slow) = watch::channel(false);
    let (tx, mut rx) = mpsc::channel(16);
    let (seat, ch, generation, initial) = room
        .lock()
        .unwrap()
        .attach(uuid, tx.clone(), stalled)
        .ok_or(bad("unavailable seat"))?;
    let mut recording = if seat == 0 && ch == 1 {
        let root = manager.0.replay_directory.lock().unwrap().clone();
        root.map(|root| {
            let state = room.lock().unwrap();
            recording::Recorder::start(
                root,
                state.id,
                generation,
                state.world.tick,
                state.replay_bytes.clone(),
            )
        })
    } else {
        None
    };
    let _connected = Connected(room.clone(), seat, ch, generation);
    let (mut rd, mut wr) = sock.into_split();
    let write_loop = async {
        let mut next = Some(Delivery::Private(initial));
        let mut bytes = Vec::new();
        while let Some(batch) = next {
            write(&mut wr, &manager.0, batch.bytes(&cipher, &mut bytes)?).await?;
            // Large initial catalogs are sent once. Keep steady-state buffers small.
            if bytes.capacity() > 32 * 1024 {
                bytes = Vec::new();
            }
            if let Some(recorder) = recording.as_mut() {
                let tick = room.lock().unwrap().world.tick;
                if !recorder.record(tick, batch.into_records()) {
                    recording = None;
                }
            }
            next = next_delivery(&mut rx, &mut ended).await;
        }
        wr.shutdown().await
    };
    let read_loop = async {
        let mut budget = 0;
        let mut start = Instant::now();
        loop {
            let raw = timeout(
                Duration::from_secs(if ch == 0 { 600 } else { 90 }),
                frame(&mut rd),
            )
            .await??;
            if start.elapsed() >= Duration::from_secs(1) {
                start = Instant::now();
                budget = 0;
            }
            budget += 1;
            if budget > 300 {
                return Err(bad("input rate exceeded"));
            }
            let m = cipher.decode_owned(raw)?;
            let mut state = room.lock().unwrap();
            if matches!(state.phase, "ending" | "ended") {
                continue;
            }
            let out = state.receive(seat, ch, m)?;
            if !out.is_empty() {
                tx.try_send(Delivery::Private(out))
                    .map_err(|_| bad("output queue full"))?;
            }
        }
    };
    tokio::pin!(write_loop);
    // Stop reading on normal completion so a late input/EOF cannot cancel a partial write.
    tokio::select! {
        biased;
        _=slow.changed()=>return Err(bad("output queue full")),
        _=handshake_stop.wait_for(|done|*done)=>{},
        r=&mut write_loop=>return r,
        r=read_loop=>return r,
    }
    if room.lock().unwrap().reason != Some("completed") {
        return Ok(());
    }
    tokio::select! {biased; _=slow.changed()=>Err(bad("output queue full")),r=&mut write_loop=>r}
}
pub(super) async fn serve(
    listener: TcpListener,
    room: Shared,
    mut stopped: watch::Receiver<bool>,
    manager: &Rooms,
) {
    let mut tasks = JoinSet::new();
    let mut tick = time::interval(Duration::from_millis(1000 / halcyon_sim::TICK_HZ));
    tick.set_missed_tick_behavior(time::MissedTickBehavior::Burst);
    loop {
        tokio::select! {
            _=stopped.changed()=>break,
            scheduled=tick.tick()=>{
                {
                    let mut state=room.lock().unwrap();let before=state.world.tick;state.tick();
                    if state.world.tick!=before{let us=scheduled.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;state.latency.observe(us);}
                }
                // Share the runtime with sockets and other rooms between catch-up ticks.
                if scheduled.elapsed()>=tick.period(){tokio::task::yield_now().await;}

            },
            res=listener.accept()=>match res{
                Ok((sock,_)) if tasks.len()<64=>{tasks.spawn(connection(sock,room.clone(),manager.clone()));}
                Ok(_)=>{}
                Err(e)=>{tracing::warn!(%e,"game listener closed");break}
            },
            Some(_)=tasks.join_next(),if !tasks.is_empty()=>{}
        }
    }
    drop(listener);
    let completed = room.lock().unwrap().reason == Some("completed");
    // Settlement and room capacity are independent of any socket's flush deadline.
    manager.finished(&room);
    close_clients(&mut tasks, completed).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use halcyon_sim::Position;

    async fn identified(addr: &str, identity: Uuid, cipher: &MatchCipher) -> TcpStream {
        let mut sock = TcpStream::connect(addr).await.unwrap();
        let port = sock.peer_addr().unwrap().port();
        sock.write_all(&halcyon_protocol::hello_with_port("127.0.0.1", port).unwrap())
            .await
            .unwrap();
        let mut ack = [0; 5];
        sock.read_exact(&mut ack).await.unwrap();
        assert_eq!(ack, ACK_FRAME);
        sock.write_all(
            &cipher
                .encode(&Message::new(0x3e8, identity.to_string().as_bytes()))
                .unwrap(),
        )
        .await
        .unwrap();
        // Reading a complete initial frame confirms that this socket is registered.
        let _ = frame(&mut sock).await.unwrap();
        sock
    }

    async fn final_frames(mut stream: TcpStream, cipher: &MatchCipher) -> Vec<u16> {
        let mut received = Vec::new();
        let mut bytes = Vec::new();
        timeout(Duration::from_secs(2), stream.read_to_end(&mut bytes))
            .await
            .unwrap()
            .unwrap();
        let mut slice = bytes.as_slice();
        while let Some(body) = halcyon_protocol::read_frame(&mut slice).unwrap() {
            received.push(cipher.decode(&body).unwrap().opcode);
        }
        received
    }

    #[tokio::test]
    async fn grants_no_grace_on_forced_close_and_bounds_a_stalled_natural_flush() {
        let mut tasks = JoinSet::<()>::new();
        tasks.spawn(std::future::pending());
        timeout(Duration::from_millis(250), close_clients(&mut tasks, false))
            .await
            .unwrap();
        assert!(tasks.is_empty());
        tasks.spawn(std::future::pending());
        timeout(Duration::from_secs(3), close_clients(&mut tasks, true))
            .await
            .unwrap();
        assert!(tasks.is_empty());

        let (tx, _rx) = mpsc::channel(1);
        let (stalled, slow) = watch::channel(false);
        let peer = Peer {
            generation: 1,
            tx,
            stalled,
        };
        peer.send(Delivery::Private(vec![]));
        peer.send(Delivery::Private(vec![]));
        assert!(*slow.borrow());
    }

    #[tokio::test]
    async fn natural_crystal_death_is_written_before_player_and_observer_eof() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let player = Uuid::new_v4();
        let id = Uuid::new_v4();
        let hosted = rooms
            .reserve(Reservation {
                match_id: id,
                players: vec![player],
                teams: vec![0],
                mode: "5v5".into(),
            })
            .unwrap();
        let room = rooms.room(id).unwrap();
        let cipher = MatchCipher::new(&id.to_string());
        let target = {
            let mut r = room.lock().unwrap();
            let crystal = r
                .world
                .entities
                .iter()
                .find(|e| e.kind == halcyon_sim::Kind::Crystal && e.team == 1)
                .unwrap();
            let (id, pos) = (crystal.id, crystal.position);
            r.world.entities[0].position = pos;
            r.seats[0].locked = true;
            r.phase("running");
            id
        };
        let data = identified(&hosted.address, player, &cipher).await;
        let observer = rooms.spectate(id, Uuid::new_v4()).unwrap();
        let view = identified(&observer.address, observer.spectator_id, &cipher).await;
        let withdrawn = rooms.spectate(id, Uuid::new_v4()).unwrap();
        let revoked_view = identified(&withdrawn.address, withdrawn.spectator_id, &cipher).await;
        {
            let mut r = room.lock().unwrap();
            r.world
                .entities
                .iter_mut()
                .find(|e| e.id == target)
                .unwrap()
                .hp = 0.1;
            r.world.tick = 15;
            r.world
                .command(1, halcyon_sim::Command::Attack(target))
                .unwrap();
            for _ in 0..5 {
                r.tick();
            }
            let reason = r.reason;
            drop(r);
            assert_eq!(reason, Some("completed"));
        }
        // The final batch is pending for this grant, but revocation takes priority.
        rooms.stop_spectating(id, withdrawn.spectator_id).unwrap();
        // The current-thread runtime cannot poll either writer between tick()'s
        // fanout and stop signal. The final snapshot is the last queued frame.
        let (player, observer) =
            tokio::join!(final_frames(data, &cipher), final_frames(view, &cipher));
        let revoked = final_frames(revoked_view, &cipher).await;
        assert!(!revoked.contains(&0x430));
        for stream in [player, observer] {
            let death = stream.iter().position(|&op| op == 0x430).unwrap();
            assert!(stream[..death].contains(&0x41e));
            assert_eq!(stream.last(), Some(&0x459));
        }
    }

    #[tokio::test]
    async fn bot_inventory_is_stable_across_visibility_and_reconnect() {
        use halcyon_sim::BasicItem;
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        let players = [Uuid::new_v4(), Uuid::new_v4()];
        rooms
            .reserve(Reservation {
                match_id: id,
                players: players.to_vec(),
                teams: vec![0, 1],
                mode: "5v5".into(),
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut r = shared.lock().unwrap();
        r.phase("running");
        r.world.tick = 1;
        for s in &mut r.seats {
            s.locked = true;
        }
        let attach = |r: &mut Room, id| {
            let (tx, rx) = mpsc::channel(16);
            let (_, ch, _, batch) = r.attach(id, tx, watch::channel(false).0).unwrap();
            assert_eq!(ch, 1);
            (rx, batch)
        };
        let (mut ally, _) = attach(&mut r, players[0]);
        let (mut enemy, _) = attach(&mut r, players[1]);
        let i = r
            .world
            .entities
            .iter()
            .position(|e| e.kind == halcyon_sim::Kind::Hero && e.bot && e.team == 0)
            .unwrap();
        let id = r.world.entities[i].id;
        let actor = u32::from(wire_id(id));
        let grants = |messages: &[Message]| {
            messages
                .iter()
                .filter_map(|m| match m.fields().unwrap() {
                    Fields::ItemGrant {
                        actor: a, instance, ..
                    } if a == actor => Some(instance),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        r.world.entities[i].gold = 2000;
        let first = r.world.purchase(id, BasicItem::WeaponBlade).unwrap();
        r.replicate(&[Event::Purchase(first)]);
        let batch = ally.try_recv().unwrap().into_records();
        assert_eq!(
            batch.iter().map(|m| m.opcode).collect::<Vec<_>>(),
            [0x41d, 0x43d, 0x41c]
        );
        assert_eq!(grants(&batch), [first.granted.instance]);
        assert!(enemy.try_recv().is_err());
        r.visible[0].remove(&id);
        let second = r.world.purchase(id, BasicItem::LightArmor).unwrap();
        let events = [Event::Purchase(second), Event::GoldAward { id, amount: 25 }];
        r.replicate(&events);
        let batch = ally.try_recv().unwrap().into_records();
        let expected = [first.granted.instance, second.granted.instance];
        assert_eq!(grants(&batch), expected);
        assert_eq!(
            batch
                .iter()
                .filter_map(|m| match m.fields().unwrap() {
                    Fields::EntityStat { stat: 6, value, .. } => Some(value),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            [-300., 25.]
        );
        assert!(enemy.try_recv().is_err());
        assert!(grants(&r.spawn_view(Some(1))).is_empty());
        assert_eq!(grants(&r.spawn_view(None)), expected);
        let generation = r.seats[0].peers[1].as_ref().unwrap().generation;
        r.detach(0, 1, generation);
        let (_, reconnect) = attach(&mut r, players[0]);
        assert_eq!(grants(&reconnect), expected);
        let spawn = batch.iter().find(|m| m.opcode == 0x3f3).unwrap();
        assert_eq!(
            f32::from_be_bytes(spawn.body[190..194].try_into().unwrap()),
            r.world.entities[i].armor()
        );
    }

    #[tokio::test]
    async fn teammates_share_ciphertext_while_opponents_and_reconnects_keep_their_view() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        let players = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
        rooms
            .reserve(Reservation {
                match_id: id,
                players: players.to_vec(),
                teams: vec![0, 0, 1],
                mode: "5v5".into(),
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut room = shared.lock().unwrap();
        room.phase("running");
        room.world.tick = 1;
        for entity in &mut room.world.entities {
            entity.position = Position {
                x: if entity.team == 0 { -80. } else { 80. },
                y: 0.,
            };
        }
        let position = room.world.entities[0].position;
        for seat in &mut room.seats {
            seat.locked = true;
        }
        let attach = |room: &mut Room, id| {
            let (tx, rx) = mpsc::channel(16);
            let (_, channel, generation, _) = room.attach(id, tx, watch::channel(false).0).unwrap();
            assert_eq!(channel, 1);
            (rx, generation)
        };
        let (mut a, generation) = attach(&mut room, players[0]);
        let (mut b, _) = attach(&mut room, players[1]);
        let (mut other, _) = attach(&mut room, players[2]);
        let movement = [Event::Move { id: 1, position }];
        room.replicate(&movement);
        let first = a.try_recv().unwrap();
        let second = b.try_recv().unwrap();
        let cipher = MatchCipher::new(&id.to_string());
        let (mut scratch_a, mut scratch_b) = (Vec::new(), Vec::new());
        let bytes_a = first.bytes(&cipher, &mut scratch_a).unwrap();
        let bytes_b = second.bytes(&cipher, &mut scratch_b).unwrap();
        assert!(std::ptr::eq(bytes_a, bytes_b));
        assert_eq!(
            bytes_a,
            cipher
                .encode(&Message::position(wire_id(1), position.x, position.y))
                .unwrap()
        );
        assert!(other.try_recv().is_err());

        room.detach(0, 1, generation);
        let (mut reconnect, _) = attach(&mut room, players[0]);
        room.replicate(&movement);
        assert_eq!(
            reconnect.try_recv().unwrap().into_records(),
            second.into_records()
        );
        assert!(other.try_recv().is_err());
    }
}
