//! The broad queries: `E36Collect` and the sphere/box tests, the rays and segment sweeps, the grid ops and samples.

use rbr_physics::collision::layout::entry::CollectHost;
use rbr_physics::collision::layout::grid_hosts::{AllocHost, GridOpsHost};
use rbr_physics::collision::layout::solver_hosts::{self, CellQueryHost, RayHost, SampleHost};
use rbr_physics::collision::layout::{self, Body, CollisionView, GridSyncHost};
use rbr_physics::math::vec::clear_vec4;
use rbr_physics::mem::Mem;
use rbr_physics::store::Raw;

use super::{
    Callees, BODY_GRID, E09_VEC4_CLEAR, E45_CELL_QUERY, ENTITY_VF118, NODE_OF, RAY_SEEDS, SAMPLE_INDEXED, SAMPLE_TABLE,
};
use crate::entities::{Op, MB560};
use crate::rigid_body::OPERATOR_NEW;
use crate::stage::on_stage;
use crate::{Ctx, Decline, Host, Regs, Return};
use rbr_physics::entities::layout::{sample_indexed, Indexed};

/// thiscall `(a, b)`, ret 8: `CCollisionMeshOpen::CopyBounds` (0x4fb070), the mesh's lower bound to `a` and upper bound
/// to `b`. Eax is `b`, the second copy's result.
pub(super) fn mesh_copy_bounds(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [a, b, ..] = r.stack;
    Ok(Return::void(layout::mesh_copy_bounds(&mut ctx.mem, r.ecx, a, b), 8))
}

/// thiscall, no arguments: `CCollisionMeshOpen::IsActive` (0x4fb0b0), eax 1.
pub(super) fn mesh_is_active(_ctx: &mut Ctx, _r: &Regs) -> Result<Return, Decline> {
    Ok(Return::void(solver_hosts::mesh_is_active(), 0))
}

/// thiscall on the E36 object (`ecx`) with `(sphere*, index*)`; eax is the 0 or 1 result.
pub(super) fn sphere_box(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [sphere, index, ..] = r.stack;
    let eax = rbr_physics::collision::layout::sphere_box(&mut ctx.mem, r.ecx, sphere, index);
    Ok(Return::void(eax, 8))
}

/// thiscall on the E36 object (`ecx`) with `(body, list**, count*)`; eax is the 0 or 1 result.
pub(super) fn collect(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [body, list_out, count_out, ..] = r.stack;
    // The original's frame holds the collect's temporaries, and its callees run below it. The original's body esp:
    // after `push ebp; and esp, -0x10; sub esp, 0xe4` and its three pushes (0x4f3950-0x4f3964).
    let frame = ((r.esp - 4) & !0xF) - 0xF0;
    let host = ctx.host;
    let eax = super::on_collision(host, &mut ctx.mem, |s| {
        layout::entry::collect(s, frame, r.ecx, body, list_out, count_out, &mut Callees(host))
    });
    Ok(Return::void(eax, 12))
}

/// thiscall on the SAT box (`ecx`) with `(other_center*, other_extent*)`; eax is 1 when the boxes overlap.
pub(super) fn world_axes_overlap(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [center, extent, ..] = r.stack;
    Ok(Return::void(layout::world_axes_overlap(&ctx.mem, r.ecx, center, extent), 8))
}

/// thiscall on the SAT box (`ecx`) with `(other_center*, other_extent*)`; eax is 1 when the boxes overlap.
pub(super) fn box_sat_overlap(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [center, extent, ..] = r.stack;
    Ok(Return::void(layout::box_sat_overlap(&ctx.mem, r.ecx, center, extent), 8))
}

/// thiscall (`ecx` unused) with `(origin*, direction*, v0*, v1*, v2*, output*)`; eax is 1 on a hit.
pub(super) fn ray_triangle(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [origin, direction, v0, v1, v2, output, ..] = r.stack;
    Ok(Return::void(layout::ray_triangle(&mut ctx.mem, origin, direction, v0, v1, v2, output), 24))
}

