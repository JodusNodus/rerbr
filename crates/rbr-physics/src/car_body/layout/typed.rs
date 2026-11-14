//! The car's typed state on the image (`notes/physics.md` "Store seam"): the `CCar`'s own words that only the car's
//! functions read and write during a step ([`CarState`], one [`Slot`] per field), their access ([`CarFields`]: `Mem`'s
//! on the image, [`CarView`]'s on the held state), the car in typed storage ([`CarOwn`]) and the round trip. The solid's
//! words (`< +0x470`: pose, velocities, accumulators), the pointers, the parts (drive, engine, VCU scratch, lights,
//! passenger), the value cells' headers (`SetDirty`), the damage handler and every word another subsystem reads or
//! writes at its address stay in the image.

use std::cell::RefCell;
use std::ops::Range;

use crate::math::vec::Vec4;
use crate::mem::Mem;
use crate::physics_core::layout::typed::{Core, CoreFields, Slot as CoreSlot};
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
/// car's and wheel's on the image (the per-call path), and the image for the rest.
pub struct CarView<'a> {
    mem: &'a mut Mem,
    own: Option<&'a CarOwn>,
    wheels: Option<&'a CarWheels>,
    core: Option<&'a Core>,
}

impl<'a> CarView<'a> {
    /// The store over `mem` with the car `own` and its `wheels` held (none: every word on the image).
    pub fn new(mem: &'a mut Mem, own: Option<&'a CarOwn>, wheels: Option<&'a CarWheels>) -> Self {
        CarView { mem, own, wheels, core: None }
    }

    /// The store with the physics core `core` held too.
    pub fn with_core(self, core: Option<&'a Core>) -> Self {
        CarView { core, ..self }
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
}

/// `lower(lift(image))` of the car's typed state into an empty image, compared with `mem` on every owned byte: their
/// count, or the first byte that differs.
pub fn round_trip(mem: &Mem, car: u32) -> Result<usize, String> {
    let own = CarOwn::lift(mem, car);
    let mut out = Mem::new();
    own.lower(&mut out);
    let mut bytes = 0;
    for (range, name) in own.owned() {
        let len = (range.end - range.start) as usize;
        let (want, got) = (mem.read(range.start, len), out.read(range.start, len));
        if let Some(k) = want.iter().zip(&got).position(|(a, b)| a != b) {
            return Err(format!("{name} at {:#010x}: +{k:#x} differs after lower(lift)", range.start));
        }
        bytes += len;
    }
    Ok(bytes)
}

/// The callers the differential's entries call: each logs its arguments and answers a fixed value; none writes the
/// car (the wall hit's bytes are the callee's, in the step's buffer).
#[derive(Default)]
struct Callers {
    log: Vec<u32>,
}

impl Callers {
    fn note(&mut self, words: &[u32]) {
        self.log.extend_from_slice(words);
    }
}

mod callers {
    use super::Callers;
    use crate::car_body::assist::AssistCalls;
    use crate::car_body::control::ControlCalls;
    use crate::car_body::controls::ControlsCalls;
    use crate::car_body::drag::DragCalls;
    use crate::car_body::edge::EdgeCalls;
    use crate::car_body::forces::ForcesCalls;
    use crate::car_body::layout::{RespawnCalls, RespawnPoseCalls, SnapshotCalls};
    use crate::car_body::retire::RetireCheckCalls;
    use crate::car_body::sensors::SensorCalls;
    use crate::car_body::step::{StepCalls, WallHit};
    use crate::car_body::wall_friction::WallFrictionCalls;
    use crate::math::vec::{Vec3, Vec4};

    fn v3(v: Vec3) -> [u32; 3] {
        [v.x.to_bits(), v.y.to_bits(), v.z.to_bits()]
    }

