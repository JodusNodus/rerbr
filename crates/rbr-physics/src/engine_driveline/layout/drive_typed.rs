//! The drive's typed state on the image (`notes/physics.md` "Store seam"): its tables ([`DriveObj`], the `CDrive`'s own
//! words; [`DriveStateObj`], its state's; [`WheelObj`], each wheel's block of the state), where they live
//! ([`DriveImage`]) with their lift and lower, the car's drive in typed storage ([`CarDrive`]), its store
//! ([`DriveView`]; [`with_drive`] per call) and the round trip. The definition, the damage handler's block (its
//! functors write it too) and the pointers stay in the image, read per call; so do the state's car and the wheel
//! worker's matrix words (the step matrix and vector, the solution), which it writes there for the original's
//! `E39_MatVec`. The records the ports take ([`DriveWorker`], [`CurveState`], ...) are views of these words: a word two
//! records name (term 3 and wheel 0's slip, the throttle and the gate level) is one field here.

use std::cell::RefCell;
use std::ops::Range;

use super::curve::{CurveInput, CurveState};
use super::drive_def::{brake_scale_at, gear_count_at, gear_ratios_at, lock_max_torque_at, DriveDefinition};
use super::drive_objects::{Drive, DriveDef, DriveObjects, DriveState, IntLane, Lane, DRIVE_DEF_PTR, DRIVE_STATE_PTR};
use super::worker::{
    DriveDamage, DriveOwn, DriveWorker, GearBand, GearWork, RatioRing, WheelBlock, WheelTerm, WorkerDef, WorkerState,
    SOLUTION_AT, STEP_MATRIX_AT, STEP_VEC_AT,
};
use crate::engine_driveline::store::{DriveStore, Part};
use crate::mem::Mem;
use crate::store::defs::{self, Defs};
use crate::store::{Object, Raw, Real, Tabled};
use crate::vcu::layout::{GearRatios, Gearbox, Locks, Outputs, Reals};
use crate::vcu::store::DriveBlocks;

crate::object! {
    /// `CDrive`'s own words a step changes (the car's `+0x4b0`; names by use in [`Drive`] and [`DriveOwn`]). The
    /// throttle's value is the gate level `M6C90` reads, the brake torque's and `lane_scaled`'s are `Hook4160`'s input
    /// block (`+0x10`). `gear_dt` is the time step per gear.
    pub struct DriveObj, size 0x1cc {
        throttle @ 0x28: Lane,
        brake @ 0x38: Lane,
        lane_scale @ 0x48: Lane,
        lane_gate @ 0x50: u32,
        clutch_slip @ 0x5c: Lane,
        brake_torque @ 0x6c: Lane,
        lane_scaled @ 0x7c: Lane,
        value_rate @ 0x8c: Lane,
        pack_out @ 0xc0: [f32; 4],
        pair_pack @ 0xd0: [f32; 2],
        pair_solution @ 0xd8: [f32; 2],
        pair_slip @ 0xe0: [f32; 2],
        wheel_offset @ 0xe8: [Real; 4],
        wheel_bias @ 0x128: [Real; 4],
        gear_dt @ 0x174: [f32; 10],
        ring_ratio @ 0x19c: f32,
        scaled_term @ 0x1a0: f32,
        speed_copy @ 0x1a4: u32,
        shift_speed @ 0x1b4: f32,
        shift_trim @ 0x1b8: u32,
        clutch_magnitude @ 0x1bc: f32,
        gear_level @ 0x1c4: f32,
        speed_factor @ 0x1c8: f32,
    }
}

crate::object! {
    /// `CDrive::CState`'s words a step changes, outside the wheels' blocks (names in [`WorkerState`]). `terms` are the
    /// three lock channels (`CDrive::Set4570`: the ratio is a term's value, the lock torque its scale).
    pub struct DriveStateObj, size 0x5c0 {
        terms @ 0x0c: [WheelTerm; 3],
        sign_word @ 0x368: i32,
        drive_gain @ 0x37c: f32,
        drive_gain_neg @ 0x380: f32,
        drive_speed_key @ 0x384: f32,
        ring @ 0x388: RatioRing,
        engine_speed @ 0x45c: f32,
        pack_gain @ 0x460: f32,
        clutch_matrix @ 0x470: [f32; 16],
        lane_active @ 0x4f0: [u32; 4],
        pair_matrix @ 0x500: [f32; 4],
        pair_inverse @ 0x510: [f32; 4],
        pair_active @ 0x520: [u32; 2],
        pair_words @ 0x528: [u32; 2],
        shift_from @ 0x544: [u32; 2],
        gear @ 0x54c: u32,
        gear_flag @ 0x558: IntLane,
        trim @ 0x560: f32,
        commanded_aux @ 0x568: u32,
        commanded @ 0x56c: IntLane,
        trim_accum @ 0x574: f32,
        trigger @ 0x578: u32,
        rng @ 0x57c: u32,
        step_count @ 0x58c: Lane,
        reset_count @ 0x59c: Lane,
        latch_count @ 0x5a4: u32,
        clutch_change_count @ 0x5a8: u32,
        repeat_count @ 0x5ac: u32,
        dirty_count @ 0x5b0: u32,
        step_iter @ 0x5b4: u32,
        branch @ 0x5b8: u32,
        pending @ 0x5bc: u32,
    }
}

