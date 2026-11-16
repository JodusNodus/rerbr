//! The [`Host`]: a host call runs the binding at its
//! address on the same [`Mem`], so the bound functions call each other directly. A call to an address with no binding,
//! or a binding that declines, has no original to fall back to: it stops the run ([`Stop`]), or with `keep_going` is
//! logged and answers zero, so one run lists every address the step still lacks (after the first stop the
//! zeros change its path: rbr-recomp's `rerbr/harness/pure_check.py` lists them exactly, from the hybrid).
//!
//! Code outside the bindings whose effect is known runs as a stand-in ([`StandIn`]): the connections' `Invoke`
//! trampolines, functions that only return, the runner-only CRT allocator, the CRT sine and cosine and vector iterators,
//! `QueryPerformanceCounter`, and the listeners the car does not depend on (in the exe by address, in the DLLs by
//! their code).
//!
//! The stack is the snapshot's, in `Mem`: a call with `sp == 0` runs its callee from `(esp - 0x1000) & !0xf` below the
//! running binding's entry, as the bridge does, with the return address `CALL_RET` and the arguments written there.

use leaves::LEAVES;
use std::cell::{Cell, Ref, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use rbr_physics::math::trig;
use rbr_physics::mem::{Backend, Mem, PagesSnapshot};
use rbr_physics::stage::typed::Stage;
use rbr_physics::store::typed::Typed;

use crate::{binding_slot, bound, Called, Ctx, Decline, Host, Regs, Return, STACK_ARGS};

mod compare;
#[cfg(feature = "poison")]
mod gate;
mod heap;
mod image;
mod leaves;
mod playback;
mod stand_in;
mod watch;

use crate::engine_driveline::SIGNALS;
use crate::rigid_body::OPERATOR_NEW as CRT_NEW;
pub use compare::{
    frame_bytes, frame_words, is_ngp_padding, recorded_words, stale_mask, static_mask, word_name, Compare, Diff,
    FRAME_WORDS, NGP_PADDING_WHEEL,
};
use heap::{Base, RunGuard, RunHeap, RunMemory};
pub use heap::{ALLOCATOR_HEAP_BASE, ALLOCATOR_HEAP_END};
pub use image::{app_state, clock, set_app_state, time_keeper, CAR_BYTES, CAR_PTR};
pub use playback::{step, Playback, SENTINEL};
pub(crate) use stand_in::is_ret;
pub use stand_in::{stand_in, StandIn};
pub(crate) use watch::{Watch, OUTSIDE_ASSISTANCE_EMIT, TYRE_PARTICLES_EMIT, WATCHED};
pub use watch::{PARTICLE_EVENT_BYTES, TYRE_PARTICLES_BYTES};

/// The return address a host call leaves in its callee's slot; never mapped (`CALL_RET` in the retired `rust_step.py`).
pub const CALL_RET: u32 = 0xfeed_2000;

/// `CPhysics::StepForward`.
pub const STEP_FORWARD: u32 = 0x5273a0;
/// `CSignalManager::CMultiSignal1<const CSigCarControl&>::Emit`, on the signal at `[SIGNALS] + EMIT_OFFSET`: the
/// plugin's playback emits each step's recorded control with it before `StepForward(1)` (the retired `rust_step.py`'s `advance`).
pub const EMIT: u32 = 0x48cd90;
const EMIT_OFFSET: u32 = 0xc90;

const CRT_COS: u32 = 0x53f3a0;
const CRT_SIN: u32 = 0x53f3b0;
const VECTOR_CTOR: u32 = 0x70c408;
const VECTOR_DTOR: u32 = 0x70c4da;
/// The executable's static `operator new` (`msvcprt:newop_s`); `operator new[]` tail-jumps here.
const CRT_NEW_ARRAY: u32 = 0x70bf84;
/// The matching scalar and array delete entries in the executable's CRT block.
pub(crate) const CRT_DELETE: u32 = 0x70c260;
const CRT_DELETE_ARRAY: u32 = 0x70c27e;
/// The import table slot of `QueryPerformanceCounter`.
pub(crate) const QUERY_PERFORMANCE_COUNTER: u32 = 0x729190;

/// The subsystems whose calls, made or taken, the runner runs in place: the entity walk, the wheel and its tyre,
/// strut and VCU, collision and the rigid body. A call between two others (the physics core, the car body, the
/// engine, NTweak, NGP) still writes its stack words.
const IN_PLACE: [&str; 7] = ["entities", "wheel", "tyre", "spring_damper", "vcu", "collision", "rigid-body"];

/// `CScratch::Step` (0x5360c0) and NGP's wrapper of it (`0x1005b660`): after each (or the wheel's direct call,
/// [`Host::tyre_stepped`]), with [`Runner::keep_tyre_particles`], the term its `FillOutputs` (0x532f10) folded into the
/// tyre-noise block's word 1 is kept for the host ([`Runner::take_tyre_terms`]), since the frame finish that clears the
/// word does not run here.
const TYRE_STEPS: [u32; 2] = [0x5360c0, 0x1005_b660];

/// [`Runner::stack_words`] of a binding not run through the host yet, and of one that never runs as a plain call (the
/// tyre steps and the watched emits, whose runs the host keeps something of).
const UNSEEN: u8 = u8::MAX - 1;
const NOT_DIRECT: u8 = u8::MAX;

/// The entries of [`Runner::slot_of`]'s cache.
const SLOT_CACHE: usize = 1024;

/// The term `CScratch::FillOutputs` (0x532f10, `rbr_physics::tyre::noise_signal`) folds into word 1 of the tyre-noise block
/// of `scratch`, from the step it ended (`dt` its divisor, `Step`'s second word): `(tread depth + carcass radius - c -
/// state +0x48) / dt`, which nothing in `Step` changes after it. `None` when the step took the no-contact path, which
/// clears the block's flag (`+0x47c`).
fn tyre_term(host: &dyn Host, mem: &Mem, scratch: u32, dt: f32) -> Option<f32> {
    use rbr_physics::tyre::layout::{sig, DEF, DEF_CARCASS_RADIUS, DEF_TREAD_DEPTH};
    // The tyre's words as the step left them (the held tyre's while the step holds it); the def's are the image's.
    let (flag, hub, depth) = crate::tyre::read_tyre(host, scratch, |t| {
        (t.scratch.signal[sig::FLAG], t.scratch.hub_height, t.state.penetration_depth)
    });
    if flag == 0 {
        return None;
    }
    let def = mem.u32(scratch + DEF);
    Some((mem.f32(def + DEF_TREAD_DEPTH) + mem.f32(def + DEF_CARCASS_RADIUS) - hub - depth) / dt)
}

/// Why the pure run could not go on: the callee at `addr`, called from the binding at `caller` (0: the driver).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stop {
    Unbound { addr: u32, caller: u32 },
    Declined { addr: u32, caller: u32, why: &'static str },
}

/// The panic payload of a strict stop, so the driver tells it from a port's own failure.
#[derive(Debug)]
pub struct Stopped(pub Stop);

/// The memory of a run at the stage start, for an in-process restart ([`Runner::snapshot`]).
#[derive(Clone)]
pub struct RunSnapshot {
    pages: Option<PagesSnapshot>,
    heap: RunHeap,
}

/// Runs bindings on `mem`, which must share its bytes with its clones (a backend's, as `Snapshot::mem` gives).
pub struct Runner {
    /// The image the runs read and write (their clones share its bytes: [`Runner::lend`]). Between steps the code here
    /// reaches it through [`Runner::image`] and [`Runner::image_mut`] only.
    raw: Mem,
    /// The image [`Runner::image`] and [`Runner::image_mut`] hand out: the same bytes, and with the `poison` feature the
    /// gate's checks ([`gate`]).
    mem: Mem,
    heap: Arc<Mutex<RunHeap>>,
    /// The bindings running, which the allocator's unwritten-read log watches.
    run_depth: Arc<AtomicUsize>,
    /// Clones of `mem` for the runs' contexts and the host calls' argument writes, so a run takes one instead of
    /// cloning `mem` (and dropping the clone) each time ([`Runner::lend`]).
    spare: RefCell<Vec<Mem>>,
    /// The binding slots of the addresses entered last, by a hash of the address: `(address, slot)`, the slot
    /// `u32::MAX` for an address with no binding ([`Runner::slot_of`]).
    slots: Box<[Cell<(u32, u32)>; SLOT_CACHE]>,
    /// Log a stop and answer zero instead of stopping.
    pub keep_going: bool,
    /// [`stand_in`] of the addresses entered with no binding.
    stand_ins: RefCell<HashMap<u32, Option<StandIn>>>,
    /// The runs entered ([`Host::entered`]).
    entered: Cell<u64>,
    /// The running bindings (address and entry registers but the stack's), innermost last.
    frames: RefCell<Vec<(u32, Registers)>>,
    /// Every stop met (with `keep_going`), with its count.
    pub stops: RefCell<BTreeMap<Stop, u64>>,
    /// Bindings run at their own address, by their slot in [`bound`], and those run at another (NGP.dll's running
    /// addresses), by that address ([`Runner::ran`]).
    ran: Box<[Cell<u64>]>,
    ran_moved: RefCell<BTreeMap<u32, u64>>,
    /// When each stop was first met: the stops met before it, and the bindings run (all addresses) by then.
    pub first_met: RefCell<BTreeMap<Stop, (usize, u64)>>,
    /// `QueryPerformanceCounter`'s count.
    ticks: Cell<u64>,
    /// Whether each binding of [`bound`] is of an [`IN_PLACE`] subsystem, and whether the running one is.
    in_place_slots: Box<[bool]>,
    in_place_now: Cell<bool>,
    /// The stack words each binding pops, the most seen through the host ([`UNSEEN`] before its first run there,
    /// [`NOT_DIRECT`] for the bindings that always run through it).
    stack_words: Box<[Cell<u8>]>,
    /// Whether each binding of [`bound`] is a leaf ([`leaves::LEAVES`]).
    leaf_slots: Box<[bool]>,
    /// [`bound`], kept for the plain calls.
    bound: &'static [&'static crate::Binding],
    /// Whether the tyre-particle payloads are kept (the live viewer's particles; off in every check).
    pub keep_tyre_particles: bool,
    /// The watched emits' kept payloads and counts ([`Watch`]).
    captures: Watch,
    /// NGP.dll's base in this image, 0 without NGP: the runs see it through [`crate::ngp::Scope`].
    pub ngp_base: u32,
    /// The typed state from the first step on, held across steps: a step runs on it without a lift, [`Runner::image`]
    /// lowers it when it changed, [`Runner::image_mut`] lowers and drops it.
    held: RefCell<Option<Typed>>,
    /// Whether the held state changed since it was last lowered (a step or a run between steps on it).
    stale: Cell<bool>,
    /// Whether the run builds the car ([`Runner::construct`]): a binding that asks for the typed state lifts it for its
    /// own frame only ([`Runner::scoped`]), so the construction's writes between such bindings land in the image.
    constructing: Cell<bool>,
    /// The frame depth whose run lifted the typed state on demand while constructing: its exit lowers and drops it.
    scoped: Cell<Option<usize>>,
    /// The gate's checks behind `mem` (the `poison` feature).
    #[cfg(feature = "poison")]
    gate: Arc<gate::Gate>,
    /// The stage's typed state, lifted at the first step that holds it: the image holds one stage for the playback's
    /// life, and a step does not write it.
    stage: std::sync::OnceLock<Arc<Stage>>,
    /// `mem` with the typed state's words poisoned while a step holds them (the `poison` feature): the clones the runs
    /// get ([`Runner::lend`]).
    #[cfg(feature = "poison")]
    poisoned: RefCell<Option<Mem>>,
}

