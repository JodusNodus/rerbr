//! `--manoeuvre`: open-loop handling manoeuvres on a pad stage (ours, test only; `harness/handling.py` runs the set).
//!
//! The car holds its start heading on the pad and a speed controller brings it to the entry speed (full throttle
//! through the gears, then a PI on the throttle, brake only far over), shifting up at the race controller's shift
//! point; once the speed has held for `CALM` s with no shift the gear is fixed. The circle manoeuvres then steer into
//! the circle (`ENTRY` s ramp, `SETTLE` s with the speed controller on), and the inputs go open loop from the run's
//! start, on a fixed schedule: the steering as the exact float the payload carries (the 8-bit steering of the
//! controller steps 0.008, which at 0.1 g/s is a step every half second), the throttle held at the speed
//! controller's mean over the second before. Steering is a fraction of full lock, positive to the left
//! (counter-clockwise: the payload's steering is its negative). The trace's `since` column counts the seconds
//! from the start of the run.
//!
//! `catch` closes the loop: the sine dwell's steering to the end of its dwell, which leaves the car turning, and from
//! there a driver who steers into the slide he saw his reaction time before.

use std::collections::VecDeque;

use rbr_gameplay::control::QuantizedControl;

use super::{Driver, Sense, FIRST, LAUNCH, PEAK_TIME, SHIFT_DOWN, SHIFT_UP, STEP_SECONDS, TOP_GEAR, UP_LIMIT};

/// The speed controller: throttle per m/s of error, per m/s of error and second, and the pedal over `OVER`.
const KP: f32 = 0.4;
const KI: f32 = 0.2;
const OVER: f32 = 1.5;
/// The speed holds within `HOLD` m/s for `CALM` s (no shift in them) before the run, and the throttle's mean is over
/// `MEAN` s.
const HOLD: f32 = 0.3;
const CALM: f32 = 2.0;
const MEAN: f32 = 1.0;
/// The heading hold's steering per rad of heading error, and its limit.
const HEADING_GAIN: f32 = 1.5;
const HEADING_LIMIT: f32 = 0.2;
/// The circle manoeuvres' steering ramp into the circle and the seconds on it before the run.
const ENTRY: f32 = 1.5;
const SETTLE: f32 = 5.0;
/// The 1.02 steps (7 ms) between gear pulses while accelerating.
const SHIFT_GAP: u32 = 60;
/// The step steer's ramp, and the throttle's and the brake's on the circle manoeuvres, s.
const STEP_RAMP: f32 = 0.15;
const PEDAL_RAMP: f32 = 0.1;
/// The sine dwell's frequency (Hz) and dwell (s).
const SINE_HZ: f32 = 0.7;
const DWELL: f32 = 0.5;
/// The run's length after its start, s: the metrics look 4 s past the pedal and 1.75 s past the sine dwell's end.
const RUN: f32 = 4.5;
/// Full lock as the payload holds it (the largest 8-bit steering, 127 of 127.5: the pure step takes no more).
const LOCK: f32 = 127.0 / 127.5;
/// The ramp steer's longest run, s.
const RAMP_RUN: f32 = 60.0;
/// `catch`: the driver takes the steering at the end of the sine's dwell, for `CATCH_RUN` s. He turns the front
/// wheels `COUNTER` of lock per rad towards where the car is going (full lock at 57 deg of slide), no faster than
/// `CATCH_RATE` of lock per s (1000 deg/s at a wheel of two turns lock to lock), and steers for no heading. He acts
/// on the slide he saw `REACTION` s before, unless the manoeuvre gives his reaction time, carried forward by that
/// time at the rate he saw over `SEEN_RATE` s (the lead of McRuer's crossover model). Ours, the values: on gravel at
/// 80 and 110 km/h he swings no car at 0.2 or 0.4 s, where 1.5 of lock per rad does at 0.4 s and 2 without the lead
/// at 0.2 s; on tarmac at 110 km/h his correction spins the Fiesta Rally2 (`notes/tyre-model.md`, "A driver's catch").
const CATCH_FROM: f32 = 0.75 / SINE_HZ + DWELL;
const CATCH_RUN: f32 = 6.0;
const COUNTER: f32 = 1.0;
const CATCH_RATE: f32 = 3.0;
const REACTION: f32 = 0.2;
const SEEN_RATE: f32 = 0.05;
/// The speed under which the car's direction of travel has no meaning, m/s.
const MOVING: f32 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    RampSteer,
    StepSteer,
    SineDwell,
    LiftInTurn,
    BrakeInTurn,
    PowerInTurn,
    Catch,
}

