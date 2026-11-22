//! `--ffb-tyres MM` (ours, a trial; `notes/ngp.md` "Force feedback"): the wheel's force rebuilt from the front tyres,
//! as AC EVO's is (its `finalFF` fits `a Mz + b Fy` of the front tyres, `b / a` 36-45 mm, linear, no lag), in place
//! of NGP's steering torque sum and its filter. Each front tyre on the ground gives its aligning moment (the
//! pneumatic trail times the lateral force) plus the lateral force times a fixed mechanical trail of `MM`, about the
//! contact's normal, then the wheel's steering gain (`inv_arm * inv_rack`) and the car's rack ratio put the sum in the
//! units NGP's curve takes, so the sensitivity, the linearity (100 %: a line, as AC's) and the steering load read it
//! as NGP's torque. Reads the tyres' scratch after the step, never writes the memory.
//!
//! `--ffb-pull W` adds [`Pull`]: the braking and drive pull NGP's torque has through the scrub radius, which the
//! front tyres' sideways force leaves out.

use std::cell::Cell;

use rbr_physics::car_body::store::CarStore;
use rbr_physics::mem::Mem;
use rbr_physics::wheel::layout::{SteerTorque, INV_ARM, INV_RACK, ON_GROUND, TYRE_SCRATCH};
use rbr_step::runner::CAR_PTR;

/// The mechanical trail, m, that gives AC EVO's torque by front slip angle on the r40 circle (`notes/ngp.md`):
/// shorter than its fit's 36-45 mm, as our tyres' pneumatic trail is shorter. `--trace-wheels` takes it.
pub const SUGGESTED_TRAIL: f32 = 0.025;

/// The contact's forward speed, m/s, the trail has its full length from: below it the torque fades in line with the
/// speed, so a car at rest gives none. With the bare sign the tyres' resting lateral force (to 630 N a wheel) changed
/// side with each turn of the contact speed's sign (to 0.05 m/s either way), 0.3-0.8 of a drive's 90th percentile
/// torque; faded, 0.01-0.03 (the user's eight drives, 2026-11-15).
const ROLLING_SPEED: f32 = 1.0;

/// The wheel pointers in the car (`car+0x470`): the front pair first.
const WHEELS: u32 = 0x470;

/// The steering torque from the front tyres with the mechanical trail `trail` (m), in the units of the car's
/// filtered torque (`CarStore::ffb_torque`). The trail torque has the tyre step's sign, `sgn(v_long) * trail * F_lat`
/// about the contact's normal, which holds for the mirrored right-hand contact frame; its world vertical part sums.
/// Below [`ROLLING_SPEED`] the sign is `v_long` over it.
/// The gain and the rack ratio count by size: a pack's Alpine A110 RGT has both negative, the Fiesta Rally2 both
/// positive, and NGP's torque has the same sign for the same turn in either.
pub fn torque(mem: &Mem, trail: f32) -> f32 {
    let car = mem.u32(CAR_PTR);
    let sum: f32 = (0..2)
        .map(|k| mem.u32(car + WHEELS + 4 * k))
        .filter(|&wheel| mem.u32(wheel + ON_GROUND) != 0)
        .map(|wheel| {
            let s = rbr_physics::tyre::layout::frame_of(mem, wheel + TYRE_SCRATCH).scratch;
            let rolling = (s.contact_velocity[1] / ROLLING_SPEED).clamp(-1.0, 1.0);
            let moment = rolling * (s.pneumatic_trail + trail) * s.lateral_force;
            moment * s.contact_frame.normal.z * (mem.f32(wheel + INV_ARM) * mem.f32(wheel + INV_RACK)).abs()
        })
        .sum();
    -mem.steering_rack_ratio(car).abs() * sum
}

/// The force the wheel gets, in [-1, 1] before the player's settings: [`torque`], with `pull`'s added, through NGP's
/// curve at the car's sensitivity, 0 while the car's force feedback is muted or its override timer runs (as NGP's
/// filter holds it).
pub fn force(mem: &Mem, trail: f32, pull: Option<&Pull>) -> f32 {
    let car = mem.u32(CAR_PTR);
    let torque = if mem.override_timer(car) > 0.0 { 0.0 } else { torque(mem, trail) + pull.map_or(0.0, Pull::torque) };
    rbr_physics::ngp::ffb::curve(torque, mem.ffb_muted(car), mem.max_ff_torque(car))
}

/// The pull's time constant, s: a 7 ms step's one pole. Over the recorded drives NGP's longitudinal part carries the
/// most of its torque's 8-30 Hz buzz (`notes/ngp.md`); 25 ms keeps 0.9 of it at 3 Hz and 0.4 at 15 Hz.
const PULL_SMOOTHING: f32 = 0.025;

/// The pull's soft limit, times the car's sensitivity `S`: about 1.1 times the p90 of NGP's torque on the recorded
/// drives (0.36 `S`), above the braking pull's p95 (to 0.9 of it), under the impacts' (2-12 times it).
const PULL_LIMIT: f32 = 0.4;

/// The braking and drive pull (`--ffb-pull W`, ours, a trial): each physics step's longitudinal part of the front
/// wheels' adds to NGP's torque sum (`steer_parts::parts`, the scrub radius's: their difference in braking or drive
/// force), times the rack ratio as NGP's torque takes the sum, soft-limited at [`PULL_LIMIT`] and smoothed over
/// [`PULL_SMOOTHING`], times the weight. Fed the runner's kept adds ([`Pull::fold`]); a wheel off the ground adds none.
pub struct Pull {
    weight: f32,
    value: Cell<f32>,
}

impl Pull {
    pub fn new(weight: f32) -> Self {
        Pull { weight, value: Cell::new(0.0) }
    }

    /// Folds the adds `torques` kept since the last call, step by step, on the car in `mem`.
    pub fn fold(&self, mem: &Mem, torques: &[SteerTorque]) {
        let car = mem.u32(CAR_PTR);
        let limit = PULL_LIMIT * mem.max_ff_torque(car);
        let rack = mem.steering_rack_ratio(car);
        let blend = crate::physics::DT / (PULL_SMOOTHING + crate::physics::DT);
        for step in crate::steer_parts::steps(torques) {
            let sum: f64 = step.iter().filter_map(crate::steer_parts::parts).map(|p| p.longitudinal).sum();
            let target = soft_limit(rack * sum as f32, limit);
            self.value.set(self.value.get() + (target - self.value.get()) * blend);
        }
    }

    /// The pull now, in the units of [`torque`].
    pub fn torque(&self) -> f32 {
        self.weight * self.value.get()
    }
}

/// `x` through `limit * tanh(x / limit)`: itself while small, at most `limit` in size.
fn soft_limit(x: f32, limit: f32) -> f32 {
    if limit > 0.0 {
        limit * (x / limit).tanh()
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::soft_limit;

    #[test]
    fn the_soft_limit_keeps_small_pulls_and_caps_large_ones() {
        assert!((soft_limit(5.0, 100.0) - 5.0).abs() < 0.01);
        assert!((soft_limit(-1e6, 100.0) + 100.0).abs() < 1e-3);
        assert_eq!(soft_limit(5.0, 0.0), 0.0);
    }
}
