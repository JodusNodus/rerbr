//! The brake walls (`E34WallVf`, `CBrakeWall`, `CBrakeWallManager`): the wall layout's constants and `VfF0`. The probes
//! are `collision::wall`, generic over `CollisionStore`.
//!
//! `brake_wall_contact` stays on `Mem`: it clears the sub-block through `car_body::light::sub_block_init`.

use crate::mem::Mem;

use super::vec_ops::vec4_copy_from;
use super::wall_tree::WallSegmentHost;
use crate::math::vec::Vec4;

pub use crate::collision::wall::{query_wall_box, wall_probe, wall_raycast, wall_segment_hit};

/// The wall's fields that `E34WallVf::Raycast` reads and writes: the latch word (+0x214) and the segment vertex table
/// (+0x218).
pub(super) const WALL_LATCH: u32 = 0x214;
pub(super) const WALL_MESH: u32 = 0x218;
/// The leaf's count (`seg[4]`) and index table (`seg[5]`); the query's list count is the word at 0x190 (`hits[100]`).
pub(super) const SEG_COUNT: u32 = 0x10;
pub(super) const SEG_INDEX: u32 = 0x14;
pub(super) const HITS_COUNT: u32 = 0x190;
/// The signal manager (global 0x893600) and the penetration emitter's offset in it (0xa780).
pub(super) const PENETRATION_EMITTER: u32 = 0xa780;
/// The wall's leaf list (+0x7c) that `CBrakeWall::QueryBox` fills (its count is the list's word at +0x190).
pub(super) const WALL_LEAVES: u32 = 0x7c;

/// `CBrakeWall::VfF0` (0x4e3970) (name ours): the wall's segment contact, on the wall at `this` with its eight pointer
/// arguments `a`..`h`; `entry` is its entry
/// esp. In the original's `and esp, -0x10` frame `CE36SubBlock::Init` clears the block at +0x2c, then `VfE8` fills the
/// four vectors at +0xc. On a hit: `CopyFrom` of the first vector to `c`, the frame word at +8 points at the second,
/// which goes to `e`; the fourth's `y` goes to `d`, and `f`, `g` and `h` get 0. Returns 1 on a hit, else 0 (eax).
pub fn brake_wall_contact(
    host: &mut impl WallSegmentHost,
    mem: &mut Mem,
    this: u32,
    args: [u32; 8],
    entry: u32,
) -> u32 {
    let [a, b, c, d, e, f, g, h] = args;
    let frame = ((entry - 4) & !0xf) - 0x4c;
    let out = frame + 0xc;
    crate::car_body::light::sub_block_init(mem, frame + 0x2c);
    if host.segment_hit(frame - 0x1c, this, a, b, out) == 0 {
        return 0;
    }
    vec4_copy_from(mem, c, out);
    mem.set_u32(frame + 8, out + 0x10);
    Vec4::load(mem, out + 0x10).store(mem, e);
    let y = mem.u32(out + 0x34);
    mem.set_u32(d, y);
    for at in [f, g, h] {
        mem.set_u32(at, 0);
    }
    1
}

/// `CBrakeWall::QueryBox`'s frame (`query_wall_box`, entered at `sp`): the fetched corner and the queried box, and the
/// entry stack pointers of the frame lookup, the fetches and the box query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WallBoxFrame {
    pub corner: u32,
    pub bounds: u32,
    pub frame_sp: u32,
    pub fetch_sp: u32,
    pub query_sp: u32,
}

pub fn wall_box_frame(sp: u32) -> WallBoxFrame {
    let base = super::aligned(sp) - 0x30;
    WallBoxFrame {
        corner: base + 0x20,
        bounds: base + 0x10,
        frame_sp: base - 4,
        fetch_sp: base - 0xc,
        query_sp: base - 0xc,
    }
}

/// The segment test's frame (`wall_segment_hit`, entered at `sp`): the two fetched ends, and the fetches' entry stack
/// pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WallEndsFrame {
    pub first: u32,
    pub second: u32,
    pub fetch_sp: u32,
}

pub fn wall_ends_frame(sp: u32) -> WallEndsFrame {
    let base = super::aligned(sp) - 0x80;
    WallEndsFrame { first: base + 0x60, second: base + 0x70, fetch_sp: base - 0xc }
}

/// The raycast's leaf list in the probe's caller's frame, below the probe's point buffer `tmp`.
pub fn probe_hits(tmp: u32) -> u32 {
    tmp - 0x1a0
}
