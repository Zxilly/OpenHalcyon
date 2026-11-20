//! CE server messages. Unidentified fields use zero defaults.
use super::Pick;
use halcyon_protocol::{
    Message,
    ce::{KnownQueue, NativeMode},
};
use halcyon_sim::{Entity, World};
use uuid::Uuid;

pub(super) const GUEST_HANDLE: &str = "Guest";

pub fn wire_id(id: u32) -> u16 {
    u16::try_from(1499 + id).unwrap_or(0xffff)
}
fn put_text(buf: &mut [u8], s: &str) {
    let n = s.len().min(buf.len());
    buf[..n].copy_from_slice(&s.as_bytes()[..n]);
}
fn mode_name(queue: &str) -> &'static str {
    // Operator queues retain their original IDs but use the casual scene asset.
    KnownQueue::parse(queue)
        .map_or(NativeMode::CasualFive, KnownQueue::mode)
        .asset()
}
pub(super) fn scene_mode(queue: &str, flag: u8) -> Message {
    let mut body = vec![0; 69];
    put_text(&mut body[..64], mode_name(queue));
    body[68] = flag;
    Message::known(crate::Opcode::ModeName, body)
}
fn settings(seat: usize, mode: &str, draft: bool) -> Message {
    let mut b = vec![0; 101];
    b[..4].copy_from_slice(&u32::from(wire_id(seat as u32 + 1)).to_be_bytes());
    put_text(&mut b[32..96], mode);
    b[100] = 1;
    if draft {
        b[7] = 1;
        b[10..12].fill(0xff);
        b[16..18].fill(5);
        b[96..100].copy_from_slice(&1f32.to_be_bytes());
    }
    Message::known(crate::Opcode::GameSetup, b)
}
pub(super) fn world_settings(seat: usize, queue: &str) -> Message {
    settings(seat, mode_name(queue), false)
}

