//! Each probed wheel through a window of a run (`--tyre-probe` dumps): the game's operating point and force, and the
//! steady answer of NGP's tyre and of a profile's model at the same inputs, with the brush's friction, ψ and sliding
//! speed and both tyres' peak μ_y and its slip angle at the wheel's load and speed. Prints one line per wheel and dump.
//! The right-hand wheels' (1, 3) contact frame is the left's turned half round (their contact velocity, spin and forces
//! all negated: α near 160-180° as read); α, κ and the force ratios print in the left wheels' frame.
//!
//! Research tooling (ours, experimental): for the circle's limit (`harness/limit_recovery.py`), which axle saturates
//! first and whether the model's rear gives less side force than NGP's at the same load and slip angle.
//!
//!   cargo run --release -p rbr-tyre-lab --example limit_frames -- --probe P --from STEP --to STEP [--model brush]

use std::collections::BTreeMap;

use rbr_tyre::brush::{self, EvalInput};
use rbr_tyre_lab::probe::{parse_probe, patch_at, wheel, Params};
use rbr_tyre_lab::steady::{grid, peak, Tyre};

/// The dumps' raw `slip_op`, `force_op` and `surface` words by (step, wheel).
fn raw(text: &str) -> BTreeMap<(u32, u32), [Vec<f32>; 2]> {
    let (mut out, mut step, mut wheel) = (BTreeMap::new(), 0, 0);
    let floats = |s: &str| s.split_whitespace().filter_map(|w| w.parse().ok()).collect::<Vec<f32>>();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("# step") {
            step = rest.split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("wheel:") {
            wheel = rest.split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("slip_op:") {
            out.entry((step, wheel)).or_insert([vec![], vec![]])[0] = floats(rest);
        } else if let Some(rest) = line.strip_prefix("force_op:") {
            out.entry((step, wheel)).or_insert([vec![], vec![]])[1] = floats(rest);
        }
    }
    out
}

fn main() {
    let (mut probe, mut from, mut to, mut model) = (String::new(), 0u32, u32::MAX, "brush".to_string());
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| panic!("{a} needs a value"));
        match a.as_str() {
            "--probe" => probe = value(),
            "--from" => from = value().parse().expect("--from STEP"),
            "--to" => to = value().parse().expect("--to STEP"),
            "--model" => model = value(),
            _ => panic!("unknown arg {a}"),
        }
    }
    let text = std::fs::read_to_string(&probe).unwrap_or_else(|e| panic!("{probe}: {e}"));
    let (dumps, cloud) = parse_probe(&text);
    let ops = raw(&text);
    println!(
        "# step wheel Fz alpha_deg kappa camber_deg | game Fy/Fz Fx/Fz | ngp Fy/Fz | {model} Fy/Fz mu psi vs | peak mu_y ngp {model} at Fz, v | their slip angles, deg"
    );
    for p in dumps.iter().filter(|d| (from..=to).contains(&d.step) && d.load_axis.len() == 8) {
        let Some([slip, force]) = ops.get(&(p.step, p.wheel)) else { continue };
        if slip.len() < 9 || force.len() < 2 || p.normal_force <= 0.0 {
            continue;
        }
        let (vlat, vlong, spin, camber) = (slip[1], slip[2], slip[3], slip[8]);
        let fz = p.normal_force;
        let v = vlat.hypot(vlong);
        // The printed frame: the right-hand wheels' turned back to the left's (the models take the dump's own).
        let side = if p.wheel % 2 == 1 { -1.0 } else { 1.0 };
        let alpha = (side * vlat).atan2(-side * vlong).to_degrees();
        let kappa = (side * (spin * p.effective_radius + vlong)) / (-side * vlong).max(0.5);
        let model_of = |name: &str, p: &Params| {
            rbr_tyre::model(name, &[wheel(p)], &rbr_tyre::Tuning::default()).unwrap_or_else(|e| panic!("{e}"))
        };
        let mk = |name: &str| Tyre {
            p,
            cloud: &cloud,
            soil: 0.0,
            model: model_of(name, p),
            speed: v,
            water: p.water_factor,
            camber,
        };
        let (mut n, mut m) = (mk("ngp"), mk(&model));
        let (_, fy_n) = n.forces(fz, vlat, vlong, spin);
        let (_, fy_m) = m.forces(fz, vlat, vlong, spin);
        let (peak_n, at_n) = peak(grid(0.5, 20.0, 0.25), |a| n.mu_y(fz, a));
        let (peak_m, at_m) = peak(grid(0.5, 20.0, 0.25), |a| m.mu_y(fz, a));
        // The brush's own friction, ψ and sliding speed at the frame, its input as the game's seam builds it.
        let cal = rbr_tyre::fit::calibration(&wheel(p), &rbr_tyre::Tuning::default()).unwrap_or_else(|e| panic!("{e}"));
        let x = EvalInput {
            fz: f64::from(fz),
            half_length: brush::trail_half_length(
                f64::from(patch_at(&cloud, fz, p.patch_length)) / 2.0,
                f64::from(fz),
                cal.fz0,
            ),
            vx: f64::from(vlong),
            vy: f64::from(vlat) + brush::camber_slip_velocity(f64::from(camber), f64::from(spin * p.effective_radius)),
            spin: f64::from(spin),
            radius: f64::from(p.effective_radius),
            bulk_temp: f64::from(p.tread_temp),
            tread_remaining: 1.0 - f64::from(p.wear_op),
            pressure: 0.0,
            pressure_factor: f64::from(p.stiffness_factor),
            wetness: f64::from(p.water_factor),
            hydroplaning: true,
            temp_grip: 1.0,
            surf_static: f64::from(p.static_friction),
            surf_kinetic: f64::from(p.static_friction),
        };
        let e = brush::eval(&x, &cal);
        let (vsx, vsy, _) = brush::slip_velocities(&x);
        println!(
            "{} {} {:.0} {:+.2} {:+.3} {:+.2} | {:+.3} {:+.3} | {:+.3} | {:+.3} {:.3} {:.2} {:.2} | {:.3} {:.3} {:.1} | {:.2} {:.2}",
            p.step,
            p.wheel,
            fz,
            alpha,
            kappa,
            camber.asin().to_degrees(),
            side * force[0] / fz,
            side * force[1] / fz,
            side * fy_n / fz,
            side * fy_m / fz,
            e.mu,
            e.psi,
            vsx.hypot(vsy),
            peak_n,
            peak_m,
            v,
            at_n,
            at_m
        );
    }
}
