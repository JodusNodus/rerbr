//! `--circle`'s hold at the limit: the pure pursuit stops adding steering once more steering stops buying lateral
//! acceleration, as a driver holding a steady circle would, and the speed ramp then runs the car wide (the hold's
//! end). Without it the pursuit winds the steering on past the front tyres' peak (0.25 → 0.77 on the flat tarmac
//! circle, fronts at 22-24°) and the circle reads the sliding grip (`notes/tyre-model.md`, "Car-level grip").
//!
//! The rule reads only what a driver feels: the lateral acceleration (the ground velocity's turn rate times the
//! speed) and the steering held, both low-passed over `TAU`. Over the last `WINDOW` seconds it compares their
//! relative rises: once the inward steering has grown by at least `RISE` (of full lock) while the lateral
//! acceleration grew by less than `GAIN` of the steering's relative rise, at `NEAR` its best of the run or more, for
//! `PERSIST` seconds on end, the steering is capped at what it held when that began. The same for every physics and
//! tyre model; no tyre constant enters. A car held in a power drift (rear-limited, steering near straight or
//! countersteering) never winds the inward steering on, so the rule leaves it alone; on rough or sloped ground
//! (RSF 345) the steering's swings away from the best lateral acceleration do not count.

use std::collections::VecDeque;

/// The low-pass time constant (s) of the lateral acceleration and the steering.
const TAU: f32 = 0.5;
/// The window (s) over which the rises are compared.
const WINDOW: f32 = 2.0;
/// The smallest rise of the inward steering (of full lock) over `WINDOW` that counts as winding on, and the least
/// steering held at its start (below it the relative rise means nothing).
const RISE: f32 = 0.05;
/// The cap holds once the lateral acceleration's relative rise is under this share of the steering's (1 where the
/// lateral acceleration grows in proportion, 0 where steering buys none).
const GAIN: f32 = 0.25;
/// Only at this share of the run's best filtered lateral acceleration (since armed) or more.
const NEAR: f32 = 0.95;
/// The condition holds this long (s) on end before the cap is set.
const PERSIST: f32 = 0.5;
/// Under this speed (m/s) the velocity's heading means nothing: the filters wait.
const MOVING: f32 = 0.5;

/// The hold's state over a circle run.
#[derive(Clone, Debug, Default)]
pub(super) struct LimitHold {
    /// The ground velocity's heading after the step before (rad), if moving.
    heading: Option<f32>,
    /// The low-passed lateral acceleration (m/s², counter-clockwise positive) and inward steering (of full lock).
    filtered: Option<(f32, f32)>,
    /// The filtered values per step over the last `WINDOW`.
    history: VecDeque<(f32, f32)>,
    /// The best filtered lateral acceleration since armed (m/s²).
    best: f32,
    /// Since when (s, the armed clock) and at which filtered inward steering the condition has held, if it does.
    onset: Option<(f32, f32)>,
    /// The armed clock (s).
    clock: f32,
    /// The inward steering's cap, once set.
    cap: Option<f32>,
}