impl Runner {
    pub fn new(mem: Mem, keep_going: bool) -> Self {
        let runtime = mem.ngp_runtime().cloned();
        let run_depth = Arc::new(AtomicUsize::new(0));
        let heap = Arc::new(Mutex::new(RunHeap::new(run_depth.clone())));
        let pages = mem.pages().cloned();
        let backend = Arc::new(RunMemory { base: Base::new(mem), heap: heap.clone() });
        // Paged memory outside the allocator's region is read and written directly.
        let mut mem = match pages {
            Some(pages) => Mem::with_pages_around(pages, ALLOCATOR_HEAP_BASE..ALLOCATOR_HEAP_END, backend),
            None => Mem::with_backend(backend),
        };
        if let Some(runtime) = runtime {
            mem.set_ngp_runtime(runtime);
        }
        #[cfg(feature = "poison")]
        let gate = Arc::new(gate::Gate::new(mem.clone()));
        Runner {
            #[cfg(not(feature = "poison"))]
            mem: mem.clone(),
            #[cfg(feature = "poison")]
            mem: gated(&gate, &mem),
            #[cfg(feature = "poison")]
            gate,
            raw: mem,
            heap,
            run_depth,
            spare: RefCell::default(),
            slots: Box::new([const { Cell::new((u32::MAX, u32::MAX)) }; SLOT_CACHE]),
            keep_going,
            stand_ins: RefCell::default(),
            entered: Cell::new(0),
            frames: RefCell::default(),
            stops: RefCell::default(),
            ran: bound().iter().map(|_| Cell::new(0)).collect(),
            ran_moved: RefCell::default(),
            first_met: RefCell::default(),
            ticks: Cell::new(1 << 40),
            in_place_slots: bound().iter().map(|b| IN_PLACE.contains(&b.subsystem)).collect(),
            in_place_now: Cell::new(false),
            stack_words: bound()
                .iter()
                .map(|b| {
                    let host_only = TYRE_STEPS.contains(&b.addr) || WATCHED.contains(&b.addr);
                    Cell::new(if host_only { NOT_DIRECT } else { UNSEEN })
                })
                .collect(),
            leaf_slots: bound().iter().map(|b| LEAVES.contains(&b.addr)).collect(),
            bound: bound(),
            keep_tyre_particles: false,
            captures: Watch::default(),
            ngp_base: 0,
            held: RefCell::default(),
            stale: Cell::new(false),
            constructing: Cell::new(false),
            scoped: Cell::new(None),
            stage: Default::default(),
            #[cfg(feature = "poison")]
            poisoned: RefCell::default(),
        }
    }

