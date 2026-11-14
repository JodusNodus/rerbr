//! `E51Wheel::M3DD30` (0x53dd30), the wheel step: the ground contact, the strut's spring and the slip.

use super::fields::{
    AXLE, BLEND_OPS, CAR, CAR_FORCE, CAR_SCRATCH, DAMAGE, DAMAGE_CHANGED, DAMAGE_FACTOR, DAMAGE_STEERING,
    DAMAGE_TOP_MOUNT_UP, DAMPER_FORCE, DEF, DEF_ANTI_ROLL_BAR_ID, DEF_ANTI_ROLL_BAR_SLOT, DEF_FLAGS, DEF_INVERSE_MASS,
    DEF_MAX_DEFORM_UP, DEF_PLATFORM_HEIGHT, DEF_TOP_MOUNT, DEF_TOP_MOUNT_DAMAGE_MAX_FORCE,
    DEF_TOP_MOUNT_DAMAGE_THRESHOLD, END_SPIN_RATE, GROUND_BODY, HUB_POSITION, HUB_VELOCITY, INV_ARM, INV_RACK,
    LAST_TAG_1, LAST_TAG_2, LAST_WEIGHT, MATRIX_ROW_1, MATRIX_ROW_2, ON_GROUND, PROBE_DIRECTION, ROLL_BAR_FORCE,
    SPEED_SCALE, SPIN_RATE, SPRING_FORCE, STRUT, STRUT_DIR, STRUT_FORCE, STRUT_INNER_BUMP_POS, STRUT_LENGTH,
    STRUT_OUTER_BUMP_POS, STRUT_TWEAK, STRUT_VEL, STRUT_VEL_TWEAK, SWEEP_RESIDUE, TYRE_SCRATCH, WHEEL_ANGLE,
    WHEEL_ANGLE_TWEAK, WHEEL_INDEX, WORLD_AXLE, WORLD_DEF, WORLD_HUB,
};
use super::refresh_pose::{DEF_IS_STEERED, NEG_KNUCKLE_Z};
use super::store::WheelStore;
use crate::car_body::layout as car_layout;
use crate::entities::layout::ROOT_IDENTITY;
use crate::fpu::round_x87;
use crate::physics_core::layout::SIGNAL_MANAGER;
use crate::rigid_body::layout::{ANGULAR_VELOCITY, LINEAR_VELOCITY};

