//! `--ffb-surface W` (ours, a trial; `notes/ngp.md` "Force feedback"): the road's texture added to the force from the
//! tyres, from NGP's own torque. The front wheels' adds through the scrub radius (their braking and drive forces) and
//! the steering axis's inclination (their vertical forces, `steer_parts`) carry the ground's roughness, and the force
//! from the tyres leaves them out. Band-passed to [`LOW_HZ`]-[`HIGH_HZ`], they lose their pull (it pointed with the
//! cornering force and was not felt) and the 40-150 Hz buzz a frame's force can't carry; soft-limited, so a rock
//! kicks without slamming the wheel. On the recorded drives a front wheel's texture is 3-5 times stronger on gravel
//! and grass than on tarmac and snow, so it also says when a wheel leaves the road. Fed the runner's kept adds
//! ([`Texture::fold`]); a wheel off the ground adds none. Reads the memory, never writes it.

use std::cell::Cell;

use rbr_physics::car_body::store::CarStore;
use rbr_physics::mem::Mem;
use rbr_physics::wheel::layout::SteerTorque;
use rbr_step::runner::CAR_PTR;

use crate::steer_parts::{parts, steps};

/// The band's edges, Hz: below the lower the pull, above the higher the buzz.
const LOW_HZ: f32 = 5.0;
const HIGH_HZ: f32 = 30.0;

/// The soft limit, times the car's sensitivity `S` (NGP's torque's p90 is about 0.36 `S` on the recorded drives).
const LIMIT: f32 = 0.3;

/// The texture's filter: its low-pass and high-pass stages' states and its value, in the units of NGP's torque.
#[derive(Clone, Copy, Default)]
struct State {
    low: f32,
    high: f32,
    value: f32,
}

/// The texture at a weight, folded a step at a time.
pub struct Texture {
    weight: f32,
    state: Cell<State>,
}

impl Texture {
    pub fn new(weight: f32) -> Self {
        Texture { weight, state: Cell::new(State::default()) }
    }

    /// Folds the adds `torques` kept since the last call, each step of `dt` s, on the car in `mem`.
    pub fn fold(&self, mem: &Mem, torques: &[SteerTorque], dt: f32) {
        let car = mem.u32(CAR_PTR);
        let rack = mem.steering_rack_ratio(car);
        let limit = LIMIT * mem.max_ff_torque(car);
        let pole = |hz: f32| 1.0 / (2.0 * std::f32::consts::PI * hz);
        let low = dt / (pole(HIGH_HZ) + dt);
        let high = pole(LOW_HZ) / (pole(LOW_HZ) + dt);
        let mut s = self.state.get();
        for step in steps(torques) {
            let sum: f64 = step.iter().filter_map(parts).map(|p| p.longitudinal + p.vertical).sum();
            let last = s.low;
            s.low += (rack * sum as f32 - s.low) * low;
            s.high = high * (s.high + s.low - last);
            s.value = limit * (s.high / limit).tanh();
        }
        self.state.set(s);
    }

    /// The texture's torque, in the units of NGP's.
    pub fn torque(&self) -> f32 {
        self.weight * self.state.get().value
    }
}

#[cfg(test)]
mod tests {
    /// The band's two poles pass a 15 Hz wave about whole, and hold a steady input (the pull) and a 120 Hz one down.
    #[test]
    fn the_band_keeps_the_texture_and_drops_the_pull_and_the_buzz() {
        let dt = 1.0 / 720.0;
        let pole = |hz: f32| 1.0 / (2.0 * std::f32::consts::PI * hz);
        let (low, high) = (dt / (pole(super::HIGH_HZ) + dt), pole(super::LOW_HZ) / (pole(super::LOW_HZ) + dt));
        let gain = |input: &dyn Fn(f32) -> f32| {
            let (mut l, mut h, mut peak) = (0.0f32, 0.0f32, 0.0f32);
            for k in 0..7200 {
                let last = l;
                l += (input(k as f32 * dt) - l) * low;
                h = high * (h + l - last);
                if k > 3600 {
                    peak = peak.max(h.abs());
                }
            }
            peak
        };
        let wave = |hz: f32| move |t: f32| (2.0 * std::f32::consts::PI * hz * t).sin();
        assert!(gain(&wave(15.0)) > 0.7, "{}", gain(&wave(15.0)));
        assert!(gain(&|_| 1.0) < 0.01, "{}", gain(&|_| 1.0));
        assert!(gain(&wave(120.0)) < 0.3, "{}", gain(&wave(120.0)));
    }
}