/// `--manoeuvre KIND,KMH,AMP[,EXTRA]`: the kind (`ramp-steer`, `step-steer`, `sine-dwell`, `lift-in-turn`,
/// `brake-in-turn`, `power-in-turn`, `catch`), the entry speed (km/h), the steering (fraction of full lock: the ramp's
/// rate per second for `ramp-steer`; the circle's steering for the `-in-turn` kinds) and `EXTRA`: the brake pedal of
/// `brake-in-turn` (0-1), the lock `ramp-steer` ends at (default 1), the driver's reaction time of `catch` (s, 0-1,
/// default 0.2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Manoeuvre {
    pub kind: Kind,
    /// m/s.
    pub speed: f32,
    pub amp: f32,
    pub extra: f32,
}

impl std::str::FromStr for Manoeuvre {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let bad = || format!("--manoeuvre {s}: want KIND,KMH,AMP[,EXTRA]");
        let mut parts = s.split(',');
        let kind = match parts.next().ok_or_else(bad)? {
            "ramp-steer" => Kind::RampSteer,
            "step-steer" => Kind::StepSteer,
            "sine-dwell" => Kind::SineDwell,
            "lift-in-turn" => Kind::LiftInTurn,
            "brake-in-turn" => Kind::BrakeInTurn,
            "power-in-turn" => Kind::PowerInTurn,
            "catch" => Kind::Catch,
            _ => return Err(bad()),
        };
        let numbers: Vec<f32> = parts.map(|x| x.parse().map_err(|_| bad())).collect::<Result<_, _>>()?;
        match numbers[..] {
            [kmh, amp] | [kmh, amp, _] if kmh > 0.0 && amp > 0.0 => {
                let extra = numbers.get(2).copied();
                if kind == Kind::BrakeInTurn && !extra.is_some_and(|p| p > 0.0 && p <= 1.0) {
                    return Err(format!("--manoeuvre {s}: brake-in-turn wants a pedal, 0-1, as EXTRA"));
                }
                if kind == Kind::Catch && !extra.is_none_or(|t| (0.0..=1.0).contains(&t)) {
                    return Err(format!("--manoeuvre {s}: catch wants a reaction time, 0-1 s, as EXTRA"));
                }
                let default = if kind == Kind::Catch { REACTION } else { 1.0 };
                Ok(Manoeuvre { kind, speed: kmh / 3.6, amp, extra: extra.unwrap_or(default) })
            }
            _ => Err(bad()),
        }
    }
}

impl Kind {
    /// Whether the run starts on a steady circle.
    fn circle(self) -> bool {
        matches!(self, Kind::LiftInTurn | Kind::BrakeInTurn | Kind::PowerInTurn)
    }
}

/// Where the manoeuvre is: the approach on the start heading, the circle's entry (seconds in), the run (seconds in).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Approach,
    Entry(f32),
    Run(f32),
}

/// The manoeuvre's state in the driver.
#[derive(Clone, Debug)]
pub struct Pad {
    man: Manoeuvre,
    phase: Phase,
    /// The heading held (rad), taken at the first control.
    heading: Option<f32>,
    /// The speed controller's integral, the throttle's mean over `MEAN` s, and the seconds speed has held.
    integral: f32,
    mean: f32,
    calm: f32,
    /// The throttle held through the run, and the steering of the last control (payload value).
    held: f32,
    pub steering: f32,
    /// `catch`: the direction of travel from the nose (rad, positive left) over the driver's reaction time and
    /// `SEEN_RATE`, the oldest first.
    seen: VecDeque<f32>,
    pub done: bool,
}

impl Pad {
    pub fn new(man: Manoeuvre) -> Self {
        Pad {
            man,
            phase: Phase::Approach,
            heading: None,
            integral: 0.3,
            mean: 0.0,
            calm: 0.0,
            held: 0.0,
            steering: 0.0,
            seen: VecDeque::new(),
            done: false,
        }
    }

