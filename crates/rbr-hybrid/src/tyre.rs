//! Bindings of the tyre ports (`rbr_physics::tyre`, exe 0x528980 and 0x531ba0-0x535d60). The ABIs are the
//! original's (`harness/vcu_vectors.py` TYRE specs): the `CScratch` methods are thiscall, `TyreClamp`, `FloorS32`
//! and `TyreFrictionCurve` stdcall, `Locate` cdecl. Their callees are vector and CRT helpers the ports compute
//! (they write only the caller's stack), so no binding calls the host. eax is given where the original returns
//! a value; elsewhere it is a scratch value no caller reads, given as 0.
//!
//! Each binding decodes the registers, reads the blocks its pointers name, runs its entry's sequence
//! (`rbr_physics::tyre::layout::entries`) on the tyre ([`on_tyre`]) and writes the entry's outputs through the
//! pointers after the tyre's state, so a pointer into the tyre's own words keeps its write. An entry that declines
//! with `Outside` leaves the tyre as it was, so the original runs.

use rbr_physics::math::vec::Vec4;
use rbr_physics::mem::Mem;
use rbr_physics::physics_core::layout::signal_emit;
use rbr_physics::tyre::layout::{self, entries, TyreView};
use rbr_physics::tyre::tyres::{Tyre, TyreId};
use rbr_physics::tyre::{grip, solve};

use crate::{Binding, Ctx, Decline, Host, Regs, Return, SignalCallees};

const fn bind(addr: u32, name: &'static str, run: crate::BindingFn) -> Binding {
    crate::bind("tyre", addr, name, run)
}

pub static BINDINGS: [Binding; 45] = [
    bind(0x528980, "Locate", locate),
    bind(0x531ba0, "CScratch::Sign", sign),
    bind(0x531c70, "CScratch::Friction", friction),
    bind(0x531fc0, "CScratch::Align", align),
    bind(0x5320c0, "CScratch::ComputeOutputs", compute_outputs),
    bind(0x5322e0, "TyreClamp", tyre_clamp),
    bind(0x532470, "CScratch::SetStateField", set_state_field),
    bind(0x532490, "CScratch::Set414", set_414),
    bind(0x533490, "CScratch::SetState48", set_state48),
    bind(0x534e70, "CScratch::BlendTable", blend_table),
    bind(0x534f80, "CMultiSignal1<CSigTyreNoise>::Emit", signal_emit::<0x534f80>),
    bind(0x535890, "CScratch::EmitNoise", emit_noise),
    bind(0x5358c0, "CScratch::RelaxCount", relax_count),
    bind(0x52db10, "NTweak::CBase::GetName", name_object),
    bind(0x5326b0, "TyreTail_Get10", tyre_tail10),
    bind(0x5326c0, "TyreTail_Get14", tyre_tail14),
    bind(0x5325f0, "FloorS32", floor_s32),
    bind(0x532a30, "CScratch::ComputeSlip", compute_slip),
    bind(0x532b60, "CScratch::SolveForces", solve_forces),
    bind(0x532f10, "CScratch::FillOutputs", fill_outputs),
    bind(0x533080, "CScratch::WriteOutputs", write_outputs),
    bind(0x5334d0, "TyreFrictionCurve", friction_curve),
    bind(0x533640, "CScratch::SolveDirs", solve_dirs),
    bind(0x533760, "CScratch::ClampGrip", clamp_grip),
    bind(0x5338a0, "CScratch::Accumulate", accumulate),
    bind(0x533ae0, "CScratch::SolveA", solve_a),
    bind(0x533bf0, "CScratch::SolveB", solve_b),
    bind(0x533d40, "CScratch::SolveC", solve_c),
    bind(0x533e80, "CScratch::SolveD", solve_d),
    bind(0x534090, "CScratch::HeatStep", heat_step),
    bind(0x534480, "CScratch::TableStep", table_step),
    bind(0x5345a0, "CScratch::RelaxStep", relax_step),
    bind(0x5346f0, "CScratch::SplitSlip", split_slip),
    bind(0x535040, "CTyreMatData::CopyFrom", copy_from),
    bind(0x5351a0, "CScratch::SolveDispatch", solve_dispatch),
    bind(0x5352a0, "CScratch::SolvePair", solve_pair),
    bind(0x535310, "CScratch::StepInner", step_inner),
    bind(0x5358f0, "CScratch::ContactPatch", contact_patch),
    bind(0x535d60, "CScratch::SolvePatch", solve_patch),
    bind(0x5360c0, "CScratch::Step", step),
    bind(0x530c50, "CDef::CDef", cdef_ctor),
    bind(0x531500, "CState::CState", state_ctor),
    bind(0x5327d0, "CScratch::Init", scratch_init),
    bind(0x5328d0, "CScratch::CScratch", scratch_ctor),
    bind(0x534760, "CScratch::CopyDefFrom", copy_def_from),
];

