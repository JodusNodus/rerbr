//! Bindings of the VCU ports (`rbr_physics::vcu`, exe 0x536880-0x5380a0). The ABIs are the original's
//! (thiscall `this` in ecx, the bytes `ret` pops). Each binding decodes the
//! registers and runs its entry's sequence (`rbr_physics::vcu::layout`). The accessors and setters on the drive, the
//! engine, the car and the lights, and the slip query, run direct on the held state (`VcuCallees`); the per-call path
//! calls the originals through the host (`VcuHost`).

use std::cell::Ref;

use rbr_physics::car_body::layout::{self as car_layout, slot as car_slot};
use rbr_physics::engine_driveline::layout::{self as driveline, DriveView, EngineView};
use rbr_physics::engine_driveline::{drive, store::DriveStore};
use rbr_physics::mem::Mem;
use rbr_physics::store::typed::Typed;
use rbr_physics::vcu::layout::{self, Unit, VcuCalls, VcuView};
use rbr_physics::vcu::store::VcuStore;

use crate::{Binding, Ctx, Decline, Host, Regs, Return};

pub const TABLE_BLEND: u32 = 0x536950;
pub const M6840: u32 = 0x506840;
pub(crate) const SET_DIRTY: u32 = 0x52f450; // NTweak::CBase::SetDirty (the gear block's +0x564)
pub const LOOKUP_PAIR_TABLE: u32 = 0x536880;
pub const LOCK_MAX: u32 = 0x5376e0;
pub const DIFF_LOCKS: u32 = 0x537b70;
pub const REV_LIMIT_CUT: u32 = 0x536ee0;
pub const CHECK_SLIP: u32 = 0x537110;
pub const GEARBOX_UPDATE: u32 = 0x537160;
pub const LAUNCH_CLUTCH: u32 = 0x5375b0;
pub const STEP: u32 = 0x537f50;
pub const STEP_BASIC: u32 = 0x5380a0;
pub const FLAG_DISPATCH: u32 = 0x537a80;
pub const READ_PACKET: u32 = 0x537f20;

/// The original callees the bindings call.
pub const GET_4520: u32 = 0x504520; // CDrive: engine speed (rad/s)
pub const GET_4540: u32 = 0x504540; // CDrive: the diff-lock speed key
pub const GET_15F8: u32 = 0x4e6770; // CCar: car speed
pub const SET_4570: u32 = 0x504570; // CDrive: diff lock (channel, value)
pub const SET_4820: u32 = 0x504820; // CDrive: clutch slip
pub const SET_STATE4: u32 = 0x50d340; // CEngine: the four cut flags
pub const SET_SCRATCH08: u32 = 0x50eb90; // CEngine: pedal
pub const SET_1E40: u32 = 0x4f1e40; // Lights: brake light
pub const SET_1ED0: u32 = 0x4f1ed0; // Lights: neutral light
pub const QUERY_SLIP: u32 = 0x536fe0; // E50_QuerySlip (fastcall on the scratch)

const fn bind(addr: u32, name: &'static str, run: crate::BindingFn) -> Binding {
    crate::bind("vcu", addr, name, run)
}

pub static BINDINGS: [Binding; 16] = [
    bind(TABLE_BLEND, "E49Proxy::VcuTableBlend", table_blend),
    bind(M6840, "CDrive::M6840", set_commanded_gear),
    bind(LOOKUP_PAIR_TABLE, "NPhysics::LookupPairTable", lookup_pair_table),
    bind(LOCK_MAX, "CScratchSim::LockMax", lock_max),
    bind(DIFF_LOCKS, "CScratchSim::DiffLocks", diff_locks),
    bind(REV_LIMIT_CUT, "CScratchSim::RevLimitCut", rev_limit_cut),
    bind(CHECK_SLIP, "CScratchSim::CheckSlip", check_slip),
    bind(GEARBOX_UPDATE, "CScratchSim::GearboxUpdate", gearbox_update),
    bind(LAUNCH_CLUTCH, "CScratchSim::LaunchClutch", launch_clutch),
    bind(STEP, "CScratchSim::Step", step_full),
    bind(STEP_BASIC, "CScratchSim::StepBasic", step_basic),
    bind(0x536f70, "CScratchSim::NopA", nop_a),
    bind(FLAG_DISPATCH, "CScratchSim::FlagDispatch", flag_dispatch),
    bind(READ_PACKET, "CScratchSim::ReadPacket", read_packet),
    bind(QUERY_SLIP, "CVehicleControlUnit::E50_QuerySlip", query_slip),
    bind(0x537c90, "CScratchSim::CopyDef", copy_def),
];

