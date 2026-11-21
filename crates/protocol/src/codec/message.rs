//! Wire message container (`Message`), generic field projection (`Fields`), and top-level dispatch.

use crate::codec::{fixed_body, invalid};
use crate::packets::match_ui::statistics;
use crate::spec::{live, outbound, static_actions};
use crate::{
    ActorDeadBody, ActorRespawnBody, ActorUndeadBody, AralRoster, AttributeChange, BuffApplication,
    BuffChange, BuyItemBody, CampVisibilityBody, CampVisibilityState, CancelAbilityBody,
    ClearAbilityOverrideBody, ClockSyncBody, ConfirmSceneOwnerBody, ConsumeItemBody, DeathBody,
    DestroyEntityBody, DraftRoster, DraftRosterBody, EndMatchBody, EntitySpawnBody, EntityStatMask,
    FixedAscii, GrantAbilityBody, HeroSpawnBody, ImpactBody, InitCooldownBody, ItemGrantBody,
    LevelUpBody, LiveFields, MatchSettingsBody, MatchStatistics, ModifyAbilityChargesBody,
    ModifyCooldownBody, MoveAndFaceBody, MoveCmd, MoveToBody, Opcode, OutboundAction,
    OverrideAbilityBody, PlayTargetAbilityBody, PlayerInfoBody, PlayerLoadedSceneBody,
    PositionBody, ProjectileAction, ResourceChange, RespawnTimerBody, SceneReadinessRowsBody,
    SendAggroStateBody, ServerMoveBody, StartMatchBody, StaticAction, StopActorBody,
    TeleportToBody, VisibilityBody, ZoneAction,
};
use binrw::BinRead;
use std::io;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub opcode: u16,
    pub body: Vec<u8>,
}

impl Message {
    pub fn known(opcode: Opcode, body: impl Into<Vec<u8>>) -> Self {
        Self::new(opcode.raw(), body)
    }

    pub fn is(&self, opcode: Opcode) -> bool {
        self.opcode == opcode.raw()
    }

    pub fn known_opcode(&self) -> Option<Opcode> {
        Opcode::try_from(self.opcode).ok()
    }

