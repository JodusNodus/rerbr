//! The engine (`CEngine`): its options, damper, torque, intake and turbo steps, and the `Sim8890`/`Sim8E20` walks.
//! Each runs on the engine's store ([`on_engine`]): the typed engine a step holds, else the image.

use crate::stage::on_stage;
use rbr_physics::engine_driveline::{engine, layout};
use rbr_physics::math::vec::Vec4;
use rbr_physics::mem::Mem;

use super::{f, on_engine, E01LOG, FLOOR_S32_FORMAT, GRAPH_PUSH, MB560, SIGNALS, WALKER};
use crate::ngp::engine_tuning;
use crate::{Ctx, Decline, Host, Regs, Return};
use rbr_physics::store::Raw;

/// thiscall `()`, st0.
pub(super) fn abs_rpm_term(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    Ok(Return::float(on_engine(ctx, r.ecx, |e| layout::abs_rpm_term(e, r.ecx)) as f64, 0))
}

/// thiscall `(slot)`, eax.
pub(super) fn slot_roll(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let i = r.stack[0];
    if i >= 4 {
        return Err(Decline::Unsupported("roll slot outside 0..4"));
    }
    Ok(Return::void(on_engine(ctx, r.ecx, |e| layout::slot_roll(e, r.ecx, i)), 4))
}

/// thiscall `(a, b, c, table*, d, block*, out*)`.
pub(super) fn smooth_damper(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [a, b, c, table, d, block_at, out, ..] = r.stack;
    on_engine(ctx, r.ecx, |e| {
        let g = layout::smooth_damper(e, r.ecx, [f(a), f(b), f(c)], table, f(d), block_at);
        e.set_f32(out, g);
    });
    Ok(Return::void(0, 0x1c))
}

/// thiscall `(a, b, table*, d, block*, out*)`.
pub(super) fn solve_damper(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [a, b, table, d, block_at, out, ..] = r.stack;
    let tuning = engine_tuning(ctx);
    on_engine(ctx, r.ecx, |e| {
        let v = layout::solve_damper(e, r.ecx, [f(a), f(b)], table, f(d), block_at, &tuning);
        e.set_f32(out, v);
    });
    Ok(Return::void(0, 0x18))
}

/// thiscall `(a, b_out*, c_out*)`.
pub(super) fn crank_tick(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [a, b_out, c_out, ..] = r.stack;
    on_engine(ctx, r.ecx, |e| {
        let (b, c) = layout::crank_tick(e, r.ecx, f(a));
        e.set_u32(b_out, b as u32);
        e.set_f32(c_out, c);
    });
    Ok(Return::void(0, 0xc))
}

/// thiscall `()`, st0.
pub(super) fn rev_gate(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let tuning = engine_tuning(ctx);
    Ok(Return::float(on_engine(ctx, r.ecx, |e| layout::rev_gate(e, r.ecx, &tuning)), 0))
}

/// thiscall `(a)`.
pub(super) fn misfire_roll(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    on_engine(ctx, r.ecx, |e| layout::misfire_roll(e, r.ecx, f(r.stack[0])));
    Ok(Return::void(0, 4))
}

/// thiscall `(a)`, st0.
pub(super) fn engine_torque(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let st0 = on_engine(ctx, r.ecx, |e| layout::engine_torque(e, r.ecx, f(r.stack[0])));
    Ok(Return::float(st0 as f64, 4))
}

/// thiscall `()`.
pub(super) fn compliance(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    on_engine(ctx, r.ecx, |e| layout::compliance(e, r.ecx));
    Ok(Return::void(0, 0))
}

/// thiscall `(v)`, eax: the options pointer (`layout::set_options`).
pub(super) fn set_options(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let eax = on_engine(ctx, r.ecx, |e| layout::set_options(e, r.ecx, r.stack[0]));
    Ok(Return::void(eax, 4))
}

/// thiscall `(a, b, c, d)`, eax: the four cut words (`layout::set_cut_flags`).
pub(super) fn set_cut_flags(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let words = [r.stack[0], r.stack[1], r.stack[2], r.stack[3]];
    let eax = on_engine(ctx, r.ecx, |e| layout::set_cut_flags(e, r.ecx, words));
    Ok(Return::void(eax, 0x10))
}