/// The host calls of `wheel_step`, one method per original callee (`car_slot` is the car's vtable slot 12, called with
/// its arguments left on the stack for `pair_walk`, which pops them as the asm does).
pub trait StepCalls {
    /// `CBase::GetName` (0x52db10) on the strut.
    fn strut_name(&mut self, strut: u32, sp: u32) -> u32;
    /// `CBase::SetDirty` (0x52f450).
    fn set_dirty(&mut self, object: u32, sp: u32);
    /// `E51Wheel::M3B900` (0x53b900), the pose refresh (name ours).
    fn refresh(&mut self, this: u32, travel: f32, sp: u32);
    /// `E51Wheel::M3D520` (0x53d520), the ground probe: 1 when it found a contact.
    fn ground_probe(&mut self, this: u32, out: u32, normal: u32, sp: u32) -> u32;
    /// The car's vtable slot 12 (`[[car] + 0x30]`) with `args`; they stay on the stack.
    fn car_slot(&mut self, car: u32, args: &[u32], sp: u32) -> u32;
    /// `E43PairWalk::MB560` (0x51b560) on `this`, with the three arguments the car call received.
    fn pair_walk(&mut self, this: u32, a: u32, b: u32, c: u32, sp: u32);
    /// `CCar::Xform6950` (0x4e6950, world compose).
    fn world_compose(&mut self, car: u32, a: u32, b: u32, sp: u32);
    /// `CCar::Xform68E0` (0x4e68e0, to world).
    fn to_world(&mut self, car: u32, a: u32, b: u32, sp: u32);
    /// `CCar::Fwd6DB0` (0x4e6600, through the thunk to `CDrive::M6DB0`): the spin rate of the car's wheel `wheel`; its st0
    /// result.
    fn spin_rate(&mut self, car: u32, wheel: u32, sp: u32) -> f32;
    /// `CCar::Fwd6DD0` (0x4e6610, `CDrive::M6DD0`): the drive's value for the wheel that, times -0.7, scales the axle into
    /// the moment (named by use); its st0 result.
    fn axle_term(&mut self, car: u32, wheel: u32, sp: u32) -> f32;
    /// `CCar::Fwd43C0` (0x4e6620, `CDrive::SetTwo`): the two values the tyre step left for the wheel `wheel` (named by
    /// use).
    fn set_drive_pair(&mut self, car: u32, wheel: u32, a: f32, b: f32, sp: u32);
    /// `CCar::GetA48` (0x4e6730, `CarStore::roll_bar_force`): the anti-roll bar's force on the cell `(id, slot)`; its st0
    /// result.
    fn roll_bar_force(&mut self, car: u32, id: u32, slot: u32, sp: u32) -> f32;
    /// `CCar::AddADC` (0x4e6660, `car_body::layout::add_steer_torque_sum`): adds `v` to the steered wheels' torque sum.
    fn add_steer_torque(&mut self, car: u32, v: f32, sp: u32);
    /// `CCar::SetA1C` (0x4e66a0, `car_body::layout::set_roll_bar_cell`): the anti-roll bar's cell `(id, slot)` takes `v`.
    fn set_roll_bar_cell(&mut self, car: u32, id: u32, slot: u32, v: f32, sp: u32);
    /// `CE36SubBlock::Init` (0x4f3e80) on the block at `this`.
    fn init_sub_block(&mut self, this: u32, sp: u32);
    /// `CSolid::Step76` (0x529c80, `rigid_body::solid_rows::point_velocity`) on the body `body`: the velocity of the point
    /// at `point`, to `out`.
    fn point_velocity(&mut self, body: u32, point: u32, out: u32, sp: u32);
    /// `CSpringDamper::Step` (0x52e090) on the strut, with its six arguments.
    fn spring_step(&mut self, strut: u32, args: [u32; 6], sp: u32);
    /// `CTyre::CScratch::SetStateField` (0x532470, `tyre::state::set_active`).
    fn set_tyre_active(&mut self, scratch: u32, v: u32, sp: u32);
    /// `CTyre::CScratch::SetState48` (0x533490, `tyre::state::set_penetration_depth`).
    fn set_penetration_depth(&mut self, scratch: u32, v: f32, sp: u32);
    /// `CTyre::CScratch::RelaxCount` (0x5358c0).
    fn relax_count(&mut self, scratch: u32, x: f32, sp: u32);
    /// `CTyre::CScratch::Step` (0x5360c0) on the scratch, with its sixteen arguments in callee order.
    fn tyre_step(&mut self, scratch: u32, args: [u32; 16], sp: u32);
    /// `CWheel::Blend4` (0x539180, [`carcass_edges`](super::carcass_edges)).
    fn carcass_edges(&mut self, this: u32, axle: u32, hub: u32, plus: u32, minus: u32, sp: u32);
    /// `CWheel::Unk97a0` (0x5397a0, [`edge_forces`](super::edge_forces)).
    fn edge_forces(&mut self, this: u32, a: u32, b: u32, c: u32, sp: u32);
    /// `E51Wheel::M3AB90` (0x53ab90, [`deform_top_mount`](super::deform_top_mount)).
    fn deform_top_mount(&mut self, this: u32, sp: u32);
    /// `E51Wheel::M3AC50` (0x53ac50, [`apply_hub_torque`](super::apply_hub_torque)).
    fn apply_hub_torque(&mut self, this: u32, torque: f32, sp: u32);
    /// `E51Wheel::M3AFB0` (0x53afb0, [`spin_cross`](super::spin_cross)).
    fn spin_cross(&mut self, this: u32, p: u32, sp: u32);
    /// `E51MultiTire::Emit` (0x53d0d0) of the tyre-particle signal `multi_tire`.
    fn emit_tyre_particles(&mut self, multi_tire: u32, sig: u32, sp: u32);
    /// `E09Vec4::Clear` (0x465d80) on `at`; `sp` is the callee's entry stack pointer, as the asm's pushes leave it.
    fn clear_vec4(&mut self, at: u32, sp: u32);
    /// `E01Vec4::CopyFrom` (0x43c5b0): `dst` takes the vector at `src`.
    fn copy_vec4(&mut self, dst: u32, src: u32, sp: u32);
    /// `E01VecSetA::Cross` (0x43c630): `out` = `a x b`.
    fn cross(&mut self, a: u32, b: u32, out: u32, sp: u32);
    /// `CFloat3::Scale` (0x473cd0): `out` = `factor * src`, four lanes.
    fn scale_vec4(&mut self, src: u32, factor: f32, out: u32, sp: u32);
}

