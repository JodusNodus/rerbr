//! The `E33Mat34` transform fetches and copies (`rbr_physics::entities::transform`, 0x51b460-0x51b780), and their
//! direct runs on the pure host.

use std::cell::Ref;
use std::ops::Range;

use rbr_physics::entities::carry::{carry, Frames, Passage};
use rbr_physics::entities::frame::Hook;
use rbr_physics::entities::layout;
use rbr_physics::entities::store::EntityStore;
use rbr_physics::entities::transform;
use rbr_physics::entities::walk::{carry_three_four, Node, Walker};
use rbr_physics::mem::Mem;
use rbr_physics::rigid_body::layout::{CarSolids, SolidView, Vec4};
use rbr_physics::rigid_body::store::RigidStore;

use super::walks::{walk_depth, PairWalk, MB560_CALL};
use super::{COPY16_WALK, GRID_WALK, MB560};
use crate::rigid_body::held_solids;
use crate::{Ctx, Decline, Host, Regs, Return};

/// `E33Mat34::FetchX`'s walk: the original pushes two saved registers, then `1`, `out` and `arg`.
const FETCH_X_CALL: u32 = 24;

/// `host.call_at(sp, target, this, args)` of an entities' matrix or pair-walk op (`MB560`, `ComposeA`, `ComposeB`,
/// `FetchX`, `FetchY`, `Combine`, `M51B460`, `M51B500`, `WB410`) from a caller on held state: run in place while the
/// pure host holds the state ([`Host::call_in_place`]: the walk's reads of held solids go through their view, none of
/// the call's words are written; residue audit, notes/physics.md "Store seam"), else the host call. Returns eax.
pub(crate) fn call_op(host: &dyn Host, sp: u32, target: u32, this: u32, args: &[u32]) -> crate::Called {
    let held = host.typed().is_some();
    held.then(|| host.call_in_place(sp, target, this, args))
        .flatten()
        .unwrap_or_else(|| host.call_at(sp, target, this, args))
}

/// The frames of a carry on the pure host: the frame words and vtables on the image, the owners' poses the held
/// solids'. `esp` and `walk_depth` follow the walk as [`PairWalk`]'s do, for the child walks' words.
struct HeldFrames<'a> {
    mem: &'a mut Mem,
    host: &'a dyn Host,
    solids: Option<Ref<'a, CarSolids>>,
    esp: u32,
    walk_depth: u32,
    out: Range<u32>,
    /// Whether the carry read or wrote a held solid's words (as `not_kept_on_held`).
    on_held: bool,
}

impl HeldFrames<'_> {
    fn held(&self, at: u32) -> bool {
        self.solids.as_ref().is_some_and(|s| s.read(at).is_some())
    }

    fn view(&mut self) -> SolidView<'_> {
        match &self.solids {
            Some(solids) => solids.view(self.mem),
            None => SolidView::image(self.mem),
        }
    }
}

