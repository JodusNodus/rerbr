//! The brush's force dynamics at low speed on one wheel (ours, experimental): a quarter car (the wheel's share of the
//! mass, its inertia, a drive or brake torque) stepped explicitly at the game's 720 Hz, as NGP's wheel step takes the
//! force, for the longitudinal carcass stiffness choices and down to standstill.
//!
//! Research tooling: reads a `--tyre-probe` dump (wheel 0) for the tyre's tables, solves the brush's calibration as
//! the game does (`rbr_tyre::fit`), and prints a table per scenario. Nothing here changes what the game computes.
//!
//! Carcass choices: `ngp` NGP's torsional stiffness over R² scaled 0.1 at rest to 1 from 20 m/s (the brush before
//! 2026-11-08), `ngp-full` the same unscaled, `rill` Rill's passenger carcass (170 kN/m), `mf` [`brush::carcass_x`]
//! (MF 6.1's relaxation length, now). Dynamics: `floor` the rolling speed
//! floored at `V_MIN` in the deflection's decay as well (before), `rill` the transport speed restored (Rill's eqs.
//! 4.20-4.21, now).

use rbr_tyre::brush::{self, Calibration, EvalInput, TransState, V_MIN};
use rbr_tyre::Wheel;
use rbr_tyre_lab::probe::{parse_probe, Params};

const DT: f64 = 1.0 / 720.0;
const G: f64 = 9.81;

#[derive(Clone, Copy, PartialEq)]
enum Carcass {
    Ngp,
    NgpFull,
    Rill,
    Mf,
}

#[derive(Clone, Copy, PartialEq)]
enum Dyn {
    Floor,
    Rill,
}

struct Tyre {
    cal: Calibration,
    p: Params,
    fz: f64,
}

impl Tyre {
    fn r(&self) -> f64 {
        f64::from(self.p.effective_radius)
    }

    fn k_x(&self, c: Carcass, v: f64) -> f64 {
        let sf = f64::from(self.p.stiffness_factor);
        let full = f64::from(self.p.carcass_tor) * sf / (self.r() * self.r());
        match c {
            Carcass::Ngp => full * (0.1 + 0.9 * ((v - 10.0) * 0.1).clamp(0.0, 1.0)),
            Carcass::NgpFull => full,
            // Rill's passenger carcasses, 160 and 180 kN/m: their mean.
            Carcass::Rill => 170_000.0 * sf,
            Carcass::Mf => brush::carcass_x(&self.cal, self.r(), sf),
        }
    }

    fn k_y(&self) -> f64 {
        f64::from(self.p.carcass_lat) * f64::from(self.p.stiffness_factor)
    }

    /// The patch's longitudinal force (NGP sign: negative when driving) at car speed `v` and wheel spin `w`.
    fn force(&self, st: &mut TransState, c: Carcass, d: Dyn, v: f64, w: f64) -> f64 {
        let e = EvalInput {
            fz: self.fz,
            half_length: f64::from(self.p.patch_length) / 2.0,
            vx: -v,
            vy: 0.0,
            spin: w,
            radius: self.r(),
            bulk_temp: self.cal.t_cal,
            tread_remaining: 1.0,
            pressure: 0.0,
            pressure_factor: f64::from(self.p.stiffness_factor),
            wetness: 0.0,
            temp_grip: 1.0,
            surf_static: 1.0,
            surf_kinetic: 1.0,
        };
        let steady = brush::eval(&e, &self.cal);
        let vr = (w * self.r()).abs();
        let vr = if d == Dyn::Floor { vr.max(V_MIN) } else { vr };
        brush::step_dynamic(st, &steady, vr, self.k_x(c, v), self.k_y(), steady.mu * self.fz, DT).0
    }

    fn steady(&self, v: f64, w: f64) -> f64 {
        let mut st = TransState::default();
        let mut f = 0.0;
        for _ in 0..20000 {
            f = self.force(&mut st, Carcass::Mf, Dyn::Rill, v, w);
        }
        f
    }
}

fn wheel_of(p: &Params) -> Wheel {
    Wheel {
        tyre: 0,
        reference_friction: p.reference_friction,
        load_axis: p.load_axis.clone(),
        cornering_friction: p.crn_my.clone(),
        traction_friction: p.trc_my.clone(),
        cornering_peak_slip: p.crn_pk.clone(),
        traction_peak_slip: p.trc_pk.clone(),
        cornering_stiffness: p.crn_stf.clone(),
        traction_stiffness: p.trc_stf.clone(),
        temperature_axis: p.temp_axis.clone(),
        rubber_friction: p.rub_my.clone(),
    }
}

/// RMS of the second difference of `xs`: the step-to-step alternation.
fn hf(xs: &[f64]) -> f64 {
    let n = xs.len().saturating_sub(2).max(1) as f64;
    (xs.windows(3).map(|w| (w[2] - 2.0 * w[1] + w[0]).powi(2)).sum::<f64>() / n).sqrt()
}

