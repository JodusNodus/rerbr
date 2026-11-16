//! The physics core's typed state on the image (`notes/physics.md` "Store seam"): the words of the physics object
//! (`CPhysics`), of its state (`CPhysics::CState`) and the two step globals that the step and the contact loop write
//! ([`CoreState`], one [`Slot`] per field), their access ([`CoreFields`]: `Mem`'s on the image, [`CoreView`]'s on the
//! held state), the core in typed storage ([`Core`], one per world, not per car) and the round trip. The pointers
//! (state, definition, scratch), the step counter's NTweak header and dirty word (`SetDirty`), the entity lists and
//! their nodes' links, the scratch's solver and its event list (the collision's), and the definition stay in the image.

use std::cell::Cell;
use std::ops::Range;

use super::globals::PHYSICS_GLOBAL;
use crate::mem::Mem;
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
