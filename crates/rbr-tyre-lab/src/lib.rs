//! Research tooling (ours, experimental): NGP tyre characterisation, and the
//! brush-v1 model (`rbr_tyre`) against it. Lab code only: nothing here changes
//! what the game computes.

pub use rbr_tyre::{brush, calibrate};
pub mod ngp;
pub mod probe;

/// Writes `text` to `path`, creating its folder; panics with the path on failure (lab output).
pub fn write(path: &std::path::Path, text: &str) {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    }
    std::fs::write(path, text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
}