/// `CScratchSim::NopA(dt)` (0x536f70, thiscall, `ret 4`): does nothing; eax is the caller's.
fn nop_a(_ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    Ok(Return::void(r.eax, 4))
}

/// The original callees of the VCU's entries, through the host. The binding's entry `esp` places the two calls the
/// original makes at a known depth (the slip query and `SetDirty`); the slip query is on the binding's scratch.
pub(crate) struct VcuHost<'a> {
    pub host: &'a dyn Host,
    pub esp: u32,
    pub scratch: u32,
}

impl VcuHost<'_> {
    fn get_f32(&self, addr: u32, this: u32) -> f32 {
        self.host.call(addr, this, &[]).st0 as f32
    }
}

impl VcuCalls for VcuHost<'_> {
    fn engine_speed(&mut self, drive: u32) -> f32 {
        self.get_f32(GET_4520, drive)
    }

    fn lock_speed(&mut self, drive: u32) -> f32 {
        self.get_f32(GET_4540, drive)
    }

    fn car_speed(&mut self, car: u32) -> f32 {
        self.get_f32(GET_15F8, car)
    }

    fn set_diff_lock(&mut self, drive: u32, channel: u32, value: f32) -> u32 {
        self.host.call(SET_4570, drive, &[channel, value.to_bits()]).eax
    }

    fn set_clutch_slip(&mut self, drive: u32, value: f32) -> u32 {
        self.host.call(SET_4820, drive, &[value.to_bits()]).eax
    }

    fn set_cut_flags(&mut self, engine: u32, flags: [u32; 4]) -> u32 {
        self.host.call(SET_STATE4, engine, &flags).eax
    }

    fn set_engine_pedal(&mut self, engine: u32, value: f32) {
        self.host.call(SET_SCRATCH08, engine, &[value.to_bits()]);
    }

    fn set_brake_light(&mut self, lights: u32, on: bool) {
        self.host.call(SET_1E40, lights, &[on as u32]);
    }

    fn set_neutral_light(&mut self, lights: u32, on: bool) {
        self.host.call(SET_1ED0, lights, &[on as u32]);
    }

    /// In the original at FlagDispatch's esp: three pushes and the return slot below the entry.
    fn query_slip(&mut self) -> bool {
        self.host.call_at(self.esp - 0x10, QUERY_SLIP, self.scratch, &[]).eax != 0
    }
}

/// The VCU's callees on the typed state the host holds over a step: the held drive's, engine's and car's run here, on
/// their typed state with the values as arguments, each counted as a run ([`Host::ran_direct`]: the call memo keeps no
/// call around them). They read nothing of the stack but their arguments, decline nowhere and run at the same address
/// with or without NGP, and no later code reads the words the host call leaves on the stack (`notes/physics.md` "Store
/// seam", direct calls). The light setters run on the image (their words are no typed state's), the slip query on the
/// held unit with its getters here. Another car's callees go through the host ([`VcuHost`]), as everything does on the
/// per-call path.
pub(crate) struct VcuCallees<'a> {
    host: VcuHost<'a>,
    held: Option<(Ref<'a, Typed>, Mem)>,
}

impl VcuCallees<'_> {
    /// `run` on the drive at `drive` when the step holds it.
    fn drive<T>(&mut self, drive: u32, run: impl FnOnce(&mut DriveView) -> T) -> Option<T> {
        let (typed, mem) = self.held.as_mut().filter(|(t, _)| t.car.drive.is(drive))?;
        self.host.host.ran_direct();
        Some(run(&mut typed.car.drive.view(mem).with_defs(Some(&typed.defs))))
    }

    /// `run` on the engine at `engine` when the step holds it.
    fn engine<T>(&mut self, engine: u32, run: impl FnOnce(&mut EngineView) -> T) -> Option<T> {
        let (typed, mem) = self.held.as_mut().filter(|(t, _)| t.car.engine.is(engine))?;
        self.host.host.ran_direct();
        Some(run(&mut typed.car.engine.view(mem).with_defs(Some(&typed.defs))))
    }

    /// `run` on the image when the step holds this unit (so `lights` is its car's): the light setters' words (the
    /// light block, its value headers and change words) are no typed state's, written where the binding writes them.
    fn lights<T>(&mut self, run: impl FnOnce(&mut Mem) -> T) -> Option<T> {
        let (_, mem) = self.held.as_mut().filter(|(t, _)| t.car.vcu.is(self.host.scratch))?;
        self.host.host.ran_direct();
        Some(run(mem))
    }
}