    impl<S> ControlsCalls<S> for Callers {
        fn scratch_step(&mut self, _: &mut S, car: u32, dt: f32) {
            self.note(&[1, car, dt.to_bits()]);
        }
        fn drive_step(&mut self, _: &mut S, car: u32, dt: f32) {
            self.note(&[2, car, dt.to_bits()]);
        }
        fn entity_step(&mut self, _: &mut S, car: u32, dt: f32) {
            self.note(&[3, car, dt.to_bits()]);
        }
        fn update_ffb_torque(&mut self, _: &mut S, car: u32) {
            self.note(&[4, car]);
        }
        fn snapshot(&mut self, _: &mut S, car: u32) {
            self.note(&[5, car]);
        }
    }

    impl EdgeCalls for Callers {
        fn body_compose(&mut self, car: u32, a: Vec4) -> Vec4 {
            self.note(&[6, car, a.x.to_bits()]);
            Vec4 { x: 1.0, y: 2.0, z: 3.0, w: 4.0 }
        }
        fn add_edge(&mut self, grid: u32, edge: Vec4) {
            self.note(&[7, grid, edge.w.to_bits()]);
        }
        fn entity_refresh(&mut self, car: u32, a: u32) -> u32 {
            self.note(&[8, car, a]);
            9
        }
    }

    impl<S> ControlCalls<S> for Callers {
        fn read_packet(&mut self, _: &mut S, scratch: u32, payload: u32) {
            self.note(&[10, scratch, payload]);
        }
        fn engine_idle(&mut self, _: &mut S, engine: u32, payload: u32) {
            self.note(&[11, engine, payload]);
        }
        fn drive_control(&mut self, _: &mut S, drive: u32, payload: u32) {
            self.note(&[12, drive, payload]);
        }
        fn replay_is_state2(&mut self, replay: u32) -> u32 {
            self.note(&[13, replay]);
            0
        }
        fn blocked(&mut self, car: u32) -> u32 {
            self.note(&[14, car]);
            0
        }
        fn reset_car(&mut self, car: u32) {
            self.note(&[15, car]);
        }
        fn motion_probe(&mut self, car: u32) -> (u32, u32) {
            self.note(&[16, car]);
            (0, 3)
        }
        fn emit_problem(&mut self, emitter: u32, pair: [u32; 2]) {
            self.note(&[17, emitter, pair[0], pair[1]]);
        }
        fn assist(&mut self, car: u32, words: [u32; 3]) {
            self.note(&[18, car, words[0], words[1], words[2]]);
        }
    }

    impl RetireCheckCalls for Callers {
        fn timer_gap(&mut self, obj: u32, at: u32) -> f64 {
            self.note(&[19, obj, at]);
            10.0
        }
        fn flow_holds(&mut self, flow: u32) -> u32 {
            self.note(&[20, flow]);
            1
        }
        fn blocked(&mut self, car: u32) -> u32 {
            self.note(&[21, car]);
            0
        }
        fn assist(&mut self, car: u32, words: [u32; 3]) {
            self.note(&[22, car, words[0], words[1], words[2]]);
        }
        fn emit_retire(&mut self, emitter: u32, code: u32) {
            self.note(&[23, emitter, code]);
        }
        fn damage_test(&mut self, handler: u32) -> u32 {
            self.note(&[24, handler]);
            1
        }
        fn drive_test(&mut self, drive: u32) -> u32 {
            self.note(&[25, drive]);
            1
        }
        fn engine_test(&mut self, engine: u32) -> u32 {
            self.note(&[26, engine]);
            1
        }
        fn motion_probe(&mut self, car: u32) -> (u32, u32) {
            self.note(&[27, car]);
            (0, 3)
        }
        fn emit_stand_still(&mut self, emitter: u32, pair: [u32; 2]) {
            self.note(&[28, emitter, pair[0], pair[1]]);
        }
    }

    impl AssistCalls for Callers {
        fn lane_test(&mut self, car: u32) -> u32 {
            self.note(&[29, car]);
            1
        }
        fn emit_outside(&mut self, code_at: u32) {
            self.note(&[30, code_at]);
        }
        fn reset_car(&mut self, car: u32) {
            self.note(&[31, car]);
        }
        fn apply_control(&mut self, car: u32, block: u32) {
            self.note(&[32, car, block]);
        }
        fn set_dirty(&mut self, torque: u32) {
            self.note(&[33, torque]);
        }
    }