    pub fn new(opcode: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            opcode,
            body: body.into(),
        }
    }

    pub fn from_plain(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() < 2 {
            return Err(invalid("missing opcode"));
        }
        Ok(Self::new(
            u16::from_be_bytes([bytes[0], bytes[1]]),
            &bytes[2..],
        ))
    }

    pub fn fields(&self) -> io::Result<Fields<'_>> {
        let b = &self.body;
        if self.is(Opcode::CreateRectZoneOfControl)
            && let Some(zone) = self.zone_action()
        {
            return Ok(Fields::Zone(zone));
        }
        if let Some(application) = BuffApplication::decode(self.opcode, b) {
            return Ok(Fields::BuffApplication(application));
        }
        Ok(match self.known_opcode() {
            Some(Opcode::ClockSync) => Fields::Clock(ClockSyncBody::read_body(b)?.seconds),
            Some(Opcode::StartMatch)
                if StartMatchBody::read_with_padding(b)
                    .is_ok_and(|(_, tail)| tail.iter().all(|&v| v == 0)) =>
            {
                Fields::HeaderOnly { remaining: b }
            }
            Some(Opcode::EndMatch)
                if let Ok((body, remaining)) = EndMatchBody::read_with_padding(b) =>
            {
                Fields::MatchRequest {
                    team_word: body.team_word,
                    reason: body.reason.to_wire(),
                    remaining,
                }
            }
            Some(Opcode::PostMatchStats) if statistics::complete(b) => {
                Fields::MatchStatistics(MatchStatistics(b))
            }
            Some(Opcode::OverrideAbility)
                if let Ok((body, _)) = OverrideAbilityBody::read_with_padding(b) =>
            {
                Fields::AbilityOverride {
                    actor: body.actor,
                    key: body.key,
                    replacement: Some(body.replacement),
                    remaining: &b[12..],
                }
            }
            Some(Opcode::ClearAbilityOverride)
                if let Ok((body, remaining)) = ClearAbilityOverrideBody::read_with_padding(b) =>
            {
                Fields::AbilityOverride {
                    actor: body.actor,
                    key: body.key,
                    replacement: None,
                    remaining,
                }
            }
            Some(Opcode::ModifyBuffDuration | Opcode::CancelBuff | Opcode::TimeoutBuff) => self
                .buff_mutation()
                .map_or(Fields::Opaque(b), |action| Fields::BuffAction {
                    actor: action.target,
                    key: action.instance,
                    duration: match action.change {
                        BuffChange::Duration(seconds) => Some(seconds),
                        BuffChange::Cancel | BuffChange::Timeout => None,
                    },
                    remaining: action.padding,
                }),
            Some(Opcode::CampVisibility) if b.len() == CampVisibilityBody::BODY_LEN => {
                let CampVisibilityBody {
                    actor,
                    index,
                    state,
                } = CampVisibilityBody::read_body(b)?;
                Fields::CampVisibility {
                    actor,
                    index,
                    state,
                }
            }
            Some(Opcode::CancelAbility)
                if let Ok((body, _)) = CancelAbilityBody::read_with_padding(b) =>
            {
                Fields::AbilityCancellation {
                    actor: body.actor,
                    reference: body.reference,
                    remaining: &b[8..],
                }
            }
            Some(Opcode::SendAggroState)
                if let Ok((body, _)) = SendAggroStateBody::read_with_padding(b) =>
            {
                Fields::AggroState {
                    prefix: [body.tower_slot, body.target_slot],
                    value: body.value(),
                    remaining: &b[6..],
                }
            }
            Some(Opcode::MoveAndFace)
                if let Ok((body, remaining)) = MoveAndFaceBody::read_with_padding(b) =>
            {
                Fields::MoveAndFace {
                    actor: body.actor,
                    reference: body.reference,
                    values: body.values,
                    remaining,
                }
            }
            Some(Opcode::StopActor)
                if let Ok((body, remaining)) = StopActorBody::read_with_padding(b) =>
            {
                Fields::StopActor {
                    actor: body.actor,
                    values: body.values,
                    flag: body.flag,
                    remaining,
                }
            }
            Some(Opcode::MoveToServerAuthoritative)
                if let Ok((body, remaining)) = ServerMoveBody::read_with_padding(b) =>
            {
                Fields::ServerMove {
                    selector: body.selector,
                    values: body.values,
                    flag: body.flag,
                    remaining,
                }
            }
            Some(Opcode::TeleportTo)
                if let Ok((body, remaining)) = TeleportToBody::read_with_padding(b) =>
            {
                Fields::ActorRelocation {
                    actor: body.actor,
                    xyz: body.position,
                    flag: body.flag,
                    remaining,
                }
            }
            Some(Opcode::ActorRespawn)
                if let Ok((body, remaining)) = ActorRespawnBody::read_with_padding(b) =>
            {
                Fields::Respawn {
                    actor: body.actor,
                    position: body.position,
                    remaining,
                }
            }
            Some(Opcode::ActorRespawnTimer)
                if let Ok((body, remaining)) = RespawnTimerBody::read_with_padding(b) =>
            {
                Fields::RespawnTimer {
                    actor: body.actor,
                    seconds: body.seconds,
                    remaining,
                }
            }
            Some(Opcode::LevelUp)
                if let Ok((body, remaining)) = LevelUpBody::read_with_padding(b) =>
            {
                Fields::ProgressAction {
                    kind: Opcode::LevelUp,
                    actor: body.actor,
                    operand: body.operand,
                    remaining,
                }
            }
            Some(Opcode::GrantAbility)
                if let Ok((body, remaining)) = GrantAbilityBody::read_with_padding(b) =>
            {
                Fields::ProgressAction {
                    kind: Opcode::GrantAbility,
                    actor: body.actor,
                    operand: body.operand,
                    remaining,
                }
            }
            Some(op @ (Opcode::DestroyEntity | Opcode::ActorUndead | Opcode::ActorDead)) => {
                let actor = match op {
                    Opcode::DestroyEntity => DestroyEntityBody::read_body(b)?.actor,
                    Opcode::ActorUndead => ActorUndeadBody::read_body(b)?.actor,
                    Opcode::ActorDead => ActorDeadBody::read_body(b)?.actor,
                    _ => unreachable!(),
                };
                Fields::ActorLifecycle {
                    kind: op,
                    actor,
                    remaining: &b[4..],
                }
            }
            Some(
                op @ (Opcode::FireProjectile
                | Opcode::FirePointProjectile
                | Opcode::FireDirectedProjectile
                | Opcode::DetonateProjectile),
            ) => ProjectileAction::read_body(op, b).map_or(Fields::Opaque(b), Fields::Projectile),
            Some(Opcode::PlayTargetAbility)
                if let Ok((body, remaining)) = PlayTargetAbilityBody::read_with_padding(b) =>
            {
                Fields::AbilityAction {
                    actor: body.actor,
                    reference: body.target,
                    selector: body.selector,
                    remaining,
                }
            }
            Some(Opcode::ModifyCooldown)
                if let Ok((body, remaining)) = ModifyCooldownBody::read_with_padding(b) =>
            {
                Fields::CooldownValue {
                    owner: body.owner,
                    key: body.key,
                    value: body.value,
                    mode: body.mode,
                    remaining,
                }
            }
            Some(Opcode::ModifyAbilityCharges)
                if let Ok((body, remaining)) = ModifyAbilityChargesBody::read_with_padding(b) =>
            {
                Fields::AbilityCharges {
                    owner: body.owner,
                    ability_slot: body.ability_slot,
                    operand: body.operand,
                    mode: body.mode,
                    remaining,
                }
            }
            Some(Opcode::InitCooldown) if let Ok(body) = InitCooldownBody::read_exact(b) => {
                Fields::CooldownSync {
                    owner: body.owner,
                    key: body.key,
                    value: body.value,
                    config_value: body.config_value,
                    raw: b[16..22].try_into().unwrap(),
                }
            }
            Some(Opcode::ActorDeath) => {
                let DeathBody { victim, source } = DeathBody::read_body(b)?;
                Fields::ActorDeath {
                    victim,
                    source,
                    remaining: &b[8..],
                }
            }
            Some(Opcode::HealthImpact) => {
                let ImpactBody {
                    target,
                    source,
                    operand,
                    word,
                    byte,
                    flag,
                } = ImpactBody::read_body(b)?;
                Fields::HealthImpact {
                    target,
                    source,
                    operand,
                    word,
                    byte,
                    flag,
                    remaining: &b[16..],
                }
            }
            Some(Opcode::MatchSettings) => {
                let prefix = MatchSettingsBody::read_prefix(b)?;
                let mode_len = prefix.mode.trimmed_bytes().len();
                Fields::GameSetup {
                    first_word: prefix.local_actor,
                    state_input: prefix.state_input,
                    word8: prefix.hero_template,
                    word12: prefix.skin_hash,
                    word96: prefix.word96,
                    mode: &b[32..32 + mode_len],
                    remaining: &b[MatchSettingsBody::PREFIX_LEN..],
                }
            }
            Some(Opcode::MoveTo)
                if let Ok((body, remaining)) = MoveToBody::read_with_padding(b) =>
            {
                Fields::MoveAction {
                    selector: body.selector,
                    x: body.x,
                    y: body.y,
                    remaining,
                }
            }
            Some(Opcode::ModifyVisibility) if matches!(b.len(), 8 | 14) => {
                let VisibilityBody {
                    actor,
                    index,
                    state,
                    masks,
                } = VisibilityBody::read_body(b)?;
                Fields::ActorVisibility {
                    actor,
                    index,
                    state,
                    masks,
                    remaining: &b[VisibilityBody::BODY_LEN..],
                }
            }
            Some(
                op @ (Opcode::ModifyVisibility
                | Opcode::ApplyBuffCompact
                | Opcode::ApplyBuffExtended),
            ) => Fields::EntityPrefix {
                kind: op,
                entity: fixed_body::read(b)?,
                remaining: &b[4..],
            },
            Some(Opcode::PlayerLoadedScene) => {
                let body = PlayerLoadedSceneBody::read_prefix(b)?;
                let len = body.player_name.trimmed_bytes().len();
                Fields::HandleRecord {
                    text: &b[..len],
                    field64: body.seat,
                    lookup: body.actor,
                    remaining: &b[PlayerLoadedSceneBody::BODY_LEN..],
                }
            }
            Some(Opcode::SceneSettings) => {
                let name: FixedAscii<64> = fixed_body::read(b)?;
                let len = name.trimmed_bytes().len();
                Fields::NameField {
                    kind: Opcode::SceneSettings,
                    text: &b[..len],
                    remaining: &b[FixedAscii::<64>::LEN..],
                }
            }
            Some(Opcode::SceneReadinessRows) => {
                let table = SceneReadinessRowsBody::read_prefix(b)?;
                let entries = table.rows.map(|row| ControlEntry {
                    reference: row.reference,
                    gate: row.gate,
                    flag: row.flag,
                });
                Fields::ControlTable {
                    entries,
                    remaining: &b[SceneReadinessRowsBody::BODY_LEN..],
                }
            }
            Some(Opcode::ConfirmSceneOwner) => {
                let body = ConfirmSceneOwnerBody::read_prefix(b)?;
                Fields::ReferencedFlag {
                    reference: body.actor,
                    flag: body.flag,
                    remaining: &b[ConfirmSceneOwnerBody::BODY_LEN..],
                }
            }
            Some(
                Opcode::DraftRefreshRequest
                | Opcode::SceneReadyConfirm
                | Opcode::RequestSceneSnapshot
                | Opcode::SceneLoadedDismiss
                | Opcode::Unresolved047C,
            ) => {
                let (flag, remaining) = b
                    .split_first()
                    .ok_or_else(|| invalid("truncated control byte"))?;
                Fields::ControlByte {
                    value: *flag,
                    remaining,
                }
            }
            Some(Opcode::EntitySpawn) if let Ok((spawn, _)) = EntitySpawnBody::read_padded(b) => {
                Fields::CompactSpawn {
                    template: spawn.template,
                    skin: spawn.skin,
                    entity: spawn.actor,
                    lane_minion: LaneMinionSpawn::from_entity(&spawn),
                    remaining: &b[12..],
                }
            }
            Some(Opcode::DraftRosterAral) if b.len() >= AralRoster::BODY_BYTES => {
                Fields::AralRoster(AralRoster::new(b)?)
            }
            Some(Opcode::DraftRosterAral) => {
                let (remaining_seconds, allotted_seconds) = DraftRosterBody::read_timers_prefix(b)?;
                Fields::AralRosterHeader {
                    remaining_seconds,
                    allotted_seconds,
                    remaining: &b[8..],
                }
            }
            Some(Opcode::DraftRosterFull) => Fields::DraftRoster(DraftRoster::new(b)?),
            Some(Opcode::ModifyActorAttribute) => {
                let prefix = AttributeChange::read_stat_modifier(b)?;
                Fields::StatModifier {
                    actor: prefix.actor,
                    source: prefix.source,
                    value: prefix.value,
                    stat: prefix.index,
                    flags: prefix.layer,
                }
            }
            Some(Opcode::ConsumeItem) => {
                let body = ConsumeItemBody::read_body(b)?;
                Fields::ItemRemoval {
                    actor: body.actor,
                    instance: body.instance,
                    flags: body.flags,
                }
            }
            Some(Opcode::ItemGrant) => {
                let body = ItemGrantBody::read_body(b)?;
                Fields::ItemGrant {
                    actor: body.actor,
                    item: body.item,
                    instance: body.instance,
                }
            }
            Some(Opcode::DraftSelectSkin) => Fields::SkinSelection(fixed_body::read(b)?),
            Some(Opcode::BuyItem) => {
                let body = BuyItemBody::read_body(b)?;
                Fields::ShopRequest {
                    actor: body.actor,
                    item: body.item,
                }
            }
            Some(Opcode::DraftSelectHero) => {
                #[derive(BinRead)]
                #[br(big)]
                struct SelectHeroWithFlags {
                    template: u32,
                    flags: u16,
                }
                #[derive(BinRead)]
                #[br(big)]
                struct SelectHeroWithSkin {
                    template: u32,
                    skin: u32,
                }
                if b.len() == 6 {
                    let parsed: SelectHeroWithFlags = fixed_body::read(b)?;
                    Fields::HeroSelection {
                        template: parsed.template,
                        choice: DraftHeroChoice::Flags(parsed.flags),
                    }
                } else {
                    let parsed: SelectHeroWithSkin = fixed_body::read(b)?;
                    Fields::HeroSelection {
                        template: parsed.template,
                        choice: DraftHeroChoice::Skin(parsed.skin),
                    }
                }
            }
            Some(Opcode::HeroSpawn) => {
                let prefix = HeroSpawnBody::read_summary_prefix(b)?;
                Fields::Spawn {
                    template: prefix.template,
                    skin: prefix.skin,
                    entity: prefix.actor,
                    team: prefix.team,
                    x: prefix.position[0],
                    height: prefix.position[1],
                    z: prefix.position[2],
                    hp: prefix.hp,
                    max_hp: prefix.max_hp,
                }
            }
            Some(Opcode::PlayerInfo) => {
                let prefix = PlayerInfoBody::read_summary_prefix(b)?;
                Fields::PlayerInfo {
                    entity: prefix.actor_id,
                    template: prefix.hero_template,
                    skin: prefix.skin_hash,
                }
            }

            Some(Opcode::ServerHelloAck) => Fields::Heartbeat,
            Some(Opcode::DraftReadyAccept | Opcode::DraftLockHero) => Fields::HeroLock,
            Some(Opcode::RequestMoveJoystick) => {
                let MoveCmd { x, y } = MoveCmd::read_body(b)?;
                Fields::Movement { x, y }
            }
            Some(Opcode::MoveToCorrection) => {
                let PositionBody { actor, x, y } = PositionBody::read_body(b)?;
                // The public Fields projection has always reported the low halfword.
                Fields::Position {
                    entity: actor as u16,
                    x,
                    y,
                }
            }
            Some(Opcode::ModifyActorResource) => {
                let prefix = ResourceChange::read_entity_stat_prefix(b)?;
                Fields::EntityStat {
                    entity: prefix.actor as u16,
                    value: prefix.value,
                    stat: prefix.index,
                }
            }
            Some(
                Opcode::RegisterPlayerUuid
                | Opcode::DraftCatalogEntry
                | Opcode::DraftModeAnnouncement,
            ) => {
                let offset = if self.is(Opcode::DraftModeAnnouncement) {
                    4
                } else {
                    0
                };
                let raw = b
                    .get(offset..)
                    .ok_or_else(|| invalid("truncated string header"))?;
                let name = std::str::from_utf8(raw.split(|v| *v == 0).next().unwrap())
                    .map_err(|_| invalid("invalid text"))?;
                match self.known_opcode() {
                    Some(Opcode::RegisterPlayerUuid) => Fields::PlayerUuid(name),
                    Some(Opcode::DraftCatalogEntry) => Fields::HeroCatalog(name),
                    _ => Fields::GameMode(name),
                }
            }
            _ => live::fields(self.opcode, b)
                .or_else(|| outbound::fields(self.opcode, b))
                .or_else(|| static_actions::fields(self.opcode, b))
                .unwrap_or(Fields::Opaque(b)),
        })
    }
}

