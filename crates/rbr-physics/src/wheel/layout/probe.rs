//! `E51Wheel::M3D520` (0x53d520), the ground probe, and the wheel tuning NGP retunes.

use super::fields::{
    BLOCK_DIFF, BLOCK_TAG_1, BLOCK_TAG_2, BLOCK_WEIGHT, CARCASS_EDGE_MINUS, CARCASS_EDGE_PLUS, FRAME_AXIS, GROUND_BODY,
    PROBE_DIRECTION, RAY_CENTRE, RAY_DISTANCE, RAY_EDGE_MINUS, RAY_EDGE_PLUS, RAY_HIT, RAY_HIT_POINT, RAY_NORMAL,
    RAY_OFFSET, RAY_VALID, SECTOR_VALUE_B, SPEED_SCALE, SWEEP_RESIDUE, TYRE_SCRATCH, WORLD_AXLE, WORLD_HUB,
};
use super::store::WheelStore;
use crate::car_body::layout as car_layout;
use crate::math::vec::Vec4;
use crate::mem::Mem;
use crate::physics_core::layout::COLLISION_GLOBAL;
use crate::stage::layout::COLLISION_SYSTEM;
use crate::tyre;

/// The probe's scratch, as offsets from the body's frame `S` (its `esp` after the prologue, see `PROBE_PROLOGUE`): the
/// E46 output `rayWork` at `PROBE_RAYS` (the list `E45_M46A0` reads), the eight ray objects of `PROBE_RAY_SIZE` bytes
/// from `PROBE_RAY_BLOCKS` (the constructor and destructor iterators' array, 0x30 past the list), the hit buffer
/// `PROBE_HIT` (0x40 bytes: the hit's words, its distance at `PROBE_HIT_DIST`), the sweep's 16-byte output and its
/// distance, and the vectors the casts take by pointer.
pub const PROBE_FV: u32 = 0x60;
pub const PROBE_V1: u32 = 0x80;
pub const PROBE_V2: u32 = 0x70;
pub const PROBE_V3: u32 = 0x50;
pub const PROBE_HIT: u32 = 0x90;
pub const PROBE_HIT_DIST: u32 = 0x30;
pub const PROBE_SWEEP_OUT: u32 = 0xD0;
pub const PROBE_SWEEP_DIST: u32 = 0x40;
pub const PROBE_RAYS: u32 = 0xE0;
pub const PROBE_RAY_BLOCKS: u32 = 0x110;
pub const PROBE_RAY_SIZE: u32 = 0x6B0;
/// The bytes of the scratch the probe uses: up to the end of the eight rays.
pub const PROBE_SCRATCH: u32 = PROBE_RAY_BLOCKS + 8 * PROBE_RAY_SIZE;
/// The body's frame below the entry's aligned `ebp`: the SEH record and its try level (16 bytes), the `_chkstk`
/// allocation (0x3694) and the three saved registers (12 bytes). `S` is that `esp`; the callees run below it.
pub const PROBE_PROLOGUE: u32 = 0x36B0;

/// The word the probe copies into the minus edge block's offset (`*0x739594`).
const MINUS_EDGE_OFFSET: u32 = 0x0073_9594;
/// The copied block's dwords (`rep movsd` of `0x1c`).
const PROBE_BLOCK_WORDS: u32 = 0x1C;
/// The wheel's copies of the centre ray (its start `+0xdb0` and end `+0xdc0`), of the probe direction (`+0xe20`), of the
/// contact point (`+0xdd0`, when the centre ray hit) and of the edge rays' hit points (`+0xe10`, `+0xdf0`).
const CENTRE_RAY_START: u32 = 0xDB0;
const CENTRE_RAY_END: u32 = 0xDC0;
const PROBE_DIRECTION_COPY: u32 = 0xE20;
const CONTACT_POINT_COPY: u32 = 0xDD0;
const MINUS_HIT_POINT_COPY: u32 = 0xE10;
const PLUS_HIT_POINT_COPY: u32 = 0xDF0;
/// The best-hit limit (`2.0`), the test vector's fill (`0.4`), the centre ray's floor (`0.2`), the blend's average
/// (`0.5`), and the plus edge block's offset (`0.1`).
const PROBE_LIMIT: f32 = 2.0;
const PROBE_FILL: f32 = 0.4;
const PROBE_POSE: f32 = 0.2;
const HALF: f32 = 0.5;
const PLUS_EDGE_OFFSET: f32 = 0.1;

