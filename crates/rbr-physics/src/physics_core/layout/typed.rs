//! The physics core's typed state on the image (`notes/physics.md` "Store seam"): the words of the physics object
//! (`CPhysics`), of its state (`CPhysics::CState`) and the two step globals that the step and the contact loop write
//! ([`CoreState`], one [`Slot`] per field), their access ([`CoreFields`]: `Mem`'s on the image, [`CoreView`]'s on the
//! held state), the core in typed storage ([`Core`], one per world, not per car) and the round trip. The pointers
//! (state, definition, scratch), the step counter's NTweak header and dirty word (`SetDirty`), the entity lists and
//! their nodes' links, the scratch's solver and its event list (the collision's), and the definition stay in the image.

use std::cell::Cell;
use std::ops::Range;

use super::contact_walk::NodeHost;
use super::detector_hosts::CollideHost;
use super::globals::PHYSICS_GLOBAL;
use super::physics_object::StepHost;
use super::step_forward::StepStack;
use crate::mem::Mem;
use crate::physics_core::store::CoreStore;
use crate::rigid_body::layout::{CarSolids, HeldSolid, SolidWords};
use crate::store::{Object, Raw, Tabled, Value};

crate::object! {
    /// The physics object's changing words (`CPhysics`, at the physics global's word; names by use).
    pub struct PhysicsObj, size 0x64 {
        /// The solver's hits the contact loop counts under `DbgFlags` 0x20000: the pass's first count, its last and
        /// the maximum.
        hits @ 0x04: u32,
        hits_last @ 0x08: u32,
        hits_maximum @ 0x0c: u32,
        /// The contact loop's pass maximum and pass count.
        pass_maximum @ 0x14: u32,
        pass_count @ 0x18: u32,
        /// The solver's hit count after the penetration pass.
        contact_hit @ 0x1c: u32,
        /// The single stepper's delay and pending flag.
        single_step_delay @ 0x5c: u32,
        single_step_pending @ 0x60: u32,
    }
}

crate::object! {
    /// The physics state's changing words (`CPhysics::CState`, the physics object's `+0x20`).
    pub struct StateObj, size 0x28 {
        /// The substep counter: the value of the NTweak `CUnsigned` at `+0x0c` (its header and dirty word stay in the
        /// image).
        step_counter @ 0x18: u32,
        /// The physics clock, s.
        elapsed @ 0x20: f64,
    }
}

crate::object! {
    /// The step globals beside the physics global (`g_1659b70`): the step calls and the substep index the loop is in.
    pub struct StepGlobals, size 0xc {
        call_count @ 0x4: u32,
        step_index @ 0x8: u32,
    }
}

/// The core's typed state: the physics object's, its state's and the step globals' words.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CoreState {
    pub physics: PhysicsObj,
    pub state: StateObj,
    pub globals: StepGlobals,
}

/// Which object of the core a [`Slot`] is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Physics,
    State,
    Globals,
}

/// A field of [`CoreState`]: its object, its offset there (the image's word) and its place in the typed state.
pub struct Slot<T> {
    pub part: Part,
    pub at: u32,
    get: fn(&CoreState) -> T,
    set: fn(&mut CoreState, T),
}

impl<T> Clone for Slot<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Slot<T> {}

/// The slots of [`CoreState`], one per field, named as the field.
#[allow(non_upper_case_globals)]
pub mod slot {
    use super::*;

    macro_rules! slots {
        ($($part:ident $obj:ident $member:ident: $($field:ident: $ty:ty),*;)*) => {
            $($(pub const $field: Slot<$ty> = Slot {
                part: Part::$part,
                at: $obj::$field.at,
                get: |c| c.$member.$field,
                set: |c, v| c.$member.$field = v,
            };)*)*
        };
    }

    slots! {
        Physics PhysicsObj physics: hits: u32, hits_last: u32, hits_maximum: u32, pass_maximum: u32, pass_count: u32,
            contact_hit: u32, single_step_delay: u32, single_step_pending: u32;
        State StateObj state: step_counter: u32, elapsed: f64;
        Globals StepGlobals globals: call_count: u32, step_index: u32;
    }
}

