//! The entity tree walks (`rbr_physics::entities::tree`, 0x51a1f0-0x51a4b0), the pair walkers and
//! `E43PairWalk::MB560` (`rbr_physics::entities::walk`, 0x51afe0-0x51b560) and the walk functor. A walk reads the
//! node's child list in the original's memory, asks each child's gate (vtable slot 32) and calls the child's slot through
//! the host.

use rbr_physics::entities::frame::Hook;
use rbr_physics::entities::layout;
use rbr_physics::entities::layout::{call_stored, FunctorCalls};
use rbr_physics::entities::store::EntityStore;
use rbr_physics::entities::tree::{
    integrate, name_walk, walk_gated, walk_pre_order, ChildCall, Children, GatedCalls, Integrate, NameWalk, PreOrder,
};
use rbr_physics::entities::walk::{self, Node, Walk as PairOps, Walker};
use rbr_physics::mem::Mem;

use super::frame::{frame_hook_at, FrameOwner};
use super::WALK_PRE_ORDER;
use crate::rigid_body::{held_solids, solid_gate_direct};
use crate::{Ctx, Decline, Host, Regs, Return};
use rbr_physics::rigid_body::layout::SolidWords;

/// Where a walk's calls enter, below the binding's entry esp (the return address's slot), as the original's pushes put
/// them: `gate` for each child's gate, `call` for the child's slot, `name` for the walk's own name call.
#[derive(Clone, Copy)]
struct Depths {
    gate: u32,
    call: u32,
    name: u32,
}

const fn depths(gate: u32, call: u32, name: u32) -> Depths {
    Depths { gate, call, name }
}

const VF48_DEPTHS: Depths = depths(8, 8, 0);
const VF5C_DEPTHS: Depths = depths(8, 8, 0);
const VF64_DEPTHS: Depths = depths(8, 8, 0);
const VF20_DEPTHS: Depths = depths(12, 16, 0);
const VF58_DEPTHS: Depths = depths(12, 16, 0);
const INTEGRATE_DEPTHS: Depths = depths(12, 16, 8);
/// The pre-order walk: its gate, then the functor's slot 1 and the descent, each with its arguments pushed after the
/// saved esi (or edi): the gate at entry-12, the calls at entry-16.
const PRE_ORDER_DEPTHS: Depths = depths(12, 16, 0);
/// The name walk: the state's slot 17 at entry-28, then the pre-order walk of the children at entry-32.
const NAME_DEPTHS: Depths = depths(0, 32, 28);
/// The name walk's functor object: the original's frame holds it at entry-20 (vtable, then the callback).
const NAME_FUNCTOR_AT: u32 = 20;

const NAME_FUNCTOR_VTABLE: u32 = 0x73cbe8; // the name walk's functor object: its vtable (slot 1 is the call) ...
const NAME_FUNCTOR_CALLBACK: u32 = 0x5194a0; // ... and the callback the functor's call makes

/// The walk's calls on the original's objects, through the host. A child's gate and its call are its virtual slots,
/// called with the child as `this`; they enter at the original's depths below `esp` (depth 0: the harness places them).
struct Walk<'a> {
    mem: Mem,
    host: &'a dyn Host,
    esp: u32,
    depths: Depths,
}

impl Walk<'_> {
    /// The callee-entry stack pointer `depth` below the binding's entry esp (0 when the depth is not recorded).
    fn at(&self, depth: u32) -> u32 {
        if depth == 0 {
            0
        } else {
            self.esp.wrapping_sub(depth)
        }
    }
}

impl Children for Walk<'_> {
    fn first_child(&self, node: u32) -> u32 {
        self.mem.u32(node + 0x48)
    }

    fn next_sibling(&self, child: u32) -> u32 {
        self.mem.u32(child + 0x10)
    }

    fn gated(&mut self, child: u32) -> bool {
        let target = layout::vtable_target(&self.mem, child, 0x80);
        solid_gate_direct(self.host, &mut self.mem, target, child)
            .unwrap_or_else(|| self.host.call_at(self.at(self.depths.gate), target, child, &[]).eax)
            != 0
    }
}