crate::object! {
    /// A wheel's block of the drive state (200 bytes from the state's `+0x48`; `Hook4160`'s first argument). Wheel 0's
    /// first five words are term 3 too (value, term, sign, scale, speed: the slip's two header words, the slip lane and
    /// the previous force's first word). The previous force's value is the curve slope (NGP's wheel speed); the curve's
    /// position and lane values are NGP's layer and core temperatures, `term_gain` its handbrake gain.
    pub struct WheelObj, size 0xc8 {
        slip_label @ 0x00: u32,
        slip_name @ 0x04: u32,
        slip @ 0x08: Lane,
        prev_label @ 0x10: u32,
        prev_force_aux @ 0x14: u32,
        prev_force @ 0x18: Lane,
        prev_gate_aux @ 0x24: u32,
        prev_gate @ 0x28: IntLane,
        inertia @ 0x3c: f32,
        force_aux @ 0x44: u32,
        force @ 0x48: Lane,
        gate_aux @ 0x54: u32,
        gate @ 0x58: IntLane,
        clutch_load @ 0x6c: f32,
        pos @ 0x78: IntLane,
        lane @ 0x88: IntLane,
        term_gain @ 0xac: f32,
        latch_bound @ 0xb0: f32,
        counter @ 0xc0: IntLane,
    }
}

/// The wheels' blocks: from the state's `+0x48`, 200 bytes apart.
const WHEELS_AT: u32 = 0x48;
const WHEEL_STRIDE: u32 = 200;
const WHEELS: u32 = 4;
/// The state's car (`+0x580`, a pointer: on the image).
const STATE_CAR: u32 = 0x580;
/// The time step per gear (`+0x174`) and the gear's scale in the damage handler's block (`+0x48`).
const GEAR_DT_AT: u32 = 0x174;
const DAMAGE_GEAR_SCALE_AT: u32 = 0x48;
/// The damage handler's diff-lock enable (`+0x90`).
const DAMAGE_LOCKS_ENABLED: u32 = 0x90;

/// The gear request of the drive at `drive` (its state's `+0x564`, an `NTweak` header whose dirty word is
/// `DriveStateObj::commanded`'s), which `CDrive::M6840` passes to `SetDirty`.
pub fn gear_request_at(mem: &Mem, drive: u32) -> u32 {
    DriveImage::of(mem, drive).state + DriveStateObj::commanded.at - crate::ntweak::DIRTY
}

/// The address of wheel `k`'s block of the state at `state`.
pub fn wheel_at(state: u32, k: u32) -> u32 {
    state + WHEELS_AT + WHEEL_STRIDE * k
}

impl WheelObj {
    /// The curve workers' view of the block.
    pub fn curve(&self) -> CurveState {
        CurveState {
            pos: self.pos,
            slope: self.prev_force.value,
            term_gain: self.term_gain,
            lane: self.lane,
            counter: self.counter,
        }
    }

    /// Writes what the curve workers change: the position, the lane and the counter.
    pub fn set_curve(&mut self, value: &CurveState) {
        self.pos = value.pos;
        self.lane = value.lane;
        self.counter = value.counter;
    }

    fn block(&self) -> WheelBlock {
        WheelBlock {
            slip: self.slip,
            prev_force: self.prev_force,
            prev_force_aux: self.prev_force_aux,
            prev_gate: self.prev_gate,
            prev_gate_aux: self.prev_gate_aux,
            force: self.force,
            force_aux: self.force_aux,
            gate: self.gate,
            gate_aux: self.gate_aux,
            inertia: self.inertia,
            clutch_load: self.clutch_load,
            latch_bound: self.latch_bound,
        }
    }

    /// The block's words but the slip (the state's store decides that one).
    fn set_block(&mut self, b: &WheelBlock) {
        self.prev_force = b.prev_force;
        self.prev_force_aux = b.prev_force_aux;
        self.prev_gate = b.prev_gate;
        self.prev_gate_aux = b.prev_gate_aux;
        self.force = b.force;
        self.force_aux = b.force_aux;
        self.gate = b.gate;
        self.gate_aux = b.gate_aux;
        self.inertia = b.inertia;
        self.clutch_load = b.clutch_load;
        self.latch_bound = b.latch_bound;
    }
}

/// The drive's typed state: its own words, its state's and its wheels' blocks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DriveTyped {
    pub own: DriveObj,
    pub state: DriveStateObj,
    pub wheels: [WheelObj; WHEELS as usize],
}

/// `DriveTyped`'s records: the views the ports take, and their stores, each writing the words the original's does.
impl DriveTyped {
    pub fn lanes(&self) -> Drive {
        let o = &self.own;
        Drive { throttle_in: o.throttle, brake_in: o.brake, brake_torque: o.brake_torque, clutch_slip: o.clutch_slip }
    }

    pub fn set_lanes(&mut self, v: &Drive) {
        self.own.throttle = v.throttle_in;
        self.own.brake = v.brake_in;
        self.own.brake_torque = v.brake_torque;
        self.own.clutch_slip = v.clutch_slip;
    }

    pub fn drive_state(&self) -> DriveState {
        let s = &self.state;
        DriveState {
            drive_speed_key: s.drive_speed_key,
            engine_speed: s.engine_speed,
            gear: s.gear as i32,
            commanded_gear: s.commanded,
        }
    }

    pub fn set_drive_state(&mut self, v: &DriveState) {
        self.state.drive_speed_key = v.drive_speed_key;
        self.state.engine_speed = v.engine_speed;
        self.state.gear = v.gear as u32;
        self.state.commanded = v.commanded_gear;
    }

    /// Term 3: wheel 0's first five words.
    fn term3(&self) -> WheelTerm {
        let w = &self.wheels[0];
        WheelTerm {
            value: f32::from_bits(w.slip_label),
            term: f32::from_bits(w.slip_name),
            sign: w.slip.dirty as i32,
            scale: w.slip.value,
            speed: f32::from_bits(w.prev_label),
        }
    }