fn f(bits: u32) -> f32 {
    f32::from_bits(bits)
}

/// A frame port's `Outside` declines, so the original runs on memory as it was.
fn frame<T>(r: Result<T, layout::Outside>) -> Result<T, Decline> {
    r.map_err(|_| Decline::Unsupported("index outside the frame"))
}

/// Runs `run` on the tyre whose scratch is at `scratch` and its store: the typed storage's while the host holds the
/// car's tyres (`Host::typed`), else per call, lifted from the image and lowered after `run` (`layout::with_tyre`).
pub(crate) fn on_tyre<T>(ctx: &mut Ctx, scratch: u32, run: impl FnOnce(&mut TyreView, TyreId) -> T) -> T {
    if let Some(typed) = ctx.host.typed() {
        let mut tyres = typed.car.tyres.borrow_mut();
        if let Some(id) = tyres.id_of(scratch) {
            return run(&mut tyres.view(&ctx.mem).with_defs(Some(&typed.defs)), id);
        }
    }
    layout::with_tyre(&mut ctx.mem, scratch, run)
}

/// `read` of the tyre whose scratch is at `scratch` as a step leaves it at this point: the typed storage's while the
/// host holds it, else the image's. For the readers outside the tyre's bindings (NGP's step wrapper and thermal words,
/// the ground probe, the pure host's tyre term).
pub(crate) fn read_tyre<T>(host: &dyn Host, mem: &Mem, scratch: u32, read: impl FnOnce(&Tyre) -> T) -> T {
    if let Some(typed) = host.typed() {
        let tyres = typed.car.tyres.borrow();
        if let Some(id) = tyres.id_of(scratch) {
            return read(tyres.tyres.get(id));
        }
    }
    read(&layout::TyreImage::of(mem, scratch).lift(mem))
}

