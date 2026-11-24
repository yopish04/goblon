//! Embeds every texture image of the repository folder `textures/` into the engine
//! (`crate::library`). Adding a file there and rebuilding (or pushing: the Windows
//! build runs on GitHub) is all it takes for the app to offer it.

use std::fs;
use std::path::Path;

const EXT: [&str; 7] = ["png", "jpg", "jpeg", "tif", "tiff", "webp", "bmp"];

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../textures");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut files: Vec<(String, String)> = Vec::new();
    if let Ok(rd) = fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            let ok = p.is_file()
                && p.extension()
                    .and_then(|x| x.to_str())
                    .is_some_and(|x| EXT.contains(&x.to_ascii_lowercase().as_str()));
            if !ok {
                continue;
            }
            println!("cargo:rerun-if-changed={}", p.display());
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let abs = fs::canonicalize(&p).unwrap().to_string_lossy().into_owned();
            files.push((name, abs));
        }
    }
    files.sort();
    let mut out = String::from(
        "/// (file name, bytes) of every texture in the repository folder textures/\npub static LIBRARY: &[(&str, &[u8])] = &[\n",
    );
    for (name, abs) in &files {
        out += &format!("    ({name:?}, include_bytes!({abs:?})),\n");
    }
    out += "];\n";
    let dst = Path::new(&std::env::var("OUT_DIR").unwrap()).join("textures.rs");
    fs::write(dst, out).unwrap();
}