    /// The worker's state; the matrix words and the car from the image at `state`.
    fn worker_state(&self, mem: &Mem, state: u32) -> WorkerState {
        let s = &self.state;
        WorkerState {
            drive: self.drive_state(),
            sign_word: s.sign_word,
            drive_gain: s.drive_gain,
            drive_gain_neg: s.drive_gain_neg,
            pack_gain: s.pack_gain,
            clutch_matrix: s.clutch_matrix,
            step_matrix: mem.f32s(state + STEP_MATRIX_AT),
            step_vec: mem.f32s(state + STEP_VEC_AT),
            lane_active: s.lane_active,
            pair_matrix: s.pair_matrix,
            pair_inverse: s.pair_inverse,
            pair_active: s.pair_active,
            pair_words: s.pair_words,
            shift_from: s.shift_from,
            commanded_aux: s.commanded_aux,
            gear_flag: s.gear_flag,
            trim: s.trim,
            trim_accum: s.trim_accum,
            trigger: s.trigger,
            rng: s.rng,
            car: mem.u32(state + STATE_CAR),
            step_count: s.step_count,
            reset_count: s.reset_count,
            latch_count: s.latch_count,
            clutch_change_count: s.clutch_change_count,
            repeat_count: s.repeat_count,
            dirty_count: s.dirty_count,
            step_iter: s.step_iter,
            branch: s.branch,
            pending: s.pending,
            terms: [s.terms[0], s.terms[1], s.terms[2], self.term3()],
            blocks: self.wheels.map(|w| w.block()),
        }
    }

    /// Writes the worker's state back, as the original's store does: term 3's sign and scale are wheel 0's slip lane, so
    /// each of the two is written only when the port changed it, and each wheel's slip likewise (a stale copy of the
    /// other must not overwrite the change); the matrix words and the car on the image at `state`.
    fn set_worker_state(&mut self, v: &WorkerState, mem: &mut Mem, state: u32) {
        let (old_term3, old_slips) = (self.term3(), self.wheels.map(|w| w.slip));
        self.set_drive_state(&v.drive);
        let s = &mut self.state;
        s.sign_word = v.sign_word;
        s.drive_gain = v.drive_gain;
        s.drive_gain_neg = v.drive_gain_neg;
        s.pack_gain = v.pack_gain;
        s.clutch_matrix = v.clutch_matrix;
        for (k, x) in v.step_matrix.iter().enumerate() {
            mem.set_f32(state + STEP_MATRIX_AT + 4 * k as u32, *x);
        }
        for (k, x) in v.step_vec.iter().enumerate() {
            mem.set_f32(state + STEP_VEC_AT + 4 * k as u32, *x);
        }
        s.lane_active = v.lane_active;
        s.pair_matrix = v.pair_matrix;
        s.pair_inverse = v.pair_inverse;
        s.pair_active = v.pair_active;
        s.pair_words = v.pair_words;
        s.shift_from = v.shift_from;
        s.commanded_aux = v.commanded_aux;
        s.gear_flag = v.gear_flag;
        s.trim = v.trim;
        s.trim_accum = v.trim_accum;
        s.trigger = v.trigger;
        s.rng = v.rng;
        mem.set_u32(state + STATE_CAR, v.car);
        s.step_count = v.step_count;
        s.reset_count = v.reset_count;
        s.latch_count = v.latch_count;
        s.clutch_change_count = v.clutch_change_count;
        s.repeat_count = v.repeat_count;
        s.dirty_count = v.dirty_count;
        s.step_iter = v.step_iter;
        s.branch = v.branch;
        s.pending = v.pending;
        s.terms = [v.terms[0], v.terms[1], v.terms[2]];
        let (term3, w0) = (v.terms[3], &mut self.wheels[0]);
        w0.slip_label = term3.value.to_bits();
        w0.slip_name = term3.term.to_bits();
        w0.prev_label = term3.speed.to_bits();
        if term3.sign != old_term3.sign {
            w0.slip.dirty = term3.sign as u32;
        }
        if term3.scale.to_bits() != old_term3.scale.to_bits() {
            w0.slip.value = term3.scale;
        }
        for (k, (wheel, block)) in self.wheels.iter_mut().zip(&v.blocks).enumerate() {
            wheel.set_block(block);
            if block.slip != old_slips[k] {
                wheel.slip = block.slip;
            }
        }
    }

    /// The drive's own words of the worker; the solution from the image at `drive`.
    fn own(&self, mem: &Mem, drive: u32) -> DriveOwn {
        let o = &self.own;
        DriveOwn {
            value_rate: o.value_rate,
            solution: mem.f32s(drive + SOLUTION_AT),
            lane_scale: o.lane_scale,
            lane_scaled: o.lane_scaled,
            lane_gate: o.lane_gate,
            gate_level: o.throttle.value,
            scaled_term: o.scaled_term,
            speed_factor: o.speed_factor,
            pack_out: o.pack_out,
            pair_pack: o.pair_pack,
            pair_solution: o.pair_solution,
            pair_slip: o.pair_slip,
            wheel_offset: o.wheel_offset.map(|r| r.value),
            shift_speed: o.shift_speed,
            shift_trim: o.shift_trim,
            gear_level: o.gear_level,
            ring_ratio: o.ring_ratio,
            wheel_bias: o.wheel_bias.map(|r| r.value),
            speed_copy: o.speed_copy,
            clutch_magnitude: o.clutch_magnitude,
        }
    }

    fn set_own(&mut self, v: &DriveOwn, mem: &mut Mem, drive: u32) {
        let o = &mut self.own;
        o.value_rate = v.value_rate;
        for (k, x) in v.solution.iter().enumerate() {
            mem.set_f32(drive + SOLUTION_AT + 4 * k as u32, *x);
        }
        o.lane_scale = v.lane_scale;
        o.lane_scaled = v.lane_scaled;
        o.lane_gate = v.lane_gate;
        o.throttle.value = v.gate_level;
        o.scaled_term = v.scaled_term;
        o.speed_factor = v.speed_factor;
        o.pack_out = v.pack_out;
        o.pair_pack = v.pair_pack;
        o.pair_solution = v.pair_solution;
        o.pair_slip = v.pair_slip;
        for (real, x) in o.wheel_offset.iter_mut().zip(v.wheel_offset) {
            real.value = x;
        }
        o.shift_speed = v.shift_speed;
        o.shift_trim = v.shift_trim;
        o.gear_level = v.gear_level;
        o.ring_ratio = v.ring_ratio;
        for (real, x) in o.wheel_bias.iter_mut().zip(v.wheel_bias) {
            real.value = x;
        }
        o.speed_copy = v.speed_copy;
        o.clutch_magnitude = v.clutch_magnitude;
    }

