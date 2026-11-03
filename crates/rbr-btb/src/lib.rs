//! BTB (Bob's Track Builder) stages, as RallySimFans installs them in `RX_CONTENT/Tracks/<folder>/` and its
//! `rbr_rx.dll` plugin plays them. Everything BTB-specific lives in this crate, so the rest of ReRBR keeps only small
//! hooks into it: dropping BTB support is dropping this crate and those hooks. Findings: `notes/btb.md`.

pub mod classic;
pub mod ids;
pub mod track;
pub mod xfile;
pub mod xform;