/// The wheel constants NGP makes per-car values (`10171300`, `10171304`, `10171308`; `notes/ngp.md`): the ground
/// probe's test vector fill, the table lerp's first weight, and the pose (the centre ray's floor, the lerps' base).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WheelTuning {
    pub fill: f32,
    pub weight: f32,
    pub pose: f32,
}

impl WheelTuning {
    /// 1.02's: 0.4 (`0x72f664`), 0.15 (`0x72d154`) and 0.2 (`0x72f428`, and the probe's push).
    pub const STOCK: Self = WheelTuning { fill: PROBE_FILL, weight: 0.15, pose: PROBE_POSE };
}

/// What NGP retunes in the ground probe: the collision system its casts run on (NGP repoints `g_7c755c` at its own
/// copy) and the wheel constants.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProbeTuning {
    pub collision: u32,
    pub wheel: WheelTuning,
}

impl ProbeTuning {
    /// 1.02's: the system at `g_7c755c`.
    pub fn stock(mem: &Mem) -> Self {
        ProbeTuning { collision: mem.u32(COLLISION_SYSTEM), wheel: WheelTuning::STOCK }
    }
}

/// The original callees the probe calls, each with the arguments the asm passes: the object `ecx` names first, then the
/// stack words in order. The callees with an `eax` return it.
pub trait GroundCalls {
    /// `TyreTail_Get14` (0x5326c0) on the tyre scratch: its state (`tyre::layout::STATE`).
    fn tyre_state(&mut self, scratch: u32) -> u32;
    /// `CWheel::Unk96e0` (0x5396e0) on the wheel (name ours).
    fn table_lerp_pair(&mut self, wheel: u32);
    /// `E39_SegTest170` (0x4ff170) on `object` with the segment's start and test vector, and a null output.
    fn query_box_faces(&mut self, object: u32, a: u32, b: u32);
    /// The eight rays' constructor iterator (`eh vector constructor iterator`, 0x70c408) over `blocks`.
    fn ray_blocks_init(&mut self, blocks: u32);
    /// The eight rays' destructor iterator (0x70c4da) over `blocks`.
    fn ray_blocks_fini(&mut self, blocks: u32);
    /// `E46_Kernel5bd0` (0x525bd0, stdcall, with ECX = the world, `physics_core::layout::detector_calls::kernel_collect`):
    /// the entities between `a` and `b`, mask 8, no exclusion, to `out`.
    fn collect_entities(&mut self, world: u32, a: u32, b: u32, out: u32);
    /// `E38Ops::M_D760X` (0x4fd760, `collision::mesh::mesh_ray_query`) on `object` with `(b, c, d, 0)`: its result.
    fn mesh_ray_query(&mut self, object: u32, b: u32, c: u32, d: u32) -> u32;
    /// `E45_M46A0` (0x5246a0) on the world with `(a, b, list, out)`: the sweep of the collected edges from `a` to `b`,
    /// its hit words to `out`; the hit object's pointer, or 0.
    fn cast_ray(&mut self, world: u32, a: u32, b: u32, list: u32, out: u32) -> u32;
    /// The hit object's virtual at `+0x154` (ECX = `hit`): its eax.
    fn hit_flag(&mut self, hit: u32) -> u32;
    /// `E51Wheel::M3D460` (0x53d460, [`update_ray_block`](super::update_ray_block)) on the wheel with `(block, limit,
    /// floor)`.
    fn update_ray_block(&mut self, wheel: u32, block: u32, limit: f32, floor: f32);
    /// `E38_004FAD90` (0x4fad90, stdcall, with ECX = `object`): `(a, b, c, d)`, its eax.
    fn segment_sweep(&mut self, object: u32, a: u32, b: u32, c: u32, d: u32) -> u32;
    /// `CWheel::Unk9290` (0x539290, [`pick_contact`](super::pick_contact)) on the wheel with `(a, b, out)`.
    fn pick_contact(&mut self, wheel: u32, a: u32, b: u32, out: u32);
    /// `CWheel::Unk9900` (0x539900) on the wheel with `(x, y, z)`.
    fn blend_tags(&mut self, wheel: u32, x: u32, y: u32, z: f32);
    /// `NTweak::CFloat3::LoadBinary` (0x4f3d20) on `to` with the data at `data`.
    fn load_binary(&mut self, to: u32, data: u32);
}

