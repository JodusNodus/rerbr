//! A car model's pace within its class (ours): the speed plan's acceleration, top speed and downforce ([`ClassPace`])
//! for each model from its physics specs ([`CarSpecs`]), so a field's cars are as quick as their physics and not all
//! alike: a Group B car pulls hard, a modern one holds more through the fast corners on its aero.
//!
//! [`straight`] runs the car's standing start in a few lines of physics, the drive force the lesser of the traction
//! (the driven wheels' share of the weight times [`GRIP`]) and [`POWER_SHARE`] of the wheel power over the speed,
//! less the drag and the rolling resistance, and fits the speed plan's law `a(v) = accel · (1 − v / top)` to the
//! times it reaches each speed. The corners and the braking take the class's grip plus [`AERO_GRIP`] of the car's
//! downforce, which grows with the square of the speed. [`for_car`] scales the class's pace by the car's over the
//! field's mean car ([`mean`]), so the class keeps its anchor (the class table in `pace`) and the field's models
//! spread around it.
//!
//! Calibrated against the NGP physics' own runs of the 50 installed cars (`scripts/car-pace-sweep.sh`,
//! `notes/season.md` "Calibration"): the standing starts' 0-60, 0-100 and 0-140 km/h within 7.6 % RMS; the 40 m
//! circle's grip within 1.6 % on the downforce alone (the tyres' `TyrePerformancePct` and the mass showed no effect).
//!
//! The speed plan leaves out what makes a car hard to drive on the limit; [`CarPace::handling`] adds it as a time
//! factor by surface, fitted to the RallySimFans hot laps (players' best times on 519 stages, each car's effect over
//! the stage and the driver): a rear-driven car loses most and more on the loose, a front-driven one some on the
//! loose, and downforce helps beyond its grip in the plan. With it the 42 installed cars on the boards sit within
//! 1.5 % RMS of their effect on tarmac, 2.0 % on gravel and 2.3 % on snow left out of the fit one at a time (2.5, 3.4
//! and 3.8 % without), past the offset all cars share.

use crate::config::Surface;
use crate::pace::ClassPace;
use rbr_formats::car_specs::{CarSpecs, Drive};

/// The tyres' grip under drive on dry tarmac (g).
const GRIP: f32 = 0.9;
/// The driven wheels' share of the weight under acceleration.
const FRONT_SHARE: f32 = 0.45;
const REAR_SHARE: f32 = 0.7;
/// The share of the peak power a standing start uses on average over the gears and shifts.
const POWER_SHARE: f32 = 0.75;
/// The rolling resistance coefficient.
const ROLLING: f32 = 0.015;
const AIR: f32 = 1.2;
const G: f32 = 9.81;
/// The fit runs to this speed (m/s): a rally car's range, past which the gearing caps it.
const FIT_TOP: f32 = 50.0;
/// The gearing's cap on the top speed (m/s): the drag alone would allow more.
const GEARED_TOP: f32 = 62.0;
/// The share of the downforce the tyres turn into grip.
const AERO_GRIP: f32 = 0.86;
/// The speed `CarSpecs::downforce` is given at (m/s): 150 km/h.
const DOWNFORCE_SPEED: f32 = 41.666_668;
/// The handling's log time on tarmac, gravel and snow: for rear drive, for front drive, and per N/kg of downforce at
/// 150 km/h (the hot laps' fit, `notes/season.md` "Calibration").
const HANDLING_REAR: [f32; 3] = [0.046, 0.066, 0.083];
const HANDLING_FRONT: [f32; 3] = [0.003, 0.025, 0.017];
const HANDLING_DOWNFORCE: [f32; 3] = [-0.011, -0.011, -0.007];

/// A car's straight-line performance as the speed plan takes it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Straight {
    /// From low speed (m/s²).
    pub accel: f32,
    /// Where the acceleration falls to 0 (m/s).
    pub top: f32,
}

/// A car's pace: its straight line, the downforce's grip per (m/s)² ([`ClassPace::aero`]) and its handling's time
/// factor on tarmac, gravel and snow ([`CarPace::handling`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CarPace {
    pub straight: Straight,
    pub aero: f32,
    pub handling: [f32; 3],
}

