//! The wheel's step (`E51Wheel::M3DD30`) and the calls around it: the pose refresh, the ground probe, `CWheel::Vf28`'s
//! finish and the strut update.

use rbr_physics::car_body::{accessors::signed_force, layout as car_layout, store::CarStore};
use rbr_physics::engine_driveline::layout::{self as drive_layout, DriveView};
use rbr_physics::math::vec::{clear_vec4, Vec4};
use rbr_physics::mem::Mem;
use rbr_physics::rigid_body::store::RigidStore;
use rbr_physics::spring_damper::{StepInput, StepOutput};
use rbr_physics::tyre::layout::{self as tyre_layout, entries, TyreView};
use rbr_physics::tyre::tyres::TyreId;
use rbr_physics::wheel::layout::{self, CarXform, StrutCalls};

use super::{
    call_wheel, on_wheels, set_dirty_held, Libm, BLEND4, M3AB90, M3AC50, M3AFB0, M3B900, M3D460, M3D520,
    SCRATCH_BELOW_ESP, UNK9290, UNK96E0, UNK97A0, UNK9900,
};
use crate::car_body::on_held_car;
use crate::collision::{RAY_CLEAR, RAY_SEEDS};
use crate::entities::{Op, BODY_COMPOSE};
use crate::rigid_body::with_solids;
use crate::vcu::SET_DIRTY;
use crate::{Ctx, Decline, Host, Regs, Return};
use std::cell::Cell;

/// `E51Wheel::M3B900` (0x53b900), `ret 4`: the wheel's pose refresh for the strut travel `x` (name ours). The
/// prologue's two calls (the lerp `M3AB90` and the turn axis `M3A9E0`, each stored) run here; the rest is
/// `layout::refresh`.
pub(super) fn refresh(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let x = f32::from_bits(r.stack[0]);
    let mut libm = Libm { host: ctx.host, eax: Cell::new(0) };
    let mut calls = Refresh { mem: ctx.mem.clone(), host: ctx.host };
    let eax = on_wheels(ctx, |m| {
        layout::deform_top_mount(m, r.ecx);
        layout::wobble_axle(&mut libm, m, r.ecx);
        layout::refresh(m, &mut calls, r.ecx, x)
    });
    Ok(Return::void(eax, 4))
}

/// The original calls `M3B900` makes, through the host (`mem` shares the image).
struct Refresh<'a> {
    mem: Mem,
    host: &'a dyn Host,
}

impl layout::RefreshCalls for Refresh<'_> {
    fn set_dirty(&mut self, object: u32) {
        if !set_dirty_held(self.host, object) {
            self.host.call(SET_DIRTY, object, &[]);
        }
    }

    fn tyre_def(&mut self, scratch: u32) -> u32 {
        if goes_direct(self.host, crate::tyre::holds_tyre(self.host, scratch)) {
            return tyre_layout::load_def_pointer(&self.mem, scratch);
        }
        self.host.call(0x5326B0, scratch, &[]).eax
    }

    fn refresh_solid(&mut self, wheel: u32) -> u32 {
        let target = self.mem.u32(self.mem.u32(wheel) + 0x40);
        call_wheel(self.host, wheel, 0, target, wheel, &[]).eax
    }
}

/// `E51Wheel::M3D520` (0x53d520), `ret 8`: the ground probe (name ours). Its scratch is the frame of the body's
/// prologue (`S`, see `layout::PROBE_PROLOGUE`), and its callees run below it.
pub(super) fn ground_probe(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [out, normal, ..] = r.stack;
    let frame = ((r.esp - 4) & !0xF) - layout::PROBE_PROLOGUE;
    let mut calls = Ground { mem: ctx.mem.clone(), host: ctx.host, sp: frame - 0x20 };
    let tuning =
        if crate::ngp::is_ngp() { crate::ngp::probe_tuning(ctx) } else { layout::ProbeTuning::stock(&ctx.mem) };
    let eax = on_wheels(ctx, |m| layout::ground_probe(m, &mut calls, r.ecx, frame, out, normal, tuning));
    Ok(Return::void(eax, 8))
}

/// The original callees of the ground probe. `sp` is the first callee's return slot, below the probe's scratch.
struct Ground<'a> {
    mem: Mem,
    host: &'a dyn Host,
    sp: u32,
}

