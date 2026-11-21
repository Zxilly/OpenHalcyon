//! CE server messages. Unidentified fields use zero defaults.
use super::Pick;
use halcyon_protocol::{
    ActorRespawnBody, ConsumeItemBody, DraftCatalogEntryBody, DraftModeAnnouncementBody,
    DraftRosterBody, DraftSlotBody, EntitySpawnBody, FixedAscii, HeroSpawnBody, ItemGrantBody,
    LevelUpBody, MatchSettingsBody, Message, PlayerInfoBody, PlayerInfoPrefix, SceneSettingsBody,
    WorldClockBody,
    ce::{KnownQueue, NativeMode},
};
use halcyon_sim::{Entity, World};
use uuid::Uuid;

pub(super) const GUEST_HANDLE: &str = "Guest";

pub fn wire_id(id: u32) -> u16 {
    u16::try_from(1499 + id).unwrap_or(0xffff)
}
fn mode_name(queue: &str) -> &'static str {
    // Operator queues retain their original IDs but use the registered scene asset.
    KnownQueue::parse(queue).map_or(NativeMode::CasualFive.asset(), KnownQueue::asset)
}
pub(super) fn scene_mode(queue: &str, flag: u8) -> Message {
    SceneSettingsBody::new(mode_name(queue), flag).encode()
}
fn settings(seat: usize, mode: &str, draft: bool) -> MatchSettingsBody {
    let local_actor = u32::from(wire_id(seat as u32 + 1));
    if draft {
        MatchSettingsBody::draft_five(local_actor, mode)
    } else {
        MatchSettingsBody::world(local_actor, mode)
    }
}
pub(super) fn world_settings(seat: usize, queue: &str) -> Message {
    settings(seat, mode_name(queue), false).encode()
}

/// The catalog is immutable across rooms on a host. Prepare the exact draft payloads once.
pub(super) fn catalog_entries(data: Option<&halcyon_data::GameData>) -> std::sync::Arc<[[u8; 32]]> {
    fn entry(name: &str) -> [u8; 32] {
        DraftCatalogEntryBody::new(name).name.into()
    }
    match data.filter(|d| !d.hero_names.is_empty()) {
        Some(data) => data
            .hero_names
            .iter()
            .map(|h| entry(&format!("*{h}*")))
            .chain(data.skin_names.iter().map(|s| entry(s)))
            .collect(),
        None => [entry("*Ringo*")].into(),
    }
}

pub fn setup(
    world: &World,
    players: &[Uuid],
    catalog: &[[u8; 32]],
    seat: usize,
    mode: &str,
    picks: &[Pick],
) -> Vec<Message> {
    let mut config = settings(seat, mode_name(mode), true);
    if matches!(
        KnownQueue::parse(mode).map(KnownQueue::mode),
        Some(
            NativeMode::RankedFold | NativeMode::CasualFold | NativeMode::Blitz | NativeMode::Aral
        )
    ) {
        config = MatchSettingsBody::draft_three(
            config.local_actor,
            mode_name(mode),
            world.entities[seat].team + 1,
        );
    }
    let mut out = vec![config.encode()];
    out.extend(draft_catalog(world, players, catalog, mode, picks));
    out
}

