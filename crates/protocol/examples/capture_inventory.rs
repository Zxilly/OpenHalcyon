//! Offline inventory of length-prefixed CE frames inside proxy segment archives.
#[path = "support/fixed_bodies.rs"]
mod fixed_bodies;
use halcyon_protocol::{
    ACK_FRAME, AndroidLayout, Fields, MAX_PAYLOAD, MatchCipher, parse_hello_endpoint, read_frame,
};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    env,
    fs::{self, File},
    io::{self, BufReader, Read},
    path::Path,
};
use uuid::Uuid;

#[derive(Deserialize)]
struct Meta {
    match_id: String,
    real_port: Option<u16>,
    #[serde(default)]
    build: Option<serde_json::Value>,
    #[serde(default)]
    revision: Option<serde_json::Value>,
}
#[derive(Default)]
struct Counts {
    frames: u64,
    typed: u64,
    opaque: u64,
    errors: u64,
    short_android: u64,
    sizes: BTreeMap<usize, u64>,
    min: usize,
    max: usize,
}
#[derive(Default)]
struct Stream {
    pending: Vec<u8>,
    reencoded: Vec<u8>,
    hello: bool,
    hello_len: usize,
    hello_port: Option<u16>,
}
fn invalid(msg: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}
impl Stream {
    fn ingest(
        &mut self,
        bytes: &[u8],
        direction: usize,
        cipher: &MatchCipher,
        ops: &mut BTreeMap<u16, Counts>,
    ) -> io::Result<()> {
        self.pending.extend_from_slice(bytes);
        let mut at = 0;
        while self.pending.len() - at >= 2 {
            let left = &self.pending[at..];
            let n = usize::from(u16::from_be_bytes([left[0], left[1]]));
            if n == 0 || n > MAX_PAYLOAD {
                return Err(invalid("invalid frame length"));
            }
            if left.len() < n + 2 {
                break;
            }
            let mut whole = &left[..n + 2];
            let frame = read_frame(&mut whole)?.ok_or(invalid("missing complete frame"))?;
            if !self.hello {
                if direction == 0 {
                    let endpoint = parse_hello_endpoint(&frame)?;
                    self.hello_port = endpoint.port();
                } else if frame != ACK_FRAME[2..] {
                    return Err(invalid("invalid server handshake"));
                }
                self.hello = true;
                self.hello_len = n;
            } else {
                let m = cipher.decode_owned(frame)?;
                self.reencoded.clear();
                cipher.encode_into(&m, &mut self.reencoded)?;
                if self.reencoded != left[..n + 2] {
                    return Err(invalid("encrypted frame did not reproduce its input bytes"));
                }
                fixed_bodies::verify(&m)?;
                let e = ops.entry(m.opcode).or_default();
                e.min = if e.frames == 0 {
                    m.body.len()
                } else {
                    e.min.min(m.body.len())
                };
                e.max = e.max.max(m.body.len());
                e.frames += 1;
                *e.sizes.entry(n).or_default() += 1;
                if direction == 1
                    && AndroidLayout::for_opcode(m.opcode)
                        .split_body(&m.body)
                        .is_err()
                {
                    e.short_android += 1;
                }
                match m.fields() {
                    Ok(Fields::Live(halcyon_protocol::LiveFields::MapPing(p))) => {
                        if p.message() != m {
                            return Err(invalid("ping fields did not reproduce the frame"));
                        }
                        e.typed += 1;
                    }
                    Ok(Fields::ActionWords(_))
                        if m.is(halcyon_protocol::Opcode::PlayPfxAtLocation) =>
                    {
                        let effect = m
                            .play_pfx_at_location()
                            .ok_or_else(|| invalid("invalid location effect"))?;
                        if effect.to_message() != m {
                            return Err(invalid("location effect did not reproduce original"));
                        }
                        e.typed += 1;
                    }
                    Ok(Fields::Live(halcyon_protocol::LiveFields::HudQuickMessage(q))) => {
                        if q.message() != m {
                            return Err(invalid("HUD fields did not reproduce the frame"));
                        }
                        e.typed += 1;
                    }
                    Ok(
                        f @ (Fields::ServerMove { .. }
                        | Fields::MoveAndFace { .. }
                        | Fields::StopActor { .. }),
                    ) => {
                        if f.movement_message().as_ref() != Some(&m) {
                            return Err(invalid(
                                "native movement fields did not reproduce the frame",
                            ));
                        }
                        e.typed += 1;
                    }
                    Ok(_)
                        if matches!(
                            m.opcode,
                            halcyon_protocol::wire_opcode::EntityActA
                                ..=halcyon_protocol::wire_opcode::PosUpdate
                        ) =>
                    {
                        let flags = m
                            .actor_flags()
                            .ok_or_else(|| invalid("invalid actor flags"))?;
                        if flags.to_message() != m {
                            return Err(invalid("actor flags did not reproduce original"));
                        }
                        e.typed += 1;
                    }
                    Ok(Fields::ActionWords(_))
                        if m.is(halcyon_protocol::Opcode::ModifyBuffStack) =>
                    {
                        let stack = m
                            .buff_stack()
                            .ok_or_else(|| invalid("unsupported buff stack body"))?;
                        if stack.to_message()? != m {
                            return Err(invalid("buff stack did not reproduce original"));
                        }
                        e.typed += 1;
                    }
                    Ok(Fields::ActionWords(_))
                        if matches!(
                            m.opcode,
                            halcyon_protocol::wire_opcode::CreateZoneOfControlVariantB
                                | halcyon_protocol::wire_opcode::DestroyZoneOfControl
                        ) =>
                    {
                        let zone = m
                            .zone_action()
                            .ok_or_else(|| invalid("unsupported zone action body"))?;
                        if zone.to_message()? != m {
                            return Err(invalid("zone action did not reproduce original"));
                        }
                        e.typed += 1;
                    }
                    Ok(Fields::BuffAction { .. }) => {
                        let action = m
                            .buff_mutation()
                            .ok_or_else(|| invalid("missing buff mutation"))?;
                        if action.to_message()? != m {
                            return Err(invalid("buff mutation fields did not reproduce original"));
                        }
                        e.typed += 1;
                    }
                    Ok(Fields::BuffApplication(action)) => {
                        if action.to_message()? != m {
                            return Err(invalid(
                                "buff application fields did not reproduce original",
                            ));
                        }
                        e.typed += 1;
                    }
                    Ok(Fields::Projectile(action)) => {
                        if action.message().as_ref() != Some(&m) {
                            return Err(invalid("projectile fields did not reproduce original"));
                        }
                        e.typed += 1;
                    }
                    Ok(Fields::Opaque(_)) => e.opaque += 1,
                    Ok(_) => e.typed += 1,
                    Err(_) => e.errors += 1,
                }
            }
            at += n + 2;
        }
        self.pending.drain(..at);
        Ok(())
    }
}
fn fill(reader: &mut impl Read, mut target: &mut [u8]) -> io::Result<usize> {
    let size = target.len();
    while !target.is_empty() {
        let n = reader.read(target)?;
        if n == 0 {
            break;
        }
        target = &mut target[n..];
    }
    Ok(size - target.len())
}