impl Ground<'_> {
    /// The callee at `addr`, run in place while the step holds the state ([`crate::collision::call_grid`]): the probe
    /// reads none of its words after it (residue audit, notes/physics.md "Store seam").
    fn call(&self, addr: u32, ecx: u32, args: &[u32]) -> u32 {
        crate::collision::call_grid(self.host, self.sp, addr, ecx, args).eax
    }

    /// The wheel's own callee at `addr` on the wheel `this` ([`call_wheel`]).
    fn wheel(&self, addr: u32, this: u32, args: &[u32]) -> u32 {
        call_wheel(self.host, this, self.sp, addr, this, args).eax
    }
}

impl layout::GroundCalls for Ground<'_> {
    fn effective_radius(&mut self, scratch: u32) -> f32 {
        // The getter's state pointer is not read: the radius is the tyre's (`TyreTail_Get14`, direct on a held tyre).
        if !goes_direct(self.host, crate::tyre::holds_tyre(self.host, scratch)) {
            self.call(0x5326C0, scratch, &[]);
        }
        crate::tyre::read_tyre(self.host, scratch, |t| t.state.effective_radius)
    }

    fn table_lerp_pair(&mut self, wheel: u32) {
        self.wheel(UNK96E0, wheel, &[]);
    }

    fn query_box_faces(&mut self, object: u32, a: u32, b: u32) {
        self.call(0x4FF170, object, &[a, b, 0]);
    }

    // The CRT's vector constructor and destructor iterators (the runner's stand-ins, `Runner::vector_ctor`) run here
    // on the runner: each element's call is placed as the stand-in places it, below its frame.
    fn ray_blocks_init(&mut self, rays: u32) {
        if self.sp != 0 {
            for i in 0..8 {
                crate::collision::ray_seeds_at(self.host, &mut self.mem, rays + i * 0x6B0);
            }
            return;
        }
        self.call(0x70C408, 0, &[rays, 0x6B0, 8, RAY_SEEDS, RAY_CLEAR]);
    }

    fn ray_blocks_fini(&mut self, rays: u32) {
        if self.sp != 0 {
            for i in (0..8).rev() {
                crate::collision::ray_clear_at(self.host, &mut self.mem, rays + i * 0x6B0);
            }
            return;
        }
        self.call(0x70C4DA, 0, &[rays, 0x6B0, 8, RAY_CLEAR]);
    }

    fn collect_entities(&mut self, world: u32, a: u32, b: u32, out: u32) {
        self.call(0x525BD0, world, &[a, b, 8, 0, out]);
    }

    fn mesh_ray_query(&mut self, object: u32, b: u32, c: u32, d: u32) -> u32 {
        crate::collision::call_grid(self.host, self.sp, 0x4FD760, object, &[b, c, d, 0]).eax
    }

    fn cast_ray(&mut self, world: u32, a: u32, b: u32, list: u32, out: u32) -> u32 {
        crate::collision::call_grid(self.host, self.sp, 0x5246A0, world, &[a, b, list, out]).eax
    }

    fn hit_flag(&mut self, hit: u32) -> u32 {
        let target = self.mem.u32(self.mem.u32(hit) + 0x154);
        self.call(target, hit, &[])
    }

    fn update_ray_block(&mut self, wheel: u32, block: u32, limit: f32, floor: f32) {
        self.wheel(M3D460, wheel, &[block, limit.to_bits(), floor.to_bits()]);
    }

    fn segment_sweep(&mut self, object: u32, a: u32, b: u32, c: u32, d: u32) -> u32 {
        self.call(0x4FAD90, object, &[a, b, c, d])
    }

    fn pick_contact(&mut self, wheel: u32, a: u32, b: u32, out: u32) {
        self.wheel(UNK9290, wheel, &[a, b, out]);
    }

    fn blend_tags(&mut self, wheel: u32, x: u32, y: u32, z: f32) {
        self.wheel(UNK9900, wheel, &[x, y, z.to_bits()]);
    }

    fn load_binary(&mut self, to: u32, data: u32) {
        self.call(0x4F3D20, to, &[data]);
    }
}