impl CarPace {
    pub fn of(specs: &CarSpecs) -> CarPace {
        let aero = AERO_GRIP * specs.downforce / (specs.mass * G * DOWNFORCE_SPEED * DOWNFORCE_SPEED);
        let handling = std::array::from_fn(|k| {
            let drive = match specs.drive {
                Drive::Rear => HANDLING_REAR[k],
                Drive::Front => HANDLING_FRONT[k],
                Drive::All => 0.0,
            };
            (drive + HANDLING_DOWNFORCE[k] * specs.downforce / specs.mass).exp()
        });
        CarPace { straight: straight(specs), aero, handling }
    }

    /// The time factor on `surface` for what the speed plan leaves out: the car's drive and downforce on the limit.
    pub fn handling(&self, surface: Surface) -> f32 {
        self.handling[match surface {
            Surface::Tarmac => 0,
            Surface::Gravel => 1,
            Surface::Snow => 2,
        }]
    }
}

/// The car's standing start, fitted to the speed plan's law.
pub fn straight(specs: &CarSpecs) -> Straight {
    let share = match specs.drive {
        Drive::Front => FRONT_SHARE,
        Drive::Rear => REAR_SHARE,
        Drive::All => 1.0,
    };
    let traction = share * GRIP * G;
    let wheel_power = specs.power * specs.efficiency * POWER_SHARE;
    let accel = |v: f32| {
        let drive = traction.min(wheel_power / (specs.mass * v.max(1.0)));
        drive - (0.5 * AIR * specs.drag_area * v * v) / specs.mass - ROLLING * G
    };
    // The times it reaches each 2 m/s, to the fit's top or as far as it gets in 60 s.
    let (mut v, mut t, dt) = (0.0f32, 0.0f32, 0.01f32);
    let mut points = Vec::new();
    let mut next = 2.0;
    while v < FIT_TOP.min(GEARED_TOP - 1.0) && t < 60.0 {
        let a = accel(v);
        if a <= 0.0 {
            break;
        }
        v += a * dt;
        t += dt;
        if v >= next {
            points.push((v, t));
            next += 2.0;
        }
    }
    fit(&points, GEARED_TOP)
}

/// The law `a(v) = accel · (1 − v / top)` closest to the times `points` (speed, time) reach, `top` at most `cap`:
/// under it a car reaches `v` at `t = −(top / accel) · ln(1 − v / top)`.
pub fn fit(points: &[(f32, f32)], cap: f32) -> Straight {
    let fastest = points.iter().map(|p| p.0).fold(0.0, f32::max);
    let mut best = (f32::MAX, Straight { accel: 1.0, top: cap });
    let mut top = fastest + 0.5;
    while top <= cap {
        let l: Vec<f32> = points.iter().map(|&(v, _)| -(1.0 - v / top).ln()).collect();
        let (ll, tl) = points.iter().zip(&l).fold((0.0, 0.0), |(ll, tl), (&(_, t), &l)| (ll + l * l, tl + t * l));
        if tl > 0.0 {
            // For this top the best accel is closed form: t = (top / accel) · l in the least squares.
            let accel = top * ll / tl;
            let error: f32 = points.iter().zip(&l).map(|(&(_, t), &l)| (t - top * l / accel).powi(2)).sum();
            if error < best.0 {
                best = (error, Straight { accel, top });
            }
        }
        top += 0.25;
    }
    best.1
}

/// The field's mean car, for [`for_car`].
pub fn mean(cars: &[CarPace]) -> Option<CarPace> {
    let n = cars.len() as f32;
    let avg = |f: &dyn Fn(&CarPace) -> f32| cars.iter().map(f).sum::<f32>() / n;
    (!cars.is_empty()).then(|| CarPace {
        straight: Straight { accel: avg(&|c| c.straight.accel), top: avg(&|c| c.straight.top) },
        aero: avg(&|c| c.aero),
        handling: std::array::from_fn(|k| avg(&|c| c.handling[k])),
    })
}