/// thiscall `()`, st0: the options block's float at +0x58, or 0 (`layout::options_first_gate`).
pub(super) fn options_gate_58(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    Ok(Return::float(on_engine(ctx, r.ecx, |e| layout::options_first_gate(e, r.ecx)) as f64, 0))
}

/// thiscall `()`, st0: the damper bias of the state's float at +0x28 (`layout::coolant_temperature`).
pub(super) fn sim_e970(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    Ok(Return::float(on_engine(ctx, r.ecx, |e| layout::coolant_temperature(e, r.ecx)), 0))
}

/// thiscall `()`, eax: 1 when the state's float at +0x188 is above 40 (`layout::engine_running`).
pub(super) fn rpm_above_40(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    Ok(Return::void(on_engine(ctx, r.ecx, |e| layout::engine_running(e, r.ecx)), 0))
}

/// thiscall `()`, eax: the state or the world object (`layout::discarded_temperatures`); nothing is written.
pub(super) fn sim_e9a0(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    Ok(Return::void(on_engine(ctx, r.ecx, |e| layout::discarded_temperatures(e, r.ecx)), 0))
}

/// thiscall `()`, st0: the state's float at +0x68 when the definition's flag is set, else 0 (`layout::boost_pressure`).
pub(super) fn sim_ee60(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    Ok(Return::float(on_engine(ctx, r.ecx, |e| layout::boost_pressure(e, r.ecx)) as f64, 0))
}

/// thiscall `()`, st0: the state's float at +0x188 (`layout::engine_state_speed`).
pub(super) fn engine_state_speed(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    Ok(Return::float(on_engine(ctx, r.ecx, |e| layout::engine_state_speed(e, r.ecx)) as f64, 0))
}

/// thiscall `(x)`: the brake-side damper step; the `eax` is the options block or the state (`layout::overheat_step`).
pub(super) fn sim8610(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let eax = on_engine(ctx, r.ecx, |e| layout::overheat_step(e, r.ecx, f(r.stack[0])));
    Ok(Return::void(eax, 4))
}

/// thiscall `(x)`: the fuel and air terms and the throttle; the `eax` is the options block (`layout::fuel_air_terms`).
pub(super) fn fuel_air_terms(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let eax = on_engine(ctx, r.ecx, |e| layout::fuel_air_terms(e, r.ecx, f(r.stack[0])));
    Ok(Return::void(eax, 4))
}

/// thiscall `(pedal)`.
pub(super) fn set_scratch08(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    on_engine(ctx, r.ecx, |e| engine_pedal(e, r.ecx, r.stack[0]));
    Ok(Return::void(0, 4))
}

/// `CEngine::SetScratch08`'s body (0x50eb90) on the engine's store: the pedal's word. Also the VCU's direct call.
pub(crate) fn engine_pedal(e: &mut layout::EngineView, engine: u32, pedal: u32) {
    let mut this = layout::EngineOwn::load(e, engine);
    engine::set_pedal(&mut this.pedal, pedal);
    this.store(e, engine);
}

/// thiscall `(row, s*)`, st0.
pub(super) fn exhaust_blend(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [row, q, ..] = r.stack;
    Ok(Return::float(on_engine(ctx, r.ecx, |e| layout::exhaust_blend_at(e, r.ecx, row, q)), 8))
}

/// `E43EngineSim::Sim80D0` (thiscall, `x`): the reset of the table entry `x`, run by the original.
const SIM80D0: u32 = 0x5180d0;

/// The host calls of `Sim8890`: the car's slot 12 (`S30`, a virtual call on the car) and `MB560` on the walker, whose
/// output is the scratch below the binding's frame (the original's frame held it).
struct Sim8890Calls<'a> {
    mem: Mem,
    host: &'a dyn Host,
    scratch: u32,
}

impl engine::CarWalkHost for Sim8890Calls<'_> {
    fn walk_object(&mut self, car: u32) -> u32 {
        let slot12 = self.mem.u32(self.mem.u32(car) + 4 * 12);
        crate::call_or_trivial(self.host, 0, slot12, car, &[])
    }

    /// `a` is the car's velocity (`Sim8E20`'s radiator airflow): `MB560` copies it on the solids' store.
    fn transform_point(&mut self, a: u32, b: u32) -> [f32; 4] {
        let c = self.scratch;
        crate::entities::call_op(self.host, 0, MB560, self.mem.u32(WALKER), &[a, b, c]);
        self.mem.f32s(c)
    }
}