/// E38RaySeeds (0x4f6550): thiscall on the ray object; eax is the object (its `E09Vec4::Clear` calls return it).
pub(super) fn ray_seeds(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let mut calls = RaySeedCalls { mem: ctx.mem.clone(), host: ctx.host, esp: r.esp };
    let eax = super::on_grids(ctx.host, &mut ctx.mem, &[r.ecx], |s| layout::ray_seeds(&mut calls, s, r.ecx));
    Ok(Return::void(eax, 0))
}

/// `E38RaySeeds` on the ray object `ray` as a call makes it, counted as a run ([`Host::ran_direct`]), without the call's
/// words or its `Clear` calls' (none is read after them: residue audit, notes/physics.md "Store seam").
pub(crate) fn ray_seeds_at(host: &dyn Host, mem: &mut Mem, ray: u32) {
    /// The two vectors the seeds clear, cleared after them (the words do not overlap).
    struct Clears([u32; 2], usize);
    impl RayHost for Clears {
        fn vec4_clear(&mut self, at: u32) -> u32 {
            self.0[self.1] = at;
            self.1 += 1;
            at
        }
    }
    host.ran_direct();
    let mut clears = Clears([0; 2], 0);
    super::on_grids(host, mem, &[ray], |s| layout::ray_seeds(&mut clears, s, ray));
    for &at in &clears.0[..clears.1] {
        clear_vec4().store(mem, at);
    }
}

/// `E38RayClear` on the ray object `ray` as a call makes it, as [`ray_seeds_at`].
pub(crate) fn ray_clear_at(host: &dyn Host, mem: &mut Mem, ray: u32) {
    host.ran_direct();
    layout::ray_clear(mem, ray);
}

/// `E38RaySeeds`' `E09Vec4::Clear` calls, at the pure host's placement below its entry `esp`, run here there (the
/// binding `vec4_clear` makes no call).
struct RaySeedCalls<'a> {
    mem: Mem,
    host: &'a dyn Host,
    esp: u32,
}

impl RayHost for RaySeedCalls<'_> {
    fn vec4_clear(&mut self, at: u32) -> u32 {
        let sp = (self.esp - 0x1000) & !0xf;
        if sp != 0 {
            clear_vec4().store(&mut self.mem, at);
            return at;
        }
        self.host.call(E09_VEC4_CLEAR, at, &[]).eax
    }
}

/// CColGrid::SyncGrid (0x4f7090): thiscall on the grid with three stack words (`a`, which the transform takes as its
/// ecx, then `b` and `c`); eax is 1 when the grid has edges. The host's copy and transform scratch is below the call's
/// frame, `count` vectors (at most 127).
pub(super) fn grid_sync(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [a, b, c, ..] = r.stack;
    let count = u32::from(ctx.mem.u8(r.ecx + 0x68d));
    if count > 127 {
        return Err(Decline::Unsupported("edge count above the scratch window"));
    }
    let scratch = r.esp - 0x800 - 16 * count;
    let flag = layout::sync_grid(&mut Callees(ctx.host), &mut ctx.mem, r.ecx, a, b, c, scratch);
    Ok(Return::void(flag, 12))
}

/// M_D760X (0x4fd760): thiscall on the grid with four stack words (the ray's start and end, the output record, and the
/// record list, or null for the grid's own list); eax is 1 on a hit.
pub(super) fn grid_ray(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [start, end, out, list, ..] = r.stack;
    let mut samples = Samples { mem: ctx.mem.clone(), host: ctx.host };
    let flag = layout::mesh_ray_query(&mut samples, &mut ctx.mem, r.ecx, start, end, out, list);
    Ok(Return::void(flag, 16))
}

/// `M_D760X`'s material table samples (0x520240): direct on the held stage's table, counted as a run (NGP's sampling;
/// the binding reads nothing of the stack but its arguments, and the ray reads none of the call's words), else
/// through the host (per call, 1.02, whose binding declines, and a table with no slots, whose eax is the caller's).
struct Samples<'a> {
    mem: Mem,
    host: &'a dyn Host,
}