impl Frames for HeldFrames<'_> {
    type Mark = (u32, u32);

    fn node(&self, at: u32) -> Node {
        self.mem.load_node(at)
    }

    fn passage(&mut self, at: u32, hook: Hook) -> Option<Passage> {
        // The hook's call, its gate's and its owner slot's as a walk in place enters them ([`PairWalk::hook`], the
        // frame hooks' bindings): none of their words is written here (the audit fills them).
        #[cfg(feature = "residue-audit")]
        {
            let sp = (self.esp - 0x1000) & !0xf;
            crate::skip_unwritten(self.host, self.mem, sp, &[0, 0]);
            crate::skip_unwritten(self.host, self.mem, sp - 8, &[]);
            crate::skip_unwritten(self.host, self.mem, sp - 16, &[0, 0]);
        }
        let out = self.out.clone();
        let passage = layout::passage(&self.view(), at, hook, &out);
        if matches!(passage, Some(Passage::Body(_))) {
            let owner = self.mem.load_entity_frame(at).owner;
            self.on_held |= self.held(owner + rbr_physics::rigid_body::layout::POSITION);
        }
        passage
    }

    fn descend(&mut self, which: Walker, receiver: u32, o2: u32, a: u32, b: u32) -> (u32, u32) {
        let mark = (self.esp, self.walk_depth);
        let sp = self.esp.wrapping_sub(self.walk_depth);
        let route = matches!(which, Walker::RoutePairOneTwo | Walker::RoutePairThreeFour | Walker::RoutePairFiveSix);
        // The child walk's words stay unwritten, as [`PairWalk::walk`] leaves them: their one reader was the passenger
        // step's walk point, whose lane 3 is `crate::RESIDUE` here (notes/physics.md "Entities", residue).
        if !(route && receiver == o2) {
            crate::skip_unwritten(self.host, self.mem, sp, &[o2, a, b]);
        }
        (self.esp, self.walk_depth) = (sp, walk_depth(which));
        mark
    }

    fn ascend(&mut self, (esp, walk_depth): (u32, u32)) {
        (self.esp, self.walk_depth) = (esp, walk_depth);
    }
}

/// The most rows a carry takes (the contact loops carry 14).
const CARRIED_ROWS: usize = 16;

/// The `n` rows at `src` carried from `receiver`'s frame to `o2`'s by the dispatcher `route` and stored at `dst`, as
/// values (`entities::carry`): the rows read once (a held solid's through its view), the walk over the local rows,
/// the result stored at `dst`; for an op entered at `esp` whose walk enters `walk_depth` below. False (nothing but
/// the child walks' words written, which the walk in place writes again) off held state, when a frame on the path is
/// not known, or when the rows overlap. Counted as a run when it read or wrote a held solid's words
/// ([`Host::ran_direct`]: the call memo sees only the image).
#[allow(clippy::too_many_arguments)]
fn carried(
    host: &dyn Host,
    mem: &mut Mem,
    (esp, walk_depth): (u32, u32),
    route: Walker,
    (receiver, o2): (u32, u32),
    src: u32,
    dst: u32,
    n: u32,
) -> bool {
    let len = 16 * n;
    if host.typed().is_none() || n as usize > CARRIED_ROWS || (src != dst && src < dst + len && dst < src + len) {
        return false;
    }
    let (solids, out) = (held_solids(host), dst..dst + len);
    let mut frames = HeldFrames { mem, host, solids, esp, walk_depth, out, on_held: false };
    frames.on_held = (0..n).any(|k| frames.held(src + 16 * k) || frames.held(dst + 16 * k));
    let mut rows = [Vec4::default(); CARRIED_ROWS];
    let rows = &mut rows[..n as usize];
    let view = frames.view();
    for (k, row) in (0..).zip(rows.iter_mut()) {
        *row = view.row(src + 16 * k);
    }
    if !carry(&mut frames, route, receiver, o2, dst, rows) {
        return false;
    }
    let mut view = frames.view();
    for (k, row) in (0..).zip(rows.iter()) {
        view.store_row(dst + 16 * k, *row);
    }
    if frames.on_held {
        host.ran_direct();
    }
    true
}

/// `E33Mat34::FetchX` (thiscall on `matrix`, `(out, arg)`) entered at `sp`, done here when its walk is of a node and
/// itself, as the probes' identity walks are: the call's words and its walk's are written as the calls write them
/// ([`crate::skip_call`]) and the matrix copied; returns `out` (eax). `None` on a host that runs the original, or when
/// the walk would run (the call's words may then be written: the caller's call writes the same).
pub(crate) fn fetch_x_direct(host: &dyn Host, mem: &mut Mem, sp: u32, matrix: u32, out: u32, arg: u32) -> Option<u32> {
    if !crate::skip_unwritten(host, mem, sp, &[out, arg]) {
        return None;
    }
    let t = mem.load_transform(matrix);
    if t.node != arg {
        return None;
    }
    super::walks::copy_vec4(host, mem, out, t.matrix);
    crate::skip_unwritten(host, mem, sp.wrapping_sub(FETCH_X_CALL), &[arg, out, 1]);
    Some(out)
}