pub fn wheel_step(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    // The body's frame: `push ebp; mov ebp, esp; and esp, -0x10; sub esp, 0x284; push ebx, esi, edi` (0x290 bytes below the
    // aligned `ebp`, which is the entry's `esp - 4`).
    let frame = ((r.esp - 4) & !0xF) - 0x290;
    let dt = f32::from_bits(r.stack[0]);
    let mut calls = Step { mem: ctx.mem.clone(), host: ctx.host };
    let slip = crate::ngp::step_slip(ctx)?;
    // The frame's `+0x6c` is read before anything writes it (lane 3 of the two matrix rows' copies).
    ctx.mem.set_u32(frame + 0x6C, crate::RESIDUE);
    on_wheels(ctx, |m| layout::wheel_step(m, &mut calls, r.ecx, dt, frame, slip));
    Ok(Return::void(0, 4))
}

/// Whether a call into the held tyre or strut runs direct (`held`), counted as a run ([`Host::ran_direct`]): the
/// getters' bindings read their object's image words only (`TyreTail_Get10`, `TyreTail_Get14`, `CBase::GetName`), and
/// the wheel reads none of the words the host call leaves on the stack (`notes/physics.md` "Store seam").
fn goes_direct(host: &dyn Host, held: bool) -> bool {
    if held {
        host.ran_direct();
    }
    held
}

/// The host calls of `wheel_step`: each one is the original callee at its address, with `sp` below the frame.
struct Step<'a> {
    mem: Mem,
    host: &'a dyn Host,
}

impl Step<'_> {
    /// `run` on the tyre whose scratch is `scratch` when the step holds it: the wheel's direct calls of the tyre's
    /// setters and relaxation, on its typed state with the values as arguments, each counted as a run
    /// ([`Host::ran_direct`]). Their bindings make no call and read nothing of the stack but their arguments, and the
    /// wheel step reads none of the words the host call leaves below its frame (`notes/physics.md` "Store seam").
    fn on_tyre<T>(&self, scratch: u32, run: impl FnOnce(&mut TyreView, TyreId) -> T) -> Option<T> {
        let typed = self.host.typed();
        let mut tyres = typed.car.tyres.borrow_mut();
        let id = tyres.id_of(scratch)?;
        self.host.ran_direct();
        Some(run(&mut tyres.view(&self.mem).with_defs(Some(&typed.defs)), id))
    }

    /// `run` on the held drive of the car at `car` (`CCar::Fwd6DB0`, `Fwd6DD0` and `Fwd43C0` forward to it), counted as
    /// a run: the forwarders' bindings read nothing of the stack but their arguments, and the wheel step reads only
    /// their st0 (`notes/physics.md` "Store seam"). `None` per call, where the host call runs.
    fn on_drive<T>(&mut self, car: u32, run: impl FnOnce(&mut DriveView) -> T) -> Option<T> {
        let drive = car_layout::drive_at(car);
        if !crate::engine_driveline::is_held(self.host, drive) {
            return None;
        }
        self.host.ran_direct();
        Some(crate::engine_driveline::on_drive_in(self.host, &mut self.mem, drive, run))
    }

    fn on(&self, sp: u32, addr: u32, ecx: u32, args: &[u32]) -> u32 {
        self.host.call_at(sp, addr, ecx, args).eax
    }

    /// The wheel's own callee at `addr` on the step's wheel `this` ([`call_wheel`]).
    fn wheel(&self, sp: u32, addr: u32, this: u32, args: &[u32]) -> u32 {
        call_wheel(self.host, this, sp, addr, this, args).eax
    }

    fn float(&self, sp: u32, addr: u32, ecx: u32, args: &[u32]) -> f32 {
        self.host.call_at(sp, addr, ecx, args).st0 as f32
    }
}

