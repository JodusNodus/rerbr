//! The entities' matrix and pair-walk ops (`rbr_physics::entities::{transform, walk, carry}`, 0x51ab70-0x51b780) as
//! Rust calls with values on the pure host ([`Op`], [`op`], [`run`]), and their bindings, the per-call adapter that
//! runs the same values first.

use std::cell::Ref;
use std::ops::Range;

use rbr_physics::entities::carry::{carry, carry_grid, Frames, Passage};
use rbr_physics::entities::frame::Hook;
use rbr_physics::entities::layout::{self, GRID_FIRST_BOXES, GRID_SECOND_BOXES, GRID_STRIDE};
use rbr_physics::entities::store::EntityStore;
use rbr_physics::entities::transform;
use rbr_physics::entities::walk::{carry_three_four, Node, Walker};
use rbr_physics::mem::Mem;
use rbr_physics::rigid_body::layout::{CarSolids, SolidView, Vec4};
use rbr_physics::rigid_body::store::RigidStore;

use super::walks::{walk_depth, PairWalk};
use super::{
    COMPOSE_B, COPY16_WALK, COPY_BLOCKS, COPY_BLOCKS_THREE_FOUR, FETCH_X, FETCH_Y, GRID_WALK, MB560, PAIR_COMBINE,
    PAIR_COMBINE_TWIN,
};
use crate::rigid_body::held_solids;
use crate::{Ctx, Decline, Host, Regs, Return};

/// An op's stack words, as values: `this` (the receiver) is the caller's, eax the result.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Op {
    /// `E43PairWalk::MB560` `(a, b, c)`: the row at `a` carried from `this`'s frame to `b`'s (Three-Four) into `c`;
    /// eax `c`.
    Mb560 { a: u32, b: u32, c: u32 },
    /// `E33Mat34::M51B4C0` `(src, o2, out)`: the row at `src` from `this`'s frame to `o2`'s (One-Two) into `out`;
    /// eax `out`.
    ComposeA { src: u32, o2: u32, out: u32 },
    /// `E33Mat34::ComposeB` (0x51b600) `(src, o2, out)`: `ComposeA` with Five-Six.
    ComposeB { src: u32, o2: u32, out: u32 },
    /// `E33Mat34::FetchX` on the transform `this` `(out, arg)`: its matrix row from its node's frame to `arg`'s
    /// (One-Two) into `out`; eax `out`.
    FetchX { out: u32, arg: u32 },
    /// `E33Mat34::FetchY` on the transform `this` `(out, arg)`: its contact grid copied to `out`, the box tables from
    /// its node's frame to `arg`'s (the grid walk `WB410`); eax `out`.
    FetchY { out: u32, arg: u32 },
    /// `E33Mat34::Combine` (0x51ab90) or its twin (0x51ab70) `(a, b)`: the pair stored at `this`; eax `this`.
    Combine { a: u32, b: u32, twin: bool },
    /// `E43PairWalk::M51B460` (One-Two) or `M51B500` (`three_four`) `(src, o2, dst, n)`: the `n` rows at `src` from
    /// `this`'s frame to `o2`'s into `dst`; eax `dst`.
    CopyBlocks { src: u32, o2: u32, dst: u32, n: u32, three_four: bool },
}

impl Op {
    /// The original function and its stack words, for the call on a host that runs it.
    fn call_words(self) -> (u32, [u32; 4], usize) {
        match self {
            Op::Mb560 { a, b, c } => (MB560, [a, b, c, 0], 3),
            Op::ComposeA { src, o2, out } => (COPY16_WALK, [src, o2, out, 0], 3),
            Op::ComposeB { src, o2, out } => (COMPOSE_B, [src, o2, out, 0], 3),
            Op::FetchX { out, arg } => (FETCH_X, [out, arg, 0, 0], 2),
            Op::FetchY { out, arg } => (FETCH_Y, [out, arg, 0, 0], 2),
            Op::Combine { a, b, twin } => ([PAIR_COMBINE, PAIR_COMBINE_TWIN][usize::from(twin)], [a, b, 0, 0], 2),
            Op::CopyBlocks { src, o2, dst, n, three_four } => {
                ([COPY_BLOCKS, COPY_BLOCKS_THREE_FOUR][usize::from(three_four)], [src, o2, dst, n], 4)
            }
        }
    }
}

