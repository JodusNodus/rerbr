//! The quaternion helpers (`E46Quat`), on the solids' store (their operands may be a solid's words).

use rbr_physics::math::quat::{from_quat, mul, normalize, Quat};
use rbr_physics::rigid_body::layout::{SolidView, Vec4};
use rbr_physics::rigid_body::store::RigidStore;
use rbr_physics::store::Raw;

use super::on_solids;
use crate::{Ctx, Decline, Regs, Return};

fn load_quat(s: &SolidView, at: u32) -> Quat {
    let v = s.row(at);
    Quat { x: v.x, y: v.y, z: v.z, w: v.w }
}

fn store_quat(s: &mut SolidView, at: u32, q: Quat) {
    s.store_row(at, Vec4 { x: q.x, y: q.y, z: q.z, w: q.w });
}

/// thiscall `(q*)`: the 3x3 matrix at `this` (rows of three floats) from the quaternion; returns `this`.
pub(super) fn set_from_quat(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    on_solids(ctx, |s| {
        let m = from_quat(load_quat(s, r.stack[0]));
        for (k, v) in m.iter().flatten().enumerate() {
            s.set_f32(r.ecx + 4 * k as u32, *v);
        }
    });
    Ok(Return::void(r.ecx, 4))
}

/// thiscall `(q*, out*)`: `out = this * q`; returns `out`.
pub(super) fn quat_mul(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [q, out, ..] = r.stack;
    on_solids(ctx, |s| {
        let p = mul(load_quat(s, r.ecx), load_quat(s, q));
        store_quat(s, out, p);
    });
    Ok(Return::void(out, 8))
}

/// fastcall `(q*)` in ecx: normalises in place; returns `q`.
pub(super) fn quat_normalize(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    on_solids(ctx, |s| {
        let q = normalize(load_quat(s, r.ecx));
        store_quat(s, r.ecx, q);
    });
    Ok(Return::void(r.ecx, 0))
}

/// thiscall `(out*)`: `out = normalize(this)`; returns `out`.
pub(super) fn quat_norm(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let out = r.stack[0];
    on_solids(ctx, |s| {
        let q = normalize(load_quat(s, r.ecx));
        store_quat(s, out, q);
    });
    Ok(Return::void(out, 4))
}