/// The core's typed fields as a store finds them: `Mem` on the image (the per-call path), [`CoreView`] on the step's
/// held state. `CoreStore` is implemented over it (`mem_store.rs`). `base` is the object's address: the physics
/// object, its state, or the physics global's for the globals. The words of the bodies the core reads at their
/// addresses are the solids' ([`SolidWords`]: held by the car's store, `CarView::with_solids`, and the core's,
/// `CoreView::with_solids`).
pub trait CoreFields: SolidWords {
    fn core_field<T: Value>(&self, base: u32, slot: Slot<T>) -> T;
    fn set_core_field<T: Value>(&mut self, base: u32, slot: Slot<T>, value: T);
}

impl CoreFields for Mem {
    #[inline]
    fn core_field<T: Value>(&self, base: u32, slot: Slot<T>) -> T {
        T::load(self, base + slot.at)
    }

    #[inline]
    fn set_core_field<T: Value>(&mut self, base: u32, slot: Slot<T>, value: T) {
        value.store(self, base + slot.at)
    }
}

/// The physics core in typed storage: where its objects are and their state, in a cell (the step's runs nest through
/// the host: the contact loop's and the car's step run inside the step and read or write the same words).
#[derive(Clone, Debug, PartialEq)]
pub struct Core {
    pub physics: u32,
    pub state_at: u32,
    pub state: Cell<CoreState>,
}

impl Core {
    /// The physics object the physics global points at, and its state, lifted from the image.
    pub fn lift(mem: &Mem) -> Self {
        let physics = mem.u32(PHYSICS_GLOBAL);
        let state_at = mem.u32(physics + super::step_forward::STEP_STATE_AT);
        let state = CoreState {
            physics: PhysicsObj::lift(mem, physics),
            state: StateObj::lift(mem, state_at),
            globals: StepGlobals::lift(mem, PHYSICS_GLOBAL),
        };
        Core { physics, state_at, state: Cell::new(state) }
    }

    /// Writes the state back to the image.
    pub fn lower(&self, mem: &mut Mem) {
        let state = self.state.get();
        state.physics.lower(mem, self.physics);
        state.state.lower(mem, self.state_at);
        state.globals.lower(mem, PHYSICS_GLOBAL);
    }

    /// The address of the object `part`.
    fn base(&self, part: Part) -> u32 {
        match part {
            Part::Physics => self.physics,
            Part::State => self.state_at,
            Part::Globals => PHYSICS_GLOBAL,
        }
    }

    /// The field `slot` of the object at `base`, if this core holds it.
    #[inline]
    pub fn read<T>(&self, base: u32, slot: Slot<T>) -> Option<T> {
        (self.base(slot.part) == base).then(|| (slot.get)(&self.state.get()))
    }

    /// Stores the field `slot` of the object at `base`; false (nothing stored) unless this core holds it.
    #[inline]
    pub fn write<T>(&self, base: u32, slot: Slot<T>, value: T) -> bool {
        if self.base(slot.part) != base {
            return false;
        }
        let mut state = self.state.get();
        (slot.set)(&mut state, value);
        self.state.set(state);
        true
    }

    /// The core's tabled objects.
    pub fn tabled(&self) -> Vec<Tabled> {
        vec![
            Tabled::of::<PhysicsObj>("physics", self.physics),
            Tabled::of::<StateObj>("physics state", self.state_at),
            Tabled::of::<StepGlobals>("physics globals", PHYSICS_GLOBAL),
        ]
    }

    /// Every owned byte range, named by its object and field.
    pub fn owned(&self) -> Vec<(Range<u32>, String)> {
        self.tabled()
            .iter()
            .flat_map(|t| t.owned().map(|(f, r)| (r, format!("{} {}", t.name, f.name))).collect::<Vec<_>>())
            .collect()
    }

    /// The core as a store over `mem`.
    pub fn view<'a>(&'a self, mem: &'a mut Mem) -> CoreView<'a> {
        CoreView::new(mem, Some(self))
    }
}

/// The tabled objects of the physics core in `mem` (the physics object the global points at, its state, the globals).
pub fn core_objects(mem: &Mem) -> Vec<Tabled> {
    Core::lift(mem).tabled()
}

/// The core's store over the held typed state: the held objects' fields typed, any other object's on the image (the
/// per-call path), the held solids' words typed ([`SolidWords`]), and the image for the rest.
pub struct CoreView<'a> {
    mem: &'a mut Mem,
    core: Option<&'a Core>,
    solids: Option<&'a CarSolids>,
}