/// `E01Vec4::CopyFrom` (0x43c5b0): the 16 bytes at `src` go to `dst`.
fn copy_vec<S: WheelStore>(mem: &mut S, dst: u32, src: u32) {
    let v = Vec4::load(mem, src);
    v.store(mem, dst);
}

/// `count` words from `src` to `dst`, in order (the `rep movsd` copies).
fn copy_words<S: WheelStore>(mem: &mut S, dst: u32, src: u32, count: u32) {
    for k in 0..count {
        let word = mem.u32(src + 4 * k);
        mem.set_u32(dst + 4 * k, word);
    }
}

/// `a * s + b` in each lane (the asm's `mulps` then `addps`).
fn scaled_plus(a: Vec4, s: f32, b: Vec4) -> Vec4 {
    Vec4 { x: a.x * s + b.x, y: a.y * s + b.y, z: a.z * s + b.z, w: a.w * s + b.w }
}

/// The miss test of a ray cast, in the asm's order: no hit; the hit's `+0x154` says so; or the block already holds a
/// hit whose distance is not above the new one. The compare is false for a NaN, so a NaN distance misses.
fn ray_misses<S: WheelStore>(mem: &S, calls: &mut impl GroundCalls, block: u32, buf: u32, obj: u32) -> bool {
    obj == 0
        || calls.hit_flag(obj) != 0
        || (mem.u32(block + RAY_HIT) != 0 && !(mem.f32(block + RAY_DISTANCE) > mem.f32(buf + PROBE_HIT_DIST)))
}

/// Whether a hit takes the best so far: its flag byte has bit 4, and its distance is below `limit`.
fn hit_takes<S: WheelStore>(mem: &S, obj: u32, dist: f32, limit: f32) -> bool {
    mem.u8(obj + 0x60) & 4 != 0 && dist < limit
}