/// The host calls of `Sim8E20`: the walk (`Sim8890Calls`), `Sim80D0` (through `Sim8D90`), and the three sinks. The sinks run
/// the original code.
struct Sim8E20Calls<'a> {
    mem: Mem,
    host: &'a dyn Host,
    walk: Sim8890Calls<'a>,
}

impl engine::IntakeResetHost for Sim8E20Calls<'_> {
    fn intake_state_reset(&mut self, this: u32, x: u32) {
        self.host.call(SIM80D0, this, &[x]);
    }
}

impl engine::CarWalkHost for Sim8E20Calls<'_> {
    fn walk_object(&mut self, car: u32) -> u32 {
        self.walk.walk_object(car)
    }

    fn transform_point(&mut self, a: u32, b: u32) -> [f32; 4] {
        self.walk.transform_point(a, b)
    }
}

impl engine::EngineStepHost for Sim8E20Calls<'_> {
    fn emit(&mut self, signal: u32) {
        let manager = self.mem.u32(SIGNALS) + 0x6050;
        let target = crate::ngp::Call::EngineNoise.address();
        self.host.call(target, manager, &[signal]);
    }

    fn graph_push(&mut self, graph: u32, value: f32) {
        self.host.call(GRAPH_PUSH, graph, &[value.to_bits()]);
    }

    /// `E01Log(6, 5, "FloorS32: Invalid argument %f\n", value)`: the double of `value` on the stack after the format.
    fn floor_log(&mut self, value: f32) {
        let bits = (value as f64).to_bits();
        self.host.call(E01LOG, 0, &[6, 5, FLOOR_S32_FORMAT, bits as u32, (bits >> 32) as u32]);
    }
}

/// `Sim8CF0`'s host: the emit runs as the original does.
struct Sim8CF0Calls<'a> {
    host: &'a dyn Host,
    /// The exhaust signal emitter selected for this run.
    emit: u32,
}

impl engine::SignalHost for Sim8CF0Calls<'_> {
    fn signal(&mut self, obj: u32, arg: u32) -> u32 {
        self.host.call(self.emit, obj, &[arg]).eax
    }
}

/// thiscall `()`, eax: the emit's eax (`layout::signal_update`).
pub(super) fn sim8cf0(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let host = ctx.host;
    let emit = crate::ngp::Call::Exhaust.address();
    let eax = on_engine(ctx, r.ecx, |e| layout::signal_update(&mut Sim8CF0Calls { host, emit }, e, r.ecx));
    Ok(Return::void(eax, 0))
}

/// thiscall `(a, b)`, st0: one engine step (`layout::engine_step`); the walk and the sinks are host calls.
pub(crate) fn engine_step(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    crate::ngp::only()?;
    let walk = Sim8890Calls { mem: ctx.mem.clone(), host: ctx.host, scratch: r.esp - 0x800 };
    let mut calls = Sim8E20Calls { mem: ctx.mem.clone(), host: ctx.host, walk };
    let tuning = engine_tuning(ctx);
    let [a, b, ..] = r.stack.map(f);
    let st0 = on_engine(ctx, r.ecx, |e| layout::engine_step(&mut calls, e, r.ecx, a, b, &tuning));
    Ok(Return::float(st0 as f64, 8))
}

/// thiscall `(x)`, void: the car's damper step (`layout::heat_step`), its walk through the host.
pub(super) fn sim8890(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    crate::ngp::only()?;
    let mut calls = Sim8890Calls { mem: ctx.mem.clone(), host: ctx.host, scratch: r.esp - 0x800 };
    let tuning = engine_tuning(ctx);
    let eax = on_engine(ctx, r.ecx, |e| layout::heat_step(&mut calls, e, r.ecx, f(r.stack[0]), &tuning));
    Ok(Return::void(eax, 4))
}

/// The host calls of `Sim8D90`: `Sim80D0` runs in the original and writes the entry it resets.
struct Sim43Calls<'a> {
    host: &'a dyn Host,
}

impl engine::IntakeResetHost for Sim43Calls<'_> {
    fn intake_state_reset(&mut self, this: u32, x: u32) {
        self.host.call(SIM80D0, this, &[x]);
    }
}

