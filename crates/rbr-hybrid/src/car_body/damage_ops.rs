//! The damage handler's functors, impacts and ops (`rbr_physics::car_body::damage`, `layout::dmg_*`).

use rbr_physics::car_body::damage;
use rbr_physics::car_body::layout;
use rbr_physics::mem::Mem;

use super::car_calls::scratch;
use crate::collision::E01_VEC4_COPY_FROM;
use crate::{Ctx, Decline, Host, Regs, Return};

/// thiscall `(a1..a8)`, `ret 0x20`: `CCarDamageHandler::ImpactScale` (slot 2) calls slot 1 with the length scaled by 0.08.
pub(super) fn damage_impact_scale(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let mut calls = ImpactHost { host: ctx.host, sp: r.esp - 0x24 };
    let eax = layout::impact_scale(&mut calls, &ctx.mem, r.ecx, r.stack[..8].try_into().unwrap());
    Ok(Return::void(eax, 0x20))
}

/// thiscall `(a, b, c, tag)` (or `(a, b, tag)`), `ret 0x10`: a damage functor (`layout::dmg_*`) on the handler (`ecx`).
fn damage_functor(ctx: &mut Ctx, r: &Regs, run: fn(&mut Mem, u32, [u32; 4]) -> u32) -> Result<Return, Decline> {
    let eax = run(&mut ctx.mem, r.ecx, r.stack[..4].try_into().unwrap());
    Ok(Return::void(eax, 0x10))
}

pub(super) fn gearbox_wear(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    damage_functor(ctx, r, layout::damage_radiator)
}

pub(super) fn grow_cooling_wear(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    damage_functor(ctx, r, layout::grow_cooling_wear)
}

pub(super) fn grow_zone(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    damage_functor(ctx, r, layout::bend_top_mount)
}

pub(super) fn clear_triple(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    damage_functor(ctx, r, layout::fail_driveshaft)
}

pub(super) fn clear_head_gasket(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    damage_functor(ctx, r, layout::crack_windshield)
}

pub(super) fn clear_windshield(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    damage_functor(ctx, r, layout::chip_windshield)
}

pub(super) fn dmg_9a0(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    damage_functor(ctx, r, layout::fail_brake_circuit)
}

pub(super) fn dmg_7b0(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    damage_functor(ctx, r, layout::fail_differential)
}

pub(super) fn dmg_aa0(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    damage_functor(ctx, r, layout::break_hinged_body)
}

pub(super) fn dmg_b20(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    damage_functor(ctx, r, layout::break_hinged_lock)
}

pub(super) fn drain_word(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    damage_functor(ctx, r, layout::drain_oil_pressure)
}

pub(super) fn dmg_930(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    damage_functor(ctx, r, layout::fail_water_pump)
}

pub(super) fn dmg_820(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    damage_functor(ctx, r, layout::fail_starter)
}

pub(super) fn cap_word(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    damage_functor(ctx, r, layout::cap_clutch)
}

pub(super) fn dmg_bb0(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    damage_functor(ctx, r, layout::break_lamps_kind_2)
}

pub(super) fn dmg_cc30(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    damage_functor(ctx, r, layout::break_lamps_kind_1)
}

pub(super) fn fail_suspension_arm(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    damage_functor(ctx, r, layout::break_lamps_kind_4)
}

pub(super) fn fail_steering_rod(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    damage_functor(ctx, r, layout::break_lamps_kind_3)
}

pub(super) fn fail_hub(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    damage_functor(ctx, r, layout::break_lamps_kind_5)
}

pub(super) fn fail_brake_disc(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    damage_functor(ctx, r, layout::break_lamps_kind_6)
}

pub(super) fn fail_water_pump(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    damage_functor(ctx, r, layout::fail_hydraulics)
}

pub(super) fn grow_intercooler_wear(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    damage_functor(ctx, r, layout::leak_oil_cooler)
}

pub(super) fn dmg_d020(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    damage_functor(ctx, r, layout::wear_gearbox)
}

pub(super) fn set_body_state(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    damage_functor(ctx, r, layout::set_body_state)
}

pub(super) fn zero2f0(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    damage_functor(ctx, r, layout::second_zero)
}

pub(super) fn dmg_d300(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    damage_functor(ctx, r, layout::always_zero)
}

/// The handler's slot 1 at the impact's stack: the callee's return slot is the thunk's arguments' end (`sp`).
struct ImpactHost<'a> {
    host: &'a dyn Host,
    sp: u32,
}

impl layout::ImpactCalls for ImpactHost<'_> {
    fn call(&mut self, target: u32, this: u32, args: [u32; 8]) -> u32 {
        self.host.call_at(self.sp, target, this, &args).eax
    }
}

/// The impact path's calls (`layout::DamageHitCalls`), each at the original's stack.
struct DamageHit<'a>(&'a dyn Host);

impl layout::DamageHitCalls for DamageHit<'_> {
    fn functor(&mut self, sp: u32, function: u32, this: u32, args: [u32; 4]) -> u32 {
        self.0.call_at(sp, function, this, &args).eax
    }

    fn copy_vec4(&mut self, sp: u32, dst: u32, src: u32) {
        self.0.call_at(sp, E01_VEC4_COPY_FROM, dst, &[src]);
    }

    fn emit(&mut self, sp: u32, emit: u32, emitter: u32, signal: u32) {
        self.0.call_at(sp, emit, emitter, &[signal]);
    }
}

/// thiscall `(a1, a2, a3, dmg, a5, src_a, src_b, _)`, `ret 0x20`: `E36DamageOps::Slot1`, the impact on a zone.
pub(super) fn damage_slot1(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let args = r.stack[..8].try_into().unwrap();
    let tuning = crate::ngp::damage_tuning(ctx);
    let eax = layout::apply_impact(&mut DamageHit(ctx.host), &mut ctx.mem, r.ecx, r.esp, args, tuning);
    Ok(Return::void(eax, 0x20))
}

/// thiscall `(a, b, c, tag)`, `ret 0x10`: `RingStore`, the speech functor.
pub(super) fn ring_store(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let args = r.stack[..4].try_into().unwrap();
    let eax = layout::ring_store(&mut DamageHit(ctx.host), &mut ctx.mem, r.ecx, r.esp, args);
    Ok(Return::void(eax, 0x10))
}

/// The calls `E36DamageOps::InitTables` and `Slot4` make to the original signal and vector helpers.
struct DamageOps<'a>(&'a dyn Host);

impl layout::DamageOpsCalls for DamageOps<'_> {
    fn call(&mut self, _mem: &mut Mem, callee: u32, this: u32, args: &[u32]) {
        self.0.call(callee, this, args);
    }
}

/// thiscall `(index, scale, value, tag)`, `ret 0x10`: append an entry to the selected table.
pub(super) fn add_damage_functor(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [index, scale, value, tag, ..] = r.stack;
    let added = damage::add_functor(&mut ctx.mem, r.ecx, index as i32, scale, value, tag);
    Ok(Return::void(u32::from(added), 0x10))
}

/// thiscall `()`: rebuild the fixed E36 functor tables.
pub(super) fn damage_ops_init_tables(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let mut calls = DamageOps(ctx.host);
    layout::init_damage_ops_tables(&mut calls, &mut ctx.mem, r.ecx);
    Ok(Return::void(0, 0))
}

/// thiscall `()`: `CCar::UpdateMrr`'s damage slot 4.
pub(super) fn damage_level_fractions(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let mut calls = DamageOps(ctx.host);
    layout::damage_level_fractions(&mut calls, &mut ctx.mem, r.ecx, scratch(r));
    Ok(Return::void(0, 0))
}