pub(super) fn draft_catalog(
    world: &World,
    players: &[Uuid],
    catalog: &[[u8; 32]],
    queue: &str,
    picks: &[Pick],
) -> Vec<Message> {
    let parsed_mode = KnownQueue::parse(queue).map(KnownQueue::mode);
    let team_size = if matches!(
        parsed_mode,
        Some(
            NativeMode::CasualFold | NativeMode::RankedFold | NativeMode::Blitz | NativeMode::Aral
        )
    ) {
        3
    } else {
        0
    };
    let mut out = Vec::with_capacity(catalog.len() + 2);
    out.push(DraftModeAnnouncementBody::new(team_size, mode_name(queue)).encode());
    out.extend(catalog.iter().map(|entry| {
        DraftCatalogEntryBody {
            name: FixedAscii(*entry),
        }
        .encode()
    }));
    out.push(snapshot(world, players, picks));
    out
}
pub(super) fn snapshot_with_timers(
    world: &World,
    players: &[Uuid],
    picks: &[Pick],
    remaining_seconds: f32,
    allotted_seconds: f32,
) -> Message {
    let slots = std::array::from_fn(|idx| {
        let mut slot = DraftSlotBody::empty(idx as u8);
        if let Some((e, pick)) = world
            .entities
            .get(idx)
            .filter(|e| e.kind == halcyon_sim::Kind::Hero)
            .zip(picks.get(idx))
        {
            slot.occupied = 1;
            slot.set_preferences(pick.preferences.to_wire());
            if pick.selected {
                slot.selected = 1;
                slot.locked = u8::from(pick.locked);
                slot.template = e.type_id;
                slot.skin = pick.skin;
            }
            slot.team = e.team + 1;
            slot.entity = wire_id(e.id);
            if let Some(uuid) = players.get(idx) {
                slot.display_name = FixedAscii::from_str_trunc(GUEST_HANDLE);
                slot.identity = FixedAscii::from_str_trunc(&uuid.to_string());
            } else {
                slot.display_name = FixedAscii::from_str_trunc(&format!("{} Bot", pick.name));
                slot.identity = FixedAscii::from_str_trunc("__Kindred_Player_Bot__");
            }
        }
        slot
    });
    DraftRosterBody::full(remaining_seconds, allotted_seconds, slots).encode_full()
}
pub fn snapshot(world: &World, players: &[Uuid], picks: &[Pick]) -> Message {
    snapshot_with_timers(
        world,
        players,
        picks,
        (300.0 - world.tick as f32 / 20.).max(0.),
        300.,
    )
}
pub fn spawn(e: &Entity, picks: &[Pick], data: Option<&halcyon_data::GameData>) -> Message {
    let hero = if e.kind == halcyon_sim::Kind::Hero {
        picks.get(e.id as usize - 1)
    } else {
        None
    };
    let mut body = HeroSpawnBody::new(
        u32::from(e.type_id),
        hero.map_or(0, |p| p.skin),
        u32::from(wire_id(e.id)),
        u32::from(e.team) + 1,
        0,
    );
    body.position = [e.position.x, 0.0, e.position.y];
    body.hp = e.hp;
    body.max_hp = e.max_hp;
    let unit = data.and_then(|d| hero.and_then(|p| d.unit(&p.name)));
    if let Some(unit) = unit {
        body.move_speed = unit.move_speed;
        body.attack_speed = unit.attack_speed;
        body.word226 = 0.5;
    }
    if hero.is_some() {
        body.energy = e.energy;
        body.max_energy = e.max_energy;
        body.armor = e.armor();
        body.shield = e.shield();
        body.weapon_power = e.weapon_power();
    }
    body.encode()
}

/// Android Solo full declarations keep these selectors for the hero's lifetime.
pub(super) fn solo_hero_profile(message: &mut Message, entity: &Entity, tick: u64) {
    let mut body =
        HeroSpawnBody::read_body(&message.body).expect("solo_hero_profile expects 750B HeroSpawn");
    body.apply_solo_profile(entity.bot, entity.team, tick);
    body.write_body(&mut message.body)
        .expect("HeroSpawnBody fits 750B buffer");
}

pub(super) fn info(
    world: &World,
    players: &[Uuid],
    picks: &[Pick],
    i: usize,
    full: bool,
) -> Message {
    let e = &world.entities[i];
    let p = &picks[i];
    let bot = i >= players.len();
    let name = if bot && full {
        format!("{} Bot", p.name)
    } else if bot {
        p.name.clone()
    } else {
        GUEST_HANDLE.into()
    };
    let actor_id = u32::from(wire_id(e.id));
    let hero_template = u32::from(e.type_id);
    let team = e.team + 1;
    if full {
        let uuid = players
            .get(i)
            .map(ToString::to_string)
            .unwrap_or_else(|| "__Kindred_Player_Bot__".to_owned());
        PlayerInfoBody::full(PlayerInfoPrefix::full(
            &name,
            &uuid,
            actor_id,
            hero_template,
            p.skin,
            team,
            bot,
        ))
    } else {
        PlayerInfoBody::compact(PlayerInfoPrefix::compact(
            &name,
            actor_id,
            hero_template,
            p.skin,
            team,
            bot,
        ))
    }
    .encode()
}