impl GatedCalls for Walk<'_> {
    fn call(&mut self, child: u32, call: ChildCall) {
        let (slot, arg) = match call {
            ChildCall::Substep(p) => (0x20, Some(p)),
            ChildCall::UnknownPass => (0x28, None),
            ChildCall::Init => (0x48, None),
            ChildCall::CarHook(p) => (0x58, Some(p)),
            ChildCall::Force => (0x5c, None),
            ChildCall::Torque => (0x64, None),
            ChildCall::UnknownArgPass(p) => (0x68, Some(p)),
            ChildCall::Integrate(p) => (0x6c, Some(p)),
        };
        let target = layout::vtable_target(&self.mem, child, slot);
        crate::wheel::call_wheel(self.host, child, self.at(self.depths.call), target, child, arg.as_slice());
    }
}

impl Integrate for Walk<'_> {
    fn name_walk_self(&mut self, node: u32) {
        let target = layout::vtable_target(&self.mem, node, 0x40);
        crate::wheel::call_wheel(self.host, node, self.at(self.depths.name), target, node, &[]);
    }
}

impl PreOrder for Walk<'_> {
    fn apply_functor(&mut self, functor: u32, child: u32) {
        let (sp, target) = (self.at(self.depths.call), layout::vtable_target(&self.mem, functor, 0x4));
        // `CPlainUnaryFunction::Vf04x` runs here on the pure host, as its binding (`functor_run`) runs it, without the
        // call's words (none is read after them: residue audit, notes/physics.md "Store seam").
        if target == super::FUNCTOR_VF04X && sp != 0 {
            self.host.ran_direct();
            let esp = self.host.call_esp(sp);
            call_stored(&mut Functor { mem: self.mem.clone(), host: self.host, esp }, functor, child);
            return;
        }
        self.host.call_at(sp, target, functor, &[child]);
    }

    fn descend(&mut self, child: u32, functor: u32) {
        walk_at(self.host, &self.mem, self.at(self.depths.call), child, EntityWalk::PreOrder(functor));
    }
}

impl NameWalk for Walk<'_> {
    fn state_pass(&mut self, node: u32) {
        let state = node + 0x38;
        let target = layout::vtable_target(&self.mem, state, 0x44);
        crate::call_or_trivial(self.host, self.at(self.depths.name), target, state, &[]);
    }

    fn walk_children_pre(&mut self, node: u32, functor: u32) {
        walk_at(self.host, &self.mem, self.at(self.depths.call), node, EntityWalk::PreOrder(functor));
    }
}

/// An entity's walk over its children (`rbr_physics::entities::tree`), as its binding runs it.
#[derive(Clone, Copy)]
pub(crate) enum EntityWalk {
    /// A gated walk: `CEntity::E43Vf20` (`Substep`), `E43Vf48` (`Init`), `E43Vf58` (`CarHook`), `E43Vf5C` (`Force`)
    /// or `E43Vf64` (`Torque`).
    Gated(ChildCall),
    /// `CEntity::E43Vf6C` with `dt`.
    Integrate(u32),
    /// `CEntity::E43_Walk1F0` with the functor.
    PreOrder(u32),
    /// `CEntity::Vf40`.
    Name,
}

