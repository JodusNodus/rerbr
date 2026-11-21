//! The car's typed state on the image (`notes/physics.md` "Store seam"): the `CCar`'s own words that only the car's
//! functions read and write during a step ([`CarState`], one [`Slot`] per field), their access ([`CarFields`]: `Mem`'s
//! on the image, [`CarView`]'s on the held state), the car in typed storage ([`CarOwn`]). The solid's
//! words (`< +0x470`: pose, velocities, accumulators) are the solids' ([`SolidWords`], held with
//! [`CarView::with_solids`]); the pointers, the parts (drive, engine, VCU scratch, lights, passenger), the value cells'
//! headers (`SetDirty`), the damage handler and every word another subsystem reads or writes at its address stay in the
//! image.

use std::cell::RefCell;
use std::ops::Range;

use crate::math::vec::Vec4;
use crate::mem::Mem;
use crate::physics_core::layout::typed::{Core, CoreFields, Slot as CoreSlot};
use crate::rigid_body::layout::{CarSolids, HeldSolid, SolidWords};
use crate::store::defs::{self, Defs};
use crate::store::{Object, Raw, Value};
use crate::wheel::layout::{slot as wheel_slot, CarWheels};

/// A field of [`CarState`]: its offset in the car (the image's word) and its place in the typed state.
pub struct Slot<T> {
    pub at: u32,
    get: fn(&CarState) -> T,
    set: fn(&mut CarState, T),
}

impl<T> Clone for Slot<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Slot<T> {}