    /// The seconds since the run's start, else `None`.
    pub fn since(&self) -> Option<f32> {
        match self.phase {
            Phase::Run(t) => Some(t),
            _ => None,
        }
    }

    /// The steering (positive left) at `t` s into the run, and the run's length.
    fn steer(&self, t: f32) -> (f32, f32) {
        let a = self.man.amp;
        match self.man.kind {
            Kind::RampSteer => {
                ((a * t).min(self.man.extra.min(1.0)), (self.man.extra.min(1.0) / a + 1.0).min(RAMP_RUN))
            }
            Kind::StepSteer => (a * (t / STEP_RAMP).min(1.0), RUN),
            Kind::SineDwell | Kind::Catch => {
                let period = 1.0 / SINE_HZ;
                let phase = if t < 0.75 * period {
                    t
                } else if t < 0.75 * period + DWELL {
                    0.75 * period
                } else {
                    (t - DWELL).min(period)
                };
                let sine = 1.0 / SINE_HZ + DWELL + 1.75 + 0.5;
                let length = if self.man.kind == Kind::Catch { CATCH_FROM + CATCH_RUN } else { sine };
                (a * (std::f32::consts::TAU * SINE_HZ * phase).sin(), length)
            }
            _ => (a, RUN),
        }
    }
}

fn wrap(angle: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    (angle + PI).rem_euclid(TAU) - PI
}

/// The slide (rad) the `catch`'s driver steers on: the one he saw `delay` steps ago, carried forward those steps at
/// its rate over the samples before it.
fn foreseen(seen: &VecDeque<f32>, delay: usize) -> f32 {
    let then = seen.len().saturating_sub(delay + 1);
    seen[then] + wrap(seen[then] - seen[0]) * delay as f32 / then.max(1) as f32
}

impl<'a> Driver<'a> {
    /// The driver running `man` (`--manoeuvre`).
    pub fn with_manoeuvre(self, man: Manoeuvre) -> Self {
        Self { pad: Some(Pad::new(man)), ..self }
    }