/// The margins of the travel clamp below the bump stop and above the droop stop (0.05, 0.02 m), the bound of the strut's
/// velocity (100 m/s), the wrap of the wheel's angle (20π) and the factor of the drive's axle term (-0.7).
const TRAVEL_LOW_MARGIN: f32 = 0.05;
const TRAVEL_HIGH_MARGIN: f32 = 0.02;
const MAX_STRUT_VEL: f32 = 100.0;
const ANGLE_WRAP: f32 = 62.831856;
const AXLE_TERM_SCALE: f32 = -0.7;

/// The multi-tire emitter's offset in the signal manager (`SIGNAL_MANAGER`); the car calls take `[ROOT_IDENTITY]`.
const STEP_TIRES_EMIT: u32 = 0x4518;
/// The world's flags (`WORLD+0x24`, then `+0x160cc`): the ground probe's bit, and the damage bit.
const FLAG_PROBE: u32 = 0x10000;
const FLAG_DAMAGE: u32 = 0x80000;
/// The car's linear velocity tweak (`+0x1b0`, its value at `LINEAR_VELOCITY`): the asm's null test is on its address.
const CAR_LINEAR_VELOCITY_TWEAK: u32 = 0x1B0;

/// `E51Wheel::M3DD30` (0x53dd30, thiscall, `dt` on the stack): the slot-96 step. `frame` is the body's `esp`: the asm's
/// `[esp+k]` is `frame+k`, and the host calls get the frame's addresses, so the lanes the asm never writes keep the
/// original's stack contents. `slip` is the tyre load thresholds and slope of the step's steering damage: NGP's per-car
/// `10172f58`, `10172f5c` and `10172f60` (`0x53ea21`, `0x53ea2d`, `0x53ea45`). The x87 steps run at 53 bits, and the
/// host calls go where NGP's patched calls go (the host's concern).
pub fn wheel_step<S: WheelStore>(
    mem: &mut S,
    calls: &mut impl StepCalls,
    this: u32,
    dt: f32,
    frame: u32,
    slip: [f32; 3],
) {
    let f = frame;
    let def = mem.u32(this + DEF);
    let car = mem.u32(this + CAR);

    // The travel: the strut's two stops bound it between their limits (the travel at +0x5bc).
    calls.clear_vec4(f + 0x50, f - 4);
    calls.clear_vec4(f + 0x40, f - 4);
    mem.set_u32(f + 0xBC, def);
    mem.set_f32(f + 0x20, 0.0);
    mem.set_f32(f + 0x2C, 0.0);
    let strut = this + STRUT;
    let name = calls.strut_name(strut, f - 4);
    let low = mem.f32(name + STRUT_INNER_BUMP_POS) - TRAVEL_LOW_MARGIN;
    mem.set_f32(f + 0x7C, low);
    let name = calls.strut_name(strut, f - 4);
    let travel_old = mem.f32(this + STRUT_LENGTH);
    let high = mem.f32(name + STRUT_OUTER_BUMP_POS) + TRAVEL_HIGH_MARGIN;
    let travel = if low <= travel_old {
        if travel_old > high {
            high
        } else {
            travel_old
        }
    } else {
        low
    };
    mem.set_f32(f + 0x1C, travel);
    calls.set_dirty(this + STRUT_TWEAK, f - 4);
    mem.set_f32(this + STRUT_LENGTH, travel);
    calls.refresh(this, mem.f32(this + STRUT_LENGTH), f - 8);

    // Two rows of the wheel's matrix, each loaded into the frame and copied to a frame slot. Lane 3 of each copy takes
    // the frame's old `+0x6c`, as the asm leaves it.
    mem.set_u32(f + 0x60, mem.u32(this + MATRIX_ROW_2));
    mem.set_u32(f + 0x64, mem.u32(this + MATRIX_ROW_2 + 4));
    mem.set_u32(f + 0x68, mem.u32(this + MATRIX_ROW_2 + 8));
    mem.set_u32(f + 0xA0, mem.u32(f + 0x60));
    mem.set_u32(f + 0xA4, mem.u32(f + 0x64));
    mem.set_u32(f + 0xA8, mem.u32(f + 0x68));
    mem.set_u32(f + 0xAC, mem.u32(f + 0x6C));
    calls.copy_vec4(f + 0x160, f + 0xA0, f - 8);
    mem.set_u32(f + 0x60, mem.u32(this + MATRIX_ROW_1));
    mem.set_u32(f + 0x64, mem.u32(this + MATRIX_ROW_1 + 4));
    mem.set_u32(f + 0x68, mem.u32(this + MATRIX_ROW_1 + 8));
    mem.set_u32(f + 0xA0, mem.u32(f + 0x60));
    mem.set_u32(f + 0xA4, mem.u32(f + 0x64));
    mem.set_u32(f + 0xA8, mem.u32(f + 0x68));
    mem.set_u32(f + 0xAC, mem.u32(f + 0x6C));
    calls.copy_vec4(f + 0x120, f + 0xA0, f - 8);
    calls.world_compose(car, this + HUB_POSITION, f + 0x80, f - 0xC);
    let g = mem.u32(ROOT_IDENTITY);
    car_walk(calls, car, f + 0x160, g, f + 0x150, f - 0x10);
    car_walk(calls, car, this + AXLE, g, this + WORLD_AXLE, f - 0x10);
    calls.carcass_edges(this, this + WORLD_AXLE, f + 0x80, f + 0x190, f + 0x170, f - 0x14);
    calls.scale_vec4(f + 0x150, -1.0, this + PROBE_DIRECTION, f - 0xC);
    calls.copy_vec4(this + WORLD_HUB, f + 0x80, f - 8);
    let wheel_index = mem.u32(this + WHEEL_INDEX);
    let spin = calls.spin_rate(car, wheel_index, f - 8);
    mem.set_f32(this + SPIN_RATE, spin);

    // The ground probe, when the world's flag is set and it finds a contact.
    let world = mem.u32(car_layout::WORLD);
    let probe_flags = mem.u32(world + WORLD_DEF);
    mem.set_f32(f + 0x7C, 0.0);
    let ground = if mem.u32(probe_flags + DEF_FLAGS) & FLAG_PROBE != 0 {
        calls.ground_probe(this, f + 0x130, f + 0x140, f - 0xC)
    } else {
        0
    };
    if ground != 0 {
        ground_contact(mem, calls, this, dt, f, car);
    } else {
        mem.set_u32(this + ON_GROUND, 0);
        mem.set_f32(f + 0x34, 0.0);
        calls.clear_vec4(f + 0xD0, f - 4);
        calls.clear_vec4(f + 0x90, f - 4);
        calls.set_penetration_depth(this + TYRE_SCRATCH, 0.0, f - 8);
        calls.set_tyre_active(this + TYRE_SCRATCH, 0, f - 8);
    }
    spring_and_slip(mem, calls, this, dt, f, car, def, slip);
}

