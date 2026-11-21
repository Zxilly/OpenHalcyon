//! First-human data-channel recording. The socket writer offers bounded batches after a successful write.
use crate::abilities;
use crate::abilities::legacy::{mobile_bulwark, ringo};
use crate::host::manager::Rooms;
use crate::modes::{aral, scene};
use crate::replication::{Batch, compact, npc_attack, npc_projectile, structure_effects};
use crate::{Message, Opcode};
use halcyon_data::EntityTemplate;
use halcyon_protocol::ReplayWriter;
use std::{
    io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::{fs, io::AsyncWriteExt, sync::mpsc};
use uuid::Uuid;

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
pub(crate) struct Recorder {
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
    if mobile_bulwark::recordable(m) {
        return true;
    }
    match m.known_opcode() {
        Some(Opcode::ModifyBuffStack)
            if m.buff_stack().is_some_and(|s| {
                (1500..=1505).contains(&s.target)
                    && s.instance > 1499
                    && (1..=12).contains(&s.count)
                    && s.suffix.is_empty()
            }) => {}
        Some(Opcode::ApplyBuffCompact)
            if matches!(m.fields(), Ok(halcyon_protocol::Fields::BuffApplication(buff))
            if [halcyon_protocol::BuffDefinition::WeaponInfusion as u16,halcyon_protocol::BuffDefinition::CrystalInfusion as u16].contains(&buff.prefix.definition)
            && buff.prefix.duration_bits==0x58b0 && buff.prefix.target==buff.prefix.source
            && (1500..=1505).contains(&buff.prefix.target) && buff.extras.is_none() && buff.suffix.is_empty()) =>
            {}
        Some(Opcode::ActorUndead)
            if halcyon_protocol::ActorUndeadBody::read_exact(&m.body)
                .is_ok_and(|body| (1500..=1505).contains(&body.actor)) => {}
        Some(Opcode::ApplyBuffCompact | Opcode::ModifyBuffDuration | Opcode::ModifyCooldown)
            if abilities::recordable_reboot(m) => {}
        Some(Opcode::PlayerInfo) => {
            let Ok(mut info) = halcyon_protocol::PlayerInfoBody::read_body(&m.body) else {
                return false;
            };
            info.redact();
            let _ = info.write_body(&mut m.body);
        }
        Some(Opcode::DraftRosterFull) => {
            let Ok(mut roster) = halcyon_protocol::DraftRosterBody::read_body(&m.body) else {
                return false;
            };
            if roster.padding.is_none() {
                return false;
            }
            roster.redact_non_bots();
            let _ = roster.write_body(&mut m.body);
        }
        Some(
            Opcode::RequestActivateItemSelf
            | Opcode::RequestActivateItemPoint
            | Opcode::ItemActivated
            | Opcode::SellItem,
        ) => return aral::item_actions::recordable(m),
        Some(Opcode::MoveToServerAuthoritative) if scene::solo_bot_paths::recordable(m) => {}
        // Audited outbound gameplay opcodes only. Unknown and control messages stay out.
        Some(Opcode::EntitySpawn)
            if halcyon_protocol::EntitySpawnBody::read_body(&m.body).is_ok_and(|spawn| {
                compact::recordable_structure(&m.body)
                    || compact::recordable_minion_birth(&m.body)
                    || compact::recordable_camp_unit(&m.body)
                    || (u16::try_from(spawn.template)
                        .ok()
                        .and_then(EntityTemplate::from_raw)
                        .is_some_and(EntityTemplate::is_solo_lane_minion)
                        && (52..=199).contains(&spawn.slot)
                        && spawn.flag117 == 0
                        && spawn.flag118 == 0
                        && spawn.level == 1
                        && matches!(
                            (spawn.lane_index, spawn.team_index),
                            (0..=2, 1) | (3..=5, 2)
                        ))
                    || aral::spawn::recordable(&m.body)
            }) => {}
        Some(Opcode::ModifyVisibility)
            if matches!(
                m.fields(),
                Ok(halcyon_protocol::Fields::ActorVisibility {
                    actor,
                    index,
                    state: 1,
                    masks: [0 | 15, 0],
                    remaining: [0, 0, 0, 0, 0, 0],
                }) if actor < 65536 && index <= 7
            ) => {}
        Some(Opcode::MoveTo)
            if halcyon_protocol::MoveToBody::read_exact(&m.body).is_ok_and(|body| {
                (14..=199).contains(&body.selector) && body.x.is_finite() && body.y.is_finite()
            }) => {}
        // Only the authored hero-slot and target combinations have verified wire meanings.
        Some(Opcode::InitCooldown) if abilities::cooldowns::recordable(m) => {}
        Some(Opcode::ModifyGameModeVar) if scene::mode_variables::recordable(&m.body) => {}
        Some(Opcode::MakeAnnouncement) if aral::announcements::recordable(&m.body) => {}
        Some(Opcode::GrantAbility)
            if halcyon_protocol::GrantAbilityBody::read_exact(&m.body).is_ok_and(|body| {
                (1500..=1509).contains(&body.actor)
                    && u8::try_from(body.operand).is_ok_and(halcyon_sim::known_native_learn_operand)
            }) => {}
        Some(
            Opcode::FirePointProjectile
            | Opcode::FireDirectedProjectile
            | Opcode::DetonateProjectile,
        ) if abilities::recordable_spell_projectile(m) => {}
        Some(Opcode::ApplyBuffCompact | Opcode::AutoActorBounce)
            if abilities::recordable_amael(m) => {}
        Some(Opcode::AutoMoveTo) if abilities::recordable_dash(m) => {}
        Some(Opcode::FireProjectile) if npc_projectile::recordable(m) => {}
        Some(Opcode::ApplyBuffExtended | Opcode::DecrementBuffStack)
            if aral::guard::recordable(m) => {}
        Some(Opcode::ApplyBuffCompact | Opcode::ShowFlyoutText | Opcode::ModifyCooldown)
            if abilities::recordable_skedaddle(m) => {}
        Some(Opcode::ApplyBuffExtended | Opcode::ApplyBuffCompact)
            if structure_effects::recordable(m) => {}
        Some(Opcode::ApplyBuffCompact)
            if matches!(m.fields(), Ok(halcyon_protocol::Fields::BuffApplication(effect))
            if effect.prefix.source == effect.prefix.target && (1500..1506).contains(&effect.prefix.target)
                && effect.prefix.instance > 1505 && matches!((effect.prefix.definition,effect.prefix.duration_bits),
                    (61,0x4066)|(268|18,0x3e00)) && matches!(effect.suffix,[]|[0,0,0,0,0,0])) => {}
        Some(Opcode::TimeoutBuff)
            if m.buff_mutation().is_some_and(|e| {
                (1500..1506).contains(&e.target)
                    && e.instance > 1505
                    && e.padding.iter().all(|&v| v == 0)
            }) => {}
        Some(Opcode::ApplyBuffCompact)
            if matches!(m.fields(), Ok(halcyon_protocol::Fields::BuffApplication(effect))
            if effect.prefix.definition==654 && effect.prefix.duration_bits==0x3c00 && (1500..=1505).contains(&effect.prefix.source)
                && effect.prefix.target!=effect.prefix.source && (1500..=u32::from(u16::MAX)).contains(&effect.prefix.target) && effect.prefix.instance>1505
                && effect.suffix==[0;6]) => {}
        Some(Opcode::ApplyBuffCompact)
            if matches!(m.fields(), Ok(halcyon_protocol::Fields::BuffApplication(effect))
                if (1500..=1505).contains(&effect.prefix.source)
                    && (1500..=1505).contains(&effect.prefix.target)
                    && effect.prefix.instance > 1505
                    && matches!((effect.prefix.definition, effect.prefix.duration_bits,
                                 effect.prefix.source == effect.prefix.target),
                                 (954, 0x4400, false) | (61 | 820, 0x4066, true))
                    && effect.suffix == [0; 6]) => {}
        Some(
            Opcode::ApplyBuffCompact
            | Opcode::CancelBuff
            | Opcode::PlayTargetAbility
            | Opcode::OverrideAbility
            | Opcode::ClearAbilityOverride,
        ) if ringo::references(m).is_some() => {}
        Some(Opcode::PlayPfxAtLocation) if abilities::recordable_chill_winds(m) => {}
        Some(Opcode::PlayTargetAbility)
            if halcyon_protocol::PlayTargetAbilityBody::read_exact(&m.body).is_ok_and(|body| {
                (1500..=1505).contains(&body.actor)
                    && body.target == u32::MAX
                    && matches!(body.selector, 1 | 2)
            }) => {}
        Some(Opcode::PlayTargetAbility) if npc_attack::actors(m).is_some() => {}
        Some(Opcode::PlayTargetAbility)
            if halcyon_protocol::PlayTargetAbilityBody::read_exact(&m.body).is_ok_and(|body| {
                (1500..=1509).contains(&body.actor)
                    && (1500..=1509).contains(&body.target)
                    && body.selector == 2
            }) => {}
        Some(Opcode::PlayPointAbility)
            if halcyon_protocol::PlayPointAbilityBody::read_exact(&m.body).is_ok_and(|body| {
                (1500..=1509).contains(&body.actor)
                    && matches!(body.selector, 0 | 1 | 3)
                    && body.coordinates.iter().all(|v| v.is_finite())
                    && body.coordinates[1] == 0.0
            }) => {}
        Some(Opcode::MoveAndFace) if ringo::recordable_face(m) => {}
        Some(Opcode::SendAggroState)
            if halcyon_protocol::TowerAggro::decode(m).is_some_and(|state| {
                matches!(state.tower_slot, 3..=7 | 9 | 11..=15)
                    && matches!(state.target_slot, None | Some(14..=199))
            }) => {}
        Some(Opcode::RequestModifyBasicAttackTarget)
            if halcyon_protocol::RequestModifyBasicAttackTargetBody::read_padded_zero(&m.body)
                .is_ok_and(|body| body.flag == 0) => {}
        Some(Opcode::EquipTalent)
            if halcyon_protocol::EquippedTalent::read_exact(&m.body).is_ok_and(|talent| {
                (1500..=1505).contains(&talent.actor)
                    && match talent.selection {
                        halcyon_protocol::TalentSelection::NoTalent => talent.level == 0,
                        halcyon_protocol::TalentSelection::Slot0
                        | halcyon_protocol::TalentSelection::Slot1
                        | halcyon_protocol::TalentSelection::Slot2 => talent.level > 0,
                    }
            }) => {}
        Some(Opcode::SetRecommendedBuildPath)
            if halcyon_protocol::SetRecommendedBuildPathBody::read_exact(&m.body)
                .is_ok_and(|body| body.actor < 65536) => {}
        Some(Opcode::SceneReadinessRows)
            if matches!(
                m.fields(),
                Ok(halcyon_protocol::Fields::ControlTable { remaining, .. })
                    if matches!(remaining.len(), 0 | 6) && remaining.iter().all(|&v| v == 0)
            ) => {}
        Some(Opcode::StartMatch)
            if halcyon_protocol::StartMatchBody::read_with_padding(&m.body)
                .is_ok_and(|(_, tail)| tail.iter().all(|&v| v == 0)) => {}
        Some(Opcode::HealthImpact)
            if matches!(
                m.fields(),
                Ok(halcyon_protocol::Fields::HealthImpact {
                    remaining: [0, 0, 0, 0, 0, 0],
                    ..
                })
            ) => {}
        Some(Opcode::LevelUp)
            if halcyon_protocol::LevelUpBody::read_padded_zero(&m.body)
                .is_ok_and(|body| body.operand == 1) => {}
        Some(Opcode::TeleportTo)
            if matches!(m.fields(),Ok(halcyon_protocol::Fields::ActorRelocation{actor:1500..=1505,xyz,flag:1,remaining:[0,0,0,0,0]}) if [
            [-78.18,1.3,0.88],[-81.81,1.3,1.97],[-77.03,1.3,-1.94],
            [79.66,1.5,-0.91],[78.60,1.5,-5.06],[83.22,1.5,0.72]
        ].contains(&xyz)) => {}
        Some(Opcode::ActorRespawn)
            if halcyon_protocol::ActorRespawnBody::read_padded_zero(&m.body).is_ok() => {}
        Some(Opcode::ActorRespawnTimer)
            if matches!(
                m.fields(),
                Ok(halcyon_protocol::Fields::RespawnTimer {
                    actor: 1500..=1505, seconds, remaining: [0, 0, 0, 0, 0, 0]
                }) if seconds.is_finite() && seconds > 0.0
            ) => {}
        Some(Opcode::ActorDead)
            if halcyon_protocol::ActorDeadBody::read_padded_zero(&m.body)
                .is_ok_and(|body| body.actor < 65536) => {}
        Some(Opcode::DestroyEntity)
            if halcyon_protocol::DestroyEntityBody::read_padded_zero(&m.body)
                .is_ok_and(|body| body.actor < 65536) => {}
        Some(Opcode::ActorDeath)
            if matches!(
                m.fields(),
                Ok(halcyon_protocol::Fields::ActorDeath {
                    remaining: [0, 0, 0, 0, 0, 0],
                    ..
                })
            ) => {}
        // A recorded vote contains only the authored three-seat view of this team.
        Some(Opcode::TeamSurrenderStateChanged) => {
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
            .is_ok_and(|canonical| canonical == *m)
            {
                return false;
            }
        }
        Some(Opcode::StopActor) if abilities::recordable_stop(m) => {}
        Some(crate::Opcode::CrystalShattered)
            if halcyon_protocol::CrystalShatteredBody::read_exact(&m.body).is_ok() => {}
        Some(crate::Opcode::RejectBuyItem)
            if halcyon_protocol::RejectBuyItemBody::read_exact(&m.body).is_ok() => {}
        Some(crate::Opcode::PostMatchStats)
            if halcyon_protocol::StatisticsWireBody::read_exact(&m.body).is_ok() => {}
        Some(crate::Opcode::EndMatch)
            if halcyon_protocol::EndMatchBody::read_with_padding(&m.body).is_ok_and(
                |(body, tail)| {
                    tail == [0]
                        && matches!(body.team_word, 1 | 2)
                        && matches!(
                            body.reason,
                            halcyon_protocol::EndMatchReason::CrystalShattered
                                | halcyon_protocol::EndMatchReason::Surrender
                        )
                },
            ) => {}
        Some(crate::Opcode::ReorderItem)
            if matches!(
                m.fields(),
                Ok(halcyon_protocol::Fields::Live(
                    halcyon_protocol::LiveFields::Item(halcyon_protocol::ItemAction::Reorder {
                        actor: 1500..=1505,
                        slots,
                    })
                )) if slots[0] != slots[1]
                    && slots
                        .iter()
                        .all(|slot| usize::from(*slot) < halcyon_sim::ARAL_ITEM_CAPACITY)
            ) => {}
        Some(crate::Opcode::CampVisibility)
            if matches!(
                m.fields(),
                Ok(halcyon_protocol::Fields::CampVisibility { .. })
            ) => {}
        Some(
            crate::Opcode::MatchSettings
            | crate::Opcode::HeroSpawn
            | crate::Opcode::ModifyActorAttribute
            | crate::Opcode::ModifyActorResource
            | crate::Opcode::MoveToCorrection
            | crate::Opcode::BuyItem
            | crate::Opcode::ItemGrant
            | crate::Opcode::ConsumeItem
            | crate::Opcode::SyncWorldClock
            | crate::Opcode::DraftTransitionToLoading
            | crate::Opcode::SceneSettings
            | crate::Opcode::SceneLoadedDismiss,
        ) => {}
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
    use crate::replication::slots::actor_slots;
    use halcyon_protocol::ReplayReader;
    #[test]
    fn records_the_authored_double_down_instance_and_rejects_surplus_bytes() {
        let mut out = Batch::new();
        for attack_target in [None, Some(2)] {
            ringo::append(
                &mut out,
                &halcyon_sim::Event::DoubleDown {
                    source: 1,
                    instance: 100,
                    attack_target,
                },
                |_| true,
            );
        }
        for opcode in [
            crate::wire_opcode::ApplyBuffCompact,
            crate::wire_opcode::CancelBuff,
        ] {
            let mut message = out.iter().find(|m| m.opcode == opcode).unwrap().clone();
            assert!(sanitize(&mut message));
            *message.body.last_mut().unwrap() = 1;
            assert!(!sanitize(&mut message));
        }
    }
    #[test]
    fn relocation_recording_requires_a_real_aral_seat_pad_and_flag() {
        let mut m = Message::actor_relocation(1503, [79.66, 1.5, -0.91], 1);
        assert!(sanitize(&mut m));
        for (offset, byte) in [(0, 1), (5, 0), (16, 0), (17, 1)] {
            let mut altered = m.clone();
            altered.body[offset] = byte;
            assert!(!sanitize(&mut altered));
        }
    }
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
            Message::known(crate::Opcode::MoveTo, move_to),
            Message::known(crate::Opcode::ActorDead, [0, 0, 5, 220, 0, 0]),
            Message::known(crate::Opcode::DestroyEntity, [0, 0, 8, 13, 0, 0]),
            Message::known(
                crate::Opcode::RequestModifyBasicAttackTarget,
                [0, 0, 5, 221, 0, 0],
            ),
            Message::ability_learned(1501, 3),
            Message::ability_learned(1501, 4),
            Message::ability_point_used(1501, [-40.5, -0.0, 1.], 3),
            Message::known(
                crate::Opcode::PlayTargetAbility,
                [0, 0, 5, 220, 0, 0, 5, 221, 2],
            ),
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
            Message::known(crate::Opcode::EquipTalent, [0, 0, 5, 220, 255, 0]),
            Message::known(crate::Opcode::EquipTalent, [0, 0, 5, 225, 1, 1]),
            Message::known(crate::Opcode::SetRecommendedBuildPath, [0, 0, 5, 220, 254]),
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
        for control in [
            crate::wire_opcode::SetSurrenderStateRequest,
            crate::wire_opcode::RequestMovePoint,
            crate::wire_opcode::RequestTargetAbility,
            crate::wire_opcode::RequestPointAbility,
            crate::wire_opcode::RequestUpgradeAbility,
            crate::wire_opcode::RequestEquipTalent,
            crate::wire_opcode::RequestRecommendedBuildPath,
        ] {
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

    #[test]
    fn retains_mask_zero_solo_lane_minions_across_all_three_lanes() {
        let mut world = halcyon_sim::World::new(7, 1);
        world.use_solo_mid_lane().unwrap();
        let mut slots = actor_slots::ActorSlots::from_roster(&world.entities);
        for _ in 0..19 * halcyon_sim::TICK_HZ {
            world.step();
        }
        let mut routes_seen = std::collections::BTreeSet::new();
        for entity in world
            .entities
            .iter()
            .filter(|e| e.kind == halcyon_sim::Kind::Minion)
        {
            let mut damaged = entity.clone();
            damaged.hp -= 10.0;
            let mut message = slots
                .declaration_at(&world, &damaged, &[], None, false)
                .unwrap();
            assert_eq!(message.body[117], 0);
            routes_seen.insert((message.body[120], message.body[121]));
            assert!(
                sanitize(&mut message),
                "expected route {} team {} minion snapshot to be recorded",
                message.body[120],
                message.body[121]
            );
        }
        assert_eq!(
            routes_seen,
            [(0, 1), (1, 1), (2, 1), (3, 2), (4, 2), (5, 2)]
                .into_iter()
                .collect()
        );
        let mut bad = slots
            .declaration_at(
                &world,
                &{
                    let mut m = world
                        .entities
                        .iter()
                        .find(|e| e.kind == halcyon_sim::Kind::Minion && e.team == 0)
                        .unwrap()
                        .clone();
                    m.hp -= 10.0;
                    m
                },
                &[],
                None,
                false,
            )
            .unwrap();
        for (route, team) in [(3, 1), (0, 2), (6, 1)] {
            bad.body[120] = route;
            bad.body[121] = team;
            assert!(!sanitize(&mut bad));
        }
    }
}