impl layout::StepCalls for Step<'_> {
    fn strut_name(&mut self, strut: u32, sp: u32) -> u32 {
        if goes_direct(self.host, crate::spring_damper::holds_strut(self.host, strut)) {
            return tyre_layout::load_name_object(&self.mem, strut);
        }
        self.on(sp, 0x52DB10, strut, &[])
    }

    fn set_dirty(&mut self, object: u32, sp: u32) {
        if !set_dirty_held(self.host, object) {
            self.on(sp, SET_DIRTY, object, &[]);
        }
    }

    fn refresh(&mut self, this: u32, travel: f32, sp: u32) {
        self.wheel(sp, M3B900, this, &[travel.to_bits()]);
    }

    fn ground_probe(&mut self, this: u32, out: u32, normal: u32, sp: u32) -> u32 {
        self.wheel(sp, M3D520, this, &[out, normal])
    }

    // The vector callees (`collision/vectors.rs`' bindings, which make no call) run here on the runner, on the
    // solids' store (their pointers may point into a solid: the car's angular velocity, the wheel's position).
    fn clear_vec4(&mut self, at: u32, sp: u32) {
        if sp != 0 {
            return with_solids(self.host, &mut self.mem, |s| s.store_row(at, clear_vec4()));
        }
        self.on(sp, 0x465D80, at, &[]);
    }

    fn copy_vec4(&mut self, dst: u32, src: u32, sp: u32) {
        if sp != 0 {
            return with_solids(self.host, &mut self.mem, |s| s.store_row(dst, s.row(src)));
        }
        self.on(sp, 0x43C5B0, dst, &[src]);
    }

    fn cross(&mut self, a: u32, b: u32, out: u32, sp: u32) {
        if sp != 0 {
            return with_solids(self.host, &mut self.mem, |s| s.store_vec3(out, s.vec3(a).cross3(s.vec3(b))));
        }
        self.on(sp, 0x43C630, a, &[b, out]);
    }

    fn scale_vec4(&mut self, src: u32, factor: f32, out: u32, sp: u32) {
        if sp != 0 {
            return with_solids(self.host, &mut self.mem, |s| s.store_row(out, s.row(src).scale(factor)));
        }
        self.on(sp, 0x473CD0, src, &[factor.to_bits(), out]);
    }

    fn car_slot(&mut self, car: u32, args: &[u32], sp: u32) -> u32 {
        // The car's vtable slot 12: `call [[car] + 0x30]`; the arguments stay on the stack for `pair_walk`.
        let vtable = self.mem.u32(car);
        let slot = self.mem.u32(vtable + 0x30);
        crate::call_or_trivial(self.host, sp, slot, car, args)
    }

    fn pair_walk(&mut self, this: u32, a: u32, b: u32, c: u32, sp: u32) {
        crate::entities::op(self.host, sp, this, Op::Mb560 { a, b, c });
    }

    fn world_compose(&mut self, car: u32, a: u32, b: u32, sp: u32) {
        self.on(sp, 0x4E6950, car, &[a, b]);
    }

    fn to_world(&mut self, car: u32, a: u32, b: u32, sp: u32) {
        self.on(sp, 0x4E68E0, car, &[a, b]);
    }

    // The car's members direct on the held car and drive, else through the host.
    fn spin_rate(&mut self, car: u32, wheel: u32, sp: u32) -> f32 {
        let direct = self.on_drive(car, |d| drive_layout::wheel_prev_force(d, wheel));
        direct.unwrap_or_else(|| self.float(sp, 0x4E6600, car, &[wheel]))
    }

    fn axle_term(&mut self, car: u32, wheel: u32, sp: u32) -> f32 {
        let direct = self.on_drive(car, |d| drive_layout::wheel_slip(d, wheel));
        direct.unwrap_or_else(|| self.float(sp, 0x4E6610, car, &[wheel]))
    }

    fn set_drive_pair(&mut self, car: u32, wheel: u32, a: f32, b: f32, sp: u32) {
        if self.on_drive(car, |d| drive_layout::set_wheel_offset_and_bias(d, wheel, a, b)).is_none() {
            self.on(sp, 0x4E6620, car, &[wheel, a.to_bits(), b.to_bits()]);
        }
    }

    fn roll_bar_force(&mut self, car: u32, id: u32, slot: u32, sp: u32) -> f32 {
        let direct = on_held_car(self.host, &mut self.mem, car, |m| m.roll_bar_force(car, id));
        match direct {
            Some(lane) => signed_force(lane, slot != 0),
            None => self.float(sp, 0x4E6730, car, &[id, slot]),
        }
    }

    fn add_steer_torque(&mut self, car: u32, v: f32, sp: u32) {
        if on_held_car(self.host, &mut self.mem, car, |m| car_layout::add_steer_torque_sum(m, car, v)).is_none() {
            self.on(sp, 0x4E6660, car, &[v.to_bits()]);
        }
    }

    fn set_roll_bar_cell(&mut self, car: u32, id: u32, slot: u32, v: f32, sp: u32) {
        if on_held_car(self.host, &mut self.mem, car, |m| car_layout::set_roll_bar_cell(m, car, id, slot, v)).is_none()
        {
            self.on(sp, 0x4E66A0, car, &[id, slot, v.to_bits()]);
        }
    }

    fn init_sub_block(&mut self, this: u32, _sp: u32) {
        crate::car_body::init_sub_block(self.host, &mut self.mem, this);
    }

    fn point_velocity(&mut self, body: u32, point: u32, out: u32, sp: u32) {
        self.on(sp, 0x529C80, body, &[point, out]);
    }

    // NGP's wrapper as its binding runs it, direct on the held strut; 0x52E090 (1.02, whose binding declines) and another
    // car's strut through the host.
    fn spring_step(&mut self, strut: u32, input: StepInput, out: [u32; 3], sp: u32) -> StepOutput {
        if crate::ngp::is_ngp() && crate::spring_damper::holds_strut(self.host, strut) {
            if let Ok(forces) = crate::ngp::spring(self.host, &mut self.mem, strut, input) {
                self.host.ran_direct();
                return forces;
            }
        }
        // 0x52E090, or NGP's 0x10059f70
        let StepInput { travel, coordinate, velocity } = input;
        let args = [travel.to_bits(), coordinate.to_bits(), velocity.to_bits(), out[0], out[1], out[2]];
        self.on(sp, crate::ngp::Call::SpringStep.address(), strut, &args);
        let [spring_stop_force, breakpoint_offset, damping_coefficient] = out.map(|at| self.mem.f32(at));
        StepOutput { spring_stop_force, breakpoint_offset, damping_coefficient }
    }

    fn set_tyre_active(&mut self, scratch: u32, v: u32, sp: u32) {
        if self.on_tyre(scratch, |t, id| entries::set_active(t, id, v)).is_none() {
            self.on(sp, 0x532470, scratch, &[v]);
        }
    }

    fn set_penetration_depth(&mut self, scratch: u32, v: f32, sp: u32) {
        if self.on_tyre(scratch, |t, id| entries::set_penetration_depth(t, id, v)).is_none() {
            self.on(sp, 0x533490, scratch, &[v.to_bits()]);
        }
    }

    // NGP's relaxation, as the binding runs it: direct with NGP's thermal words (none on a 1.02 image, where the
    // binding declines).
    fn relax_count(&mut self, scratch: u32, x: f32, sp: u32) {
        if let Ok(ngp) = crate::ngp::thermal_in(self.host, &self.mem, scratch) {
            if self.on_tyre(scratch, |t, id| entries::relax_and_count(t, id, x, &ngp)).is_some() {
                return;
            }
        }
        self.on(sp, 0x5358C0, scratch, &[x.to_bits()]);
    }

    // NGP's wrapper as its binding runs it, direct on the held tyre (its term kept, `Host::tyre_stepped`); a decline
    // leaves the memory as it was, and the host call runs. 0x5360C0 (1.02, whose binding declines) and another car's
    // tyre through the host.
    fn tyre_step(&mut self, scratch: u32, args: [u32; 16], sp: u32) {
        if crate::ngp::is_ngp()
            && crate::tyre::holds_tyre(self.host, scratch)
            && crate::ngp::tyre_step(self.host, &mut self.mem, scratch, &args).is_ok()
        {
            self.host.ran_direct();
            self.host.tyre_stepped(scratch, f32::from_bits(args[1]));
            return;
        }
        // 0x5360C0, or NGP's wrapper 0x1005b660
        self.on(sp, crate::ngp::Call::TyreStep.address(), scratch, &args);
    }

    fn carcass_edges(&mut self, this: u32, axle: u32, hub: u32, plus: u32, minus: u32, sp: u32) {
        self.wheel(sp, BLEND4, this, &[axle, hub, plus, minus]);
    }

    fn edge_forces(&mut self, this: u32, a: u32, b: u32, c: u32, sp: u32) {
        self.wheel(sp, UNK97A0, this, &[a, b, c]);
    }

    fn deform_top_mount(&mut self, this: u32, sp: u32) {
        self.wheel(sp, M3AB90, this, &[]);
    }

    fn apply_hub_torque(&mut self, this: u32, torque: f32, sp: u32) {
        self.wheel(sp, M3AC50, this, &[torque.to_bits()]);
    }

    fn spin_cross(&mut self, this: u32, p: u32, sp: u32) {
        self.wheel(sp, M3AFB0, this, &[p]);
    }

    fn emit_tyre_particles(&mut self, multi_tire: u32, sig: u32, sp: u32) {
        // 0x53D0D0, or NGP's 0x1005f8a0, which reads the wheel from esi
        self.host.call_at(sp, crate::ngp::Call::TyreParticles.address(), multi_tire, &[sig]);
    }
}