/// Sign changes of `xs` about its mean of the last half (oscillation count).
fn crossings(xs: &[f64]) -> usize {
    let tail = &xs[xs.len() / 2..];
    let m = tail.iter().sum::<f64>() / tail.len() as f64;
    xs.windows(2).filter(|w| (w[0] - m) * (w[1] - m) < 0.0).count()
}

const CARCASSES: [(Carcass, &str); 4] =
    [(Carcass::Ngp, "ngp"), (Carcass::NgpFull, "ngp-full"), (Carcass::Rill, "rill"), (Carcass::Mf, "mf")];

fn main() {
    let mut probe = String::from("/tmp/tyre-lab/x.probe");
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--probe" => probe = args.next().expect("--probe PATH"),
            _ => panic!("unknown arg {a}"),
        }
    }
    let text = std::fs::read_to_string(&probe).unwrap_or_else(|e| panic!("{probe}: {e}"));
    let (dumps, _) = parse_probe(&text);
    let p = dumps
        .iter()
        .find(|d| d.wheel == 0 && d.load_axis.len() == 8 && d.normal_force > 0.0 && d.patch_length > 0.0)
        .expect("no wheel-0 dump")
        .clone();
    let cal = rbr_tyre::fit::calibration(&wheel_of(&p)).expect("calibration");
    let t = Tyre { cal, fz: f64::from(p.normal_force), p };
    let r = t.r();
    let m = t.fz / G;
    let mu_fz = t.steady(10.0, 10.0 * 1.15 / r).abs();
    println!(
        "wheel 0: Fz {:.0} N, R {r:.3} m, k_y {:.0} N/m, k_x ngp full {:.0} / rill {:.0} / mf {:.0} N/m, Cx(Fz) {:.0} N, peak Fx ~{mu_fz:.0} N",
        t.fz,
        t.k_y(),
        t.k_x(Carcass::NgpFull, 30.0),
        t.k_x(Carcass::Rill, 0.0),
        t.k_x(Carcass::Mf, 0.0),
        brush::stiffness_at(t.fz, cal.t_cal, 1.0, f64::from(t.p.stiffness_factor), cal.cx0, brush::M_CX, &cal),
    );

    // 1. Kinematic slip step: kappa 0.05 at fixed speeds, distance to 63 % of the steady force.
    println!("\n## Slip step kappa 0.05 (kinematic): distance to 63 % of the steady force, m");
    println!("| V m/s | ngp | ngp-full | rill | mf |\n| --- | --- | --- | --- | --- |");
    for v in [1.0, 2.0, 3.0, 5.0, 8.0, 15.0, 25.0] {
        let w = v * 1.05 / r;
        let target = t.steady(v, w);
        let cells: Vec<String> = CARCASSES
            .iter()
            .map(|&(c, _)| {
                let mut st = TransState::default();
                let n = (1..200000).find(|_| t.force(&mut st, c, Dyn::Rill, v, w) / target >= 0.63).unwrap_or(0);
                format!("{:.3}", n as f64 * DT * v)
            })
            .collect();
        println!("| {v} | {} |", cells.join(" | "));
    }

    // 2. The wheel-tyre mode: speed held, a constant drive torque of 0.3 of the peak force, wheel free from rolling.
    println!("\n## Wheel on a held speed, drive torque 0.3 peak Fx R from free rolling, explicit Euler at 720 Hz");
    println!("Per carcass: peak / settled wheel slip speed (m/s), the time it takes to settle within 10 % (s), the");
    println!(
        "oscillation's crossings of the settled value in 2 s, HF (RMS of the step-to-step second difference of wR over"
    );
    println!("the last second, m/s).\n");
    println!("| V m/s | inertia | ngp | ngp-full | rill | mf |\n| --- | --- | --- | --- | --- | --- |");
    for v in [0.5, 1.0, 2.0, 3.0, 5.0, 10.0] {
        for inertia in [0.6, 1.2, 4.0] {
            let cells: Vec<String> = CARCASSES
                .iter()
                .map(|&(c, _)| {
                    let mut st = TransState::default();
                    let mut w = v / r;
                    let torque = 0.3 * mu_fz * r;
                    let mut slip = Vec::new();
                    for _ in 0..1440 {
                        let fx = t.force(&mut st, c, Dyn::Rill, v, w);
                        w += DT * (torque + r * fx) / inertia;
                        slip.push(w * r - v);
                    }
                    let settled = slip[1080..].iter().sum::<f64>() / 360.0;
                    let peak = slip.iter().cloned().fold(f64::MIN, f64::max);
                    let band = 0.1 * settled.abs() + 1e-3;
                    let settle = slip.iter().rposition(|s| (s - settled).abs() > band).map_or(0, |k| k + 1);
                    format!(
                        "{peak:.2} / {settled:.3} / {:.2} / {} / {:.4}",
                        settle as f64 * DT,
                        crossings(&slip),
                        hf(&slip[720..])
                    )
                })
                .collect();
            println!("| {v} | {inertia} | {} |", cells.join(" | "));
        }
    }

    // 3. Pull-away from rest and 4. braking to a stop and holding on a grade, the brush down to standstill.
    println!("\n## Pull-away from rest, the brush alone down to 0 m/s (Rill's restored transport speed), mass Fz/g");
    println!("Drive torque ramped to T over 0.1 s, inertia 4 kg m² (in gear); 3 s.\n");
    println!("| T / (peak Fx R) | carcass | dyn | 0-3 m/s s | max slip speed m/s | 63 % force at m | HF slip m/s | Fx sign flips |");
    println!("| --- | --- | --- | --- | --- | --- | --- | --- |");
    for share in [0.5, 0.9, 1.3] {
        for &(c, cname) in &CARCASSES {
            for (d, dname) in [(Dyn::Floor, "floor"), (Dyn::Rill, "rill")] {
                let mut st = TransState::default();
                let (mut v, mut w, mut x) = (0.0f64, 0.0f64, 0.0f64);
                let tq = share * mu_fz * r;
                let (mut t3, mut smax, mut x63, mut flips) = (f64::NAN, 0.0f64, f64::NAN, 0);
                let (mut slips, mut last) = (Vec::new(), 0.0f64);
                let final_f = share.min(1.0) * mu_fz * 0.6;
                for n in 0..2160 {
                    let time = n as f64 * DT;
                    let torque = tq * (time / 0.1).min(1.0);
                    let fx = t.force(&mut st, c, d, v, w);
                    w += DT * (torque + r * fx) / 4.0;
                    v += DT * (-fx) / m;
                    x += v * DT;
                    if v >= 3.0 && t3.is_nan() {
                        t3 = time;
                    }
                    if x63.is_nan() && -fx >= final_f {
                        x63 = x;
                    }
                    if n > 72 && fx * last < 0.0 {
                        flips += 1;
                    }
                    last = fx;
                    smax = smax.max(w * r - v);
                    slips.push(w * r - v);
                }
                println!(
                    "| {share} | {cname} | {dname} | {t3:.2} | {smax:.2} | {x63:.3} | {:.4} | {flips} |",
                    hf(&slips[72..])
                );
            }
        }
    }

    println!("\n## Braking from 5 m/s to a stop, then held 3 s on a 10 % grade, the brush alone to 0 m/s");
    println!("Inertia 1.2 kg m²; brake torque B peak Fx R (static when the wheel stops); the grade pulls from the stop on.\n");
    println!("| B | carcass | dyn | stop s | stop m | locked s | HF slip in the last 1 m/s | moved 0-1 s after, mm | crept 1-3 s, mm | held Fx / grade force |");
    println!("| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |");
    for brake in [0.6, 2.0] {
        for &(c, cname) in &CARCASSES {
            for (d, dname) in [(Dyn::Floor, "floor"), (Dyn::Rill, "rill")] {
                let mut st = TransState::default();
                let (mut v, mut w, mut x) = (5.0f64, 5.0 / r, 0.0f64);
                let tb = brake * mu_fz * r;
                let (mut stop, mut stop_x, mut locked) = (None, 0.0, 0.0);
                let mut slips = Vec::new();
                let grade = m * G * 0.1f64.atan().sin();
                let mut fx = 0.0;
                let (mut x_stop, mut x_one) = (0.0, f64::NAN);
                for n in 0..(720 * 8) {
                    let time = n as f64 * DT;
                    let pull = if stop.is_some() { grade } else { 0.0 };
                    fx = t.force(&mut st, c, d, v, w);
                    // The brake holds the wheel when it can (static); otherwise it opposes the spin.
                    let free = w + DT * (r * fx) / 1.2;
                    let wtb = DT * tb / 1.2;
                    w = if free.abs() <= wtb { 0.0 } else { free - wtb * free.signum() };
                    v += DT * (-fx - pull) / m;
                    x += v * DT;
                    if v > 0.5 && w * r < 0.1 * v {
                        locked += DT;
                    }
                    match stop {
                        None => {
                            if v.abs() < 1.0 {
                                slips.push(w * r - v);
                            }
                            if v <= 0.01 && w == 0.0 {
                                stop = Some(time);
                                stop_x = x;
                                x_stop = x;
                            }
                        }
                        Some(at) if x_one.is_nan() && time > at + 1.0 => x_one = x,
                        Some(at) if time > at + 3.0 => break,
                        Some(_) => {}
                    }
                }
                println!(
                    "| {brake} | {cname} | {dname} | {:.2} | {stop_x:.2} | {locked:.2} | {:.4} | {:.1} | {:.2} | {:.3} |",
                    stop.unwrap_or(f64::NAN),
                    hf(&slips),
                    (x_one - x_stop) * 1000.0,
                    (x - x_one) * 1000.0,
                    -fx / grade
                );
            }
        }
    }
}
