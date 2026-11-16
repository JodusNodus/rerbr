//! `CPhysics::StepForward` and its body passes (`Work5500`, `Work5640`, `Work5760`), and the profiler scope and
//! sample enters around them.

use rbr_physics::mem::Mem;
use rbr_physics::physics_core::layout::contact_walk::{NodeHost, SolidHost};
use rbr_physics::physics_core::layout::prof_calls::{EnterHost, ProfHost};
use rbr_physics::physics_core::layout::{self, physics_object as core_physics};
use rbr_physics::store::Raw;

use super::UNLINK;
use crate::rigid_body::{VF_DX, VF_EX};
use crate::{Ctx, Decline, Host, Regs, Return};

/// The counter: the pure host's without a call ([`Host::query_counter`]).
struct Timer<'a>(&'a dyn Host);

impl ProfHost for Timer<'_> {
    fn counter(&mut self, at: u32) -> u32 {
        self.0.query_counter(at)
    }
}

/// fastcall `(handle)`: `ProfExit`, which ends in `E46_ProfCore5b0`; eax is the start's high word.
pub(super) fn prof_exit(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    // The counter goes to the original's 8-byte local below the entry esp.
    let mut timer = Timer(ctx.host);
    let eax = layout::prof_exit(&mut timer, &mut ctx.mem, r.ecx, r.esp - 8);
    Ok(Return::void(eax, 0))
}

/// The profiler scope of a bound function, as the original's frame runs it: `E46ProfChan::Enter(chan, sample, name, 0)`
/// (ecx the channel, the sample and the name on the stack), the body, then `ProfExit(chan)`. `local` is the 8-byte word
/// the counter goes to (below `ProfExit`'s entry). Returns the eax `ProfExit` leaves, which the function's `ret` keeps.
pub(crate) fn scope(ctx: &mut Ctx, chan: u32, sample: u32, name: u32, local: u32, body: impl FnOnce(&mut Ctx)) -> u32 {
    let mut timer = Timer(ctx.host);
    layout::prof_chan_enter(&mut timer, &mut ctx.mem, chan, sample, name, 0);
    body(ctx);
    layout::prof_exit(&mut timer, &mut ctx.mem, chan, local)
}

/// thiscall `(count)`: `CPhysics::StepForward`. Its body, profiler scope and stack slots follow the original's call
/// order. The ported NTweak setter, Work5500 contact walk and profiler helpers stay in Rust; the other callees are
/// typed host calls and use the call-site ESP recorded in `StepStack`.
pub(super) fn step_forward_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    crate::ngp::only()?;
    let stack = layout::StepStack::new(r.esp);
    layout::seed_step_forward_entry(
        &mut ctx.mem,
        stack,
        r.ebx,
        r.esi,
        r.edi,
        core_physics::STEP_FORWARD_SAMPLE,
        core_physics::STEP_FORWARD_NAME,
    );
    let eax = scope(
        ctx,
        stack.outer_channel(),
        core_physics::STEP_FORWARD_SAMPLE,
        core_physics::STEP_FORWARD_NAME,
        stack.outer_prof_local(),
        |ctx| {
            let mut calls = PhysicsStepCalls { mem: ctx.mem.clone(), host: ctx.host, stack };
            super::on_core(ctx, |core| layout::step_forward(core, &mut calls, r.ecx, r.stack[0], r.ebp, stack));
        },
    );
    Ok(Return::void(eax, 4))
}

/// Typed host calls made by StepForward and the Work5500 body pass, at their original entry stack pointers.
struct PhysicsStepCalls<'a> {
    mem: Mem,
    host: &'a dyn Host,
    stack: layout::StepStack,
}

impl<S: Raw> NodeHost<S> for PhysicsStepCalls<'_> {
    fn force_input(&mut self, mem: &mut S, node: u32) {
        let target = layout::virtual_target(mem, node, 0x5c);
        self.host.call_at(self.stack.work_slot_void(), target, node, &[]);
    }

    fn force_to_accel(&mut self, mem: &mut S, node: u32) {
        let target = layout::virtual_target(mem, node, 0x64);
        self.host.call_at(self.stack.work_slot_void(), target, node, &[]);
    }

    fn integrate(&mut self, mem: &mut S, node: u32, p: f32) {
        layout::seed_body_pass_edi(mem.mem_mut(), self.stack);
        let target = layout::virtual_target(mem, node, 0x6c);
        self.host.call_at(self.stack.work_slot_arg(), target, node, &[p.to_bits()]);
    }

    fn child_walk(&mut self, mem: &mut S, node: u32, p: f32) {
        let target = layout::virtual_target(mem, node, 0x58);
        self.host.call_at(self.stack.work_slot_arg(), target, node, &[p.to_bits()]);
    }
}

