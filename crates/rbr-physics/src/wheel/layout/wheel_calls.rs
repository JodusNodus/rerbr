//! The wheel's axis, lerp, setter, tag and transform calls (`M3A8A0`-`M3D460`, `Copy100`, `Blend4`, `Unk9290`,
//! `Unk94d0`, `Unk97a0`) and the slot forwards.

use super::fields::{
    AXLE, BLOCK_CLAMP_A, BLOCK_CLAMP_B, BLOCK_DIFF, BLOCK_TAG_1, BLOCK_TAG_2, BLOCK_TOTAL, BLOCK_WEIGHT,
    CARCASS_EDGE_MINUS, CARCASS_EDGE_MINUS_COPY, CARCASS_EDGE_PLUS, CARCASS_EDGE_PLUS_COPY, CAR_FLAG_XFORM, CAR_FORCE,
    CAR_SCRATCH, CAR_SCRATCH_WORD, DEF_FLAGS, DEF_PAIR_TAG, EDGE_ACROSS, FLAG_PAIR_TAG, FRAME_AXIS, HUB_POSITION,
    HUB_VELOCITY, INV_ARM, INV_RACK, ORIENTATION, PROBE_DIRECTION, RAY_DISTANCE, RAY_EDGE_MINUS, RAY_EDGE_PLUS,
    RAY_NORMAL, RAY_VALID, SPIN_RATE, STRUT, STRUT_DIR, STRUT_INNER_BUMP_POS, STRUT_LENGTH, STRUT_OUTER_BUMP_POS,
    STRUT_TWEAK, WALK_OUT, WORLD_AXLE, XFORM_CAR, XFORM_PAIRS,
};
use super::host::{transform_call, CarSlotCalls, CarXform, SlotCalls, StrutCalls, SubCalls, WalkCalls};
use super::store::WheelStore;
use crate::car_body::store::CarStore;
use crate::car_body::transform::FRAME;
use crate::math::vec::Vec4;
use crate::ngp;
use crate::rigid_body::layout::{ANGULAR_VELOCITY, LINEAR_VELOCITY};
use crate::wheel::axes::{self, Trig};
use crate::wheel::axis_force::{self, AxisForceIn};
use crate::wheel::{contact, hub, ray, strut, walk};

/// `M3D460` (0x53d460) on the ray block `blk`: when the CDef's flag bit is set, the block takes the CDef's pair tag for
/// both tags and a zero weight (written before `M3D230`). Then the blend runs, NGP's (`ngp::layout::tag_blend`, its call
/// at `0x53d49d`; 1.02's `M3D230` is not run), and the difference `distance - total` is read (after the call) and stored
/// at `+0x64`, limited by `ray::limit_ray` with `limit` and `floor`; its valid word and limit words follow.
pub fn update_ray_block<S: WheelStore>(mem: &mut S, blk: u32, limit: f32, floor: f32) {
    let def = mem.world_def();
    if mem.u32(def + DEF_FLAGS) & FLAG_PAIR_TAG != 0 {
        let tag = mem.u32(def + DEF_PAIR_TAG);
        mem.set_u32(blk + BLOCK_TAG_1, tag);
        mem.set_u32(blk + BLOCK_TAG_2, tag);
        mem.set_f32(blk + BLOCK_WEIGHT, 0.0);
    }
    ngp::layout::tag_blend::update(mem.mem_mut(), blk);
    let d = mem.f32(blk + RAY_DISTANCE) - mem.f32(blk + BLOCK_TOTAL);
    let out = ray::limit_ray(d, limit, floor);
    mem.set_f32(blk + BLOCK_DIFF, out.difference);
    mem.set_u32(blk + RAY_VALID, u32::from(out.valid));
    if out.reset_limits {
        mem.set_f32(blk + BLOCK_CLAMP_A, 0.0);
        mem.set_f32(blk + BLOCK_CLAMP_B, 0.0);
    }
}

// The edge functions: each runs one original function in its order. They read the inputs and store the results between
// the host calls, as the asm does; the ports they call are pure. The tests and the hybrid binding call these.

