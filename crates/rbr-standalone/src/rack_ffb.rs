//! `--ffb-rack` (ours, a trial; `notes/ngp.md` "Force feedback"): NGP's steering torque without its filter, for the
//! wheel to map as a line. NGP's force is its rack sum (every steered wheel's lateral, longitudinal and vertical
//! force about the steering axis plus the tyre's aligning moment) times the rack ratio, blended with the last value
//! by 0.5-0.7 a step, then through its curve. This takes the last step's sum, unblended; the viewer maps it as a line
//! (linearity 100). Reads the car's store after the step, never writes it.

use rbr_physics::car_body::store::CarStore;

/// The force the wheel gets, in [-1, 1] before the player's settings: the rack ratio times the steering torque sum
/// through NGP's curve at the car's sensitivity, 0 while the car's force feedback is muted or its override timer
/// runs (as NGP's filter holds it).
pub fn force<S: CarStore>(mem: &S, car: u32) -> f32 {
    let torque = if mem.override_timer(car) > 0.0 {
        0.0
    } else {
        mem.steering_rack_ratio(car) * mem.steer_torque_sum(car).value
    };
    rbr_physics::ngp::ffb::curve(torque, mem.ffb_muted(car), mem.max_ff_torque(car))
}
