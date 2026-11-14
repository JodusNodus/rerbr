//! The drive line in memory, [`crate::ngp::drive_line`]: NGP's sampler `0x10015fc0`, the target of the calls of
//! `CDriveLine::SampleBig` (`0x50b300`) that NGP retargets (`CSolid::VfC8`, `E35CarOps::Step230`), with its own
//! `ClampMix` `0x100161c0` and `BlendB` `0x10016aa0`; and NGP's `BlendB` and `LerpAt` rewrites at `CCar::Split17C`'s
//! calls (`0x4e555f`, `0x4e5571`).

use crate::ngp::drive_line::{hermite_point_f32, project, tangent_lerp_f32, Line, V3};
use crate::stage::store::StageStore;

/// The drive line at `line` in the stage's store: its entries' positions and tangents, in doubles.
struct StoreLine<'a, S: ?Sized> {
    store: &'a S,
    line: u32,
}

fn wide(v: crate::math::vec::Vec4) -> V3 {
    [v.x, v.y, v.z].map(f64::from)
}

impl<S: StageStore + ?Sized> Line for StoreLine<'_, S> {
    fn count(&self) -> u32 {
        self.store.line_count(self.line)
    }

    fn position(&self, k: u32) -> V3 {
        wide(self.store.line_position(self.line, k))
    }

    fn tangent(&self, k: u32) -> V3 {
        wide(self.store.line_tangent(self.line, k))
    }
}

/// NGP's `BlendB` rewrite, `0x10016d40` (thiscall on the drive line, `(i, t, out)`, `ret 0xc`): the Hermite point in
/// doubles, rounded to f32 lane by lane. `out`'s fourth lane is not written.
pub fn point_at<S: StageStore + ?Sized>(store: &S, line: u32, idx: i32, t: f32) -> [f32; 3] {
    hermite_point_f32(&StoreLine { store, line }, idx, t)
}

/// NGP's `LerpAt` rewrite, `0x10016960` (thiscall on the drive line, `(i, t, out)`, `ret 0xc`): the tangents of `i`
/// and `i + 1` lerped. `out`'s fourth lane is not written.
pub fn tangent_at<S: StageStore + ?Sized>(store: &S, line: u32, idx: i32, t: f32) -> [f32; 3] {
    tangent_lerp_f32(&StoreLine { store, line }, idx, t)
}

/// NGP's sampler (its `SampleBig`), `0x10015fc0` (thiscall on the drive line, `(pos, x)`, `ret 8`, st0): the position
/// along the line of the point `p` (the three f32 at `pos`) from the index `x`.
pub fn position_of<S: StageStore + ?Sized>(store: &S, line: u32, p: [f32; 3], x: f32) -> f32 {
    project(&StoreLine { store, line }, p, x)
}
