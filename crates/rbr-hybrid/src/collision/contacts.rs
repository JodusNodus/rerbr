//! `CBody`'s contact slots (`VfD0`, `VfD8`, `VfDC`, `VfE8`, `VfF4`; `rbr_physics::collision::layout::{body_slots, collide_hosts}`).

use rbr_physics::collision::grid_contact::ContactKind;
use rbr_physics::collision::layout::body_slots::{self, BoxEdgeHitCalls, EdgeHitsCalls, GridHitCalls};
use rbr_physics::collision::layout::collide_hosts::{GridContactHost, PairContactHost};
use rbr_physics::collision::layout::{self, Body};
use rbr_physics::collision::store::CollisionStore;
use rbr_physics::mem::Mem;
use rbr_physics::physics_core::layout::ZERO_VEC4;

use super::edge::{edge_edge_at, edge_pair_at};
use super::queries::{sample_a_at, sample_b_at};
use super::solver::{alloc_event_at, init_full_at, make_event_at};
use super::{enter_direct, E37_GRID_COPY};
use crate::entities::Op;
use crate::{Ctx, Decline, Host, Regs, Return};

/// CBody::VfD8 (0x4e3070): thiscall on the body `this` with `(a, b, c, d, e)`, where `c` is the other body and `e` the
/// event list; eax is the hit flag. The frame goes below the entry esp (`layout::record_edge_hits_frame`).
pub(crate) fn body_vf_d8(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [a, b, other, _, list, ..] = r.stack;
    let frame = layout::record_edge_hits_frame(r.esp, a, b);
    let mut calls = D8CallsAt { mem: &ctx.mem, host: ctx.host, this: r.ecx, other, list };
    let hit = body_slots::record_edge_hits(&mut calls, &frame);
    Ok(Return::void(hit, 0x14))
}

/// The calls of CBody::VfD8 through the host: the original callees at their own stack pointers (the host places them
/// below the frame), and the other body's and this body's virtuals through their vtables.
struct D8CallsAt<'a> {
    mem: &'a Mem,
    host: &'a dyn Host,
    this: u32,
    other: u32,
    list: u32,
}

const VF30_SLOT: u32 = 0x30;
const VF34_SLOT: u32 = 0x34;
const VF_E8_SLOT: u32 = 0xe8;
const APPEND_SLOT: u32 = 0x4;

impl D8CallsAt<'_> {
    fn virtual_call(&mut self, object: u32, slot: u32, args: &[u32]) -> u32 {
        let target = layout::vtable_slot(self.mem, object, slot);
        crate::call_or_trivial(self.host, 0, target, object, args)
    }
}

impl EdgeHitsCalls for D8CallsAt<'_> {
    fn ray_seeds(&mut self, ray: u32) {
        super::ray_seeds_at(self.host, &mut self.mem.clone(), ray);
    }

    fn fetch_y(&mut self, matrix: u32, out: u32, worker: u32) {
        crate::entities::op(self.host, 0, matrix, Op::FetchY { out, arg: worker });
    }

    fn other_second_transform(&mut self) -> u32 {
        self.virtual_call(self.other, VF34_SLOT, &[])
    }

    fn other_transform(&mut self) -> u32 {
        let target = layout::vtable_slot(self.mem, self.other, VF30_SLOT);
        crate::call_or_trivial(self.host, 0, target, self.other, &[])
    }

    fn this_transform(&mut self) -> u32 {
        let target = layout::vtable_slot(self.mem, self.this, VF30_SLOT);
        crate::call_or_trivial(self.host, 0, target, self.this, &[])
    }

    fn combine(&mut self, out: u32, src: u32, worker: u32) {
        crate::entities::op(self.host, 0, out, Op::Combine { a: src, b: worker, twin: false });
    }

    fn sub_block_init(&mut self, block: u32) {
        crate::car_body::init_sub_block(self.host, &mut self.mem.clone(), block);
    }

    fn grid_count(&mut self) -> u32 {
        layout::grid_edge_count(self.mem, self.this)
    }

    fn grid_hit(&mut self, twin: u32, desc: u32, payload: u32) -> bool {
        self.virtual_call(self.other, VF_E8_SLOT, &[twin, desc, 0x8000_0001, payload, 1]) != 0
    }

    fn compose(&mut self, out: u32, payload: u32, worker: u32) {
        let identity = self.mem.u32(layout::IDENTITY_WORKER_SLOT);
        crate::entities::op(self.host, 0, identity, Op::ComposeA { src: payload, o2: worker, out });
    }

    fn alloc_event(&mut self) -> u32 {
        let mem = &mut self.mem.clone();
        let esp = enter_direct(self.host, mem, 0, &[layout::EVENT_SIZE], true);
        alloc_event_at(self.host, mem, esp, layout::EVENT_SIZE)
    }

    fn init_full(&mut self, event: u32, aux: u32, payload: u32) -> u32 {
        let mem = &mut self.mem.clone();
        let args = [0, self.this, aux, ZERO_VEC4, payload, 0];
        let esp = enter_direct(self.host, mem, 0, &args, true);
        init_full_at(self.host, mem, esp, event, args)
    }

    fn vertex_sample(&mut self, index: u32, event: u32) {
        let grid = Body::new(self.this).grid(self.mem);
        let (out_a, out_b) = (event + 0xa4, event + 0xa8);
        let mem = &mut self.mem.clone();
        enter_direct(self.host, mem, 0, &[index, out_a, out_b], true);
        sample_b_at(mem, grid, index, out_a, out_b);
    }

    fn append(&mut self, event: u32) {
        let target = layout::vtable_slot(self.mem, self.list, APPEND_SLOT);
        self.host.call(target, self.list, &[event]);
    }

    fn ray_clear(&mut self, ray: u32) {
        super::ray_clear_at(self.host, &mut self.mem.clone(), ray);
    }
}
/// CBody::VfD0's calls at the stack pointers the port gives; `mem` reads the entities' vtables.
struct VfD0Calls<'a> {
    host: &'a dyn Host,
    mem: Mem,
}