/// thiscall `(out, arg)`: `E33Mat34::FetchX` (0x51b6c0), the copy of the matrix to `out` and the walk of the worker
/// against `out`; returns `out`. The walk's callee enters 24 bytes below the entry esp (the original's pushes).
pub(super) fn fetch_x_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [out, arg, ..] = r.stack;
    let t = ctx.mem.load_transform(r.ecx);
    let at = (r.esp, FETCH_X_CALL);
    if carried(ctx.host, &mut ctx.mem, at, Walker::RoutePairOneTwo, (t.node, arg), t.matrix, out, 1) {
        return Ok(Return::void(out, 8));
    }
    let mut pairs = PairWalk { mem: &mut ctx.mem, host: ctx.host, esp: r.esp, walk_depth: FETCH_X_CALL };
    let eax = transform::transform_one_two(&mut pairs, t, out, arg);
    Ok(Return::void(eax, 8))
}

/// thiscall `(out, arg)`: `E33Mat34::FetchY` (0x51b780), `fetch_x` on a contact grid: the grid copy to `out` and the
/// grid walk of the worker against `out`; returns `out`. The walk enters where `FetchX`'s does.
pub(super) fn fetch_y_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [out, arg, ..] = r.stack;
    let t = ctx.mem.load_transform(r.ecx);
    let mut calls = GridFetchAt { mem: ctx.mem.clone(), host: ctx.host, sp: r.esp - FETCH_X_CALL };
    let eax = transform::transform_grid(&mut calls, t, out, arg);
    Ok(Return::void(eax, 8))
}

/// `FetchY`'s grid copy, in Rust, and its walk, entered at `sp`.
struct GridFetchAt<'a> {
    mem: Mem,
    host: &'a dyn Host,
    sp: u32,
}

impl transform::GridFetch for GridFetchAt<'_> {
    fn copy_grid(&mut self, to: u32, from: u32) {
        layout::copy_grid(&mut self.mem, to, from);
    }

    fn grid_walk(&mut self, receiver: u32, o2: u32, a: u32, b: u32) {
        crate::entities::call_op(self.host, self.sp, GRID_WALK, receiver, &[o2, a, b]);
    }
}

/// thiscall `(src, o2, out)`: `E33Mat34::ComposeB` (0x51b600), `copy16_walk_run` with the Five-Six walk. Returns `out`.
pub(super) fn compose_b_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [src, o2, out, ..] = r.stack;
    let at = (r.esp, FETCH_X_CALL);
    if carried(ctx.host, &mut ctx.mem, at, Walker::RoutePairFiveSix, (r.ecx, o2), src, out, 1) {
        return Ok(Return::void(out, 12));
    }
    let t = transform::Transform { matrix: src, node: r.ecx };
    let mut pairs = PairWalk { mem: &mut ctx.mem, host: ctx.host, esp: r.esp, walk_depth: FETCH_X_CALL };
    let eax = transform::transform_five_six(&mut pairs, t, out, o2);
    Ok(Return::void(eax, 12))
}

/// The copies' walk enters 24 bytes below the entry esp (the original's pushes for the call: `1`, `dst`, `o2`).
const COPY_BLOCKS_CALL: u32 = 24;

/// thiscall `(src, o2, dst, n)`: `E43PairWalk` (0x51b460), the copy of `n` blocks from `src` to `dst`, then the walk
/// of `receiver` against `o2`. Returns `dst`.
pub(super) fn copy_blocks_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [src, o2, dst, n, ..] = r.stack;
    let at = (r.esp, COPY_BLOCKS_CALL);
    if carried(ctx.host, &mut ctx.mem, at, Walker::RoutePairOneTwo, (r.ecx, o2), src, dst, n) {
        return Ok(Return::void(dst, 16));
    }
    let mut pairs = PairWalk { mem: &mut ctx.mem, host: ctx.host, esp: r.esp, walk_depth: COPY_BLOCKS_CALL };
    let eax = transform::copy_blocks_and_walk_one_two(&mut pairs, r.ecx, o2, src, dst, n);
    Ok(Return::void(eax, 16))
}

