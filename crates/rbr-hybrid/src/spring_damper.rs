//! Bindings of the spring-damper ports (`rbr_physics::spring_damper`, exe 0x52dd70-0x52e8a0). The ABIs are the
//! original's: the leaves are cdecl, the `CSpringDamper` methods thiscall.
//! eax is the residue the original leaves where it is an argument or `this` field; elsewhere it is a scratch value
//! no caller reads, given as 0.

use rbr_physics::spring_damper as sd;
use rbr_physics::spring_damper::layout::{self, StrutView};
use rbr_physics::spring_damper::store::StrutStore;
use rbr_physics::spring_damper::strut::StrutId;

use crate::engine_driveline::SIGNALS;
use rbr_physics::mem::Mem;

use crate::{Binding, Ctx, Decline, Host, Regs, Return};

const fn bind(addr: u32, name: &'static str, run: crate::BindingFn) -> Binding {
    crate::bind("spring_damper", addr, name, run)
}

pub static BINDINGS: [Binding; 4] = [
    bind(0x52e010, "CSpringDamper::ResetOutputs", reset_outputs),
    bind(0x52e060, "CSpringDamper::UpdateAndReset", update_and_reset),
    bind(0x52e090, "CSpringDamper::Step", step),
    bind(0x52dbd0, "CSpringDamper::CopyStateToDef", copy_state_to_def),
];

fn f(bits: u32) -> f32 {
    f32::from_bits(bits)
}

/// Runs `run` on the strut whose controller is `controller` (the original's `ecx`): in the typed storage the host
/// holds ([`crate::Host::typed`]).
pub(crate) fn on_strut<T>(ctx: &mut Ctx, controller: u32, run: impl FnOnce(&mut StrutView, StrutId) -> T) -> T {
    on_strut_in(ctx.host, &mut ctx.mem, controller, run)
}

/// [`on_strut`] on `host` and `mem`.
pub(crate) fn on_strut_in<T>(
    host: &dyn Host,
    mem: &mut Mem,
    controller: u32,
    run: impl FnOnce(&mut StrutView, StrutId) -> T,
) -> T {
    let typed = host.typed();
    let mut struts = typed.car.struts.borrow_mut();
    let id = struts.id_of(controller).unwrap_or_else(|| panic!("the strut at {controller:#x} is not the car's"));
    run(&mut struts.view(mem).with_defs(Some(&typed.defs)), id)
}

/// Whether the step holds the strut whose controller is `controller`.
pub(crate) fn holds_strut(host: &dyn Host, controller: u32) -> bool {
    host.typed().car.struts.borrow().id_of(controller).is_some()
}

/// thiscall `()`: emits the payload at `this+0x1c` through the original's signal manager, then resets it. eax is
/// the state pointer. The payload is also handed to the host as a typed damper event for the game.
fn reset_outputs(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    // The signal manager's damper signal.
    let signal = ctx.mem.u32(SIGNALS) + 0xc2b8;
    let target = crate::ngp::Call::DamperSignal.address();
    ctx.host.call(target, signal, &[r.ecx + 0x1c]);
    let emitted = on_strut(ctx, r.ecx, |store, strut| sd::strut::reset_outputs(store, strut));
    ctx.host.emit_damper(r.ecx, emitted);
    Ok(Return::void(ctx.mem.u32(r.ecx + 8), 0))
}

/// thiscall `()`: `UpdateForces`, then `ResetOutputs`' reset without the emit. The wheel's setup update
/// (`E51Wheel::M3D030`) tail-jumps here. eax is the owner wheel's id (the reset signal's).
fn update_and_reset(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let owner_id = on_strut(ctx, r.ecx, |store, strut| {
        sd::strut::update_forces(store, strut);
        sd::strut::reset_outputs(store, strut);
        store.controller(strut).signal.owner_id
    });
    Ok(Return::void(owner_id, 0))
}

/// thiscall `(travel, coordinate, velocity, spring_force*, breakpoint*, damping*)`: the three outputs go to the
/// caller's out pointers.
fn step(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [travel, coordinate, velocity, spring_force, breakpoint, damping, ..] = r.stack;
    let ngp = crate::ngp::damper_step(&ctx.mem)?;
    let input = sd::StepInput { travel: f(travel), coordinate: f(coordinate), velocity: f(velocity) };
    let out = on_strut(ctx, r.ecx, |store, strut| sd::strut::step(store, strut, input, ngp));
    ctx.mem.set_f32(spring_force, out.spring_stop_force);
    ctx.mem.set_f32(breakpoint, out.breakpoint_offset);
    ctx.mem.set_f32(damping, out.damping_coefficient);
    Ok(Return::void(0, 0x18))
}

/// thiscall `(setup)`, `ret 4`: `CSpringDamper::CopyStateToDef` (0x52dbd0), the setup into the strut's definition
/// (`layout::apply_setup`).
fn copy_state_to_def(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    layout::apply_setup(&mut ctx.mem, r.ecx, r.stack[0]);
    Ok(Return::void(0, 4))
}