    /// Lifts the car's and the physics core's typed state into typed storage for a step, with the stage's
    /// (nothing without a car, or while it is held already). With the `poison` feature
    /// the runs' memory has their words poisoned until [`Runner::lower_typed`]. Nothing is held before, so the image
    /// holds every word: the lift and the stage's read it directly.
    pub(crate) fn lift_typed(&self) {
        if self.held.borrow().is_some() {
            return;
        }
        let car = self.raw.u32(CAR_PTR);
        if car == 0 {
            return;
        }
        let stage = self.stage.get_or_init(|| Arc::new(Stage::lift(&self.raw))).clone();
        let typed = Typed::lift(&self.raw, car, stage);
        #[cfg(feature = "poison")]
        {
            let owned = typed.owned();
            self.gate.hold(&owned);
            let mut poisoned = self.raw.clone();
            poisoned.poison(owned);
            poisoned.watch_writes(typed.watched());
            *self.poisoned.borrow_mut() = Some(poisoned);
        }
        *self.held.borrow_mut() = Some(typed);
    }

    /// The typed state for a binding that asks for it with nothing held, inside a run: lifted as a step lifts it, and
    /// marked changed. Between steps it stays held as after a step; while constructing ([`Runner::construct`]) only for
    /// the asking binding's frame.
    #[cold]
    #[inline(never)]
    fn lift_on_demand(&self) {
        if self.run_depth.load(Ordering::Relaxed) == 0 {
            return;
        }
        self.lift_typed();
        if self.held.borrow().is_some() {
            self.set_stale(true);
            if self.constructing.get() {
                self.scoped.set(Some(self.frames.borrow().len()));
            }
        }
    }

    #[cold]
    #[inline(never)]
    fn release_scoped(&self) {
        self.scoped.set(None);
        self.lower_typed();
    }

    /// Runs `build`, a run that constructs the car (the boot's `CCar::UpdateA720`, which writes the pointers and
    /// definitions the lift reads), with each binding that asks for the typed state on a lift of its own, lowered and
    /// dropped when its frame exits; nothing is held after it.
    pub fn construct<R>(&self, build: impl FnOnce() -> R) -> R {
        self.lower_typed();
        self.constructing.set(true);
        let out = build();
        self.constructing.set(false);
        self.release_scoped();
        out
    }

    /// Marks the held state changed (after a step, or a run between steps on it), so [`Runner::image`] lowers it.
    pub(crate) fn held_changed(&self) {
        if self.held.borrow().is_some() {
            self.set_stale(true);
        }
    }

    fn set_stale(&self, stale: bool) {
        self.stale.set(stale);
        #[cfg(feature = "poison")]
        self.gate.set_stale(stale);
    }

    /// Lowers the held typed state into the image where it changed since the last lower, and drops it (the stage's
    /// stays as it is): the next step lifts again.
    pub(crate) fn lower_typed(&self) {
        #[cfg(feature = "poison")]
        self.poisoned.borrow_mut().take();
        if let Some(typed) = self.held.borrow_mut().take() {
            if self.stale.get() {
                typed.lower(&mut self.raw.clone());
            }
        }
        self.set_stale(false);
    }

    /// Drops the held typed state without lowering it (the image is about to be written whole: [`Runner::restore`]).
    fn drop_typed(&self) {
        #[cfg(feature = "poison")]
        self.poisoned.borrow_mut().take();
        self.held.borrow_mut().take();
        self.set_stale(false);
    }

