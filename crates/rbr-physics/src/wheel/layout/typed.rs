//! The wheel's typed state on the image: the words of the wheel's state (`CWheel::CState`, `+0x3d0`) and scratch
//! (`CWheel::CScratch`, `+0x800`) that only the wheel's functions read and write during a step ([`Wheel`], one
//! [`Slot`] per field), the car's wheels in typed storage ([`CarWheels`]), their store ([`WheelView`]) and the round
//! trip. The definition, the pointers, the damage record, the solid's words and every word another subsystem reads or
//! writes at its address stay in the image (`notes/physics.md` "Store seam").

use std::cell::RefCell;
use std::ops::Range;

use super::store::WheelStore;
use crate::car_body::layout::{CarOwn, Slot as CarSlot};
use crate::math::vec::Vec4;
use crate::mem::Mem;
use crate::rigid_body::layout::CarSolids;
use crate::store::defs::{self, Defs};
use crate::store::{Object, Raw, Value};

/// A field of [`Wheel`]: its offset in the wheel (the image's word) and its place in the typed state.
pub struct Slot<T> {
    pub at: u32,
    get: fn(&Wheel) -> T,
    set: fn(&mut Wheel, T),
}

impl<T> Clone for Slot<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Slot<T> {}

/// `object!` with a [`Slot`] per field (`slot::field`).
macro_rules! wheel_state {
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

        /// The slots of [`Wheel`], one per field, named as the field.
        #[allow(non_upper_case_globals)]
        pub mod slot {
            use super::*;
            $( pub const $field: Slot<$ty> = Slot { at: $at, get: |w| w.$field, set: |w, v| w.$field = v }; )*
        }
    };
}

wheel_state! {
    /// The wheel's typed state: the suspension pose the refresh and the axes leave (`M3B900`, `M3AB90`, `M3A9E0`,
    /// `M3AB00`), the walk, the ground probe's contact words and copies, the step's strut velocity, spin and forces,
    /// and the damage-changed flag. Offsets are the wheel's.
    pub struct Wheel, size 0x1420 {
        /// The deformed top mount (`M3AB90`).
        deformed_top_mount @ 0x570: Vec4,
        /// The dirty words of the reals of the wheel's angle (`+0x5a0`) and the strut's length (`+0x5b0`), whose
        /// values stay in the image; `SetDirty` sets them ([`CarWheels::set_dirty`]).
        wheel_angle_dirty @ 0x5A8: u32,
        strut_length_dirty @ 0x5B8: u32,
        /// The dirty word of the real at `+0x5c0` and its value, the strut's velocity.
        strut_velocity_dirty @ 0x5C8: u32,
        strut_velocity @ 0x5CC: f32,
        /// The dirty word of the real at `+0x5d0` and its value, the steering rod's length after damage.
        steering_rod_dirty @ 0x5D8: u32,
        steering_rod_length @ 0x5DC: f32,
        /// The name, dirty word and value of the definition's `WishboneLength` real, copied by the refresh.
        wishbone_length @ 0x5E4: [u32; 3],
        /// The wobbled axle (`M3A9E0`; lane 3 kept from the old value).
        wobbled_axle @ 0x5F0: Vec4,
        /// The inclined axle and its side vector (`M3AB00`).
        inclined_axle @ 0x600: Vec4,
        inclined_side @ 0x610: Vec4,
        /// The refresh's strut direction, wishbone tip, inverse moment arm and rack, and drive axle direction.
        strut_dir @ 0x9A0: Vec4,
        tip @ 0x9B0: Vec4,
        inv_arm @ 0x9C0: f32,
        inv_rack @ 0x9C4: f32,
        drive_axle_dir @ 0x9D0: Vec4,
        /// The spin rate the step stores (the drive's).
        spin_rate @ 0xA50: f32,
        /// The world axle across the probe direction (the probe's, lanes 0 to 2).
        frame_axis @ 0xA90: Vec4,
        /// The contact distance (the probe's).
        speed_scale @ 0xAA0: f32,
        /// The steering walk's output (`M3AEF0`).
        walk_out @ 0xC10: Vec4,
        /// The two sector values (`Unk96e0`).
        sector_values @ 0xD70: [f32; 2],
        /// The sweep's residue and the ground body (the probe's).
        sweep_residue @ 0xD78: f32,
        ground_body @ 0xD7C: u32,
        /// The knuckle's three axes, under the debug flag (the refresh's).
        debug_rows @ 0xD80: [Vec4; 3],
        /// The probe's copies of the centre ray's start and end and of the contact point, `Blend4`'s of the plus edge,
        /// the probe's of the plus edge's hit, `Blend4`'s of the minus edge, the probe's of the minus edge's hit and of
        /// the probe direction.
        centre_ray_start @ 0xDB0: Vec4,
        centre_ray_end @ 0xDC0: Vec4,
        contact_point_copy @ 0xDD0: Vec4,
        carcass_edge_plus_copy @ 0xDE0: Vec4,
        plus_hit_point_copy @ 0xDF0: Vec4,
        carcass_edge_minus_copy @ 0xE00: Vec4,
        minus_hit_point_copy @ 0xE10: Vec4,
        probe_direction_copy @ 0xE20: Vec4,
        /// The across-normal velocity of the last edge force (`Unk94d0`).
        edge_across @ 0xE40: Vec4,
        /// The refresh's rod mount and steering rod.
        rod_mount @ 0xE50: Vec4,
        steering_rod @ 0xE60: Vec4,
        /// The step's anti-roll bar, spring, damper and strut forces.
        roll_bar_force @ 0xE70: f32,
        spring_force @ 0xE74: f32,
        damper_force @ 0xE78: f32,
        strut_force @ 0xE7C: f32,
        /// The spin rate read again at the end of the step.
        end_spin_rate @ 0xEA0: f32,
        /// 1 when the ground probe found a contact (the car's step counts the wheels that are on).
        on_ground @ 0xEBC: u32,
        /// 1 when the damage record changed.
        damage_changed @ 0xEC0: u32,
    }
}

