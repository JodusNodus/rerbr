//! The engine scorecard's measurements (`harness/engine_scorecard.py`, `data/engine-targets.csv`): `cargo run
//! --release -p rbr-engine --example engine_scorecard -- [CAR_FOLDER ...] [class:GROUP:YEAR ...]`. Prints `key value`
//! lines: `SAMPLE:KEY` for each synthetic sample and each RSF car folder named (needs `RBR_RSF`), `model:KEY` for the
//! model's constants, `class:GROUP:YEAR:restrictor_mm` for each class asked (0 for none). Warm engine, reference air,
//! steady at a fixed speed unless a key says otherwise.

#[path = "../tests/rsf/mod.rs"]
mod rsf;

use rbr_engine::calibrate;
use rbr_engine::class;
use rbr_engine::engine::{step, Spec, State, Step, RPM};
use rbr_engine::gas::{AFR, LAMBDA, LHV};
use rbr_engine::realistic::Realistic;
use rbr_engine::{friction, samples, turbo, Car, Params};
use std::f64::consts::PI;
use std::path::Path;

/// The pedal in 0..=1 at which `f` (rising with the pedal) crosses 0, by bisection.
fn pedal_where(f: impl Fn(f64) -> f64) -> f64 {
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..30 {
        let mid = 0.5 * (lo + hi);
        if f(mid) < 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    hi
}

/// The steady torque at `rpm` with the manifold at `kpa`, or NaN when the pedal cannot reach it.
fn torque_at_map(spec: &Spec, rpm: f64, kpa: f64) -> f64 {
    let pedal = pedal_where(|x| calibrate::part(spec, rpm, x).0.p_im - kpa * 1e3);
    let (s, out) = calibrate::part(spec, rpm, pedal);
    if (s.p_im - kpa * 1e3).abs() < 500.0 {
        out.torque
    } else {
        f64::NAN
    }
}

/// The steady closed pedal at `rpm`, come down from 300 rpm under the limit (so the overrun cut has armed, as on a
/// coast down).
fn coast(spec: &Spec, rpm: f64) -> (State, f64) {
    let mut s = State::new();
    calibrate::steady(spec, &mut s, spec.rpm_limit - 300.0, 0.0);
    let out = calibrate::steady(spec, &mut s, rpm, 0.0);
    (s, out.torque)
}

/// Seconds at a fixed `rpm` from the steady pedal for 5 bar BMEP to 90 % of the steady WOT torque (the [Chalmers]
/// case, as `tests/model.rs` `spool_at`).
fn spool(spec: &Spec, v_d: f64, rpm: f64) -> f64 {
    let aim = 5e5 * v_d / (4.0 * PI);
    let pedal = pedal_where(|x| calibrate::part(spec, rpm, x).1.torque - aim);
    let mut s = calibrate::part(spec, rpm, pedal).0;
    let wot = calibrate::wot(spec, rpm).1.torque;
    let x = Step::at(rpm / RPM, 1.0, calibrate::DT);
    (1..=(5.0 / calibrate::DT) as usize)
        .find(|_| step(spec, &mut s, &x).torque >= 0.9 * wot)
        .map_or(f64::INFINITY, |i| i as f64 * calibrate::DT)
}

fn measure(p: &Params) -> Vec<(&'static str, f64)> {
    let m = Realistic::new(p);
    let spec = m.spec();
    let v_d = spec.geometry.v_d;
    let bmep = |t: f64| t * 4.0 * PI / v_d / 1e5;
    let celsius = |s: &State| s.t_em - 273.15;
    let idle = spec.rpm_idle;
    let mut out = Vec::new();

    // Closed pedal on a coast down (the overrun cut where the ECU has one), and NGP's braking point at the limit.
    for (rpm, bmep_key, map_key) in [
        (2000.0, "closed_bmep_2000", "closed_map_2000"),
        (3000.0, "closed_bmep_3000", "closed_map_3000"),
        (4000.0, "closed_bmep_4000", "closed_map_4000"),
        (5000.0, "closed_bmep_5000", "closed_map_5000"),
    ] {
        let (s, t) = coast(spec, rpm);
        out.push((bmep_key, bmep(t)));
        out.push((map_key, s.p_im / 1e3));
        if rpm == 3000.0 && spec.turbo.is_some() {
            out.push(("cut_exhaust_3000", celsius(&s)));
        }
    }
    let ngp = -f64::from(p.brake_coefficient) * f64::from(p.max_torque);
    out.push(("closed_limit_ratio", calibrate::closed(spec, spec.rpm_limit).1.torque / ngp));
    out.push(("friction_scale", m.calibration().friction_scale));
    let warm = spec.friction(2000.0, friction::phi(friction::T_OIL_REF), 1.0);
    out.push(("cold_friction_2000", spec.friction(2000.0, friction::phi(293.15), 1.0) / warm));

    // Part load: the manifold pressure of no net torque, and torque at a manifold pressure.
    let pedal = pedal_where(|x| calibrate::part(spec, 2500.0, x).1.torque);
    out.push(("zero_map_2500", calibrate::part(spec, 2500.0, pedal).0.p_im / 1e3));
    let t50 = torque_at_map(spec, 2500.0, 50.0);
    out.push(("bmep_50_2500", bmep(t50)));
    out.push(("bmep_90_2500", bmep(torque_at_map(spec, 2500.0, 90.0))));
    if spec.turbo.is_some() {
        let t100 = torque_at_map(spec, 2500.0, 100.0);
        out.push(("bmep_100_1500", bmep(torque_at_map(spec, 1500.0, 100.0))));
        out.push(("bmep_100_2500", bmep(t100)));
        out.push(("share_50_2500", 100.0 * t50 / t100));
    }
    // K as a gross indicated efficiency on the λ the model burns.
    for (rpm, key) in [(2500.0, "eta_ig_2500"), (4000.0, "eta_ig_4000"), (idle, "eta_ig_idle")] {
        out.push((key, spec.k.at(rpm) * AFR * LAMBDA / LHV));
    }

    // Idle, and a closed pedal over idle with the fuel on (under the overrun cut's start).
    let (s, _) = calibrate::part(spec, idle, 0.0);
    out.push(("idle_map", s.p_im / 1e3));
    if spec.turbo.is_some() {
        out.push(("idle_exhaust", celsius(&s)));
    }
    let lowest = [50.0, 100.0, 200.0, 300.0, 500.0, 700.0]
        .iter()
        .map(|d| {
            let mut s = State::new();
            calibrate::steady(spec, &mut s, idle, 0.0);
            calibrate::steady(spec, &mut s, idle + d, 0.0);
            s.p_im / 1e3
        })
        .fold(f64::INFINITY, f64::min);
    out.push(("fired_map_min", lowest));

    // Exhaust and turbo.
    if spec.turbo.is_some() {
        out.push(("exhaust_015_1500", celsius(&calibrate::part(spec, 1500.0, 0.15).0)));
        let c = m.calibration();
        let rated = c.wot.iter().map(|k| (k.0 * k.2, k.0)).fold((0.0, 0.0), |a, b| if b.0 > a.0 { b } else { a }).1;
        out.push(("rated_rpm", rated));
        out.push(("exhaust_rated", celsius(&calibrate::wot(spec, rated).0)));
        for (rpm, key) in [(1000.0, "spool_1000"), (2000.0, "spool_2000"), (3000.0, "spool_3000")] {
            out.push((key, spool(spec, v_d, rpm)));
        }
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (queries, folders): (Vec<&String>, Vec<&String>) = args.iter().partition(|a| a.starts_with("class:"));
    let mut cars: Vec<(String, Params)> = samples::all().into_iter().map(|(n, p)| (n.to_owned(), p)).collect();
    if !folders.is_empty() {
        let root = std::env::var_os("RBR_RSF").expect("RBR_RSF for RSF cars");
        for f in folders {
            cars.push((f.clone(), rsf::params(Path::new(&root), f).expect("the car")));
        }
    }
    let results: Vec<(String, Vec<(&str, f64)>)> = std::thread::scope(|scope| {
        let handles: Vec<_> = cars.iter().map(|(n, p)| (n, scope.spawn(|| measure(p)))).collect();
        handles.into_iter().map(|(n, h)| (n.clone(), h.join().expect("a measurement"))).collect()
    });
    for (name, rows) in results {
        for (key, value) in rows {
            println!("{name}:{key} {value}");
        }
    }
    println!("model:bearing_loss_80k {}", turbo::losses(80_000.0 / RPM));
    println!("model:inertia {}", turbo::INERTIA);
    println!("model:lambda {LAMBDA}");
    for q in queries {
        let mut parts = q.splitn(3, ':').skip(1);
        let (Some(group), Some(year)) = (parts.next(), parts.next().and_then(|y| y.parse().ok())) else {
            panic!("{q}: class:GROUP:YEAR");
        };
        let car = Car { folder: String::new(), group: Some(group.to_owned()), year: Some(year) };
        let mm = class::classify(&car, true).restrictor.map_or(0.0, |d| d * 1000.0);
        println!("{q}:restrictor_mm {mm}");
    }
}
