//! The vector helpers the collision code calls (`E01Vec4`, `E09Vec4`, `E21Vec3`, `E33Vec4`, `CFloat3`).

use rbr_physics::collision::layout::{self};
use rbr_physics::math::vec::{clear_vec4, Vec3};

use rbr_physics::mem::Mem;

use crate::rigid_body::{on_solids, with_solids};
use crate::{Ctx, Decline, Host, Regs, Return};

/// `E01Vec4::CopyFrom` of `src` to `dst` as a call makes it, its eax (`dst`), counted as a run ([`Host::ran_direct`]):
/// its binding makes no call and reads nothing of the stack but its argument, and its callers read none of the call's
/// words (residue audit, notes/physics.md "Store seam").
pub(crate) fn copy_vec4(host: &dyn Host, mem: &mut Mem, dst: u32, src: u32) -> u32 {
    host.ran_direct();
    with_solids(host, mem, |s| layout::vec4_copy_from(s, dst, src))
}

/// `CFloat3::Length` of the vector at `vector` as a call returns it, run here as [`copy_vec4`].
pub(crate) fn length(host: &dyn Host, mem: &Mem, vector: u32) -> f32 {
    host.ran_direct();
    Vec3::load(mem, vector).length3()
}

/// thiscall on `dst` (`ecx`) with `(src*)`, ret 4: `E01Vec4::CopyFrom` (0x43c5b0), the sixteen bytes of `src` to `dst`.
/// Eax is `dst`.
pub(super) fn vec4_copy_from(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [src, ..] = r.stack;
    Ok(Return::void(on_solids(ctx, |s| layout::vec4_copy_from(s, r.ecx, src)), 4))
}

/// thiscall on the row (`ecx`), no arguments: `E09Vec4::Clear` (0x465d80), the zero row. Eax is the row.
pub(super) fn vec4_clear(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    clear_vec4().store(&mut ctx.mem, r.ecx);
    Ok(Return::void(r.ecx, 0))
}

/// thiscall on the vector (`ecx`) with `(scale, out*)`, ret 8: `CFloat3::Scale` (0x473cd0), `out = this * scale` over
/// four lanes. Eax is `out`.
pub(super) fn float3_scale(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [scale, out, ..] = r.stack;
    on_solids(ctx, |s| layout::scaled(s, r.ecx, out, f32::from_bits(scale)));
    Ok(Return::void(out, 8))
}

/// thiscall on the three-float vector (`ecx`), no arguments: SSE `Length`, returned as a float in st0.
pub(super) fn float3_length(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let vector = Vec3::load(&ctx.mem, r.ecx);
    Ok(Return::float(f64::from(vector.length3()), 0))
}

pub(super) fn vec_cross(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [b, out, ..] = r.stack;
    Ok(Return::void(layout::vec3_cross(&mut ctx.mem, r.ecx, b, out), 8))
}

/// E33Vec4::MinMaxA (0x4e0ae0): thiscall on the pair with `(src, lo, hi)`, ret 0xc; eax is `hi`.
pub(super) fn min_max_a(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [src, lo, hi, ..] = r.stack;
    let eax = layout::bounds_union(&mut ctx.mem, r.ecx, src, lo, hi);
    Ok(Return::void(eax, 12))
}

/// thiscall on the vector (`ecx`), no arguments: `E01Vec4::Normalize` (0x43c6a0), in place. Eax is the vector.
pub(super) fn vec4_normalize(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    Ok(Return::void(layout::vec4_normalize(&mut ctx.mem, r.ecx), 0))
}