impl<S: Raw> core_physics::StepHost<S> for PhysicsStepCalls<'_> {
    fn pre_step(&mut self, entry_sp: u32, target: u32, node: u32, dt: f32) {
        self.host.call_at(entry_sp, target, node, &[dt.to_bits()]);
    }

    fn post_bodies(&mut self, entry_sp: u32, target: u32, node: u32, dt: f32) {
        crate::call_or_trivial(self.host, entry_sp, target, node, &[dt.to_bits()]);
    }

    fn init_pass(&mut self, entry_sp: u32, target: u32, node: u32) {
        self.host.call_at(entry_sp, target, node, &[]);
    }

    fn post_contact(&mut self, entry_sp: u32, target: u32, node: u32) {
        self.host.call_at(entry_sp, target, node, &[]);
    }

    fn end_step(&mut self, entry_sp: u32, target: u32, node: u32) {
        self.host.call_at(entry_sp, target, node, &[]);
    }

    fn telemetry_substep(&mut self, entry_sp: u32, receiver: u32, count: u32) {
        crate::call_or_trivial(self.host, entry_sp, 0x499e60, receiver, &[count]);
    }

    fn collide_window(&mut self, entry_sp: u32, physics: u32, window: f32) {
        self.host.call_at(entry_sp, 0x526d20, physics, &[window.to_bits()]);
    }

    fn penetration_scope(&mut self, stack: layout::StepStack, physics: u32, telemetry: u32) {
        layout::seed_penetration_entry(
            &mut self.mem,
            stack,
            core_physics::PENETRATION_SAMPLE,
            core_physics::PENETRATION_NAME,
        );
        let mut nested = Ctx { mem: self.mem.clone(), host: self.host };
        scope(
            &mut nested,
            stack.penetration_channel(),
            core_physics::PENETRATION_SAMPLE,
            core_physics::PENETRATION_NAME,
            stack.penetration_prof_local(),
            |ctx| {
                ctx.host.call_at(stack.loop_void(), 0x526cc0, physics, &[]);
                let events = layout::step_scratch_events(&ctx.mem, physics);
                crate::call_or_trivial(ctx.host, stack.loop_arg(), 0x499e70, telemetry, &[events]);
                layout::seed_penetration_exit(&mut ctx.mem, stack);
            },
        );
    }

    fn solver_clear_rows(&mut self, entry_sp: u32, solver: u32) {
        self.host.call_at(entry_sp, 0x4fb7f0, solver, &[]);
    }

    fn solver_hit_count(&mut self, entry_sp: u32, solver: u32) -> u32 {
        self.host.call_at(entry_sp, 0x4fbca0, solver, &[]).eax
    }

    fn solver_sweep_events(&mut self, entry_sp: u32, solver: u32) {
        self.host.call_at(entry_sp, 0x4fbc20, solver, &[]);
    }

    fn solver_step(&mut self, entry_sp: u32, solver: u32) {
        self.host.call_at(entry_sp, 0x4fc590, solver, &[]);
    }

    fn solver_pass(&mut self, entry_sp: u32, solver: u32) {
        self.host.call_at(entry_sp, 0x4fca20, solver, &[]);
    }

    fn solver_post(&mut self, entry_sp: u32, target: u32, solver: u32) {
        self.host.call_at(entry_sp, target, solver, &[]);
    }

    fn telemetry_after_solve(&mut self, entry_sp: u32, receiver: u32) {
        crate::call_or_trivial(self.host, entry_sp, 0x499e90, receiver, &[]);
    }

    fn reset_drive_part(&mut self, entry_sp: u32, drive: u32) {
        self.host.call_at(entry_sp, 0x504510, drive, &[]);
    }
}

/// The calls the damper workers make on a collision event's solids, on the original: the slot 0x80 of the solid (its
/// vtable), then `CSolid::VfDx` and `VfEx` with the float.
struct Solids<'a>(&'a dyn Host);

impl SolidHost for Solids<'_> {
    fn takes_part(&mut self, target: u32, solid: u32) -> u32 {
        self.0.call(target, solid, &[]).eax
    }

    fn back_step(&mut self, solid: u32, f: f32) {
        self.0.call(VF_DX, solid, &[f.to_bits()]);
    }

    fn advance_rest(&mut self, solid: u32, f: f32) {
        self.0.call(VF_EX, solid, &[f.to_bits()]);
    }
}

/// thiscall `(f)`: `CPhysics::Work5640`. The original never reads its float argument.
pub(super) fn advance_contact_bodies(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    layout::advance_contact_bodies(&mut ctx.mem, &mut Solids(ctx.host), r.ecx);
    Ok(Return::void(0, 4))
}

/// thiscall `(window)`: `CPhysics::Work5760`.
pub(super) fn advance_contact_bodies_remainder(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    layout::advance_contact_bodies_remainder(&mut ctx.mem, &mut Solids(ctx.host), r.ecx, f32::from_bits(r.stack[0]));
    Ok(Return::void(0, 4))
}

/// The sample enter's unlink, through the original.
impl EnterHost for Timer<'_> {
    fn unlink(&mut self, sample: u32) {
        crate::collision::call_grid(self.0, 0, UNLINK, sample, &[]);
    }
}

/// thiscall `(ptr, name, flag)`: `E46ProfChan::Enter`, the channel's sample enter.
pub(super) fn chan_enter(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let mut timer = Timer(ctx.host);
    let [ptr, name, flag, ..] = r.stack;
    let eax = layout::prof_chan_enter(&mut timer, &mut ctx.mem, r.ecx, ptr, name, flag);
    Ok(Return::void(eax, 12))
}