    /// `raw` as the image, and `mem` over it.
    fn set_image(&mut self, raw: Mem) {
        assert!(self.held.borrow().is_none(), "the image changes with nothing held");
        #[cfg(feature = "poison")]
        {
            self.gate = Arc::new(gate::Gate::new(raw.clone()));
            self.mem = gated(&self.gate, &raw);
        }
        #[cfg(not(feature = "poison"))]
        {
            self.mem = raw.clone();
        }
        self.raw = raw;
    }

    /// Puts the allocator's blocks into the pages `mem` keeps the rest in (a boot image's), before anything runs: every
    /// access is then direct, and the bytes read before a caller wrote them are not logged
    /// ([`Runner::allocator_unwritten_reads`] stays empty). The blocks read and write as before: the region reads zero
    /// where nothing wrote, and a block is zero when allocated. For the live run; the checks keep the log.
    pub fn untrack_allocator(&mut self) {
        let Some(pages) = self.raw.pages().cloned() else {
            return;
        };
        let mut heap = self.heap.lock().unwrap_or_else(|p| p.into_inner());
        assert!(heap.live.is_empty() && self.runs() == 0, "the allocator moves before anything runs");
        pages.write(ALLOCATOR_HEAP_BASE, &vec![0; (ALLOCATOR_HEAP_END - ALLOCATOR_HEAP_BASE) as usize]);
        heap.pages = Some(pages.clone());
        drop(heap);
        let runtime = self.raw.ngp_runtime().cloned();
        let mut raw = Mem::with_pages(pages);
        if let Some(runtime) = runtime {
            raw.set_ngp_runtime(runtime);
        }
        self.set_image(raw);
    }

    /// The tyre-particle payloads emitted since the last call (with [`Runner::keep_tyre_particles`]), in emit order.
    pub fn take_tyre_particles(&self) -> Vec<[u8; TYRE_PARTICLES_BYTES]> {
        self.captures.take_tyre_particles()
    }

    /// The tyre steps since the last call (with [`Runner::keep_tyre_particles`]), in order: each one's scratch and the
    /// term its `FillOutputs` folded into the tyre-noise block's word 1, `None` for the no-contact path
    /// (`rbr_recording::sound::TyreMax`).
    pub fn take_tyre_terms(&self) -> Vec<(u32, Option<f32>)> {
        self.captures.take_tyre_terms()
    }

    /// The damage, damping and engine-fire emits since the last call (with [`Runner::keep_tyre_particles`]), in emit
    /// order: each one's kind ([`PARTICLE_EMITS`]) and payload.
    pub fn take_particle_events(&self) -> Vec<(u32, [u8; PARTICLE_EVENT_BYTES])> {
        self.captures.take_particle_events()
    }

    /// The `CSigEngineFire` emits since the last call.
    pub fn take_engine_fires(&self) -> u32 {
        self.captures.take_engine_fires()
    }

    /// The image for a reader between steps: what the step left, the typed state's words included. Every reader
    /// outside a step goes through this (or [`Runner::image_mut`]) rather than `mem`, so the typed state can stay held
    /// across steps: it is lowered into the image first when it changed since the last
    /// lower (it stays held). Not inside a run. With the `poison` feature, a clone of it read after the next step
    /// panics at a held word until this is called again.
    pub fn image(&self) -> &Mem {
        if self.stale.get() {
            self.lower_held();
        }
        &self.mem
    }

    #[cold]
    #[inline(never)]
    fn lower_held(&self) {
        assert_eq!(self.run_depth.load(Ordering::Relaxed), 0, "the image is lowered between steps only");
        if let Some(typed) = self.held.borrow().as_ref() {
            typed.lower(&mut self.raw.clone());
        }
        self.set_stale(false);
    }

    /// Runs `read` on the typed state held across steps, without lowering it; `None` when nothing is held (the image
    /// holds every word then: [`Runner::unheld_image`]). For readers between steps that run every step.
    pub fn with_held<R>(&self, read: impl FnOnce(&Typed) -> R) -> Option<R> {
        self.held.borrow().as_ref().map(read)
    }

    /// The image without lowering the held state, for a reader between steps of the words the typed state does not
    /// hold (pointers, headers, definitions): a held word in it may be older than the held state's ([`Runner::with_held`]
    /// reads those). With the `poison` feature, reading a held word through it while stale panics.
    pub fn unheld_image(&self) -> &Mem {
        &self.mem
    }

    /// The image for a writer between steps (a clone sharing its bytes: writes land in the image). With the typed
    /// state held across steps, it is lowered and dropped first, so the next step lifts what the writer left.
    pub fn image_mut(&self) -> Mem {
        self.lower_typed();
        self.mem.clone()
    }

    /// The `CSigCarDamageSpeech` codes emitted since the last call, a bit per code (`1 << code`).
    pub fn take_speech(&self) -> u32 {
        self.captures.take_speech()
    }

    /// The run's memory for an in-process restart: the pages overlay and the allocator heap's bookkeeping (its
    /// bytes live in the pages in an untracked run). Restoring writes the pages back onto themselves, so the
    /// snapshot's pages stay valid; the heap's shared pages follow.
    pub fn snapshot(&self) -> RunSnapshot {
        self.image();
        RunSnapshot {
            pages: self.raw.pages().map(|pages| pages.snapshot()),
            heap: self.heap.lock().unwrap_or_else(|p| p.into_inner()).clone(),
        }
    }

    /// Writes a [`Runner::snapshot`] back: the pages, then the allocator heap's bookkeeping. The typed state held across
    /// steps is dropped: the next step lifts the restored image's.
    pub fn restore(&mut self, snapshot: &RunSnapshot) {
        self.drop_typed();
        if let (Some(pages), Some(snap)) = (self.raw.pages(), snapshot.pages.as_ref()) {
            pages.restore(snap);
        }
        let mut heap = self.heap.lock().unwrap_or_else(|p| p.into_inner());
        let current = heap.pages.clone();
        *heap = snapshot.heap.clone();
        heap.pages = current;
    }

