//! Print aggregate coverage for a directory containing private VGR chunks.
#[path = "support/fixed_bodies.rs"]
mod fixed_bodies;
use halcyon_protocol::{AndroidLayout, Fields, READ_SIZES, ReplayReader, ReplayWriter};
use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, BufRead, BufReader, BufWriter, Read, Write},
    path::Path,
};

#[derive(Default)]
struct Counts {
    typed: u64,
    opaque: u64,
    sizes: BTreeMap<usize, u64>,
}

fn inspect(
    path: &Path,
    inventory: &mut BTreeMap<u16, Counts>,
    entities: &mut BTreeMap<u32, (u32, u32, u32, usize)>,
) -> io::Result<()> {
    let mut reader = ReplayReader::new(BufReader::new(fs::File::open(path)?));
    let mut original = BufReader::new(fs::File::open(path)?);
    let mut encoded = Vec::new();
    let mut reference = Vec::new();
    let mut index = 0;
    loop {
        let record = reader.next_record().map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("{} record {index}: {error}", path.display()),
            )
        })?;
        let Some(record) = record else {
            if !original.fill_buf()?.is_empty() {
                return Err(io::Error::other("replay writer omitted trailing bytes"));
            }
            return Ok(());
        };
        encoded.clear();
        ReplayWriter::new(&mut encoded).write_record(record.time, &record.message)?;
        reference.resize(encoded.len(), 0);
        original.read_exact(&mut reference)?;
        if encoded != reference {
            return Err(io::Error::other(format!(
                "replay round-trip mismatch at record {index}"
            )));
        }
        let message = record.message;
        fixed_bodies::verify(&message)?;
        let counts = inventory.entry(message.opcode).or_default();
        let context = |error: io::Error| {
            io::Error::new(
                error.kind(),
                format!(
                    "{} record {index}, opcode {:#06x}: {error}",
                    path.display(),
                    message.opcode
                ),
            )
        };
        AndroidLayout::for_opcode(message.opcode)
            .split_body(&message.body)
            .map_err(context)?;
        *counts.sizes.entry(message.body.len() + 2).or_default() += 1;
        match message.fields().map_err(context)? {
            Fields::Spawn {
                entity,
                template,
                skin,
                team,
                ..
            } => {
                counts.typed += 1;
                entities
                    .entry(entity)
                    .or_insert((template, skin, team, message.body.len() + 2));
            }

            Fields::Opaque(_) => counts.opaque += 1,
            _ => counts.typed += 1,
        }
        index += 1;
    }
}

fn main() -> io::Result<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let (args, reference) = match args.as_slice() {
        [dir, flag, path] if flag == "--empirical" => (std::slice::from_ref(dir), Some(path)),
        args => (args, None),
    };
    let [directory] = args else {
        return Err(io::Error::other(
            "usage: replay_inventory <chunk-directory> [--empirical <entities.json>]",
        ));
    };
    let mut inventory: BTreeMap<u16, Counts> = READ_SIZES
        .iter()
        .map(|&(op, _)| (op, Counts::default()))
        .collect();
    let mut chunks = Vec::new();
    let mut entities = BTreeMap::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_file()
            && path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("vgr"))
        {
            chunks.push(path);
        }
    }
    if chunks.is_empty() {
        return Err(io::Error::other("directory contains no .vgr chunks"));
    }
    chunks.sort_by_cached_key(|p| {
        p.file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.rsplit_once('.'))
            .and_then(|(_, n)| n.parse::<u64>().ok())
    });
    for chunk in chunks {
        inspect(&chunk, &mut inventory, &mut entities)?;
    }
    if let Some(reference) = reference {
        #[derive(serde::Deserialize)]
        struct Row {
            eid: u32,
            entity_type_id: u32,
            skin_hash_hex: String,
            team: u32,
            payload_len: usize,
        }
        let rows: Vec<Row> = serde_json::from_reader(BufReader::new(fs::File::open(reference)?))?;
        let mut expected = BTreeMap::new();
        for r in rows {
            let skin = r
                .skin_hash_hex
                .strip_prefix("0x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .ok_or_else(|| io::Error::other("invalid reference skin hash"))?;
            if expected
                .insert(r.eid, (r.entity_type_id, skin, r.team, r.payload_len))
                .is_some()
            {
                return Err(io::Error::other("duplicate reference actor"));
            }
        }
        if expected != entities {
            return Err(io::Error::other(
                "replay spawn reference disagrees with decoded actors",
            ));
        }
        println!("entity_checks\t{}", entities.len());
    }
    let mut out = BufWriter::new(io::stdout().lock());
    writeln!(
        out,
        "opcode\tandroid_reader_bytes\trecords_with_fields\topaque_records"
    )?;
    for (opcode, counts) in inventory {
        if reference.is_some() && counts.typed + counts.opaque > 0 {
            writeln!(
                out,
                "registry\t{opcode}\t{}",
                halcyon_protocol::Opcode::try_from(opcode).is_ok()
            )?;
            for (size, count) in &counts.sizes {
                writeln!(out, "size\tVGR\t{opcode}\t{size}\t{count}")?;
            }
        }
        let size = match AndroidLayout::for_opcode(opcode) {
            AndroidLayout::Fixed { frame_bytes } => frame_bytes.to_string(),
            AndroidLayout::Clock => "clock".into(),
            AndroidLayout::IgnoredByReceiver => "ignored".into(),
            AndroidLayout::Unlisted => "unlisted".into(),
        };
        writeln!(
            out,
            "0x{opcode:04X}\t{size}\t{}\t{}",
            counts.typed, counts.opaque
        )?;
    }
    out.flush()
}