/// thiscall, no arguments: `1 - cos` of the engine's value into the lane at `+0x24` (`layout::pedal_cosine`).
pub(super) fn sim_ebb0(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    on_engine(ctx, r.ecx, |e| layout::pedal_cosine(e, r.ecx));
    Ok(Return::void(0, 0))
}

/// thiscall `(pos, x)`, st0: the arc position of `pos` on the drive line (`layout::project_point`).
pub(super) fn sample_big(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [pos, x, ..] = r.stack;
    let point = ctx.mem.f32s(pos);
    Ok(Return::float(on_stage(ctx.host, &ctx.mem, |line| layout::project_point(line, r.ecx, point, f(x))) as f64, 8))
}

/// thiscall `(b, s, out)`, eax = `out`: `out = this + (b - this) * s` (`Vec4::lerp`, the `E11_VecLerp` of the drive line).
pub(super) fn vec_lerp(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [b, s, out, ..] = r.stack;
    let a = Vec4::load(&ctx.mem, r.ecx);
    let v = a.lerp(Vec4::load(&ctx.mem, b), f(s));
    v.store(&mut ctx.mem, out);
    Ok(Return::void(out, 12))
}

/// thiscall `(x, y)`, x87 return: the table entry `x` takes the key `y` (`layout::intake_entry_update`).
pub(super) fn sim8d90(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [x, y, ..] = r.stack;
    let host = ctx.host;
    let v = on_engine(ctx, r.ecx, |e| layout::intake_entry_update(&mut Sim43Calls { host }, e, r.ecx, x, y));
    Ok(Return::float(v, 8))
}

/// thiscall `()`, eax.
pub(super) fn stall_guard(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    Ok(Return::void(engine_running(ctx, r.ecx) as u32, 0))
}

/// `CEngine::SimECF0` on `engine`: whether it runs.
pub(crate) fn engine_running(ctx: &mut Ctx, engine: u32) -> bool {
    let stall_speed = engine_tuning(ctx).stall_speed;
    on_engine(ctx, engine, |e| {
        let objects = layout::EngineObjects::at(e, engine);
        let state = layout::EngineState::load(e, objects.state);
        let options = layout::EngineOptions::load(e, objects.options);
        engine::stall_guard(state.speed, &options, stall_speed)
    })
}

/// thiscall `(x, q*)`.
pub(super) fn torque_curve(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [x, q_at, ..] = r.stack;
    on_engine(ctx, r.ecx, |e| layout::torque_curve(e, r.ecx, f(x), q_at));
    Ok(Return::void(0, 8))
}

/// thiscall `(i)`: the table entry `i` and its roll (`layout::intake_state_reset`). The binding declines slots past the four
/// stall terms, as `SimD870` does.
pub(super) fn intake_state_reset(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let i = r.stack[0];
    if i >= 4 {
        return Err(Decline::Unsupported("entry outside 0..4"));
    }
    let tuning = engine_tuning(ctx);
    Ok(Return::void(on_engine(ctx, r.ecx, |e| layout::intake_state_reset(e, r.ecx, i, &tuning)), 4))
}

/// thiscall `(x)`: the damper and load stage (`layout::turbo_energy_step`). Returns the eax the original leaves.
pub(super) fn turbo_energy_step(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    crate::ngp::only()?;
    let [x, ..] = r.stack;
    let tuning = engine_tuning(ctx);
    Ok(Return::void(on_engine(ctx, r.ecx, |e| layout::turbo_energy_step(e, r.ecx, f(x), &tuning)), 4))
}

/// thiscall `(setup)`, `ret 4`: `CDrive::M8770` (0x508770), the setup into the drive's definition
/// (`layout::drive_setup_into_def`).
pub(super) fn drive_setup(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let def = layout::DriveObjects::at(&ctx.mem, r.ecx).def;
    layout::drive_setup_into_def(&mut ctx.mem, def, r.stack[0]);
    Ok(Return::void(0, 4))
}

/// thiscall `(worker)`, `ret 4`: `E39_Link3AB0` (0x503ab0), the worker object stored and enabled. Leaves eax the
/// argument.
pub(super) fn link_worker(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    layout::link_worker(&mut ctx.mem, r.ecx, r.stack[0]);
    Ok(Return::void(r.stack[0], 4))
}