impl EntityWalk {
    /// The binding's address, its stack word and the depths of its calls.
    fn call(self) -> (u32, Option<u32>, Depths) {
        match self {
            EntityWalk::Gated(ChildCall::Substep(p)) => (super::VF20, Some(p), VF20_DEPTHS),
            EntityWalk::Gated(ChildCall::Init) => (super::VF48, None, VF48_DEPTHS),
            EntityWalk::Gated(ChildCall::CarHook(p)) => (super::VF58, Some(p), VF58_DEPTHS),
            EntityWalk::Gated(ChildCall::Force) => (super::VF5C, None, VF5C_DEPTHS),
            EntityWalk::Gated(ChildCall::Torque) => (super::VF64, None, VF64_DEPTHS),
            EntityWalk::Gated(call) => unreachable!("no binding walks {call:?}"),
            EntityWalk::Integrate(dt) => (super::INTEGRATE, Some(dt), INTEGRATE_DEPTHS),
            EntityWalk::PreOrder(functor) => (WALK_PRE_ORDER, Some(functor), PRE_ORDER_DEPTHS),
            EntityWalk::Name => (super::NAME_WALK, None, NAME_DEPTHS),
        }
    }
}

/// `walk` of `node` as a call at `sp` (0: placed by the host) makes it, run here as its binding runs it, in a frame at
/// the call's entry, counted as a run ([`Host::ran_direct`]) without the call's words (none is read after them:
/// residue audit, notes/physics.md "Store seam"). Eax is 0.
pub(crate) fn walk_at(host: &dyn Host, mem: &Mem, sp: u32, node: u32, walk: EntityWalk) -> u32 {
    let mem = mem.clone();
    host.ran_direct();
    let esp = host.call_esp(sp);
    run_walk(host, mem, esp, node, walk);
    0
}

/// `walk` of `node` entered at `esp`: the binding's body.
fn run_walk(host: &dyn Host, mut mem: Mem, esp: u32, node: u32, walk: EntityWalk) {
    let depths = walk.call().2;
    match walk {
        EntityWalk::Gated(call) => walk_gated(&mut Walk { mem, host, esp, depths }, node, call),
        EntityWalk::Integrate(dt) => integrate(&mut Walk { mem, host, esp, depths }, node, dt),
        EntityWalk::PreOrder(functor) => walk_pre_order(&mut Walk { mem, host, esp, depths }, node, functor),
        EntityWalk::Name => {
            // Its functor object is in the original's frame, at entry-20: the vtable and the callback.
            let functor = esp - NAME_FUNCTOR_AT;
            mem.set_u32(functor, NAME_FUNCTOR_VTABLE);
            mem.set_u32(functor + 4, NAME_FUNCTOR_CALLBACK);
            name_walk(&mut Walk { mem, host, esp, depths }, node, functor);
        }
    }
}

/// The binding of `walk`, popping `pop`.
fn walk_run(ctx: &mut Ctx, r: &Regs, walk: EntityWalk, pop: u32) -> Result<Return, Decline> {
    run_walk(ctx.host, ctx.mem.clone(), r.esp, r.ecx, walk);
    Ok(Return::void(0, pop))
}

/// thiscall `(p)`: `CEntity::E43Vf58`, each gated child's slot 22 with `p`.
pub(super) fn vf58(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    walk_run(ctx, r, EntityWalk::Gated(ChildCall::CarHook(r.stack[0])), 4)
}

/// thiscall: `CEntity::E43Vf48`, each gated child's slot 18.
pub(super) fn vf48(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    walk_run(ctx, r, EntityWalk::Gated(ChildCall::Init), 0)
}

/// thiscall: `CEntity::E43Vf5C`, each gated child's slot 23.
pub(super) fn vf5c(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    walk_run(ctx, r, EntityWalk::Gated(ChildCall::Force), 0)
}

/// thiscall: `CEntity::E43Vf64`, each gated child's slot 25.
pub(super) fn vf64(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    walk_run(ctx, r, EntityWalk::Gated(ChildCall::Torque), 0)
}

/// thiscall `(p)`: `CEntity::E43Vf20`, each gated child's slot 8 with `p`.
pub(super) fn vf20(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    walk_run(ctx, r, EntityWalk::Gated(ChildCall::Substep(r.stack[0])), 4)
}

/// thiscall `(dt)`: `CEntity::E43Vf6C`, the object's name walk, then each gated child's slot 27 with `dt`.
pub(super) fn integrate_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    walk_run(ctx, r, EntityWalk::Integrate(r.stack[0]), 4)
}