    /// The word at `offset` of the drive (`Part::Drive`) or its state, where the typed state holds all four bytes;
    /// `None` where it holds none of them. A word it holds in part is read byte by byte ([`word_bytes`]).
    fn held_word(&self, part: Part, offset: u32) -> Option<u32> {
        let index = |at: u32, n: u32| offset.checked_sub(at).filter(|d| d % 4 == 0 && d / 4 < n).map(|d| d / 4);
        match part {
            Part::Drive => {
                if let Some(k) = index(GEAR_DT_AT, 10) {
                    return Some(self.own.gear_dt[k as usize].to_bits());
                }
                for (at, reals) in [
                    (DriveObj::wheel_offset.at, &self.own.wheel_offset),
                    (DriveObj::wheel_bias.at, &self.own.wheel_bias),
                ] {
                    if let Some(k) = index(at, 16) {
                        let r = reals[k as usize / 4];
                        return Some([r.label, r.name, r.dirty, r.value.to_bits()][k as usize % 4]);
                    }
                }
            }
            Part::State => {
                if let Some(k) = index(DriveStateObj::terms.at, 15) {
                    let t = self.state.terms[k as usize / 5];
                    return Some(
                        [t.value.to_bits(), t.term.to_bits(), t.sign as u32, t.scale.to_bits(), t.speed.to_bits()]
                            [k as usize % 5],
                    );
                }
                if let Some(rel) = offset.checked_sub(WHEELS_AT).filter(|d| *d < WHEEL_STRIDE * WHEELS) {
                    let w = &self.wheels[(rel / WHEEL_STRIDE) as usize];
                    match rel % WHEEL_STRIDE {
                        0x08 => return Some(w.slip.dirty),
                        0x0c => return Some(w.slip.value.to_bits()),
                        0x18 => return Some(w.prev_force.dirty),
                        0x1c => return Some(w.prev_force.value.to_bits()),
                        _ => {}
                    }
                }
            }
        }
        None
    }

    /// Writes `value` at `offset` where the typed state holds all four bytes (see [`DriveTyped::held_word`]); false
    /// where it does not.
    fn set_held_word(&mut self, part: Part, offset: u32, value: u32) -> bool {
        let index = |at: u32, n: u32| offset.checked_sub(at).filter(|d| d % 4 == 0 && d / 4 < n).map(|d| d / 4);
        match part {
            Part::Drive => {
                if let Some(k) = index(GEAR_DT_AT, 10) {
                    self.own.gear_dt[k as usize] = f32::from_bits(value);
                    return true;
                }
                for at in [DriveObj::wheel_offset.at, DriveObj::wheel_bias.at] {
                    if let Some(k) = index(at, 16) {
                        let reals = if at == DriveObj::wheel_offset.at {
                            &mut self.own.wheel_offset
                        } else {
                            &mut self.own.wheel_bias
                        };
                        let r = &mut reals[k as usize / 4];
                        match k % 4 {
                            0 => r.label = value,
                            1 => r.name = value,
                            2 => r.dirty = value,
                            _ => r.value = f32::from_bits(value),
                        }
                        return true;
                    }
                }
            }
            Part::State => {
                if let Some(k) = index(DriveStateObj::terms.at, 15) {
                    let t = &mut self.state.terms[k as usize / 5];
                    match k % 5 {
                        0 => t.value = f32::from_bits(value),
                        1 => t.term = f32::from_bits(value),
                        2 => t.sign = value as i32,
                        3 => t.scale = f32::from_bits(value),
                        _ => t.speed = f32::from_bits(value),
                    }
                    return true;
                }
                if let Some(rel) = offset.checked_sub(WHEELS_AT).filter(|d| *d < WHEEL_STRIDE * WHEELS) {
                    let w = &mut self.wheels[(rel / WHEEL_STRIDE) as usize];
                    match rel % WHEEL_STRIDE {
                        0x08 => w.slip.dirty = value,
                        0x0c => w.slip.value = f32::from_bits(value),
                        0x18 => w.prev_force.dirty = value,
                        0x1c => w.prev_force.value = f32::from_bits(value),
                        _ => return false,
                    }
                    return true;
                }
            }
        }
        false
    }
}

/// The objects of a part's word at `offset` as a byte image: each byte the typed state owns from its tables lowered at
/// their image addresses, the rest from `mem`. The slow path of [`DriveStore::word`] (an index no record names).
fn word_bytes(d: &DriveTyped, image: &DriveImage, mem: &Mem, part: Part, offset: u32) -> [u8; 4] {
    let base = image.base(part);
    let at = base.wrapping_add(offset);
    if !(0..4).any(|i| image.owns(at.wrapping_add(i))) {
        return mem.u32(at).to_le_bytes();
    }
    let mut held = Mem::new();
    image.lower(d, &mut held);
    std::array::from_fn(|i| {
        let at = base.wrapping_add(offset).wrapping_add(i as u32);
        if image.owns(at) {
            held.u8(at)
        } else {
            mem.u8(at)
        }
    })
}