/// Remove the private human handle and admission identity from a player-info body.
pub(super) fn redact_player_info(body: &mut [u8]) {
    if let Ok(mut info) = PlayerInfoBody::read_body(body) {
        info.redact();
        let _ = info.write_body(body);
    }
}

pub(super) fn camp_marker_spawn(
    actor_id: u32,
    template_raw: u16,
    slot: u8,
    pos: halcyon_sim::Position,
) -> Message {
    let mut body = EntitySpawnBody::new(
        u32::from(template_raw),
        EntitySpawnBody::JUNGLE_MINION_SKIN,
        actor_id,
        slot,
        0,
    );
    body.position = [
        pos.x,
        f32::from_bits(EntitySpawnBody::GROUND_HEIGHT_BITS),
        pos.y,
    ];
    body.forward = [0.0, 0.0, 1.0];
    body.visibility_required = EntitySpawnBody::VISIBILITY_ALL_REQUIRED;
    body.visibility_mask[0] = EntitySpawnBody::VISIBILITY_ACTIVE_MASK;
    body.encode()
}

pub(super) fn append_5v5_camp_markers(out: &mut Vec<Message>, world: &World, viewer_index: u8) {
    if !world.is_5v5_enabled() {
        return;
    }
    for (idx, (actor_id, template_raw, pos)) in
        world.camp_5v5_declarations().into_iter().enumerate()
    {
        let slot = if actor_id == halcyon_sim::map_5v5::BLACKCLAW_PIT_MARKER {
            38
        } else if actor_id == halcyon_sim::map_5v5::GHOSTWING_PIT_MARKER {
            39
        } else {
            40 + u8::try_from(idx).unwrap_or(0)
        };
        out.push(camp_marker_spawn(actor_id, template_raw, slot, pos));
    }
    for (actor_id, _, _, vis) in world.camp_5v5_markers() {
        if let Some(state) = vis.get(usize::from(viewer_index)).copied().flatten() {
            out.push(Message::camp_visibility(
                actor_id,
                viewer_index,
                halcyon_protocol::CampVisibilityState::from_wire(state as u8),
            ));
        }
    }
}

pub(super) fn world_clock(tick: u64) -> Message {
    WorldClockBody {
        seconds: tick as f32 / halcyon_sim::TICK_HZ as f32,
    }
    .encode()
}

/// Replay chunks contain both roster forms before the first world spawn.
pub(super) fn world_header(
    world: &World,
    players: &[Uuid],
    picks: &[Pick],
    mode: &str,
    seat: usize,
) -> Vec<Message> {
    let seats = picks.len().min(10);
    let mut out: Vec<_> = (0..seats)
        .map(|i| info(world, players, picks, i, false))
        .collect();
    out.push(world_settings(seat, mode));
    out.push(Message::known(crate::Opcode::DraftTransitionToLoading, [0]));
    out.push(scene_mode(mode, 1));
    out.push(Message::known(crate::Opcode::SceneLoadedDismiss, [0]));
    out.extend((0..seats).map(|i| info(world, players, picks, i, true)));
    out.push(world_clock(world.tick));
    out
}

