//! The force at a carcass edge's ground hit along the hit's normal, with friction against the part of the edge's velocity
//! across the normal. The sequences are `layout::edge_forces` and `layout::edge_force`.

use crate::math::sse;
use crate::math::vec::Vec4;

/// The force per unit of the scaled probe direction along the normal (800000; named by use).
const STIFFNESS: f32 = 800_000.0;
/// The size of the across-normal velocity below which no friction is applied.
const MIN_SLIDING: f32 = 0.001;
/// The friction against the across-normal velocity, as a fraction of the normal force (0.3).
const FRICTION: f32 = 0.3;

/// What [`axis_force`] reads: the hit's normal `axis` (four lanes) and its ray block's scale (named by use), the ground
/// probe's direction, the velocity `v` (three lanes), the force so far (four lanes) and the word the original takes as
/// the fourth lane of the across-normal part (a leftover, which the model keeps as an input).
#[derive(Clone, Copy, Debug)]
pub struct AxisForceIn {
    pub axis: Vec4,
    pub scale: f32,
    pub probe_direction: Vec4,
    pub velocity: Vec4,
    pub force: Vec4,
    pub residue: f32,
}

/// What [`axis_force`] writes: the force and the across-normal part of the velocity (four lanes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxisForceOut {
    pub force: Vec4,
    pub across: Vec4,
}

/// `force + d * s` in four lanes: the vector's product with `s` (the vector first), then the sum (the product first).
fn add_scaled(force: Vec4, d: Vec4, s: f32) -> Vec4 {
    Vec4 {
        x: sse::add(sse::mul(d.x, s), force.x),
        y: sse::add(sse::mul(d.y, s), force.y),
        z: sse::add(sse::mul(d.z, s), force.z),
        w: sse::add(sse::mul(d.w, s), force.w),
    }
}

/// The velocity `v` split along the normal `n`: `p = (v . n) n`, the across part `r = v - p` in three lanes (lane 3 the
/// residue). The force gets `n * s` with `s = -(probe_direction . n) * scale * 800000` (that dot summed y, z, then x),
/// then, when `|r| > 0.001`, `r * ((-1 / |r|) * s * 0.3)`. Every lane in the original's SSE order.
pub fn axis_force(i: &AxisForceIn) -> AxisForceOut {
    let (n, v) = (i.axis, i.velocity);
    let dot = sse::add(sse::add(sse::mul(v.x, n.x), sse::mul(v.y, n.y)), sse::mul(v.z, n.z));
    let p = Vec4 { x: sse::mul(dot, n.x), y: sse::mul(dot, n.y), z: sse::mul(dot, n.z), w: sse::mul(dot, n.w) };
    let across = Vec4 { x: sse::sub(v.x, p.x), y: sse::sub(v.y, p.y), z: sse::sub(v.z, p.z), w: i.residue };
    // The length is the square root of `(x*x + y*y) + z*z`, taken as a float.
    let len =
        sse::add(sse::add(sse::mul(across.x, across.x), sse::mul(across.y, across.y)), sse::mul(across.z, across.z))
            .sqrt();
    let d = i.probe_direction;
    let k = sse::add(sse::add(sse::mul(d.y, n.y), sse::mul(d.z, n.z)), sse::mul(d.x, n.x));
    let s = sse::mul(sse::sub(0.0, sse::mul(k, i.scale)), STIFFNESS);
    let mut force = add_scaled(i.force, n, s);
    if len > MIN_SLIDING {
        force = add_scaled(force, across, sse::mul(sse::mul(sse::div(-1.0, len), s), FRICTION));
    }
    AxisForceOut { force, across }
}
