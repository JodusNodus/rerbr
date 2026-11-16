//! `E90Group`'s build (0x601270-0x602fe0): the segment, particle, spring, pin and bend pools.

use rbr_physics::entities::layout;

use rbr_physics::mem::Mem;

use crate::{Ctx, Decline, Host, Regs, Return};

struct E90BuildHost<'a>(&'a dyn Host);

impl rbr_physics::entities::layout::FenceGroupCalls for E90BuildHost<'_> {
    fn allocate(&mut self, sp: u32, size: u32) -> u32 {
        self.0.call_at(sp, 0x70bec2, 0, &[size]).eax
    }

    fn init(&mut self, sp: u32, pool: u32, count: u32, mem: u32) -> u32 {
        self.0.call_at(sp, 0x601270, pool, &[count, mem]).eax
    }

    fn build_post(&mut self, sp: u32, handler: u32, segment: u32, dummies: u32, mem: u32) -> u32 {
        self.0.call_at(sp, 0x602fe0, handler, &[segment, dummies, mem]).eax
    }

    fn build_span(&mut self, sp: u32, handler: u32, span: u32, prev: u32, post: u32, dummy: u32, mem: u32) -> u32 {
        self.0.call_at(sp, 0x6029b0, handler, &[span, prev, post, dummy, mem]).eax
    }
}

struct E90SegPoolHost<'a>(&'a dyn Host);

/// `E90Seg::E90Seg` and `E90Seg::~E90Seg`, the element functions of the segment array's iterators.
const SEG_CTOR: u32 = 0x600ff0;
const SEG_DTOR: u32 = 0x601030;
/// The CRT's `eh vector constructor iterator` and `eh vector destructor iterator` (stdcall), and `operator delete`.
const VECTOR_CTOR: u32 = 0x70c408;
const VECTOR_DTOR: u32 = 0x70c4da;
const DELETE: u32 = 0x70c260;
const SEG_SIZE: u32 = 0x78;

impl rbr_physics::entities::layout::SegmentPoolCalls for E90SegPoolHost<'_> {
    fn allocate(&mut self, sp: u32, size: u32) -> u32 {
        self.0.call_at(sp, 0x70bec2, 0, &[size]).eax
    }

    fn construct_all(&mut self, sp: u32, segments: u32, count: u32) {
        self.0.call_at(sp, VECTOR_CTOR, 0, &[segments, SEG_SIZE, count, SEG_CTOR, SEG_DTOR]);
    }

    fn destroy_all(&mut self, sp: u32, segments: u32, count: u32) {
        self.0.call_at(sp, VECTOR_DTOR, 0, &[segments, SEG_SIZE, count, SEG_DTOR]);
    }

    fn release(&mut self, sp: u32, block: u32) {
        self.0.call_at(sp, DELETE, 0, &[block]);
    }
}

/// thiscall, no args, `ret 0`: `E90Seg::E90Seg` (0x600ff0), the cleared pools. Eax is the segment.
pub(super) fn e90_seg_ctor_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let eax = layout::construct_segment(&mut ctx.mem, r.ecx);
    Ok(Return::void(eax, 0))
}

/// thiscall, no args, `ret 0`: `E90Seg::~E90Seg` (0x601030), the pools emptied and their owned storage deleted.
pub(super) fn e90_seg_dtor_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    layout::destroy_segment(&mut ctx.mem, r.ecx, r.esp, &mut E90SegPoolHost(ctx.host));
    Ok(Return::void(0, 0))
}

/// The pool and segment adders, `E90Seg::Init` and the pool inits are not leaves: they keep their calls. The adders are
/// leaves called with exactly the words they pop, which the host runs without writing them: run here as a run counted.
struct E90SegmentHost<'a> {
    host: &'a dyn Host,
    mem: Mem,
}

impl rbr_physics::entities::layout::FenceSegmentCalls for E90SegmentHost<'_> {
    fn init(&mut self, sp: u32, segment: u32, dummy: u32) {
        self.host.call_at(sp, 0x605b60, segment, &[dummy]);
    }

    fn parts_init(&mut self, sp: u32, segment: u32, count: u32, mem: u32) -> u32 {
        self.host.call_at(sp, 0x6026a0, segment, &[count, mem]).eax
    }

    fn springs_init(&mut self, sp: u32, segment: u32, count: u32, mem: u32) -> u32 {
        self.host.call_at(sp, 0x6027a0, segment, &[count, mem]).eax
    }

    fn bends_init(&mut self, sp: u32, segment: u32, count: u32, mem: u32) -> u32 {
        self.host.call_at(sp, 0x6028b0, segment, &[count, mem]).eax
    }

    fn pins_init(&mut self, sp: u32, segment: u32, count: u32, mem: u32) -> u32 {
        self.host.call_at(sp, 0x602930, segment, &[count, mem]).eax
    }

    fn add_particle(&mut self, _sp: u32, segment: u32, pos: u32, inv_mass: f32) -> u32 {
        self.host.ran_direct();
        layout::add_particle(&mut self.mem, segment, pos, inv_mass)
    }

    fn add_pin(&mut self, _sp: u32, segment: u32, particle: u32) {
        self.host.ran_direct();
        layout::add_pin(&mut self.mem, segment, particle);
    }

    fn add_spring(&mut self, _sp: u32, segment: u32, a: u32, b: u32, kind: u32, stiffness: f32) -> u32 {
        self.host.ran_direct();
        layout::add_spring(&mut self.mem, segment, a, b, kind, stiffness)
    }

    fn add_fixed(&mut self, _sp: u32, segment: u32, spring: u32) {
        self.host.ran_direct();
        layout::add_fixed(&mut self.mem, segment, spring);
    }

    fn add_bend(&mut self, _sp: u32, segment: u32, spring: u32, particle: u32) {
        self.host.ran_direct();
        layout::add_bend(&mut self.mem, segment, spring, particle);
    }
}