impl VfD0Calls<'_> {
    fn slot(&self, sp: u32, entity: u32, slot: u32, args: &[u32]) -> u32 {
        let target = self.mem.u32(self.mem.u32(entity) + slot);
        self.host.call_at(sp, target, entity, args).eax
    }
}

impl GridContactHost for VfD0Calls<'_> {
    fn ray_seeds(&mut self, _sp: u32, grid: u32) {
        super::ray_seeds_at(self.host, &mut self.mem, grid);
    }

    fn ray_clear(&mut self, _sp: u32, grid: u32) {
        super::ray_clear_at(self.host, &mut self.mem, grid);
    }

    fn collide_init(&mut self, sp: u32, other: u32, body: u32, list: u32) -> u32 {
        crate::entities::collide_init(self.host, sp, other, body, list)
    }

    fn transform(&mut self, sp: u32, entity: u32) -> u32 {
        let target = self.mem.u32(self.mem.u32(entity) + 0x30);
        crate::call_or_trivial(self.host, sp, target, entity, &[])
    }

    fn grid_active(&mut self, sp: u32, entity: u32) -> u32 {
        self.slot(sp, entity, 0x110, &[])
    }

    fn grid(&mut self, sp: u32, entity: u32) -> u32 {
        self.slot(sp, entity, 0x118, &[])
    }

    fn active(&mut self, sp: u32, entity: u32) -> u32 {
        self.slot(sp, entity, 0x80, &[])
    }

    fn contact_query(&mut self, sp: u32, entity: u32, args: [u32; 8]) -> u32 {
        self.slot(sp, entity, 0xf0, &args)
    }

    fn append(&mut self, sp: u32, list: u32, event: u32) {
        self.slot(sp, list, 4, &[event]);
    }

    fn fetch_y(&mut self, sp: u32, matrix: u32, out: u32, transform: u32) {
        crate::entities::op(self.host, sp, matrix, Op::FetchY { out, arg: transform });
    }

    fn fetch_x(&mut self, sp: u32, matrix: u32, out: u32, transform: u32) {
        crate::entities::op(self.host, sp, matrix, Op::FetchX { out, arg: transform });
    }

    fn combine(&mut self, sp: u32, desc: u32, point: u32, transform: u32) {
        crate::entities::op(self.host, sp, desc, Op::Combine { a: point, b: transform, twin: false });
    }

    fn combine_twin(&mut self, sp: u32, desc: u32, point: u32, transform: u32) {
        crate::entities::op(self.host, sp, desc, Op::Combine { a: point, b: transform, twin: true });
    }

    fn compose(&mut self, sp: u32, transform: u32, point: u32, other: u32, out: u32) {
        crate::entities::op(self.host, sp, transform, Op::ComposeA { src: point, o2: other, out });
    }

    fn compose_normal(&mut self, sp: u32, transform: u32, point: u32, other: u32, out: u32) {
        crate::entities::op(self.host, sp, transform, Op::ComposeB { src: point, o2: other, out });
    }

    fn grid_copy(&mut self, sp: u32, grid: u32, src: u32, a: u32, b: u32) {
        super::call_grid(self.host, sp, E37_GRID_COPY, grid, &[src, a, b]);
    }

    fn edge_edge(
        &mut self,
        sp: u32,
        grid: u32,
        from: u32,
        to: u32,
        hit: u32,
        t: u32,
        normal: u32,
        material: u32,
    ) -> u32 {
        let args = [from, to, hit, t, normal, material];
        enter_direct(self.host, &mut self.mem, sp, &args, false);
        u32::from(edge_edge_at(self.host, &mut self.mem, grid, args))
    }

    fn normalize(&mut self, sp: u32, vector: u32) {
        enter_direct(self.host, &mut self.mem, sp, &[], false);
        layout::vec4_normalize(&mut self.mem, vector);
    }

    fn add(&mut self, sp: u32, a: u32, out: u32, b: u32) -> u32 {
        enter_direct(self.host, &mut self.mem, sp, &[out, b], true);
        layout::vec3_add_stale(&mut self.mem, a, out, b, crate::RESIDUE)
    }

    fn sub(&mut self, sp: u32, a: u32, out: u32, b: u32) -> u32 {
        enter_direct(self.host, &mut self.mem, sp, &[out, b], true);
        layout::vec4_sub_stale(&mut self.mem, a, out, b, crate::RESIDUE)
    }

    fn make_event(&mut self, sp: u32, kind: ContactKind, hit: u32, body: u32, other: u32, at: u32, normal: u32) -> u32 {
        let args = [kind as u32, hit, 0, body, other, at, ZERO_VEC4, normal, 0];
        let esp = enter_direct(self.host, &mut self.mem, sp, &args, true);
        make_event_at(self.host, &mut self.mem, esp, args)
    }

    fn vertex_sample(&mut self, sp: u32, grid: u32, index: u32, out_a: u32, out_b: u32) {
        enter_direct(self.host, &mut self.mem, sp, &[index, out_a, out_b], true);
        sample_b_at(&mut self.mem, grid, index, out_a, out_b);
    }

    fn edge_sample(&mut self, sp: u32, grid: u32, index: u32, t: f32, out_a: u32, out_b: u32) {
        let args = [index, t.to_bits(), out_a, out_b];
        enter_direct(self.host, &mut self.mem, sp, &args, true);
        sample_a_at(&mut self.mem, grid, index, t, out_a, out_b);
    }
}

