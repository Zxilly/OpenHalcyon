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
struct Connected(Shared, usize, usize, u64);
impl Drop for Connected {
    fn drop(&mut self) {
        self.0.lock().unwrap().detach(self.1, self.2, self.3);
    }
}
async fn connection(mut sock: TcpStream, room: Shared, manager: Rooms) -> io::Result<()> {
    let mut admitted = room.lock().unwrap().admission_ready.subscribe();
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
            next = rx.recv().await;
        }
        Ok(())
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
            let out = room.lock().unwrap().receive(seat, ch, m)?;
            if !out.is_empty() {
                tx.try_send(Delivery::Private(out))
                    .map_err(|_| bad("output queue full"))?;
            }
        }
    };
    tokio::select! { r=write_loop=>r,r=read_loop=>r, _=slow.changed()=>Err(bad("output queue full")) }
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
    tasks.shutdown().await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use halcyon_sim::Position;

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