    impl<S> RespawnPoseCalls<S> for Callers {
        fn respawn_car(&mut self, _: &mut S, sp: u32, car: u32, a: u32, b: u32) {
            self.note(&[34, sp, car, a, b]);
        }
    }

    impl<S> RespawnCalls<S> for Callers {
        fn refresh_rotation(&mut self, _: &mut S, sp: u32, car: u32) {
            self.note(&[35, sp, car]);
        }
        fn compose_body_into_world(&mut self, _: &mut S, sp: u32, car: u32, a: u32, out: u32) {
            self.note(&[36, sp, car, a, out]);
        }
        fn clear_accumulators(&mut self, _: &mut S, sp: u32, car: u32) {
            self.note(&[37, sp, car]);
        }
        fn wake(&mut self, _: &mut S, sp: u32, car: u32) {
            self.note(&[38, sp, car]);
        }
        fn emit_respawn(&mut self, _: &mut S, sp: u32, emitter: u32, payload: u32) {
            self.note(&[39, sp, emitter, payload]);
        }
    }

    impl<S> SnapshotCalls<S> for Callers {
        fn compose_body_into_world(&mut self, _: &mut S, sp: u32, car: u32, a: u32, out: u32) {
            self.note(&[40, sp, car, a, out]);
        }
        fn emit_replay_state(&mut self, _: &mut S, sp: u32, emitter: u32, payload: u32) {
            self.note(&[41, sp, emitter, payload]);
        }
    }

    impl<S> StepCalls<S> for Callers {
        fn passenger_step(&mut self, _: &mut S, passenger: u32, dt: f32) {
            self.note(&[42, passenger, dt.to_bits()]);
        }
        fn scratch_idle(&mut self, _: &mut S, scratch: u32, _: f32) {
            self.note(&[43, scratch]);
        }
        fn drive_idle(&mut self, _: &mut S, drive: u32, _: f32) {
            self.note(&[44, drive]);
        }
        fn solid_step(&mut self, _: &mut S, car: u32, _: f32) {
            self.note(&[45, car]);
        }
        fn raycast(&mut self, _: &mut S, car: u32, hit: &mut WallHit) -> u32 {
            self.note(&[46, car]);
            *hit = WallHit { penetration: 0.75, perpendicular: 0.125, flags: 1, ..*hit };
            1
        }
        fn impulse(&mut self, _: &mut S, car: u32, _: &mut WallHit, perpendicular: f32, penetration: u32, dt: f32) {
            self.note(&[47, car, perpendicular.to_bits(), penetration, dt.to_bits()]);
        }
        fn emit_brake_wall(&mut self, _: &mut S, hit: &WallHit) {
            self.note(&[48, hit.flags]);
        }
        fn walk_vector(&mut self, a: Vec3, car: u32) -> Vec3 {
            self.note(&[49, car]);
            self.note(&v3(a));
            Vec3 { x: a.y, y: a.z, z: a.x }
        }
        fn hinged_active(&mut self, _: &mut S, hinged: u32) -> u32 {
            self.note(&[50, hinged]);
            1
        }
        fn hinged_inactive_step(&mut self, _: &mut S, hinged: u32) {
            self.note(&[51, hinged]);
        }
        fn hinged_detach(&mut self, _: &mut S, hinged: u32) -> u32 {
            self.note(&[52, hinged]);
            0
        }
        fn mounted_step_ready(&mut self, _: &mut S, body: u32) -> u32 {
            self.note(&[53, body]);
            1
        }
        fn mounted_step(&mut self, _: &mut S, body: u32, _: f32) {
            self.note(&[54, body]);
        }
        fn mounted_detach_ready(&mut self, _: &mut S, body: u32) -> u32 {
            self.note(&[55, body]);
            0
        }
        fn mounted_detach_step(&mut self, _: &mut S, body: u32) -> u32 {
            self.note(&[56, body]);
            0
        }
        fn mounted_exclude(&mut self, _: &mut S, body: u32, car: u32) {
            self.note(&[57, body, car]);
        }
        fn drive_line_value(&mut self, _: &mut S, car: u32, timer: f64) -> f32 {
            self.note(&[58, car, timer.to_bits() as u32]);
            1.5
        }
        fn timekeeper_record(&mut self, _: &mut S, value: f32, _: f64) {
            self.note(&[59, value.to_bits()]);
        }
        fn timekeeper_update(&mut self, _: &mut S) -> u32 {
            self.note(&[60]);
            6
        }
        fn drive_speed(&mut self, _: &mut S, drive: u32) -> f32 {
            self.note(&[61, drive]);
            3.25
        }
        fn check_retire(&mut self, _: &mut S, car: u32, v: u32) {
            self.note(&[62, car, v]);
        }
        fn emit_position(&mut self, _: &mut S, payload: [u32; 5]) {
            self.note(&[63]);
            self.note(&payload);
        }
        fn drive_engine_speed(&mut self, _: &mut S, drive: u32) -> u32 {
            self.note(&[64, drive]);
            0x4400_0000
        }
        fn emit_car_state(&mut self, _: &mut S, payload: [u32; 3]) {
            self.note(&[65]);
            self.note(&payload);
        }
    }