/// `Copy100` (0x5390d0): `x` is the car transform (`CCar::Xform6870`, `car_body::transform::compose_with_body`, on the
/// car) of the tyre's centre, then the orientation is copied to `out`. The asm's test of `this+0xf0` (its address, never
/// zero) always takes `this+0x100`.
pub fn transform_and_copy<S: WheelStore>(
    mem: &mut S,
    this: u32,
    x_at: u32,
    out_at: u32,
    car_transform: impl FnOnce(u32, Vec4) -> Vec4,
) {
    let tmp = hub::tyre_centre(mem.load_at(this, AXLE), mem.load_hub_offset(this), mem.load_at(this, HUB_POSITION));
    let x = car_transform(mem.load_car(this), tmp);
    x.store(mem, x_at);
    mem.load_at(this, ORIENTATION).store(mem, out_at);
}

/// `Blend4` (0x539180): the carcass edges, `b` moved along the axle `a` by plus (`c`) and minus (`d`) half the tyre's
/// `CarcassWidth`. `c` is stored before `a` and `b` are read again for `d`, as the asm does, so an overlap of `c` with an
/// operand gives the asm's `d`.
pub fn carcass_edges<S: WheelStore>(mem: &mut S, this: u32, a_at: u32, b_at: u32, c_at: u32, d_at: u32) {
    let k = mem.load_carcass_width(this) * 0.5;
    let c = hub::along_axle(Vec4::load(mem, a_at), k, Vec4::load(mem, b_at));
    c.store(mem, c_at);
    let d = hub::along_axle(Vec4::load(mem, a_at), 0.0 - k, Vec4::load(mem, b_at));
    d.store(mem, d_at);
    mem.store_at(this, CARCASS_EDGE_MINUS, d);
    mem.store_at(this, CARCASS_EDGE_PLUS, c);
    mem.store_at(this, CARCASS_EDGE_PLUS_COPY, c);
    mem.store_at(this, CARCASS_EDGE_MINUS_COPY, d);
}

/// `Unk9290` (0x539290): the contact normal of the ray blocks `a_at` and `b_at`. `out` is `m * axis`, then `+ world_axle *
/// second` and `+ probe_direction * first`. Each step is stored before the next reads `out`, which may overlap the
/// vectors.
pub fn pick_contact<S: WheelStore>(mem: &mut S, this: u32, a_at: u32, b_at: u32, out_at: u32) {
    let axis = mem.load_at(this, FRAME_AXIS);
    let mix = contact::mix_contacts(axis, &mem.load_ray_block(a_at), &mem.load_ray_block(b_at));
    axis.scale(mix.axis_scale).store(mem, out_at);
    let v = mem.load_at(this, WORLD_AXLE).scale(mix.second) + Vec4::load(mem, out_at);
    v.store(mem, out_at);
    let v = mem.load_at(this, PROBE_DIRECTION).scale(mix.first) + Vec4::load(mem, out_at);
    v.store(mem, out_at);
}

/// `M3A8A0` (0x53a8a0): the car's eight transforms into the CDef's axes. The car is `this+0x44` when that is set and its
/// flags carry bit 3, else 0 (the calls still go to it). Each pair's source is read just before its call, after the calls
/// before it, and its result is stored after the call.
pub fn car_frames<S: WheelStore>(host: &mut impl CarXform, mem: &mut S, this: u32) {
    let obj = mem.load_xform_car(this);
    let car = if obj != 0 && mem.car_flags(obj) & CAR_FLAG_XFORM != 0 { obj } else { 0 };
    for (i, &(guard, src, out)) in XFORM_PAIRS.iter().enumerate() {
        let def = mem.load_def(this);
        let a = (def.wrapping_add(guard) != 0).then(|| Vec4::load(mem, def + src));
        let v = transform_call(host, i, car, a);
        v.store(mem, def + out);
    }
}

/// `M3AEF0` (0x53aef0): the walk `MB560` on the world frame (its `c` is the walk output), then `strut_dir * gain + walk`
/// stored to the walk output. `GetB1C` is read only after the walk call, when `|inv_rack| > 0.001` (as the asm does).
pub fn steering_walk<S: WheelStore + CarStore>(host: &mut impl WalkCalls, mem: &mut S, this: u32) -> Vec4 {
    let car = mem.load_car(this);
    let a = (car.wrapping_add(0x180) != 0).then(|| Vec4::load(mem, car + 0x190));
    let walk = host.pair_walk(mem.u32(FRAME), a, car);
    let gain =
        walk::steer_gain(mem.load_f32(this, INV_ARM), mem.load_f32(this, INV_RACK), || mem.steer_target_rate(car));
    let v = walk::steered_walk(mem.load_at(this, STRUT_DIR), gain, walk);
    mem.store_at(this, WALK_OUT, v);
    v
}