pub(super) fn item_grant(actor: u32, item: u32, instance: u32) -> Message {
    ItemGrantBody {
        actor,
        item,
        instance,
    }
    .encode()
}
pub(super) fn append_inventory(out: &mut Vec<Message>, world: &World, entity: &Entity) {
    let actor = u32::from(wire_id(entity.id));
    if let Some(slots) = &entity.aral_inventory {
        for held in slots.slots().iter().flatten() {
            out.push(item_grant(actor, held.template, held.instance));
        }
    }
    for held in &entity.inventory {
        if entity.aral_inventory.is_none() {
            out.push(item_grant(actor, held.kind.template(), held.instance));
        }
        // HP, max energy, armor, shield and weapon power already include
        // equipment in the full declaration. Restore the remaining modifiers.
        for (stat, bonus) in world.item_modifiers(held.kind) {
            if stat.restore_after_spawn() {
                out.push(modifier(actor, stat, bonus));
            }
        }
    }
}
pub(super) fn append_purchase(
    out: &mut Vec<Message>,
    world: &World,
    receipt: &halcyon_sim::Purchase,
    include_item_changes: bool,
) {
    let actor = u32::from(wire_id(receipt.actor));
    out.push(Message::entity_stat(
        actor as u16,
        -(receipt.paid as f32),
        6,
    ));
    // A spawn in this batch already describes the post-tick inventory.
    if !include_item_changes {
        return;
    }
    for used in &receipt.consumed {
        for (stat, bonus) in world.item_modifiers(used.kind) {
            out.push(modifier(actor, stat, -bonus));
        }
        out.push(
            ConsumeItemBody {
                actor,
                instance: used.instance,
                flags: 1,
            }
            .encode(),
        );
    }
    let granted = receipt.granted;
    out.push(item_grant(actor, granted.kind.template(), granted.instance));
    for (stat, bonus) in world.item_modifiers(granted.kind) {
        out.push(modifier(actor, stat, bonus));
    }
}

pub(super) fn modifier(actor: u32, stat: halcyon_sim::NativeItemStat, value: f32) -> Message {
    halcyon_protocol::AttributeChange {
        actor,
        source: u32::MAX,
        value,
        index: stat as u8,
        layer: stat.layer(),
        set_mode: 0,
    }
    .message()
}

/// Both references must be known to the recipient. For a lethal tick the victim
/// may already have left the post-step visibility set.
fn combat_event(event: &halcyon_sim::Event, visible: impl Fn(u32) -> bool) -> Option<Message> {
    use halcyon_sim::Event;
    if let Event::Level { id, .. } = *event {
        if !visible(id) {
            return None;
        }
        return Some(
            LevelUpBody {
                actor: u32::from(wire_id(id)),
                operand: 1,
            }
            .encode_padded(),
        );
    }
    if let Event::Respawn { id, position } = *event {
        if !visible(id) {
            return None;
        }
        return Some(
            ActorRespawnBody {
                actor: u32::from(wire_id(id)),
                // OpenHalcyon's authoritative world is planar, with height zero.
                position: [position.x, 0.0, position.y],
            }
            .encode_padded(),
        );
    }
    let (target, source, operand) = match *event {
        Event::Damage {
            source,
            target,
            amount,
            ..
        } => (target, source, Some(-amount)),
        Event::Heal {
            source,
            target,
            amount,
            ..
        } => (target, source, Some(amount)),
        Event::Death { id, killer } => (id, killer, None),
        _ => return None,
    };
    if !visible(target) || !visible(source) {
        return None;
    }
    let target = u32::from(wire_id(target));
    let source = u32::from(wire_id(source));
    Some(match operand {
        // This presentation tuple occurs with both signs in the local CE
        // recording. The simulator supplies the applied health change.
        Some(value)
            if matches!(
                event,
                Event::Damage {
                    effect: halcyon_sim::ImpactEffect::ChillWinds
                        | halcyon_sim::ImpactEffect::AmaelKick,
                    ..
                }
            ) =>
        {
            Message::health_impact(target, source, value, 74, 1, 0)
        }
        Some(value) => Message::health_impact(target, source, value, 5, 0, 0),
        None => Message::actor_death(target, source),
    })
}

pub(super) fn append_combat(
    out: &mut Vec<Message>,
    event: &halcyon_sim::Event,
    visible: impl Fn(u32) -> bool,
) {
    let Some(message) = combat_event(event, visible) else {
        return;
    };
    out.push(message);
    if let halcyon_sim::Event::Death { id, killer } = *event
        && (1..=10).contains(&id)
    {
        out.push(hero_counter(id, 42));
        if (1..=10).contains(&killer) {
            out.push(hero_counter(killer, 41));
        }
    }
}
pub(super) fn append_respawn_timer(out: &mut Vec<Message>, world: &World, actor: u32) {
    if let Some(seconds) = world.respawn_seconds(actor) {
        out.push(Message::actor_respawn_timer(
            u32::from(wire_id(actor)),
            seconds,
        ));
    }
}