/// thiscall `(src, o2, dst, n)`: `E43PairWalk` (0x51b500), `copy_blocks_run`'s twin whose walk is `RoutePairThreeFour`.
pub(super) fn copy_blocks_three_four_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [src, o2, dst, n, ..] = r.stack;
    let at = (r.esp, COPY_BLOCKS_CALL);
    if carried(ctx.host, &mut ctx.mem, at, Walker::RoutePairThreeFour, (r.ecx, o2), src, dst, n) {
        return Ok(Return::void(dst, 16));
    }
    let mut pairs = PairWalk { mem: &mut ctx.mem, host: ctx.host, esp: r.esp, walk_depth: COPY_BLOCKS_CALL };
    let eax = transform::copy_blocks_and_walk_three_four(&mut pairs, r.ecx, o2, src, dst, n);
    Ok(Return::void(eax, 16))
}

/// `E43PairWalk::MB560` (`(a, b, c)`: `a` carried from `this`'s frame to `b`'s into `c`, Three-Four) or
/// `E33Mat34::M51B4C0` (`(src, o2, out)`, One-Two), `target`, entered at `esp`, as values ([`carried`]).
pub(super) fn carried_op(host: &dyn Host, mem: &mut Mem, esp: u32, target: u32, this: u32, args: [u32; 3]) -> bool {
    let [src, o2, out] = args;
    let (route, depth) = match target {
        MB560 => (Walker::RoutePairThreeFour, MB560_CALL),
        _ => (Walker::RoutePairOneTwo, FETCH_X_CALL),
    };
    carried(host, mem, (esp, depth), route, (this, o2), src, out, 1)
}

/// `E33Mat34::M51B4C0` or `E43PairWalk::MB560` (`target`, thiscall on `this` with `args`) entered at `sp`, run here
/// on the pure host as their bindings (`copy16_walk_run`, `mb560_run`) would, its walks in place
/// ([`PairWalk::walk`]); false (nothing written) for another target or on a host that runs the original.
pub(crate) fn transform_direct(
    host: &dyn Host,
    mem: &mut Mem,
    sp: u32,
    target: u32,
    this: u32,
    args: [u32; 3],
) -> bool {
    if !matches!(target, COPY16_WALK | MB560) || !crate::skip_unwritten(host, mem, sp, &args) {
        return false;
    }
    let [a, b, c] = args;
    if carried_op(host, mem, sp, target, this, args) {
        return true;
    }
    if target == MB560 {
        carry_three_four(&mut PairWalk { mem, host, esp: sp, walk_depth: MB560_CALL }, this, a, b, c);
    } else {
        let t = transform::Transform { matrix: a, node: this };
        transform::transform_one_two(&mut PairWalk { mem, host, esp: sp, walk_depth: FETCH_X_CALL }, t, c, b);
    }
    true
}

/// thiscall `(src, o2, out)`: `E33Mat34` (0x51b4c0), `FetchX` on the transform whose matrix is `src`, with `this` as its
/// worker. Returns `out`.
pub(super) fn copy16_walk_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [src, o2, out, ..] = r.stack;
    if carried_op(ctx.host, &mut ctx.mem, r.esp, COPY16_WALK, r.ecx, [src, o2, out]) {
        return Ok(Return::void(out, 12));
    }
    let t = transform::Transform { matrix: src, node: r.ecx };
    let mut pairs = PairWalk { mem: &mut ctx.mem, host: ctx.host, esp: r.esp, walk_depth: FETCH_X_CALL };
    let eax = transform::transform_one_two(&mut pairs, t, out, o2);
    Ok(Return::void(eax, 12))
}