impl SampleHost for Samples<'_> {
    fn sample(&mut self, byte: u32, low: f32, high: f32) -> u32 {
        if crate::ngp::is_ngp() && self.host.typed().is_some() {
            let found = on_stage(self.host, &self.mem, |s| sample_indexed(s, SAMPLE_TABLE, byte as u8, low, high));
            let eax = match found {
                Indexed::Texel(texel) => Some(texel),
                Indexed::OutOfRange => Some(0),
                Indexed::NoTable => None,
            };
            if let Some(eax) = eax {
                self.host.ran_direct();
                return eax;
            }
        }
        self.host.call(SAMPLE_INDEXED, SAMPLE_TABLE, &[byte, low.to_bits(), high.to_bits()]).eax
    }
}

/// thiscall `(origin, end, out)`, ret 0xc: `CCollisionSystem::Vf00` (0x4fe810), the segment query on the system
/// (`layout::segment_query`); eax is 1 on a hit.
pub(super) fn system_vf00(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [origin, end, out, ..] = r.stack;
    let frame = layout::system_frame(r.esp);
    let mut calls = SystemCallsAt { host: ctx.host, frame };
    let eax = layout::segment_query(&mut calls, &mut ctx.mem, r.ecx, origin, end, out, frame);
    Ok(Return::void(eax, 0xc))
}

/// The lookups of `CCollisionSystem::Vf00` through the host, each at its entry stack pointer in the frame.
struct SystemCallsAt<'a> {
    host: &'a dyn Host,
    frame: layout::SystemFrame,
}

impl SampleHost for SystemCallsAt<'_> {
    fn sample(&mut self, byte: u32, low: f32, high: f32) -> u32 {
        self.host
            .call_at(self.frame.sample_sp, SAMPLE_INDEXED, SAMPLE_TABLE, &[byte, low.to_bits(), high.to_bits()])
            .eax
    }
}

/// thiscall `(origin, direction, root, out)`, ret 0x10: `E38Ops::M_DE40X` (0x4fde40), the mesh ray over the tree
/// (`layout::mesh_ray_worklist`). Its result is the record at `out`; eax is not modelled.
pub(super) fn mesh_ray_worklist(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [origin, direction, root, out, ..] = r.stack;
    layout::mesh_ray_worklist(&mut ctx.mem, r.ecx, origin, direction, root, out);
    Ok(Return::void(0, 0x10))
}

/// E38NodeWalk (0x4fdd60): stdcall with three stack words (the query box, the root, and the output); eax is not
/// modelled.
pub(super) fn node_walk(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [query, root, out, ..] = r.stack;
    layout::node_walk(&mut ctx.mem, query, root, out);
    Ok(Return::void(0, 12))
}

/// E38SegmentSweep (0x4fad90): stdcall with four stack words (the segment's start and end, the hit tags, and the
/// distance). The query's scratch is below the caller's frame, as in the collect's.
pub(super) fn segment_sweep(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [start, end, out, distance, ..] = r.stack;
    let host = ctx.host;
    super::on_collision(host, &mut ctx.mem, |s| {
        layout::segment_sweep(&mut Callees(host), s, r.esp - 0x800, start, end, out, distance)
    });
    Ok(Return::void(0, 16))
}

/// E38RayClear (0x4f6740): thiscall on the ray object; leaves eax as the caller had it, which the port does not model.
pub(super) fn ray_clear(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    layout::ray_clear(&mut ctx.mem, r.ecx);
    Ok(Return::void(0, 0))
}

impl GridSyncHost for Callees<'_> {
    fn copy_transform(&mut self, _: &mut Mem, a: u32, edges: u32, worker: u32, out: u32, count: u32) {
        crate::entities::op(
            self.0,
            0,
            a,
            Op::CopyBlocks { src: edges, o2: worker, dst: out, n: count, three_four: false },
        );
    }

    fn copy_vec4(&mut self, mem: &mut Mem, dst: u32, src: u32) {
        super::copy_vec4(self.0, mem, dst, src);
    }
}

impl RayHost for Callees<'_> {
    fn vec4_clear(&mut self, at: u32) -> u32 {
        self.0.call(E09_VEC4_CLEAR, at, &[]).eax
    }
}

impl Callees<'_> {
    /// `E45_M40C0` over the box `lo`..`hi` to `cells`: the held table's cells as values, written here, else the call.
    fn cell_query(&mut self, store: &mut CollisionView, sp: u32, lo: u32, hi: u32, cells: u32) {
        let mem = store.mem_mut();
        match super::box_cells_direct(self.0, mem, lo, hi) {
            Some(found) => rbr_physics::physics_core::layout::set_cells(mem, cells, found),
            None => {
                super::call_grid(self.0, sp, E45_CELL_QUERY, 0, &[lo, hi, cells]);
            }
        }
    }
}