/// The car's wheels in typed storage, by the car's wheel slot: each wheel's address and state. The state is in a
/// `RefCell` borrowed for one field at a time, so the wheel's runs nest through the host (the step calls the refresh,
/// the probe, the edge forces) on the same state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CarWheels {
    pub wheels: [Option<(u32, RefCell<Wheel>)>; 4],
}

impl CarWheels {
    /// The wheels of the car at `car`, lifted from the image.
    pub fn lift(mem: &Mem, car: u32) -> Self {
        let pointers = crate::car_body::store::CarStore::wheel_pointers(mem, car);
        CarWheels { wheels: pointers.map(|at| (at != 0).then(|| (at, RefCell::new(Wheel::lift(mem, at))))) }
    }

    /// Writes every wheel's state back to the image.
    pub fn lower(&self, mem: &mut Mem) {
        for (at, wheel) in self.wheels.iter().flatten() {
            wheel.borrow().lower(mem, *at);
        }
    }

    /// The state of the wheel at `this`, if it is one of these.
    pub fn held(&self, this: u32) -> Option<&RefCell<Wheel>> {
        self.wheels.iter().flatten().find(|(at, _)| *at == this).map(|(_, wheel)| wheel)
    }

    /// The field `slot` of the wheel at `this`, if it is one of these.
    pub fn read<T>(&self, this: u32, slot: Slot<T>) -> Option<T> {
        self.held(this).map(|wheel| (slot.get)(&wheel.borrow()))
    }

    /// `NTweak::CBase::SetDirty` (0x52f450, `ntweak::set_dirty`) on the tweak at `at` where one of these wheels holds
    /// its dirty word: the word becomes 1. False for any other tweak, which the caller dirties on the image.
    pub fn set_dirty(&self, at: u32) -> bool {
        let word = at.wrapping_add(crate::ntweak::DIRTY);
        let dirty =
            [slot::wheel_angle_dirty, slot::strut_length_dirty, slot::strut_velocity_dirty, slot::steering_rod_dirty];
        self.wheels.iter().flatten().any(|(this, wheel)| {
            let slot = dirty.into_iter().find(|s| this.wrapping_add(s.at) == word);
            slot.inspect(|s| (s.set)(&mut wheel.borrow_mut(), 1)).is_some()
        })
    }