/// The catalog is immutable across rooms on a host. Prepare the exact draft payloads once.
pub(super) fn catalog_entries(data: Option<&halcyon_data::GameData>) -> std::sync::Arc<[[u8; 32]]> {
    fn entry(name: &str) -> [u8; 32] {
        let mut body = [0; 32];
        put_text(&mut body, name);
        body
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
    if KnownQueue::parse(mode) == Some(KnownQueue::Ranked) {
        config.body[16..18].fill(3);
        config.body[96..100].copy_from_slice(&5f32.to_be_bytes());
        config.body[100] = world.entities[seat].team + 1;
    }
    let mut out = vec![config];
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
    let mut mode = vec![0; 68];
    put_text(&mut mode[4..], mode_name(queue));
    let mut out = Vec::with_capacity(catalog.len() + 2);
    out.push(Message::known(crate::Opcode::GameMode, mode));
    out.extend(
        catalog
            .iter()
            .map(|entry| Message::known(crate::Opcode::HeroCatalog, *entry)),
    );
    out.push(snapshot(world, players, picks));
    out
}
pub fn snapshot(world: &World, players: &[Uuid], picks: &[Pick]) -> Message {
    let mut body = vec![0; 2590];
    body[..4].copy_from_slice(&(300.0 - world.tick as f32 / 20.).max(0.).to_be_bytes());
    body[4..8].copy_from_slice(&300f32.to_be_bytes());
    for idx in 0..16 {
        let r = &mut body[idx * 161..][..161];
        r[9] = idx as u8;
        r[12..15].fill(0xff);
        r[17..21].fill(0xff);
        if let Some((e, pick)) = world
            .entities
            .get(idx)
            .filter(|e| e.kind == halcyon_sim::Kind::Hero)
            .zip(picks.get(idx))
        {
            r[8] = 1;
            if pick.selected {
                r[10] = 1;
                r[11] = u8::from(pick.locked);
                r[17..19].copy_from_slice(&e.type_id.to_be_bytes());
                r[21..25].copy_from_slice(&pick.skin.to_be_bytes());
            }
            r[16] = e.team + 1;
            r[19..21].copy_from_slice(&wire_id(e.id).to_be_bytes());
            if let Some(uuid) = players.get(idx) {
                put_text(&mut r[25..57], GUEST_HANDLE);
                put_text(&mut r[105..141], &uuid.to_string());
            } else {
                put_text(&mut r[25..57], &format!("{} Bot", pick.name));
                put_text(&mut r[105..141], "__Kindred_Player_Bot__");
            }
        }
    }
    Message::known(crate::Opcode::Snapshot5V5, body)
}
pub fn spawn(e: &Entity, picks: &[Pick], data: Option<&halcyon_data::GameData>) -> Message {
    let mut b = vec![0; 750];
    b[2..4].copy_from_slice(&e.type_id.to_be_bytes());
    let hero = if e.kind == halcyon_sim::Kind::Hero {
        picks.get(e.id as usize - 1)
    } else {
        None
    };
    if let Some(p) = hero {
        b[4..8].copy_from_slice(&p.skin.to_be_bytes());
    }
    b[10..12].copy_from_slice(&wire_id(e.id).to_be_bytes());
    b[12..16].copy_from_slice(&(u32::from(e.team) + 1).to_be_bytes());
    b[16..18].fill(0xff);
    b[18..22].copy_from_slice(&e.position.x.to_be_bytes());
    b[26..30].copy_from_slice(&e.position.y.to_be_bytes());
    b[30..34].copy_from_slice(&1f32.to_be_bytes());
    b[42..46].copy_from_slice(&e.hp.to_be_bytes());
    b[46..50].copy_from_slice(&e.max_hp.to_be_bytes());
    let unit = data.and_then(|d| hero.and_then(|p| d.unit(&p.name)));
    if let Some(unit) = unit {
        for (offset, value) in [(74, unit.move_speed), (98, unit.attack_speed), (226, 0.5)] {
            b[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
    }
    if hero.is_some() {
        for (offset, value) in [
            (122, e.energy),
            (126, e.max_energy),
            (190, e.armor()),
            (202, e.shield()),
            (214, e.weapon_power()),
        ] {
            b[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
    }
    Message::known(crate::Opcode::EntitySpawn, b)
}

/// Android Solo full declarations keep these selectors for the hero's lifetime.
pub(super) fn solo_hero_profile(message: &mut Message, entity: &Entity, tick: u64) {
    let body = &mut message.body;
    let selector = if entity.bot {
        u32::from(entity.team)
    } else {
        u32::MAX
    };
    body[0x2c5..0x2c9].copy_from_slice(&selector.to_be_bytes());
    body[0x2e5..0x2e9].copy_from_slice(&u32::MAX.to_be_bytes());
    if tick == 0 {
        // The ten Solo opening pads share the same measured ground height.
        body[22..26].copy_from_slice(&0x3d75_be29_u32.to_be_bytes());
        body[34..38].copy_from_slice(&(-0.0_f32).to_be_bytes());
        // Initial per-team flags differ from later visibility checkpoints.
        body[0x2c9..0x2d1].fill(1);
        body[0x2d1 + usize::from(entity.team) + 1] = 15;
    }
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
    let mut b = vec![0; if full { 222 } else { 216 }];
    let bot = i >= players.len();
    let name = if bot && full {
        format!("{} Bot", p.name)
    } else if bot {
        p.name.clone()
    } else {
        GUEST_HANDLE.into()
    };
    put_text(&mut b[..64], &name);
    if full {
        let uuid = players
            .get(i)
            .map(ToString::to_string)
            .unwrap_or_else(|| "__Kindred_Player_Bot__".to_owned());
        put_text(&mut b[64..128], &uuid);
        for chunk in b[176..204].as_chunks_mut::<4>().0 {
            chunk.copy_from_slice(&0x811c9dc5u32.to_be_bytes());
        }
        b[204..208].fill(0xff);
        if !bot {
            b[206] = 0;
        }
    }
    b[160..164].copy_from_slice(&u32::from(wire_id(e.id)).to_be_bytes());
    b[164..168].copy_from_slice(&u32::from(e.type_id).to_be_bytes());
    b[168..172].copy_from_slice(&p.skin.to_be_bytes());
    b[172..176].copy_from_slice(&0x9241f10eu32.to_be_bytes());
    b[209] = 2;
    b[210] = e.team + 1;
    b[212] = u8::from(bot);
    b[215] = u8::from(!bot);
    Message::known(crate::Opcode::PlayerInfo, b)
}

/// Remove the private human handle and admission identity from a player-info body.
pub(super) fn redact_player_info(body: &mut [u8]) {
    if body[212] == 0 {
        body[..128].fill(0);
    }
}

/// Replay chunks contain both roster forms before the first world spawn.
pub(super) fn world_header(
    world: &World,
    players: &[Uuid],
    picks: &[Pick],
    mode: &str,
    seat: usize,
) -> Vec<Message> {
    let mut out: Vec<_> = (0..10)
        .map(|i| info(world, players, picks, i, false))
        .collect();
    out.push(world_settings(seat, mode));
    out.push(Message::known(crate::Opcode::BeginLoading, [0]));
    out.push(scene_mode(mode, 1));
    out.push(Message::known(crate::Opcode::SceneTransition, [0]));
    out.extend((0..10).map(|i| info(world, players, picks, i, true)));
    out.push(Message::known(crate::Opcode::OpeningWorldClock, [0; 4]));
    out
}

pub(super) fn item_grant(actor: u32, item: u32, instance: u32) -> Message {
    Message::known(
        crate::Opcode::ItemGrant,
        [
            actor.to_be_bytes(),
            item.to_be_bytes(),
            instance.to_be_bytes(),
        ]
        .concat(),
    )
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
        let mut body = [actor.to_be_bytes(), used.instance.to_be_bytes()].concat();
        body.extend(1u16.to_be_bytes());
        out.push(Message::known(crate::Opcode::RemoveItem, body));
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
        let mut body = [0; 14];
        body[..4].copy_from_slice(&u32::from(wire_id(id)).to_be_bytes());
        body[4..8].copy_from_slice(&1_u32.to_be_bytes());
        return Some(Message::known(crate::Opcode::LevelUp, body));
    }
    if let Event::Respawn { id, position } = *event {
        if !visible(id) {
            return None;
        }
        let mut body = [0; 22];
        body[..4].copy_from_slice(&u32::from(wire_id(id)).to_be_bytes());
        body[4..8].copy_from_slice(&position.x.to_be_bytes());
        // OpenHalcyon's authoritative world is planar, with height zero.
        body[12..16].copy_from_slice(&position.y.to_be_bytes());
        return Some(Message::known(crate::Opcode::ActorRespawn, body));
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
                    effect: halcyon_sim::ImpactEffect::ChillWinds,
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
                crate::wire_opcode::EntityStat,
                crate::wire_opcode::StatModifier,
                crate::wire_opcode::RemoveItem,
                crate::wire_opcode::StatModifier,
                crate::wire_opcode::StatModifier,
                crate::wire_opcode::RemoveItem,
                crate::wire_opcode::ItemGrant,
                crate::wire_opcode::StatModifier,
                crate::wire_opcode::StatModifier,
                crate::wire_opcode::StatModifier
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
                crate::wire_opcode::StatModifier,
                crate::wire_opcode::StatModifier
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
            ("aral_casual", "*GameMode_Aral_Casual*"),
            ("casual_aral", "*GameMode_Aral_Casual*"),
            ("custom_pool", "*GameMode_5v5_Casual*"),
            ("3v3_standard", "*GameMode_5v5_Casual*"),
        ] {
            assert_eq!(mode_name(queue), name, "{queue}");
        }
    }
}