impl CellQueryHost<CollisionView<'_>> for Callees<'_> {
    fn query_cells(&mut self, store: &mut CollisionView, lo: u32, hi: u32, cells: u32) {
        self.cell_query(store, 0, lo, hi, cells);
    }
}

impl AllocHost for Callees<'_> {
    fn allocate(&mut self, bytes: u32) -> u32 {
        self.0.call(OPERATOR_NEW, 0, &[bytes]).eax
    }
}

impl CollectHost<CollisionView<'_>> for Callees<'_> {
    fn virtual_call(&mut self, store: &mut CollisionView, sp: u32, target: u32, args: &[u32]) -> u32 {
        let mem = store.mem_mut();
        // The getters every body in the cells is asked (`CBody::Vf118`, `E43PairWalk::MFc00`) are read here on the
        // pure host.
        if matches!(target, BODY_GRID | ENTITY_VF118 | NODE_OF) && sp != 0 {
            return match target {
                BODY_GRID => Body::new(args[0]).grid(mem),
                ENTITY_VF118 => 0,
                _ => rbr_physics::entities::layout::node_of(args[0]),
            };
        }
        self.0.call_at(sp, target, args[0], &args[1..]).eax
    }

    fn transform(&mut self, store: &mut CollisionView, sp: u32, target: u32, this: u32, a: u32, b: u32, c: u32) {
        let mem = store.mem_mut();
        debug_assert!(target == MB560 || target != 0);
        let host = self.0;
        let run = |mem: &mut Mem| crate::entities::transform_direct(host, mem, sp, target, this, [a, b, c]);
        if crate::call_memo::call(host, mem, crate::call_memo::Key([sp, target, this, a, b, c]), run) {
            return;
        }
        self.0.call_at(sp, target, this, &[a, b, c]);
    }

    fn query_cells(&mut self, store: &mut CollisionView, sp: u32, lo: u32, hi: u32, cells: u32) {
        self.cell_query(store, sp, lo, hi, cells);
    }

    fn length(&mut self, store: &CollisionView, _sp: u32, vector: u32) -> f32 {
        super::length(self.0, store.mem(), vector)
    }
}

/// `E39_SegTest170` (0x4ff170): fastcall on the collider (`ecx`) with `(a, b, out)`; eax is whether the walk found
/// anything. The query box is in the original's frame, 0x18 bytes below the entry esp.
pub(super) fn seg_test(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [a, b, out, ..] = r.stack;
    let eax = layout::seg_test(&mut ctx.mem, r.ecx, a, b, out, r.esp - 0x18);
    Ok(Return::void(eax, 12))
}

/// `E39_SegTest70` (0x4ff070): fastcall on the collider (`ecx`) with `(a, b, out)`; eax is whether the walk found
/// anything. The query box is in the original's frame, 0x18 bytes below the entry esp.
pub(super) fn segment_test(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [a, b, out, ..] = r.stack;
    let eax = layout::segment_test(&mut ctx.mem, r.ecx, a, b, out, r.esp - 0x18);
    Ok(Return::void(eax, 12))
}

/// `E38Ops::M_E2D0` (0x4fe2d0): thiscall with `(point, v0, v1, v2, out)`, `ret 0x14`; eax is 1 on a hit.
pub(super) fn point_triangle(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [point, v0, v1, v2, out, ..] = r.stack;
    let eax = layout::point_triangle(&mut ctx.mem, point, v0, v1, v2, out);
    Ok(Return::void(eax, 20))
}

/// `E38Ops::M_EBC0`'s material table samples (0x520240), at its esp for them (0x8c below its entry).
struct FaceQueryCalls<'a> {
    host: &'a dyn Host,
    sp: u32,
}

impl SampleHost for FaceQueryCalls<'_> {
    fn sample(&mut self, byte: u32, low: f32, high: f32) -> u32 {
        self.host.call_at(self.sp, SAMPLE_INDEXED, SAMPLE_TABLE, &[byte, low.to_bits(), high.to_bits()]).eax
    }
}

