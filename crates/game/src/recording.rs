//! First-human data-channel recording. The socket writer offers bounded batches after a successful write.
use super::*;
use halcyon_protocol::ReplayWriter;
use std::{
    io,
    path::{Path, PathBuf},
};
use tokio::{fs, io::AsyncWriteExt};

const BATCH_LIMIT: usize = 64 * 1024;
const MATCH_LIMIT: u64 = 128 * 1024 * 1024;
const QUEUE_DEPTH: usize = 8;
const CHUNK_TICKS: u64 = halcyon_sim::TICK_HZ * 10;

impl Rooms {
    /// Call before serving. The caller controls retention and the directory's Windows ACL.
    pub fn set_replay_directory(&self, path: &Path) -> io::Result<()> {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(path)?;
        let root = path.canonicalize()?;
        if !root.is_dir() {
            return Err(io::Error::other("replay path must be a directory"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if root.metadata()?.permissions().mode() & 0o077 != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "replay directory must be private to its owner",
                ));
            }
        }
        *self.0.replay_directory.lock().unwrap() = Some(root);
        Ok(())
    }
}
struct Sample {
    tick: u64,
    records: Batch,
}
pub(super) struct Recorder {
    tx: mpsc::Sender<Sample>,
    budget: Arc<AtomicU64>,
}
impl Recorder {
    pub fn start(
        root: PathBuf,
        room_id: Uuid,
        generation: u64,
        first_tick: u64,
        budget: Arc<AtomicU64>,
    ) -> Self {
        let (tx, rx) = mpsc::channel(QUEUE_DEPTH);
        tokio::spawn(async move {
            let directory = root
                .join(room_id.to_string())
                .join(format!("seat-0-{generation}-{}", Uuid::new_v4()));
            if let Err(error) = save(directory, first_tick, rx).await {
                tracing::warn!(room=%room_id,%error,"replay recording stopped");
            }
        });
        Self { tx, budget }
    }
    pub fn record(&mut self, tick: u64, mut batch: Batch) -> bool {
        batch.retain_mut(sanitize);
        if batch.is_empty() {
            return true;
        }
        let size: usize = batch.iter().map(|m| 10 + m.body.len()).sum();
        if size > BATCH_LIMIT
            || self
                .budget
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                    used.checked_add(size as u64).filter(|n| *n <= MATCH_LIMIT)
                })
                .is_err()
        {
            tracing::warn!("replay recording exceeded its byte budget");
            return false;
        }
        if self
            .tx
            .try_send(Sample {
                tick,
                records: batch,
            })
            .is_err()
        {
            tracing::warn!("replay recording queue unavailable");
            return false;
        }
        true
    }
}
fn sanitize(m: &mut Message) -> bool {
    match m.opcode {
        0x3ee if matches!(m.body.len(), 216 | 222) => {
            if m.body[212] == 0 {
                m.body[..128].fill(0);
            }
        }
        0x459 if m.body.len() == 2590 => {
            for row in m.body[..16 * 161].as_chunks_mut::<161>().0 {
                if !row[105..141].starts_with(b"__Kindred_Player_Bot__") {
                    row[25..57].fill(0);
                    row[105..141].fill(0);
                }
            }
        }
        // Audited outbound gameplay opcodes only. Unknown and control messages stay out.
        0x45c if matches!(m.body.len(), 96 | 102) && m.body[96..].iter().all(|&v| v == 0) => {}
        0x3ef if m.body.is_empty() || m.body == [0; 6] => {}
        0x41e if m.body.len() == 22 && m.body[16..] == [0; 6] => {}
        0x430 if m.body.len() == 14 && m.body[8..] == [0; 6] => {}
        0x3f1 if matches!(m.body.as_slice(), [0, 0, 0, 1 | 2, 0, 0]) => {}
        0x3e9 | 0x3f3 | 0x41c | 0x41d | 0x42e | 0x439 | 0x43d | 0x44b | 0x451 | 0x46c | 0x46f
        | 0x470 => {}
        _ => return false,
    }
    true
}
struct Chunk {
    file: fs::File,
    partial: PathBuf,
    index: u64,
}
impl Chunk {
    async fn open(dir: &Path, index: u64) -> io::Result<Self> {
        let partial = dir.join(format!("{index:08}.vgr.partial"));
        let mut options = fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        options.mode(0o600);
        let file = options.open(&partial).await?;
        Ok(Self {
            file,
            partial,
            index,
        })
    }
    async fn close(mut self, complete: bool) -> io::Result<()> {
        self.file.flush().await?;
        self.file.sync_data().await?;
        drop(self.file);
        if complete {
            fs::rename(&self.partial, self.partial.with_extension("")).await?;
        }
        Ok(())
    }
}
async fn save(dir: PathBuf, first_tick: u64, mut rx: mpsc::Receiver<Sample>) -> io::Result<()> {
    fs::create_dir_all(&dir).await?;
    let mut chunk: Option<Chunk> = None;
    let mut bytes = Vec::new();
    while let Some(sample) = rx.recv().await {
        let index = sample.tick / CHUNK_TICKS;
        if chunk.as_ref().is_none_or(|c| c.index != index) {
            if let Some(previous) = chunk.take() {
                let full = first_tick <= previous.index * CHUNK_TICKS;
                previous.close(full).await?;
            }
            chunk = Some(Chunk::open(&dir, index).await?);
        }
        bytes.clear();
        let mut writer = ReplayWriter::new(&mut bytes);
        let seconds = sample.tick as f32 / halcyon_sim::TICK_HZ as f32;
        for msg in sample.records {
            writer.write_record(seconds, &msg)?;
        }
        chunk.as_mut().unwrap().file.write_all(&bytes).await?;
    }
    if let Some(last) = chunk {
        last.close(false).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use halcyon_protocol::ReplayReader;
    #[test]
    fn keeps_authored_combat_frames_without_arbitrary_tails() {
        let mut death = Message::actor_death(1510, 1511);
        let mut impact = Message::health_impact(1510, 1511, -7., 5, 0, 0);
        assert!(sanitize(&mut death) && sanitize(&mut impact));
        death.body.push(0);
        impact.body[16] = 1;
        assert!(!sanitize(&mut death) && !sanitize(&mut impact));
    }

    #[tokio::test]
    async fn rotation_marks_only_complete_simulated_windows() {
        let temp = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel(QUEUE_DEPTH);
        for tick in [40, 200, 399, 400] {
            tx.send(Sample {
                tick,
                records: vec![Message::position(1500, 1., 2.)],
            })
            .await
            .unwrap();
        }
        drop(tx);
        save(temp.path().to_owned(), 40, rx).await.unwrap();
        assert!(temp.path().join("00000000.vgr.partial").is_file());
        assert!(temp.path().join("00000002.vgr.partial").is_file());
        let data = std::fs::read(temp.path().join("00000001.vgr")).unwrap();
        let mut reader = ReplayReader::new(&data[..]);
        assert_eq!(reader.next_record().unwrap().unwrap().time, 10.);
        assert_eq!(reader.next_record().unwrap().unwrap().time, 19.95);
        assert!(reader.next_record().unwrap().is_none());
    }
    #[tokio::test]
    async fn filesystem_failure_closes_only_the_recorder() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("file");
        std::fs::write(&file, b"existing").unwrap();
        let mut recorder = Recorder::start(file.clone(), Uuid::new_v4(), 1, 0, Arc::default());
        tokio::time::timeout(std::time::Duration::from_secs(3), recorder.tx.closed())
            .await
            .unwrap();
        assert!(!recorder.record(0, vec![Message::position(1, 0., 0.)]));
        assert_eq!(std::fs::read(file).unwrap(), b"existing");
    }

    #[tokio::test]
    async fn overflow_stops_without_waiting_for_disk() {
        let (tx, _rx) = mpsc::channel(1);
        let mut r = Recorder {
            tx,
            budget: Arc::default(),
        };
        assert!(r.record(0, vec![Message::position(1, 0., 0.)]));
        assert!(!r.record(1, vec![Message::position(2, 0., 0.)]));
        let (tx, _rx) = mpsc::channel(1);
        let mut r = Recorder {
            tx,
            budget: Arc::new(AtomicU64::new(MATCH_LIMIT)),
        };
        assert!(!r.record(2, vec![Message::position(3, 0., 0.)]));
    }
}
