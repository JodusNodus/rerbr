//! The drive definition's curve rows and tables and the curve workers' state (`Worker4060`, `Hook4160`).

use super::drive_objects::IntLane;
use super::drive_typed::{DriveObj, DriveStateObj, WheelObj};
use super::worker::{DriveWorker, GearWork};
use crate::engine_driveline::store::DriveStore;
use crate::mem::Mem;

/// The E39 curve words of the def (`n`): the rows at `+0x4ec`, `0xb0` apart, and the three ten-knot tables at `+0x59c`,
/// `+0x5c4` and `+0x5ec`. The last knot of the third table is the value at `+0x610`. Names are ours.
const CURVE_ROW_AT: u32 = 0x4EC;
const CURVE_ROW_STRIDE: u32 = 0xB0;
pub(super) const CURVE_TABLE_A_AT: u32 = 0x59C;
pub(super) const CURVE_TABLE_B_AT: u32 = 0x5C4;
pub(super) const CURVE_TABLE_C_AT: u32 = 0x5EC;

/// The ratio ring of the drive state at `state` (`E39_RatioCA0`'s argument).
pub fn ratio_ring_at(state: u32) -> u32 {
    state + DriveStateObj::ring.at
}

/// The def's brake block of wheel `k` (`+0x12c`, 240 bytes a block, the tweak keys `BrakeRatio_*` at `+0xe0` in it):
/// `Hook4160`'s second argument.
pub fn brake_block_at(def: u32, k: u32) -> u32 {
    def + 0x12c + 0xf0 * k
}

/// The curve input of the drive `this` (`+0x10`): `Hook4160`'s fourth argument.
pub fn curve_input_at(this: u32) -> u32 {
    this + CURVE_INPUT_AT
}

const CURVE_INPUT_AT: u32 = 0x10;

/// The address of row `index` of the def at `def`.
pub fn curve_row_at(def: u32, index: u32) -> u32 {
    def + CURVE_ROW_AT + CURVE_ROW_STRIDE * index
}

/// A curve row: the breaks (`+0xc`, `+0x1c`, `+0x2c`, `+0x3c`, 0x10 apart), the values (`+0x6c` to `+0x9c`, 0x10 apart),
/// the gain (`+0x4c`), the rate (`+0x5c`) and the counter's limit (`+0xac`). The row's breaks are `breaks[0]` (`+0xc`),
/// `breaks[2]` (`+0x2c`) and `breaks[1]` (`+0x1c`, the upper one), and `breaks[3]` (`+0x3c`) is the lower break `WorkerF60`
/// uses. The names are ours.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CurveRow {
    pub breaks: [u32; 4],
    pub values: [f32; 4],
    pub gain: f32,
    pub rate: f32,
    pub limit: u32,
}

impl CurveRow {
    /// The row at `at` (see [`curve_row_at`]).
    pub fn load(mem: &Mem, at: u32) -> Self {
        CurveRow {
            breaks: std::array::from_fn(|k| mem.u32(at + 0xC + 0x10 * k as u32)),
            values: std::array::from_fn(|k| mem.f32(at + 0x6C + 0x10 * k as u32)),
            gain: mem.f32(at + 0x4C),
            rate: mem.f32(at + 0x5C),
            limit: mem.u32(at + 0xAC),
        }
    }
}

/// The three ten-knot tables of the def: `a` (`+0x59c`), `b` (`+0x5c4`) and `c` (`+0x5ec`). `c[9]` is the value at `+0x610`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CurveTables {
    pub a: [f32; 10],
    pub b: [f32; 10],
    pub c: [f32; 10],
}

impl CurveTables {
    pub fn load(mem: &Mem, def: u32) -> Self {
        CurveTables {
            a: mem.f32s(def + CURVE_TABLE_A_AT),
            b: mem.f32s(def + CURVE_TABLE_B_AT),
            c: mem.f32s(def + CURVE_TABLE_C_AT),
        }
    }
}

/// The drive state's words the curve workers read and update (`b`, also Hook4160's `a1`, a wheel's [`WheelObj`]): the
/// position (an `IntLane` at `+0x70`), the slope (`+0x1c`), the term's gain (`+0xac`), the lane (an `IntLane` at `+0x80`)
/// and the counter (an `IntLane` at `+0xb8`, its value at `+0xc4`). Names are ours.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CurveState {
    pub pos: IntLane,
    pub slope: f32,
    pub term_gain: f32,
    pub lane: IntLane,
    pub counter: IntLane,
}