/// `op` on `this` from a caller, its eax. While the pure host holds the state, a Rust call with values ([`run`]: no
/// call, none of its words, no frame of its own); when its value walk backs out, its binding in place at `sp`
/// ([`Host::call_in_place`]: the walk's calls placed as through the host, none of the call's words). Elsewhere (the
/// per-call path) the call at `sp` (0: the host places it).
pub(crate) fn op(host: &dyn Host, sp: u32, this: u32, op: Op) -> u32 {
    let (target, words, len) = op.call_words();
    let args = &words[..len];
    let ran = host.with_image(&mut |mem| run(host, mem, sp, this, op));
    if let Some(eax) = ran {
        // Counted as the run it replaces ([`Host::entered`]: a caller that keeps what it read while nothing ran).
        host.ran_direct();
        return eax;
    }
    if let Some(called) = host.call_in_place(sp, target, this, args) {
        return called.eax;
    }
    host.call_at(sp, target, this, args).eax
}

/// `host.call_at(sp, target, this, args)` of a pair walk from a caller on held state: in place while the pure host
/// holds the state ([`Host::call_in_place`]), else the call.
fn in_place_or_call(host: &dyn Host, sp: u32, target: u32, this: u32, args: &[u32]) -> u32 {
    host.call_in_place(sp, target, this, args).unwrap_or_else(|| host.call_at(sp, target, this, args)).eax
}

/// `op` on `this` with values on held state, its eax: the rows read once (a held solid's through its view), carried
/// through the frames on the path (`entities::carry`) and stored where the original stores them; the pair stored. `None`
/// (nothing written but `FetchY`'s grid copy, which the walk in place makes again) off held state, or when the value
/// walk backs out: a frame's hook, gate or owner slot not known (`layout::passage`), a body loop that would read the
/// output, overlapping rows, more than [`CARRIED_ROWS`] rows, a grid of more than [`GRID_BOXES`] boxes.
pub(crate) fn run(host: &dyn Host, mem: &mut Mem, esp: u32, this: u32, op: Op) -> Option<u32> {
    let route = |three_four| [Walker::RoutePairOneTwo, Walker::RoutePairThreeFour][usize::from(three_four)];
    let (eax, done) = match op {
        Op::Mb560 { a, b, c } => (c, carried(host, mem, esp, Walker::RoutePairThreeFour, (this, b), a, c, 1)),
        Op::ComposeA { src, o2, out } => {
            (out, carried(host, mem, esp, Walker::RoutePairOneTwo, (this, o2), src, out, 1))
        }
        Op::ComposeB { src, o2, out } => {
            (out, carried(host, mem, esp, Walker::RoutePairFiveSix, (this, o2), src, out, 1))
        }
        Op::FetchX { out, arg } => {
            let t = mem.load_transform(this);
            (out, carried(host, mem, esp, Walker::RoutePairOneTwo, (t.node, arg), t.matrix, out, 1))
        }
        Op::FetchY { out, arg } => (out, grid_carried(host, mem, esp, this, out, arg)),
        Op::Combine { a, b, .. } => (mem.store_pair(this, a, b), true),
        Op::CopyBlocks { src, o2, dst, n, three_four } => {
            (dst, carried(host, mem, esp, route(three_four), (this, o2), src, dst, n))
        }
    };
    done.then_some(eax)
}

/// The frames of a carry on the pure host: the frame words and vtables on the image, the owners' poses the held
/// solids'.
struct HeldFrames<'a> {
    mem: &'a mut Mem,
    /// Where the walk in place would be entered and how far below it a child walk enters, for the words the audit
    /// fills (the walk's hooks, gates, owner slots and child walks leave theirs unwritten).
    esp: u32,
    walk_depth: u32,
    solids: Option<Ref<'a, CarSolids>>,
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

    /// The `n` rows at `at`, read through the solids' view.
    fn load(&mut self, at: u32, rows: &mut [Vec4]) {
        self.on_held |= (0..rows.len() as u32).any(|k| self.held(at + 16 * k));
        let view = self.view();
        for (k, row) in (0..).zip(rows.iter_mut()) {
            *row = view.row(at + 16 * k);
        }
    }

    fn store(&mut self, at: u32, rows: &[Vec4]) {
        self.on_held |= (0..rows.len() as u32).any(|k| self.held(at + 16 * k));
        let mut view = self.view();
        for (k, row) in (0..).zip(rows.iter()) {
            view.store_row(at + 16 * k, *row);
        }
    }
}

