//! `CBody`'s own slots and links: the grid, the corners (`VfCC`), `Vf40`, the bounds and the filtered notify.

use rbr_physics::collision::layout::body_hosts::{BodyResetHost, ClearGridHost, CornerHost};
use rbr_physics::collision::layout::cell_links::{self, HeldLinks};
use rbr_physics::collision::layout::links::{self, NotifyFilteredHost};
use rbr_physics::collision::layout::state::StateHost;
use rbr_physics::collision::layout::{self, Body, CellLink, Vec4};
use rbr_physics::collision::overlap;
use rbr_physics::mem::Mem;
use rbr_physics::physics_core::layout::detect_calls::{self as detect, GridHost};

use super::{Callees, E37_WALL_SYNC};
use crate::entities::Op;
use crate::{Ctx, Decline, Host, Regs, Return};

/// The original callees of `CBody::CState::Unk44`: the owner's slot 12 (thiscall, `+0x30` of its vtable), and the wall
/// sync (E37, `CColGrid::SyncGrid`), at `sp` (0: placed by the host). `mem` reads the owner's vtable.
struct StateCallees<'a> {
    host: &'a dyn Host,
    mem: Mem,
    sp: u32,
}

impl StateHost for StateCallees<'_> {
    fn owner_transform(&mut self, owner: u32) -> u32 {
        let slot = self.mem.u32(self.mem.u32(owner) + 0x30);
        crate::call_or_trivial(self.host, self.sp, slot, owner, &[])
    }

    fn sync_grid(&mut self, wall: u32, body: u32, vec_a: u32, vec_b: u32) -> u32 {
        super::call_grid(self.host, self.sp, E37_WALL_SYNC, wall, &[body, vec_a, vec_b]).eax
    }
}

/// thiscall on the state (`ecx`), no arguments: `CBody::CState::Unk44` (0x4e33e0), the wall reset of its two vectors.
/// Eax is the owner's slot-12 result, or the wall sync's when the owner has a wall.
pub(super) fn state_unka44(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let mut host = StateCallees { host: ctx.host, mem: ctx.mem.clone(), sp: 0 };
    let eax = layout::reset_bounds(&mut host, &mut ctx.mem, r.ecx);
    Ok(Return::void(eax, 0))
}

/// `CBody::CState::Unk44` on `state` as a call at `sp` (0: placed by the host) makes it, run here as its binding runs
/// it, its calls where they land below the call's entry, counted as a run ([`Host::ran_direct`]) without the call's
/// words (none is read after them: residue audit, notes/physics.md "Store seam").
pub(crate) fn reset_state_at(host: &dyn Host, mem: &Mem, sp: u32, state: u32) -> u32 {
    let mut mem = mem.clone();
    host.ran_direct();
    let below = (host.call_esp(sp) - 0x1000) & !0xf;
    let mut calls = StateCallees { host, mem: mem.clone(), sp: below };
    layout::reset_bounds(&mut calls, &mut mem, state)
}

/// `run` on the cells' lists a step holds (`CellLinks::view`) when the link at `link` moves between cells' lists
/// (`to` the list it goes to, 0 for none: `CellLinks::moves`); `None` per call and for any other list, which is the
/// image's.
pub(crate) fn on_cell_links<T>(
    host: &dyn Host,
    mem: &Mem,
    link: u32,
    to: u32,
    run: impl FnOnce(&mut HeldLinks) -> T,
) -> Option<T> {
    let typed = host.typed()?;
    let links = &typed.car.collision.links;
    links.moves(mem, link, to).then(|| run(&mut links.view(mem)))
}

/// A cell list's first link or a link's next (the word at `at`) as the step leaves it: the held record's while a step
/// holds the cells' lists, else the image's.
pub(crate) fn cell_word(host: &dyn Host, mem: &Mem, at: u32) -> u32 {
    host.typed().map_or_else(|| mem.u32(at), |t| t.car.collision.links.next(mem, at))
}