impl LimitHold {
    /// Observes the state after a step of `dt` s (`velocity`, m/s) and the inward steering held over it (of full
    /// lock, positive into the counter-clockwise circle); `armed` once the car has settled onto the circle. Returns
    /// the cap on the inward steering, and whether it was set by this step.
    pub(super) fn observe(&mut self, velocity: [f32; 3], inward: f32, dt: f32, armed: bool) -> (Option<f32>, bool) {
        let speed = velocity[0].hypot(velocity[1]);
        if speed < MOVING {
            self.heading = None;
            return (self.cap, false);
        }
        let heading = velocity[1].atan2(velocity[0]);
        let Some(before) = self.heading.replace(heading) else {
            return (self.cap, false);
        };
        let turn = (heading - before + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        let lateral = speed * turn / dt;
        let k = (dt / TAU).min(1.0);
        let (a, s) = match self.filtered {
            Some((a, s)) => (a + k * (lateral - a), s + k * (inward - s)),
            None => (lateral, inward),
        };
        self.filtered = Some((a, s));
        self.history.push_back((a, s));
        let n = (WINDOW / dt).round() as usize;
        if self.history.len() <= n {
            return (self.cap, false);
        }
        let (a0, s0) = self.history.pop_front().expect("window");
        if self.cap.is_some() || !armed {
            self.onset = None;
            return (self.cap, false);
        }
        self.clock += dt;
        self.best = self.best.max(a);
        let rise = s - s0;
        let flat = rise >= RISE && s0 >= RISE && a0 > 0.0 && (a - a0) / a0 < GAIN * rise / s0;
        if !flat || a < NEAR * self.best {
            self.onset = None;
            return (self.cap, false);
        }
        let (since, held) = *self.onset.get_or_insert((self.clock, s));
        if self.clock - since < PERSIST {
            return (self.cap, false);
        }
        self.cap = Some(held);
        (self.cap, true)
    }

    /// The filtered lateral acceleration (m/s²), if any.
    pub(super) fn lateral(&self) -> Option<f32> {
        self.filtered.map(|(a, _)| a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 720.0;

    /// Drives `hold` for `seconds` at `speed` m/s, the lateral acceleration and inward steering at time t from `at`;
    /// returns the first cap set and the time it was set.
    fn run(hold: &mut LimitHold, seconds: f32, speed: f32, at: impl Fn(f32) -> (f32, f32)) -> Option<(f32, f32)> {
        let mut heading = 0.0f32;
        let mut first = None;
        for k in 0..(seconds / DT) as u32 {
            let t = k as f32 * DT;
            let (lateral, inward) = at(t);
            heading += lateral / speed * DT;
            let velocity = [speed * heading.cos(), speed * heading.sin(), 0.0];
            if let (Some(cap), true) = hold.observe(velocity, inward, DT, true) {
                first.get_or_insert((cap, t));
            }
        }
        first
    }

    #[test]
    fn steering_that_buys_lateral_acceleration_is_left_alone() {
        // Below the limit: the steering grows with the lateral acceleration (an understeer gradient on an offset).
        let mut hold = LimitHold::default();
        assert_eq!(run(&mut hold, 20.0, 20.0, |t| (4.0 + 0.4 * t, 0.15 + 0.01 * t)), None);
    }

    #[test]
    fn steering_wound_on_at_a_flat_lateral_acceleration_is_held() {
        // The lateral acceleration rises to 11 m/s² by 10 s and stays; the steering winds on from 0.3 at 0.1 per s.
        let mut hold = LimitHold::default();
        let at = |t: f32| (4.0 + 0.7 * t.min(10.0), if t < 10.0 { 0.15 + 0.015 * t } else { 0.3 + 0.1 * (t - 10.0) });
        let (cap, t) = run(&mut hold, 20.0, 20.0, at).expect("a cap");
        assert!((10.5..12.0).contains(&t), "set at {t} s");
        assert!((0.3..0.4).contains(&cap), "cap {cap}");
        assert_eq!(hold.observe([20.0, 0.0, 0.0], 0.9, DT, true), (Some(cap), false));
    }

    #[test]
    fn a_power_drift_countersteering_is_left_alone() {
        // A rear-limited car: the lateral acceleration levels off while the steering unwinds through straight.
        let mut hold = LimitHold::default();
        let at = |t: f32| (4.0 + 0.5 * t.min(10.0), 0.15 - 0.05 * (t - 10.0).max(0.0));
        assert_eq!(run(&mut hold, 20.0, 18.0, at), None);
    }

    #[test]
    fn a_dip_of_the_lateral_acceleration_under_more_steering_is_left_alone() {
        // Sloped ground: for 2 s the lateral acceleration falls to 0.8 of its rise while the steering swings on.
        let mut hold = LimitHold::default();
        let dip = |t: f32| (10.0..12.0).contains(&t);
        let at = |t: f32| {
            let a = 4.0 + 0.5 * t;
            if dip(t) {
                (0.8 * a, 0.3 + 0.1 * (t - 10.0))
            } else {
                (a, 0.2 + 0.01 * t)
            }
        };
        assert_eq!(run(&mut hold, 20.0, 15.0, at), None);
    }

    #[test]
    fn nothing_is_held_until_armed() {
        let mut hold = LimitHold::default();
        for k in 0..20 * 720 {
            let inward = 0.3 + 0.1 * k as f32 * DT;
            assert_eq!(hold.observe([20.0, 0.0, 0.0], inward, DT, false), (None, false));
        }
    }
}