/// thiscall `(functor)`: `CEntity::E43_Walk1F0`, the pre-order walk with the functor on each gated child.
pub(super) fn walk_pre_order_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    walk_run(ctx, r, EntityWalk::PreOrder(r.stack[0]), 4)
}

/// thiscall: `CEntity::Vf40`, the name walk.
pub(super) fn name_walk_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    walk_run(ctx, r, EntityWalk::Name, 0)
}

/// The pair walk's calls on the original's objects: node reads, the frame hooks (`Vf04..Vf18`) and the walkers
/// (`E43PairWalk`), through the host. The ops' walks in place (`matrices`) reach `walk` and `copy_vec4`.
pub(super) struct PairWalk<'a> {
    pub(super) mem: &'a mut Mem,
    pub(super) host: &'a dyn Host,
    pub(super) esp: u32,
    /// The depth below `esp` at which a child walk enters (the original's pushes for that walker).
    pub(super) walk_depth: u32,
}

impl PairOps for PairWalk<'_> {
    fn node(&self, at: u32) -> Node {
        self.mem.load_node(at)
    }

    fn hook(&mut self, at: u32, hook: Hook, a: u32, b: u32) {
        let index = match hook {
            Hook::One => 1,
            Hook::Two => 2,
            Hook::Three => 3,
            Hook::Four => 4,
            Hook::Five => 5,
            Hook::Six => 6,
        };
        let target = layout::vtable_target(self.mem, at, 4 * index);
        // The pure host places the call below this walker's entry, as it places a binding's call (the walker may run
        // in place, [`PairWalk::walk`]).
        let sp = (self.esp - 0x1000) & !0xf;
        // A `CEntityFrame` hook, run here as its binding (`frame_hook`) would.
        if let Some(hook) = frame_hook_at(target) {
            if sp != 0 {
                let frame = self.mem.load_entity_frame(at);
                let mut owner = FrameOwner { mem: &mut *self.mem, host: self.host, esp: sp };
                rbr_physics::entities::frame::entity_hook(&frame, hook, a, b, &mut owner);
                return;
            }
        }
        self.host.call_at(sp, target, at, &[a, b]);
    }

    fn walk(&mut self, which: Walker, receiver: u32, o2: u32, a: u32, b: u32) -> u32 {
        let sp = self.esp.wrapping_sub(self.walk_depth);
        // A dispatcher on a node and itself returns its `a` at once (`walk::run`): the identity walks of every
        // `FetchX` the probes make.
        let route = matches!(
            which,
            Walker::RoutePairOneTwo | Walker::RoutePairThreeFour | Walker::RoutePairFiveSix | Walker::RoutePairGrids
        );
        if route && receiver == o2 && sp != 0 {
            return a;
        }
        // The pure host runs the child walk here, as its binding (`pair_walker`) would, without the call's words: their
        // one reader was the passenger step's walk point, whose lane 3 is `crate::RESIDUE` there.
        if sp != 0 {
            let mut child = PairWalk { mem: &mut *self.mem, host: self.host, esp: sp, walk_depth: walk_depth(which) };
            return walk::run(&mut child, which, receiver, o2, a, b);
        }
        self.host.call_at(sp, rbr_physics::entities::layout::walker_address(which), receiver, &[o2, a, b]).eax
    }

    fn grid_count(&self, grid: u32) -> u32 {
        self.mem.grid_count(grid)
    }

    fn copy_vec4(&mut self, to: u32, from: u32) {
        copy_vec4(self.host, self.mem, to, from);
    }
}