/// Writes `bytes` at a part's `offset`: the typed state's bytes into it (lowered, written, lifted back), the rest into
/// `mem`.
fn set_word_bytes(d: &mut DriveTyped, image: &DriveImage, mem: &mut Mem, part: Part, offset: u32, bytes: [u8; 4]) {
    let base = image.base(part);
    let at = base.wrapping_add(offset);
    if !(0..4).any(|i| image.owns(at.wrapping_add(i))) {
        mem.set_u32(at, u32::from_le_bytes(bytes));
        return;
    }
    let mut held = Mem::new();
    image.lower(d, &mut held);
    for (i, byte) in bytes.into_iter().enumerate() {
        let at = base.wrapping_add(offset).wrapping_add(i as u32);
        if image.owns(at) {
            held.set_u8(at, byte);
        } else {
            mem.set_u8(at, byte);
        }
    }
    *d = image.lift(&held);
}

/// Where a drive's typed state lives in the image: the drive and the state it points at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriveImage {
    pub drive: u32,
    pub state: u32,
}

impl DriveImage {
    /// The drive at `drive`, its state as its pointer gives it.
    pub fn of(mem: &Mem, drive: u32) -> Self {
        DriveImage { drive, state: mem.u32(drive + DRIVE_STATE_PTR) }
    }

    fn base(&self, part: Part) -> u32 {
        match part {
            Part::Drive => self.drive,
            Part::State => self.state,
        }
    }

    /// The drive's typed state, read from the image.
    pub fn lift(&self, mem: &Mem) -> DriveTyped {
        DriveTyped {
            own: DriveObj::lift(mem, self.drive),
            state: DriveStateObj::lift(mem, self.state),
            wheels: std::array::from_fn(|k| WheelObj::lift(mem, wheel_at(self.state, k as u32))),
        }
    }

    /// Writes the drive's typed state back: every owned word, nothing else.
    pub fn lower(&self, d: &DriveTyped, mem: &mut Mem) {
        d.own.lower(mem, self.drive);
        d.state.lower(mem, self.state);
        for (k, wheel) in d.wheels.iter().enumerate() {
            wheel.lower(mem, wheel_at(self.state, k as u32));
        }
    }

    /// The owned byte ranges with their names.
    pub fn owned(&self) -> impl Iterator<Item = (Range<u32>, String)> + '_ {
        let own = DriveObj::owned(self.drive).map(|r| (r, "drive".to_string()));
        let state = DriveStateObj::owned(self.state).map(|r| (r, "drive state".to_string()));
        let wheels = (0..WHEELS)
            .flat_map(move |k| WheelObj::owned(wheel_at(self.state, k)).map(move |r| (r, format!("drive wheel{k}"))));
        own.chain(state).chain(wheels)
    }

    /// The drive's tables at their addresses, for the twin's compare.
    pub fn tabled(&self) -> Vec<Tabled> {
        let wheels = (0..WHEELS).map(|k| Tabled::of::<WheelObj>(format!("drive wheel{k}"), wheel_at(self.state, k)));
        [Tabled::of::<DriveObj>("drive", self.drive), Tabled::of::<DriveStateObj>("drive state", self.state)]
            .into_iter()
            .chain(wheels)
            .collect()
    }

    fn owns(&self, at: u32) -> bool {
        let field = |base: u32, f: fn(u32) -> bool| at.checked_sub(base).is_some_and(f);
        field(self.drive, |o| DriveObj::field_at(o).is_some())
            || field(self.state, |o| DriveStateObj::field_at(o).is_some())
            || (0..WHEELS).any(|k| {
                at.checked_sub(wheel_at(self.state, k))
                    .is_some_and(|o| o < WHEEL_STRIDE && WheelObj::field_at(o).is_some())
            })
    }
}

/// A drive in typed storage: where it lives in the image and its state. The state is in a cell borrowed by each store
/// access alone: the drive's step calls other drive entries through the host, which run on the same state.
#[derive(Clone, Debug, PartialEq)]
pub struct CarDrive {
    pub image: DriveImage,
    pub typed: RefCell<DriveTyped>,
}

impl CarDrive {
    /// The drive of the car at `car`, lifted from the image.
    pub fn lift(mem: &Mem, car: u32) -> Self {
        Self::of(mem, crate::car_body::layout::drive_at(car))
    }

    /// The drive at `drive`, lifted from the image.
    pub fn of(mem: &Mem, drive: u32) -> Self {
        let image = DriveImage::of(mem, drive);
        CarDrive { image, typed: RefCell::new(image.lift(mem)) }
    }

    /// Writes the drive's state back to the image.
    pub fn lower(&self, mem: &mut Mem) {
        self.image.lower(&self.typed.borrow(), mem);
    }

    /// Whether this is the drive at `drive`.
    pub fn is(&self, drive: u32) -> bool {
        self.image.drive == drive
    }

    /// The wheel whose block of this drive's state is at `at`, if one is.
    pub fn wheel_of(&self, at: u32) -> Option<u32> {
        (0..WHEELS).find(|&k| wheel_at(self.image.state, k) == at)
    }

    /// The drive as a store: its state here, its definition, damage block and pointers in `mem`.
    pub fn view<'a>(&'a self, mem: &'a mut Mem) -> DriveView<'a> {
        DriveView { mem, at: DriveRef { image: self.image, held: Some(&self.typed), defs: None } }
    }

    /// Every owned byte range, named.
    pub fn owned(&self) -> Vec<(Range<u32>, String)> {
        self.image.owned().collect()
    }
}

/// Runs `run` on the drive at `drive` per call: each store access lifts the drive from the image and lowers what it
/// changes, so the image holds the drive between accesses (the drive's step calls other entries through the host).
pub fn with_drive<T>(mem: &mut Mem, drive: u32, run: impl FnOnce(&mut DriveView) -> T) -> T {
    let at = DriveRef { image: DriveImage::of(mem, drive), held: None, defs: None };
    run(&mut DriveView { mem, at })
}

