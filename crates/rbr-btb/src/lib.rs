//! BTB (Bob's Track Builder) stages, as RallySimFans installs them in `RX_CONTENT/Tracks/<folder>/` and its
//! `rbr_rx.dll` plugin plays them. Everything BTB-specific lives in this crate, so the rest of ReRBR keeps only small
//! hooks into it: dropping BTB support is dropping this crate and those hooks. Findings: `notes/btb.md`.

/// Which `d3dx9_30.dll` the plugin ran against when it built a stage's files. Its float results differ between the two
/// (`notes/btb.md` "Collision" and "Collision meshes (ksh)"); ReRBR builds [`D3dx::Windows`]'s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum D3dx {
    /// The native Windows DLL: every non-zero mesh position one unit in the last place down on load
    /// ([`col::shift_input`]), the pairwise transform sum ([`col::transform`]), the x87 quaternion with an SSE
    /// `rsqrtss` normalise ([`ksh::rsqrtss`], bit-exact where the CPU's is Intel's).
    #[default]
    Windows,
    /// Wine's builtin DLL: the file's floats, a left-to-right transform sum, no surface map indexes, and its `math.c`
    /// quaternion normalised by division.
    Wine,
}

pub mod classic;
pub mod col;
pub mod ids;
pub mod ksh;
pub mod scene;
pub mod track;
pub mod xfile;
pub mod xform;