#[derive(Debug, PartialEq)]
pub struct ControlEntry {
    pub reference: u32,
    pub gate: u8,
    pub flag: u8,
}

/// Endian-converted operands from an Android action reader. The opcode determines the layout.
#[derive(Debug, PartialEq)]
pub struct ActionWords<'a> {
    pub(crate) words: &'a [[u8; 4]],
    pub halfword: Option<u16>,
    pub bytes: &'a [u8],
    pub remaining: &'a [u8],
}

impl ActionWords<'_> {
    pub fn words(&self) -> impl ExactSizeIterator<Item = u32> + '_ {
        self.words.iter().copied().map(u32::from_be_bytes)
    }
}

/// Fields shared by the four captured Android Solo lane-minion templates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LaneMinionSpawn {
    pub slot: u8,
    pub stat_mask: u8,
    pub route: u8,
    pub wire_team: u8,
    /// Current and maximum HP; the masked creation form omits both values.
    pub health: Option<(f32, f32)>,
}

impl LaneMinionSpawn {
    fn from_entity(spawn: &EntitySpawnBody) -> Option<Self> {
        if !(335..=338).contains(&spawn.template)
            || spawn.skin != EntitySpawnBody::MINION_SKIN
            || !matches!(
                (spawn.lane_index, spawn.team_index),
                (0..=2, 1) | (3..=5, 2)
            )
        {
            return None;
        }
        let health = match spawn.stat_mask() {
            EntityStatMask::Full => Some((spawn.hp, spawn.max_hp)),
            EntityStatMask::Pristine => None,
            EntityStatMask::Unknown(_) => return None,
        };
        Some(Self {
            slot: spawn.slot,
            stat_mask: spawn.flag117,
            route: spawn.lane_index,
            wire_team: spawn.team_index,
            health,
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum DraftHeroChoice {
    Skin(u32),
    Flags(u16),
}

#[derive(Debug, PartialEq)]
pub enum Fields<'a> {
    Zone(ZoneAction<'a>),
    Live(LiveFields<'a>),
    StaticAction(StaticAction<'a>),
    Outbound(OutboundAction<'a>),
    HeaderOnly {
        remaining: &'a [u8],
    },
    MatchRequest {
        team_word: u32,
        reason: u8,
        remaining: &'a [u8],
    },
    ActionWords(ActionWords<'a>),
    MatchStatistics(MatchStatistics<'a>),
    AbilityOverride {
        actor: u32,
        key: u32,
        replacement: Option<u32>,
        remaining: &'a [u8],
    },
    BuffApplication(BuffApplication<'a>),
    BuffAction {
        actor: u32,
        key: u32,
        duration: Option<f32>,
        remaining: &'a [u8],
    },
    CampVisibility {
        actor: u32,
        index: u8,
        state: CampVisibilityState,
    },
    AbilityCancellation {
        actor: u32,
        reference: u32,
        remaining: &'a [u8],
    },
    AggroState {
        prefix: [u8; 2],
        value: f32,
        remaining: &'a [u8],
    },
    MoveAndFace {
        actor: u32,
        reference: u32,
        values: [f32; 3],
        remaining: &'a [u8],
    },
    StopActor {
        actor: u32,
        values: [f32; 3],
        flag: u8,
        remaining: &'a [u8],
    },
    ServerMove {
        selector: u8,
        values: [f32; 3],
        flag: u8,
        remaining: &'a [u8],
    },
    ActorRelocation {
        actor: u32,
        xyz: [f32; 3],
        flag: u8,
        remaining: &'a [u8],
    },
    Respawn {
        actor: u32,
        position: [f32; 3],
        remaining: &'a [u8],
    },
    RespawnTimer {
        actor: u32,
        seconds: f32,
        remaining: &'a [u8],
    },
    ProgressAction {
        kind: Opcode,
        actor: u32,
        operand: u32,
        remaining: &'a [u8],
    },
    ActorLifecycle {
        kind: Opcode,
        actor: u32,
        remaining: &'a [u8],
    },
    Projectile(ProjectileAction<'a>),
    AbilityAction {
        actor: u32,
        reference: u32,
        selector: u8,
        remaining: &'a [u8],
    },
    CooldownValue {
        owner: u32,
        key: u32,
        value: f32,
        mode: u8,
        remaining: &'a [u8],
    },
    AbilityCharges {
        owner: u32,
        ability_slot: u32,
        operand: u32,
        mode: u8,
        remaining: &'a [u8],
    },
    CooldownSync {
        owner: u32,
        key: u32,
        value: f32,
        config_value: f32,
        raw: &'a [u8; 6],
    },
    ActorDeath {
        victim: u32,
        source: u32,
        remaining: &'a [u8],
    },
    HealthImpact {
        target: u32,
        source: u32,
        operand: f32,
        word: u16,
        byte: u8,
        flag: u8,
        remaining: &'a [u8],
    },
    GameSetup {
        first_word: u32,
        /// Raw wire value. Android can clear this word before testing whether it is zero.
        state_input: u32,
        word8: u32,
        word12: u32,
        word96: u32,
        mode: &'a [u8],
        remaining: &'a [u8],
    },
    MoveAction {
        selector: u8,
        x: f32,
        y: f32,
        remaining: &'a [u8],
    },
    HandleRecord {
        text: &'a [u8],
        field64: u32,
        lookup: u32,
        remaining: &'a [u8],
    },
    ControlTable {
        entries: [ControlEntry; 16],
        remaining: &'a [u8],
    },
    ReferencedFlag {
        reference: u32,
        flag: u8,
        remaining: &'a [u8],
    },
    ControlByte {
        value: u8,
        remaining: &'a [u8],
    },
    ActorVisibility {
        actor: u32,
        index: u8,
        state: u8,
        masks: [u8; 2],
        remaining: &'a [u8],
    },
    EntityPrefix {
        kind: Opcode,
        entity: u32,
        remaining: &'a [u8],
    },
    NameField {
        kind: Opcode,
        text: &'a [u8],
        remaining: &'a [u8],
    },
    CompactSpawn {
        template: u32,
        skin: u32,
        entity: u32,
        lane_minion: Option<LaneMinionSpawn>,
        remaining: &'a [u8],
    },
    AralRosterHeader {
        remaining_seconds: f32,
        allotted_seconds: f32,
        remaining: &'a [u8],
    },
    DraftRoster(DraftRoster<'a>),
    AralRoster(AralRoster<'a>),
    StatModifier {
        actor: u32,
        source: u32,
        value: f32,
        stat: u8,
        flags: u8,
    },
    ItemRemoval {
        actor: u32,
        instance: u32,
        flags: u16,
    },
    ItemGrant {
        actor: u32,
        item: u32,
        instance: u32,
    },
    SkinSelection(u32),
    ShopRequest {
        actor: u32,
        item: u32,
    },
    HeroSelection {
        template: u32,
        choice: DraftHeroChoice,
    },
    Spawn {
        template: u32,
        skin: u32,
        entity: u32,
        team: u32,
        x: f32,
        height: f32,
        z: f32,
        hp: f32,
        max_hp: f32,
    },
    PlayerInfo {
        entity: u32,
        template: u32,
        skin: u32,
    },
    Clock(f32),
    Heartbeat,
    HeroLock,
    Movement {
        x: f32,
        y: f32,
    },
    Position {
        entity: u16,
        x: f32,
        y: f32,
    },
    EntityStat {
        entity: u16,
        value: f32,
        stat: u8,
    },
    PlayerUuid(&'a str),
    HeroCatalog(&'a str),
    GameMode(&'a str),
    Opaque(&'a [u8]),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MatchCipher;

    #[test]
    fn spectator_exit_accepts_only_the_wrapped_zero_and_zero_alignment() {
        for (bytes, ok) in [
            (&[0][..], true),
            (&[0; 6][..], true),
            (&[][..], false),
            (&[1][..], false),
            (&[0, 0, 0, 0, 0, 1][..], false),
        ] {
            assert_eq!(
                Message::known(Opcode::SpectatorExitMatchRequest, bytes).is_spectator_exit(),
                ok
            );
        }
    }

    #[test]
    fn relocation_retains_all_coordinates_flag_and_padding() {
        let mut m = Message::actor_relocation(1503, [79.66, 1.5, -0.91], 1);
        assert_eq!(&m.body[8..12], &1.5f32.to_be_bytes());
        assert!(matches!(
            m.fields(),
            Ok(Fields::ActorRelocation {
                actor: 1503,
                xyz: [79.66, 1.5, -0.91],
                flag: 1,
                remaining: [0, 0, 0, 0, 0]
            })
        ));
        m.body.truncate(17);
        m.body[16] = 0;
        assert!(matches!(
            m.fields(),
            Ok(Fields::ActorRelocation {
                flag: 0,
                remaining: [],
                ..
            })
        ));
    }

    #[test]
    fn respawn_timer_keeps_fractional_seconds_in_a_padded_frame() {
        let timer = Message::actor_respawn_timer(1503, 13.4974);
        assert_eq!(timer.opcode, Opcode::ActorRespawnTimer as u16);
        let cipher = MatchCipher::new("local-respawn-timer-test");
        let wire = cipher.encode(&timer).unwrap();
        let decoded = cipher.decode(&wire[2..]).unwrap();
        assert_eq!(decoded, timer);
        assert_eq!(
            decoded.fields().unwrap(),
            Fields::RespawnTimer {
                actor: 1503,
                seconds: 13.4974,
                remaining: &[0; 6],
            }
        );
    }

    #[test]
    fn action_layouts_convert_only_measured_operands() {
        for (op, n, half, bytes) in [
            (crate::wire_opcode::AutoMoveTo, 5, false, 1),
            (crate::wire_opcode::AutoOrbit, 8, false, 0),
            (crate::wire_opcode::CancelAutoOrbit, 4, false, 0),
            (crate::wire_opcode::PlayPointAbility, 4, false, 1),
            (crate::wire_opcode::PlayPfxAtLocation, 6, false, 0),
            (crate::wire_opcode::ModifyGameModeVar, 2, false, 0),
            (crate::wire_opcode::ModifyBuffStack, 3, false, 0),
            (crate::wire_opcode::SetMinimapSpawnPhase, 2, false, 1),
        ] {
            let words: Vec<_> = (0..n).map(|j| 0xa0123400u32 + j).collect();
            let mut b: Vec<_> = words.iter().flat_map(|w| w.to_be_bytes()).collect();
            if half {
                b.extend(0xcde1u16.to_be_bytes());
            }
            b.extend([0x89, 0xab].into_iter().take(bytes));
            let len = b.len();
            let padded = (len + 2).next_multiple_of(8) - 2;
            for size in [len, padded] {
                b.resize(size, 0xa5);
                let message = Message::new(op, b.as_slice());
                let Fields::ActionWords(a) = message.fields().unwrap() else {
                    panic!("opcode {op:x}")
                };
                assert_eq!(a.words().collect::<Vec<_>>(), words);
                assert_eq!(a.halfword, half.then_some(0xcde1));
                assert_eq!(a.bytes, &[0x89, 0xab][..bytes]);
                assert_eq!(a.remaining, &b[len..]);
            }
            assert!(matches!(
                Message::new(op, &b[..len - 1]).fields().unwrap(),
                Fields::Opaque(_)
            ));
        }
    }

    #[test]
    fn android_match_markers_and_sixteen_statistics_slots() {
        assert!(matches!(
            Message::known(Opcode::StartMatch, [0; 6]).fields().unwrap(),
            Fields::HeaderOnly { .. }
        ));
        assert!(matches!(
            Message::known(Opcode::StartMatch, [1; 6]).fields().unwrap(),
            Fields::Opaque(_)
        ));
        assert!(matches!(
            Message::known(Opcode::EndMatch, [0x10, 0, 0, 2, 7, 0xaa])
                .fields()
                .unwrap(),
            Fields::MatchRequest {
                team_word: 0x10000002,
                reason: 7,
                remaining: [0xaa]
            }
        ));
        let mut b = vec![0u8; 1614];
        let i = 15;
        b[4 * i..4 * i + 4].copy_from_slice(&0x10203040u32.to_be_bytes());
        b[0x40 + i] = 2;
        b[0xa0 + 4 * i..0xa4 + 4 * i].copy_from_slice(&123.5f32.to_be_bytes());
        b[0x220 + 2 * i..0x222 + 2 * i].copy_from_slice(&0x1234u16.to_be_bytes());
        let item = 0x300 + 40 * i + 9 * 4;
        b[item..item + 4].copy_from_slice(&0xba987654u32.to_be_bytes());
        b[0x50 + i] = 255;
        b[0x62c..0x62e].copy_from_slice(&[1, 2]);
        let inputs = [0u32, 0x80000000, 0x7fc01234, 0x3f800000];
        for (index, word) in inputs.iter().enumerate() {
            let offset = 0x63c + index * 4;
            b[offset..offset + 4].copy_from_slice(&word.to_be_bytes());
        }
        let m = Message::known(Opcode::PostMatchStats, b.as_slice());
        let Fields::MatchStatistics(stats) = m.fields().unwrap() else {
            panic!()
        };
        assert_eq!(stats.team_ids(), [1, 2]);
        assert_eq!(stats.minion_damage_ratio_inputs(), inputs);
        let last = stats.slot(i).unwrap();
        assert_eq!(last.slot_flag, 255);
        assert_eq!(
            (
                last.player,
                last.team,
                last.weapon_damage_to_heroes,
                last.net_worth_at_five,
                last.items[9]
            ),
            (0x10203040, 2, 123.5, 0x1234, 0xba987654)
        );
        assert!(stats.slot(16).is_none());
        assert!(matches!(
            Message::known(Opcode::PostMatchStats, &b[..1611])
                .fields()
                .unwrap(),
            Fields::Opaque(_)
        ));
    }

    #[test]
    fn game_setup_retains_raw_state_input_and_other_converted_words() {
        for state_input in [0u32, 1, 0x01020304] {
            let mut b = vec![0; 102];
            for (offset, value) in [
                (0, 51u32),
                (4, state_input),
                (8, 0x01234567),
                (12, 0x89abcdef),
                (96, 0x3f800000),
            ] {
                b[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
            }
            b[32..36].copy_from_slice(b"mode");
            b[100..].copy_from_slice(&[7, 8]);
            assert_eq!(
                Message::known(Opcode::MatchSettings, b.clone())
                    .fields()
                    .unwrap(),
                Fields::GameSetup {
                    first_word: 51,
                    state_input,
                    word8: 0x01234567,
                    word12: 0x89abcdef,
                    word96: 0x3f800000,
                    mode: b"mode",
                    remaining: &[7, 8]
                }
            );
            assert!(
                Message::known(Opcode::MatchSettings, &b[..99])
                    .fields()
                    .is_err()
            );
        }
    }

    #[test]
    fn paired_ability_and_buff_records_share_the_same_key() {
        let b = [0, 1, 0, 0, 0, 0, 0, 7, 0x40, 0, 0, 0, 8, 9];
        for (op, replacement, n) in [
            (crate::wire_opcode::OverrideAbility, Some(0x40000000), 12),
            (crate::wire_opcode::ClearAbilityOverride, None, 8),
        ] {
            assert_eq!(
                Message::new(op, b).fields().unwrap(),
                Fields::AbilityOverride {
                    actor: 65536,
                    key: 7,
                    replacement,
                    remaining: &b[n..]
                }
            );
        }
        for (op, duration, n) in [
            (crate::wire_opcode::ModifyBuffDuration, Some(2.), 12),
            (crate::wire_opcode::CancelBuff, None, 8),
            (crate::wire_opcode::TimeoutBuff, None, 8),
        ] {
            assert_eq!(
                Message::new(op, b).fields().unwrap(),
                Fields::BuffAction {
                    actor: 65536,
                    key: 7,
                    duration,
                    remaining: &b[n..]
                }
            );
        }
        let aggro = [4, 5, 0x40, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8];
        assert_eq!(
            Message::known(Opcode::SendAggroState, aggro)
                .fields()
                .unwrap(),
            Fields::AggroState {
                prefix: [4, 5],
                value: 2.,
                remaining: &aggro[6..]
            }
        );
    }

    #[test]
    fn ability_charge_messages_preserve_integer_operands_and_padding() {
        let h5810 = Message::modify_ability_charges(1504, 0, 4, 1);
        assert_eq!(h5810.body, [0, 0, 5, 224, 0, 0, 0, 0, 0, 0, 0, 4, 1, 0]);
        let mut synthetic = Message::modify_ability_charges(1505, 2, u32::MAX, 0);
        synthetic.body[13] = 7;
        assert_eq!(
            synthetic.fields().unwrap(),
            Fields::AbilityCharges {
                owner: 1505,
                ability_slot: 2,
                operand: u32::MAX,
                mode: 0,
                remaining: &[7],
            }
        );
        synthetic.body.pop();
        assert!(matches!(
            synthetic.fields(),
            Ok(Fields::AbilityCharges {
                operand: u32::MAX,
                mode: 0,
                remaining: [],
                ..
            })
        ));
        synthetic.body.pop();
        assert!(!matches!(
            synthetic.fields(),
            Ok(Fields::AbilityCharges { .. })
        ));
    }

    #[test]
    fn ability_projectile_and_cooldown_prefixes_remain_distinct() {
        let b = [
            0, 1, 0, 0, 0, 0, 0, 2, 0x3f, 0, 0, 0, 0x40, 0, 0, 0, 7, 8, 9, 10, 11, 12,
        ];
        assert_eq!(
            Message::known(Opcode::InitCooldown, b).fields().unwrap(),
            Fields::CooldownSync {
                owner: 65536,
                key: 2,
                value: 0.5,
                config_value: 2.,
                raw: &[7, 8, 9, 10, 11, 12]
            }
        );
        assert_eq!(
            Message::known(Opcode::ModifyCooldown, &b[..14])
                .fields()
                .unwrap(),
            Fields::CooldownValue {
                owner: 65536,
                key: 2,
                value: 0.5,
                mode: 0x40,
                remaining: &[0]
            }
        );
        let m = Message::known(Opcode::FireProjectile, b);
        let Fields::Projectile(ProjectileAction::FireTracked(f)) = m.fields().unwrap() else {
            panic!()
        };
        assert_eq!((f.projectile, f.profile_word, f.values), (65536, 2, [0.5]));
        assert_eq!(f.padding, &b[17..]);
        assert_eq!(
            Message::known(Opcode::PlayTargetAbility, &b[..14])
                .fields()
                .unwrap(),
            Fields::AbilityAction {
                actor: 65536,
                reference: 2,
                selector: 0x3f,
                remaining: &b[9..14]
            }
        );
        assert!(matches!(
            Message::known(Opcode::InitCooldown, &b[..21])
                .fields()
                .unwrap(),
            Fields::Opaque(_)
        ));
        for (code, kind) in [
            (crate::wire_opcode::DestroyEntity, Opcode::DestroyEntity),
            (crate::wire_opcode::ActorDead, Opcode::ActorDead),
        ] {
            assert_eq!(
                Message::new(code, &b[..6]).fields().unwrap(),
                Fields::ActorLifecycle {
                    kind,
                    actor: 65536,
                    remaining: &b[4..6]
                }
            );
        }
    }

    #[test]
    fn health_and_death_keep_raw_presentation_fields_and_padding() {
        let data = [0, 1, 0, 2, 0, 3, 0, 4, 0xc0, 0x60, 0, 0, 0, 74, 2, 1, 0xf1];
        let msg = Message::known(Opcode::HealthImpact, data);
        assert_eq!(
            msg.fields().unwrap(),
            Fields::HealthImpact {
                target: 65538,
                source: 196612,
                operand: -3.5,
                word: 74,
                byte: 2,
                flag: 1,
                remaining: &[0xf1]
            }
        );
        assert!(
            Message::known(Opcode::HealthImpact, &data[..15])
                .fields()
                .is_err()
        );
        assert_eq!(
            Message::known(Opcode::ActorDeath, &data[..9])
                .fields()
                .unwrap(),
            Fields::ActorDeath {
                victim: 65538,
                source: 196612,
                remaining: &[0xc0]
            }
        );
        assert!(
            Message::known(Opcode::ActorDeath, &data[..7])
                .fields()
                .is_err()
        );
    }

    #[test]
    fn compact_lane_minion_health_is_present_only_in_the_full_stat_form() {
        for template in 335u32..=338 {
            for size in [122, 126] {
                for mask in [0, 1] {
                    let mut body = vec![0xab; size];
                    body[..4].copy_from_slice(&template.to_be_bytes());
                    body[4..8].copy_from_slice(&0xeb39ce55u32.to_be_bytes());
                    body[36..40].copy_from_slice(&321.5f32.to_be_bytes());
                    body[40..44].copy_from_slice(&475f32.to_be_bytes());
                    body[116..122].copy_from_slice(&[56, mask, 0, 1, 1, 1]);
                    let message = Message::known(Opcode::EntitySpawn, body.clone());
                    let Fields::CompactSpawn {
                        lane_minion: Some(minion),
                        remaining,
                        ..
                    } = message.fields().unwrap()
                    else {
                        panic!("expected Solo lane minion")
                    };
                    assert_eq!(
                        minion,
                        LaneMinionSpawn {
                            slot: 56,
                            stat_mask: mask,
                            route: 1,
                            wire_team: 1,
                            health: (mask == 0).then_some((321.5, 475.0))
                        }
                    );
                    assert_eq!(remaining, &body[12..]);
                    let cipher = MatchCipher::new("synthetic");
                    let bytes = cipher.encode(&message).unwrap();
                    let decoded = cipher.decode(&bytes[2..]).unwrap();
                    assert_eq!(&decoded.body[..size], &body);
                    body[121] = 2;
                    assert!(matches!(
                        Message::known(Opcode::EntitySpawn, body).fields().unwrap(),
                        Fields::CompactSpawn {
                            lane_minion: None,
                            ..
                        }
                    ));
                }
            }
        }
    }

    #[test]
    fn actor_visibility_roundtrips_distinct_index_state_and_masks() {
        let mut message = Message::actor_visibility(1500, 4, 1, [0x81, 0x42]);
        assert_eq!(
            message.body,
            [0, 0, 5, 220, 4, 1, 0x81, 0x42, 0, 0, 0, 0, 0, 0]
        );
        for length in [14, 8] {
            message.body[8..].fill(0x55);
            message.body.truncate(length);
            assert_eq!(
                message.fields().unwrap(),
                Fields::ActorVisibility {
                    actor: 1500,
                    index: 4,
                    state: 1,
                    masks: [0x81, 0x42],
                    remaining: &message.body[8..],
                }
            );
        }
    }

    #[test]
    fn read_only_prefixes_and_compact_spawn_variants() {
        for (code, kind) in [
            (
                crate::wire_opcode::ModifyVisibility,
                Opcode::ModifyVisibility,
            ),
            (
                crate::wire_opcode::ApplyBuffCompact,
                Opcode::ApplyBuffCompact,
            ),
            (
                crate::wire_opcode::ApplyBuffExtended,
                Opcode::ApplyBuffExtended,
            ),
        ] {
            assert_eq!(
                Message::new(code, [0x12, 0x34, 0x56, 0x78, 0xab])
                    .fields()
                    .unwrap(),
                Fields::EntityPrefix {
                    kind,
                    entity: 0x12345678,
                    remaining: &[0xab]
                }
            );
            assert!(Message::new(code, [0, 1, 2]).fields().is_err());
        }
        for size in [122, 126] {
            let mut body = vec![0xfe; size];
            body[..12].copy_from_slice(&[0, 1, 0, 5, 0, 2, 0, 6, 0, 3, 0, 7]);
            assert_eq!(
                Message::known(Opcode::EntitySpawn, body.clone())
                    .fields()
                    .unwrap(),
                Fields::CompactSpawn {
                    template: 65541,
                    skin: 131078,
                    entity: 196615,
                    lane_minion: None,
                    remaining: &body[12..]
                }
            );
        }
        assert!(matches!(
            Message::known(Opcode::EntitySpawn, [0; 8])
                .fields()
                .unwrap(),
            Fields::Opaque(_)
        ));
        let mut body = vec![0; 74];
        body[..5].copy_from_slice(b"Guest");
        body[64] = 10;
        assert_eq!(
            Message::known(Opcode::PlayerLoadedScene, body.clone())
                .fields()
                .unwrap(),
            Fields::HandleRecord {
                text: b"Guest",
                field64: 10 << 24,
                lookup: 0,
                remaining: &body[72..]
            }
        );
        assert_eq!(
            Message::known(Opcode::SceneSettings, body.clone())
                .fields()
                .unwrap(),
            Fields::NameField {
                kind: Opcode::SceneSettings,
                text: b"Guest",
                remaining: &body[64..]
            }
        );
        let mut table = vec![0; 102];
        table[90..96].copy_from_slice(&[1, 2, 3, 4, 5, 6]);
        let msg = Message::known(Opcode::SceneReadinessRows, table);
        let Fields::ControlTable { entries, remaining } = msg.fields().unwrap() else {
            panic!()
        };
        assert_eq!(
            entries[15],
            ControlEntry {
                reference: 0x01020304,
                gate: 5,
                flag: 6
            }
        );
        assert_eq!(remaining, [0; 6]);
        assert!(
            Message::known(Opcode::SceneReadinessRows, [0; 95])
                .fields()
                .is_err()
        );
        assert_eq!(
            Message::known(Opcode::ConfirmSceneOwner, [0, 0, 0, 7, 1, 3])
                .fields()
                .unwrap(),
            Fields::ReferencedFlag {
                reference: 7,
                flag: 1,
                remaining: &[3]
            }
        );
        let action = Message::known(Opcode::MoveTo, [5, 0x3f, 0x80, 0, 0, 0xc0, 0, 0, 0]);
        assert_eq!(
            action.fields().unwrap(),
            Fields::MoveAction {
                selector: 5,
                x: 1.,
                y: -2.,
                remaining: &[]
            }
        );
        let roster = Message::known(
            Opcode::DraftRosterAral,
            [0x41, 0xa0, 0, 0, 0x41, 0xf0, 0, 0, 0x7f],
        );
        assert_eq!(
            roster.fields().unwrap(),
            Fields::AralRosterHeader {
                remaining_seconds: 20.,
                allotted_seconds: 30.,
                remaining: &[0x7f]
            }
        );
        assert!(
            Message::known(Opcode::DraftRosterAral, [0; 7])
                .fields()
                .is_err()
        );
    }

    #[test]
    fn inventory_event_fields_preserve_source_and_flags() {
        let modifier = [0, 0, 0, 37, 0, 0, 0, 23, 0xc1, 0x48, 0, 0, 7, 2];
        let removal = [0, 0, 0, 37, 0, 0, 0x11, 0x22, 0, 1];
        let decoded = Message::known(Opcode::ModifyActorAttribute, modifier);
        assert_eq!(
            decoded.fields().unwrap(),
            Fields::StatModifier {
                actor: 37,
                source: 23,
                value: -12.5,
                stat: 7,
                flags: 2
            }
        );
        assert_eq!(
            Message::known(Opcode::ConsumeItem, removal)
                .fields()
                .unwrap(),
            Fields::ItemRemoval {
                actor: 37,
                instance: 0x1122,
                flags: 1
            }
        );
        assert!(
            Message::known(Opcode::ModifyActorAttribute, &modifier[..13])
                .fields()
                .is_err()
        );
        assert!(
            Message::known(Opcode::ConsumeItem, &removal[..9])
                .fields()
                .is_err()
        );
    }
}