impl CurveState {
    /// The curve state at `at` on the image: for a pointer no held drive's wheel has.
    pub fn load(mem: &Mem, at: u32) -> Self {
        CurveState {
            pos: WheelObj::pos.load(mem, at),
            slope: mem.f32(at + WheelObj::prev_force.at + 4),
            term_gain: WheelObj::term_gain.load(mem, at),
            lane: WheelObj::lane.load(mem, at),
            counter: WheelObj::counter.load(mem, at),
        }
    }

    /// Writes the position, the lane and the counter at `at` on the image.
    pub fn store(&self, mem: &mut Mem, at: u32) {
        WheelObj::pos.store(mem, at, self.pos);
        WheelObj::lane.store(mem, at, self.lane);
        WheelObj::counter.store(mem, at, self.counter);
    }
}

/// The input block of Hook4160 (`x`, the drive's `+0x10`): the values of the drive's brake torque lane (`+0x60`, the
/// drive's `+0x70`) and of its handbrake torque lane (`+0x70`, the drive's `+0x80`, which `Set44C0` scales by
/// `MaxHandbrakePressure`). NGP's brake reads them as the pedal and handbrake pressures.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CurveInput {
    pub brake_torque: f32,
    pub handbrake_torque: f32,
}

impl CurveInput {
    /// The block at `at` on the image (a drive's `+0x10`): for a pointer that is not a held drive's.
    pub fn load(mem: &Mem, at: u32) -> Self {
        let drive = at.wrapping_sub(CURVE_INPUT_AT);
        CurveInput {
            brake_torque: DriveObj::brake_torque.load(mem, drive).value,
            handbrake_torque: DriveObj::lane_scaled.load(mem, drive).value,
        }
    }
}

/// A wheel's brake block of the def (`c`, the block `Hook4160` reads): the curve
/// row index (`+0xdc`) and the words NGP's brake reads as its per-wheel keys (`Brake*_NGP`, see
/// [`crate::ngp::brake::Brake`]): the effective radius (`+0x6c`, the block gain of 1.02's curve workers), the ratio
/// (`+0xec`), the piston area (`+0xcc`), the plate area (`+0x9c`), the internal convection (`+0xbc`), the heat
/// absorption (`+0x8c`), the specific heat (`+0x7c`) and the mass (`+0x3c`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BrakeBlock {
    pub index: u32,
    pub effective_radius: f32,
    pub ratio: f32,
    pub piston_area: f32,
    pub plate_area: f32,
    pub internal_convection: f32,
    pub heat_absorption: f32,
    pub specific_heat: f32,
    pub mass: f32,
}

impl BrakeBlock {
    pub fn load(mem: &Mem, at: u32) -> Self {
        BrakeBlock {
            index: mem.u32(at + 0xDC),
            effective_radius: mem.f32(at + 0x6C),
            ratio: mem.f32(at + 0xEC),
            piston_area: mem.f32(at + 0xCC),
            plate_area: mem.f32(at + 0x9C),
            internal_convection: mem.f32(at + 0xBC),
            heat_absorption: mem.f32(at + 0x8C),
            specific_heat: mem.f32(at + 0x7C),
            mass: mem.f32(at + 0x3C),
        }
    }

    /// The block's gain (`+0x6c`, the effective radius) that the curve workers scale by.
    pub fn gain(mem: &Mem, at: u32) -> f32 {
        mem.f32(at + 0x6C)
    }

    /// The block's row index (`+0xdc`) that `drive_curve::row_value` picks the row by.
    pub fn row_index(mem: &Mem, at: u32) -> u32 {
        mem.u32(at + 0xDC)
    }
}

/// Runs `port` on a fresh load of the drive's worker and stores it back.
pub fn with_worker<S: DriveStore, T>(store: &mut S, port: impl FnOnce(&mut DriveWorker) -> T) -> T {
    let mut w = store.worker();
    let out = port(&mut w);
    store.store_worker(&w);
    out
}

/// `with_worker` with the gear's words too (the gear workers load them; stored after the worker).
pub fn with_gear<S: DriveStore, T>(store: &mut S, port: impl FnOnce(&mut DriveWorker, &mut GearWork) -> T) -> T {
    let mut w = store.worker();
    let mut g = store.gear_work();
    let out = port(&mut w, &mut g);
    store.store_worker(&w);
    store.store_gear_work(&g);
    out
}
