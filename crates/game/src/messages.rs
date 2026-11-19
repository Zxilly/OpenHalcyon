//! CE server messages. Unidentified fields use zero defaults.
use super::Pick;
use halcyon_protocol::Message;
use halcyon_sim::{Entity, World};
use uuid::Uuid;

pub fn wire_id(id: u32) -> u16 {
    u16::try_from(1499 + id).unwrap_or(0xffff)
}
fn put_text(buf: &mut [u8], s: &str) {
    let n = s.len().min(buf.len());
    buf[..n].copy_from_slice(&s.as_bytes()[..n]);
}
fn mode_name(queue: &str) -> &'static str {
    match queue {
        "5v5_pvp_ranked" | "5v5_ranked" | "ranked_5v5" => "*GameMode_5v5_Ranked*",
        "5v5_bots_solo" | "solo" => "*GameMode_5v5_SoloBots*",
        "5v5_bots_coop" | "5v5_coop" => "*GameMode_5v5_CoopBots*",
        "5v5_private" | "5v5_pvp_private" => "*GameMode_5v5_Private*",
        "5v5_private_draft" => "*GameMode_5v5_PrivateDraft*",
        "5v5_private_bots" => "*GameMode_5v5_PrivateBots*",
        "5v5_practice" => "*GameMode_5v5_Practice*",
        "aral_casual" | "casual_aral" | "aral" => "*GameMode_Aral_Casual*",
        // Includes Android's 5v5_pvp_casual and operator-defined queues.
        // Only the wire label is resolved here; the queue ID remains intact.
        _ => "*GameMode_5v5_Casual*",
    }
}
pub(super) fn scene_mode(queue: &str, flag: u8) -> Message {
    let mut body = vec![0; 69];
    put_text(&mut body[..64], mode_name(queue));
    body[68] = flag;
    Message::new(0x046f, body)
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
    Message::new(0x3e9, b)
}
pub(super) fn world_settings(seat: usize, queue: &str) -> Message {
    settings(seat, mode_name(queue), false)
}

pub fn setup(
    world: &World,
    players: &[Uuid],
    catalog: &[String],
    seat: usize,
    mode: &str,
    picks: &[Pick],
) -> Vec<Message> {
    let game_mode = mode_name(mode);
    let mut mode = vec![0; 68];
    put_text(&mut mode[4..], game_mode);
    let mut out = vec![settings(seat, game_mode, true), Message::new(0x454, mode)];
    for name in catalog {
        let mut b = vec![0; 32];
        put_text(&mut b, name);
        out.push(Message::new(0x453, b));
    }
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
        if let Some(e) = world.entities.iter().take(10).nth(idx) {
            r[8] = 1;
            let pick = &picks[idx];
            if pick.selected {
                r[10] = 1;
                r[11] = u8::from(pick.locked);
                r[17..19].copy_from_slice(&e.type_id.to_be_bytes());
                r[21..25].copy_from_slice(&pick.skin.to_be_bytes());
            }
            r[16] = e.team + 1;
            r[19..21].copy_from_slice(&wire_id(e.id).to_be_bytes());
            if let Some(uuid) = players.get(idx) {
                put_text(&mut r[25..57], "Guest");
                put_text(&mut r[105..141], &uuid.to_string());
            } else {
                put_text(&mut r[25..57], &format!("{} Bot", pick.name));
                put_text(&mut r[105..141], "__Kindred_Player_Bot__");
            }
        }
    }
    Message::new(0x459, body)
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
    Message::new(0x3f3, b)
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
        "Guest".into()
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
    Message::new(0x3ee, b)
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
    out.push(Message::new(0x46c, [0]));
    out.push(scene_mode(mode, 1));
    out.push(Message::new(0x470, [0]));
    out.extend((0..10).map(|i| info(world, players, picks, i, true)));
    out.push(Message::new(0x451, [0; 4]));
    out
}

pub(super) fn item_grant(actor: u32, item: u32, instance: u32) -> Message {
    Message::new(
        0x43d,
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
    for held in &entity.inventory {
        out.push(item_grant(actor, held.kind.template(), held.instance));
        // HP, max energy, armor, shield and weapon power already include
        // equipment in the full declaration. Restore the remaining modifiers.
        for (stat, bonus) in world.item_bonuses(held.kind) {
            if matches!(stat, 3 | 5 | 15 | 25) {
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
        for (stat, bonus) in world.item_bonuses(used.kind) {
            out.push(modifier(actor, stat, -bonus));
        }
        let mut body = [actor.to_be_bytes(), used.instance.to_be_bytes()].concat();
        body.extend(1u16.to_be_bytes());
        out.push(Message::new(0x44b, body));
    }
    let granted = receipt.granted;
    out.push(item_grant(actor, granted.kind.template(), granted.instance));
    for (stat, bonus) in world.item_bonuses(granted.kind) {
        out.push(modifier(actor, stat, bonus));
    }
}

pub(super) fn modifier(actor: u32, stat: u8, value: f32) -> Message {
    let mut b = [
        actor.to_be_bytes(),
        u32::MAX.to_be_bytes(),
        value.to_be_bytes(),
    ]
    .concat();
    b.extend([stat, 1]);
    Message::new(0x41c, b)
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
        return Some(Message::new(0x0434, body));
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
        return Some(Message::new(0x0432, body));
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
fn hero_counter(id: u32, stat: u8) -> Message {
    let mut body = vec![0; 22];
    body[..4].copy_from_slice(&u32::from(wire_id(id)).to_be_bytes());
    body[4..8].copy_from_slice(&u32::MAX.to_be_bytes());
    body[8..12].copy_from_slice(&1f32.to_be_bytes());
    body[12] = stat;
    Message::new(0x41c, body)
}

#[cfg(test)]
mod combat_tests {
    use super::*;
    use halcyon_protocol::Fields;
    use halcyon_sim::Event;
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
            assert_eq!(message.opcode, 0x0434);
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
            [0x430, 0x41c, 0x41c]
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