/// CBodyLink::Unlink (0x4e0020) and the `CLink::Unlink` twin (0x47d160): thiscall on the link; eax is the pointer the
/// original read last (see `layout::unlink`). A cell's link is the held lists' while a step holds them.
pub(super) fn body_link_unlink(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let held = on_cell_links(ctx.host, &ctx.mem, r.ecx, 0, |l| cell_links::unlink(l, r.ecx));
    Ok(Return::void(held.unwrap_or_else(|| layout::unlink(&mut ctx.mem, r.ecx)), 0))
}

/// cdecl `(link, cell)`: a body's cell link moved to `cell` (`layout::move_to_cell`), on the held lists while a step
/// holds them; eax is the cell.
pub(super) fn move_to_cell(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [link, cell, ..] = r.stack;
    if on_cell_links(ctx.host, &ctx.mem, link, cell, |l| cell_links::move_to_cell(l, link, cell)).is_none() {
        layout::move_to_cell(&mut ctx.mem, link, cell);
    }
    Ok(Return::void(cell, 0))
}

/// CCellGrid::LookupTwin (0x4e03b0): thiscall on the grid with `(x, y)`; eax is the point's link. The grid's header and
/// slots are the held table's while a step holds it ([`Host::typed`]), else the image's.
pub(super) fn lookup_twin(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [x, y, ..] = r.stack.map(f32::from_bits);
    let mut alloc = Callees(ctx.host);
    let held = ctx.host.typed().and_then(|t| {
        let c = &t.car.collision;
        c.cells.link_or_build(&mut alloc, &mut ctx.mem, r.ecx, x, y, |block| c.links.add_block(block))
    });
    let link = held.unwrap_or_else(|| layout::cell_link_at(&mut alloc, &mut ctx.mem, r.ecx, x, y));
    Ok(Return::void(link, 8))
}

/// The link of the point `(x, y)` on the grid at `grid` from the table a step holds, in place of the call at `sp`,
/// counted as a run of
/// `LookupTwin` ([`Host::ran_direct`]): `None` per call, for another grid and for a cell with no link block yet
/// (the caller calls, and the binding builds it).
pub(crate) fn held_cell_link(host: &dyn Host, mem: &mut Mem, sp: u32, grid: u32, x: f32, y: f32) -> Option<u32> {
    match host.typed()?.car.collision.cells.link_at(mem, grid, x, y)? {
        CellLink::Link(link) if sp != 0 => {
            host.ran_direct();
            Some(link)
        }
        _ => None,
    }
}

/// The cell registration of the pose and window steps on the cell table and lists a step holds: `CCellGrid::LookupTwin`
/// on the grid at `grid` at `(x, y)`, then `CCar::AppendA980` of the link at `link` to the point's cell, without the
/// calls or their words (placed at `sp`: none is read after them, residue audit), counted as two runs. False (nothing
/// done) per call and for a cell with no link block yet, where the caller calls.
pub(crate) fn register_cell_direct(
    host: &dyn Host,
    mem: &mut Mem,
    sp: u32,
    grid: u32,
    link: u32,
    x: f32,
    y: f32,
) -> bool {
    let Some(typed) = host.typed() else {
        return false;
    };
    let c = &typed.car.collision;
    let Some(CellLink::Link(cell)) = c.cells.link_at(mem, grid, x, y) else {
        return false;
    };
    if !c.links.moves(mem, link, cell) || sp == 0 {
        return false;
    }
    cell_links::move_to_cell(&mut c.links.view(mem), link, cell);
    host.ran_direct();
    host.ran_direct();
    true
}