/// The car's vtable call with `a, b, c`, then `pair_walk` on its result with the same three (the asm leaves them on the
/// stack for `MB560` to pop).
fn car_walk(calls: &mut impl StepCalls, car: u32, a: u32, b: u32, c: u32, sp: u32) {
    let r = calls.car_slot(car, &[a, b, c], sp);
    calls.pair_walk(r, a, b, c, sp);
}

/// The branch of `wheel_step` with a ground contact (`M3D520` returned 1): the contact's frame and its force.
fn ground_contact<S: WheelStore>(mem: &mut S, calls: &mut impl StepCalls, this: u32, dt: f32, f: u32, car: u32) {
    mem.set_u32(this + ON_GROUND, 1);
    calls.to_world(car, this + NEG_KNUCKLE_Z, f + 0x1C0, f - 0xC);
    let g = mem.u32(ROOT_IDENTITY);
    car_walk(calls, car, this + HUB_POSITION, g, f + 0xA0, f - 0x10);
    calls.cross(car + ANGULAR_VELOCITY, f + 0xA0, f + 0xE0, f - 0xC);
    // The car's linear velocity, unless the car is the null object (`car == -0x1b0`).
    let pf = if car.wrapping_add(CAR_LINEAR_VELOCITY_TWEAK) == 0 { 0 } else { car + LINEAR_VELOCITY };
    for k in 0..3 {
        let v = mem.f32(pf + 4 * k) + mem.f32(f + 0xE0 + 4 * k);
        mem.set_f32(f + 0xE0 + 4 * k, v);
    }
    let s = mem.f32(this + STRUT_VEL);
    mem.set_f32(f + 0x1C, s);
    for k in 0..4 {
        let v = mem.f32(f + 0x1C0 + 4 * k) * s + mem.f32(f + 0xE0 + 4 * k);
        mem.set_f32(f + 0xE0 + 4 * k, v);
    }
    calls.copy_vec4(this + HUB_VELOCITY, f + 0xE0, f - 8);
    calls.clear_vec4(f + 0x1B0, f - 4);
    calls.clear_vec4(f + 0x1A0, f - 4);
    calls.init_sub_block(f + 0x280, f - 4);
    calls.clear_vec4(f + 0x230, f - 4);
    calls.clear_vec4(f + 0x1F0, f - 4);
    calls.clear_vec4(f + 0x200, f - 4);

    mem.set_u32(f + 0x218, mem.u32(f + 0x88));
    mem.set_u32(f + 0x210, mem.u32(f + 0x80));
    mem.set_u32(f + 0x214, mem.u32(f + 0x84));
    mem.set_u32(f + 0x21C, mem.u32(f + 0x8C));
    mem.set_f32(f + 0x268, 0.0);
    mem.set_f32(f + 0x260, 0.0);
    mem.set_u32(f + 0x28C, 0);
    mem.set_f32(f + 0x274, 0.0);
    mem.set_f32(f + 0x264, 0.0);
    mem.set_f32(f + 0x278, 0.0);
    mem.set_f32(f + 0x270, 0.0);
    let ground_body = mem.u32(this + GROUND_BODY);
    if ground_body != 0 {
        calls.point_velocity(ground_body, f + 0x130, f + 0x1D0, f - 0xC);
        for k in 0..3 {
            let v = mem.f32(this + HUB_VELOCITY + 4 * k) - mem.f32(f + 0x1D0 + 4 * k);
            mem.set_f32(f + 0x180 + 4 * k, v);
        }
    } else {
        calls.copy_vec4(f + 0x180, this + HUB_VELOCITY, f - 8);
    }

    let args = [
        mem.u32(this + SPIN_RATE),
        dt.to_bits(),
        mem.u32(this + SPEED_SCALE),
        mem.u32(this + SWEEP_RESIDUE),
        this + BLEND_OPS,
        this + LAST_TAG_1,
        f + 0x140,
        f + 0x80,
        this + WORLD_AXLE,
        f + 0x180,
        f + 0x130,
        f + 0x1B0,
        f + 0x1A0,
        f + 0x20,
        f + 0x2C,
        f + 0x1E0,
    ];
    calls.tyre_step(this + TYRE_SCRATCH, args, f - 0x44);
    calls.edge_forces(this, f + 0x1B0, f + 0x1A0, f + 0x20, f - 0x10);
    mem.set_u32(f + 0x1E0, mem.u32(this + WHEEL_INDEX));
    calls.copy_vec4(f + 0x220, f + 0x140, f - 8);
    let tag_1 = mem.u32(this + LAST_TAG_1);
    let tag_2 = mem.u32(this + LAST_TAG_2);
    let weight = mem.u32(this + LAST_WEIGHT);
    mem.set_u32(f + 0x280, tag_1);
    mem.set_u32(f + 0x284, tag_2);
    mem.set_u32(f + 0x288, weight);
    calls.emit_tyre_particles(mem.u32(SIGNAL_MANAGER) + STEP_TIRES_EMIT, f + 0x1E0, f - 8);

    let r5 = calls.car_slot(car, &[f + 0xD0], f - 8);
    calls.pair_walk(mem.u32(ROOT_IDENTITY), f + 0x1B0, r5, f + 0xD0, f - 0x10);
    let r4 = calls.car_slot(car, &[f + 0x90], f - 8);
    calls.pair_walk(mem.u32(ROOT_IDENTITY), f + 0x1A0, r4, f + 0x90, f - 0x10);

    let t = mem.f32(f + 0x98) * mem.f32(f + 0x128)
        + mem.f32(f + 0x94) * mem.f32(f + 0x124)
        + mem.f32(f + 0x90) * mem.f32(f + 0x120);
    mem.set_f32(f + 0x7C, t);
    let knuckle = this + NEG_KNUCKLE_Z;
    let x1 = mem.f32(knuckle + 8) * mem.f32(f + 0xD8);
    let x2 = mem.f32(knuckle + 4) * mem.f32(f + 0xD4) + x1;
    let x3 = x2 + mem.f32(f + 0xD0) * mem.f32(knuckle);
    mem.set_u32(f + 0x3C, knuckle);
    mem.set_f32(f + 0x34, x3);
    mem.set_u32(f + 0x38, f + 0x60);
    let neg = 0.0 - x3;
    mem.set_f32(f + 0x18, neg);
    mem.set_u32(f + 0x1C, f + 0xD0);
    for k in 0..4 {
        let v = mem.f32(knuckle + 4 * k) * neg + mem.f32(f + 0xD0 + 4 * k);
        mem.set_f32(f + 0x60 + 4 * k, v);
    }
    let v = mem.f32(f + 0x60) + mem.f32(f + 0x50);
    mem.set_f32(f + 0x50, v);
    let v = mem.f32(f + 0x64) + mem.f32(f + 0x54);
    mem.set_f32(f + 0x54, v);
    let v = mem.f32(f + 0x68) + mem.f32(f + 0x58);
    mem.set_f32(f + 0x58, v);
    let v = mem.f32(f + 0x40) + mem.f32(f + 0x90);
    mem.set_f32(f + 0x40, v);
    let v = mem.f32(f + 0x44) + mem.f32(f + 0x94);
    mem.set_f32(f + 0x44, v);
    let v = mem.f32(f + 0x48) + mem.f32(f + 0x98);
    mem.set_f32(f + 0x48, v);
    calls.set_penetration_depth(this + TYRE_SCRATCH, mem.f32(this + SPEED_SCALE), f - 8);
}