impl VcuCalls for VcuCallees<'_> {
    fn engine_speed(&mut self, drive: u32) -> f32 {
        self.drive(drive, |d| drive::engine_speed(&d.state())).unwrap_or_else(|| self.host.engine_speed(drive))
    }

    fn lock_speed(&mut self, drive: u32) -> f32 {
        self.drive(drive, |d| drive::drive_speed_key(&d.state())).unwrap_or_else(|| self.host.lock_speed(drive))
    }

    fn car_speed(&mut self, car: u32) -> f32 {
        let held = self.held.as_ref().and_then(|(t, _)| t.car.own.read(car, car_slot::forward_speed));
        match held {
            Some(speed) => {
                self.host.host.ran_direct();
                speed
            }
            None => self.host.car_speed(car),
        }
    }

    fn set_diff_lock(&mut self, drive: u32, channel: u32, value: f32) -> u32 {
        self.drive(drive, |d| driveline::set_diff_lock(d, channel, value))
            .unwrap_or_else(|| self.host.set_diff_lock(drive, channel, value))
    }

    fn set_clutch_slip(&mut self, drive: u32, value: f32) -> u32 {
        self.drive(drive, |d| crate::engine_driveline::clutch_slip(d, value))
            .map_or_else(|| self.host.set_clutch_slip(drive, value), |()| 0)
    }

    fn set_cut_flags(&mut self, engine: u32, flags: [u32; 4]) -> u32 {
        self.engine(engine, |e| driveline::set_cut_flags(e, engine, flags))
            .unwrap_or_else(|| self.host.set_cut_flags(engine, flags))
    }

    fn set_engine_pedal(&mut self, engine: u32, value: f32) {
        if self.engine(engine, |e| crate::engine_driveline::engine_pedal(e, engine, value.to_bits())).is_none() {
            self.host.set_engine_pedal(engine, value);
        }
    }

    fn set_brake_light(&mut self, lights: u32, on: bool) {
        if self.lights(|mem| car_layout::set_brake_light(mem, lights, on as u32)).is_none() {
            self.host.set_brake_light(lights, on);
        }
    }

    fn set_neutral_light(&mut self, lights: u32, on: bool) {
        if self.lights(|mem| car_layout::set_neutral_light(mem, lights, on as u32)).is_none() {
            self.host.set_neutral_light(lights, on);
        }
    }

    /// The VCU's own query on the held unit (a second view of its `Cell`, the outer FlagDispatch's still open), its
    /// getters through `self`, as the binding runs it.
    fn query_slip(&mut self) -> bool {
        let Some((typed, mem)) = self.held.as_ref().filter(|(t, _)| t.car.vcu.is(self.host.scratch)) else {
            return self.host.query_slip();
        };
        let (typed, mut mem) = (Ref::clone(typed), mem.clone());
        self.host.host.ran_direct();
        let view = typed.car.vcu.view(&mut mem, Some(&typed.car.drive)).with_defs(Some(&typed.defs));
        layout::query_slip(&view, self)
    }
}

/// The callees of the VCU binding entered with `r`: direct on the typed state the host holds, else through the host.
fn calls<'a>(ctx: &Ctx<'a>, r: &Regs) -> VcuCallees<'a> {
    let host = ctx.host;
    VcuCallees { host: VcuHost { host, esp: r.esp, scratch: r.ecx }, held: host.typed().map(|t| (t, ctx.mem.clone())) }
}

fn f(bits: u32) -> f32 {
    f32::from_bits(bits)
}

/// Runs `run` on the unit whose scratch is `scratch` (the original's `ecx`): in the typed storage the host holds over a
/// step ([`crate::Host::typed`]), else (another car's, or no step holds it) lifted from the image into a store of its
/// own and lowered after it (the per-call path).
pub(crate) fn on_vcu<T>(ctx: &mut Ctx, scratch: u32, run: impl FnOnce(&mut VcuView) -> T) -> T {
    if let Some(typed) = ctx.host.typed() {
        if typed.car.vcu.is(scratch) {
            return run(&mut typed.car.vcu.view(&mut ctx.mem, Some(&typed.car.drive)).with_defs(Some(&typed.defs)));
        }
    }
    layout::with_vcu(&mut ctx.mem, scratch, run)
}