/// `E51Wheel::M3D520` (0x53d520, thiscall, `ret 8`): the ground probe. Three ray casts along the probe direction (from
/// the carcass edges `+0xa20` and `+0xa40` and the world hub `+0xa30`), each into its block (`+0xc90`, `+0xd00`,
/// `+0xc20`) with `M3D460`, then the blend of the hits into `out` (the contact point) and `normal` (the contact normal).
/// Returns 1 when the blend runs, 0 when no ray found a block to blend (the early return). `frame` is the body's `S`
/// (the scratch is `PROBE_SCRATCH` bytes above it); `normal` is the second stack argument. `tuning` is
/// [`ProbeTuning::stock`] for 1.02.
pub fn ground_probe<S: WheelStore>(
    mem: &mut S,
    calls: &mut impl GroundCalls,
    this: u32,
    frame: u32,
    out: u32,
    normal: u32,
    tuning: ProbeTuning,
) -> u32 {
    let o = this;
    let hub = o + WORLD_HUB;
    let dir = o + PROBE_DIRECTION;
    let minus = o + RAY_EDGE_MINUS;
    let centre = o + RAY_CENTRE;
    let plus = o + RAY_EDGE_PLUS;
    let list = frame + PROBE_RAYS;
    let blocks = frame + PROBE_RAY_BLOCKS;
    let buf = frame + PROBE_HIT;
    let world = mem.u32(car_layout::WORLD);
    let object = tuning.collision;
    let sweep = mem.u32(COLLISION_GLOBAL);

    let radius = mem.f32(calls.tyre_state(o + TYRE_SCRATCH) + tyre::layout::STATE_EFFECTIVE_RADIUS);
    let offset = mem.u32(MINUS_EDGE_OFFSET);
    mem.set_u32(minus + RAY_OFFSET, offset);
    mem.set_f32(centre + RAY_OFFSET, 0.0);
    mem.set_f32(plus + RAY_OFFSET, PLUS_EDGE_OFFSET);
    mem.set_u32(o + GROUND_BODY, 0);
    let mut best = PROBE_LIMIT;
    calls.table_lerp_pair(o);
    let fv = frame + PROBE_FV;
    for k in 0..3 {
        mem.set_f32(fv + 4 * k, tuning.wheel.fill);
    }
    calls.query_box_faces(object, hub, fv);
    calls.ray_blocks_init(blocks);
    calls.collect_entities(world, hub, fv, list);

    // Ray 1: from the minus edge, its test vector is the probe direction plus the start; the hit goes to its block.
    let start = o + CARCASS_EDGE_MINUS;
    let v1 = frame + PROBE_V1;
    (scaled_plus(Vec4::load(mem, dir), 1.0, Vec4::load(mem, start))).store(mem, v1);
    mem.set_u32(minus + RAY_HIT, calls.mesh_ray_query(object, start, v1, minus + RAY_HIT_POINT));
    let obj = calls.cast_ray(world, start, v1, list, buf);
    if !ray_misses(mem, calls, minus, buf, obj) {
        mem.set_u32(minus + RAY_HIT, 1);
        copy_words(mem, minus + RAY_HIT_POINT, buf, 16);
        let dist = mem.f32(buf + PROBE_HIT_DIST);
        if hit_takes(mem, obj, dist, PROBE_LIMIT) {
            mem.set_u32(o + GROUND_BODY, obj);
            best = dist;
        }
        calls.update_ray_block(o, minus, radius, mem.f32(o + SECTOR_VALUE_B));
    } else if mem.u32(minus + RAY_HIT) != 0 {
        calls.update_ray_block(o, minus, radius, mem.f32(o + SECTOR_VALUE_B));
    } else {
        mem.set_u32(minus + RAY_VALID, 0);
    }

    // Ray 2: from the plus edge (the same test vector form).
    let start = o + CARCASS_EDGE_PLUS;
    let v2 = frame + PROBE_V2;
    (scaled_plus(Vec4::load(mem, dir), 1.0, Vec4::load(mem, start))).store(mem, v2);
    mem.set_u32(plus + RAY_HIT, calls.mesh_ray_query(object, start, v2, plus + RAY_HIT_POINT));
    let obj = calls.cast_ray(world, start, v2, list, buf);
    if !ray_misses(mem, calls, plus, buf, obj) {
        mem.set_u32(plus + RAY_HIT, 1);
        copy_words(mem, plus + RAY_HIT_POINT, buf, 16);
        let dist = mem.f32(buf + PROBE_HIT_DIST);
        if hit_takes(mem, obj, dist, best) {
            mem.set_u32(o + GROUND_BODY, obj);
            best = dist;
        }
        calls.update_ray_block(o, plus, radius, mem.f32(o + SECTOR_VALUE_B));
    } else if mem.u32(plus + RAY_HIT) != 0 {
        calls.update_ray_block(o, plus, radius, mem.f32(o + SECTOR_VALUE_B));
    } else {
        mem.set_u32(plus + RAY_VALID, 0);
    }

    // Ray 3: from the world hub; the hit goes to the centre block.
    let v3 = frame + PROBE_V3;
    (scaled_plus(Vec4::load(mem, dir), 1.0, Vec4::load(mem, hub))).store(mem, v3);
    copy_vec(mem, o + CENTRE_RAY_START, hub);
    copy_vec(mem, o + CENTRE_RAY_END, v3);
    copy_vec(mem, o + PROBE_DIRECTION_COPY, dir);
    mem.set_u32(centre + RAY_HIT, calls.mesh_ray_query(object, hub, v3, centre + RAY_HIT_POINT));
    let obj = calls.cast_ray(world, hub, v3, list, buf);
    let miss = ray_misses(mem, calls, centre, buf, obj);
    if !miss {
        mem.set_u32(centre + RAY_HIT, 1);
        copy_words(mem, centre + RAY_HIT_POINT, buf, 16);
        if hit_takes(mem, obj, mem.f32(buf + PROBE_HIT_DIST), best) {
            mem.set_u32(o + GROUND_BODY, obj);
        }
    } else if mem.u32(centre + RAY_HIT) == 0 {
        mem.set_u32(centre + RAY_VALID, 0);
        mem.set_f32(o + SWEEP_RESIDUE, 0.0);
    }
    if !miss || mem.u32(centre + RAY_HIT) != 0 {
        // The sweep path: the centre block's update with the pose floor, then the sweep from the hub along the probe.
        calls.update_ray_block(o, centre, radius, tuning.wheel.pose);
        let hv = calls.segment_sweep(sweep, hub, v3, frame + PROBE_SWEEP_OUT, frame + PROBE_SWEEP_DIST);
        let dist = mem.f32(frame + PROBE_SWEEP_DIST);
        let residue = if hv != 0 && radius > dist { radius - dist } else { 0.0 };
        mem.set_f32(o + SWEEP_RESIDUE, residue);
    }

    // The blend. Without a block the probe clears its blocks and returns 0.
    let edge_valid = mem.u32(minus + RAY_VALID) != 0 || mem.u32(plus + RAY_VALID) != 0;
    if mem.u32(centre + RAY_VALID) == 0 && !edge_valid {
        calls.ray_blocks_fini(blocks);
        return 0;
    }
    // The asm stores the cross's three lanes only; lane 3 (+0xA9C) keeps its old value, and the stack copies read it.
    let cross = Vec4::load(mem, o + WORLD_AXLE).cross3(Vec4::load(mem, dir));
    (crate::math::vec::Vec3 { x: cross.x, y: cross.y, z: cross.z }).store(mem, o + FRAME_AXIS);
    let distance = if mem.u32(minus + RAY_VALID) != 0 && mem.u32(plus + RAY_VALID) != 0 {
        let average = (mem.f32(plus + BLOCK_DIFF) + mem.f32(minus + BLOCK_DIFF)) * HALF;
        if mem.u32(centre + RAY_VALID) == 0 || !(average > mem.f32(centre + BLOCK_DIFF)) {
            calls.pick_contact(o, minus, plus, normal);
            average
        } else {
            calls.load_binary(normal, centre + RAY_NORMAL);
            mem.f32(centre + BLOCK_DIFF)
        }
    } else {
        if mem.u32(minus + RAY_VALID) == 0 {
            copy_words(mem, minus, plus, PROBE_BLOCK_WORDS);
        }
        if mem.u32(centre + RAY_VALID) == 0 {
            mem.set_f32(centre + BLOCK_DIFF, radius);
        }
        let centre_diff = mem.f32(centre + BLOCK_DIFF);
        let edge_diff = mem.f32(minus + BLOCK_DIFF);
        if !edge_valid {
            calls.load_binary(normal, centre + RAY_NORMAL);
            centre_diff
        } else if mem.u32(centre + RAY_VALID) == 0 {
            calls.load_binary(normal, minus + RAY_NORMAL);
            (centre_diff + edge_diff) * HALF
        } else if centre_diff > edge_diff {
            calls.pick_contact(o, centre, minus, normal);
            (centre_diff + edge_diff) * HALF
        } else {
            calls.load_binary(normal, centre + RAY_NORMAL);
            centre_diff
        }
    };
    mem.set_f32(o + SPEED_SCALE, distance);
    (scaled_plus(Vec4::load(mem, dir), distance, Vec4::load(mem, hub))).store(mem, out);
    if mem.u32(centre + RAY_HIT) == 0 {
        let source = if mem.u32(minus + RAY_HIT) != 0 { minus } else { plus };
        copy_words(mem, centre, source, PROBE_BLOCK_WORDS);
    }
    calls.blend_tags(o, mem.u32(centre + BLOCK_TAG_1), mem.u32(centre + BLOCK_TAG_2), mem.f32(centre + BLOCK_WEIGHT));
    if mem.u32(centre + RAY_HIT) != 0 {
        copy_vec(mem, o + CONTACT_POINT_COPY, out);
    }
    if mem.u32(minus + RAY_HIT) != 0 {
        copy_vec(mem, o + MINUS_HIT_POINT_COPY, minus + RAY_HIT_POINT);
    }
    if mem.u32(plus + RAY_HIT) != 0 {
        copy_vec(mem, o + PLUS_HIT_POINT_COPY, plus + RAY_HIT_POINT);
    }
    calls.ray_blocks_fini(blocks);
    1
}