    /// Runs `CSpringDamper::ResetOutputs` on each strut — the wheel frame finish's damper reset, which the physics
    /// step does not run — and keeps each emitted payload as a typed damper event for the host. On the held struts,
    /// lifted for the call and lowered after it when nothing holds them (before the first step, after a reset).
    pub fn reset_strut_dampers(&self, struts: [u32; 4]) {
        let held = self.held.borrow().is_some();
        self.lift_typed();
        let mut mem = self.lend();
        for strut in struts {
            let payload = crate::spring_damper::on_strut_in(self, &mut mem, strut, |s, id| {
                rbr_physics::spring_damper::strut::reset_outputs(s, id)
            });
            self.emit_damper(strut, payload);
        }
        self.give_back(mem);
        self.held_changed();
        if !held {
            self.lower_typed();
        }
    }

    /// The damper-signal payloads emitted since the last call, in emit order: the strut and its payload.
    pub fn take_damper_signals(&self) -> Vec<(u32, rbr_physics::spring_damper::DamperSignal)> {
        self.captures.take_damper_signals()
    }

    /// The code of the `CSigOutsideAssistance` emitted since the last call, if any.
    pub fn take_outside_assistance(&self) -> Option<u32> {
        self.captures.take_outside_assistance()
    }

    /// The last `CSigCarStandStill` payload (count, kind) and `CSigCarProblem` kind emitted since the last call.
    pub fn take_hud_signals(&self) -> (Option<[u32; 2]>, Option<u32>) {
        self.captures.take_hud_signals()
    }

    /// The code of the `CSigRetireCar` emitted since the last call, if any.
    pub fn take_retire(&self) -> Option<u32> {
        self.captures.take_retire()
    }

    /// Whether the brake-wall emits since the last call came from inside an emitting wall and from a crossing of one
    /// (the rally school's `Checkpoint` inputs).
    pub fn take_wall_emits(&self) -> (bool, bool) {
        self.captures.take_wall_emits()
    }

    /// Runs the binding at `addr` from the entry registers `regs` (`regs.esp` its return address slot; `regs.stack`
    /// is read from there). A stop panics with [`Stopped`] unless `keep_going`.
    pub fn enter(&self, addr: u32, regs: Regs) -> Return {
        self.enter_with(addr, regs, self.lend())
    }

    /// [`Runner::enter`] with a clone of `mem` ([`Runner::lend`]) for the run's context.
    #[inline(always)]
    fn enter_with(&self, addr: u32, mut regs: Regs, mem: Mem) -> Return {
        // The outermost run opens the NGP scope ([`step`] opens its own); the runs inside it see the same base.
        let _ngp = (self.run_depth.load(Ordering::Relaxed) == 0).then(|| crate::ngp::Scope::new(self.ngp_base));
        let _run = RunGuard::new(&self.run_depth);
        self.entered.set(self.entered.get() + 1);
        self.raw.u32s_into(regs.esp + 4, &mut regs.stack);
        if WATCHED.contains(&addr) {
            self.captures.watch(&self.raw, addr, &regs, self.keep_tyre_particles);
        }
        match self.slot_of(crate::ngp::file_at(self.ngp_base, addr)) {
            Some(slot) => self.run_bound(slot, addr, &regs, mem),
            None => {
                self.give_back(mem);
                self.enter_unbound(addr, regs)
            }
        }
    }

    /// [`binding_slot`] of `addr`, from the cache when the address was entered before.
    #[inline]
    fn slot_of(&self, addr: u32) -> Option<usize> {
        let cell = &self.slots[((addr >> 4 ^ addr >> 13) as usize) % SLOT_CACHE];
        let (at, slot) = cell.get();
        if at != addr {
            let slot = binding_slot(addr).map_or(u32::MAX, |s| s as u32);
            cell.set((addr, slot));
            return (slot != u32::MAX).then_some(slot as usize);
        }
        (slot != u32::MAX).then_some(slot as usize)
    }

    /// The running binding's address (0: the driver), the caller a stop names.
    fn caller(&self) -> u32 {
        self.frames.borrow().last().map_or(0, |f| f.0)
    }

    /// [`Runner::enter`] of the binding in `slot` of [`bound`].
    #[inline(always)]
    fn run_bound(&self, slot: usize, addr: u32, regs: &Regs, mem: Mem) -> Return {
        let b = bound()[slot];
        let outer = self.in_place_now.replace(self.in_place_slots[slot]);
        let r = self.with_frame(addr, regs, || {
            let mut ctx = Ctx { mem, host: self.host() };
            let r = (b.run)(&mut ctx, regs);
            self.give_back(ctx.mem);
            r
        });
        self.in_place_now.set(outer);
        match r {
            Ok(r) => {
                self.ran_bound(slot, addr, regs);
                self.learn_pop(slot, r.pop);
                r
            }
            Err(Decline::Unsupported(why)) => self.stop(Stop::Declined { addr, caller: self.caller(), why }),
            Err(Decline::NotBound) => self.stop(Stop::Unbound { addr, caller: self.caller() }),
        }
    }

    /// The bookkeeping after the binding in `slot` ran at `addr`: its count, and the tyre step's term.
    #[inline(always)]
    fn ran_bound(&self, slot: usize, addr: u32, regs: &Regs) {
        let b = bound()[slot];
        if self.keep_tyre_particles && TYRE_STEPS.contains(&b.addr) {
            self.tyre_stepped(regs.ecx, f32::from_bits(regs.stack[1]));
        }
        if b.addr == addr {
            let ran = &self.ran[slot];
            ran.set(ran.get() + 1);
        } else {
            *self.ran_moved.borrow_mut().entry(addr).or_default() += 1;
        }
    }