/// Runs `run` (a call whose callees read the tyre's words at their addresses) with the tyre whose scratch is at
/// `scratch` on the image: a held tyre is lowered before it and lifted after it, through memory without poison (the
/// words are the tyre's).
fn on_image<T>(ctx: &mut Ctx, scratch: u32, run: impl FnOnce(&mut Ctx) -> T) -> T {
    let Some(id) = ctx.host.typed().and_then(|typed| typed.car.tyres.borrow().id_of(scratch)) else {
        return run(ctx);
    };
    let mut image = unpoisoned(&ctx.mem);
    ctx.host.typed().expect("held").car.tyres.borrow().lower_tyre(id, &mut image);
    let out = run(ctx);
    ctx.host.typed().expect("held").car.tyres.borrow_mut().lift_tyre(id, &unpoisoned(&ctx.mem));
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

/// cdecl `(table, x, n, out)`: the segment at `out` is the hint, and the result is stored back there.
fn locate(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [table, x, n, out, ..] = r.stack;
    entries::locate(&mut ctx.mem, table, f(x), n, out).map_err(|_| Decline::Unsupported("locate outside the table"))?;
    Ok(Return::void(0, 0))
}

/// thiscall `()`, st0.
fn sign(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    Ok(Return::float(on_tyre(ctx, r.ecx, |v, t| entries::rolling_torque(v, t)), 0))
}

/// thiscall `(k)`, st0.
fn friction(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    Ok(Return::float(on_tyre(ctx, r.ecx, |v, t| entries::hold(v, t, f(r.stack[0]))), 4))
}

/// thiscall `(a*, b*)`; eax is 1 when it aligned.
fn align(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let (normal, axle) = (Vec4::load(&ctx.mem, r.stack[0]), Vec4::load(&ctx.mem, r.stack[1]));
    let aligned = on_tyre(ctx, r.ecx, |v, t| entries::build_contact_frame(v, t, normal, axle));
    Ok(Return::void(aligned as u32, 8))
}

/// thiscall with 17 stack words: scales at 1, 2, 4 and 5, `src` at 6, the output pointers at 7 to 16.
fn compute_outputs(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    crate::ngp::only()?;
    let s = &r.stack;
    let stiffnesses = limit_stiffnesses(ctx, r.ecx, s[6]);
    let (shares, scales) = ([f(s[1]), f(s[2])], [f(s[4]), f(s[5])]);
    let out = frame(on_tyre(ctx, r.ecx, |v, t| entries::friction_limits(v, t, stiffnesses, shares, scales)))?;
    layout::store_friction_limits(&mut ctx.mem, std::array::from_fn(|i| s[7 + i]), &out);
    Ok(Return::void(0, 0x44))
}

/// stdcall `(a, b, c, out*)`.
fn tyre_clamp(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [a, b, c, out, ..] = r.stack;
    entries::clamp_deflection(&mut ctx.mem, f(a), f(b), f(c), out);
    Ok(Return::void(0, 0x10))
}

/// thiscall `(value)`: the wear table pointer (`CScratch::Set414`).
fn set_414(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    layout::store_wear_table(&mut ctx.mem, r.ecx, r.stack[0]);
    Ok(Return::void(0, 4))
}

/// stdcall `(x, table, count)`, st0.
fn floor_s32(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [x, table, count, ..] = r.stack;
    Ok(Return::float(entries::table_lerp(&ctx.mem, f(x), table, count), 0xc))
}

/// thiscall `(dir*, spin)`.
fn compute_slip(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [dir, spin, ..] = r.stack;
    let velocity = [ctx.mem.f32(dir), ctx.mem.f32(dir + 4), ctx.mem.f32(dir + 8)];
    on_tyre(ctx, r.ecx, |v, t| entries::compute_slip(v, t, velocity, f(spin)));
    Ok(Return::void(0, 8))
}

/// thiscall `(k, out1*, out2*)`: `out2` is read (the asm adds into it), and both are written.
fn solve_forces(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [k, out1, out2, ..] = r.stack;
    let torque_in = Vec4::load(&ctx.mem, out2);
    let (force, torque) = on_tyre(ctx, r.ecx, |v, t| entries::solve_forces(v, t, f(k), torque_in));
    force.store(&mut ctx.mem, out1);
    torque.store(&mut ctx.mem, out2);
    Ok(Return::void(0, 0xc))
}

/// thiscall `(src*, pair*, divisor)`.
fn fill_outputs(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [src, pair, divisor, ..] = r.stack;
    let velocity = Vec4::load(&ctx.mem, src);
    let debug_words: [u32; 3] = std::array::from_fn(|k| ctx.mem.u32(pair + 4 * k as u32));
    on_tyre(ctx, r.ecx, |v, t| entries::fill_noise_signal(v, t, velocity, debug_words, f(divisor)));
    Ok(Return::void(0, 0xc))
}

/// thiscall `(out*)`.
fn write_outputs(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let block = on_tyre(ctx, r.ecx, |v, t| entries::write_output_block(v, t));
    layout::store_output_block(&mut ctx.mem, r.stack[0], &block);
    Ok(Return::void(0, 4))
}

/// stdcall with 13 words: values 0 to 8, out pointers 9 and 10, the table and its count; st0.
fn friction_curve(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    Ok(Return::float(entries::friction_curve(&mut ctx.mem, &r.stack), 0x34))
}

/// thiscall `(a, b, c, out0*, out1*, out2*)`.
fn solve_dirs(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let s = &r.stack;
    let out = on_tyre(ctx, r.ecx, |v, t| entries::slip_directions(v, t, [f(s[0]), f(s[1]), f(s[2])]));
    for (at, value) in [s[3], s[4], s[5]].into_iter().zip(out) {
        ctx.mem.set_f32(at, value);
    }
    Ok(Return::void(0, 0x18))
}

/// thiscall `(s, a, b, c, d, out_a*, out_b*)`.
fn clamp_grip(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [s, a, b, c, d, out_a, out_b, ..] = r.stack;
    let g = grip::GripInput {
        normal_force: f(s),
        static_friction: f(a),
        kinetic_friction: f(b),
        lateral_stiffness: f(c),
        torsional_stiffness: f(d),
    };
    let (deflection, torsion) = on_tyre(ctx, r.ecx, |v, t| entries::limit_to_friction_circle(v, t, g));
    ctx.mem.set_f32(out_a, deflection);
    ctx.mem.set_f32(out_b, torsion);
    Ok(Return::void(0, 0x1c))
}

/// thiscall `()`.
fn accumulate(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    on_tyre(ctx, r.ecx, |v, t| entries::soil_drag(v, t));
    Ok(Return::void(0, 0))
}

/// The contact solvers: thiscall `(p*, qb*, r*, ...)`, the contact read from the three blocks and its indentation and
/// forces stored back after the tyre.
fn with_contact(ctx: &mut Ctx, r: &Regs, run: impl FnOnce(&mut solve::Contact)) {
    let [p, qb, rr, ..] = r.stack;
    let blocks = layout::load_contact_blocks(&ctx.mem, p, qb, rr);
    let contact = on_tyre(ctx, r.ecx, |v, t| entries::with_contact(v, t, blocks, run));
    layout::store_contact_blocks(&mut ctx.mem, p, qb, &contact);
}

/// thiscall `(p*, qb*, r*, idx)`.
fn solve_a(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    with_contact(ctx, r, |c| solve::solve_tread_only(c, r.stack[3] as i32));
    Ok(Return::void(0, 0x10))
}

fn solve_b(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    with_contact(ctx, r, |c| solve::solve_both(c, r.stack[3] as i32));
    Ok(Return::void(0, 0x10))
}

fn solve_c(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    with_contact(ctx, r, |c| solve::solve_neither(c, r.stack[3] as i32));
    Ok(Return::void(0, 0x10))
}

fn solve_d(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    with_contact(ctx, r, |c| solve::solve_carcass_only(c, r.stack[3] as i32));
    Ok(Return::void(0, 0x10))
}

/// thiscall `(p*, qb*, r*, f1, f2, a6)`.
fn solve_dispatch(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [_, _, _, f1, f2, a6, ..] = r.stack;
    with_contact(ctx, r, |c| solve::solve_contact(c, f1 != 0, f2 != 0, a6 as i32));
    Ok(Return::void(0, 0x18))
}

/// thiscall `(a, b, c)`.
fn heat_step(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [a, b, c, ..] = r.stack;
    let ngp = crate::ngp::thermal(ctx, r.ecx)?;
    frame(on_tyre(ctx, r.ecx, |v, t| entries::heat_step(v, t, f(a), f(b), f(c), &ngp)))?;
    Ok(Return::void(0, 0xc))
}

/// thiscall `(idx, x)`.
fn table_step(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [idx, x, ..] = r.stack;
    let ngp = crate::ngp::thermal(ctx, r.ecx)?;
    frame(on_tyre(ctx, r.ecx, |v, t| entries::relax_sector(v, t, idx, f(x), &ngp)))?;
    Ok(Return::void(0, 8))
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

/// thiscall `(dt)`.
fn relax_step(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let ngp = crate::ngp::thermal(ctx, r.ecx)?;
    on_tyre(ctx, r.ecx, |v, t| entries::relax(v, t, f(r.stack[0]), &ngp));
    Ok(Return::void(0, 4))
}

/// thiscall `()`.
fn split_slip(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    on_tyre(ctx, r.ecx, |v, t| entries::weigh_models(v, t));
    Ok(Return::void(0, 0))
}

/// thiscall `(src*)`: the material words copied from `src` into the block at `this`, a held tyre's when it is one's
/// (`this` its scratch's `+0x1a0`).
fn copy_from(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let material = layout::load_material(&ctx.mem, r.stack[0]);
    let scratch = r.ecx.wrapping_sub(layout::MATERIAL);
    let held = ctx.host.typed().is_some_and(|typed| typed.car.tyres.borrow().id_of(scratch).is_some());
    if held {
        on_tyre(ctx, scratch, |v, t| entries::adopt_material(v, t, &material));
    } else {
        layout::store_material(&mut ctx.mem, r.ecx, &material);
    }
    Ok(Return::void(0, 4))
}

/// thiscall `()`.
fn solve_pair(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    on_tyre(ctx, r.ecx, |v, t| entries::limit_sticking(v, t));
    Ok(Return::void(0, 0))
}

/// thiscall `(rec*, vec3*, vec4*, out3*, out4*, out5*)`.
fn step_inner(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    crate::ngp::only()?;
    let s = &r.stack;
    // The model reads the scratch's own words, which are the record's when the record is the scratch's (at +0x20).
    if s[0] != r.ecx + 0x20 {
        return Err(Decline::Unsupported("record is not the scratch's"));
    }
    let out = frame(on_tyre(ctx, r.ecx, |v, t| entries::patch_forces(v, t)))?;
    layout::store_patch_forces(&mut ctx.mem, std::array::from_fn(|i| s[1 + i]), &out);
    Ok(Return::void(0, 0x18))
}

/// thiscall `(out*)`; eax is 1 when solved, and the value the port returns is stored at `out`.
fn contact_patch(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let solved = on_tyre(ctx, r.ecx, |v, t| entries::contact_patch(v, t));
    if let Some(value) = solved {
        ctx.mem.set_f32(r.stack[0], value);
    }
    Ok(Return::void(solved.is_some() as u32, 4))
}

/// thiscall `()`.
fn solve_patch(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    crate::ngp::only()?;
    frame(on_tyre(ctx, r.ecx, |v, t| entries::solve_sliding(v, t)))?;
    Ok(Return::void(0, 0))
}

/// thiscall with 16 stack words (`a` to `p`), `ret 0x40`. The arguments are loaded once, `Step` runs on the tyre, and
/// its outputs are stored after the tyre.
pub(crate) fn step(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let ngp = crate::ngp::thermal(ctx, r.ecx)?;
    // A tyre model also reads the wheel's surface classes (read only).
    let surface = if crate::ngp::tyre_model_set() {
        rbr_physics::wheel::layout::surface_classes(&ctx.mem, r.ecx)
    } else {
        ([rbr_physics::tyre::model::UNKNOWN_CLASS; 2], 0.0)
    };
    let (args, ptrs) = layout::load_step(&ctx.mem, &r.stack[..16]);
    let out = crate::ngp::with_tyre_model(r.ecx, surface, |model| {
        frame(on_tyre(ctx, r.ecx, |v, t| entries::step(v, t, &args, &ngp, model)))
    })?;
    layout::store_step(&mut ctx.mem, &ptrs, &out);
    Ok(Return::void(0, 0x40))
}

/// thiscall `()`: the CDef's defaults, the object in eax.
fn cdef_ctor(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    crate::ngp::only()?;
    layout::construct_def(&mut ctx.mem, r.ecx);
    Ok(Return::void(r.ecx, 0))
}

/// thiscall `(a, b, c)`: the state's defaults, the object in eax; the three words are not stored.
fn state_ctor(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    layout::construct_state(&mut ctx.mem, r.ecx);
    Ok(Return::void(r.ecx, 0xC))
}

/// thiscall `()`: the scratch's sub-block defaults; the original leaves `this` in eax.
fn scratch_init(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    layout::init_scratch(&mut ctx.mem, r.ecx);
    Ok(Return::void(r.ecx, 0))
}

/// thiscall `(def, state, record)`: the scratch's constructor, the object in eax.
fn scratch_ctor(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [def, state, record, ..] = r.stack;
    layout::construct_scratch(&mut ctx.mem, r.ecx, def, state, record);
    Ok(Return::void(r.ecx, 0xC))
}

/// thiscall `(src)`: the setup copied into the def, the state's copies, and `DamperUpdateB` (in the port).
fn copy_def_from(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let copies = layout::copy_setup_into_def(&mut ctx.mem, r.ecx, r.stack[0]);
    on_tyre(ctx, r.ecx, |v, t| entries::adopt_setup(v, t, copies));
    Ok(Return::void(0, 4))
}

/// thiscall `(sig)`: `CMultiSignal1<CSigTyreNoise>::Emit` and the other instances of the template (the tire-particle one),
/// the physics core's emit with the connections in the emulator; `EMIT` is the instance. A count out of range
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
