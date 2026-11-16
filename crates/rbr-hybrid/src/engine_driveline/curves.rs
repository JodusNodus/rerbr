//! The `E39_*` row and table helpers, `E12_SampleX` and the drive line's lerps and blends.

use crate::stage::on_stage;
use rbr_physics::engine_driveline::{drive_curve, layout};
use rbr_physics::mem::Mem;

use super::E12_SAMPLE_X;
use crate::{Ctx, Decline, Regs, Return};

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

// The E41 helpers (stdcall).

// CEngine.