/// The class's pace for `car`: the acceleration and top speed scaled by the car's over the field's `mean` car, and
/// the car's own downforce. The mean car's ([`for_car`] of `mean` itself) is the base its times compare with; the
/// speed plan's time ratio then takes the handling's ([`CarPace::handling`]) over the mean car's.
pub fn for_car(class: ClassPace, car: CarPace, mean: CarPace) -> ClassPace {
    ClassPace {
        accel: class.accel * car.straight.accel / mean.straight.accel,
        top_speed: class.top_speed * car.straight.top / mean.straight.top,
        aero: car.aero,
        ..class
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn specs(kw: f32, kg: f32, drive: Drive) -> CarSpecs {
        let (drag_area, power, downforce) = (0.8, kw * 1000.0, 0.0);
        CarSpecs { mass: kg, drag_area, power, power_rpm: 6000.0, efficiency: 0.88, drive, downforce }
    }

    #[test]
    fn the_fit_finds_the_law_it_was_given() {
        let law = Straight { accel: 7.0, top: 55.0 };
        let points: Vec<(f32, f32)> =
            (1..20).map(|k| 2.0 * k as f32).map(|v| (v, -(law.top / law.accel) * (1.0 - v / law.top).ln())).collect();
        let found = fit(&points, 80.0);
        assert!((found.accel - 7.0).abs() < 0.1 && (found.top - 55.0).abs() < 0.3, "{found:?}");
    }

    #[test]
    fn power_weight_and_traction_order_the_cars() {
        let r5 = straight(&specs(210.0, 1431.0, Drive::All));
        let wrc = straight(&specs(279.0, 1391.0, Drive::All));
        let rally4 = straight(&specs(154.0, 1280.0, Drive::Front));
        let rwd = straight(&specs(210.0, 1431.0, Drive::Rear));
        assert!(wrc.accel > r5.accel && r5.accel > rally4.accel, "{wrc:?} {r5:?} {rally4:?}");
        assert!(r5.accel > rwd.accel, "the same power on two driven wheels pulls away slower: {r5:?} {rwd:?}");
        // The field's mean car runs the class's own pace; a quicker one a quicker pace.
        let class = crate::pace::class_pace("Rally2");
        let pace = |s: Straight| CarPace { straight: s, aero: 0.0, handling: [1.0; 3] };
        let m = mean(&[pace(r5), pace(wrc)]).unwrap();
        assert_eq!(for_car(class, m, m), class);
        assert!(for_car(class, pace(wrc), m).accel > class.accel);
    }

    #[test]
    fn rear_drive_costs_most_on_the_loose() {
        let rear = CarPace::of(&specs(210.0, 1300.0, Drive::Rear));
        let all = CarPace::of(&specs(210.0, 1300.0, Drive::All));
        let cost = |s| rear.handling(s) / all.handling(s);
        assert!(cost(Surface::Tarmac) > 1.0, "{rear:?}");
        assert!(cost(Surface::Snow) > cost(Surface::Gravel) && cost(Surface::Gravel) > cost(Surface::Tarmac));
    }

    #[test]
    fn downforce_holds_the_fast_corners() {
        let mut modern = specs(210.0, 1431.0, Drive::All);
        modern.downforce = 3100.0;
        let group_b = specs(265.0, 1300.0, Drive::All);
        let (modern, group_b) = (CarPace::of(&modern), CarPace::of(&group_b));
        // 3.1 kN at 150 km/h on 1431 kg: about a fifth more grip at 150 km/h.
        assert!((modern.aero * DOWNFORCE_SPEED * DOWNFORCE_SPEED - 0.19).abs() < 0.01, "{modern:?}");
        let m = mean(&[modern, group_b]).unwrap();
        let class = crate::pace::class_pace("WRC");
        let (a, b) = (for_car(class, modern, m), for_car(class, group_b, m));
        assert!(a.aero > b.aero && b.accel > a.accel, "{a:?} {b:?}");
        // On a road of fast sweepers the downforce wins.
        // A 200 m radius sweeper, three times round.
        let sweeper: Vec<[f32; 3]> =
            (0..2500).map(|i| i as f32 * 0.0075).map(|t| [200.0 * t.cos(), 200.0 * t.sin(), 0.0]).collect();
        let time = |p: &ClassPace| crate::pace::reference_time(&sweeper, Surface::Tarmac, 0.0, p);
        assert!(time(&a) < time(&b), "{} {}", time(&a), time(&b));
    }
}
