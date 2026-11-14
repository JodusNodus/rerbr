//! The two-dot combine and the impulse response combine, which add two dot results to two floats. The dot is
//! `a x b` (`o1`), the def's local inverse inertia diagonal times `o1` lanewise, that times `a` (`o2`), then
//! `((o2.y*c.y + o2.z*c.z) + o2.x*c.x) * 0.6`. The x87 runs in single precision here (the recorded results have 24-bit
//! mantissas), so every product and sum rounds to float, in operation order. The temporaries are read in lanes 0..2
//! only, so the port has no stale inputs.
//! The response combine keeps the order: `*f = dot1 + *f` and `*e = (dot2 + *e) + inverse_mass`.

use super::layout::Vec4;
use crate::fpu::{precision, Precision};

/// The 0.6 the dot ends with.
const SCALE: f32 = 0.6;

/// The dot: `a` and `b` are vectors (their first three lanes), `c` is three floats, `scale` is the def's vector (its
/// first three lanes). Returns the single-precision result.
pub fn response_dot(a: Vec4, b: Vec4, c: [f32; 3], scale: Vec4) -> f32 {
    let o1 = a.cross3(b);
    let t = Vec4 { x: scale.x * o1.x, y: scale.y * o1.y, z: scale.z * o1.z, w: scale.w * o1.w };
    let o2 = t.cross3(a);
    let sum = (o2.y * c[1] + o2.z * c[2]) + o2.x * c[0];
    sum * SCALE
}

/// What the response combine reads: the vectors `a`, `b` and `c` (the first three lanes), and the def's local inverse
/// inertia diagonal and inverse mass.
pub struct ResponseInput {
    pub a: Vec4,
    pub b: Vec4,
    pub c: Vec4,
    pub inverse_inertia: Vec4,
    pub inverse_mass: f32,
}

/// The dot at NGP's 53-bit precision control: the dot and its scale round to doubles, and the result stays one.
fn response_dot_pc53(a: Vec4, b: Vec4, c: [f32; 3], scale: Vec4) -> f64 {
    let o1 = a.cross3(b);
    let t = Vec4 { x: scale.x * o1.x, y: scale.y * o1.y, z: scale.z * o1.z, w: scale.w * o1.w };
    let o2 = t.cross3(a);
    let mul = |x: f32, y: f32| f64::from(x) * f64::from(y);
    ((mul(o2.y, c[1]) + mul(o2.z, c[2])) + mul(o2.x, c[0])) * f64::from(SCALE)
}

/// The response combine with `gate` (its int argument) and the two floats at `f` and `e`. Returns the new `(f, e)`.
/// The `f` part runs only when `gate != 0`. Under NGP the sums take the dot's double and round once, at the store.
pub fn response_combine(input: &ResponseInput, gate: u32, f: f32, e: f32) -> (f32, f32) {
    let b = [input.b.x, input.b.y, input.b.z];
    match precision() {
        Precision::Pc24 => {
            let f = if gate != 0 { response_dot(input.a, input.c, b, input.inverse_inertia) + f } else { f };
            let dot = response_dot(input.a, input.b, b, input.inverse_inertia);
            (f, (dot + e) + input.inverse_mass)
        }
        Precision::Pc53 => {
            let f = if gate != 0 {
                (response_dot_pc53(input.a, input.c, b, input.inverse_inertia) + f64::from(f)) as f32
            } else {
                f
            };
            let dot = response_dot_pc53(input.a, input.b, b, input.inverse_inertia);
            (f, ((dot + f64::from(e)) + f64::from(input.inverse_mass)) as f32)
        }
    }
}