    /// A binding's stack words, learned from what it pops at its runs through the host ([`Runner::stack_words`]).
    #[cold]
    fn learn_pop(&self, slot: usize, pop: u32) {
        let words = &self.stack_words[slot];
        if words.get() != NOT_DIRECT {
            // A cdecl binding pops nothing, so the words it reads are learned as none: only a call that passes none
            // runs it as a plain call.
            let popped = (pop / 4).min(STACK_ARGS as u32) as u8;
            if words.get() == UNSEEN || popped > words.get() {
                words.set(popped);
            }
        }
    }

    /// The registers of a call at `sp` (0: below the running binding's frame) on `ecx`: the other registers are the
    /// caller's entry values.
    #[inline(always)]
    fn call_regs(&self, sp: u32, ecx: u32) -> Regs {
        let outer = self.frames.borrow().last().map(|f| f.1).unwrap_or_default().regs();
        let esp = if sp == 0 { (outer.esp - 0x1000) & !0xf } else { sp };
        Regs { ecx, esp, ..outer }
    }

    /// The host the bindings get.
    #[inline(always)]
    fn host(&self) -> &dyn Host {
        self
    }

    /// [`Host::call_at`] through the host, out of line: the call's words written, and its callee entered as [`Runner::enter`] does.
    #[inline(never)]
    fn call_host(&self, sp: u32, addr: u32, ecx: u32, args: &[u32]) -> Called {
        let regs = self.call_regs(sp, ecx);
        let mut mem = self.lend();
        write_call(&mut mem, regs.esp, args);
        let r = self.enter_with(addr, regs, mem);
        Called { eax: r.eax, st0: r.st0.unwrap_or(f64::NAN) }
    }

    /// The plain call of `addr` at `sp` (None, nothing changed: the call goes through the host). The
    /// binding gets `args` as its stack words, and nothing is read back from the stack or counted but
    /// [`Host::entered`]. A leaf ([`leaves::LEAVES`]) of or from the [`IN_PLACE`] subsystems runs so when `args` are the
    /// words it pops ([`Runner::stack_words`]), with nothing written: a caller that passes more may read them after it, or
    /// call on with them as its next callee's (the pose step's parent-frame call leaves the arguments of 0x51b4c0,
    /// `rigid_body/pose.rs`). Another binding (B1b) runs in its own frame (the callees it leaves the placement of run
    /// below it, as through the host) as a run of its own subsystem ([`Runner::in_place_now`]): with `write`, after the
    /// call's words are written as the host writes them, when `args` hold at least the words it pops (a cdecl callee
    /// pops none); without, when they are exactly those words. The watched emits and the tyre steps never run so
    /// ([`NOT_DIRECT`]).
    #[inline(always)]
    fn call_plain(&self, sp: u32, addr: u32, ecx: u32, args: &[u32], write: bool) -> Option<Called> {
        match self.slot_of(crate::ngp::file_at(self.ngp_base, addr)) {
            Some(slot) => self.run_plain(slot, addr, self.call_regs(sp, ecx), args, write),
            None => self.invoke_plain(addr, self.call_regs(sp, ecx), args, write),
        }
    }

    /// [`Host::call_in_place`]: [`Runner::run_bound`] as [`Host::call_at`] enters it, but the call's words.
    #[inline(always)]
    fn run_in_place(&self, sp: u32, addr: u32, ecx: u32, args: &[u32]) -> Option<Called> {
        if WATCHED.contains(&addr) {
            return None;
        }
        let slot = self.slot_of(crate::ngp::file_at(self.ngp_base, addr))?;
        let mut regs = self.call_regs(sp, ecx);
        self.raw.u32s_into(regs.esp + 4, &mut regs.stack);
        regs.stack[..args.len()].copy_from_slice(args);
        Host::ran_direct(self);
        let r = self.run_bound(slot, addr, &regs, self.lend());
        Some(Called { eax: r.eax, st0: r.st0.unwrap_or(f64::NAN) })
    }

    /// [`Runner::call_plain`] of the binding in `slot` at `addr`, entered with `regs` but the stack words.
    #[inline(always)]
    fn run_plain(&self, slot: usize, addr: u32, mut regs: Regs, args: &[u32], write: bool) -> Option<Called> {
        let words = usize::from(self.stack_words[slot].get());
        if words != args.len() && !(write && words <= args.len()) {
            return None;
        }
        regs.stack[..args.len()].copy_from_slice(args);
        let run = self.bound[slot].run;
        let mut ctx = Ctx { mem: self.lend(), host: self as &dyn Host };
        let leaf = self.leaf_slots[slot] && (self.in_place_now.get() || self.in_place_slots[slot]);
        let r = if leaf && words == args.len() {
            run(&mut ctx, &regs)
        } else {
            if write {
                write_call(&mut ctx.mem, regs.esp, args);
            }
            self.entered.set(self.entered.get() + 1);
            let outer = self.in_place_now.replace(self.in_place_slots[slot]);
            let r = self.with_frame(addr, &regs, || run(&mut ctx, &regs));
            self.in_place_now.set(outer);
            r
        };
        self.give_back(ctx.mem);
        let r = r.ok()?;
        if leaf && words == args.len() {
            self.entered.set(self.entered.get() + 1);
        }
        Some(Called { eax: r.eax, st0: r.st0.unwrap_or(f64::NAN) })
    }

    /// [`Runner::call_plain`] of an address with no binding: an `Invoke` trampoline's method runs as a plain call, entered
    /// as the stand-in enters it ([`Runner::stand_in`]); the other stand-ins go through the host.
    #[inline(never)]
    fn invoke_plain(&self, addr: u32, mut regs: Regs, args: &[u32], write: bool) -> Option<Called> {
        let known = *self.stand_ins.borrow_mut().entry(addr).or_insert_with(|| stand_in(&self.raw, addr));
        let Some(StandIn::Invoke { method, owner_offset }) = known else {
            return None;
        };
        let conn = regs.ecx;
        let owner_offset = owner_offset.map_or(0, |at| self.raw.u32(conn + at));
        let target = self.raw.u32(conn + method);
        let slot = self.slot_of(crate::ngp::file_at(self.ngp_base, target))?;
        regs.ecx = self.raw.u32(conn + 0x10).wrapping_add(owner_offset);
        regs.eax = conn;
        self.entered.set(self.entered.get() + 1);
        self.run_plain(slot, target, regs, args, write)
    }