/// `M3AFB0` (0x53afb0): runs `steering_walk`, then writes the spin's cross product to the caller's `p`.
pub fn spin_cross<S: WheelStore + CarStore>(host: &mut impl WalkCalls, mem: &mut S, this: u32, p: u32) {
    let walk = steering_walk(host, mem, this);
    let cross = walk::spin_cross(walk, mem.load_at(this, AXLE), mem.load_f32(this, SPIN_RATE));
    cross.store(mem, p);
}

/// `M3AB90` (0x53ab90): the deformed top mount, stored to `+0x570`.
pub fn deform_top_mount<S: WheelStore>(mem: &mut S, this: u32) {
    let v = axes::deformed_top_mount(&mem.load_top_mount_deform(this));
    mem.store_deformed_top_mount(this, v);
}

/// `M3A9E0` (0x53a9e0): the wobbled axle, stored to `+0x5f0`.
pub fn wobble_axle<S: WheelStore>(trig: &mut impl Trig, mem: &mut S, this: u32) {
    let v = axes::wobbled_axle(trig, &mem.load_axle_wobble(this));
    mem.store_wobbled_axle(this, v);
}

/// `M3AC50` (0x53ac50): the torque on the hub. The hub damage and the axle are stored when `torque` is above the CDef's
/// threshold.
pub fn apply_hub_torque<S: WheelStore>(trig: &mut impl Trig, mem: &mut S, this: u32, torque: f32) {
    let update = axes::apply_hub_torque(trig, &mem.load_hub_damage(this, torque));
    mem.store_hub_damage(this, update.as_ref());
}

/// `M3D030` (0x53d030): the car frames, the deformed top mount and the inclined axle, then the strut's two names
/// (`GetName`, each read after its call), `SetDirty`, the rest travel (stored before `M3B900`, which reads it), `M3B900`
/// and `UpdateAndReset`.
pub fn update_strut<S: WheelStore>(
    host: &mut (impl CarXform + StrutCalls),
    trig: &mut impl Trig,
    mem: &mut S,
    this: u32,
) {
    car_frames(host, mem, this);
    let deform = mem.load_top_mount_deform(this);
    mem.store_deformed_top_mount(this, axes::deformed_top_mount(&deform));
    let inclination = mem.load_inclination(this);
    mem.store_inclined_axle(this, &axes::inclined_axle(trig, &inclination));
    let strut = this + STRUT;
    let name = host.strut_name(strut);
    let bump_stop = mem.f32(name + STRUT_INNER_BUMP_POS);
    let name = host.strut_name(strut);
    let droop_stop = mem.f32(name + STRUT_OUTER_BUMP_POS);
    let travel = strut::rest_travel(bump_stop, droop_stop);
    host.mark_dirty(this + STRUT_TWEAK);
    mem.set_f32(this + STRUT_LENGTH, travel);
    host.refresh(this, travel);
    host.update_and_reset(strut);
}

/// `CWheel::E43Vf20` (0x539090, vtable slot 8): clears the wheel's linear (`+0x1c0`) and angular (`+0x190`) velocity,
/// calls the wheel's `Vf40` (`refresh_solid`, 0x53b5b0), then forwards `p` to `CSolid::E43Vf20` (`walk_children`,
/// 0x529200).
pub fn clear_and_forward<S: WheelStore>(host: &mut impl SlotCalls, mem: &mut S, this: u32, p: u32) {
    mem.store_at(this, LINEAR_VELOCITY, Vec4::default());
    mem.store_at(this, ANGULAR_VELOCITY, Vec4::default());
    host.refresh_solid(this);
    host.walk_children(this, p);
}