/// thiscall, no stack arguments: `E90Group::Build` (0x601470). Native allocation keeps the original placement; the
/// group loop and its segment/dummy links run through the typed layout port.
pub(super) fn e90_build_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let mut calls = E90BuildHost(ctx.host);
    layout::build_fence_group(&mut ctx.mem, r.ecx, r.esp, &mut calls);
    Ok(Return::void(0, 0))
}

/// thiscall `(count, block)`: `E90SegPool::Init` (0x601270), its CRT calls at the original's stack positions.
pub(super) fn e90_seg_pool_init_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [count, block, ..] = r.stack;
    let mut calls = E90SegPoolHost(ctx.host);
    let eax = layout::init_segment_pool(&mut ctx.mem, r.ecx, count, block, r.esp, &mut calls);
    Ok(Return::void(eax, 8))
}

/// thiscall `(post, dummy, block)`: `CFenceHandler::BuildPost` (0x602fe0).
pub(super) fn e90_build_post_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [post, dummy, block, ..] = r.stack;
    let mut calls = E90SegmentHost { host: ctx.host, mem: ctx.mem.clone() };
    let eax = layout::build_fence_post(&mut ctx.mem, r.ecx, post, dummy, block, r.esp, &mut calls);
    Ok(Return::void(eax, 12))
}

/// thiscall `(span, prev, next, dummy, block)`: `CFenceHandler::BuildSpan` (0x6029b0).
pub(super) fn e90_build_span_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [span, prev, next, dummy, block, ..] = r.stack;
    let mut calls = E90SegmentHost { host: ctx.host, mem: ctx.mem.clone() };
    let eax = layout::build_fence_span(&mut ctx.mem, r.ecx, span, prev, next, dummy, block, r.esp, &mut calls);
    Ok(Return::void(eax, 20))
}

/// The D3DX normalise is a leaf called with exactly its words: run here as a run counted.
struct E90SegInitHost<'a> {
    host: &'a dyn Host,
    mem: Mem,
}

impl rbr_physics::entities::layout::SegmentInitCalls for E90SegInitHost<'_> {
    fn normalize(&mut self, _sp: u32, out: u32, v: u32) {
        self.host.ran_direct();
        layout::d3dx_vec3_normalize(&mut self.mem, out, v);
    }
}

struct E90PoolHost<'a>(&'a dyn Host);

impl rbr_physics::entities::layout::ClothPoolCalls for E90PoolHost<'_> {
    fn allocate(&mut self, sp: u32, size: u32) -> u32 {
        self.0.call_at(sp, 0x70bf84, 0, &[size]).eax
    }

    fn release(&mut self, sp: u32, block: u32) {
        self.0.call_at(sp, 0x70c27e, 0, &[block]);
    }

    fn construct(&mut self, _sp: u32, _element: u32) {
        // `E90_PartCtor` (0x602220) is `mov eax, ecx; ret`, a leaf run without words.
        self.0.ran_direct();
    }
}

/// thiscall `(dummy)`, `ret 4`: `E90Seg::Init` (0x605b60). The two normalisations go through the D3DX thunk.
pub(super) fn e90_seg_init_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let mut calls = E90SegInitHost { host: ctx.host, mem: ctx.mem.clone() };
    layout::init_segment(&mut ctx.mem, r.ecx, r.stack[0], r.esp, &mut calls);
    Ok(Return::void(0, 4))
}

/// thiscall `(count, block)`, `ret 8`: `E90PartPool::Init` (0x6026a0). Eax is the cursor.
pub(super) fn e90_part_pool_init_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [count, block, ..] = r.stack;
    let eax = layout::init_particle_pool(&mut ctx.mem, r.ecx, count, block, r.esp, &mut E90PoolHost(ctx.host));
    Ok(Return::void(eax, 8))
}

/// thiscall `(count, block)`, `ret 8`: `E90PtrTab::Init` (0x6027a0). Eax is the cursor.
pub(super) fn e90_ptr_tab_init_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [count, block, ..] = r.stack;
    let eax = layout::init_spring_table(&mut ctx.mem, r.ecx, count, block, r.esp, &mut E90PoolHost(ctx.host));
    Ok(Return::void(eax, 8))
}

/// thiscall `(count, block)`, `ret 8`: `E90BendPool::Init` (0x6028b0). Eax is the cursor.
pub(super) fn e90_bend_pool_init_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [count, block, ..] = r.stack;
    let eax = layout::init_bend_pool(&mut ctx.mem, r.ecx, count, block, r.esp, &mut E90PoolHost(ctx.host));
    Ok(Return::void(eax, 8))
}

/// thiscall `(count, block)`, `ret 8`: `E90PinPool::Init` (0x602930). Eax is the cursor.
pub(super) fn e90_pin_pool_init_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [count, block, ..] = r.stack;
    let eax = layout::init_pin_pool(&mut ctx.mem, r.ecx, count, block, r.esp, &mut E90PoolHost(ctx.host));
    Ok(Return::void(eax, 8))
}