fn hero_counter(id: u32, stat: u8) -> Message {
    halcyon_protocol::AttributeChange {
        actor: u32::from(wire_id(id)),
        source: u32::MAX,
        value: 1.,
        index: stat,
        layer: 0,
        set_mode: 0,
    }
    .message()
}

#[cfg(test)]
mod combat_tests {
    use super::*;
    use halcyon_protocol::Fields;
    use halcyon_sim::Event;
    #[test]
    fn reconnect_restores_split_pierce_and_movement_amplifier_in_the_native_layers() {
        use halcyon_protocol::AttributeChange;
        use halcyon_sim::BasicItem;

        let mut world = World::new(7, 10);
        world.entities[0].gold = 10_000;
        let mut expected = Vec::new();
        for item in [
            BasicItem::Bonesaw,
            BasicItem::BrokenMyth,
            BasicItem::Pulseweave,
        ] {
            let receipt = world.purchase(1, item).unwrap();
            let mut frames = Vec::new();
            append_purchase(&mut frames, &world, &receipt, true);
            expected.extend(
                frames
                    .iter()
                    .filter_map(AttributeChange::read)
                    .filter(|m| matches!(m.index, 9 | 11 | 18)),
            );
        }
        let mut snapshot = Vec::new();
        append_inventory(&mut snapshot, &world, &world.entities[0]);
        let restored: Vec<_> = snapshot
            .iter()
            .filter_map(AttributeChange::read)
            .filter(|m| matches!(m.index, 9 | 11 | 18))
            .collect();
        assert_eq!(restored, expected);
        let layers: Vec<_> = restored
            .iter()
            .map(|m| (m.index, m.layer, m.value))
            .collect();
        assert_eq!(
            layers,
            [
                (9, 1, 0.1),
                (9, 1, 0.1),
                (11, 1, 0.1),
                (11, 1, 0.35),
                (18, 2, 0.08)
            ]
        );
    }

    #[test]
    fn native_crit_upgrade_removes_components_and_restores_final_modifiers() {
        use halcyon_protocol::AttributeChange;
        use halcyon_sim::BasicItem::*;
        let mut world = World::new(12, 10);
        world.entities[0].gold = 2900;
        for item in [SixSins, MinionsFoot, LuckyStrike] {
            world.purchase(1, item).unwrap();
        }
        let receipt = world.purchase(1, TyrantsMonocle).unwrap();
        let mut frames = Vec::new();
        append_purchase(&mut frames, &world, &receipt, true);
        assert_eq!(
            frames.iter().map(|m| m.opcode).collect::<Vec<_>>(),
            [
                crate::wire_opcode::ModifyActorResource,
                crate::wire_opcode::ModifyActorAttribute,
                crate::wire_opcode::ConsumeItem,
                crate::wire_opcode::ModifyActorAttribute,
                crate::wire_opcode::ModifyActorAttribute,
                crate::wire_opcode::ConsumeItem,
                crate::wire_opcode::ItemGrant,
                crate::wire_opcode::ModifyActorAttribute,
                crate::wire_opcode::ModifyActorAttribute,
                crate::wire_opcode::ModifyActorAttribute
            ]
        );
        let modifiers = |msgs: &[Message]| {
            msgs.iter()
                .filter_map(AttributeChange::read)
                .map(|v| (v.index, v.value, v.layer, v.set_mode))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            modifiers(&frames),
            [
                (4, -25., 1, 0),
                (30, -0.2, 1, 0),
                (31, -0.05, 1, 0),
                (4, 60., 1, 0),
                (30, 0.35, 1, 0),
                (31, 0.15, 1, 0)
            ]
        );
        frames.clear();
        append_inventory(&mut frames, &world, &world.entities[0]);
        assert_eq!(modifiers(&frames), [(30, 0.35, 1, 0), (31, 0.15, 1, 0)]);
    }