impl Frames for HeldFrames<'_> {
    type Mark = (u32, u32);

    fn node(&self, at: u32) -> Node {
        self.mem.load_node(at)
    }

    fn passage(&mut self, at: u32, hook: Hook, rows: u32, count: u32) -> Option<Passage> {
        // The hook's call, its gate's and its owner slot's as a walk in place enters them ([`PairWalk::hook`], the
        // frame hooks' bindings): none of their words is written here.
        let out = self.out.clone();
        let passage = layout::passage(&self.view(), at, hook, &out);
        if matches!(passage, Some(Passage::Points { .. })) {
            // `CCarBody`'s scatter is a call of the walk in place, which writes its words; a later reader of the dead
            // stack there (the tyre noise payload's word `+0x46c`, drive2 step 3 with the audit's fills) sees them, so
            // they are written here too, (the original's own words, not `crate::RESIDUE`).
            let sp = (self.esp - 0x1000) & !0xf;
            for (k, word) in [crate::pure::CALL_RET, rows, count].into_iter().enumerate() {
                self.mem.set_u32(sp - 16 + 4 * k as u32, word);
            }
        }
        if matches!(passage, Some(Passage::Body(_))) {
            let owner = self.mem.load_entity_frame(at).owner;
            self.on_held |= self.held(owner + rbr_physics::rigid_body::layout::POSITION);
        }
        passage
    }

    fn descend(&mut self, which: Walker, _receiver: u32, _o2: u32, _a: u32, _b: u32) -> (u32, u32) {
        let mark = (self.esp, self.walk_depth);
        let sp = self.esp.wrapping_sub(self.walk_depth);
        // The child walk's words stay unwritten, as [`PairWalk::walk`] leaves them: their one reader was the passenger
        // step's walk point, whose lane 3 is `crate::RESIDUE` here (notes/physics.md "Entities", residue).
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
/// values (`entities::carry`). False, nothing written, off held state or when the walk backs out ([`run`]). Counted as
/// a run when it read or wrote a held solid's words ([`Host::ran_direct`]: the call memo sees only the image).
#[allow(clippy::too_many_arguments)]
fn carried(
    host: &dyn Host,
    mem: &mut Mem,
    esp: u32,
    route: Walker,
    (receiver, o2): (u32, u32),
    src: u32,
    dst: u32,
    n: u32,
) -> bool {
    let len = 16 * n;
    if n as usize > CARRIED_ROWS || (src != dst && src < dst + len && dst < src + len) {
        return false;
    }
    let mut frames =
        HeldFrames { mem, esp, walk_depth: WALK_CALL, solids: held_solids(host), out: dst..dst + len, on_held: false };
    let mut rows = [Vec4::default(); CARRIED_ROWS];
    let rows = &mut rows[..n as usize];
    frames.load(src, rows);
    if !carry(&mut frames, route, receiver, o2, dst, rows) {
        return false;
    }
    frames.store(dst, rows);
    if frames.on_held {
        host.ran_direct();
    }
    true
}

/// The most boxes of a grid a carry takes: the first table's room before the second's.
const GRID_BOXES: usize = ((GRID_SECOND_BOXES - GRID_FIRST_BOXES) / 16) as usize;

/// `FetchY` on the transform `this` as values: the grid copied to `out` (`layout::copy_grid`, as the original's `rep
/// movsd`), then its box tables read, carried from the transform's node's frame to `arg`'s (`carry_grid`) and stored.
/// False off held state, for a null or overlapping `out`, more than [`GRID_BOXES`] boxes, or when the walk backs out.
fn grid_carried(host: &dyn Host, mem: &mut Mem, esp: u32, this: u32, out: u32, arg: u32) -> bool {
    let t = mem.load_transform(this);
    let overlaps = t.matrix != out && t.matrix < out + GRID_STRIDE && out < t.matrix + GRID_STRIDE;
    if out == 0 || overlaps {
        return false;
    }
    layout::copy_grid(mem, out, t.matrix);
    let count = mem.grid_count(out) as usize;
    if count > GRID_BOXES {
        return false;
    }
    let mut frames = HeldFrames {
        mem,
        esp,
        walk_depth: WALK_CALL,
        solids: held_solids(host),
        out: out..out + GRID_STRIDE,
        on_held: false,
    };
    let (mut first, mut second) = ([Vec4::default(); GRID_BOXES], [Vec4::default(); GRID_BOXES]);
    let (first, second) = (&mut first[..count], &mut second[..count]);
    frames.load(out + GRID_FIRST_BOXES, first);
    frames.load(out + GRID_SECOND_BOXES, second);
    if !carry_grid(&mut frames, t.node, arg, out, first, second) {
        return false;
    }
    frames.store(out + GRID_FIRST_BOXES, first);
    frames.store(out + GRID_SECOND_BOXES, second);
    if frames.on_held {
        host.ran_direct();
    }
    true
}

/// Where an op's walk enters below its entry esp: the original pushes two saved registers (or the copy's), then the
/// walk's three words (`FetchX`: `1`, `out`, `arg`).
const WALK_CALL: u32 = 24;

/// `E33Mat34::FetchX` (thiscall on `matrix`, `(out, arg)`) entered at `sp`, done here when its walk is of a node and
/// itself, as the probes' identity walks are: none of the call's words or its walk's is written and the matrix
/// is copied; returns `out` (eax). `None` without a given `sp`, or when the walk would run (the call's words may then be written: the caller's call writes the same).
pub(crate) fn fetch_x_direct(host: &dyn Host, mem: &mut Mem, sp: u32, matrix: u32, out: u32, arg: u32) -> Option<u32> {
    if sp == 0 {
        return None;
    }
    let t = mem.load_transform(matrix);
    if t.node != arg {
        return None;
    }
    super::walks::copy_vec4(host, mem, out, t.matrix);
    Some(out)
}

/// A binding's run: [`run`] with its stack words, else `in_place` (the walk in the binding's own frame).
fn bound(ctx: &mut Ctx, r: &Regs, op: Op, pop: u32, in_place: impl FnOnce(&mut PairWalk) -> u32) -> Return {
    let eax = run(ctx.host, &mut ctx.mem, r.esp, r.ecx, op).unwrap_or_else(|| {
        in_place(&mut PairWalk { mem: &mut ctx.mem, host: ctx.host, esp: r.esp, walk_depth: WALK_CALL })
    });
    Return::void(eax, pop)
}

/// thiscall `(out, arg)`: `E33Mat34::FetchX` (0x51b6c0), the copy of the matrix to `out` and the walk of the worker
/// against `out`; returns `out`. The walk's callee enters 24 bytes below the entry esp (the original's pushes).
pub(super) fn fetch_x_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [out, arg, ..] = r.stack;
    let t = ctx.mem.load_transform(r.ecx);
    Ok(bound(ctx, r, Op::FetchX { out, arg }, 8, |w| transform::transform_one_two(w, t, out, arg)))
}

