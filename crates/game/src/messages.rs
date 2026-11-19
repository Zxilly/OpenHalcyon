//! CE server messages. Unidentified fields use zero defaults.
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
pub fn setup(
    world: &World,
    players: &[Uuid],
    catalog: &[String],
    seat: usize,
    mode: &str,
) -> Vec<Message> {
    let game_mode = if mode.contains("aral") {
        "*GameMode_Aral_Casual*"
    } else {
        "*GameMode_5v5_SoloBots*"
    };
    let mut settings = vec![0; 101];
    settings[..4].copy_from_slice(&u32::from(wire_id(seat as u32 + 1)).to_be_bytes());
    settings[7] = 1;
    settings[10..12].fill(0xff);
    settings[16..18].fill(5);
    put_text(&mut settings[32..64], game_mode);
    let mut mode = vec![0; 68];
    put_text(&mut mode[4..], game_mode);
    let mut out = vec![Message::new(0x3e9, settings), Message::new(0x454, mode)];
    for name in catalog {
        let mut b = vec![0; 32];
        put_text(&mut b, name);
        out.push(Message::new(0x453, b));
    }
    out.push(snapshot(world, players));
    out.push(Message::new(0x3f1, [0, 0, 0, 0, 8, 0]));
    out
}
pub fn snapshot(world: &World, players: &[Uuid]) -> Message {
    let mut body = vec![0; 2590];
    body[..4].copy_from_slice(&((world.tick as f32) / 20.).to_be_bytes());
    for idx in 0..16 {
        let r = &mut body[idx * 161..][..161];
        r[9] = idx as u8;
        r[12..15].fill(0xff);
        r[17..21].fill(0xff);
        if let Some(e) = world.entities.iter().take(10).nth(idx) {
            r[8] = 1;
            r[16] = e.team + 1;
            r[19..21].copy_from_slice(&wire_id(e.id).to_be_bytes());
            if let Some(uuid) = players.get(idx) {
                put_text(&mut r[25..57], "Guest");
                put_text(&mut r[105..141], &uuid.to_string());
            } else {
                put_text(&mut r[25..57], &format!("Ringo Bot {}", idx + 1));
                put_text(&mut r[105..141], "__Kindred_Player_Bot__");
            }
        }
    }
    Message::new(0x459, body)
}
pub fn spawn(e: &Entity) -> Message {
    let mut b = vec![0; 750];
    b[2..4].copy_from_slice(&e.type_id.to_be_bytes());
    b[10..12].copy_from_slice(&wire_id(e.id).to_be_bytes());
    b[12..16].copy_from_slice(&(u32::from(e.team) + 1).to_be_bytes());
    b[16..18].fill(0xff);
    b[18..22].copy_from_slice(&e.position.x.to_be_bytes());
    b[22..26].copy_from_slice(&e.position.y.to_be_bytes());
    Message::new(0x3f3, b)
}
