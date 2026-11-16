//! `E90Group`'s build (0x601270-0x602fe0): the segment, particle, spring, pin and bend pools.

use rbr_physics::entities::layout;

use rbr_physics::mem::Mem;

use crate::enter;
use crate::{Ctx, Decline, Host, Regs, Return};

struct E90BuildHost<'a> {
    host: &'a dyn Host,
    mem: Mem,
}

impl rbr_physics::entities::layout::FenceGroupCalls for E90BuildHost<'_> {
    fn allocate(&mut self, sp: u32, size: u32) -> u32 {
        self.host.call_at(sp, 0x70bec2, 0, &[size]).eax
    }

    /// `E90SegPool::Init` (0x601270).
    fn init(&mut self, sp: u32, pool: u32, count: u32, mem: u32) -> u32 {
        let esp = enter(self.host, &mut self.mem, sp, &[count, mem]);
        layout::init_segment_pool(&mut self.mem, pool, count, mem, esp, &mut E90SegPoolHost(self.host))
    }

    /// `CFenceHandler::BuildPost` (0x602fe0).
    fn build_post(&mut self, sp: u32, handler: u32, segment: u32, dummies: u32, mem: u32) -> u32 {
        let esp = enter(self.host, &mut self.mem, sp, &[segment, dummies, mem]);
        let mut calls = E90SegmentHost { host: self.host, mem: self.mem.clone() };
        layout::build_fence_post(&mut self.mem, handler, segment, dummies, mem, esp, &mut calls)
    }

    /// `CFenceHandler::BuildSpan` (0x6029b0).
    fn build_span(&mut self, sp: u32, handler: u32, span: u32, prev: u32, post: u32, dummy: u32, mem: u32) -> u32 {
        let esp = enter(self.host, &mut self.mem, sp, &[span, prev, post, dummy, mem]);
        let mut calls = E90SegmentHost { host: self.host, mem: self.mem.clone() };
        layout::build_fence_span(&mut self.mem, handler, span, prev, post, dummy, mem, esp, &mut calls)
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
    /// `E90Seg::Init` (0x605b60).
    fn init(&mut self, sp: u32, segment: u32, dummy: u32) {
        let esp = enter(self.host, &mut self.mem, sp, &[dummy]);
        let mut calls = E90SegInitHost { host: self.host, mem: self.mem.clone() };
        layout::init_segment(&mut self.mem, segment, dummy, esp, &mut calls);
    }

    fn parts_init(&mut self, sp: u32, segment: u32, count: u32, mem: u32) -> u32 {
        let esp = enter(self.host, &mut self.mem, sp, &[count, mem]);
        layout::init_particle_pool(&mut self.mem, segment, count, mem, esp, &mut E90PoolHost(self.host))
    }

    fn springs_init(&mut self, sp: u32, segment: u32, count: u32, mem: u32) -> u32 {
        let esp = enter(self.host, &mut self.mem, sp, &[count, mem]);
        layout::init_spring_table(&mut self.mem, segment, count, mem, esp, &mut E90PoolHost(self.host))
    }

    fn bends_init(&mut self, sp: u32, segment: u32, count: u32, mem: u32) -> u32 {
        let esp = enter(self.host, &mut self.mem, sp, &[count, mem]);
        layout::init_bend_pool(&mut self.mem, segment, count, mem, esp, &mut E90PoolHost(self.host))
    }

    fn pins_init(&mut self, sp: u32, segment: u32, count: u32, mem: u32) -> u32 {
        let esp = enter(self.host, &mut self.mem, sp, &[count, mem]);
        layout::init_pin_pool(&mut self.mem, segment, count, mem, esp, &mut E90PoolHost(self.host))
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

/// `E90Group::Build` (0x601470), called at `sp` as the host would call it. Native allocation keeps the original
/// placement; the group loop and its segment/dummy links run through the typed layout port.
pub(super) fn build(host: &dyn Host, mem: &mut Mem, sp: u32, group: u32) {
    let esp = enter(host, mem, sp, &[]);
    layout::build_fence_group(mem, group, esp, &mut E90BuildHost { host, mem: mem.clone() });
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