/// `E45_M40C0` (`lo`, `hi`, `out`) entered at `sp` (0: placed by the host) on the cell table a step holds, as values:
/// the box's four cells (`physics_core::layout::detect_calls::box_cells_of`), each corner's link read from the held
/// table, without the binding, the lookups or their words (none is read after them: residue audit). The caller writes
/// the cells to its frame. `None` (nothing done) per call, for another grid and for a
/// corner whose cell has no link block yet, where the caller calls.
pub(crate) fn box_cells_direct(host: &dyn Host, mem: &mut Mem, lo: u32, hi: u32) -> Option<[u32; 4]> {
    struct Held<'a> {
        cells: &'a rbr_physics::collision::layout::CellTable,
        mem: &'a Mem,
        found: u32,
        all: bool,
    }
    impl GridHost for Held<'_> {
        fn cell_link_at(&mut self, grid: u32, x: f32, y: f32) -> u32 {
            match self.cells.link_at(self.mem, grid, x, y) {
                Some(CellLink::Link(link)) => {
                    self.found += 1;
                    link
                }
                _ => {
                    self.all = false;
                    0
                }
            }
        }
    }
    let typed = host.typed()?;
    let mut held = Held { cells: &typed.car.collision.cells, mem, found: 0, all: true };
    let cells = detect::box_cells_of(mem, &mut held, lo, hi);
    let (all, found) = (held.all, held.found);
    if !all {
        return None;
    }
    // Counted as the run of M40C0 and of each lookup, as the in-place calls were.
    for _ in 0..=found {
        host.ran_direct();
    }
    Some(cells)
}

/// CBody::Vf40 (0x4e1120): thiscall on the body with no arguments; eax is the base walk's result.
pub(super) fn body_vf40(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    Ok(Return::void(layout::reset_body(&mut Vf40Callees { host: ctx.host, mem: &ctx.mem, sp: r.esp }, r.ecx), 0))
}

/// CBody::ClearGrid (0x4e11a0): thiscall on the body with the source and two bound vectors; eax is the grid's wall
/// sync result, or `u32::MAX` when no grid is attached.
pub(super) fn body_clear_grid(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [source, lower, upper, ..] = r.stack;
    let flag = layout::clear_grid(&mut BodyCallees(ctx.host), &ctx.mem, r.ecx, source, lower, upper);
    Ok(Return::void(flag, 12))
}

/// CBody::Vf118 (0x4e11f0): thiscall on the body; eax is its grid pointer (+0x64).
pub(super) fn body_grid(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    Ok(Return::void(Body::new(r.ecx).grid(&ctx.mem), 0))
}

/// CBody::VfCC (0x4e1230): thiscall on the body with two corner matrices; eax is the flag. The corners go to the
/// frame below the call (as `collect`), where the original's own frame would hold them.
pub(super) fn body_vf_cc(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [first, second, ..] = r.stack;
    let flag = layout::aabb_pretest(&mut BodyCallees(ctx.host), &mut ctx.mem, r.ecx, first, second, r.esp - 0x800);
    Ok(Return::void(flag, 8))
}

/// The original callees of CBody's own methods, run through the host: thiscall on the state and on the body, and
/// `E33Mat34::FetchX` on a corner matrix.
pub(crate) struct BodyCallees<'a>(pub(crate) &'a dyn Host);

impl ClearGridHost for BodyCallees<'_> {
    fn sync_grid(&mut self, grid: u32, source: u32, lower: u32, upper: u32) -> u32 {
        super::call_grid(self.0, 0, E37_WALL_SYNC, grid, &[source, lower, upper]).eax
    }
}

impl CornerHost for BodyCallees<'_> {
    fn fetch_x(&mut self, matrix: u32, out: u32, worker: u32) -> u32 {
        crate::entities::op(self.0, 0, matrix, Op::FetchX { out, arg: worker })
    }
}

