//! The per-step update of the steering cells and the force-feedback torque. The steering slew moves toward the
//! steering input by `(1 / dt) * (input - slew)`, its step capped at 20 per second. The steering target is the new slew
//! times the setup's two gains (`MaxSteeringLock`, `SteeringRackRatio`), and its rate is the change per second. The
//! torque demand is then cleared, and the scratch sim, drive and entity steps, the anti-roll bars, the force-feedback
//! torque and (with the snapshot flag set) the replay snapshot run. The profiler channel is the binding's scope, not this
//! port's. The order is the original's.

use super::accessors::set_cell;
use super::roll_bars;
use crate::car_body::store::{CarStore, Cell};
use crate::mem::Mem;

/// The calls the controls step makes out of its own body. Each cell write is its own dirtying (inside `set_cell`).
pub trait ControlsCalls<S = Mem> {
    /// The scratch sim's step.
    fn scratch_step(&mut self, mem: &mut S, car: u32, dt: f32);
    /// The drive's step.
    fn drive_step(&mut self, mem: &mut S, car: u32, dt: f32);
    /// The entity's step on the car.
    fn entity_step(&mut self, mem: &mut S, car: u32, dt: f32);
    /// The force-feedback torque update. Only its kernel is ported (`steering_ffb::filtered_torque`).
    fn update_ffb_torque(&mut self, mem: &mut S, car: u32);
    /// The replay snapshot, run only while the snapshot flag is set.
    fn snapshot(&mut self, mem: &mut S, car: u32);
}

/// The steering slew's rate limit, per second.
const SLEW_LIMIT: f32 = 20.0;

/// Runs the controls step of `car` for the step `dt`, s.
pub fn step_controls<S: CarStore>(calls: &mut impl ControlsCalls<S>, mem: &mut S, car: u32, dt: f32) {
    let inv_dt = 1.0 / dt;
    let slew = mem.cell(car, Cell::SteerSlew);
    let mut step = mem.steer_input(car) - slew.value;
    let mut rate = inv_dt * step;
    // `|rate|`, with the original's compare: NaN takes the negated branch.
    rate = if rate > 0.0 { rate } else { 0.0 - rate };
    if rate > SLEW_LIMIT {
        step = (step / rate) * SLEW_LIMIT;
    }
    let new_slew = slew.value + step;
    mem.store_cell(car, Cell::SteerSlew, set_cell(slew, new_slew));

    let old_target = mem.steer_target(car);
    let target = (new_slew * mem.max_steering_lock(car)) * mem.steering_rack_ratio(car);
    let cell = set_cell(mem.cell(car, Cell::SteerTarget), target);
    mem.store_cell(car, Cell::SteerTarget, cell);

    let target_rate = (target - old_target) * inv_dt;
    let cell = set_cell(mem.cell(car, Cell::SteerTargetRate), target_rate);
    mem.store_cell(car, Cell::SteerTargetRate, cell);

    let cell = set_cell(mem.cell(car, Cell::SteerTorqueSum), 0.0);
    mem.store_cell(car, Cell::SteerTorqueSum, cell);

    calls.scratch_step(mem, car, dt);
    calls.drive_step(mem, car, dt);
    calls.entity_step(mem, car, dt);
    roll_bars::update_roll_bars(mem, car);
    calls.update_ffb_torque(mem, car);
    if mem.snapshot_on(car) {
        calls.snapshot(mem, car);
    }
}