/// `lower(lift(image))` of the car's drive into an empty image, compared with `mem` on every owned byte: their count, or
/// the first byte that differs.
pub fn round_trip(mem: &Mem, car: u32) -> Result<usize, String> {
    let drive = CarDrive::lift(mem, car);
    let mut out = Mem::new();
    drive.lower(&mut out);
    let mut bytes = 0;
    for (range, name) in drive.owned() {
        let len = (range.end - range.start) as usize;
        let (want, got) = (mem.read(range.start, len), out.read(range.start, len));
        if let Some(k) = want.iter().zip(&got).position(|(a, b)| a != b) {
            return Err(format!("{name} at {:#010x}: +{k:#x} differs after lower(lift)", range.start));
        }
        bytes += len;
    }
    Ok(bytes)
}

/// A drive's typed state for its accesses: held (typed storage), or the image's (lifted per access).
#[derive(Clone, Copy, Debug)]
pub struct DriveRef<'a> {
    image: DriveImage,
    held: Option<&'a RefCell<DriveTyped>>,
    defs: Option<&'a Defs>,
}

impl<'a> DriveRef<'a> {
    /// The drive at `drive`: `held`'s state when it is that drive, else the image's.
    pub fn of(mem: &Mem, held: Option<&'a CarDrive>, drive: u32) -> Self {
        match held.filter(|d| d.is(drive)) {
            Some(d) => DriveRef { image: d.image, held: Some(&d.typed), defs: None },
            None => DriveRef { image: DriveImage::of(mem, drive), held: None, defs: None },
        }
    }

    /// The drive with the step's typed definitions (`None`: its definition on the image).
    pub fn with_defs(self, defs: Option<&'a Defs>) -> Self {
        DriveRef { defs, ..self }
    }

    /// The typed definition at `def`, where the step holds it.
    fn held_def(&self, def: u32) -> Option<&'a DriveDefinition> {
        self.defs?.drive(def)
    }

    fn read<T>(&self, mem: &Mem, f: impl FnOnce(&DriveTyped) -> T) -> T {
        match self.held {
            Some(typed) => f(&typed.borrow()),
            None => f(&self.image.lift(mem)),
        }
    }

    fn write<T>(&self, mem: &mut Mem, f: impl FnOnce(&mut DriveTyped, &mut Mem) -> T) -> T {
        match self.held {
            Some(typed) => f(&mut typed.borrow_mut(), mem),
            None => {
                let mut d = self.image.lift(mem);
                let out = f(&mut d, mem);
                self.image.lower(&d, mem);
                out
            }
        }
    }

    fn objects(&self, mem: &Mem) -> DriveObjects {
        DriveObjects::at(mem, self.image.drive)
    }

    pub fn gearbox(&self, mem: &Mem) -> Gearbox {
        self.read(mem, |d| Gearbox {
            gear: d.state.gear,
            commanded: d.state.commanded.value as u32,
            changed: d.state.commanded.dirty,
            slip: d.state.gear_flag.value as u32,
        })
    }

    pub fn store_gearbox(&self, mem: &mut Mem, v: &Gearbox) {
        self.write(mem, |d, _| {
            d.state.gear = v.gear;
            d.state.commanded.value = v.commanded as i32;
            d.state.commanded.dirty = v.changed;
            d.state.gear_flag.value = v.slip as i32;
        })
    }

    /// The drive's definition (its pointer alone: the VCU's gear setter reads no other).
    fn def(&self, mem: &Mem) -> u32 {
        mem.u32(self.image.drive + DRIVE_DEF_PTR)
    }

    pub fn gear_count(&self, mem: &Mem) -> u32 {
        let def = self.def(mem);
        match self.held_def(def) {
            Some(d) => defs::checked(d.accessors.gear_count, || gear_count_at(mem, def)),
            None => gear_count_at(mem, def),
        }
    }

    pub fn gear_ratios(&self, mem: &Mem) -> GearRatios {
        let def = self.def(mem);
        match self.held_def(def) {
            Some(d) => defs::checked(d.ratios, || gear_ratios_at(mem, def)),
            None => gear_ratios_at(mem, def),
        }
    }

    pub fn outputs(&self, mem: &Mem) -> Outputs {
        let objects = self.objects(mem);
        let (max_torque, brake_scale) = match self.held_def(objects.def) {
            Some(d) => defs::checked((d.lock_max_torque, d.accessors.max_footbrake_pressure), || {
                (lock_max_torque_at(mem, objects.def), brake_scale_at(mem, objects.def))
            }),
            None => (lock_max_torque_at(mem, objects.def), brake_scale_at(mem, objects.def)),
        };
        self.read(mem, |d| {
            let terms = &d.state.terms;
            let locks = Locks {
                ratio: terms.map(|t| t.value),
                enabled: mem.u32(objects.damage + DAMAGE_LOCKS_ENABLED) != 0,
                max_torque,
                torque: terms.map(|t| t.scale),
            };
            let o = &d.own;
            let reals = Reals {
                throttle: o.throttle.value,
                brake: o.brake.value,
                brake_scaled: o.brake_torque.value,
                clutch_slip: o.clutch_slip.value,
                brake_scale,
            };
            Outputs { locks, reals }
        })
    }

    pub fn store_outputs(&self, mem: &mut Mem, v: &Outputs) {
        self.write(mem, |d, _| {
            for (term, torque) in d.state.terms.iter_mut().zip(v.locks.torque) {
                term.scale = torque;
            }
            let r = &v.reals;
            let o = &mut d.own;
            for (lane, x) in [
                (&mut o.throttle, r.throttle),
                (&mut o.brake, r.brake),
                (&mut o.brake_torque, r.brake_scaled),
                (&mut o.clutch_slip, r.clutch_slip),
            ] {
                *lane = Lane { dirty: 1, value: x };
            }
        })
    }
}

