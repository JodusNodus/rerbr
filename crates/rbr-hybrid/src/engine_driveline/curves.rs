//! The `E39_*` row and table helpers, `E12_SampleX` and the drive line's lerps and blends.

use crate::stage::on_stage;
use rbr_physics::engine_driveline::{drive_curve, drive_worker, layout, sim};
use rbr_physics::mem::Mem;
use rbr_physics::stage::store::StageStore;

use super::{f, held_ring, held_wheel, on_drive, E12_SAMPLE_X};
use crate::{Ctx, Decline, Regs, Return};
use rbr_physics::engine_driveline::store::{DriveStore, EngineTables};
use rbr_physics::store::{Raw, Value};

/// stdcall `(v, p)`, st0: the ring at `p` takes `v`, and the mean is returned: a held drive's typed ring where `p` is
/// its, else the image's.
pub(super) fn ring_average(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let (v, p) = (f(r.stack[0]), r.stack[1]);
    let mean = match held_ring(ctx.host, p) {
        Some(drive) => on_drive(ctx, drive, |d| {
            let mut ring = d.ratio_ring();
            let mean = drive_worker::ring_average(&mut ring, v);
            d.store_ratio_ring(&ring);
            mean
        }),
        None => {
            let mut ring = layout::RatioRing::load(&ctx.mem, p);
            let mean = drive_worker::ring_average(&mut ring, v);
            ring.store(&mut ctx.mem, p);
            mean
        }
    };
    Ok(Return::float(mean as f64, 8))
}

/// stdcall `(table, a, b, c, n)`, st0: the table lerp `E39_TabE00` (ten knots at `table`).
pub(super) fn unsigned_table_lerp(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let table = ctx.mem.f32_table(r.stack[0], 10);
    let v = drive_curve::unsigned_table_lerp(
        &table,
        r.stack[1] as i32,
        r.stack[2] as i32,
        r.stack[3] as i32,
        r.stack[4] as i32,
    );
    Ok(Return::float(v, 0x14))
}

/// The curve state at `b` (a wheel's block of a drive state) for `run`, and written back: a held drive's typed wheel,
/// else the image's.
fn on_curve_state<T>(ctx: &mut Ctx, b: u32, run: impl FnOnce(&Mem, &mut layout::CurveState) -> T) -> T {
    match held_wheel(ctx.host, b) {
        Some((drive, k)) => on_drive(ctx, drive, |d| {
            let mut state = d.curve_state(k);
            let out = run(d.mem(), &mut state);
            d.store_curve_state(k, &state);
            out
        }),
        None => {
            let mut state = layout::CurveState::load(&ctx.mem, b);
            let out = run(&ctx.mem, &mut state);
            state.store(&mut ctx.mem, b);
            out
        }
    }
}

/// stdcall `(dt, a, b, c, n, s)`, eax: `E39_WorkerF60`, the row `a` and the state `b`; eax is 1 when the counter counted.
pub(super) fn count_down(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [dt, a, b, c, n, s, ..] = r.stack;
    let ok = on_curve_state(ctx, b, |mem, state| {
        let row = layout::CurveRow::load(mem, a);
        let tables = layout::CurveTables::load(mem, n);
        let block_gain = layout::BrakeBlock::gain(mem, c);
        drive_curve::count_down(f(dt), &row, state, &tables, block_gain, f(s))
    });
    Ok(Return::void(u32::from(ok), 0x18))
}

/// stdcall `(dt, b, c, n, s)`, st0: `E39_Worker4060`, the row the block `c` picks in the def `n`.
pub(super) fn row_value(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [dt, b, c, n, s, ..] = r.stack;
    let v = on_curve_state(ctx, b, |mem, state| row_value_of(mem, state, f(dt), c, n, f(s)));
    Ok(Return::float(v, 0x14))
}

/// `E39_Worker4060` on the curve state `state`, the block `c` and the def `n` (read on `mem`).
pub(crate) fn row_value_of(mem: &Mem, state: &mut layout::CurveState, dt: f32, c: u32, n: u32, s: f32) -> f64 {
    let row = layout::CurveRow::load(mem, layout::curve_row_at(n, layout::BrakeBlock::row_index(mem, c)));
    let tables = layout::CurveTables::load(mem, n);
    let block_gain = layout::BrakeBlock::gain(mem, c);
    drive_curve::row_value(dt, &row, state, &tables, block_gain, s)
}

/// stdcall `(x)`, `ret 4`, st0: `E12_SampleX` (0x475ea0), the drive line's value at `x`. The original asserts outside the
/// table, so the port declines there (before anything is written).
pub(super) fn e12_sample_x(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    match on_stage(ctx.host, &ctx.mem, |line| layout::distance_at(line, f32::from_bits(r.stack[0]))) {
        Some(st0) => Ok(Return::float(st0 as f64, 4)),
        None => Err(Decline::Unsupported("E12_SampleX outside the table (the original asserts)")),
    }
}

