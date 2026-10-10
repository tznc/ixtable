//! Test fixtures under `tests/fixtures/access` (see its README).
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/access")
}

pub fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ixtable-access-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d.join(name)
}

/// The template fixture folder zipped into an `.accdt` package.
pub fn template() -> PathBuf {
    let out = temp("Order Desk.accdt");
    let root = dir().join("template");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&out).unwrap());
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut stack = vec![root.clone()];
    while let Some(d) = stack.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(&d)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let name = p
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            zip.start_file(name, options).unwrap();
            zip.write_all(&std::fs::read(&p).unwrap()).unwrap();
        }
    }
    zip.finish().unwrap();
    out
}

/// A gzipped binary fixture, unpacked to a temporary file.
pub fn binary(name: &str) -> PathBuf {
    let gz = std::fs::read(dir().join(format!("{name}.gz"))).unwrap();
    let mut bytes = vec![];
    flate2::read::GzDecoder::new(&gz[..])
        .read_to_end(&mut bytes)
        .unwrap();
    let out = temp(name);
    std::fs::write(&out, bytes).unwrap();
    out
}