impl PairContactHost for VfD0Calls<'_> {
    fn second_transform(&mut self, sp: u32, entity: u32) -> u32 {
        let target = self.mem.u32(self.mem.u32(entity) + 0x34);
        crate::call_or_trivial(self.host, sp, target, entity, &[])
    }

    fn edge_pair(&mut self, sp: u32, grid: u32, args: [u32; 10]) -> u32 {
        enter_direct(self.host, &mut self.mem, sp, &args, false);
        edge_pair_at(self.host, &mut self.mem, grid, args)
    }

    fn pair_event(&mut self, sp: u32, hit: u32, t: u32, body: u32, other: u32, at: u32, normal: u32) -> u32 {
        let args = [1, hit, t, body, other, at, ZERO_VEC4, normal, 0];
        let esp = enter_direct(self.host, &mut self.mem, sp, &args, true);
        make_event_at(self.host, &mut self.mem, esp, args)
    }
}

/// CBody::VfDC (0x4e2850): thiscall on the body with `(a, b, other, d, list)`, ret 0x14 (`d` is not read); eax is 1
/// when it appended an event.
pub(super) fn body_vf_dc(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [a, b, other, _, list, ..] = r.stack;
    let mut calls = VfD0Calls { host: ctx.host, mem: ctx.mem.clone() };
    let eax = super::on_collision(ctx.host, &mut ctx.mem, |s| {
        layout::pair_contacts(&mut calls, s, r.ecx, a, b, other, list, r.esp)
    });
    Ok(Return::void(eax, 0x14))
}

/// CBody::VfD0 (0x4e1eb0): thiscall on the body with `(matrix, other, list)`, ret 0xc; eax is 1 when it appended a
/// contact (or `CollideInit`'s eax).
pub(super) fn body_vf_d0(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [matrix, other, list, ..] = r.stack;
    let mut calls = VfD0Calls { host: ctx.host, mem: ctx.mem.clone() };
    let eax = super::on_collision(ctx.host, &mut ctx.mem, |s| {
        layout::grid_contacts(&mut calls, s, r.ecx, matrix, other, list, r.esp)
    });
    Ok(Return::void(eax, 12))
}

/// thiscall `(a, b, c, d, e)`, ret 0x14: `CBody::VfE8` (0x4e2f50), the other side's box test. The original calls its
/// own slot 0xf4 with the record's words and, on a hit, copies the grid's flags into the record.
pub(crate) fn body_vf_e8(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [a, b, _, record, last, ..] = r.stack;
    let mut calls = E8CallsAt { mem: &mut ctx.mem, host: ctx.host, this: r.ecx, sp: r.esp };
    let hit = body_slots::grid_hit(&mut calls, a, b, record, last);
    Ok(Return::void(hit, 0x14))
}