/// thiscall `(out, arg)`: `E33Mat34::FetchY` (0x51b780), `fetch_x` on a contact grid: the grid copy to `out` and the
/// grid walk of the worker against `out`; returns `out`. The walk enters where `FetchX`'s does.
pub(super) fn fetch_y_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [out, arg, ..] = r.stack;
    if let Some(eax) = run(ctx.host, &mut ctx.mem, r.esp, r.ecx, Op::FetchY { out, arg }) {
        return Ok(Return::void(eax, 8));
    }
    let t = ctx.mem.load_transform(r.ecx);
    let mut calls = GridFetchAt { mem: ctx.mem.clone(), host: ctx.host, sp: r.esp - WALK_CALL };
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
        in_place_or_call(self.host, self.sp, GRID_WALK, receiver, &[o2, a, b]);
    }
}

/// thiscall `(src, o2, out)`: `E33Mat34::ComposeB` (0x51b600), `copy16_walk_run` with the Five-Six walk. Returns `out`.
pub(super) fn compose_b_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [src, o2, out, ..] = r.stack;
    let t = transform::Transform { matrix: src, node: r.ecx };
    Ok(bound(ctx, r, Op::ComposeB { src, o2, out }, 12, |w| transform::transform_five_six(w, t, out, o2)))
}