impl<'a> CoreView<'a> {
    /// The store over `mem` with `core` held (none: every word on the image).
    pub fn new(mem: &'a mut Mem, core: Option<&'a Core>) -> Self {
        CoreView { mem, core, solids: None }
    }

    /// The store with the car's solids held too (none: the solids' words on the image).
    pub fn with_solids(self, solids: Option<&'a CarSolids>) -> Self {
        CoreView { solids, ..self }
    }
}

impl Raw for CoreView<'_> {
    #[inline]
    fn mem(&self) -> &Mem {
        self.mem
    }
    #[inline]
    fn mem_mut(&mut self) -> &mut Mem {
        self.mem
    }
}

/// A solid's word at its address: the held solids' where their tables name it, else the image's.
impl SolidWords for CoreView<'_> {
    #[inline]
    fn word(&self, at: u32) -> u32 {
        self.solids.map_or_else(|| self.mem.u32(at), |s| s.word(self.mem, at))
    }

    #[inline]
    fn set_word(&mut self, at: u32, value: u32) {
        match self.solids {
            Some(s) => s.set_word(self.mem, at, value),
            None => self.mem.set_u32(at, value),
        }
    }

    #[inline]
    fn words<const N: usize>(&self, at: u32) -> [u32; N] {
        self.solids.map_or_else(|| self.mem.u32s(at), |s| s.words(self.mem, at))
    }

    #[inline]
    fn set_words4(&mut self, at: u32, words: [u32; 4]) {
        match self.solids {
            Some(s) => s.set_words4(self.mem, at, words),
            None => self.mem.set_words4(at, words),
        }
    }
    #[inline]
    fn held(&self, this: u32) -> Option<&HeldSolid> {
        self.solids?.held(this)
    }
}

impl CoreFields for CoreView<'_> {
    #[inline]
    fn core_field<T: Value>(&self, base: u32, slot: Slot<T>) -> T {
        self.core.and_then(|c| c.read(base, slot)).unwrap_or_else(|| T::load(self.mem, base + slot.at))
    }

    #[inline]
    fn set_core_field<T: Value>(&mut self, base: u32, slot: Slot<T>, value: T) {
        if !self.core.is_some_and(|c| c.write(base, slot, value)) {
            value.store(self.mem, base + slot.at)
        }
    }
}

/// `lower(lift(image))` of the core's typed state into an empty image, compared with `mem` on every owned byte: their
/// count, or the first byte that differs.
pub fn round_trip(mem: &Mem) -> Result<usize, String> {
    let core = Core::lift(mem);
    let mut out = Mem::new();
    core.lower(&mut out);
    let mut bytes = 0;
    for (range, name) in core.owned() {
        let len = (range.end - range.start) as usize;
        let (want, got) = (mem.read(range.start, len), out.read(range.start, len));
        if let Some(k) = want.iter().zip(&got).position(|(a, b)| a != b) {
            return Err(format!("{name} at {:#010x}: +{k:#x} differs after lower(lift)", range.start));
        }
        bytes += len;
    }
    Ok(bytes)
}

/// The calls a step and a contact loop make, logged as words (the slot or callee's tag, then its arguments), with
/// fixed answers: the solver hits once, its fraction is a quarter.
#[derive(Default)]
struct Calls(Vec<u32>);

impl Calls {
    fn log(&mut self, words: &[u32]) {
        self.0.extend_from_slice(words);
    }
}

impl<S> NodeHost<S> for Calls {
    fn force_input(&mut self, _: &mut S, node: u32) {
        self.log(&[0x5c, node]);
    }
    fn force_to_accel(&mut self, _: &mut S, node: u32) {
        self.log(&[0x64, node]);
    }
    fn integrate(&mut self, _: &mut S, node: u32, p: f32) {
        self.log(&[0x6c, node, p.to_bits()]);
    }
    fn child_walk(&mut self, _: &mut S, node: u32, p: f32) {
        self.log(&[0x58, node, p.to_bits()]);
    }
}