/// `CWheel::E43Vf5C` (0x539a90, vtable slot 23): the car's slot 28 (its force) with `this+0x9f0`, then its slot 29 (its
/// scratch) with `this+0xa00`. The asm reads the car again before the second call. Then both vectors are cleared and
/// `+0xa10` is zeroed.
pub fn forward_to_car_and_clear<S: WheelStore>(host: &mut impl CarSlotCalls, mem: &mut S, this: u32) {
    let car = mem.u32(this + XFORM_CAR);
    host.accumulate_force(car, this + CAR_FORCE);
    let car = mem.u32(this + XFORM_CAR);
    host.accumulate_scratch(car, this + CAR_SCRATCH);
    mem.store_at(this, CAR_FORCE, Vec4::default());
    mem.store_at(this, CAR_SCRATCH, Vec4::default());
    mem.store_u32(this, CAR_SCRATCH_WORD, 0);
}

/// Where `Unk94d0` reads its remainder's lane 3: this far below its aligned `ebp` (`(sp - 4) & !0xf`).
const EDGE_RESIDUE: u32 = 0x14;

/// `CWheel::Unk94d0` (0x5394d0, thiscall on the wheel; stack `block`, `v`, `acc` and two words it does not read; `ret
/// 0x14`) entered at `sp`: `axis_force::axis_force` of the velocity at `v` on the edge ray block `block` (its normal and
/// its `+0x5c` scale), into the accumulator at `acc` and `this+0xe40` (name ours). The frame's temporaries are not
/// written. Returns eax, the copy's destination.
pub fn edge_force<S: WheelStore>(mem: &mut S, this: u32, block: u32, v: u32, acc: u32, sp: u32) -> u32 {
    let input = AxisForceIn {
        axis: mem.load_at(block, RAY_NORMAL),
        scale: mem.f32(block + BLOCK_CLAMP_A),
        probe_direction: mem.load_at(this, PROBE_DIRECTION),
        velocity: mem.load_at(v, 0),
        force: mem.load_at(acc, 0),
        residue: mem.f32(((sp - 4) & !0xF) - EDGE_RESIDUE),
    };
    let out = axis_force::axis_force(&input);
    mem.store_at(acc, 0, out.force);
    mem.store_at(this, EDGE_ACROSS, out.across);
    this + EDGE_ACROSS
}

/// Whether an edge ray block is gated on: its valid word is nonzero and its `+0x5c` scale is not zero (a NaN scale counts
/// as nonzero, as the asm's `ucomiss` pair gives).
fn gated_on<S: WheelStore>(mem: &S, block: u32) -> bool {
    mem.u32(block + RAY_VALID) != 0 && mem.f32(block + BLOCK_CLAMP_A) != 0.0
}

/// `CWheel::Unk97a0` (0x5397a0, thiscall, stack `a`, `b`, `c`; returns 1 when an edge block is gated on) (name ours).
/// The vector `v`, the velocity of the edge, is the cross of the world axle and the probe direction, scaled by the spin
/// rate times 0.2, plus the hub's velocity. Each gated-on edge block (the plus edge's first) gets `v` through
/// `SubCalls::edge_force`. The port sets the cross's lane 3 and `v`'s lane 3 to 0. The asm's four-lane multiply carries
/// the stack's old lane 3 through the cross into `v`, but `Unk94d0` reads only `v`'s first three lanes, so it has no
/// effect.
pub fn edge_forces<S: WheelStore>(host: &mut impl SubCalls, mem: &mut S, this: u32, a: u32, b: u32, c: u32) -> bool {
    let plus = this + RAY_EDGE_PLUS;
    let minus = this + RAY_EDGE_MINUS;
    let on_plus = gated_on(mem, plus);
    let on_minus = gated_on(mem, minus);
    if !on_plus && !on_minus {
        return false;
    }
    let cross = mem.load_at(this, WORLD_AXLE).cross3(mem.load_at(this, PROBE_DIRECTION));
    let k = mem.f32(this + SPIN_RATE) * 0.2;
    let v = Vec4 {
        x: mem.f32(this + HUB_VELOCITY) + k * cross.x,
        y: mem.f32(this + HUB_VELOCITY + 4) + k * cross.y,
        z: mem.f32(this + HUB_VELOCITY + 8) + k * cross.z,
        w: 0.0,
    };
    if on_plus {
        host.edge_force(this, plus, v, a, b, c);
    }
    if on_minus {
        host.edge_force(this, minus, v, a, b, c);
    }
    true
}
