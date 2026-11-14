//! The drive definition's curve rows and tables and the curve workers' state (`Worker4060`, `Hook4160`).

use super::drive_objects::IntLane;
use super::worker::{DriveWorker, GearWork};
use crate::engine_driveline::store::EngineStore;

/// The E39 curve words of the def (`n`): the rows at `+0x4ec`, `0xb0` apart, and the three ten-knot tables at `+0x59c`,
/// `+0x5c4` and `+0x5ec`. The last knot of the third table is the value at `+0x610`. Names are ours.
const CURVE_ROW_AT: u32 = 0x4EC;
const CURVE_ROW_STRIDE: u32 = 0xB0;
pub(super) const CURVE_TABLE_A_AT: u32 = 0x59C;
pub(super) const CURVE_TABLE_B_AT: u32 = 0x5C4;
pub(super) const CURVE_TABLE_C_AT: u32 = 0x5EC;

/// The ratio ring of the drive state at `state` (`+0x388`, `E39_RatioCA0`).
pub fn ratio_ring_at(state: u32) -> u32 {
    state + 0x388
}

/// The curve state of block `k` of the drive state at `state` (`+0x48`, 200 bytes a block): `Hook4160`'s first
/// argument, and the block's slip words.
pub fn curve_state_at(state: u32, k: u32) -> u32 {
    state + 0x48 + 200 * k
}

/// The def's brake block of wheel `k` (`+0x12c`, 240 bytes a block, the tweak keys `BrakeRatio_*` at `+0xe0` in it):
/// `Hook4160`'s second argument.
pub fn brake_block_at(def: u32, k: u32) -> u32 {
    def + 0x12c + 0xf0 * k
}

/// The curve input of the drive `this` (`+0x10`): `Hook4160`'s fourth argument.
pub fn curve_input_at(this: u32) -> u32 {
    this + 0x10
}

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
    pub fn load<S: EngineStore>(mem: &S, at: u32) -> Self {
        mem.load_curve_row(at)
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
    pub fn load<S: EngineStore>(mem: &S, def: u32) -> Self {
        mem.load_curve_tables(def)
    }
}

/// The drive state's words the curve workers read and update (`b`, also Hook4160's `a1`): the position (an `IntLane` at
/// `+0x70`), the slope (`+0x1c`), the term's gain (`+0xac`), the lane (an `IntLane` at `+0x80`) and the counter (an
/// `IntLane` at `+0xb8`, its value at `+0xc4`). Names are ours.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CurveState {
    pub pos: IntLane,
    pub slope: f32,
    pub term_gain: f32,
    pub lane: IntLane,
    pub counter: IntLane,
}

impl CurveState {
    pub fn load<S: EngineStore>(mem: &S, at: u32) -> Self {
        mem.load_curve_state(at)
    }

    pub fn store<S: EngineStore>(&self, mem: &mut S, at: u32) {
        mem.store_curve_state(at, self)
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
    pub fn load<S: EngineStore>(mem: &S, at: u32) -> Self {
        mem.load_curve_input(at)
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
    pub fn load<S: EngineStore>(mem: &S, at: u32) -> Self {
        mem.load_brake_block(at)
    }
}

/// Runs `port` on a fresh load of the worker at `this` and stores it back.
pub fn with_worker<S: EngineStore, T>(mem: &mut S, this: u32, port: impl FnOnce(&mut DriveWorker) -> T) -> T {
    let mut w = DriveWorker::load(mem, this);
    let out = port(&mut w);
    w.store(mem, this);
    out
}

/// `with_worker` with the gear's words too (the gear workers load them).
pub fn with_gear<S: EngineStore, T>(
    mem: &mut S,
    this: u32,
    port: impl FnOnce(&mut DriveWorker, &mut GearWork) -> T,
) -> T {
    let mut w = DriveWorker::load(mem, this);
    let mut g = GearWork::load(mem, this);
    let out = port(&mut w, &mut g);
    w.store(mem, this);
    g.store(mem, this);
    out
}