/// The common tail of `wheel_step`: the strut's spring, the force and its slip, the car's side forces, the damage, and
/// the relaxation.
fn spring_and_slip<S: WheelStore>(
    mem: &mut S,
    calls: &mut impl StepCalls,
    this: u32,
    dt: f32,
    f: u32,
    car: u32,
    def: u32,
    slip: [f32; 3],
) {
    let round24 = round_x87;
    let [slip_lo, slip_hi, slip_k] = slip;
    let y = mem.f32(def + DEF_PLATFORM_HEIGHT) + mem.f32(this + STRUT_LENGTH);
    let z = mem.u32(this + STRUT_VEL);
    calls.spring_step(
        this + STRUT,
        [mem.u32(this + STRUT_LENGTH), y.to_bits(), z, f + 0x24, f + 0x28, f + 0x30],
        f - 0x1C,
    );

    let roll_bar =
        calls.roll_bar_force(car, mem.u32(def + DEF_ANTI_ROLL_BAR_ID), mem.u32(def + DEF_ANTI_ROLL_BAR_SLOT), f - 0xC);
    mem.set_f32(f + 0x38, roll_bar);
    let t = mem.f32(f + 0x38) + mem.f32(f + 0x24) + mem.f32(f + 0x28) + mem.f32(f + 0x34);
    let m = mem.f32(def + DEF_INVERSE_MASS);
    let num = t * m * dt + mem.f32(this + STRUT_VEL);
    let den = m * mem.f32(f + 0x30) * dt + 1.0;
    let vel = num / den;
    let vel = if -MAX_STRUT_VEL > vel {
        -MAX_STRUT_VEL
    } else if vel > MAX_STRUT_VEL {
        MAX_STRUT_VEL
    } else {
        vel
    };
    mem.set_f32(f + 0x1C, vel);
    let old_vel = mem.f32(this + STRUT_VEL);
    let travel = vel * dt + mem.f32(this + STRUT_LENGTH);
    mem.set_f32(f + 0x3C, old_vel);
    mem.set_f32(f + 0x18, travel);
    calls.set_dirty(this + STRUT_TWEAK, f - 4);
    mem.set_f32(this + STRUT_LENGTH, mem.f32(f + 0x18));
    calls.set_dirty(this + STRUT_VEL_TWEAK, f - 4);

    let q = (vel - mem.f32(f + 0x3C)) / dt / m;
    mem.set_f32(this + STRUT_VEL, vel);
    let s = mem.f32(f + 0x34) - q;
    mem.set_f32(f + 0x34, s);
    mem.set_u32(f + 0x3C, this + NEG_KNUCKLE_Z);
    mem.set_u32(f + 0x18, f + 0x50);
    for k in 0..4 {
        let v = mem.f32(this + NEG_KNUCKLE_Z + 4 * k) * s + mem.f32(f + 0x50 + 4 * k);
        mem.set_f32(f + 0x50 + 4 * k, v);
    }
    let p = vel * mem.f32(f + 0x30);
    let spring = mem.f32(f + 0x24);
    let breakpoint = mem.f32(f + 0x28);
    mem.set_f32(this + ROLL_BAR_FORCE, mem.f32(f + 0x38));
    mem.set_f32(this + SPRING_FORCE, spring);
    mem.set_f32(this + STRUT_FORCE, s);
    mem.set_f32(f + 0x34, p + spring + breakpoint);
    mem.set_f32(this + DAMPER_FORCE, p + breakpoint);

    let v0 = mem.f32(this + HUB_POSITION) - mem.f32(def + DEF_TOP_MOUNT);
    let v1 = mem.f32(this + HUB_POSITION + 4) - mem.f32(def + DEF_TOP_MOUNT + 4);
    let v2 = mem.f32(this + HUB_POSITION + 8) - mem.f32(def + DEF_TOP_MOUNT + 8);
    mem.set_f32(f + 0x110, v0);
    mem.set_f32(f + 0x114, v1);
    mem.set_f32(f + 0x118, v2);
    calls.cross(f + 0x110, f + 0x50, f + 0xC0, f - 0xC);
    for k in 0..3 {
        let v = mem.f32(f + 0xC0 + 4 * k) + mem.f32(f + 0x40 + 4 * k);
        mem.set_f32(f + 0xC0 + 4 * k, v);
    }
    if mem.u32(def + DEF_IS_STEERED) != 0 {
        let v = mem.f32(this + STRUT_DIR + 8) * mem.f32(f + 0xC8)
            + mem.f32(this + STRUT_DIR + 4) * mem.f32(f + 0xC4)
            + mem.f32(this + STRUT_DIR) * mem.f32(f + 0xC0);
        let v = v * mem.f32(this + INV_ARM) * mem.f32(this + INV_RACK);
        calls.add_steer_torque(car, v, f - 8);
    }
    calls.cross(this + HUB_POSITION, f + 0x50, f + 0x100, f - 0xC);
    let v = mem.f32(f + 0x100) + mem.f32(f + 0x40);
    mem.set_f32(f + 0x40, v);
    let v = mem.f32(f + 0x104) + mem.f32(f + 0x44);
    mem.set_f32(f + 0x44, v);
    let v = mem.f32(f + 0x108) + mem.f32(f + 0x48);
    mem.set_f32(f + 0x48, v);
    let axle_term = calls.axle_term(car, mem.u32(this + WHEEL_INDEX), f - 8);
    mem.set_f32(f + 0x38, round24(axle_term as f64 * AXLE_TERM_SCALE as f64) as f32);
    let axle = this + AXLE;
    mem.set_u32(f + 0x3C, axle);
    mem.set_u32(f + 0x18, f + 0x40);
    for k in 0..4 {
        let v = mem.f32(axle + 4 * k) * mem.f32(f + 0x38) + mem.f32(f + 0x40 + 4 * k);
        mem.set_f32(f + 0x40 + 4 * k, v);
    }
    calls.spin_cross(this, f + 0xF0, f - 8);
    let v = mem.f32(f + 0x50) + mem.f32(this + CAR_FORCE);
    let d0 = mem.f32(f + 0x40) - mem.f32(f + 0xF0);
    let d1 = mem.f32(f + 0x44) - mem.f32(f + 0xF4);
    let d2 = mem.f32(f + 0x48) - mem.f32(f + 0xF8);
    mem.set_f32(this + CAR_FORCE, v);
    let v = mem.f32(f + 0x54) + mem.f32(this + CAR_FORCE + 4);
    mem.set_f32(this + CAR_FORCE + 4, v);
    let v = mem.f32(f + 0x58) + mem.f32(this + CAR_FORCE + 8);
    mem.set_f32(this + CAR_FORCE + 8, v);
    mem.set_f32(f + 0x40, d0);
    mem.set_f32(this + CAR_SCRATCH, d0 + mem.f32(this + CAR_SCRATCH));
    mem.set_f32(f + 0x44, d1);
    mem.set_f32(this + CAR_SCRATCH + 4, d1 + mem.f32(this + CAR_SCRATCH + 4));
    mem.set_f32(f + 0x48, d2);
    mem.set_f32(this + CAR_SCRATCH + 8, d2 + mem.f32(this + CAR_SCRATCH + 8));

    let damage = mem.u32(this + DAMAGE);
    let world = mem.u32(car_layout::WORLD);
    let damage_on = mem.u32(mem.u32(world + WORLD_DEF) + DEF_FLAGS) & FLAG_DAMAGE != 0;
    if damage_on {
        let v = mem.f32(damage + DAMAGE_FACTOR) * mem.f32(f + 0x34);
        if v > mem.f32(def + DEF_TOP_MOUNT_DAMAGE_THRESHOLD) {
            let w = (v - mem.f32(def + DEF_TOP_MOUNT_DAMAGE_THRESHOLD))
                / (mem.f32(def + DEF_TOP_MOUNT_DAMAGE_MAX_FORCE) - mem.f32(def + DEF_TOP_MOUNT_DAMAGE_THRESHOLD));
            let sq = w * w + mem.f32(damage + DAMAGE_TOP_MOUNT_UP);
            mem.set_f32(damage + DAMAGE_TOP_MOUNT_UP, sq);
            if mem.f32(damage + DAMAGE_TOP_MOUNT_UP) > 1.0 {
                mem.set_f32(damage + DAMAGE_TOP_MOUNT_UP, 1.0);
            }
            calls.deform_top_mount(this, f - 4);
            mem.set_u32(this + DAMAGE_CHANGED, 1);
        }
        let v = mem.f32(damage + DAMAGE_FACTOR) * mem.f32(f + 0x7C);
        calls.apply_hub_torque(this, v, f - 8);
        let axle = this + AXLE;
        let s = mem.f32(axle + 8) * mem.f32(f + 0x58) + mem.f32(axle + 4) * mem.f32(f + 0x54);
        let s = s + mem.f32(f + 0x50) * mem.f32(axle);
        let x = 0.0 - s * mem.f32(damage + DAMAGE_FACTOR);
        if x > slip_lo {
            if x > slip_hi {
                mem.set_f32(damage + DAMAGE_STEERING, 1.0);
            } else {
                let v = (x - slip_lo) * slip_k;
                if v > mem.f32(damage + DAMAGE_STEERING) {
                    mem.set_f32(damage + DAMAGE_STEERING, v);
                }
            }
            mem.set_u32(this + DAMAGE_CHANGED, 1);
        }
    }
    calls.set_drive_pair(car, mem.u32(this + WHEEL_INDEX), mem.f32(f + 0x20), mem.f32(f + 0x2C), f - 0x10);

    let v = mem.f32(this + STRUT_LENGTH) - mem.f32(damage + DAMAGE_TOP_MOUNT_UP) * mem.f32(def + DEF_MAX_DEFORM_UP);
    calls.set_roll_bar_cell(
        car,
        mem.u32(def + DEF_ANTI_ROLL_BAR_ID),
        mem.u32(def + DEF_ANTI_ROLL_BAR_SLOT),
        v,
        f - 0x10,
    );
    let spin = calls.spin_rate(car, mem.u32(this + WHEEL_INDEX), f - 8);
    mem.set_f32(f + 0x18, round24(spin as f64 * dt as f64) as f32 + mem.f32(this + WHEEL_ANGLE));
    calls.set_dirty(this + WHEEL_ANGLE_TWEAK, f - 4);
    mem.set_f32(this + WHEEL_ANGLE, mem.f32(f + 0x18));
    let x = mem.f32(this + WHEEL_ANGLE);
    let wrapped = if x > ANGLE_WRAP {
        Some(x - ANGLE_WRAP)
    } else if ANGLE_WRAP > x {
        Some(x + ANGLE_WRAP)
    } else {
        None
    };
    if let Some(v) = wrapped {
        mem.set_f32(f + 0x18, v);
        calls.set_dirty(this + WHEEL_ANGLE_TWEAK, f - 4);
        mem.set_f32(this + WHEEL_ANGLE, mem.f32(f + 0x18));
    }
    calls.relax_count(this + TYRE_SCRATCH, dt, f - 8);
    let spin = calls.spin_rate(car, mem.u32(this + WHEEL_INDEX), f - 8);
    mem.set_f32(this + END_SPIN_RATE, spin);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_are_the_originals() {
        // The float words at 0x72dd78, 0x72fcb4, 0x72bf10, 0x740bd4 and 0x72f43c.
        assert_eq!(TRAVEL_LOW_MARGIN.to_bits(), 0x3D4C_CCCD);
        assert_eq!(TRAVEL_HIGH_MARGIN.to_bits(), 0x3CA3_D70A);
        assert_eq!(MAX_STRUT_VEL.to_bits(), 0x42C8_0000);
        assert_eq!((-MAX_STRUT_VEL).to_bits(), 0xC2C8_0000);
        assert_eq!(ANGLE_WRAP.to_bits(), 0x427B_53D2);
        assert_eq!(AXLE_TERM_SCALE.to_bits(), 0xBF33_3333);
    }
}