fn inspect(path: &Path, ops: &mut [BTreeMap<u16, Counts>; 2], sizes: bool) -> io::Result<bool> {
    let f = File::open(path.join("match_meta.json"))?;
    if f.metadata()?.len() > 16 << 20 {
        return Err(invalid("oversized metadata"));
    }
    let meta: Meta =
        serde_json::from_reader(BufReader::new(f)).map_err(|_| invalid("invalid metadata"))?;
    Uuid::parse_str(&meta.match_id).map_err(|_| invalid("invalid match identifier"))?;
    let build = meta
        .build
        .as_ref()
        .or(meta.revision.as_ref())
        .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse::<u64>().ok()));
    let cipher = MatchCipher::new(&meta.match_id);
    let mut input = BufReader::new(File::open(path.join("packets.bin"))?);
    let mut streams = [Stream::default(), Stream::default()];
    let mut chunk = [0; 8192];
    let mut short_segment = 0;
    let mut short_header = 0;
    loop {
        let mut head = [0; 13];
        let n = fill(&mut input, &mut head)?;
        if n < 13 {
            short_header = n;
            break;
        }
        let d = usize::from(head[8]);
        if d > 1 {
            return Err(invalid("unknown segment direction"));
        }
        let mut left = u32::from_be_bytes(head[9..13].try_into().unwrap()) as usize;
        while left > 0 {
            let take = left.min(chunk.len());
            let n = fill(&mut input, &mut chunk[..take])?;
            streams[d].ingest(&chunk[..n], d, &cipher, &mut ops[d])?;
            left -= n;
            if n < take {
                short_segment = left;
                break;
            }
        }
        if short_segment != 0 {
            break;
        }
    }
    let tails = [streams[0].pending.len(), streams[1].pending.len()];
    let clean = short_segment == 0
        && short_header == 0
        && tails == [0, 0]
        && streams.iter().all(|s| s.hello);
    println!(
        "build={} hello={} ack={} short_segment_header_bytes={} missing_segment_bytes={} c2s_tail_bytes={} s2c_tail_bytes={}",
        build.map_or("unspecified".into(), |v| v.to_string()),
        streams[0].hello,
        streams[1].hello,
        short_header,
        short_segment,
        tails[0],
        tails[1]
    );
    if clean && sizes {
        if let Some(port) = meta.real_port {
            if streams[0].hello_port != Some(port) {
                return Err(invalid("hello port differs from archive metadata"));
            }
            println!("hello_port_check\tok");
        }
        for (direction, opcode, stream) in [
            ("C2S", halcyon_protocol::Opcode::Hello, &streams[0]),
            ("S2C", halcyon_protocol::Opcode::Heartbeat, &streams[1]),
        ] {
            println!(
                "handshake\t{direction}\t{}\t{}\t1",
                opcode.raw(),
                stream.hello_len
            );
        }
    }
    Ok(clean)
}
fn main() -> io::Result<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let (args, sizes) = if args.last().is_some_and(|a| a == "--sizes") {
        (&args[..args.len() - 1], true)
    } else {
        (args.as_slice(), false)
    };
    let [root] = args else {
        return Err(invalid(
            "usage: capture_inventory <archive-directory> [--sizes]",
        ));
    };
    let root = Path::new(root);
    let mut dirs = vec![];
    if root.join("match_meta.json").is_file() {
        dirs.push(root.to_owned());
    } else {
        for f in fs::read_dir(root)? {
            let f = f?;
            if f.file_type()?.is_dir() && f.path().join("match_meta.json").is_file() {
                dirs.push(f.path());
            }
        }
    }
    dirs.sort();
    let mut inspected = 0;
    let mut bad = false;
    let mut ops = Default::default();
    for (idx, d) in dirs.iter().enumerate() {
        print!("archive={} ", idx + 1);
        if !d.join("packets.bin").is_file() {
            println!("no_packets");
            continue;
        }
        inspected += 1;
        match inspect(d, &mut ops, sizes) {
            Ok(clean) => bad |= !clean,
            Err(error) => {
                println!("error={error}");
                bad = true;
            }
        }
    }
    if inspected == 0 {
        return Err(invalid("no packet archives"));
    }
    println!(
        "direction\topcode\tframes\ttyped_shape\topaque\tfield_errors\tshort_android_reader\tbody_min\tbody_max"
    );
    for (direction, values) in ["C2S", "S2C"].into_iter().zip(ops) {
        let mut total = 0;
        for (op, c) in &values {
            total += c.frames;
            if sizes {
                println!(
                    "registry\t{op}\t{}",
                    halcyon_protocol::Opcode::try_from(*op).is_ok()
                );
                for (size, count) in &c.sizes {
                    println!("size\t{direction}\t{op}\t{size}\t{count}");
                }
            }
            println!(
                "{direction}\t0x{op:04X}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                c.frames, c.typed, c.opaque, c.errors, c.short_android, c.min, c.max
            );
        }
        println!("total {direction} frames={total} opcodes={}", values.len());
    }
    if bad {
        Err(invalid("incomplete or invalid archive boundary"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use halcyon_protocol::{Message, hello};

    #[test]
    fn fragmented_handshake_and_coalesced_cipher_frames_keep_direction_and_tails() {
        let cipher = MatchCipher::new("00000000-0000-4000-8000-000000000001");
        let mut streams = [Stream::default(), Stream::default()];
        let mut counts: [BTreeMap<u16, Counts>; 2] = Default::default();
        let c_wire = [
            hello("127.0.0.1").unwrap(),
            cipher
                .encode(&Message::known(
                    halcyon_protocol::Opcode::RequestUpgradeAbility,
                    [1],
                ))
                .unwrap(),
        ]
        .concat();
        let mut s_wire = ACK_FRAME.to_vec();
        let frame = cipher
            .encode(&Message::known(
                halcyon_protocol::Opcode::DestroyZoneOfControl,
                1u32.to_be_bytes(),
            ))
            .unwrap();
        s_wire.extend_from_slice(&frame);
        s_wire.extend_from_slice(&frame);
        s_wire.extend_from_slice(&frame[..3]);
        for (direction, wire) in [c_wire, s_wire].iter().enumerate() {
            for chunk in wire.chunks(3) {
                streams[direction]
                    .ingest(chunk, direction, &cipher, &mut counts[direction])
                    .unwrap();
            }
        }
        assert_eq!(
            counts[0][&halcyon_protocol::wire_opcode::RequestUpgradeAbility].frames,
            1
        );
        assert_eq!(
            counts[1][&halcyon_protocol::wire_opcode::DestroyZoneOfControl].frames,
            2
        );
        assert_eq!(streams[1].pending.len(), 3);
        assert!(streams[0].pending.is_empty());
        assert!(
            Stream::default()
                .ingest(&[255, 255], 0, &cipher, &mut counts[0])
                .is_err()
        );
    }
}
