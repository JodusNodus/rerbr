//! Bindings of the tyre ports (`rbr_physics::tyre`, exe 0x528980 and 0x531ba0-0x535d60). The ABIs are the
//! original's: the `CScratch` methods are thiscall, `TyreClamp`, `FloorS32`
//! and `TyreFrictionCurve` stdcall, `Locate` cdecl. Their callees are vector and CRT helpers the ports compute
//! (they write only the caller's stack), so no binding calls the host. eax is given where the original returns
//! a value; elsewhere it is a scratch value no caller reads, given as 0.
//!
//! Each binding decodes the registers, reads the blocks its pointers name, runs its entry's sequence
//! (`rbr_physics::tyre::layout::entries`) on the tyre ([`on_tyre`]) and writes the entry's outputs through the
//! pointers after the tyre's state, so a pointer into the tyre's own words keeps its write. An entry that declines
//! with `Outside` leaves the tyre as it was, so the original runs.

use rbr_physics::mem::Mem;
use rbr_physics::physics_core::layout::signal_emit;
use rbr_physics::tyre::layout::{self, entries, TyreView};
use rbr_physics::tyre::tyres::{Tyre, TyreId};

use crate::{Binding, Ctx, Decline, Host, Regs, Return, SignalCallees};

const fn bind(addr: u32, name: &'static str, run: crate::BindingFn) -> Binding {
    crate::bind("tyre", addr, name, run)
}

pub static BINDINGS: [Binding; 11] = [
    bind(0x532470, "CScratch::SetStateField", set_state_field),
    bind(0x533490, "CScratch::SetState48", set_state48),
    bind(0x534e70, "CScratch::BlendTable", blend_table),
    bind(0x534f80, "CMultiSignal1<CSigTyreNoise>::Emit", signal_emit::<0x534f80>),
    bind(0x535890, "CScratch::EmitNoise", emit_noise),
    bind(0x5358c0, "CScratch::RelaxCount", relax_count),
    bind(0x52db10, "NTweak::CBase::GetName", name_object),
    bind(0x5326b0, "TyreTail_Get10", tyre_tail10),
    bind(0x5326c0, "TyreTail_Get14", tyre_tail14),
    bind(0x5360c0, "CScratch::Step", step),
    bind(0x534760, "CScratch::CopyDefFrom", copy_def_from),
];

fn f(bits: u32) -> f32 {
    f32::from_bits(bits)
}

/// A frame port's `Outside` declines, so the original runs on memory as it was.
fn frame<T>(r: Result<T, layout::Outside>) -> Result<T, Decline> {
    r.map_err(|_| Decline::Unsupported("index outside the frame"))
}

/// Runs `run` on the tyre whose scratch is at `scratch` and its store, the typed storage's (`Host::typed`).
pub(crate) fn on_tyre<T>(ctx: &mut Ctx, scratch: u32, run: impl FnOnce(&mut TyreView, TyreId) -> T) -> T {
    on_tyre_in(ctx.host, &mut ctx.mem, scratch, run)
}

/// [`on_tyre`] on `host` and `mem`.
fn on_tyre_in<T>(host: &dyn Host, mem: &mut Mem, scratch: u32, run: impl FnOnce(&mut TyreView, TyreId) -> T) -> T {
    let typed = host.typed();
    let mut tyres = typed.car.tyres.borrow_mut();
    let id = tyres.id_of(scratch).unwrap_or_else(|| panic!("the tyre at {scratch:#x} is not the car's"));
    run(&mut tyres.view(mem).with_defs(Some(&typed.defs)), id)
}

/// `read` of the tyre whose scratch is at `scratch` as a step leaves it at this point. For the readers outside the tyre's bindings (NGP's step wrapper and thermal words,
/// the ground probe, the pure host's tyre term).
pub(crate) fn read_tyre<T>(host: &dyn Host, scratch: u32, read: impl FnOnce(&Tyre) -> T) -> T {
    let typed = host.typed();
    let tyres = typed.car.tyres.borrow();
    let id = tyres.id_of(scratch).unwrap_or_else(|| panic!("the tyre at {scratch:#x} is not the car's"));
    read(tyres.tyres.get(id))
}

/// Whether the step holds the tyre whose scratch is at `scratch`.
pub(crate) fn holds_tyre(host: &dyn Host, scratch: u32) -> bool {
    host.typed().car.tyres.borrow().id_of(scratch).is_some()
}

/// Runs `run` (a call whose callees read the tyre's words at their addresses) with the tyre whose scratch is at
/// `scratch` on the image: a held tyre is lowered before it and lifted after it, through memory without poison (the
/// words are the tyre's).
fn on_image<T>(ctx: &mut Ctx, scratch: u32, run: impl FnOnce(&mut Ctx) -> T) -> T {
    let Some(id) = ctx.host.typed().car.tyres.borrow().id_of(scratch) else {
        return run(ctx);
    };
    let mut image = unpoisoned(&ctx.mem);
    ctx.host.typed().car.tyres.borrow().lower_tyre(id, &mut image);
    let out = run(ctx);
    ctx.host.typed().car.tyres.borrow_mut().lift_tyre(id, &unpoisoned(&ctx.mem));
    out
}

/// `mem` sharing its storage, without the poison of a check build.
fn unpoisoned(mem: &Mem) -> Mem {
    #[cfg(feature = "poison")]
    return mem.unpoisoned();
    #[cfg(not(feature = "poison"))]
    mem.clone()
}

