use sha2::{Digest, Sha256};
use std::{env, fs, io, path::Path};

fn collect_sources(dir: &Path, paths: &mut Vec<String>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_dir() {
            collect_sources(&path, paths)?;
        } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
            let name = path.to_str().ok_or(io::ErrorKind::InvalidData)?;
            paths.push(name.replace('\\', "/"));
        }
    }
    Ok(())
}

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-changed=src");
    let mut paths = Vec::new();
    collect_sources(Path::new("src"), &mut paths)?;
    paths.sort_unstable();
    let mut digest = Sha256::new();
    for name in paths {
        let source = fs::read_to_string(&name)?.replace("\r\n", "\n");
        for bytes in [name.as_bytes(), source.as_bytes()] {
            digest.update((bytes.len() as u64).to_be_bytes());
            digest.update(bytes);
        }
    }
    let out = env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR for build scripts");
    fs::write(Path::new(&out).join("simulation.sha256"), digest.finalize())
}