    /// The control for `step` of a manoeuvre (module comment).
    pub(super) fn pad_control(&mut self, step: u32, sense: &Sense) -> QuantizedControl {
        let mut pad = self.pad.take().expect("a manoeuvre");
        let dt = rbr_recording::STEP_SECONDS / self.per.max(1) as f32;
        let speed = sense.pose.speed();
        let f = sense.pose.forward();
        let yaw = f[1].atan2(f[0]);
        let heading = *pad.heading.get_or_insert(yaw);
        let error = pad.man.speed - speed;
        // The speed controller.
        pad.integral = (pad.integral + KI * error * dt).clamp(0.0, 1.0);
        let demand = (KP * error + pad.integral).clamp(0.0, 1.0);
        let (mut throttle, mut brake) = (demand, if error < -OVER { (-0.15 * (error + OVER)).min(0.8) } else { 0.0 });
        pad.mean += (throttle - pad.mean) * (dt / MEAN).min(1.0);
        let mut flags = 0;
        // Positive left, as the schedules are: a target to the left (counter-clockwise) steers left.
        let mut steer = (wrap(heading - yaw) * HEADING_GAIN).clamp(-HEADING_LIMIT, HEADING_LIMIT);
        let kind = pad.man.kind;
        match pad.phase {
            Phase::Approach | Phase::Entry(_) => {
                let up = SHIFT_UP.min(UP_LIMIT * self.limit);
                let frozen = pad.calm >= CALM || !matches!(pad.phase, Phase::Approach);
                if !frozen {
                    // A standing start, as the plain driver's: first once the revs pass `LAUNCH` of the limit or
                    // set no new maximum for `PEAK_TIME` (the experimental engine's start limiter sits under it).
                    if sense.gear < FIRST && throttle > 0.0 && sense.engine > self.launch_peak {
                        (self.launch_peak, self.launch_peak_at) = (sense.engine, step);
                    }
                    let peaked = sense.engine > LAUNCH * self.limit
                        || (self.launch_peak > 0.0
                            && step >= self.launch_peak_at + self.steps((PEAK_TIME / STEP_SECONDS) as u32));
                    if (sense.gear < FIRST && peaked)
                        || (sense.gear >= FIRST && sense.gear < TOP_GEAR && sense.engine > up)
                    {
                        flags = self.shift(step, SHIFT_GAP, 0x01);
                    } else if sense.gear > FIRST && sense.engine < SHIFT_DOWN {
                        flags = self.shift(step, SHIFT_GAP, 0x02);
                    }
                }
                let shifted = flags != 0 || step < self.shifted + self.steps(SHIFT_GAP);
                let held = error.abs() < HOLD && sense.gear >= FIRST && !shifted;
                pad.calm = if held { pad.calm + dt } else { 0.0 };
                match pad.phase {
                    Phase::Approach if pad.calm >= CALM => {
                        pad.held = pad.mean;
                        pad.phase = if kind.circle() { Phase::Entry(0.0) } else { Phase::Run(0.0) };
                        eprintln!("autopilot: step {step}: {kind:?} at {speed:.2} m/s in gear {}", sense.gear);
                    }
                    Phase::Entry(t) => {
                        steer = pad.man.amp * (t / ENTRY).min(1.0);
                        if t >= ENTRY + SETTLE {
                            pad.held = pad.mean;
                            pad.phase = Phase::Run(0.0);
                            eprintln!(
                                "autopilot: step {step}: {kind:?} on the circle at {speed:.2} m/s, throttle {:.3}",
                                pad.held
                            );
                        } else {
                            pad.phase = Phase::Entry(t + dt);
                        }
                    }
                    _ => {}
                }
            }
            Phase::Run(t) => {
                let (s, length) = pad.steer(t);
                steer = s;
                let ramp = (t / PEDAL_RAMP).min(1.0);
                match kind {
                    Kind::RampSteer => {}
                    Kind::StepSteer => (throttle, brake) = (pad.held, 0.0),
                    Kind::SineDwell | Kind::Catch => (throttle, brake) = (0.0, 0.0),
                    Kind::LiftInTurn => (throttle, brake) = (pad.held * (1.0 - ramp), 0.0),
                    Kind::BrakeInTurn => (throttle, brake) = (pad.held * (1.0 - ramp), pad.man.extra * ramp),
                    Kind::PowerInTurn => (throttle, brake) = (pad.held + (1.0 - pad.held) * ramp, 0.0),
                }
                if kind == Kind::Catch {
                    let v = sense.pose.velocity;
                    let travel = if speed > MOVING { wrap(v[1].atan2(v[0]) - yaw) } else { 0.0 };
                    let (delay, window) = ((pad.man.extra / dt).round() as usize, (SEEN_RATE / dt).round() as usize);
                    pad.seen.push_back(travel);
                    if pad.seen.len() > delay + window + 1 {
                        pad.seen.pop_front();
                    }
                    if t >= CATCH_FROM {
                        let want = (foreseen(&pad.seen, delay) * COUNTER).clamp(-LOCK, LOCK);
                        let last = -pad.steering;
                        steer = last + (want - last).clamp(-CATCH_RATE * dt, CATCH_RATE * dt);
                    }
                }
                pad.done = t >= length;
                pad.phase = Phase::Run(t + dt);
            }
        }
        // The exact float (positive left; the payload's is negative left), clamped to the lock.
        pad.steering = -steer.clamp(-LOCK, LOCK);
        self.pulling = false;
        let pedal = |x: f32| (x * 255.0).round() as u8;
        let q =
            QuantizedControl { flags, throttle: pedal(throttle), brake: pedal(brake), ..QuantizedControl::default() };
        self.pad = Some(pad);
        q
    }

    /// The manoeuvre's steering for the payload (the exact float), if one runs.
    pub fn pad_steering(&self) -> Option<f32> {
        self.pad.as_ref().map(|p| p.steering)
    }

    /// Whether the manoeuvre has run its length.
    pub fn pad_done(&self) -> bool {
        self.pad.as_ref().is_some_and(|p| p.done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catch_s_driver_foresees_a_steady_rate() {
        let seen: VecDeque<f32> = (0..6).map(|i| 0.01 * i as f32).collect();
        assert!((foreseen(&seen, 3) - seen[5]).abs() < 1e-6, "three steps late, two of rate");
        assert_eq!(foreseen(&seen, 0), seen[5], "no reaction time");
        assert_eq!(foreseen(&VecDeque::from([0.2]), 3), 0.2, "nothing seen before");
    }
}