/// The stiffnesses `ComputeOutputs` passes through from its record `src`: `None` when the record is the scratch's own
/// words (`this+0x20`), which the tyre's state holds.
pub(crate) fn limit_stiffnesses(ctx: &Ctx, scratch: u32, src: u32) -> Option<[f32; 2]> {
    (src != scratch + 0x20).then(|| layout::load_limit_stiffnesses(&ctx.mem, src))
}

/// thiscall `(flag)`.
fn set_state_field(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    on_tyre(ctx, r.ecx, |v, t| entries::set_active(v, t, r.stack[0]));
    Ok(Return::void(0, 4))
}

/// thiscall `(v)`.
fn set_state48(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    on_tyre(ctx, r.ecx, |v, t| entries::set_penetration_depth(v, t, f(r.stack[0])));
    Ok(Return::void(0, 4))
}

/// thiscall `(i, j, a, b, o1, o2)`, eax 1.
fn blend_table(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [i, j, a, b, o1, o2, ..] = r.stack;
    let out = frame(on_tyre(ctx, r.ecx, |v, t| entries::surface_friction(v, t, [i, j], [f(a), f(b)])))?;
    layout::store_surface_friction(&mut ctx.mem, o1, o2, out);
    Ok(Return::void(1, 0x18))
}

/// thiscall `(dt)`: RelaxStep, then the counter at state+0x1c.
fn relax_count(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let ngp = crate::ngp::thermal(ctx, r.ecx)?;
    on_tyre(ctx, r.ecx, |v, t| entries::relax_and_count(v, t, f(r.stack[0]), &ngp));
    Ok(Return::void(0, 4))
}

/// thiscall `()`: the name object (eax).
fn name_object(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    Ok(Return::void(layout::load_name_object(&ctx.mem, r.ecx), 0))
}

/// thiscall `()`: the setup pointer at +0x10 (eax).
fn tyre_tail10(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    Ok(Return::void(layout::load_def_pointer(&ctx.mem, r.ecx), 0))
}

/// thiscall `()`: the state pointer at +0x14 (eax).
fn tyre_tail14(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    Ok(Return::void(layout::load_state_pointer(&ctx.mem, r.ecx), 0))
}

/// thiscall with 16 stack words (`a` to `p`), `ret 0x40`: [`step_in`].
fn step(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    step_in(ctx.host, &mut ctx.mem, r.ecx, &r.stack[..16])?;
    Ok(Return::void(0, 0x40))
}

/// `Step` on the tyre whose scratch is at `scratch`, with its sixteen stack words `stack`, on `host` and `mem`: the
/// arguments are loaded once, `Step` runs on the tyre, and its outputs are stored after the tyre.
pub(crate) fn step_in(host: &dyn Host, mem: &mut Mem, scratch: u32, stack: &[u32]) -> Result<(), Decline> {
    let ngp = crate::ngp::thermal_in(host, mem, scratch)?;
    // A tyre model also reads the wheel's surface classes (read only).
    let surface = if crate::ngp::tyre_model_set() {
        rbr_physics::wheel::layout::surface_classes(mem, scratch)
    } else {
        ([rbr_physics::tyre::model::UNKNOWN_CLASS; 2], 0.0)
    };
    let (args, ptrs) = layout::load_step(mem, stack);
    let out = crate::ngp::with_tyre_model(scratch, surface, |model| {
        frame(on_tyre_in(host, mem, scratch, |v, t| entries::step(v, t, &args, &ngp, model)))
    })?;
    layout::store_step(mem, &ptrs, &out);
    Ok(())
}

/// thiscall `(src)`: the setup copied into the def, the state's copies, and `DamperUpdateB` (in the port).
fn copy_def_from(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let copies = layout::copy_setup_into_def(&mut ctx.mem, r.ecx, r.stack[0]);
    on_tyre(ctx, r.ecx, |v, t| entries::adopt_setup(v, t, copies));
    Ok(Return::void(0, 4))
}

/// thiscall `(sig)`: `CMultiSignal1<CSigTyreNoise>::Emit` and the other instances of the template (the tire-particle one),
/// the physics core's emit with the connections through the host; `EMIT` is the instance. A count out of range
/// declines (the original traps).
pub(crate) fn signal_emit<const EMIT: u32>(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    crate::decline_emit_trap(&ctx.mem, r.ecx)?;
    signal_emit::emit(&mut ctx.mem, &mut SignalCallees(ctx.host, crate::emit_name(EMIT)), r.ecx, r.stack[0]);
    Ok(Return::void(0, 4))
}

/// thiscall: `CScratch::EmitNoise`, the tyre-noise signal emitted with the scratch's signal block, which the emit's
/// connections read at its address (so a held tyre is on the image around it, [`on_image`]). Declines like
/// `signal_emit` when the emit would trap. Under NGP the emit is NGP's wrapper (`ngp::original_for`), which the host
/// runs; eax is what it leaves.
fn emit_noise(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    crate::decline_emit_trap(&ctx.mem, layout::noise_signal(&ctx.mem))?;
    on_image(ctx, r.ecx, |ctx| {
        let target = crate::ngp::Call::TyreNoise.address();
        if target != 0x534F80 {
            // NGP's emit wrapper (0x100ee970): the host runs it, as `layout::emit_noise` runs the emit.
            let signal = layout::noise_signal(&ctx.mem);
            let called = ctx.host.call(target, signal, &[r.ecx + 0x420]);
            ctx.mem.set_u32(r.ecx + 0x424, 0);
            return Ok(Return::void(called.eax, 0));
        }
        layout::emit_noise(&mut ctx.mem, &mut SignalCallees(ctx.host, crate::emit_name(0x534f80)), r.ecx);
        Ok(Return::void(0, 0))
    })
}
