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
        0x3f2
            if m.body.len() == 122
                && (super::compact::recordable_mid_structure(&m.body)
                    || super::compact::recordable_minion_birth(&m.body)
                    || (matches!(
                        u32::from_be_bytes(m.body[..4].try_into().unwrap()),
                        335..=338
                    ) && matches!(
                        m.body[116..122],
                        [52..=199, 0, 0, 1, 1, 1] | [52..=199, 0, 0, 1, 4, 2]
                    ))
                    || super::aral_spawn::recordable(&m.body)) => {}
        0x42b
            if matches!(
                m.body.as_slice(),
                [0, 0, _, _, 0..=7, 1, 0 | 15, 0, 0, 0, 0, 0, 0, 0]
            ) => {}
        0x3f8
            if m.body.len() == 9
                && (14..=199).contains(&m.body[0])
                && m.body[1..]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|b| f32::from_be_bytes(*b).is_finite()) => {}
        // Only the authored hero-slot and target combinations have verified wire meanings.
        0x48a if super::native_cooldown::recordable(m) => {}
        0x435 if super::aral::announcements::recordable(&m.body) => {}
        0x43a
            if matches!(m.body.as_slice(), [0, 0, 5, 220..=229, 0, 0, 0, operand]
            if halcyon_sim::known_native_learn_operand(*operand)) => {}
        0x40d if super::npc_projectile::recordable(m) => {}
        0x43e
            if matches!(m.fields(), Ok(halcyon_protocol::Fields::BuffApplication(effect))
                if (1500..=1505).contains(&effect.prefix.source)
                    && (1500..=1505).contains(&effect.prefix.target)
                    && effect.prefix.source != effect.prefix.target
                    && effect.prefix.instance > 1505
                    && effect.prefix.duration_bits == 0x4400
                    && effect.prefix.definition == 954 && effect.suffix == [0; 6]) => {}
        0x415 if super::npc_attack::actors(m).is_some() => {}
        0x415
            if matches!(
                m.body.as_slice(),
                [0, 0, 5, 220..=229, 0, 0, 5, 220..=229, 2]
            ) => {}
        0x416
            if m.body.len() == 17
                && matches!(m.body[..4], [0, 0, 5, 220..=229])
                && matches!(m.body[16], 0 | 1 | 3)
                && m.body[4..16]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|v| f32::from_be_bytes(*v).is_finite())
                && f32::from_be_bytes(m.body[8..12].try_into().unwrap()) == 0.0 => {}
        0x48c
            if halcyon_protocol::TowerAggro::decode(m).is_some_and(|state| {
                matches!(state.tower_slot, 3..=7 | 9 | 11..=15)
                    && matches!(state.target_slot, None | Some(14..=199))
            }) => {}
        0x424 if matches!(m.body.as_slice(), [_, _, _, _, 0, 0]) => {}
        0x481
            if matches!(
                m.body.as_slice(),
                [0, 0, 5, 220..=225, 255, 0] | [0, 0, 5, 220..=225, 0..=2, 1..=255]
            ) => {}
        0x488 if matches!(m.body.as_slice(), [0, 0, _, _, 0..=2 | 254]) => {}
        0x45c if matches!(m.body.len(), 96 | 102) && m.body[96..].iter().all(|&v| v == 0) => {}
        0x3ef if m.body.is_empty() || m.body == [0; 6] => {}
        0x41e if m.body.len() == 22 && m.body[16..] == [0; 6] => {}
        0x434 if m.body.len() == 14 && m.body[4..] == [0, 0, 0, 1, 0, 0, 0, 0, 0, 0] => {}
        0x409 if matches!(m.fields(),Ok(halcyon_protocol::Fields::ActorRelocation{actor:1500..=1505,xyz,flag:1,remaining:[0,0,0,0,0]}) if [
            [-78.18,1.3,0.88],[-81.81,1.3,1.97],[-77.03,1.3,-1.94],
            [79.66,1.5,-0.91],[78.60,1.5,-5.06],[83.22,1.5,0.72]
        ].contains(&xyz)) => {}
        0x432 if m.body.len() == 22 && m.body[16..] == [0; 6] => {}
        0x433
            if matches!(
                m.fields(),
                Ok(halcyon_protocol::Fields::RespawnTimer {
                    actor: 1500..=1505, seconds, remaining: [0, 0, 0, 0, 0, 0]
                }) if seconds.is_finite() && seconds > 0.0
            ) => {}
        0x431 | 0x40b if matches!(m.body.as_slice(), [0, 0, _, _, 0, 0]) => {}
        0x430 if m.body.len() == 14 && m.body[8..] == [0; 6] => {}
        // A recorded vote contains only the authored three-seat view of this team.
        0x473 if m.body.len() == 155 => {
            let Ok(halcyon_protocol::Fields::Live(
                halcyon_protocol::LiveFields::AralSurrenderBoard(b),
            )) = m.fields()
            else {
                return false;
            };
            if !Message::aral_surrender_board(
                b.wire_team,
                b.phase,
                b.deadline,
                b.actors,
                b.votes,
                b.cooldowns,
            )
            .is_ok_and(|canonical| canonical.body == m.body)
            {
                return false;
            }
        }
        0x452 | 0x48f if m.body.is_empty() => {}
        0x48d if m.body.len() == 1612 => {}
        0x3f1 if matches!(m.body.as_slice(), [0, 0, 0, 1 | 2, 0 | 2, 0]) => {}
        0x438
            if m.body.len() == 6
                && matches!(
                    u32::from_be_bytes(m.body[..4].try_into().unwrap()),
                    1500..=1505
                )
                && m.body[4] != m.body[5]
                && m.body[4..]
                    .iter()
                    .all(|slot| usize::from(*slot) < halcyon_sim::ARAL_ITEM_CAPACITY) => {}
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
        let mut tower = halcyon_protocol::TowerAggro {
            tower_slot: 7,
            target_slot: Some(17),
        }
        .message()
        .unwrap();
        assert!(sanitize(&mut tower));
        tower.body[7] = 1;
        assert!(!sanitize(&mut tower));
        death.body.push(0);
        impact.body[16] = 1;
        assert!(!sanitize(&mut death) && !sanitize(&mut impact));
    }

    #[test]
    fn records_authored_actions_and_discards_request_controls_and_surplus_bytes() {
        let mut move_to = vec![14];
        move_to.extend_from_slice(&(-80.0_f32).to_be_bytes());
        move_to.extend_from_slice(&1.0_f32.to_be_bytes());
        for mut m in [
            Message::new(0x03f8, move_to),
            Message::new(0x0431, [0, 0, 5, 220, 0, 0]),
            Message::new(0x040b, [0, 0, 8, 13, 0, 0]),
            Message::new(0x0424, [0, 0, 5, 221, 0, 0]),
            Message::ability_learned(1501, 3),
            Message::ability_learned(1501, 4),
            Message::ability_point_used(1501, [-40.5, -0.0, 1.], 3),
            Message::new(0x0415, [0, 0, 5, 220, 0, 0, 5, 221, 2]),
            halcyon_protocol::BuffApplication {
                prefix: halcyon_protocol::BuffPrefix {
                    target: 1502,
                    source: 1500,
                    duration_bits: 0x4400,
                    instance: 1800,
                    definition: 954,
                },
                extras: None,
                suffix: &[0; 6],
            }
            .to_message()
            .unwrap(),
            Message::new(0x0481, [0, 0, 5, 220, 255, 0]),
            Message::new(0x0481, [0, 0, 5, 225, 1, 1]),
            Message::new(0x0488, [0, 0, 5, 220, 254]),
        ] {
            assert!(sanitize(&mut m));
            m.body.push(42);
            assert!(!sanitize(&mut m));
        }
        assert!(!sanitize(&mut Message::ability_point_used(
            1501,
            [f32::NAN, 0., 0.],
            0
        )));
        assert!(!sanitize(&mut Message::ability_point_used(
            1501,
            [0., 1., 0.],
            0
        )));
        assert!(!sanitize(&mut Message::ability_learned(1550, 3)));
        assert!(!sanitize(&mut Message::ability_learned(1501, 5)));
        let mut board =
            Message::aral_surrender_board(1, 1, 30., [1500, 1502, 1504], [1, 0, 0], [120.; 3])
                .unwrap();
        assert!(sanitize(&mut board));
        board.body[55] = 42;
        assert!(!sanitize(&mut board));
        for control in [0x0472, 0x03f5, 0x0411, 0x0412, 0x0436, 0x0484, 0x0485] {
            assert!(!sanitize(&mut Message::new(control, [0; 6])));
        }
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
