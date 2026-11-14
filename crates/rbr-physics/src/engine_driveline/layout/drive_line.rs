//! The drive line table in memory: up to 10000 entries of 0x30 bytes from the table's address. Entry `k`'s fields
//! start at `this + 0x30k + 0x10`: the position (vec4, +0), the tangent (vec4, +0x10), the distance (float, +0x20)
//! and a dword tag (+0x24). The count is at `this + 0x75310`. Entry `k`'s distance is also the word at the next
//! entry's base, so `last` reads the float past the end.

use crate::engine_driveline::drive_line::{self, MAX_POINTS};
use crate::engine_driveline::store::EngineStore;
use crate::math::vec::Vec4;
use crate::stage::store::StageStore;

/// The count of entries at `this`.
pub const COUNT_AT: u32 = 0x0007_5310;
/// The table's capacity (`DRIVELINE_MAX_N_CTLPOINTS`).
pub const MAX_ENTRIES: u32 = MAX_POINTS;
/// The stride of one entry in the table (`0x30` bytes).
pub const STRIDE: u32 = 0x30;
/// The E12 host word that `Thunk4F0` loads (`ecx` for `E12_SampleX`).
const DISTANCE_HOST_AT: u32 = 0x007e_aba8;

/// The address of entry `k`'s fields, `this + 0x30k + 0x10`. The index wraps as the original's 32-bit one does.
pub fn entry_at(this: u32, k: u32) -> u32 {
    this.wrapping_add(k.wrapping_mul(STRIDE)).wrapping_add(0x10)
}

/// `CDriveLine::BlendB` (0x50a650): the point on the spline between entries `idx` and `idx + 1` at `t`.
pub fn spline_at<S: StageStore + ?Sized>(line: &S, this: u32, idx: i32, t: f32) -> Vec4 {
    let (k, next) = (idx as u32, (idx as u32).wrapping_add(1));
    let (p0, v0) = (line.line_position(this, k), line.line_tangent(this, k));
    let (p1, v1) = (line.line_position(this, next), line.line_tangent(this, next));
    drive_line::hermite(p0, v0, p1, v1, t)
}

/// `CDriveLine::Sample` (0x50b180): the point at the distance `d`. The first entry from 1 whose distance is above `d`
/// (`comiss`+`ja`: a NaN `d` finds none) gives the spline from the entry before it at `(d - d0) / (d1 - d0)`; with
/// none, the last entry's position (an empty table's reads `this - 0x20`).
pub fn sample<S: StageStore + ?Sized>(line: &S, this: u32, d: f32) -> Vec4 {
    let n = line.line_count(this);
    for k in 1..n {
        let d1 = line.line_distance(this, k);
        if d1 > d {
            let d0 = line.line_distance(this, k - 1);
            return spline_at(line, this, (k - 1) as i32, (d - d0) / (d1 - d0));
        }
    }
    line.line_position(this, n.wrapping_sub(1))
}

/// `CDriveLine::ClampMix` (0x50b0f0): the point at the position `x` along the entries, `BlendB` at the segment of `x` (see `drive_line::segment_of`). An index of
/// 0x7fffffff wraps to `this - 0x30`.
pub fn point_at<S: StageStore + ?Sized>(line: &S, this: u32, x: f32) -> Vec4 {
    let (idx, t) = drive_line::segment_of(x);
    spline_at(line, this, idx, t)
}

/// `CDriveLine::LerpAt` (0x50a920): the lerp of entry `idx`'s tangent `v` and entry `idx + 1`'s, at `t`, as the jump
/// into `E11_VecLerp` takes them. The count check is the caller's (the original traps past it).
pub fn tangent_lerp<S: StageStore + ?Sized>(line: &S, this: u32, idx: i32, t: f32) -> Vec4 {
    let k = idx as u32;
    line.line_tangent(this, k).lerp(line.line_tangent(this, k.wrapping_add(1)), t)
}

/// `CDriveLine::Last` (0x50ad40): the distance of the last entry, entry `count - 1`. The original traps on an empty
/// table.
pub fn last_distance<S: StageStore + ?Sized>(line: &S, this: u32) -> f32 {
    let n = line.line_count(this);
    assert!(n > 0, "CDriveLine::Last on an empty table (the original traps)");
    line.line_distance(this, n - 1)
}

/// `CDriveLine::Thunk4F0` (0x50a4f0): the distance at a position, forwarded: loads the E12 host word and tail-jumps to `E12_SampleX` with `id` as its float
/// argument. The E12 side is `distance_at(host, x)`.
pub fn forward_distance_at<S: EngineStore>(mem: &S, id: u32, distance_at: impl FnOnce(u32, f32) -> f32) -> f32 {
    distance_at(mem.u32(DISTANCE_HOST_AT), f32::from_bits(id))
}

/// `CDriveLine::SetDriveLine` (0x50aec0): copies the signal's `n` entries (count at +0, base at +4, each entry
/// at its own 0x30 stride) and stores them (`store_drive_line`).
pub fn set_drive_line<S: EngineStore>(mem: &mut S, this: u32, sig: u32) {
    let n = mem.u32(sig);
    assert!(n <= MAX_ENTRIES, "CDriveLine::SetDriveLine count {n} exceeds 10000 (the original traps)");
    let base = mem.u32(sig + 4);
    let entries: Vec<_> = (0..n).map(|k| mem.load_entry(base + k * STRIDE)).collect();
    mem.store_drive_line(this, &entries);
}