impl<S> StepHost<S> for Calls {
    fn pre_step(&mut self, sp: u32, target: u32, node: u32, dt: f32) {
        self.log(&[1, sp, target, node, dt.to_bits()]);
    }
    fn post_bodies(&mut self, sp: u32, target: u32, node: u32, dt: f32) {
        self.log(&[2, sp, target, node, dt.to_bits()]);
    }
    fn init_pass(&mut self, sp: u32, target: u32, node: u32) {
        self.log(&[3, sp, target, node]);
    }
    fn post_contact(&mut self, sp: u32, target: u32, node: u32) {
        self.log(&[4, sp, target, node]);
    }
    fn end_step(&mut self, sp: u32, target: u32, node: u32) {
        self.log(&[5, sp, target, node]);
    }
    fn telemetry_substep(&mut self, sp: u32, receiver: u32, count: u32) {
        self.log(&[6, sp, receiver, count]);
    }
    fn collide_window(&mut self, sp: u32, physics: u32, window: f32) {
        self.log(&[7, sp, physics, window.to_bits()]);
    }
    fn penetration_scope(&mut self, stack: StepStack, physics: u32, telemetry: u32) {
        self.log(&[8, stack.argument(), physics, telemetry]);
    }
    fn solver_clear_rows(&mut self, sp: u32, solver: u32) {
        self.log(&[9, sp, solver]);
    }
    fn solver_hit_count(&mut self, sp: u32, solver: u32) -> u32 {
        self.log(&[10, sp, solver]);
        1
    }
    fn solver_sweep_events(&mut self, sp: u32, solver: u32) {
        self.log(&[11, sp, solver]);
    }
    fn solver_step(&mut self, sp: u32, solver: u32) {
        self.log(&[12, sp, solver]);
    }
    fn solver_pass(&mut self, sp: u32, solver: u32) {
        self.log(&[13, sp, solver]);
    }
    fn solver_post(&mut self, sp: u32, target: u32, solver: u32) {
        self.log(&[14, sp, target, solver]);
    }
    fn telemetry_after_solve(&mut self, sp: u32, receiver: u32) {
        self.log(&[15, sp, receiver]);
    }
    fn reset_drive_part(&mut self, sp: u32, drive: u32) {
        self.log(&[16, sp, drive]);
    }
}

impl CollideHost for Calls {
    fn chan_enter(&mut self, sp: u32, chan: u32, sample: u32, name: u32) {
        self.log(&[20, sp, chan, sample, name]);
    }
    fn prof_exit(&mut self, sp: u32, chan: u32) {
        self.log(&[21, sp, chan]);
    }
    fn run_collision_detectors(&mut self, sp: u32, physics: u32, word: u32) {
        self.log(&[22, sp, physics, word]);
    }
    fn telemetry_events(&mut self, sp: u32, receiver: u32, events: u32) {
        self.log(&[23, sp, receiver, events]);
    }
    fn telemetry_after_solve(&mut self, sp: u32, receiver: u32) {
        self.log(&[24, sp, receiver]);
    }
    fn solver_sweep_events(&mut self, sp: u32, solver: u32) {
        self.log(&[25, sp, solver]);
    }
    fn solver_fraction(&mut self, sp: u32, solver: u32) -> f64 {
        self.log(&[26, sp, solver]);
        0.25
    }
    fn solver_hit_count(&mut self, sp: u32, solver: u32) -> u32 {
        self.log(&[27, sp, solver]);
        2
    }
    fn solver_pass(&mut self, sp: u32, solver: u32) {
        self.log(&[28, sp, solver]);
    }
    fn solver_post(&mut self, sp: u32, target: u32, solver: u32) {
        self.log(&[29, sp, target, solver]);
    }
    fn advance_contact_bodies(&mut self, sp: u32, physics: u32, fraction: f32) {
        self.log(&[30, sp, physics, fraction.to_bits()]);
    }
    fn advance_contact_bodies_remainder(&mut self, sp: u32, physics: u32, window: f32) {
        self.log(&[31, sp, physics, window.to_bits()]);
    }
    fn init_pass(&mut self, sp: u32, target: u32, node: u32) {
        self.log(&[32, sp, target, node]);
    }
    fn post_contact(&mut self, sp: u32, target: u32, node: u32) {
        self.log(&[33, sp, target, node]);
    }
}

/// The core's entries in a row on the physics object the global points at, with the host calls answered by [`Calls`]
/// and their stack words below `entry`: three steps of the step, then the contact loop with the solver's hits counted
/// (`DbgFlags` 0x20000 set first). Returns the logged calls and the contact loop's pass count.
fn entries<S: CoreStore>(store: &mut S, entry: u32) -> Vec<u32> {
    let physics = store.u32(PHYSICS_GLOBAL);
    let def = store.u32(physics + 0x24);
    store.set_u32(def + 0x160e0, store.u32(def + 0x160e0) | 0x2_0000);
    let mut calls = Calls::default();
    crate::physics_core::step::step_forward(store, &mut calls, physics, 3, 0x1234_5678, StepStack::new(entry));
    let passes = crate::physics_core::contact_loop::collide_window(store, &mut calls, physics, 0.02, entry);
    calls.log(&[passes]);
    calls.0
}