/// `layout::copy_vec4` on the solids' store: `from` is a solid's vector at times (`MB560` on the car's position).
pub(super) fn copy_vec4(host: &dyn Host, mem: &mut Mem, to: u32, from: u32) {
    let solids = held_solids(host);
    if !solids.reaches(from, 16) && !solids.reaches(to, 16) {
        return layout::copy_vec4(mem, to, from);
    }
    if solids.read(from).is_some() || solids.read(to).is_some() {
        host.ran_direct(); // as `not_kept_on_held`
    }
    let mut store = solids.view(mem);
    if to.abs_diff(from) >= 16 {
        let words = store.words::<4>(from);
        store.set_words4(to, words);
        return;
    }
    for k in 0..4 {
        let word = store.word(from + 4 * k);
        store.set_word(to + 4 * k, word);
    }
}

/// The functor's calls: the callee enters below the functor's frame, the argument pushed at entry-4 and the call's
/// return address at entry-8.
struct Functor<'a> {
    mem: Mem,
    host: &'a dyn Host,
    esp: u32,
}

impl FunctorCalls for Functor<'_> {
    fn word(&self, at: u32) -> u32 {
        self.mem.u32(at)
    }

    fn call(&mut self, callee: u32, this: u32, arg: u32) -> u32 {
        crate::wheel::call_wheel(self.host, this, self.esp.wrapping_sub(8), callee, this, &[arg]).eax
    }
}

/// thiscall: `E43PairWalk::MFc00`, returns `this`; no calls.
pub(super) fn node_of_run(_ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    Ok(Return::void(layout::node_of(r.ecx), 0))
}

/// thiscall `(arg)`: `CPlainUnaryFunction::Vf04x`, the stored callback with `arg`; the caller's argument is popped (4),
/// and the callee's eax is the result.
pub(super) fn functor_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [arg, ..] = r.stack;
    let mut calls = Functor { mem: ctx.mem.clone(), host: ctx.host, esp: r.esp };
    let eax = call_stored(&mut calls, r.ecx, arg);
    Ok(Return::void(eax, 4))
}

/// thiscall `(a, out)`: `E44Shell::M51C8C0`, `out[i] = this[i] * a[i]` over four floats.
pub(super) fn mul4_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [a, out, ..] = r.stack;
    layout::componentwise_mul(&mut ctx.mem, r.ecx, a, out);
    Ok(Return::void(0, 8))
}

/// Where a walker's child walks enter below its entry esp (the original's pushes for that walker).
pub(super) fn walk_depth(which: Walker) -> u32 {
    match which {
        Walker::PairOneTwoShallower | Walker::PairThreeFourShallower | Walker::PairFiveSixShallower => 28,
        Walker::PairGridsShallower => 24,
        Walker::RoutePairFiveSix | Walker::RoutePairGrids => 20,
        _ => 32,
    }
}

/// thiscall `(o2, a, b)`: a pair walker (`walk::run`) on `r.ecx` against `o2`. The walk returns its `a`, the original's
/// eax.
fn pair_walker(ctx: &mut Ctx, r: &Regs, which: Walker) -> Result<Return, Decline> {
    let [o2, a, b, ..] = r.stack;
    let mut pairs = PairWalk { mem: &mut ctx.mem, host: ctx.host, esp: r.esp, walk_depth: walk_depth(which) };
    let eax = walk::run(&mut pairs, which, r.ecx, o2, a, b);
    Ok(Return::void(eax, 12))
}

/// `E43PairWalk::WFE0`: the receiver deeper than `o2` (hook One).
pub(super) fn pair_one_two_deeper_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    pair_walker(ctx, r, Walker::PairOneTwoDeeper)
}

/// `E43PairWalk::WB0C0`: the receiver deeper than `o2` (hooks Three and Four).
pub(super) fn pair_three_four_deeper_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    pair_walker(ctx, r, Walker::PairThreeFourDeeper)
}

/// `E43PairWalk::WB130`: the receiver shallower than `o2` (hook Four).
pub(super) fn pair_three_four_shallower_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    pair_walker(ctx, r, Walker::PairThreeFourShallower)
}