/// `CBody::VfCC` (thiscall on `body`, `(first, second)`) entered at `sp`, run here (the probes call it on each body
/// in their cells): none of the call's words written, then [`body_vf_cc`]'s
/// port, its `FetchX` calls placed where the pure host places a call below the entry and done here when they can
/// ([`crate::entities::fetch_x_direct`]). Returns the flag (eax); `None` (nothing written) on a host that runs the
/// original.
///
/// `last` is the call before it in the same probe kernel, whose corners are the same: while nothing has run since
/// ([`Host::entered`]) and its fetches were direct, this call's fetches would write the words they wrote, so only the
/// call's own words are written and its corners tested.
pub(crate) fn body_vf_cc_direct(
    host: &dyn Host,
    mem: &mut Mem,
    sp: u32,
    body: u32,
    first: u32,
    second: u32,
    last: &mut Option<VfCcCorners>,
) -> Option<u32> {
    if sp == 0 {
        return None;
    }
    let key = (sp, first, second, host.entered());
    if let Some(VfCcCorners { corners: [lo, hi], .. }) = last.filter(|c| c.key == key) {
        return Some(u32::from({
            let bounds = Body::new(body).bounds(mem);
            overlap::corners_overlap_bounds(bounds.lower, bounds.upper, lo, hi)
        }));
    }
    let scratch = sp - 0x800;
    let mut calls = DirectVfCcCalls { mem: mem.clone(), host, esp: sp, direct: true };
    let flag = layout::aabb_pretest(&mut calls, mem, body, first, second, scratch);
    *last = calls.direct.then(|| VfCcCorners {
        key: (sp, first, second, host.entered()),
        corners: [Vec4::load(mem, scratch), Vec4::load(mem, scratch + 16)],
    });
    Some(flag)
}

/// The corners a direct `CBody::VfCC` fetched ([`body_vf_cc_direct`]), keyed by its entry sp, its two corner
/// arguments and the host's runs entered after it.
#[derive(Clone, Copy)]
pub(crate) struct VfCcCorners {
    key: (u32, u32, u32, u64),
    corners: [Vec4; 2],
}

impl VfCcCorners {
    /// The corners, for a call at `sp` with the corner arguments `first` and `second` while the host has entered
    /// `entered` runs: what that call's fetches would give.
    pub(crate) fn for_call(&self, sp: u32, first: u32, second: u32, entered: u64) -> Option<[Vec4; 2]> {
        (self.key == (sp, first, second, entered)).then_some(self.corners)
    }
}

/// [`body_vf_cc_direct`]'s `FetchX` calls, at the pure host's placement below the entry `esp`; `direct` until one is
/// run by the host.
struct DirectVfCcCalls<'a> {
    mem: Mem,
    host: &'a dyn Host,
    esp: u32,
    direct: bool,
}

impl CornerHost for DirectVfCcCalls<'_> {
    fn fetch_x(&mut self, matrix: u32, out: u32, worker: u32) -> u32 {
        let sp = (self.esp - 0x1000) & !0xf;
        match crate::entities::fetch_x_direct(self.host, &mut self.mem, sp, matrix, out, worker) {
            Some(eax) => eax,
            None => {
                self.direct = false;
                crate::entities::op(self.host, sp, matrix, Op::FetchX { out, arg: worker })
            }
        }
    }
}

/// `CBody::Vf40`'s two calls at the original's stack pointers, from its entry `sp`: the state's slot 0x44 below the
/// return address and the saved esi, then the tail jump to `CEntity::Vf40` at the entry itself.
pub(crate) struct Vf40Callees<'a> {
    pub(crate) host: &'a dyn Host,
    pub(crate) mem: &'a Mem,
    pub(crate) sp: u32,
}

impl BodyResetHost for Vf40Callees<'_> {
    fn reset_state(&mut self, state: u32) -> u32 {
        reset_state_at(self.host, self.mem, self.sp - 8, state)
    }

    fn reset_entity(&mut self, body: u32) -> u32 {
        crate::entities::walk_at(self.host, self.mem, self.sp, body, crate::entities::EntityWalk::Name)
    }
}

