//! Reader for Bob's Track Builder `.xform` placement files.
//!
//! A `.xform` file is an array of 4x4 `f32` matrices in row-major order
//! (Direct3D convention: the translation is elements 12..14, element 15 is
//! 1.0), one per placed instance of the mesh its `objectlist.ini` entry
//! names. See `notes/formats.md` "BTB meshes".

use crate::le::{self, Reader, Truncated};
use std::fmt::{self, Display, Formatter};

/// Failure to read an `.xform` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Truncated {
        offset: usize,
        needed: usize,
        len: usize,
    },
    /// The file length is not a multiple of one 4x4 matrix (64 bytes).
    Misaligned {
        len: usize,
    },
}

impl From<Truncated> for Error {
    fn from(t: Truncated) -> Self {
        Error::Truncated { offset: t.offset, needed: t.needed, len: t.len }
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Error::Truncated { offset, needed, len } => le::fmt_truncated(f, *offset, *needed, *len),
            Error::Misaligned { len } => write!(f, "length {len} is not a multiple of one 4x4 matrix (64 bytes)"),
        }
    }
}

impl std::error::Error for Error {}

/// Reads the file into its placement matrices, each 16 `f32` in row-major
/// file order. Errors when the length is not a multiple of 64 bytes.
pub fn parse(bytes: &[u8]) -> Result<Vec<[f32; 16]>, Error> {
    if !bytes.len().is_multiple_of(64) {
        return Err(Error::Misaligned { len: bytes.len() });
    }
    let mut r = Reader::<Error>::new(bytes);
    let mut out = Vec::with_capacity(bytes.len() / 64);
    while !r.is_empty() {
        let mut m = [0.0f32; 16];
        for slot in &mut m {
            *slot = r.f32()?;
        }
        out.push(m);
    }
    Ok(out)
}