/// `CWheel::Vf28` (0x53f230, thiscall, plain `ret`): `layout::finish`, with the frame of the body (`push ebp; mov ebp,
/// esp; and esp, -0x10; sub esp, 0x94; push ebx, esi, edi`: 0xa0 bytes below the aligned `ebp`) (name ours).
pub(super) fn finish(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let frame = ((r.esp - 4) & !0xF) - 0xA0;
    let mut calls = Finish { host: ctx.host };
    let eax = layout::finish(&mut ctx.mem, &mut calls, r.ecx, frame);
    Ok(Return::void(eax, 0))
}

/// The original callees `Vf28` makes, each at its entry stack pointer (`sp`).
struct Finish<'a> {
    host: &'a dyn Host,
}

impl layout::FinishCalls for Finish<'_> {
    fn reset_outputs(&mut self, strut: u32, sp: u32) {
        self.host.call_at(sp, 0x52E010, strut, &[]);
    }

    fn tyre_def(&mut self, scratch: u32, sp: u32) {
        self.host.call_at(sp, 0x5326B0, scratch, &[]);
    }

    fn probe_ground(&mut self, this: u32, index: u32, drop: u32, sp: u32) {
        self.host.call_at(sp, 0x529D30, this, &[index, drop]);
    }

    fn emit(&mut self, signal: u32, payload: u32, sp: u32) {
        self.host.call_at(sp, 0x53D0D0, signal, &[payload]);
    }

    fn emit_noise(&mut self, scratch: u32, sp: u32) {
        self.host.call_at(sp, 0x535890, scratch, &[]);
    }
}

