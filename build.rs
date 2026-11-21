//! Packs everything in `static/` into the executable, gzip-compressed, so the
//! app is a single file. Generates `$OUT_DIR/embedded.rs` with a lookup table.

use flate2::{Compression, write::GzEncoder};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::{env, fs};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

fn fnv(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3))
}

fn main() {
    println!("cargo:rerun-if-changed=static");
    println!("cargo:rerun-if-changed=build.rs");
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("static");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let mut files = Vec::new();
    walk(&root, &mut files);

    let mut table = String::from("pub static ASSETS: &[(&str, &[u8], &str)] = &[\n");
    for (i, file) in files.iter().enumerate() {
        let rel = file.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let data = fs::read(file).unwrap();
        let mut enc = GzEncoder::new(Vec::new(), Compression::best());
        enc.write_all(&data).unwrap();
        let gz = enc.finish().unwrap();
        let gz_path = out_dir.join(format!("asset{i}.gz"));
        fs::write(&gz_path, &gz).unwrap();
        table.push_str(&format!(
            "    (\"/{rel}\", include_bytes!({:?}), \"\\\"{:016x}\\\"\"),\n",
            gz_path.to_string_lossy().replace('\\', "/"),
            fnv(&data)
        ));
    }
    table.push_str("];\n");
    fs::write(out_dir.join("embedded.rs"), table).unwrap();
}