/// The visitor's slot 1 (`Vf04`, thiscall with the body) that `CBodyLink::NotifyFiltered` calls, at its entry stack
/// pointer: below the return address, the saved esi and edi and the pushed body.
struct Notify<'a> {
    host: &'a dyn Host,
    sp: u32,
}

impl NotifyFilteredHost for Notify<'_> {
    fn first(&self, mem: &Mem, head: u32) -> u32 {
        cell_word(self.host, mem, head)
    }

    fn next(&self, mem: &Mem, link: u32) -> u32 {
        cell_word(self.host, mem, link)
    }

    fn visit(&mut self, mem: &mut Mem, visitor: u32, body: u32) -> u32 {
        let target = mem.u32(mem.u32(visitor) + 4);
        // A detector's is run here on the pure host, as the host would run it: the call's words, then the binding
        // entered at `sp` (the calls it makes are all placed from its entry).
        if let Some(run) = crate::physics_core::detector_vf04(target) {
            if self.sp != 0 {
                self.host.ran_direct();
                let mut stack = [0; crate::STACK_ARGS];
                stack[0] = body;
                let regs = crate::Regs { ecx: visitor, esp: self.sp, stack, ..crate::Regs::default() };
                let mut ctx = Ctx { mem: mem.clone(), host: self.host };
                return run(&mut ctx, &regs).map_or_else(|_| unreachable!("a detector declines nothing"), |r| r.eax);
            }
        }
        self.host.call_at(self.sp, target, visitor, &[body]).eax
    }
}

/// CBodyLink::NotifyFiltered (0x4e0080): thiscall on the list head with the visitor, ret 4. Its callers (the bound
/// detectors) do not read eax.
pub(super) fn notify_filtered(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let mut calls = Notify { host: ctx.host, sp: r.esp - 0x10 };
    links::notify_filtered(&mut ctx.mem, r.ecx, r.stack[0], &mut calls);
    Ok(Return::void(0, 4))
}

/// CBody::Vf10C (0x4e11b0) and its twin Vf114 (0x4e11e0): thiscall on the body; eax is 1 when it has a grid.
pub(super) fn body_has_grid(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    Ok(Return::void(layout::body_has_grid(&ctx.mem, r.ecx) as u32, 0))
}

/// CBody::Vf110 (0x4e11c0): thiscall on the body; eax is 1 when its grid is active.
pub(super) fn body_grid_active(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    Ok(Return::void(layout::body_grid_active(&ctx.mem, r.ecx) as u32, 0))
}

pub(crate) const BODY_VF110: u32 = 0x4e11c0;
pub(crate) const BODY_VF11C: u32 = 0x4e1200;

/// `CBody::Vf11C` (`body_vf11c`) on `body` with `(lo, hi)` entered at `sp`, run here, its two copies too, none of
/// the calls' words written; false (nothing done) without a given `sp`.
pub(crate) fn body_box_direct(mem: &mut Mem, sp: u32, body: u32, lo: u32, hi: u32) -> bool {
    if sp == 0 {
        return false;
    }
    let (lower, upper) = Body::new(body).bounds_at();
    for (to, from) in [(lo, lower), (hi, upper)] {
        layout::vec4_copy_from(mem, to, from);
    }
    true
}

/// CBody::Vf11C (0x4e1200): thiscall on the body with `(lo, hi)`, ret 8: `E01Vec4::CopyFrom` of its lower bound to `lo`
/// and its upper bound to `hi`, each entering below the saved esi and the pushed source; eax is the second copy's.
pub(super) fn body_vf11c(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [lo, hi, ..] = r.stack;
    let (lower, upper) = Body::new(r.ecx).bounds_at();
    super::copy_vec4(ctx.host, &mut ctx.mem, lo, lower);
    let eax = super::copy_vec4(ctx.host, &mut ctx.mem, hi, upper);
    Ok(Return::void(eax, 8))
}