    impl<S> DragCalls<S> for Callers {
        fn pre_step(&mut self, _: &mut S, car: u32) {
            self.note(&[66, car]);
        }
        fn walk_vector(&mut self, a: Vec3, car: u32) -> Vec3 {
            StepCalls::<S>::walk_vector(self, a, car)
        }
        fn drag_force(&mut self, _: &mut S, car: u32, force: Vec3) {
            self.note(&[67, car]);
            self.note(&v3(force));
        }
        fn spin_damping(&mut self, _: &mut S, car: u32, damping: Vec3) {
            self.note(&[68, car]);
            self.note(&v3(damping));
        }
    }

    impl<S> WallFrictionCalls<S> for Callers {
        fn to_world(&mut self, car: u32, v: Vec3) -> Vec3 {
            self.note(&[69, car]);
            Vec3 { x: v.x, y: v.y, z: v.z + 9.0 }
        }
        fn walk_vector(&mut self, a: Vec3, car: u32) -> Vec3 {
            StepCalls::<S>::walk_vector(self, a, car)
        }
        fn wall_force(&mut self, _: &mut S, car: u32, force: Vec3) {
            self.note(&[70, car]);
            self.note(&v3(force));
        }
        fn wall_torque(&mut self, _: &mut S, car: u32, torque: Vec3) {
            self.note(&[71, car]);
            self.note(&v3(torque));
        }
    }

    impl<S> ForcesCalls<S> for Callers {
        fn passenger_idle(&mut self, passenger: u32) {
            self.note(&[72, passenger]);
        }
        fn update_walked_vector(&mut self, _: &mut S, car: u32) {
            self.note(&[73, car]);
        }
        fn walk_car_vector(&mut self, car: u32) -> Vec4 {
            self.note(&[74, car]);
            Vec4 { x: 0.5, y: -0.25, z: 2.0, w: 1.0 }
        }
        fn push_graph(&mut self, graph: u32, first: f32, second: f32) {
            self.note(&[75, graph, first.to_bits(), second.to_bits()]);
        }
        fn mounted_step_ready(&mut self, body: u32) -> u32 {
            self.note(&[76, body]);
            0
        }
        fn mounted_action(&mut self, body: u32) {
            self.note(&[77, body]);
        }
    }