    /// [`Runner::enter`] of an address with no binding: its original's binding, or a stand-in.
    #[inline(never)]
    fn enter_unbound(&self, addr: u32, regs: Regs) -> Return {
        if let Some(exe) = crate::ngp::original_for(addr) {
            return self.enter(exe, regs);
        }
        match self.stand_in(addr, &regs) {
            Some(r) => r,
            None => self.stop(Stop::Unbound { addr, caller: self.caller() }),
        }
    }

    /// A clone of `mem`, from the spare ones when one still shares its bytes.
    #[inline(always)]
    fn lend(&self) -> Mem {
        #[cfg(feature = "poison")]
        if let Some(poisoned) = self.poisoned.borrow().as_ref() {
            return poisoned.clone();
        }
        match self.spare.borrow_mut().pop() {
            Some(mem) if mem.shares(&self.raw) => mem,
            _ => self.raw.clone(),
        }
    }

    /// [`Host::with_image`]: `run` on a [`Runner::lend`] of the image, given back after.
    fn on_image(&self, run: &mut dyn FnMut(&mut Mem) -> Option<u32>) -> Option<u32> {
        let mut mem = self.lend();
        let r = run(&mut mem);
        self.give_back(mem);
        r
    }

    /// Keeps a clone of [`Runner::lend`] for the next run (not a poisoned one).
    #[inline(always)]
    fn give_back(&self, mem: Mem) {
        #[cfg(feature = "poison")]
        if self.poisoned.borrow().is_some() {
            return;
        }
        self.spare.borrow_mut().push(mem);
    }

    /// Runs the stand-in at `addr` (see [`stand_in`]), or `None` without one.
    fn stand_in(&self, addr: u32, regs: &Regs) -> Option<Return> {
        let angle = f32::from_bits(regs.stack[0]);
        // The code and the import slot a stand-in is known by do not change while the image runs.
        let known = *self.stand_ins.borrow_mut().entry(addr).or_insert_with(|| stand_in(&self.raw, addr));
        let r = match known? {
            StandIn::Invoke { method, owner_offset } => {
                let conn = regs.ecx;
                let owner_offset = owner_offset.map_or(0, |at| self.raw.u32(conn + at));
                let owner = self.raw.u32(conn + 0x10).wrapping_add(owner_offset);
                let r = Regs { ecx: owner, eax: conn, ..*regs };
                self.enter(self.raw.u32(conn + method), r)
            }
            StandIn::Ret(pop) | StandIn::Listener(pop) => Return::void(regs.eax, pop),
            StandIn::Sin => Return::float_with_eax(regs.eax, trig::x87_sin(angle), 0),
            StandIn::Cos => Return::float_with_eax(regs.eax, trig::x87_cos(angle), 0),
            StandIn::VectorCtor => self.with_frame(addr, regs, || self.vector_ctor(regs.stack)),
            StandIn::VectorDtor => self.with_frame(addr, regs, || self.vector_dtor(regs.stack)),
            StandIn::Counter => self.counter(regs.stack[0]),
            StandIn::Allocate => {
                let ptr = self.heap.lock().unwrap_or_else(|p| p.into_inner()).allocate(regs.stack[0]);
                Return::void(ptr, 0)
            }
            StandIn::Free => {
                self.heap.lock().unwrap_or_else(|p| p.into_inner()).free(regs.stack[0]);
                Return::void(regs.eax, 0)
            }
        };
        Some(r)
    }

    /// `eh vector constructor iterator(ptr, size, count, ctor, dtor)`, stdcall: `ctor` (thiscall) on each element in
    /// order; eax is the count (0 for a count below 1).
    fn vector_ctor(&self, [ptr, size, count, ctor, ..]: [u32; STACK_ARGS]) -> Return {
        let n = (count as i32).max(0) as u32;
        for i in 0..n {
            self.host().call_direct(0, ctor, ptr.wrapping_add(i.wrapping_mul(size)), &[]);
        }
        Return::void(n, 0x14)
    }

    /// `eh vector destructor iterator(ptr, size, count, dtor)`, stdcall: `dtor` (thiscall) on each element from the
    /// last; eax is the last call's (`size * count` without one).
    fn vector_dtor(&self, [ptr, size, count, dtor, ..]: [u32; STACK_ARGS]) -> Return {
        let mut eax = size.wrapping_mul(count);
        let mut at = ptr.wrapping_add(eax);
        for _ in 0..(count as i32).max(0) {
            at = at.wrapping_sub(size);
            eax = self.host().call_direct(0, dtor, at, &[]).eax;
        }
        Return::void(eax, 0x10)
    }

    /// `QueryPerformanceCounter(out)`, stdcall: as `replay.py`'s stub, a counter that starts at 2^40 and goes up by
    /// 1000 a call (only the profiler reads it).
    fn counter(&self, out: u32) -> Return {
        let ticks = self.ticks.get() + 1000;
        self.ticks.set(ticks);
        let mut mem = self.raw.clone();
        mem.map(out, &ticks.to_le_bytes());
        Return::void(1, 4)
    }

    /// The bindings run so far (all addresses).
    pub fn runs(&self) -> u64 {
        self.ran.iter().map(Cell::get).sum::<u64>() + self.ran_moved.borrow().values().sum::<u64>()
    }

    /// Bindings run, by address.
    pub fn ran(&self) -> BTreeMap<u32, u64> {
        let mut out: BTreeMap<u32, u64> =
            self.ran.iter().zip(bound()).filter(|(n, _)| n.get() != 0).map(|(n, b)| (b.addr, n.get())).collect();
        for (&addr, &n) in self.ran_moved.borrow().iter() {
            *out.entry(addr).or_default() += n;
        }
        out
    }