/// thiscall `(src, o2, dst, n)`: `E43PairWalk` (0x51b460), the copy of `n` blocks from `src` to `dst`, then the walk
/// of `receiver` against `o2`. Returns `dst`. The walk enters 24 bytes below the entry esp (the original's pushes for
/// the call: `1`, `dst`, `o2`).
pub(super) fn copy_blocks_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [src, o2, dst, n, ..] = r.stack;
    let op = Op::CopyBlocks { src, o2, dst, n, three_four: false };
    Ok(bound(ctx, r, op, 16, |w| transform::copy_blocks_and_walk_one_two(w, r.ecx, o2, src, dst, n)))
}

/// thiscall `(src, o2, dst, n)`: `E43PairWalk` (0x51b500), `copy_blocks_run`'s twin whose walk is `RoutePairThreeFour`.
pub(super) fn copy_blocks_three_four_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [src, o2, dst, n, ..] = r.stack;
    let op = Op::CopyBlocks { src, o2, dst, n, three_four: true };
    Ok(bound(ctx, r, op, 16, |w| transform::copy_blocks_and_walk_three_four(w, r.ecx, o2, src, dst, n)))
}

/// thiscall `(src, o2, out)`: `E33Mat34` (0x51b4c0), `FetchX` on the transform whose matrix is `src`, with `this` as its
/// worker. Returns `out`.
pub(super) fn copy16_walk_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [src, o2, out, ..] = r.stack;
    let t = transform::Transform { matrix: src, node: r.ecx };
    Ok(bound(ctx, r, Op::ComposeA { src, o2, out }, 12, |w| transform::transform_one_two(w, t, out, o2)))
}

/// thiscall `(a, b, c)`: `E43PairWalk::MB560`, which copies 16 bytes from `a` to `c` and walks `r.ecx` against `b` with
/// `RoutePairThreeFour` (`c` as its `a`, 1 as its `b`). That walk returns its `a` (`c`), which is the original's eax.
pub(super) fn mb560_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [a, b, c, ..] = r.stack;
    Ok(bound(ctx, r, Op::Mb560 { a, b, c }, 12, |w| {
        carry_three_four(w, r.ecx, a, b, c);
        c
    }))
}

/// thiscall `(a, b)`: `E33Mat34::Combine` (0x51ab90) and its twin `CombineTwin` (0x51ab70), store the pair in `this`
/// and return `this`.
pub(super) fn pair_combine_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [a, b, ..] = r.stack;
    Ok(Return::void(ctx.mem.store_pair(r.ecx, a, b), 8))
}

/// `E33Mat34::M51B4C0` or `E43PairWalk::MB560` (`target`, thiscall on `this` with `args`) entered at `sp`, run here
/// on the pure host as their bindings would: as values ([`run`]), else their walks in place ([`PairWalk::walk`]);
/// false (nothing done) for another target or without a given `sp`.
pub(crate) fn transform_direct(
    host: &dyn Host,
    mem: &mut Mem,
    sp: u32,
    target: u32,
    this: u32,
    args: [u32; 3],
) -> bool {
    if !matches!(target, COPY16_WALK | MB560) || sp == 0 {
        return false;
    }
    let [a, b, c] = args;
    let op = if target == MB560 { Op::Mb560 { a, b, c } } else { Op::ComposeA { src: a, o2: b, out: c } };
    if run(host, mem, sp, this, op).is_some() {
        return true;
    }
    if target == MB560 {
        carry_three_four(&mut PairWalk { mem, host, esp: sp, walk_depth: WALK_CALL }, this, a, b, c);
    } else {
        let t = transform::Transform { matrix: a, node: this };
        transform::transform_one_two(&mut PairWalk { mem, host, esp: sp, walk_depth: WALK_CALL }, t, c, b);
    }
    true
}