/// `CCar::BodyToWorldPair` and `CCar::BodyCompose` (thiscall `(a, b)`, `ret 8`) and the strut's callees, as `M3D030`
/// makes them. A transform's source goes through the scratch (null when the definition's guard is zero), its output is
/// read back from the scratch in four lanes.
struct Strut<'a> {
    mem: Mem,
    host: &'a dyn Host,
    scratch: u32,
}

/// The car's transforms, and the strut's callees: `GetName`, `SetDirty`, `M3B900` and `UpdateAndReset`.
const BODY_TO_WORLD_PAIR: u32 = 0x4E69C0;
const GET_NAME: u32 = 0x52DB10;
const UPDATE_AND_RESET: u32 = 0x52E060;

impl Strut<'_> {
    fn transform(&mut self, callee: u32, car: u32, a: Option<Vec4>) -> Vec4 {
        let (a_at, out) = (self.scratch, self.scratch + 0x10);
        let src = match a {
            Some(v) => {
                v.store(&mut self.mem, a_at);
                a_at
            }
            None => 0,
        };
        self.host.call(callee, car, &[src, out]);
        Vec4::load(&self.mem, out)
    }
}

impl CarXform for Strut<'_> {
    fn body_to_world_pair(&mut self, car: u32, a: Option<Vec4>) -> Vec4 {
        self.transform(BODY_TO_WORLD_PAIR, car, a)
    }

    fn body_compose(&mut self, car: u32, a: Option<Vec4>) -> Vec4 {
        self.transform(BODY_COMPOSE, car, a)
    }
}

impl StrutCalls for Strut<'_> {
    fn strut_name(&mut self, strut: u32) -> u32 {
        self.host.call(GET_NAME, strut, &[]).eax
    }

    fn mark_dirty(&mut self, object: u32) {
        if !set_dirty_held(self.host, object) {
            self.host.call(SET_DIRTY, object, &[]);
        }
    }

    fn refresh(&mut self, wheel: u32, travel: f32) {
        self.host.call(M3B900, wheel, &[travel.to_bits()]);
    }

    fn update_and_reset(&mut self, strut: u32) {
        self.host.call(UPDATE_AND_RESET, strut, &[]);
    }
}

/// thiscall `()`: `E51Wheel::M3D030`, the strut's update after the car's frames (`layout::update_strut`) (name ours).
pub(super) fn update_strut(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let host = ctx.host;
    let mut calls = Strut { mem: ctx.mem.clone(), host, scratch: r.esp - SCRATCH_BELOW_ESP };
    let mut libm = Libm { host, eax: Cell::new(0) };
    on_wheels(ctx, |m| layout::update_strut(&mut calls, &mut libm, m, r.ecx));
    Ok(Return::void(0, 0))
}
