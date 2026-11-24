//! Built-in vintage textures: every image of the repository folder `textures/`
//! (embedded at build time by build.rs). Layers refer to them as `lib:<file name>`.

include!(concat!(env!("OUT_DIR"), "/textures.rs"));

/// file names of the built-in textures, sorted
pub fn names() -> Vec<&'static str> {
    LIBRARY.iter().map(|(n, _)| *n).collect()
}

/// encoded bytes of a built-in texture
pub fn get(name: &str) -> Option<&'static [u8]> {
    LIBRARY.iter().find(|(n, _)| *n == name).map(|(_, b)| *b)
}

/// display name: file name without extension, '-' and '_' as spaces
pub fn title(name: &str) -> String {
    let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
    stem.replace(['-', '_'], " ")
}