/// `E43PairWalk::W9D0`: equal-depth pair walk using owner hooks One and Two.
pub(super) fn pair_one_two_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    pair_walker(ctx, r, Walker::PairOneTwo)
}

/// `E43PairWalk::WA20`: equal-depth pair walk using owner hooks Three and Four.
pub(super) fn pair_three_four_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    pair_walker(ctx, r, Walker::PairThreeFour)
}

/// `E43PairWalk::WB050`: the receiver shallower than `o2` (hook Two).
pub(super) fn pair_one_two_shallower_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    pair_walker(ctx, r, Walker::PairOneTwoShallower)
}

/// `E43PairWalk::WA70`: equal-depth pair walk using owner hooks Five and Six.
pub(super) fn pair_five_six_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    pair_walker(ctx, r, Walker::PairFiveSix)
}

/// `E43PairWalk::WB1A0`: the receiver deeper than `o2` (hook Five).
pub(super) fn pair_five_six_deeper_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    pair_walker(ctx, r, Walker::PairFiveSixDeeper)
}

/// `E43PairWalk::WB210`: the receiver shallower than `o2` (hook Six).
pub(super) fn pair_five_six_shallower_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    pair_walker(ctx, r, Walker::PairFiveSixShallower)
}

/// `E43PairWalk::WAC0`: the grid pair walk.
pub(super) fn pair_grids_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    pair_walker(ctx, r, Walker::PairGrids)
}

/// `E43PairWalk::WB280`: the receiver deeper than `o2` (`E37Walk5E40`).
pub(super) fn pair_grids_deeper_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    pair_walker(ctx, r, Walker::PairGridsDeeper)
}

/// `E43PairWalk::WB2D0`: the receiver shallower than `o2` (`E37Walk5EB0`).
pub(super) fn pair_grids_shallower_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    pair_walker(ctx, r, Walker::PairGridsShallower)
}

/// `E43PairWalk::WB410`: the dispatcher to the grid walkers (`FetchY`'s grid walk).
pub(super) fn route_pair_grids_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    pair_walker(ctx, r, Walker::RoutePairGrids)
}

/// stdcall `(node, grids, n)`, ret 0xc: `E37Walk5E40` (hooks One and Three) or `E37Walk5EB0` (Two and Four); eax is
/// `grids`.
fn grid_hooks_run(ctx: &mut Ctx, r: &Regs, first: Hook, second: Hook) -> Result<Return, Decline> {
    let [node, grids, n, ..] = r.stack;
    let mut pairs = PairWalk { mem: &mut ctx.mem, host: ctx.host, esp: r.esp, walk_depth: 0 };
    let eax = walk::grid_hooks(&mut pairs, node, grids, n, first, second);
    Ok(Return::void(eax, 12))
}

pub(super) fn grid_hooks_lo_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    grid_hooks_run(ctx, r, Hook::One, Hook::Three)
}

pub(super) fn grid_hooks_hi_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    grid_hooks_run(ctx, r, Hook::Two, Hook::Four)
}

/// thiscall `(a, b)`: `E43PairWalk::MAB10`, `MAB20`, `MAB30` and `MAB40`, return `a`.
pub(super) fn pair_echo_run(_ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    Ok(Return::void(walk::return_first(r.ecx, r.stack[0], r.stack[1]), 8))
}

/// `E43PairWalk::WB320`: the dispatcher to the One-Two walkers.
pub(super) fn route_pair_one_two_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    pair_walker(ctx, r, Walker::RoutePairOneTwo)
}

/// `E43PairWalk::WB370`: the dispatcher to the Three-Four walkers.
pub(super) fn route_pair_three_four_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    pair_walker(ctx, r, Walker::RoutePairThreeFour)
}

/// `E43PairWalk::WB3C0`: the dispatcher to the Five-Six walkers. Its walk enters below the saved esi and the three
/// arguments.
pub(super) fn route_pair_five_six_run(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    pair_walker(ctx, r, Walker::RoutePairFiveSix)
}