    #[test]
    fn each_visible_level_gain_uses_the_single_step_wire_operand() {
        let mut records = vec![];
        for level in [2, 3] {
            let event = Event::Level { id: 4, level };
            append_combat(&mut records, &event, |_| false);
            append_combat(&mut records, &event, |id| id == 4);
        }
        assert_eq!(records.len(), 2);
        for message in records {
            assert_eq!(message.opcode, crate::wire_opcode::LevelUp);
            assert!(matches!(
                message.fields().unwrap(),
                Fields::ProgressAction {
                    actor: 1503,
                    operand: 1,
                    remaining: [0, 0, 0, 0, 0, 0],
                    ..
                }
            ));
        }
    }

    #[test]
    fn respawn_uses_the_authoritative_actor_and_spawn_point_inside_vision() {
        let p = halcyon_sim::Position { x: 31.25, y: -4.5 };
        let mut records = vec![];
        append_combat(&mut records, &Event::Spawn { id: 4 }, |_| true);
        append_combat(&mut records, &Event::Respawn { id: 4, position: p }, |_| {
            false
        });
        assert!(records.is_empty());
        append_combat(&mut records, &Event::Respawn { id: 4, position: p }, |id| {
            id == 4
        });
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].fields().unwrap(),
            Fields::Respawn {
                actor: 1503,
                position: [31.25, 0., -4.5],
                remaining: &[0; 6]
            }
        );
    }

    #[test]
    fn visible_hero_death_adds_the_authoritative_counters_once() {
        let mut out = vec![];
        let hero = Event::Death { id: 3, killer: 7 };
        append_combat(&mut out, &hero, |id| id == 3);
        assert!(out.is_empty());
        append_combat(&mut out, &hero, |_| true);
        assert_eq!(
            out.iter().map(|m| m.opcode).collect::<Vec<_>>(),
            [
                crate::wire_opcode::ActorDeath,
                crate::wire_opcode::ModifyActorAttribute,
                crate::wire_opcode::ModifyActorAttribute
            ]
        );
        assert_eq!(
            (&out[1].body[12..15], &out[2].body[12..15]),
            (&[42, 0, 0][..], &[41, 0, 0][..])
        );
        out.clear();
        append_combat(&mut out, &Event::Death { id: 21, killer: 3 }, |_| true);
        assert_eq!(out.len(), 1);
    }
    #[test]
    fn signed_health_and_death_require_both_visible_references() {
        let damage = Event::Damage {
            effect: halcyon_sim::ImpactEffect::Standard,
            source: 2,
            target: 4,
            amount: 7.25,
            hp: 0.,
        };
        let heal = Event::Heal {
            source: 2,
            target: 4,
            amount: 3.5,
            hp: 8.,
        };
        let death = Event::Death { id: 4, killer: 2 };
        for (e, expected) in [(&damage, -7.25), (&heal, 3.5)] {
            assert!(combat_event(e, |id| id == 4).is_none());
            assert!(combat_event(e, |id| id == 2).is_none());
            let msg = combat_event(e, |_| true).unwrap();
            assert_eq!(
                msg.fields().unwrap(),
                Fields::HealthImpact {
                    target: 1503,
                    source: 1501,
                    operand: expected,
                    word: 5,
                    byte: 0,
                    flag: 0,
                    remaining: &[0; 6]
                }
            );
        }
        assert!(combat_event(&death, |id| id == 4).is_none());
        assert_eq!(
            combat_event(&death, |_| true).unwrap().fields().unwrap(),
            Fields::ActorDeath {
                victim: 1503,
                source: 1501,
                remaining: &[0; 6]
            }
        );
    }
}

#[cfg(test)]
mod mode_tests {
    use super::mode_name;