/// A drive as a [`DriveStore`] (and the VCU's [`DriveBlocks`]): its typed state ([`CarDrive::view`], or per access on
/// the image, [`with_drive`]); the definition, the damage block, the pointers and the words named in the module doc
/// read and written on the image.
pub struct DriveView<'a> {
    mem: &'a mut Mem,
    at: DriveRef<'a>,
}

impl<'a> DriveView<'a> {
    /// The view with the step's typed definitions (`None`: the definition on the image).
    pub fn with_defs(self, defs: Option<&'a Defs>) -> Self {
        DriveView { at: self.at.with_defs(defs), ..self }
    }
}

impl DriveView<'_> {
    /// The gear's scale (the damage block's `+0x48` at `shift`) as an offset from the drive.
    fn gear_scale_offset(&self, damage: u32, shift: u32) -> u32 {
        damage.wrapping_add(DAMAGE_GEAR_SCALE_AT).wrapping_add(shift).wrapping_sub(self.at.image.drive)
    }
}

impl Raw for DriveView<'_> {
    fn mem(&self) -> &Mem {
        self.mem
    }

    fn mem_mut(&mut self) -> &mut Mem {
        self.mem
    }
}

impl DriveStore for DriveView<'_> {
    fn at(&self) -> u32 {
        self.at.image.drive
    }

    fn objects(&self) -> DriveObjects {
        self.at.objects(self.mem)
    }

    fn def(&self) -> DriveDef {
        let at = self.objects().def;
        match self.at.held_def(at) {
            Some(d) => defs::checked(d.accessors, || DriveDef::load(self.mem, at)),
            None => DriveDef::load(self.mem, at),
        }
    }

    fn lanes(&self) -> Drive {
        self.at.read(self.mem, DriveTyped::lanes)
    }

    fn store_lanes(&mut self, value: &Drive) {
        self.at.write(self.mem, |d, _| d.set_lanes(value))
    }

    fn state(&self) -> DriveState {
        self.at.read(self.mem, DriveTyped::drive_state)
    }

    fn store_state(&mut self, value: &DriveState) {
        self.at.write(self.mem, |d, _| d.set_drive_state(value))
    }

    fn store_commanded_gear(&mut self, value: &DriveState) {
        self.at.write(self.mem, |d, _| d.state.commanded = value.commanded_gear)
    }

    fn worker(&self) -> DriveWorker {
        let (image, objects) = (self.at.image, self.objects());
        let mem = &*self.mem;
        let def = match self.at.held_def(objects.def) {
            Some(held) => defs::checked(held.worker, || WorkerDef::load(mem, objects.def)),
            None => WorkerDef::load(mem, objects.def),
        };
        self.at.read(mem, |d| DriveWorker {
            def,
            state: d.worker_state(mem, image.state),
            own: d.own(mem, image.drive),
            damage: DriveDamage::load(mem, objects.damage),
        })
    }

    fn store_worker(&mut self, value: &DriveWorker) {
        let (image, objects) = (self.at.image, self.objects());
        self.at.write(self.mem, |d, mem| {
            d.set_worker_state(&value.state, mem, image.state);
            d.set_own(&value.own, mem, image.drive);
            value.damage.store(mem, objects.damage);
        })
    }

    /// The gear's words: its band, its scale in the damage block (`+0x48`, a word per gear) and its time step (the
    /// drive's `+0x174`, a word per gear), each at the state's gear whatever its value (a word the typed state holds
    /// is its).
    fn gear_work(&self) -> GearWork {
        let objects = self.objects();
        let gear = self.at.read(self.mem, |d| d.state.gear);
        let shift = gear.wrapping_mul(4);
        GearWork {
            gear,
            band: match self.at.held_def(objects.def).and_then(|d| d.band(gear)) {
                Some(band) => defs::checked(band, || GearBand::load(self.mem, objects.def, gear)),
                None => GearBand::load(self.mem, objects.def, gear),
            },
            scale: f32::from_bits(self.word(Part::Drive, self.gear_scale_offset(objects.damage, shift))),
            dt: f32::from_bits(self.word(Part::Drive, GEAR_DT_AT.wrapping_add(shift))),
        }
    }

    /// Writes the gear's scale and time step (at the gear `gear_work` read), the scale first.
    fn store_gear_work(&mut self, value: &GearWork) {
        let shift = value.gear.wrapping_mul(4);
        let scale_at = self.gear_scale_offset(self.objects().damage, shift);
        self.set_word(Part::Drive, scale_at, value.scale.to_bits());
        self.set_word(Part::Drive, GEAR_DT_AT.wrapping_add(shift), value.dt.to_bits());
    }

    fn ratio_ring(&self) -> RatioRing {
        self.at.read(self.mem, |d| d.state.ring)
    }

    fn store_ratio_ring(&mut self, value: &RatioRing) {
        self.at.write(self.mem, |d, _| d.state.ring = *value)
    }

    fn curve_state(&self, wheel: u32) -> CurveState {
        self.at.read(self.mem, |d| d.wheels[wheel as usize].curve())
    }

    fn store_curve_state(&mut self, wheel: u32, value: &CurveState) {
        self.at.write(self.mem, |d, _| d.wheels[wheel as usize].set_curve(value))
    }

    fn curve_input(&self) -> CurveInput {
        self.at.read(self.mem, |d| CurveInput {
            brake_torque: d.own.brake_torque.value,
            handbrake_torque: d.own.lane_scaled.value,
        })
    }

    fn latch_bound(&self, wheel: u32) -> f32 {
        self.at.read(self.mem, |d| d.wheels[wheel as usize].latch_bound)
    }

    fn store_latch_bound(&mut self, wheel: u32, value: f32) {
        self.at.write(self.mem, |d, _| d.wheels[wheel as usize].latch_bound = value)
    }

    fn word(&self, part: Part, offset: u32) -> u32 {
        let image = self.at.image;
        self.at.read(self.mem, |d| {
            d.held_word(part, offset)
                .unwrap_or_else(|| u32::from_le_bytes(word_bytes(d, &image, self.mem, part, offset)))
        })
    }

    fn set_word(&mut self, part: Part, offset: u32, value: u32) {
        let image = self.at.image;
        self.at.write(self.mem, |d, mem| {
            if !d.set_held_word(part, offset, value) {
                set_word_bytes(d, &image, mem, part, offset, value.to_le_bytes());
            }
        })
    }
}

