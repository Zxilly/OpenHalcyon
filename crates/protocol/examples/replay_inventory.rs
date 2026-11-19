//! Print aggregate coverage for a directory containing private VGR chunks.
use halcyon_protocol::{AndroidLayout, Fields, READ_SIZES, ReplayReader};
use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, BufReader, BufWriter, Write},
    path::Path,
};

#[derive(Default)]
struct Counts {
    typed: u64,
    opaque: u64,
}

fn inspect(path: &Path, inventory: &mut BTreeMap<u16, Counts>) -> io::Result<()> {
    let mut reader = ReplayReader::new(BufReader::new(fs::File::open(path)?));
    let mut index = 0;
    loop {
        let record = reader.next_record().map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("{} record {index}: {error}", path.display()),
            )
        })?;
        let Some(record) = record else { return Ok(()) };
        let message = record.message;
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
        match message.fields().map_err(context)? {
            Fields::Opaque(_) => counts.opaque += 1,
            _ => counts.typed += 1,
        }
        index += 1;
    }
}

fn main() -> io::Result<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let [directory] = args.as_slice() else {
        return Err(io::Error::other(
            "usage: replay_inventory <chunk-directory>",
        ));
    };
    let mut inventory: BTreeMap<u16, Counts> = READ_SIZES
        .iter()
        .map(|&(op, _)| (op, Counts::default()))
        .collect();
    let mut chunks = 0;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_file()
            && path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("vgr"))
        {
            inspect(&path, &mut inventory)?;
            chunks += 1;
        }
    }
    if chunks == 0 {
        return Err(io::Error::other("directory contains no .vgr chunks"));
    }
    let mut out = BufWriter::new(io::stdout().lock());
    writeln!(
        out,
        "opcode\tandroid_reader_bytes\trecords_with_fields\topaque_records"
    )?;
    for (opcode, counts) in inventory {
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