    #[test]
    fn maps_queue_names_to_manifest_labels() {
        for (queue, name) in [
            ("5v5_pvp_casual", "*GameMode_5v5_Casual*"),
            ("5v5_casual", "*GameMode_5v5_Casual*"),
            ("casual_5v5", "*GameMode_5v5_Casual*"),
            ("5v5_pvp_ranked", "*GameMode_5v5_Ranked*"),
            ("5v5_ranked", "*GameMode_5v5_Ranked*"),
            ("ranked_5v5", "*GameMode_5v5_Ranked*"),
            ("5v5_bots_solo", "*GameMode_5v5_SoloBots*"),
            ("solo", "*GameMode_5v5_SoloBots*"),
            ("5v5_bots_coop", "*GameMode_5v5_CoopBots*"),
            ("5v5_private", "*GameMode_5v5_Private*"),
            ("5v5_private_draft", "*GameMode_5v5_PrivateDraft*"),
            ("5v5_private_bots", "*GameMode_5v5_PrivateBots*"),
            ("5v5_practice", "*GameMode_5v5_Practice*"),
            ("3v3_pvp_casual", "*GameMode_HF_Casual*"),
            ("3v3_pvp_ranked", "*GameMode_HF_Ranked*"),
            ("ranked", "*GameMode_HF_Ranked*"),
            ("blitz_3v3", "*GameMode_Blitz_Revamp*"),
            ("blitz", "*GameMode_Blitz_Revamp*"),
            ("blitz_pvp_ranked", "*GameMode_Blitz_Revamp*"),
            ("aral_casual", "*GameMode_Aral_Casual*"),
            ("casual_aral", "*GameMode_Aral_Casual*"),
            ("custom_pool", "*GameMode_5v5_Casual*"),
            ("3v3_standard", "*GameMode_5v5_Casual*"),
        ] {
            assert_eq!(mode_name(queue), name, "{queue}");
        }
    }

    #[test]
    fn camp_marker_spawn_and_solo_hero_profile_match_golden_offsets() {
        let marker =
            super::camp_marker_spawn(2055, 345, 40, halcyon_sim::Position { x: 12.5, y: -7.25 });
        assert_eq!(marker.opcode, crate::wire_opcode::EntitySpawn);
        assert_eq!(marker.body.len(), 122);
        assert_eq!(&marker.body[..4], &345u32.to_be_bytes());
        assert_eq!(&marker.body[4..8], &0x4dd5_b7d0_u32.to_be_bytes());
        assert_eq!(&marker.body[8..12], &2055u32.to_be_bytes());
        assert_eq!(&marker.body[12..16], &12.5f32.to_be_bytes());
        assert_eq!(&marker.body[16..20], &0x3d75_be29_u32.to_be_bytes());
        assert_eq!(&marker.body[20..24], &(-7.25f32).to_be_bytes());
        assert_eq!(&marker.body[24..32], &[0; 8]);
        assert_eq!(&marker.body[32..36], &1.0_f32.to_be_bytes());
        assert_eq!(&marker.body[88..96], &[1; 8]);
        assert_eq!(marker.body[96], 15);
        assert_eq!(&marker.body[112..116], &u32::MAX.to_be_bytes());
        assert_eq!(&marker.body[116..122], &[40, 0, 0, 1, 255, 0]);

        let world = halcyon_sim::World::new(1, 10);
        let mut msg = super::spawn(&world.entities[0], &[], None);
        super::solo_hero_profile(&mut msg, &world.entities[0], 0);
        assert_eq!(msg.body.len(), 750);
        assert_eq!(&msg.body[22..26], &0x3d75_be29_u32.to_be_bytes());
        assert_eq!(&msg.body[34..38], &(-0.0_f32).to_be_bytes());
        assert_eq!(&msg.body[0x2c5..0x2c9], &u32::MAX.to_be_bytes());
        assert_eq!(&msg.body[0x2c9..0x2d1], &[1; 8]);
        assert_eq!(
            msg.body[0x2d1 + usize::from(world.entities[0].team) + 1],
            15
        );
        assert_eq!(&msg.body[0x2e5..0x2e9], &u32::MAX.to_be_bytes());
    }
}