    /// Every owned byte range, named by its wheel slot and field.
    pub fn owned(&self) -> Vec<(Range<u32>, String)> {
        let slots = self.wheels.iter().enumerate().filter_map(|(k, w)| w.as_ref().map(|(at, _)| (k, *at)));
        slots
            .flat_map(|(k, at)| {
                Wheel::FIELDS.iter().map(move |f| (at + f.at..at + f.at + f.len, format!("wheel{k} {}", f.name)))
            })
            .collect()
    }

    /// The wheels as a store over `mem`.
    pub fn view<'a>(&'a self, mem: &'a mut Mem) -> WheelView<'a> {
        WheelView::new(mem, Some(self))
    }
}

/// The wheel's store: the held wheels' state in [`CarWheels`], every other wheel's on the image (the per-call path,
/// on which the image holds the state between calls and each nested run reads and writes it there), the step's typed
/// definitions (the wheels' and the tyres' setups), and the image for the rest.
pub struct WheelView<'a> {
    mem: &'a mut Mem,
    wheels: Option<&'a CarWheels>,
    car: Option<&'a CarOwn>,
    defs: Option<&'a Defs>,
    solids: Option<&'a CarSolids>,
}

impl<'a> WheelView<'a> {
    /// The store over `mem` with `wheels` held (none: every wheel on the image).
    pub fn new(mem: &'a mut Mem, wheels: Option<&'a CarWheels>) -> Self {
        WheelView { mem, wheels, car: None, defs: None, solids: None }
    }

    /// The store with the car's own state held too: the car's words the wheel reads (`WheelStore::car_field`).
    pub fn with_car(self, car: Option<&'a CarOwn>) -> Self {
        WheelView { car, ..self }
    }

    /// The store with the step's typed definitions (none: every definition on the image).
    pub fn with_defs(self, defs: Option<&'a Defs>) -> Self {
        WheelView { defs, ..self }
    }

    /// The store with the car's solids held (none: every solid's words on the image): the solids' words the wheel
    /// reads and writes (`WheelStore::solid_u32`).
    pub fn with_solids(self, solids: Option<&'a CarSolids>) -> Self {
        WheelView { solids, ..self }
    }

    fn held(&self, this: u32) -> Option<&'a RefCell<Wheel>> {
        self.wheels.and_then(|w| w.held(this))
    }
}

impl Raw for WheelView<'_> {
    #[inline]
    fn mem(&self) -> &Mem {
        self.mem
    }
    #[inline]
    fn mem_mut(&mut self) -> &mut Mem {
        self.mem
    }
}

