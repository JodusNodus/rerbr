//! The drive workers (`rbr_physics::engine_driveline::drive_worker`): wheels, gears, pair, clutch, and `Step1E0`.

use rbr_physics::engine_driveline::drive_step::DriveStepHost;
use rbr_physics::engine_driveline::drive_worker::{FreeWheelHost, GearHost, MatInverseHost, MatVecHost};
use rbr_physics::engine_driveline::{drive_leaf, drive_worker, layout, mat4};
use rbr_physics::mem::Mem;

use super::{f, on_drive, CAR_GET30, EMIT_GEAR_CHANGE, FWD18E20, GEAR_CHANGE_SLOT, MATVEC, SIGNALS};
use crate::ngp::clutch_scale;
use crate::{Ctx, Decline, Host, Regs, Return};

/// Runs a typed drive-worker port on the worker of the drive at `this` ([`on_drive`]): loads its view, runs the port,
/// stores the view back (the same edge as the vector tests).
fn with_worker<T>(ctx: &mut Ctx, this: u32, port: impl FnOnce(&mut layout::DriveWorker) -> T) -> T {
    on_drive(ctx, this, |d| layout::with_worker(d, port))
}

/// `with_worker` for the gear workers, which also load the gear's words (stored last).
fn with_gear<T>(
    ctx: &mut Ctx,
    this: u32,
    port: impl FnOnce(&mut layout::DriveWorker, &mut layout::GearWork) -> T,
) -> T {
    on_drive(ctx, this, |d| layout::with_gear(d, port))
}

pub(super) fn set_lane_scale(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let x = layout::LaneScale::load(&ctx.mem, r.stack[0]);
    with_worker(ctx, r.ecx, |w| drive_leaf::set_lane_scale(w, x));
    Ok(Return::void(0, 4))
}

/// The wheel worker's external calls, run by the host on the image. The port's values are written where the
/// original reads them, the original runs, and its results are read back. `Step4990` and `Step3870` are ports.
struct WheelCalls<'a> {
    mem: Mem,
    host: &'a dyn Host,
    this: u32,
    state: u32,
}

fn store_floats(mem: &mut Mem, at: u32, values: &[f32]) {
    for (k, v) in values.iter().enumerate() {
        mem.set_f32(at + 4 * k as u32, *v);
    }
}

impl drive_worker::MatInverseHost for WheelCalls<'_> {
    fn mat4_inverse_transposed(&mut self, mat: [f32; 16]) -> [f32; 16] {
        mat4::inverse_transposed(mat)
    }
}

impl drive_worker::MatVecHost for WheelCalls<'_> {
    fn mat_vec(&mut self, m: [f32; 16], v: [f32; 4]) -> [f32; 4] {
        let matrix = self.state + layout::STEP_MATRIX_AT;
        let vec = self.state + layout::STEP_VEC_AT;
        let out = self.this + layout::SOLUTION_AT;
        store_floats(&mut self.mem, matrix, &m);
        store_floats(&mut self.mem, vec, &v);
        self.host.call(MATVEC, matrix, &[vec, out]);
        self.mem.f32s(out)
    }
}

impl drive_worker::FreeWheelHost for WheelCalls<'_> {
    fn free_wheel_step(&mut self, w: &mut layout::DriveWorker, i: usize, dt: f32) {
        drive_worker::free_wheel_step(w, i, dt);
    }
}

fn wheel_calls<'a>(ctx: &Ctx<'a>, r: &Regs) -> WheelCalls<'a> {
    WheelCalls { mem: ctx.mem.clone(), host: ctx.host, this: r.ecx, state: ctx.mem.u32(r.ecx + 8) }
}

/// The gear workers' external calls, run by the host. The payload goes where the original's `M79B0` and `M9210`
/// keep it, their frame below the return address.
struct Gear<'a> {
    mem: Mem,
    host: &'a dyn Host,
    payload_at: u32,
}

impl GearHost for Gear<'_> {
    fn clutch_holds_only_with_capacity(&self) -> bool {
        crate::ngp::engine_model_is_set()
    }

    fn car_signal_word(&mut self, car: u32) -> u32 {
        self.host.call(CAR_GET30, car, &[]).eax
    }

    fn emit_gear_change(&mut self, payload: [u32; 4]) {
        for (k, word) in payload.iter().enumerate() {
            self.mem.set_u32(self.payload_at + 4 * k as u32, *word);
        }
        let slot = self.mem.u32(SIGNALS) + GEAR_CHANGE_SLOT;
        self.host.call(EMIT_GEAR_CHANGE, slot, &[self.payload_at]);
    }
}