/// The car at `car`'s VCU state as a step leaves it at this point: the typed storage's while the host holds it, else
/// the image's. For the readers outside the VCU's bindings (NGP's engine and drive wrappers).
pub(crate) fn unit_of(host: &dyn Host, mem: &Mem, car: u32) -> Unit {
    let scratch = rbr_physics::car_body::layout::scratch_at(car);
    if let Some(typed) = host.typed() {
        if typed.car.vcu.is(scratch) {
            return typed.car.vcu.unit.get();
        }
    }
    layout::VcuImage::of(mem, scratch).lift(mem)
}

/// stdcall `(table, x, lo, hi, n)`, st0. The VCU always passes 11 knots; the port's index math is checked for
/// counts from 2.
fn table_blend(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [table, x, lo, hi, n, ..] = r.stack;
    if !(2..=64).contains(&n) {
        return Err(Decline::Unsupported("knot count outside 2..=64"));
    }
    Ok(Return::float(layout::table_blend(&ctx.mem, table, f(x), f(lo), f(hi), n), 0x14))
}

/// stdcall `(table, x, n)`, st0.
fn lookup_pair_table(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [table, x, n, ..] = r.stack;
    if !(1..=64).contains(&n) {
        return Err(Decline::Unsupported("record count outside 1..=64"));
    }
    Ok(Return::float(layout::lookup_pair_table(&ctx.mem, table, f(x), n), 0xc))
}

/// thiscall `(t1, t2, t3, f)`, st0: the largest of the throttle, brake and speed maps.
fn lock_max(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [t1, t2, t3, x, ..] = r.stack;
    let pedals = on_vcu(ctx, r.ecx, |vcu| vcu.scratch().pedals);
    Ok(Return::float(layout::lock_max(&ctx.mem, pedals, [t1, t2, t3], f(x)) as f64, 0x10))
}

/// thiscall `()`: the three diff locks, sent to the drive's `Set4570`.
fn diff_locks(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let mut calls = calls(ctx, r);
    let eax = on_vcu(ctx, r.ecx, |vcu| layout::diff_locks(vcu, &mut calls));
    Ok(Return::void(eax, 0))
}

/// fastcall `()`, `ret 0`: `E50_QuerySlip` (0x536fe0), the slip query before a downshift.
fn query_slip(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let mut calls = calls(ctx, r);
    let on = on_vcu(ctx, r.ecx, |vcu| layout::query_slip(vcu, &mut calls));
    Ok(Return::void(on as u32, 0))
}

/// thiscall `()`: the rev-limit cut flags, sent to the engine's `SetState4`.
fn rev_limit_cut(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let margin = crate::ngp::vcu_tuning(ctx).cut_margin;
    let mut calls = calls(ctx, r);
    let eax = on_vcu(ctx, r.ecx, |vcu| layout::rev_limit_cut(vcu, &mut calls, margin));
    Ok(Return::void(eax, 0))
}

/// thiscall `(x)`, eax: the slip gate.
fn check_slip(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let on = on_vcu(ctx, r.ecx, |vcu| layout::check_slip(vcu, f(r.stack[0])));
    Ok(Return::void(on as u32, 4))
}

/// thiscall `(dt)`: the gear logic. eax is 0 only on the flag-off return; Step, the one caller, reloads it at once, so
/// the other exits' residue is not modelled.
fn gearbox_update(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let tuning = crate::ngp::vcu_tuning(ctx);
    let mut calls = calls(ctx, r);
    on_vcu(ctx, r.ecx, |vcu| layout::gearbox_update(vcu, &mut calls, f(r.stack[0]), tuning));
    Ok(Return::void(0, 4))
}

/// thiscall `(dt)`: the launch clutch, sent to the drive's `Set4820` (nothing with the launch mode off).
fn launch_clutch(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let tuning = crate::ngp::vcu_tuning(ctx);
    let mut calls = calls(ctx, r);
    let eax = on_vcu(ctx, r.ecx, |vcu| layout::launch_clutch(vcu, &mut calls, f(r.stack[0]), tuning));
    Ok(Return::void(eax, 4))
}

