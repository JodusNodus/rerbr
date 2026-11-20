//! The drive line table in memory: 10000 entries of 0x30 bytes from the table's address. Entry `k`'s fields
//! start at `this + 0x30k + 0x10`: the position (vec4, +0), the tangent (vec4, +0x10), the distance (float, +0x20)
//! and a dword tag (+0x24). The count is at `this + 0x75310`. Entry `k`'s distance is also the word at the next
//! entry's base, so `last` reads the float past the end.
//!
//! Ours: a longer line (a converted stage over about 25 km at 2.5 m) carries on after the count, entry 10000's fields
//! at `this + 0x75320` ([`entry_at`]), in a block the boot sizes for it ([`block_size`]). The original traps on such a
//! count; a table of 10000 entries or fewer is laid out, filled and read as the original's.

use crate::engine_driveline::drive_line::{self, MAX_POINTS};
use crate::engine_driveline::store::EngineStore;
use crate::math::vec::Vec4;
use crate::stage::store::StageStore;

/// The count of entries at `this`.
pub const COUNT_AT: u32 = 0x0007_5310;
/// The original table's capacity (`DRIVELINE_MAX_N_CTLPOINTS`).
pub const MAX_ENTRIES: u32 = MAX_POINTS;
/// The stride of one entry in the table (`0x30` bytes).
pub const STRIDE: u32 = 0x30;
/// Ours: the entries from [`MAX_ENTRIES`] sit this far past the original's place, after the count's 16 bytes.
const OVERFLOW_SHIFT: u32 = 0x10;
/// Ours: the indices below this are a long line's; larger ones (a wrapped negative index) address as the original's.
const OVERFLOW_END: u32 = 1 << 20;
/// The block `CreateDriveLine` allocates (the table, the count and the words past it).
const BLOCK_SIZE: u32 = 0x75330;
/// The E12 host word that `Thunk4F0` loads (`ecx` for `E12_SampleX`).
const DISTANCE_HOST_AT: u32 = 0x007e_aba8;

/// The address of entry `k`'s fields, `this + 0x30k + 0x10`, and from [`MAX_ENTRIES`] past the count (ours). The index
/// wraps as the original's 32-bit one does.
pub fn entry_at(this: u32, k: u32) -> u32 {
    let shift = if (MAX_ENTRIES..OVERFLOW_END).contains(&k) { OVERFLOW_SHIFT } else { 0 };
    this.wrapping_add(k.wrapping_mul(STRIDE)).wrapping_add(0x10 + shift)
}

/// The entries a table of `count` holds: the original's 10000, or for a longer line (ours) its own and one past them,
/// which `store_drive_line` fills as the original fills those past its count.
pub fn entries_held(count: u32) -> u32 {
    if count <= MAX_ENTRIES {
        MAX_ENTRIES
    } else {
        count + 1
    }
}

/// The bytes of the block for a table of `count` entries: the original's, or for a longer line (ours) to its last
/// held entry and the distance past it that `last` may read.
pub fn block_size(count: u32) -> u32 {
    if count <= MAX_ENTRIES {
        BLOCK_SIZE
    } else {
        entry_at(0, entries_held(count)) + 0x20
    }
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
    let base = mem.u32(sig + 4);
    let entries: Vec<_> = (0..n).map(|k| mem.load_entry(base + k * STRIDE)).collect();
    mem.store_drive_line(this, &entries);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_table_of_10000_entries_or_fewer_is_the_original_s() {
        assert_eq!(entry_at(0x100, 0), 0x110);
        assert_eq!(entry_at(0x100, MAX_ENTRIES - 1), 0x100 + COUNT_AT - STRIDE);
        // Wrapped negative indices address as the original's 32-bit index does.
        assert_eq!(entry_at(0x100, u32::MAX), 0x100 - 0x20);
        assert_eq!(entry_at(0x100, 0x7fff_ffff), 0x100u32.wrapping_add(0x7fff_ffffu32.wrapping_mul(STRIDE)) + 0x10);
        assert_eq!(entries_held(0), MAX_ENTRIES);
        assert_eq!(entries_held(MAX_ENTRIES), MAX_ENTRIES);
        assert_eq!(block_size(1711), 0x75330);
        assert_eq!(block_size(MAX_ENTRIES), 0x75330);
    }

    #[test]
    fn a_longer_line_carries_on_after_the_count() {
        assert_eq!(entry_at(0, MAX_ENTRIES), COUNT_AT + 0x10);
        assert_eq!(entry_at(0, MAX_ENTRIES + 1), COUNT_AT + 0x10 + STRIDE);
        assert_eq!(entries_held(MAX_ENTRIES + 1), MAX_ENTRIES + 2);
        assert_eq!(block_size(MAX_ENTRIES + 1), COUNT_AT + 0x10 + 2 * STRIDE + 0x20);
    }
}