fn gear<'a>(ctx: &Ctx<'a>, r: &Regs) -> Gear<'a> {
    Gear { mem: ctx.mem.clone(), host: ctx.host, payload_at: r.esp - 0x10 }
}

/// thiscall `(dt)`, void.
pub(super) fn wheel_worker(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let mut host = wheel_calls(ctx, r);
    let dt = f(r.stack[0]);
    let split = crate::ngp::drive_split(ctx)?;
    with_worker(ctx, r.ecx, |w| drive_worker::wheel_worker(w, &mut host, dt, split));
    Ok(Return::void(0, 4))
}

/// thiscall `(a, out*, b, c, d)`: NGP's rewrite (`0x10047300`) of the clutch-torque worker (`M93A0`), stored at `out`
/// when the port gives a value.
pub(crate) fn clutch_torque(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let mut host = gear(ctx, r);
    let [a, out, b, c, d, ..] = r.stack;
    let written = with_gear(ctx, r.ecx, |w, g| drive_worker::clutch_torque(w, g, &mut host, f(a), [b, c, d].map(f)));
    if let Some(value) = written {
        ctx.mem.set_f32(out, value);
    }
    Ok(Return::void(0, 0x14))
}

/// The callees of `Step1E0`: the wheel and gear workers' (run as their bindings run them), and the car forwarder, which
/// the host runs.
struct Step1Calls<'a> {
    wheel: WheelCalls<'a>,
    gear: Gear<'a>,
    host: &'a dyn Host,
    this: u32,
}

impl MatInverseHost for Step1Calls<'_> {
    fn mat4_inverse_transposed(&mut self, mat: [f32; 16]) -> [f32; 16] {
        self.wheel.mat4_inverse_transposed(mat)
    }
}

impl MatVecHost for Step1Calls<'_> {
    fn mat_vec(&mut self, m: [f32; 16], v: [f32; 4]) -> [f32; 4] {
        self.wheel.mat_vec(m, v)
    }
}

impl FreeWheelHost for Step1Calls<'_> {
    fn free_wheel_step(&mut self, w: &mut layout::DriveWorker, i: usize, dt: f32) {
        self.wheel.free_wheel_step(w, i, dt);
    }
}

impl GearHost for Step1Calls<'_> {
    fn clutch_holds_only_with_capacity(&self) -> bool {
        self.gear.clutch_holds_only_with_capacity()
    }

    fn car_signal_word(&mut self, car: u32) -> u32 {
        self.gear.car_signal_word(car)
    }

    fn emit_gear_change(&mut self, payload: [u32; 4]) {
        self.gear.emit_gear_change(payload);
    }
}

impl DriveStepHost for Step1Calls<'_> {
    fn car_forwarder(&mut self, car: u32, dt: f32, speed: f32) -> f64 {
        self.host.call(FWD18E20, car, &[dt.to_bits(), speed.to_bits()]).st0
    }

    fn ngp_speed_term(&mut self, dt: f32) -> f32 {
        crate::ngp::starter_step(&mut self.gear.mem, self.host, self.this, dt)
    }

    fn ngp_curve_term(&mut self, args: [u32; 4], dt: f32) -> f32 {
        let [a, b, c, d] = args;
        let args = [a, b, c, d, dt.to_bits()];
        if let Ok(v) = crate::ngp::curve_term(&mut self.gear.mem, self.host, self.this, args) {
            return v;
        }
        let target = crate::ngp::at(crate::ngp::CURVE_TERM).expect("NGP drive step");
        self.host.call(target, self.this, &args).st0 as f32
    }

    fn ngp_wheel_worker(&mut self, dt: f32) {
        let target = crate::ngp::at(0x10048d50).expect("NGP drive step");
        self.host.call(target, self.this, &[dt.to_bits()]);
    }
}

/// thiscall `(dt)`: `CDrive::Step1E0`, the drive's step, with the car forwarder (and NGP's rewrites) run by the host.
pub(super) fn step1e0(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    crate::ngp::only()?;
    let mut calls = Step1Calls { wheel: wheel_calls(ctx, r), gear: gear(ctx, r), host: ctx.host, this: r.ecx };
    let scale = clutch_scale(ctx);
    on_drive(ctx, r.ecx, |d| layout::step_drive(&mut calls, d, f(r.stack[0]), scale));
    Ok(Return::void(0, 4))
}

// CDriveLine.