/// `E38Ops::M_EBC0` (0x4febc0): thiscall on the collider with `(point, out, list)`, `ret 0xc`; eax is 1 on a hit. The
/// face tests (0x4fe2d0) run in Rust.
pub(super) fn face_query(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [point, out, list, ..] = r.stack;
    let mut calls = FaceQueryCalls { host: ctx.host, sp: r.esp - 0x8c };
    let eax = layout::face_query(&mut calls, &mut ctx.mem, r.ecx, point, out, list);
    Ok(Return::void(eax, 12))
}

/// The calls of `E37GridOps::M6610`, at the entry stack pointers the port gives.
struct GridOps<'a>(&'a dyn Host);

impl GridOpsHost for GridOps<'_> {
    fn ray_seeds(&mut self, sp: u32, grid: u32) {
        self.0.call_at(sp, RAY_SEEDS, grid, &[]);
    }

    fn copy_blocks_one_two(&mut self, sp: u32, a: u32, src: u32, o2: u32, dst: u32, n: u32) {
        crate::entities::op(self.0, sp, a, Op::CopyBlocks { src, o2, dst, n, three_four: false });
    }

    fn copy_blocks_three_four(&mut self, sp: u32, a: u32, src: u32, o2: u32, dst: u32, n: u32) {
        crate::entities::op(self.0, sp, a, Op::CopyBlocks { src, o2, dst, n, three_four: true });
    }
}

/// E37GridOps::AddEdge (0x4f65b0): thiscall on the grid with `(vec*)`, ret 4. eax is the loop's pointer after a move
/// (`this + 0x48 + 16 n` for `n` points), else the caller's with its low byte the zero count (`mov al`).
pub(super) fn grid_add_edge(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let moved = layout::grid_add_edge(&mut ctx.mem, r.ecx, r.stack[0]);
    let eax = match moved {
        0 => r.eax & !0xff,
        n => r.ecx + 0x48 + 16 * n,
    };
    Ok(Return::void(eax, 4))
}

/// E37GridOps::M6610 (0x4f6610): thiscall on the grid with `(src, a, b)`, ret 0xc; eax is the grid. Declines a source
/// with one point (the asm's bounds loop would not end) and no grid with a source.
pub(super) fn grid_ops_m6610(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [src, a, b, ..] = r.stack;
    let host = ctx.host;
    let eax = super::on_grids(host, &mut ctx.mem, &[r.ecx, src], |s| {
        layout::copy_grid(&mut GridOps(host), s, r.ecx, src, a, b, r.esp)
    })
    .ok_or(Decline::Unsupported("a grid with one point, or none with a source"))?;
    Ok(Return::void(eax, 12))
}

/// thiscall on the grid with `(index, t, out_1, out_2)`, ret 0x10: `CColGrid::SampleA` (0x4e0840). Eax as the original
/// leaves it: `out_1`, or 0 (the grid's data word) when the grid has no data.
pub(super) fn grid_sample_a(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [index, t, out_1, out_2, ..] = r.stack;
    let samples = layout::edge_sample(&ctx.mem, r.ecx, index as usize, f32::from_bits(t));
    Ok(Return::void(store_samples(ctx, r.ecx, out_1, out_2, samples), 0x10))
}

/// The samples' stores of SampleA and SampleB; returns their eax.
fn store_samples(ctx: &mut Ctx, grid: u32, out_1: u32, out_2: u32, samples: (u32, u32)) -> u32 {
    let empty = layout::ColGrid::new(grid).data(&ctx.mem) == 0;
    layout::store_samples(&mut ctx.mem, out_1, out_2, samples);
    if empty {
        0
    } else {
        out_1
    }
}

/// thiscall on the grid with `(index, out_1, out_2)`, ret 0xc: `CColGrid::SampleB` (0x4e0950). Eax as SampleA's.
pub(super) fn grid_sample_b(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [index, out_1, out_2, ..] = r.stack;
    let samples = layout::vertex_sample(&ctx.mem, r.ecx, index as usize);
    Ok(Return::void(store_samples(ctx, r.ecx, out_1, out_2, samples), 0xc))
}