/// thiscall `(x)`, st0: `E12_SampleX` on the E12 host word.
pub(super) fn thunk_4f0(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let host = ctx.host;
    let st0 = layout::forward_distance_at(&ctx.mem, r.stack[0], |e12, x| {
        host.call(E12_SAMPLE_X, e12, &[x.to_bits()]).st0 as f32
    });
    Ok(Return::float(st0 as f64, 4))
}

/// thiscall `(idx, t, out*)`.
pub(super) fn blend_b(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [idx, t, out, ..] = r.stack;
    let v = on_stage(ctx.host, &ctx.mem, |line| layout::spline_at(line, r.ecx, idx as i32, f(t)));
    v.store(&mut ctx.mem, out);
    Ok(Return::void(0, 0xc))
}

/// thiscall `(i, t, out)`, `ret 0xc`: `CDriveLine::LerpAt` (0x50a920), which jumps into `E11_VecLerp`. Eax is `out`.
pub(super) fn lerp_at(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [idx, t, out, ..] = r.stack;
    let v = on_stage(ctx.host, &ctx.mem, |line| {
        (idx < line.line_count(r.ecx)).then(|| layout::tangent_lerp(line, r.ecx, idx as i32, f(t)))
    });
    let Some(v) = v else {
        return Err(Decline::Unsupported("drive line index past its count (LerpAt traps)"));
    };
    v.store(&mut ctx.mem, out);
    Ok(Return::void(out, 0xc))
}

/// thiscall `()`, st0.
pub(super) fn last(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    Ok(Return::float(on_stage(ctx.host, &ctx.mem, |line| layout::last_distance(line, r.ecx)) as f64, 0))
}

/// thiscall `(signal*)`. Declines a count over 10000 (the original traps).
pub(super) fn set_drive_line(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    if ctx.mem.u32(r.stack[0]) > 10000 {
        return Err(Decline::Unsupported("drive line count over 10000"));
    }
    layout::set_drive_line(&mut ctx.mem, r.ecx, r.stack[0]);
    Ok(Return::void(0, 4))
}

/// thiscall `(x, out*)`.
pub(super) fn clamp_mix(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [x, out, ..] = r.stack;
    let v = on_stage(ctx.host, &ctx.mem, |line| layout::point_at(line, r.ecx, f(x)));
    v.store(&mut ctx.mem, out);
    Ok(Return::void(0, 8))
}

// The E41 helpers (stdcall).

/// `(a, b_desc*, c_desc*, d, e, out_f*, out_g*, out_h*)`.
pub(super) fn blend_tables_checked(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [a, b_desc, c_desc, d, e, out_f, out_g, out_h, ..] = r.stack;
    let mem = &mut ctx.mem;
    let count = mem.u32(b_desc + 4) as usize;
    let b = mem.blend_records(b_desc, count);
    let c = mem.blend_records(c_desc, count);
    let (vf, vg, vh) = sim::blend_tables_checked(f(a), f(d), f(e), &b, &c);
    mem.set_f32(out_f, vf);
    mem.set_f32(out_g, vg);
    mem.set_u32(out_h, vh);
    Ok(Return::void(0, 0x20))
}

/// `(a, b_desc*, c_desc*, d, e, out_f*, out_g*)`.
pub(super) fn blend_tables(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [a, b_desc, c_desc, d, e, out_f, out_g, ..] = r.stack;
    let mem = &mut ctx.mem;
    let count = mem.u32(b_desc + 4) as usize;
    let b = mem.blend_records(b_desc, count);
    let c = mem.blend_records(c_desc, count);
    let (vf, vg) = sim::blend_tables(f(a), f(d), f(e), &b, &c);
    mem.set_f32(out_f, vf);
    mem.set_f32(out_g, vg);
    Ok(Return::void(0, 0x1c))
}

/// `(keys*, x, out0*, out1*, out2*, out3*)`.
pub(super) fn find_bracket(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [keys, x, o0, o1, o2, o3, ..] = r.stack;
    let keys = ctx.mem.bracket_keys(keys);
    let b = sim::find_bracket(&keys, f(x));
    ctx.mem.store_bracket(&b, [o0, o1, o2, o3]);
    Ok(Return::void(0, 0x18))
}

/// `(table*, x, lo, hi, n)`, st0.
pub(super) fn table_lerp(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [table, x, lo, hi, n, ..] = r.stack;
    let table = ctx.mem.f32_table(table, n as i32 as usize);
    Ok(Return::float(sim::table_lerp(&table, f(x), f(lo), f(hi)), 0x14))
}

/// `(x, out*)`.
pub(super) fn split_unit_fraction(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    sim::split_unit_fraction(f(r.stack[0])).store(&mut ctx.mem, r.stack[1]);
    Ok(Return::void(0, 8))
}

/// `(curve*, x, n)`, st0.
pub(super) fn curve_eta(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [curve, x, n, ..] = r.stack;
    let curve = ctx.mem.curve_knots(curve, n as i32 as usize);
    Ok(Return::float(sim::curve_eta(&curve, f(x)), 0xc))
}

// CEngine.
