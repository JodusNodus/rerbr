//! The realistic model on the synthetic samples (no game data): the calibration's identity, braking, idle, limiter,
//! overrun cut, spool, anti-lag, determinism and odd inputs.

use rbr_engine::bench::{self, inputs, Row};
use rbr_engine::calibrate;
use rbr_engine::engine::RPM;
use rbr_engine::realistic::Realistic;
use rbr_engine::{samples, Inputs, Model, Params};
use std::sync::OnceLock;

/// Every sample calibrated once.
fn models() -> &'static [(&'static str, Params, Realistic)] {
    static MODELS: OnceLock<Vec<(&'static str, Params, Realistic)>> = OnceLock::new();
    MODELS.get_or_init(|| {
        samples::all()
            .into_iter()
            .map(|(n, p)| {
                let m = Realistic::new(&p);
                (n, p, m)
            })
            .collect()
    })
}

fn rpm(w: f32) -> f64 {
    f64::from(w) * RPM
}

#[test]
fn steady_wot_reproduces_the_table_at_every_knot() {
    for (name, p, m) in models() {
        for &(w, tq) in &p.torque_knots {
            let n = rpm(w);
            if n < 150.0 {
                continue;
            }
            let want = f64::from(tq * p.max_torque);
            let got = calibrate::wot(m.spec(), n).1.torque;
            assert!((got - want).abs() <= 0.005 * want, "{name} {n:.0} rpm: {got:.2} for {want:.2}");
        }
    }
}

#[test]
fn closed_throttle_at_the_limit_brakes_with_the_brake_coefficient() {
    for (name, p, m) in models() {
        let c = m.calibration();
        if c.friction_scale != c.friction_scale_raw {
            continue;
        }
        let got = calibrate::closed(m.spec(), rpm(p.rev_limit)).1.torque;
        let want = -f64::from(p.brake_coefficient * p.max_torque);
        assert!((got - want).abs() <= 0.005 * want.abs(), "{name}: {got:.2} for {want:.2}");
    }
}

fn settled(rows: &[Row], from: f64) -> (f64, f64) {
    rows.iter().filter(|r| r.time >= from).fold((f64::MAX, f64::MIN), |(lo, hi), r| (lo.min(r.rpm), hi.max(r.rpm)))
}

#[test]
fn it_idles_in_neutral_and_recovers_from_a_dip() {
    for (name, p, m) in models() {
        let idle = rpm(p.idle_speed);
        let band = if m.calibration().class.unwrap().idle == rbr_engine::class::Idle::Ecu { 50.0 } else { 150.0 };
        let mut m = m.clone();
        let rows = bench::run(&mut m, idle * 1.3, Some(f64::from(p.inertia)), 12.0, |_| 0.0, Some((6.0, 300.0)));
        let (lo, hi) = settled(&rows[..(6.0 * 720.0) as usize], 4.0);
        assert!(lo > idle - band && hi < idle + band, "{name}: idles at {lo:.0}..{hi:.0} for {idle:.0}");
        let (lo, hi) = settled(&rows, 10.0);
        assert!(lo > idle - band && hi < idle + band, "{name}: after the dip {lo:.0}..{hi:.0} for {idle:.0}");
    }
}

#[test]
fn the_limiter_holds_wot_in_neutral() {
    for (name, p, m) in models() {
        let limit = rpm(p.rev_limit);
        let mut m = m.clone();
        let rows = bench::run(&mut m, rpm(p.idle_speed), Some(f64::from(p.inertia)), 6.0, |_| 1.0, None);
        let (lo, hi) = settled(&rows, 4.0);
        assert!(lo > limit - 100.0 && hi < limit + 250.0, "{name}: {lo:.0}..{hi:.0} at a {limit:.0} limit");
    }
}

#[test]
fn the_overrun_cut_never_drives() {
    for (name, p, m) in models() {
        // Anti-lag keeps firing, and a fixed-stop car has no overrun cut: its idle circuit keeps fuelling.
        let class = m.calibration().class.unwrap();
        if class.anti_lag || class.idle == rbr_engine::class::Idle::Stop {
            continue;
        }
        let idle = rpm(p.idle_speed);
        let mut n = idle + 900.0;
        while n < rpm(p.rev_limit) {
            let mut m = m.clone();
            let rows = bench::run(&mut m, n, None, 3.0, |t| if t < 1.0 { 1.0 } else { 0.0 }, None);
            let worst = rows.iter().filter(|r| r.time >= 1.0).map(|r| r.torque).fold(f64::MIN, f64::max);
            assert!(worst <= 0.0, "{name} at {n:.0}: {worst:.1}");
            n += 500.0;
        }
    }
}

#[test]
fn part_throttle_never_beats_wot_and_a_closed_pedal_holds_idle() {
    for (name, p, m) in models() {
        let spec = m.spec();
        let idle = rpm(p.idle_speed);
        let t = calibrate::part(spec, idle, 0.0).1.torque;
        assert!(t.abs() < 1.0, "{name}: closed pedal at idle {t:.1}");
        let mut n = 1000.0;
        while n <= rpm(p.rev_limit) {
            let wot = calibrate::wot(spec, n).1.torque;
            for k in 0..10 {
                let pedal = f64::from(k) / 10.0;
                let t = calibrate::part(spec, n, pedal).1.torque;
                assert!(t <= wot + 1e-3 * wot.abs() + 0.1, "{name} {n:.0} rpm pedal {pedal}: {t:.1} over WOT {wot:.1}");
            }
            n += 500.0;
        }
    }
}

/// Seconds from `from` until the torque first reaches `share` of the run's last value.
fn time_to(rows: &[Row], from: f64, share: f64) -> f64 {
    let last = rows.last().unwrap().torque;
    rows.iter().find(|r| r.time >= from && r.torque >= share * last).map_or(f64::INFINITY, |r| r.time - from)
}

#[test]
fn the_turbo_spools_like_gt_power_and_anti_lag_keeps_it() {
    for (name, p, m) in models() {
        if !p.turbo {
            continue;
        }
        let mut s = m.clone();
        let rows = bench::run(&mut s, 4000.0, None, 6.0, |t| if t >= 2.0 { 1.0 } else { 0.0 }, None);
        let t = time_to(&rows, 2.0, 0.9);
        assert!((0.4..=1.2).contains(&t), "{name}: spool to 90 % in {t:.2} s");
        if m.calibration().class.unwrap().anti_lag {
            let mut s = m.clone();
            let program = |t: f64| if (3.0..4.0).contains(&t) { 0.0 } else { 1.0 };
            let rows = bench::run(&mut s, 4500.0, None, 7.0, program, None);
            assert!(rows.iter().any(|r| r.anti_lag), "{name}: no anti-lag");
            let t = time_to(&rows, 4.0, 0.9);
            assert!(t < 0.15, "{name}: re-applied to 90 % in {t:.2} s");
        }
    }
}

fn bits(m: &mut Realistic, p: &Params) -> Vec<u32> {
    let idle = rpm(p.idle_speed);
    bench::run(m, idle, Some(f64::from(p.inertia)), 3.0, |t| if (0.5..1.5).contains(&t) { 1.0 } else { 0.0 }, None)
        .iter()
        .map(|r| (r.torque as f32).to_bits())
        .collect()
}

#[test]
fn it_is_deterministic_and_finite_for_any_input() {
    for (name, p, m) in models() {
        assert_eq!(bits(&mut m.clone(), p), bits(&mut m.clone(), p), "{name}");
        let mut m = m.clone();
        let odd = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.0, -1.0, -1e30, 1e30, f32::MIN_POSITIVE];
        for &v in &odd {
            for field in 0..9 {
                let mut i: Inputs = inputs(400.0, 0.5);
                match field {
                    0 => i.dt = v,
                    1 => i.speed = v,
                    2 => i.throttle = v,
                    3 => i.damage = v,
                    4 => i.pressure = v,
                    5 => i.temperature = v,
                    6 => i.friction = v,
                    7 => i.engine_temperature = v,
                    _ => {
                        i.dt = v;
                        i.speed = v;
                        i.throttle = v;
                    }
                }
                for _ in 0..50 {
                    let t = m.torque(&i);
                    assert!(t.is_finite(), "{name}: field {field} = {v}: {t}");
                }
                let t = m.telemetry();
                assert!(t.manifold_pressure.is_finite() && t.boost_pressure.is_finite() && t.turbo_speed.is_finite());
                let t = m.torque(&inputs(400.0, 0.5));
                assert!(t.is_finite(), "{name}: after field {field} = {v}: {t}");
            }
        }
        let _ = Realistic::new(&Params::default()).calibration();
    }
}