    /// Addresses of stand-in block bytes read by the pure code before a caller wrote them.
    pub fn allocator_unwritten_reads(&self) -> Vec<u32> {
        self.heap.lock().unwrap_or_else(|p| p.into_inner()).unwritten_reads.iter().copied().collect()
    }

    fn stop(&self, stop: Stop) -> Return {
        let runs = self.runs();
        let mut first_met = self.first_met.borrow_mut();
        let order = first_met.len();
        first_met.entry(stop.clone()).or_insert((order, runs));
        drop(first_met);
        if !self.keep_going {
            std::panic::panic_any(Stopped(stop));
        }
        *self.stops.borrow_mut().entry(stop).or_default() += 1;
        Return::void(0, 0)
    }

    /// The running bindings and stand-ins, outermost first (after a port's panic, where it failed).
    pub fn chain(&self) -> Vec<u32> {
        self.frames.borrow().iter().map(|f| f.0).collect()
    }

    /// [`Runner::chain`] after a stop, its frames cleared: the stop's unwind skipped their pops, and a run after it
    /// (the live drive's restart) would report them again under its own.
    pub fn take_chain(&self) -> Vec<u32> {
        self.frames.take().into_iter().map(|f| f.0).collect()
    }

    /// Runs `f` as the code at `addr` entered with `regs` would: host calls in it are placed below `regs.esp` and
    /// name `addr` as their caller (the driver's own emits).
    #[inline(always)]
    pub fn with_frame<T>(&self, addr: u32, regs: &Regs, f: impl FnOnce() -> T) -> T {
        self.frames.borrow_mut().push((addr, Registers::of(regs)));
        let out = f();
        let depth = self.frames.borrow().len();
        if self.scoped.get() == Some(depth) {
            self.release_scoped();
        }
        self.frames.borrow_mut().pop();
        out
    }
}

/// `mem` behind `gate`'s checks, with its NGP runtime.
#[cfg(feature = "poison")]
fn gated(gate: &Arc<gate::Gate>, mem: &Mem) -> Mem {
    let mut gated = Mem::with_backend(gate.clone());
    if let Some(runtime) = mem.ngp_runtime().cloned() {
        gated.set_ngp_runtime(runtime);
    }
    gated
}

/// A frame's entry registers but the stack's: what a call from it keeps ([`Host::call_at`]).
#[derive(Clone, Copy, Default)]
struct Registers {
    ecx: u32,
    edx: u32,
    esp: u32,
    eax: u32,
    ebx: u32,
    esi: u32,
    edi: u32,
    ebp: u32,
}

impl Registers {
    fn of(r: &Regs) -> Self {
        let Regs { ecx, edx, esp, eax, ebx, esi, edi, ebp, .. } = *r;
        Registers { ecx, edx, esp, eax, ebx, esi, edi, ebp }
    }

    fn regs(self) -> Regs {
        let Registers { ecx, edx, esp, eax, ebx, esi, edi, ebp } = self;
        Regs { ecx, edx, esp, eax, ebx, esi, edi, ebp, stack: [0; STACK_ARGS] }
    }
}

/// The words a call at `esp` writes before its callee runs: the return address and `args`.
#[inline(always)]
fn write_call(mem: &mut Mem, esp: u32, args: &[u32]) {
    for (k, w) in std::iter::once(CALL_RET).chain(args.iter().copied()).enumerate() {
        mem.set_u32(esp + 4 * k as u32, w);
    }
}

impl std::fmt::Display for Stop {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Stop::Unbound { addr, caller } => {
                write!(f, "unbound {addr:#x} (called from {caller:#x})")
            }
            Stop::Declined { addr, caller, why } => {
                write!(f, "{addr:#x} declined (called from {caller:#x}): {why}")
            }
        }
    }
}

/// [`Runner`] as its bindings' host: a call runs as a plain call when it can ([`Runner::call_plain`]), and a skipped call
/// writes nothing.
impl Host for Runner {
    fn entered(&self) -> u64 {
        self.entered.get()
    }

    fn emit_damper(&self, strut: u32, signal: rbr_physics::spring_damper::DamperSignal) {
        self.captures.push_damper(strut, signal);
    }

    fn typed(&self) -> Ref<'_, Typed> {
        if let Ok(typed) = Ref::filter_map(self.held.borrow(), Option::as_ref) {
            return typed;
        }
        self.lift_on_demand();
        Ref::map(self.held.borrow(), |held| held.as_ref().expect("a run on a car holds its typed state"))
    }

    fn ran_direct(&self) {
        self.entered.set(self.entered.get() + 1);
    }

    fn tyre_stepped(&self, scratch: u32, dt: f32) {
        if self.keep_tyre_particles {
            self.captures.push_tyre_term(scratch, tyre_term(self, &self.raw, scratch, dt));
        }
    }

    fn query_counter(&self, out: u32) -> u32 {
        self.ran_direct();
        self.counter(out).eax
    }

    fn call_at(&self, sp: u32, addr: u32, ecx: u32, args: &[u32]) -> Called {
        assert!(args.len() <= STACK_ARGS, "host call {addr:#x}: too many arguments");
        match self.call_plain(sp, addr, ecx, args, true) {
            Some(called) => called,
            None => self.call_host(sp, addr, ecx, args),
        }
    }

    fn call_in_place(&self, sp: u32, addr: u32, ecx: u32, args: &[u32]) -> Option<Called> {
        self.run_in_place(sp, addr, ecx, args)
    }

    fn with_image(&self, run: &mut dyn FnMut(&mut Mem) -> Option<u32>) -> Option<u32> {
        self.on_image(run)
    }

    fn call_esp(&self, sp: u32) -> u32 {
        self.call_regs(sp, 0).esp
    }

    fn call_direct(&self, sp: u32, addr: u32, ecx: u32, args: &[u32]) -> Called {
        match self.call_plain(sp, addr, ecx, args, false) {
            Some(called) => called,
            None => self.call_at(sp, addr, ecx, args),
        }
    }
}

#[cfg(test)]
mod tests;