/// thiscall `(packet)`, `ret 4`: the packet's flags as gear requests. eax is whether anything was requested.
fn flag_dispatch(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let packet = layout::packet(&ctx.mem, r.stack[0]);
    apply_packet(ctx, r, packet)
}

/// thiscall `(packet)`, `ret 4`: copies the packet into the scratch, then tail-jumps to FlagDispatch with the stack
/// as it entered (the asm stores the packet pointer back into its own argument slot, the same word).
fn read_packet(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let packet = layout::packet(&ctx.mem, r.stack[0]);
    on_vcu(ctx, r.ecx, |vcu| layout::read_packet(vcu, packet));
    apply_packet(ctx, r, packet)
}

/// FlagDispatch on `packet`. Its slip query goes through the host to the VCU's own binding, which finds the unit as
/// this run left it: on the per-call path the image, so ReadPacket's writes are lowered before it.
fn apply_packet(ctx: &mut Ctx, r: &Regs, packet: rbr_physics::vcu::layout::Packet) -> Result<Return, Decline> {
    let tuning = crate::ngp::vcu_tuning(ctx);
    let mut calls = calls(ctx, r);
    let on = on_vcu(ctx, r.ecx, |vcu| layout::apply_packet(vcu, &mut calls, packet, &tuning));
    Ok(Return::void(on as u32, 4))
}

/// thiscall `(value)`, `ret 4`: `CDrive::M6840` (0x506840), the drive's own. Its `SetDirty` on the gear request,
/// entered two words below its entry, runs through the host; on a held drive it is skipped (its word is the typed
/// state's) and the port sets the flag.
fn set_commanded_gear(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let (drive, value) = (r.ecx, r.stack[0]);
    let sp = r.esp - 12;
    let skipped = crate::engine_driveline::is_held(ctx.host, drive) && sp != 0;
    if !skipped {
        let request = rbr_physics::engine_driveline::layout::gear_request_at(&ctx.mem, drive);
        ctx.host.call_at(sp, SET_DIRTY, request, &[]);
    }
    crate::engine_driveline::on_drive(ctx, drive, |d| layout::set_commanded_gear(d, drive, value, skipped));
    Ok(Return::void(0, 4))
}

fn step_full(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    run_step(ctx, r, true)
}

fn step_basic(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    run_step(ctx, r, false)
}

/// thiscall `(dt)`: Step (`gear`, with the gear logic) or StepBasic; eax is the state pointer.
fn run_step(ctx: &mut Ctx, r: &Regs, gear: bool) -> Result<Return, Decline> {
    let tuning = crate::ngp::vcu_tuning(ctx);
    let mut calls = calls(ctx, r);
    on_vcu(ctx, r.ecx, |vcu| layout::step(vcu, &mut calls, f(r.stack[0]), gear, tuning));
    Ok(Return::void(layout::state_of(&ctx.mem, r.ecx), 4))
}

