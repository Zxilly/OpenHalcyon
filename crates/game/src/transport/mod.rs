pub(crate) mod observer;
pub(crate) mod reconnect;
pub(crate) mod recording;

use crate::host::manager::{Manager, Rooms, Shared};
use crate::modes::aral::inventory as aral_inventory;
use crate::replication::{Batch, messages::wire_id};
use crate::{Message, RoomEndReason};
use halcyon_protocol::{
    ACK_FRAME, Fields, MatchCipher,
    async_frame::{FrameReader, FrameWriter},
    parse_hello,
};
use std::{
    io,
    sync::{Arc, OnceLock, atomic::Ordering},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{mpsc, watch},
    task::JoinSet,
    time::{self, timeout},
};
use uuid::Uuid;

fn bad(msg: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

pub(crate) enum Delivery {
    Private(Batch),
    Team(Arc<TeamBatch>),
}

/// CE encrypts each frame independently with the room key. Teammates admitted
/// to the same room can share the ciphertext for their common visibility set.
pub(crate) struct TeamBatch {
    messages: Batch,
    bytes: OnceLock<Result<Vec<u8>, io::ErrorKind>>,
}

impl TeamBatch {
    pub(crate) fn new(messages: Batch) -> Arc<Self> {
        Arc::new(Self {
            messages,
            bytes: OnceLock::new(),
        })
    }

    pub(crate) fn messages(&self) -> &[Message] {
        &self.messages
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

    fn messages(&self) -> &[Message] {
        match self {
            Self::Private(messages) => messages,
            Self::Team(team) => team.messages(),
        }
    }

    fn contains(&self, opcode: u16) -> bool {
        self.messages().iter().any(|m| m.opcode == opcode)
    }

    fn declares_actor(&self, actor: u16, slot: u8) -> bool {
        self.messages().iter().any(|message| {
            message.is(crate::Opcode::HeroSpawn)
                && halcyon_protocol::HeroSpawnBody::read_unpadded_or_padded(&message.body)
                    .is_ok_and(|spawn| spawn.actor as u16 == actor && spawn.slot == slot)
        })
    }

    pub(crate) fn into_records(self) -> Batch {
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
// Independent, exact-read test client so each assertion consumes just one frame.
#[cfg(test)]
pub(super) async fn frame<R: tokio::io::AsyncRead + Unpin>(reader: &mut R) -> io::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let size = reader.read_u16().await? as usize;
    if !(2..=halcyon_protocol::MAX_PAYLOAD).contains(&size) {
        return Err(bad("invalid frame size"));
    }
    let mut bytes = vec![0; size];
    reader.read_exact(&mut bytes).await?;
    Ok(bytes)
}
pub(super) async fn acknowledge<W: AsyncWrite + Unpin>(
    writer: &mut W,
    manager: &Manager,
) -> io::Result<()> {
    let mut frames = FrameWriter::default();
    timeout(
        Duration::from_secs(5),
        frames.write(writer, (&ACK_FRAME[2..]).into()),
    )
    .await??;
    manager
        .bytes
        .fetch_add(ACK_FRAME.len() as u64, Ordering::Relaxed);
    Ok(())
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
    let (mut admitted, mut handshake_stop, mut ended) = {
        let room = room.lock().unwrap();
        (
            room.admission_ready.subscribe(),
            room.stop.subscribe(),
            room.settled.subscribe(),
        )
    };
    let mut frames = FrameReader::new();
    let handshake = async {
        timeout(Duration::from_secs(10), admitted.wait_for(|ready| *ready))
            .await?
            .map_err(|_| bad("reservation closed"))?;
        sock.set_nodelay(true)?;
        let cipher = MatchCipher::new(&room.lock().unwrap().id.to_string());
        let uuid = timeout(Duration::from_secs(10), async {
            parse_hello(&frames.required(&mut sock).await?)?;
            acknowledge(&mut sock, &manager.0).await?;
            let msg = cipher.decode_owned(frames.required(&mut sock).await?.into())?;
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
    let mut replay_root = (seat == 0
        && (ch == 1 || {
            let state = room.lock().unwrap();
            state.legacy_aral() || state.solo_scene()
        }))
    .then(|| manager.0.replay_directory.lock().unwrap().clone())
    .flatten();
    let mut recording = None;
    let _connected = Connected(room.clone(), seat, ch, generation);
    let (mut rd, mut wr) = sock.into_split();
    let write_loop = async {
        let mut next = Some(Delivery::Private(initial));
        let mut bytes = Vec::new();
        let (mut waiting_for_scene, mut registration, aral_owner) = {
            let state = room.lock().unwrap();
            let solo = ch == 0 && state.solo_scene();
            let actor = &state.world.entities[seat];
            let slot = if (ch == 0 && state.legacy_aral()) || solo {
                state.hero_slot(actor)
            } else {
                None
            };
            let registration = slot.map(|slot| (wire_id(actor.id), slot));
            let aral_owner = if state.legacy_aral() {
                registration
            } else {
                None
            };
            (solo, registration, aral_owner)
        };
        let mut aral_inventory_known = false;
        let mut data_header = ch == 1;
        while let Some(mut batch) = next {
            if let Some((actor, slot)) = aral_owner
                && aral_inventory_known
                && batch.declares_actor(actor, slot)
                && let Some(filtered) =
                    aral_inventory::without_existing_baseline(batch.messages(), u32::from(actor))
            {
                batch = Delivery::Private(filtered);
            }
            write(&mut wr, &manager.0, batch.bytes(&cipher, &mut bytes)?).await?;
            if let Some((actor, _)) = aral_owner {
                let actor = u32::from(actor);
                if batch
                    .messages()
                    .iter()
                    .any(|m| aral_inventory::affects_owner(m, actor))
                {
                    let mut state = room.lock().unwrap();
                    let seat = &mut state.seats[seat];
                    if seat.peers[ch]
                        .as_ref()
                        .is_some_and(|peer| peer.generation == generation)
                    {
                        aral_inventory::record_delivery(
                            &mut seat.aral_reorder_view,
                            actor,
                            batch.messages(),
                        );
                        aral_inventory_known = seat.aral_reorder_view.is_some();
                    }
                }
            }
            let opening_scene = ch == 0 && batch.contains(crate::wire_opcode::SceneSettings);
            let declared_actor =
                registration.is_some_and(|(actor, slot)| batch.declares_actor(actor, slot));
            if (waiting_for_scene && opening_scene)
                || data_header
                || declared_actor
                || (opening_scene && registration.is_some())
            {
                let mut state = room.lock().unwrap();
                let current_peer = state.seats[seat].peers[ch]
                    .as_ref()
                    .is_some_and(|p| p.generation == generation);
                if waiting_for_scene && opening_scene {
                    if current_peer {
                        state.seats[seat].scene_written = true;
                    }
                    waiting_for_scene = false;
                }
                let dead_reconnect =
                    opening_scene && registration.is_some() && state.world.entities[seat].hp <= 0.;
                if data_header || declared_actor || dead_reconnect {
                    if current_peer {
                        if ch == 0 {
                            state.seats[seat].world_actor_written = true;
                        }
                        state.world_connected(seat, ch);
                    }
                    registration = None;
                    data_header = false;
                }
            }
            // Large initial catalogs are sent once. Keep steady-state buffers small.
            if bytes.capacity() > 32 * 1024 {
                bytes = Vec::new();
            }
            // A shared control connection becomes a world stream at the scene header.
            // Skip the private draft, including controls that later yield to a second socket.
            if replay_root.is_some()
                && (ch == 1 || batch.contains(crate::wire_opcode::SceneSettings))
            {
                let state = room.lock().unwrap();
                recording = replay_root.take().map(|root| {
                    recording::Recorder::start(
                        root,
                        state.id,
                        generation,
                        state.world.tick,
                        state.replay_bytes.clone(),
                    )
                });
            }
            if let Some(recorder) = recording.as_mut() {
                let tick = room.lock().unwrap().world.tick;
                if !recorder.record(tick, batch.into_records()) {
                    recording = None;
                }
            }
            next = next_delivery(&mut rx, &mut ended).await;
        }
        // The existing authorized whole-room termination has a native menu-exit
        // signal. Each socket receives only its own authenticated UUID.
        let force_menu = room.lock().unwrap().reason == Some(crate::RoomEndReason::AdminTerminated);
        if force_menu {
            bytes.clear();
            encode(&cipher, &[kick(uuid)], &mut bytes)?;
            write(&mut wr, &manager.0, &bytes).await?;
        }
        wr.shutdown().await
    };
    let read_loop = async {
        let mut budget = 0;
        let mut start = Instant::now();
        loop {
            let raw = timeout(
                Duration::from_secs(if ch == 0 { 600 } else { 90 }),
                frames.next(&mut rd),
            )
            .await??;
            let Some(raw) = raw else {
                return Ok(());
            };
            if start.elapsed() >= Duration::from_secs(1) {
                start = Instant::now();
                budget = 0;
            }
            budget += 1;
            if budget > 300 {
                return Err(bad("input rate exceeded"));
            }
            let m = cipher.decode_owned(raw.into())?;
            let mut state = room.lock().unwrap();
            let out = match state.phase {
                // ARAL keeps its authenticated connections open between the
                // crystal defeat or surrender and the final statistics report.
                crate::RoomPhase::Ending
                    if state.reason == Some(crate::RoomEndReason::Completed)
                        && !*state.stop.borrow()
                        && m.is(crate::Opcode::ClockSync)
                        && halcyon_protocol::ClockSyncBody::read_with_padding(&m.body)
                            .is_ok_and(|(clock, pad)| {
                                !pad.is_empty() && clock.seconds.is_finite() && clock.seconds >= 0.
                            }) =>
                {
                    vec![m]
                }
                crate::RoomPhase::Ending | crate::RoomPhase::Ended => continue,
                _ => state.receive(seat, ch, m)?,
            };
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
    if !allows_final_flush(room.lock().unwrap().reason) {
        return Ok(());
    }
    tokio::select! {biased; _=slow.changed()=>Err(bad("output queue full")),r=&mut write_loop=>r}
}
fn kick(player: Uuid) -> Message {
    let mut uuid = Uuid::encode_buffer();
    let player_uuid = halcyon_protocol::FixedAscii::<64>::from_str_trunc(
        player.hyphenated().encode_lower(&mut uuid),
    );
    // Native disconnect code zero requests an immediate return to menus.
    halcyon_protocol::KickPlayerBody {
        player_uuid: player_uuid.into(),
        disconnect_code: 0,
    }
    .encode()
}

fn allows_final_flush(reason: Option<RoomEndReason>) -> bool {
    matches!(
        reason,
        Some(RoomEndReason::Completed | RoomEndReason::AdminTerminated)
    )
}

pub(crate) async fn serve(
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
                    let wake = time::Instant::now();
                    let mut state = room.lock().unwrap();
                    let locked = time::Instant::now();
                    let before = state.world.tick;
                    state.tick();
                    if state.world.tick != before {
                        state.latency.observe(scheduled, wake, locked, time::Instant::now());
                    }
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
    let final_flush = allows_final_flush(room.lock().unwrap().reason);
    // Commit match settlement before closing client sockets so immediate CE polls see PostMatch.
    manager.finished(&room).await;
    room.lock().unwrap().settled.send_replace(true);
    close_clients(&mut tasks, final_flush).await;
}

#[cfg(test)]
#[path = "transport_tests.rs"]
mod tests;