/// The calls of CBody::VfE8 through the host: `VfF4` (slot 0xf4) on this body, entered 0x2c below the entry esp (the
/// original pushes its eight arguments after `esi` and `edi`).
struct E8CallsAt<'a> {
    mem: &'a mut Mem,
    host: &'a dyn Host,
    this: u32,
    sp: u32,
}

/// The entry esp of `VfF4` when the original calls it from CBody::VfE8's frame.
const VF_E8_CALL_SP: u32 = 0x2c;
const VF_F4_SLOT: u32 = 0xf4;

impl GridHitCalls for E8CallsAt<'_> {
    fn box_edge_hit(&mut self, a: u32, b: u32, record: u32) -> u32 {
        let target = layout::vtable_slot(self.mem, self.this, VF_F4_SLOT);
        let args = layout::grid_hit_args(a, b, record);
        self.host.call_at(self.sp - VF_E8_CALL_SP, target, self.this, &args).eax
    }

    fn take_grid_flags(&mut self) -> (u32, u32) {
        let this = self.this;
        super::on_collision(self.host, self.mem, |s| s.take_body_grid_flags(this))
    }

    fn store_hit(&mut self, record: u32, flags: (u32, u32)) {
        layout::store_grid_hit(self.mem, record, flags, self.this);
    }
}

/// thiscall `(a, b, c, d, e, f, g, h)`, ret 0x20: `CBody::VfF4` (0x4e2e90), the box test on the body's grid. Its frame
/// is below the entry esp (`layout::box_edge_hit_frame`); eax is the hit flag.
pub(crate) fn body_vf_f4(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [a, b, c, d, e, f, g, h, ..] = r.stack;
    let frame = layout::box_edge_hit_frame(r.esp, [a, b, c, d, e, f, g, h]);
    let mut calls = F4CallsAt { mem: &mut ctx.mem, host: ctx.host, this: r.ecx, sp: layout::box_edge_hit_sp(r.esp) };
    let hit = body_slots::box_edge_hit(&mut calls, &frame);
    Ok(Return::void(hit, 0x20))
}

/// The calls of CBody::VfF4 through the host, each at the entry stack pointer the original gives its callee (`sp` is
/// the saved-register frame's bottom, `layout::box_edge_hit_sp`).
struct F4CallsAt<'a> {
    mem: &'a mut Mem,
    host: &'a dyn Host,
    this: u32,
    sp: u32,
}

impl F4CallsAt<'_> {
    /// A thiscall with no arguments on the body's vtable slot `slot`, entered at `sp`.
    fn virtual_call(&mut self, slot: u32, sp: u32) -> u32 {
        let target = layout::vtable_slot(self.mem, self.this, slot);
        crate::call_or_trivial(self.host, sp, target, self.this, &[])
    }
}

impl BoxEdgeHitCalls for F4CallsAt<'_> {
    fn second_transform(&mut self) -> u32 {
        self.virtual_call(VF34_SLOT, self.sp - 4)
    }

    // The original pushes the compose arguments before the second and third calls, so those run lower; the callee
    // takes none, and nothing it writes is read after the call, so the entry at `sp - 4` is equivalent.
    fn transform(&mut self) -> u32 {
        let target = layout::vtable_slot(self.mem, self.this, VF30_SLOT);
        crate::call_or_trivial(self.host, self.sp - 4, target, self.this, &[])
    }

    fn fetch_x(&mut self, matrix: u32, out: u32, t: u32) {
        crate::entities::op(self.host, self.sp - 12, matrix, Op::FetchX { out, arg: t });
    }

    fn edge_edge(&mut self, from: u32, to: u32, hit: u32, t: u32, normal: u32, material: u32) -> bool {
        let grid = Body::new(self.this).grid(self.mem);
        let args = [from, to, hit, t, normal, material];
        enter_direct(self.host, self.mem, self.sp - 0x1c, &args, false);
        edge_edge_at(self.host, self.mem, grid, args)
    }

    fn store_hit(&mut self, f: u32, g: u32, h: u32) {
        layout::store_box_edge_hit(self.mem, f, g, h);
    }

    fn compose(&mut self, transform: u32, point: u32, out: u32) {
        let identity = self.mem.u32(layout::IDENTITY_WORKER_SLOT);
        crate::entities::op(self.host, self.sp - 0x10, transform, Op::ComposeA { src: point, o2: identity, out });
    }

    fn compose_normal(&mut self, transform: u32, point: u32, out: u32) {
        let identity = self.mem.u32(layout::IDENTITY_WORKER_SLOT);
        crate::entities::op(self.host, self.sp - 0x10, transform, Op::ComposeB { src: point, o2: identity, out });
    }
}
