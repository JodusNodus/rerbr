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

/// The table's torque at `n` rpm, N·m.
fn table(p: &Params, n: f64) -> f64 {
    let knots = p.torque_knots.iter().map(|&(w, q)| (rpm(w), f64::from(q * p.max_torque))).collect();
    rbr_engine::curve::Curve(knots).at(n)
}

/// The table is an upper bound everywhere (every 250 rpm and at the knots) and met at every knot except a turbo's
/// held ones, where the turbo cannot make the boost the table asks for. The rev limiter cuts from 200 rpm below
/// RPMLimit, so there the engine without it (the calibration's) meets the table.
#[test]
fn steady_wot_meets_the_table_on_boost_and_never_beats_it() {
    for (name, p, m) in models() {
        let held = &m.calibration().held;
        let cut = rpm(p.rev_limit) - rbr_engine::ecu::LIMITER_RANGE;
        let is_held = |n: f64| held.iter().any(|h| (h.0 - n).abs() < 1.0);
        for &(n, want, got) in m.calibration().wot.iter().filter(|k| k.0 >= 150.0 && !is_held(k.0)) {
            assert!(
                (got - want).abs() <= 0.005 * want,
                "{name} {n:.0} rpm without the limiter: {got:.2} for {want:.2}"
            );
        }
        let last = p.torque_knots.iter().map(|k| rpm(k.0)).fold(0.0, f64::max);
        let mut points: Vec<f64> = (1..).map(|i| f64::from(i) * 250.0).take_while(|&n| n <= last).collect();
        points.extend(p.torque_knots.iter().map(|k| rpm(k.0)).filter(|&n| n >= 150.0));
        for n in points {
            let want = table(p, n);
            let got = calibrate::wot(m.spec(), n).1.torque;
            assert!(got <= want + 0.001 * want.abs(), "{name} {n:.0} rpm: {got:.2} over the table's {want:.2}");
            let knot = p.torque_knots.iter().any(|k| (rpm(k.0) - n).abs() < 1e-6);
            if knot && n <= cut && !is_held(n) {
                assert!((got - want).abs() <= 0.005 * want, "{name} {n:.0} rpm: {got:.2} for {want:.2}");
            }
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
        assert!(lo > limit - 200.0 && hi < limit + 50.0, "{name}: {lo:.0}..{hi:.0} at a {limit:.0} limit");
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
        // Up to the limiter's range: inside it the cut, not the pedal, sets the torque.
        while n <= rpm(p.rev_limit) - rbr_engine::ecu::LIMITER_RANGE {
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

fn dbw_turbos() -> impl Iterator<Item = &'static (&'static str, Params, Realistic)> {
    models()
        .iter()
        .filter(|(_, p, m)| p.turbo && m.calibration().class.unwrap().actuation == rbr_engine::class::Actuation::Dbw)
}

/// The electronic gate's aim follows the plate, so a part pedal has one steady state whichever way it is approached.
#[test]
fn a_dbw_turbo_part_pedal_is_the_same_from_closed_and_from_wot() {
    for (name, p, m) in dbw_turbos() {
        let spec = m.spec();
        let mut n = (rpm(p.idle_speed) / 250.0).ceil() * 250.0;
        while n <= rpm(p.rev_limit) {
            for k in 0..=10 {
                let pedal = f64::from(k) / 10.0;
                let up = calibrate::part(spec, n, pedal).1.torque;
                let down = calibrate::steady(spec, &mut calibrate::wot(spec, n).0, n, pedal).torque;
                let band = 0.02 * up.abs().max(down.abs()) + 1.0;
                assert!(
                    (up - down).abs() <= band,
                    "{name} {n:.0} rpm pedal {pedal}: {up:.1} from closed, {down:.1} from WOT"
                );
            }
            n += 250.0;
        }
    }
}

/// A tuned electronic gate: a WOT tip-in at a fixed speed overshoots its steady torque by at most 5 % and stays within
/// 2 % of it from 1 s on (inferred, tuning practice).
#[test]
fn a_dbw_turbo_tip_in_barely_overshoots() {
    for (name, _, m) in dbw_turbos() {
        for n in [3000.0, 5000.0] {
            let mut m = m.clone();
            let rows = bench::run(&mut m, n, None, 5.0, |t| if t >= 1.0 { 1.0 } else { 0.0 }, None);
            let last = rows.last().unwrap().torque;
            let peak = rows.iter().map(|r| r.torque).fold(f64::MIN, f64::max);
            assert!(peak <= 1.05 * last, "{name} {n:.0} rpm: peak {peak:.1} over steady {last:.1}");
            let off = rows.iter().filter(|r| r.time >= 2.0).map(|r| (r.torque - last).abs()).fold(0.0, f64::max);
            assert!(off <= 0.02 * last, "{name} {n:.0} rpm: {off:.1} off steady {last:.1} after 1 s");
        }
    }
}

/// The most net torque over rpm (from idle to the limiter) rises with the pedal on every sample.
#[test]
fn the_most_torque_rises_with_the_pedal() {
    for (name, p, m) in models() {
        let from = (rpm(p.idle_speed) / 250.0).ceil() * 250.0;
        let to = rpm(p.rev_limit) - rbr_engine::ecu::LIMITER_RANGE;
        let most = |pedal: f64| {
            (0..)
                .map(|i| from + 250.0 * f64::from(i))
                .take_while(|&n| n <= to)
                .map(|n| calibrate::part(m.spec(), n, pedal).1.torque)
                .fold(f64::MIN, f64::max)
        };
        let row: Vec<f64> = (0..=10).map(|k| most(f64::from(k) / 10.0)).collect();
        assert!(row.windows(2).all(|w| w[1] > w[0]), "{name}: {row:.1?}");
    }
}

/// Seconds at a fixed `n` rpm from the steady pedal that gives 5 bar BMEP until 90 % of the steady WOT torque.
fn spool_at(m: &Realistic, n: f64) -> f64 {
    use rbr_engine::engine::{step, Step};
    let spec = m.spec();
    let aim = 5e5 * m.calibration().displacement / (4.0 * std::f64::consts::PI);
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..30 {
        let mid = 0.5 * (lo + hi);
        if calibrate::part(spec, n, mid).1.torque < aim {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let mut s = calibrate::part(spec, n, hi).0;
    let wot = calibrate::wot(spec, n).1.torque;
    let x = Step::at(n / RPM, 1.0, calibrate::DT);
    (1..=(5.0 / calibrate::DT) as usize)
        .find(|_| step(spec, &mut s, &x).torque >= 0.9 * wot)
        .map_or(f64::INFINITY, |i| i as f64 * calibrate::DT)
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
        // Chalmers (GT-Power, a road 1.5 L four, T45 at 1.16e-5 kg·m² against our 1e-5): at 1000 rpm from 5 bar BMEP
        // to 90 % of the steady WOT torque in 0.54 s (1.0 s on a three, 0.68 s with half the inertia). A rally turbo
        // makes almost no boost at 1000 rpm, so the same case also runs at the first 500 rpm step with 1.5 bar.
        let t = spool_at(m, 1000.0);
        assert!(t <= 1.2, "{name}: 5 bar BMEP to 90 % at 1000 rpm in {t:.2} s");
        let n = (2..).map(|i| f64::from(i) * 500.0).find(|&n| calibrate::wot(m.spec(), n).0.p_boost >= 1.5e5).unwrap();
        let t = spool_at(m, n);
        assert!((0.4..=1.2).contains(&t), "{name}: 5 bar BMEP to 90 % at {n:.0} rpm in {t:.2} s");
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

/// A lift from steady WOT at 5000 rpm: the blow-off valve opens within 0.1 s and vents the boost (it stays open while
/// the manifold is under vacuum, as a real one does); with anti-lag running it stays shut. The telemetry reports the
/// air's pressure.
#[test]
fn the_blow_off_valve_vents_a_lift_unless_anti_lag_runs() {
    use rbr_engine::engine::{step, Step};
    for (name, p, m) in models() {
        if !p.turbo {
            continue;
        }
        for anti_lag in [true, false] {
            let mut m = m.clone();
            m.spec_mut().class.anti_lag = anti_lag;
            let spec = m.spec();
            let (mut s, wot) = calibrate::wot(spec, 5000.0);
            assert_eq!(wot.blow_off, 0.0, "{name}: open at WOT");
            let lift = Step::at(5000.0 / RPM, 0.0, calibrate::DT);
            let open: Vec<f64> = (0..360).map(|_| step(spec, &mut s, &lift).blow_off).collect();
            if anti_lag {
                assert!(open.iter().all(|&o| o == 0.0), "{name}: open in anti-lag");
            } else {
                let first = open.iter().position(|&o| o >= 0.9);
                assert!(first.is_some_and(|k| k as f64 * calibrate::DT < 0.1), "{name}: opened late {first:?}");
                assert!(s.p_boost < 1.1e5, "{name}: {} Pa of boost left after 0.5 s", s.p_boost);
            }
        }
    }
    let (_, _, m) = models().iter().find(|(_, p, _)| p.turbo).unwrap();
    let mut m = m.clone();
    let x = Inputs {
        dt: 1.0 / 720.0,
        speed: 300.0,
        throttle: 1.0,
        damage: 1.0,
        pressure: 9e4,
        temperature: 290.0,
        engine_temperature: 360.0,
        ..Inputs::default()
    };
    m.torque(&x);
    assert_eq!(m.telemetry().air_pressure, 9e4);
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
        let mut restarted = m.clone();
        let first = bits(&mut restarted, p);
        restarted.restart();
        assert_eq!(bits(&mut restarted, p), first, "{name}: a restart is a new model");
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