/// thiscall `(setup)`, `ret 4`: `CScratchSim::CopyDef` (0x537c90), the VCU setup into the scratch's definition, then
/// the setup's flags into the scratch (`rbr_physics::vcu::layout::copy_def`).
fn copy_def(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let flags = layout::copy_def(&mut ctx.mem, r.ecx, r.stack[0]);
    on_vcu(ctx, r.ecx, |vcu| layout::copy_flags(vcu, flags));
    Ok(Return::void(0, 4))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::Script;

    fn regs(ecx: u32, stack: &[u32]) -> Regs {
        let mut r = Regs { ecx, ..Regs::default() };
        r.stack[..stack.len()].copy_from_slice(stack);
        r
    }

    /// Eleven knots at `at`, 0x10 apart, value at +0x0c: knot k is k * 0.25.
    fn knots(mem: &mut Mem, at: u32) {
        for k in 0..11 {
            mem.map(at + 0x10 * k, &[0; 0x10]);
            mem.set_f32(at + 0x10 * k + 0x0c, k as f32 * 0.25);
        }
    }

    #[test]
    fn table_blend_runs_on_game_memory() {
        let host = Script::default();
        let mut ctx = Ctx { mem: Mem::new(), host: &host };
        knots(&mut ctx.mem, 0x1000);
        let r = regs(0, &[0x1000, 0.25f32.to_bits(), 0, 1f32.to_bits(), 11]);
        assert_eq!(table_blend(&mut ctx, &r), Ok(Return::float(0.625, 0x14)));
        let r = regs(0, &[0x1000, 0, 0, 1f32.to_bits(), 0]);
        assert!(table_blend(&mut ctx, &r).is_err());
    }

    #[test]
    fn gearbox_update_requests_an_upshift_above_the_gears_rpm() {
        // Launch flag on, VCU off: only the rpm rules run. Gear 2 at 500 rad/s is above its 3000 rpm threshold.
        let (this, def, state, car, drive, drive_def, gears) = (0x1000, 0x2000, 0x4000, 0x5000, 0x6000, 0x7000, 0x8000);
        let host = Script { floats: vec![(GET_4520, drive, 500.0), (GET_15F8, car, 10.0)], ..Script::default() };
        let mut ctx = Ctx { mem: Mem::new(), host: &host };
        ctx.mem.map(this, &[0; 0x60]);
        ctx.mem.set_u32(this, def);
        ctx.mem.set_u32(this + 4, state);
        ctx.mem.set_u32(this + 0x40, 1);
        ctx.mem.set_u32(this + 0x58, drive);
        ctx.mem.map(def, &[0; 0xcac]);
        ctx.mem.set_f32(def + 0x18 + 0x20 * 2, 3000.0);
        ctx.mem.set_f32(def + 0x158, 0.25);
        ctx.mem.map(state, &[0; 0x6c]);
        ctx.mem.set_u32(state + 0x68, car);
        // The drive and its state whole: the drive's blocks are its typed state, lifted per access.
        ctx.mem.map(drive, &[0; 0x200]);
        ctx.mem.set_u32(drive + 4, drive_def);
        ctx.mem.set_u32(drive + 8, gears);
        ctx.mem.map(drive_def, &[0; 0x658]);
        ctx.mem.set_u32(drive_def + 0x654, 6);
        ctx.mem.map(gears, &[0; 0x5c0]);
        ctx.mem.set_u32(gears + 0x54c, 2);
        ctx.mem.set_u32(gears + 0x570, 2);
        assert_eq!(gearbox_update(&mut ctx, &regs(this, &[0.01f32.to_bits()])), Ok(Return::void(0, 4)));
        let got = rbr_physics::engine_driveline::layout::DriveRef::of(&ctx.mem, None, drive).gearbox(&ctx.mem);
        assert_eq!((got.gear, got.commanded, got.changed), (2, 3, 1));
        let got = layout::VcuImage::of(&ctx.mem, this).lift(&ctx.mem).state;
        assert_eq!((got.shift_lockout, got.shift_cooldown), (0.25, 0.05));
        assert_eq!(*host.log.borrow(), vec![(GET_4520, drive, vec![]), (GET_15F8, car, vec![])]);
    }

    #[test]
    fn rev_limit_cut_sends_the_flags_to_the_engine() {
        // Engine speed 0: below the limit, no cooldown -> no cut.
        let host = Script::default();
        let mut ctx = Ctx { mem: Mem::new(), host: &host };
        let (this, def, state, engine, drive) = (0x1000, 0x2000, 0x4000, 0x5000, 0x6000);
        ctx.mem.map(this, &[0; 0x60]);
        ctx.mem.set_u32(this, def);
        ctx.mem.set_u32(this + 4, state);
        ctx.mem.set_u32(this + 0x54, engine);
        ctx.mem.set_u32(this + 0x58, drive);
        ctx.mem.map(def, &[0; 0xcac]);
        ctx.mem.set_f32(def + 0x188, 7000.0);
        ctx.mem.map(state, &[0; 0x68]);
        let ret = rev_limit_cut(&mut ctx, &regs(this, &[])).unwrap();
        assert_eq!(ret, Return::void(0, 0));
        let log = host.log.borrow();
        assert_eq!(log[0], (GET_4520, drive, vec![]));
        assert_eq!(log[1].0, SET_STATE4);
        assert_eq!(log[1].1, engine);
        let unit = layout::VcuImage::of(&ctx.mem, this).lift(&ctx.mem);
        let want = rbr_physics::vcu::cut::rev_limit_cut(0.0, &layout::def(&ctx.mem, def), &unit.state, 300.0);
        assert_eq!(log[1].2, want);
    }
}