impl WheelStore for WheelView<'_> {
    #[inline]
    fn get<T: Value>(&self, this: u32, slot: Slot<T>) -> T {
        match self.held(this) {
            Some(wheel) => (slot.get)(&wheel.borrow()),
            None => T::load(self.mem, this + slot.at),
        }
    }

    #[inline]
    fn set<T: Value>(&mut self, this: u32, slot: Slot<T>, value: T) {
        match self.held(this) {
            Some(wheel) => (slot.set)(&mut wheel.borrow_mut(), value),
            None => value.store(self.mem, this + slot.at),
        }
    }

    #[inline]
    fn car_field<T: Value>(&self, car: u32, slot: CarSlot<T>) -> T {
        self.car.and_then(|c| c.read(car, slot)).unwrap_or_else(|| T::load(self.mem, car + slot.at))
    }

    #[inline]
    fn solid_u32(&self, at: u32) -> u32 {
        self.solids.and_then(|s| s.read(at)).unwrap_or_else(|| self.mem.u32(at))
    }

    #[inline]
    fn solid_vec4(&self, at: u32) -> Vec4 {
        match self.solids {
            Some(_) => Vec4::from([0, 4, 8, 12].map(|k| self.solid_f32(at + k))),
            None => Vec4::load(self.mem, at),
        }
    }

    #[inline]
    fn set_solid_vec4(&mut self, at: u32, v: Vec4) {
        let Some(solids) = self.solids else {
            return v.store(self.mem, at);
        };
        for (k, lane) in [v.x, v.y, v.z, v.w].into_iter().enumerate() {
            let at = at + 4 * k as u32;
            if !solids.write(at, lane.to_bits()) {
                self.mem.set_u32(at, lane.to_bits());
            }
        }
    }

    #[inline]
    fn def_u32(&self, def: u32, off: u32) -> u32 {
        let image = || self.mem.u32(def + off);
        match self.defs.and_then(|d| d.wheel(def)).and_then(|d| d.word_of(off)) {
            Some(word) => defs::checked(word, image),
            None => image(),
        }
    }

    #[inline]
    fn hub_offset_of(&self, tyre_def: u32) -> f32 {
        let image = || self.mem.u32(tyre_def + super::TYRE_DEF_HUB_OFFSET);
        f32::from_bits(match self.defs.and_then(|d| d.tyre(tyre_def)) {
            Some(d) => defs::checked(d.hub_offset.to_bits(), image),
            None => image(),
        })
    }

    #[inline]
    fn load_carcass_width(&self, this: u32) -> f32 {
        let tyre_def = self.tyre_def(this);
        let image = || self.mem.u32(tyre_def + super::TYRE_DEF_CARCASS_WIDTH);
        f32::from_bits(match self.defs.and_then(|d| d.tyre(tyre_def)) {
            Some(d) => defs::checked(d.carcass_width().to_bits(), image),
            None => image(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::fields::*;
    use super::super::refresh_pose::{
        DEBUG_ROWS, ROD_MOUNT, STEERING_ROD, STEERING_ROD_LENGTH, STEERING_ROD_TWEAK, TIP, WISHBONE_COPY,
    };
    use super::*;
    use crate::ntweak::DIRTY;
    use crate::store::check_table;

    const WHEEL_AT: u32 = 0x10000;

    #[test]
    fn table_is_ordered_and_fits_and_names_the_layout() {
        check_table::<Wheel>();
        for (at, named) in [
            (slot::deformed_top_mount.at, DEFORMED_TOP_MOUNT),
            (slot::wheel_angle_dirty.at, WHEEL_ANGLE_TWEAK + DIRTY),
            (slot::strut_length_dirty.at, STRUT_TWEAK + DIRTY),
            (slot::strut_velocity_dirty.at, STRUT_VEL_TWEAK + DIRTY),
            (slot::strut_velocity.at, STRUT_VEL),
            (slot::steering_rod_dirty.at, STEERING_ROD_TWEAK + DIRTY),
            (slot::steering_rod_length.at, STEERING_ROD_LENGTH),
            (slot::wishbone_length.at, WISHBONE_COPY),
            (slot::wobbled_axle.at, WOBBLED_AXLE),
            (slot::inclined_axle.at, INCLINED_AXLE),
            (slot::inclined_side.at, INCLINED_SIDE),
            (slot::strut_dir.at, STRUT_DIR),
            (slot::tip.at, TIP),
            (slot::inv_arm.at, INV_ARM),
            (slot::inv_rack.at, INV_RACK),
            (slot::spin_rate.at, SPIN_RATE),
            (slot::frame_axis.at, FRAME_AXIS),
            (slot::speed_scale.at, SPEED_SCALE),
            (slot::walk_out.at, WALK_OUT),
            (slot::sector_values.at, SECTOR_VALUE_A),
            (slot::sector_values.at + 4, SECTOR_VALUE_B),
            (slot::sweep_residue.at, SWEEP_RESIDUE),
            (slot::ground_body.at, GROUND_BODY),
            (slot::debug_rows.at, DEBUG_ROWS),
            (slot::carcass_edge_plus_copy.at, CARCASS_EDGE_PLUS_COPY),
            (slot::carcass_edge_minus_copy.at, CARCASS_EDGE_MINUS_COPY),
            (slot::edge_across.at, EDGE_ACROSS),
            (slot::rod_mount.at, ROD_MOUNT),
            (slot::steering_rod.at, STEERING_ROD),
            (slot::roll_bar_force.at, ROLL_BAR_FORCE),
            (slot::spring_force.at, SPRING_FORCE),
            (slot::damper_force.at, DAMPER_FORCE),
            (slot::strut_force.at, STRUT_FORCE),
            (slot::end_spin_rate.at, END_SPIN_RATE),
            (slot::on_ground.at, ON_GROUND),
            (slot::damage_changed.at, DAMAGE_CHANGED),
        ] {
            assert_eq!(at, named);
        }
        // The pointers, the strut and the tyre scratch are not owned.
        for at in [DEF, XFORM_CAR, DAMAGE, CAR, WHEEL_INDEX, STRUT, TYRE_SCRATCH, STRUT_LENGTH, WHEEL_ANGLE] {
            assert!(Wheel::field_at(at).is_none(), "{at:#x}");
        }
    }

    /// A wheel image of `pattern`-derived bytes.
    fn image(pattern: u8) -> Mem {
        let mut mem = Mem::new();
        let bytes: Vec<u8> = (0..0x1420u32).map(|k| (k as u8).wrapping_mul(31) ^ pattern ^ (k >> 8) as u8).collect();
        mem.map(WHEEL_AT, &bytes);
        mem
    }

    #[test]
    fn the_view_reads_and_writes_the_held_state_and_the_image_otherwise() {
        let mut mem = image(0);
        let held = RefCell::new(Wheel::lift(&mem, WHEEL_AT));
        let wheels = CarWheels { wheels: [Some((WHEEL_AT, held)), None, None, None] };
        let mut view = wheels.view(&mut mem);
        let v = Vec4 { x: 1.0, y: 2.0, z: 3.0, w: 4.0 };
        view.set(WHEEL_AT, slot::strut_dir, v);
        view.set(WHEEL_AT, slot::damage_changed, 1);
        assert_eq!(view.get(WHEEL_AT, slot::strut_dir), v);
        // The image keeps its words until the lower.
        assert_eq!(mem.read(WHEEL_AT + STRUT_DIR, 16), image(0).read(WHEEL_AT + STRUT_DIR, 16));
        wheels.lower(&mut mem);
        assert_eq!(Vec4::load(&mem, WHEEL_AT + STRUT_DIR), v);
        assert_eq!(mem.u32(WHEEL_AT + DAMAGE_CHANGED), 1);
        // A wheel not held is the image's.
        let mut other = image(7);
        other.map(0x30000, &[0; 0x10]);
        let mut view = WheelView::new(&mut other, Some(&wheels));
        view.set(0x30000 - 0x9A0, slot::strut_dir, v);
        assert_eq!(Vec4::load(&other, 0x30000), v);
    }

    #[test]
    fn set_dirty_sets_a_held_wheels_dirty_word_only() {
        let mut mem = image(0);
        let wheels =
            CarWheels { wheels: [Some((WHEEL_AT, RefCell::new(Wheel::lift(&mem, WHEEL_AT)))), None, None, None] };
        for tweak in [WHEEL_ANGLE_TWEAK, STRUT_TWEAK, STRUT_VEL_TWEAK, STEERING_ROD_TWEAK] {
            assert!(wheels.set_dirty(WHEEL_AT + tweak), "{tweak:#x}");
        }
        // Another tweak, another wheel's: the image's.
        assert!(!wheels.set_dirty(WHEEL_AT + WISHBONE_COPY - 4));
        assert!(!wheels.set_dirty(0x30000 + STRUT_TWEAK));
        wheels.lower(&mut mem);
        for tweak in [WHEEL_ANGLE_TWEAK, STRUT_TWEAK, STRUT_VEL_TWEAK, STEERING_ROD_TWEAK] {
            assert_eq!(mem.u32(WHEEL_AT + tweak + DIRTY), 1, "{tweak:#x}");
        }
    }

    #[test]
    fn lower_writes_the_owned_words_and_nothing_else() {
        let a = image(0);
        let wheel = Wheel::lift(&a, WHEEL_AT);
        let mut b = image(0x5a);
        wheel.lower(&mut b, WHEEL_AT);
        let owned: Vec<_> = Wheel::owned(WHEEL_AT).collect();
        for at in WHEEL_AT..WHEEL_AT + 0x1420 {
            let want = if owned.iter().any(|r| r.contains(&at)) { &a } else { &image(0x5a) };
            assert_eq!(b.u8(at), want.u8(at), "{at:#x}");
        }
    }
}
