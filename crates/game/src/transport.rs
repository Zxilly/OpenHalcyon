use super::*;
use halcyon_protocol::{ACK_FRAME, MAX_PAYLOAD, MatchCipher, parse_hello};
use std::{io, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinSet,
    time::{self, timeout},
};

fn bad(msg: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
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
        let msg = cipher.decode(&frame(&mut sock).await?)?;
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
    let _connected = Connected(room.clone(), seat, ch, generation);
    let (mut rd, mut wr) = sock.into_split();
    let write_loop = async {
        let mut next = Some(initial);
        while let Some(batch) = next {
            let mut bytes = Vec::new();
            for m in batch {
                bytes.extend(cipher.encode(&m)?);
            }
            write(&mut wr, &manager.0, &bytes).await?;
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
            let m = cipher.decode(&raw)?;
            let out = room.lock().unwrap().receive(seat, ch, m)?;
            if !out.is_empty() {
                tx.try_send(out).map_err(|_| bad("output queue full"))?;
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