/// `object!` with a [`Slot`] per field (`slot::field`), as the wheel's state.
macro_rules! car_state {
    (
        $(#[$meta:meta])*
        pub struct $name:ident, size $size:literal {
            $( $(#[$fmeta:meta])* $field:ident @ $at:literal : $ty:ty ),* $(,)?
        }
    ) => {
        crate::object! {
            $(#[$meta])*
            pub struct $name, size $size {
                $( $(#[$fmeta])* $field @ $at : $ty, )*
            }
        }

        /// The slots of [`CarState`], one per field, named as the field.
        #[allow(non_upper_case_globals)]
        pub mod slot {
            use super::*;
            $( pub const $field: Slot<$ty> = Slot { at: $at, get: |c| c.$field, set: |c, v| c.$field = v }; )*
        }
    };
}

car_state! {
    /// The car's typed state (names by use, `CarStore`): the controls step's counter, marks and kept controls, the
    /// contact timers and flags, the value cells' values (their headers stay in the image), the smoothed vector, the
    /// speeds and direction the step stores, the respawn countdown and the assistance block. Offsets are the car's.
    pub struct CarState, size 0x1d60 {
        /// The control update's step counter and, at its first four thousand-step marks, the timer word and the
        /// payload's steering.
        step_count @ 0x950: u32,
        step_mark_timers @ 0x954: [u32; 4],
        step_mark_steering_0 @ 0x974: u32,
        step_mark_steering_1 @ 0x994: u32,
        step_mark_steering_2 @ 0x9b4: u32,
        step_mark_steering_3 @ 0x9d4: u32,
        /// The snapshot flag, the partial and wall contact times, the partial-contact flag.
        snapshot_on @ 0x9e4: u32,
        partial_contact @ 0x9e8: f32,
        wall_contact @ 0x9ec: f32,
        partial_contact_flag @ 0x9f0: u32,
        /// The values of the anti-roll bars' matrix cells `(i, j)`, `+0xa28 + 0x10 (j + 4 i)`.
        roll_bar_0_0 @ 0xa28: f32,
        roll_bar_0_1 @ 0xa38: f32,
        roll_bar_0_2 @ 0xa48: f32,
        roll_bar_0_3 @ 0xa58: f32,
        roll_bar_1_0 @ 0xa68: f32,
        roll_bar_1_1 @ 0xa78: f32,
        roll_bar_1_2 @ 0xa88: f32,
        roll_bar_1_3 @ 0xa98: f32,
        /// The values of the steered wheels' torque sum, the filtered steering torque, the steering slew, target and
        /// target rate.
        steer_torque_sum @ 0xadc: f32,
        ffb_torque @ 0xaec: f32,
        steer_slew @ 0xafc: f32,
        steer_target @ 0xb0c: f32,
        steer_target_rate @ 0xb1c: f32,
        smoothed_vector @ 0x15d0: Vec4,
        respawn_countdown @ 0x15e4: f32,
        /// The value of the speed sum's cell.
        speed_sum @ 0x15f4: f32,
        forward_speed @ 0x15f8: f32,
        speed @ 0x15fc: f32,
        overload_flag @ 0x1668: u32,
        wall_penetration @ 0x166c: f32,
        override_timer @ 0x1670: f32,
        /// The four words the outside assistance clears.
        assistance_block @ 0x1680: [u32; 4],
        /// The kept throttle and brake words and the steering input (the payload's words).
        throttle @ 0x1694: u32,
        brake @ 0x1698: u32,
        steer_input @ 0x16a4: f32,
        grid_pushed @ 0x16a8: u32,
    }
}

/// The slots of the step marks' steering, by mark.
pub const STEP_MARK_STEERING: [Slot<u32>; 4] =
    [slot::step_mark_steering_0, slot::step_mark_steering_1, slot::step_mark_steering_2, slot::step_mark_steering_3];

/// The slots of the value cells, typed: an offset of one of them (a cell's value, its real's `+0xc`) is typed, any
/// other cell value stays the image's.
const CELL_VALUES: [Slot<f32>; 14] = [
    slot::roll_bar_0_0,
    slot::roll_bar_0_1,
    slot::roll_bar_0_2,
    slot::roll_bar_0_3,
    slot::roll_bar_1_0,
    slot::roll_bar_1_1,
    slot::roll_bar_1_2,
    slot::roll_bar_1_3,
    slot::steer_torque_sum,
    slot::ffb_torque,
    slot::steer_slew,
    slot::steer_target,
    slot::steer_target_rate,
    slot::speed_sum,
];

/// The typed slot of the cell value at `offset` in the car, if it is one.
pub fn cell_value_slot(offset: u32) -> Option<Slot<f32>> {
    CELL_VALUES.into_iter().find(|s| s.at == offset)
}

/// The car's typed fields and the wheels' ground flags as a store finds them: `Mem` on the image (the per-call path),
/// [`CarView`] on the step's held state. `CarStore` is implemented over it (`car.rs`); the physics core's fields it reads
/// (the clock and the substep counter) are the core's (`CoreFields`).
pub trait CarFields: CoreFields {
    /// The field `slot` of the car at `car`.
    fn field<T: Value>(&self, car: u32, slot: Slot<T>) -> T;
    /// Stores the field `slot` of the car at `car`.
    fn set_field<T: Value>(&mut self, car: u32, slot: Slot<T>, value: T);
    /// The ground flag of the wheel at `wheel` (the wheel's typed state's).
    fn wheel_on_ground(&self, wheel: u32) -> u32;
    /// The word at `off` of the car definition at `def` (the image's; a typed store's held definition's where it holds
    /// the word, `CarView`).
    #[inline]
    fn def_u32(&self, def: u32, off: u32) -> u32 {
        self.u32(def + off)
    }
    #[inline]
    fn def_f32(&self, def: u32, off: u32) -> f32 {
        f32::from_bits(self.def_u32(def, off))
    }
}

impl CarFields for Mem {
    #[inline]
    fn field<T: Value>(&self, car: u32, slot: Slot<T>) -> T {
        T::load(self, car + slot.at)
    }

    #[inline]
    fn set_field<T: Value>(&mut self, car: u32, slot: Slot<T>, value: T) {
        value.store(self, car + slot.at)
    }

    #[inline]
    fn wheel_on_ground(&self, wheel: u32) -> u32 {
        self.u32(wheel + wheel_slot::on_ground.at)
    }
}

/// The car's own state in typed storage: its address and state, in a `RefCell` borrowed for one field at a time, so
/// the car's runs nest through the host (the controls step runs the drive, whose wheels add to the torque sum; the
/// step runs the retire check) on the same state.
#[derive(Clone, Debug, PartialEq)]
pub struct CarOwn {
    pub at: u32,
    pub state: RefCell<CarState>,
}

impl CarOwn {
    /// The car at `car`, lifted from the image.
    pub fn lift(mem: &Mem, car: u32) -> Self {
        CarOwn { at: car, state: RefCell::new(CarState::lift(mem, car)) }
    }

    /// Writes the state back to the image.
    pub fn lower(&self, mem: &mut Mem) {
        self.state.borrow().lower(mem, self.at);
    }

    /// The field `slot` of the car at `car`, if it is this one.
    pub fn read<T>(&self, car: u32, slot: Slot<T>) -> Option<T> {
        (car == self.at).then(|| (slot.get)(&self.state.borrow()))
    }

    /// Every owned byte range, named by its field.
    pub fn owned(&self) -> Vec<(Range<u32>, String)> {
        let at = self.at;
        CarState::FIELDS.iter().map(|f| (at + f.at..at + f.at + f.len, format!("car {}", f.name))).collect()
    }
}

/// The car's store over the held typed state: the held car's fields and the held wheels' ground flags typed, any other
/// car's and wheel's on the image (the per-call path), the held solids' words typed ([`SolidWords`]), and the image for
/// the rest.
pub struct CarView<'a> {
    mem: &'a mut Mem,
    own: Option<&'a CarOwn>,
    wheels: Option<&'a CarWheels>,
    defs: Option<&'a Defs>,
    core: Option<&'a Core>,
    solids: Option<&'a CarSolids>,
}

impl<'a> CarView<'a> {
    /// The store over `mem` with the car `own` and its `wheels` held (none: every word on the image).
    pub fn new(mem: &'a mut Mem, own: Option<&'a CarOwn>, wheels: Option<&'a CarWheels>) -> Self {
        CarView { mem, own, wheels, defs: None, core: None, solids: None }
    }

    /// The store with the step's typed definitions (none: the car's definition on the image).
    pub fn with_defs(self, defs: Option<&'a Defs>) -> Self {
        CarView { defs, ..self }
    }

    /// The store with the physics core `core` held too.
    pub fn with_core(self, core: Option<&'a Core>) -> Self {
        CarView { core, ..self }
    }

    /// The store with the car's solids held too (none: the solids' words on the image).
    pub fn with_solids(self, solids: Option<&'a CarSolids>) -> Self {
        CarView { solids, ..self }
    }

    fn held(&self, car: u32) -> Option<&'a CarOwn> {
        self.own.filter(|own| own.at == car)
    }
}

impl Raw for CarView<'_> {
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
impl SolidWords for CarView<'_> {
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

impl CoreFields for CarView<'_> {
    #[inline]
    fn core_field<T: Value>(&self, base: u32, slot: CoreSlot<T>) -> T {
        self.core.and_then(|c| c.read(base, slot)).unwrap_or_else(|| T::load(self.mem, base + slot.at))
    }

    #[inline]
    fn set_core_field<T: Value>(&mut self, base: u32, slot: CoreSlot<T>, value: T) {
        if !self.core.is_some_and(|c| c.write(base, slot, value)) {
            value.store(self.mem, base + slot.at)
        }
    }
}

impl CarFields for CarView<'_> {
    #[inline]
    fn field<T: Value>(&self, car: u32, slot: Slot<T>) -> T {
        match self.held(car) {
            Some(own) => (slot.get)(&own.state.borrow()),
            None => T::load(self.mem, car + slot.at),
        }
    }

    #[inline]
    fn set_field<T: Value>(&mut self, car: u32, slot: Slot<T>, value: T) {
        match self.held(car) {
            Some(own) => (slot.set)(&mut own.state.borrow_mut(), value),
            None => value.store(self.mem, car + slot.at),
        }
    }

    #[inline]
    fn wheel_on_ground(&self, wheel: u32) -> u32 {
        let on_ground = wheel_slot::on_ground;
        self.wheels.and_then(|w| w.read(wheel, on_ground)).unwrap_or_else(|| self.mem.u32(wheel + on_ground.at))
    }

    #[inline]
    fn def_u32(&self, def: u32, off: u32) -> u32 {
        let image = || self.mem.u32(def + off);
        match self.defs.and_then(|d| d.car(def)).and_then(|d| d.word_of(off)) {
            Some(word) => defs::checked(word, image),
            None => image(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::car::*;
    use super::*;
    use crate::car_body::store::{CarStore, Cell};
    use crate::store::check_table;

    const CAR_AT: u32 = 0x10000;

    #[test]
    fn table_is_ordered_and_fits_and_names_the_layout() {
        check_table::<CarState>();
        for (at, named) in [
            (slot::step_count.at, STEP_COUNT),
            (slot::snapshot_on.at, SNAPSHOT_ON),
            (slot::partial_contact.at, PARTIAL_CONTACT),
            (slot::wall_contact.at, WALL_CONTACT),
            (slot::partial_contact_flag.at, super::super::PARTIAL_CONTACT_FLAG),
            (slot::steer_target_rate.at, STEER_TARGET_RATE),
            (slot::speed.at, SPEED),
            (slot::overload_flag.at, OVERLOAD_FLAG),
            (slot::wall_penetration.at, WALL_PENETRATION),
            (slot::override_timer.at, OVERRIDE_TIMER),
            (slot::steer_input.at, STEER_INPUT),
            (slot::grid_pushed.at, GRID_PUSHED),
            (slot::throttle.at, super::super::THROTTLE_AT),
            (slot::brake.at, super::super::BRAKE_AT),
        ] {
            assert_eq!(at, named);
        }
        for mark in 0..4 {
            assert_eq!(STEP_MARK_STEERING[mark as usize].at, super::super::step_mark_steering_at(0, mark));
            assert_eq!(slot::step_mark_timers.at + 4 * mark, super::super::step_mark_timer_at(0, mark));
        }
        for (i, j) in (0..2).flat_map(|i| (0..4).map(move |j| (i, j))) {
            assert!(cell_value_slot(roll_bar_cell_at(0, i, j) + 0xc).is_some(), "({i}, {j})");
        }
        for at in [steer_slew_at(0), steer_target_at(0), steer_target_rate_at(0), steer_torque_sum_at(0)] {
            assert!(cell_value_slot(at + 0xc).is_some(), "{at:#x}");
        }
        // The pointers, the solid's words, the cells' headers and the parts are not owned.
        for at in [0x34, 0x140, 0x190, 0x1c0, 0x370, 0x470, 0x4a0, 0x4b0, 0x690, DAMAGE_HANDLER, 0xae8, 0x15f0, 0x1690]
            .into_iter()
            .chain([SPEED_PRE, DIR_BACK])
        {
            assert!(CarState::field_at(at).is_none(), "{at:#x}");
        }
    }

    /// A car image of `pattern`-derived bytes.
    fn image(pattern: u8) -> Mem {
        let mut mem = Mem::new();
        let bytes: Vec<u8> = (0..0x1d60u32).map(|k| (k as u8).wrapping_mul(29) ^ pattern ^ (k >> 8) as u8).collect();
        mem.map(CAR_AT, &bytes);
        mem
    }

    #[test]
    fn the_view_reads_and_writes_the_held_state_and_the_image_otherwise() {
        let mut mem = image(0);
        let held = CarOwn::lift(&mem, CAR_AT);
        let mut view = CarView::new(&mut mem, Some(&held), None);
        view.set_override_timer(CAR_AT, 2.5);
        view.set_wall_contact(CAR_AT, 0.25);
        let cell = view.cell(CAR_AT, Cell::FfbTorque);
        view.store_cell(CAR_AT, Cell::FfbTorque, crate::car_body::accessors::set_cell(cell, 7.0));
        assert_eq!(view.override_timer(CAR_AT), 2.5);
        assert_eq!(view.ffb_torque(CAR_AT).value, 7.0);
        // The header's dirty word is the image's at once; the values wait for the lower.
        assert_eq!(mem.u32(CAR_AT + 0xae8), 1);
        assert_eq!(mem.u32(CAR_AT + OVERRIDE_TIMER), image(0).u32(CAR_AT + OVERRIDE_TIMER));
        held.lower(&mut mem);
        assert_eq!(mem.f32(CAR_AT + OVERRIDE_TIMER), 2.5);
        assert_eq!(mem.f32(CAR_AT + WALL_CONTACT), 0.25);
        assert_eq!(mem.f32(CAR_AT + 0xaec), 7.0);
        // Another car is the image's.
        let mut other = image(7);
        other.map(0x30000, &[0; 0x1d60]);
        let mut view = CarView::new(&mut other, Some(&held), None);
        view.set_override_timer(0x30000, 1.5);
        assert_eq!(other.f32(0x30000 + OVERRIDE_TIMER), 1.5);
    }

    #[test]
    fn lower_writes_the_owned_words_and_nothing_else() {
        let a = image(0);
        let state = CarState::lift(&a, CAR_AT);
        let mut b = image(0x5a);
        state.lower(&mut b, CAR_AT);
        let owned: Vec<_> = CarState::owned(CAR_AT).collect();
        for at in CAR_AT..CAR_AT + 0x1d60 {
            let want = if owned.iter().any(|r| r.contains(&at)) { &a } else { &image(0x5a) };
            assert_eq!(b.u8(at), want.u8(at), "{at:#x}");
        }
    }
}