/// The core's entries ([`entries`]) per call on `per_call` (the image's words) and held on `held` (a copy of the same
/// image: [`CoreView`] over the lifted core, lowered after), compared on every call's words, every byte of the physics
/// object, its state's first `0x30` bytes, the globals and the entries' stack frame below `entry`: their count, or the
/// first that differs.
pub fn differential(per_call: &mut Mem, held: &mut Mem, entry: u32) -> Result<usize, String> {
    let a = entries(per_call, entry);
    let core = Core::lift(held);
    let b = entries(&mut core.view(held), entry);
    if a != b {
        let k = a.iter().zip(&b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
        return Err(format!("the calls differ at word {k}: per call {:x?}, held {:x?}", a.get(k), b.get(k)));
    }
    core.lower(held);
    let mut bytes = 4 * a.len();
    for (at, len, name) in [
        (core.physics, 0x100, "physics"),
        (core.state_at, 0x30, "physics state"),
        (PHYSICS_GLOBAL, 0xc, "physics globals"),
        (entry - 0x60, 0x68, "stack"),
    ] {
        let (want, got) = (per_call.read(at, len), held.read(at, len));
        if let Some(i) = want.iter().zip(&got).position(|(x, y)| x != y) {
            return Err(format!("{name} +{i:#x} differs: per call {:#04x}, held {:#04x}", want[i], got[i]));
        }
        bytes += len;
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::check_table;

    const PHYSICS: u32 = 0x2000;
    const STATE: u32 = 0x3000;

    fn image() -> Mem {
        let mut mem = Mem::new();
        mem.map(PHYSICS_GLOBAL, &[0; 0xc]);
        mem.map(PHYSICS, &(0..0x64u32).map(|k| (k * 7 + 3) as u8).collect::<Vec<_>>());
        mem.map(STATE, &(0..0x28u32).map(|k| (k * 13 + 5) as u8).collect::<Vec<_>>());
        mem.set_u32(PHYSICS_GLOBAL, PHYSICS);
        mem.set_u32(PHYSICS_GLOBAL + 4, 0x1234);
        mem.set_u32(PHYSICS_GLOBAL + 8, 7);
        mem.set_u32(PHYSICS + 0x20, STATE);
        mem
    }

    #[test]
    fn tables_are_ordered_and_fit() {
        check_table::<PhysicsObj>();
        check_table::<StateObj>();
        check_table::<StepGlobals>();
    }

    #[test]
    fn round_trip_keeps_every_owned_byte() {
        assert_eq!(round_trip(&image()), Ok(8 * 4 + 4 + 8 + 2 * 4));
    }

    /// The view's fields are the held state's for the held objects, the image's for another, and stores go to the same.
    #[test]
    fn view_holds_the_core_and_passes_the_rest() {
        let mut mem = image();
        let core = Core::lift(&mem);
        let mut view = core.view(&mut mem);
        view.set_core_field(PHYSICS, slot::pass_count, 9);
        view.set_core_field(STATE, slot::elapsed, 2.5);
        view.set_core_field(PHYSICS_GLOBAL, slot::step_index, 3);
        view.set_core_field(0x4000, slot::pass_count, 11);
        assert_eq!(view.core_field(PHYSICS, slot::pass_count), 9);
        assert_eq!(view.core_field(STATE, slot::elapsed), 2.5);
        assert_eq!(view.core_field(0x4000, slot::pass_count), 11);
        assert_ne!(mem.u32(PHYSICS + 0x18), 9);
        assert_eq!(mem.u32(PHYSICS_GLOBAL + 8), 7);
        assert_eq!(mem.u32(0x4018), 11);
        core.lower(&mut mem);
        assert_eq!(mem.u32(PHYSICS + 0x18), 9);
        assert_eq!(f64::load(&mem, STATE + 0x20), 2.5);
        assert_eq!(mem.u32(PHYSICS_GLOBAL + 8), 3);
    }
}