    impl SensorCalls for Callers {
        fn engine_speed(&mut self, _: u32) -> f32 {
            100.0
        }
        fn drive_gear(&mut self, _: u32) -> u32 {
            2
        }
        fn drive_speed(&mut self, _: u32) -> f32 {
            3.25
        }
        fn engine_word_3(&mut self, _: u32) -> f32 {
            0.5
        }
        fn engine_word_5(&mut self, _: u32) -> f32 {
            0.25
        }
        fn engine_word_10(&mut self, _: u32) -> f32 {
            0.125
        }
        fn engine_word_14(&mut self, _: u32) -> u32 {
            1
        }
        fn timer_gap(&mut self, _: u32, _: u32) -> f64 {
            10.0
        }
        fn emit_sensor(&mut self, payload: &[u32; 16]) {
            self.note(&[78]);
            self.note(payload);
        }
    }
}

/// The car's entries that call no host, in a row on the car at `car` (`scratch`: free memory of 0x400 bytes for the
/// payloads and the entries' frames): the controls step, the control update with a call for help, the forces, the step,
/// the retire check, the cells' setters, the grid edge, the outside assistance, the respawn pose and respawn, the
/// snapshot and the sensors. Returns what the callers were given, with the getters' answers.
fn entries<S: crate::car_body::store::CarStore>(mem: &mut S, car: u32, scratch: u32) -> Vec<u32> {
    use crate::car_body::{control, controls, forces, layout, retire, roll_bars, sensors, step};
    let mut c = Callers::default();
    let dt = 1.0 / 720.0;
    let payload = scratch + 0x380;
    for (k, word) in [0x3f00_0000, 0, 0, 0, 0xbe80_0000, 0x10, 0, 0].into_iter().enumerate() {
        mem.set_u32(payload + 4 * k as u32, word);
    }
    controls::step_controls(&mut c, mem, car, dt);
    let steps = control::apply_control(&mut c, mem, car, payload, scratch + 0x3c0);
    forces::apply_forces(&mut c, mem, car, 0.5);
    step::step_car(&mut c, mem, car, dt);
    retire::check_retire(&mut c, mem, car, 6);
    layout::set_roll_bar_cell(mem, car, 1, 0, 0.75);
    layout::add_steer_torque_sum(mem, car, 0.25);
    roll_bars::update_roll_bars(mem, car);
    layout::update_ffb_torque(mem, car);
    let eax = layout::refresh_edge(&mut c, mem, car, 3);
    layout::outside_assistance(&mut c, mem, car, scratch + 0x100, 1, 0x4000_0000, 0x4080_0000);
    layout::respawn_pose(mem, &mut c, car, scratch + 0x300, scratch + 0x310, scratch + 0x200);
    layout::respawn_car(mem, &mut c, car, scratch + 0x320, scratch + 0x330, scratch + 0x280);
    layout::snapshot(mem, &mut c, car, scratch + 0x380);
    sensors::sample_sensors(&mut c, mem, car);
    c.note(&[steps, eax, mem.respawn_countdown(car).to_bits(), mem.step_count(car)]);
    c.log
}

/// The car's host-free entries in a row ([`entries`]) on the car at `car`, per call on `per_call` (every field on the
/// image) and on the car held in typed storage over the row on `held` (lifted before, lowered after), two copies of one
/// image; `scratch` is free memory of both (0x400 bytes). Compares every byte of the car and the scratch, and what the
/// callers were given: the bytes compared, or the first that differs.
pub fn differential(per_call: &mut Mem, held: &mut Mem, car: u32, scratch: u32) -> Result<usize, String> {
    const CAR_BYTES: u32 = 0x1d60;
    const SCRATCH_BYTES: u32 = 0x400;
    let own = CarOwn::lift(held, car);
    let wheels = CarWheels::lift(held, car);
    let a = entries(per_call, car, scratch);
    let b = entries(&mut CarView::new(held, Some(&own), Some(&wheels)), car, scratch);
    if a != b {
        let k = a.iter().zip(&b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
        return Err(format!("the callers differ at word {k}: per call {:x?}, held {:x?}", a.get(k), b.get(k)));
    }
    own.lower(held);
    wheels.lower(held);
    let mut bytes = 0;
    for (at, len, name) in [(car, CAR_BYTES, "car"), (scratch, SCRATCH_BYTES, "scratch")] {
        let (want, got) = (per_call.read(at, len as usize), held.read(at, len as usize));
        if let Some(i) = want.iter().zip(&got).position(|(x, y)| x != y) {
            let field = CarState::field_at(i as u32).filter(|_| name == "car").map_or("", |f| f.name);
            return Err(format!("{name} +{i:#x} {field} differs: per call {:#04x}, held {:#04x}", want[i], got[i]));
        }
        bytes += len as usize;
    }
    Ok(bytes)
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
