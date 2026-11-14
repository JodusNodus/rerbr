//! The scratch accumulate: the body's scale times the vector `delta` is added to the scratch, lane by lane. When the
//! gate word has `FLUSH_BIT` the call runs the body's flush, which the host makes. The gate is the caller's.

use super::host::HostUnavailable;
use super::layout::{Body, Vec4};

/// The gate bit that runs the flush.
pub const FLUSH_BIT: u32 = 0x4000;

/// The body's flush, which the gate runs. Its result is not used.
pub trait FlushHost {
    fn flush(&mut self) -> Result<(), HostUnavailable>;
}

/// Adds `delta * scratch_scale` to the scratch, then runs the flush when `gate` has `FLUSH_BIT`. A host error stops the
/// call after the scratch is written.
pub fn accumulate(body: &mut Body, delta: Vec4, gate: u32, host: &mut impl FlushHost) -> Result<(), HostUnavailable> {
    let k = body.scratch_scale;
    let s = body.scratch;
    body.scratch = Vec4 { x: s.x + delta.x * k, y: s.y + delta.y * k, z: s.z + delta.z * k, w: s.w + delta.w * k };
    if gate & FLUSH_BIT != 0 {
        host.flush()?;
    }
    Ok(())
}