/// The VCU's blocks of this drive (`vcu::store`); the drive address is the view's.
impl DriveBlocks for DriveView<'_> {
    fn gearbox(&self, _drive: u32) -> Gearbox {
        self.at.gearbox(self.mem)
    }

    fn store_gearbox(&mut self, _drive: u32, value: &Gearbox) {
        self.at.store_gearbox(self.mem, value)
    }

    fn gear_count(&self, _drive: u32) -> u32 {
        self.at.gear_count(self.mem)
    }

    fn gear_ratios(&self, _drive: u32) -> GearRatios {
        self.at.gear_ratios(self.mem)
    }

    fn outputs(&self, _drive: u32) -> Outputs {
        self.at.outputs(self.mem)
    }

    fn store_outputs(&mut self, _drive: u32, value: &Outputs) {
        self.at.store_outputs(self.mem, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::check_table;

    const DRIVE: u32 = 0x1000;
    const STATE: u32 = 0x4000;

    #[test]
    fn tables_are_ordered_and_fit() {
        check_table::<DriveObj>();
        check_table::<DriveStateObj>();
        check_table::<WheelObj>();
        // The wheels' blocks are inside the state, after the terms and before the state's next field.
        assert_eq!(DriveStateObj::terms.at + 3 * 0x14, WHEELS_AT);
        const { assert!(WHEELS_AT + WHEELS * WHEEL_STRIDE <= DriveStateObj::sign_word.at) };
    }

    /// The drive and its state mapped with `pattern`-derived bytes, the state pointer set.
    fn image(pattern: u8) -> Mem {
        let mut mem = Mem::new();
        let bytes = |n: u32, salt: u8| (0..n).map(|k| (k as u8).wrapping_mul(29) ^ salt ^ pattern).collect::<Vec<_>>();
        mem.map(DRIVE, &bytes(0x200, 1));
        mem.map(STATE, &bytes(0x5c0, 2));
        mem.set_u32(DRIVE + DRIVE_STATE_PTR, STATE);
        mem
    }

    #[test]
    fn lower_writes_the_lifted_owned_words_and_nothing_else() {
        let a = image(0);
        let image_a = DriveImage::of(&a, DRIVE);
        let d = image_a.lift(&a);
        let mut b = image(0x5a);
        b.set_u32(DRIVE + DRIVE_STATE_PTR, STATE);
        image_a.lower(&d, &mut b);
        let owned: Vec<_> = image_a.owned().map(|(r, _)| r).collect();
        for (base, len) in [(DRIVE, 0x200), (STATE, 0x5c0)] {
            for at in base..base + len {
                let want = if owned.iter().any(|r| r.contains(&at)) { &a } else { &b };
                assert_eq!(b.u8(at), want.u8(at), "{at:#x}");
            }
        }
        // The pointers and the image's words are not owned.
        for at in [
            DRIVE + DRIVE_STATE_PTR,
            DRIVE + SOLUTION_AT,
            STATE + STEP_MATRIX_AT,
            STATE + STEP_VEC_AT,
            STATE + STATE_CAR,
        ] {
            assert!(!owned.iter().any(|r| r.contains(&at)), "{at:#x}");
        }
        assert_eq!(DriveImage::of(&b, DRIVE).lift(&b), d);
    }

    #[test]
    fn a_word_reads_and_writes_as_its_bytes_do() {
        let mut mem = image(0);
        let drive = CarDrive::of(&mem, DRIVE);
        let mut view = drive.view(&mut mem);
        // Every word of both objects: the named fast paths and the byte path agree with the image.
        let words: Vec<(Part, u32)> = (0..0x200)
            .step_by(4)
            .map(|o| (Part::Drive, o))
            .chain((0..0x5c0).step_by(4).map(|o| (Part::State, o)))
            .collect();
        for &(part, offset) in &words {
            let at = if part == Part::Drive { DRIVE } else { STATE } + offset;
            assert_eq!(view.word(part, offset), image(0).u32(at), "{part:?} {offset:#x}");
            view.set_word(part, offset, offset ^ 0xdead_beef);
        }
        drive.lower(&mut mem);
        for &(part, offset) in &words {
            let at = if part == Part::Drive { DRIVE } else { STATE } + offset;
            assert_eq!(mem.u32(at), offset ^ 0xdead_beef, "{part:?} {offset:#x}");
        }
    }

    #[test]
    fn term_three_is_wheel_zeros_slip() {
        let mut mem = image(0);
        let mut d = DriveImage::of(&mem, DRIVE).lift(&mem);
        let mut w = d.worker_state(&mem, STATE);
        w.terms[3].scale = 2.5;
        d.set_worker_state(&w, &mut mem, STATE);
        assert_eq!(d.worker_state(&mem, STATE).blocks[0].slip.value, 2.5);
        let mut w = d.worker_state(&mem, STATE);
        w.set_slip(0, 4.0);
        w.terms[3].scale = 9.0;
        d.set_worker_state(&w, &mut mem, STATE);
        // Both changed: the slip's store comes last, as in the original's.
        assert_eq!(d.worker_state(&mem, STATE).terms[3].scale, 4.0);
    }
}
