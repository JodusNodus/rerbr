//! loose-v1 (`rbr_tyre::loose`, the game's tyre on soil under `--physics experimental-brush`) against NGP's sweep on a
//! loose-surface frame (gravel or snow) and the published loose-surface curves (`notes/tyre-model.md`, "Loose
//! surfaces: loose-v1").
//!
//! Research tooling (ours, experimental): reads a `--tyre-probe` dump of a run on soil, calibrates the wheel exactly as
//! the game does (`rbr_tyre::fit`), and prints for both models, at the probed surface and temperature on a flat road
//! at 15 m/s: μ_y against the slip angle (and over its peak), μ_x against the slip ratio driving and braking, a locked
//! wheel's side force against the slip angle, and the peaks at 2/4/6 kN. NGP's soil lateral force (∝ v_lat², added on
//! top of both in the game) is left out, as in `sweep`. Prints only.
//!
//! cargo run -q --release -p rbr-tyre-lab --example loose -- --probe FILE [--wheel N] [--fz N] [--speed M/S]

use rbr_physics::tyre::patch_forces::patch_forces;
use rbr_tyre::brush::EvalInput;
use rbr_tyre::loose::{self, Loose};
use rbr_tyre_lab::ngp::{blend_at, patch_input};
use rbr_tyre_lab::probe::{parse_probe, wheel, Params};

struct Lab {
    p: Params,
    loose: Loose,
    v: f64,
}

impl Lab {
    /// (|Fy|, Fx) over Fz of NGP or loose-v1 at the slip angle `alpha` (deg) and slip ratio `kappa` (-1 locked); Fx
    /// positive braking.
    fn mu(&self, ngp: bool, fz: f64, alpha: f64, kappa: f64) -> (f64, f64) {
        let p = &self.p;
        let r = f64::from(p.effective_radius);
        let a = alpha.to_radians();
        let (vlat, vlong) = (self.v * a.sin(), -self.v * a.cos());
        let spin = (1.0 + kappa) * -vlong / r;
        let (fy, fx) = if ngp {
            let b = blend_at(p, fz as f32);
            let n =
                patch_forces(&patch_input(p, &b, fz as f32, p.patch_length, vlat as f32, vlong as f32, spin as f32));
            (f64::from(n.lateral_force), f64::from(n.longitudinal_force))
        } else {
            let x = EvalInput {
                fz,
                half_length: f64::from(p.patch_length) / 2.0,
                vx: vlong,
                vy: vlat,
                spin,
                radius: r,
                bulk_temp: f64::from(p.tread_temp),
                tread_remaining: 1.0 - f64::from(p.wear_op),
                pressure: 0.0,
                pressure_factor: f64::from(p.stiffness_factor),
                wetness: 0.0,
                hydroplaning: false,
                temp_grip: 1.0,
                surf_static: f64::from(p.static_friction),
                surf_kinetic: f64::from(p.static_friction),
            };
            let o = loose::eval(&x, &self.loose);
            (o.fy, o.fx)
        };
        (fy.abs() / fz, fx / fz)
    }

    fn lat_peak(&self, ngp: bool, fz: f64) -> (f64, f64) {
        (0..=160).map(|k| f64::from(k) * 0.25).map(|a| (self.mu(ngp, fz, a, 0.0).0, a)).fold((0.0, 0.0), |m, x| {
            if x.0 > m.0 {
                x
            } else {
                m
            }
        })
    }

    fn lon_peak(&self, ngp: bool, fz: f64, sign: f64) -> (f64, f64) {
        (0..=50).map(|k| sign * f64::from(k) * 0.02).map(|k| ((self.mu(ngp, fz, 0.0, k).1).abs(), k.abs())).fold(
            (0.0, 0.0),
            |m, x| {
                if x.0 > m.0 {
                    x
                } else {
                    m
                }
            },
        )
    }
}

fn main() {
    let (mut probe, mut wheel_no, mut fz, mut v) = (String::new(), 0u32, 4000.0, 15.0);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| panic!("{a} needs a value"));
        match a.as_str() {
            "--probe" => probe = value(),
            "--wheel" => wheel_no = value().parse().expect("--wheel N"),
            "--fz" => fz = value().parse().expect("--fz N"),
            "--speed" => v = value().parse().expect("--speed M/S"),
            _ => panic!("unknown arg {a}"),
        }
    }
    let text = std::fs::read_to_string(&probe).unwrap_or_else(|e| panic!("{probe}: {e}"));
    let (dumps, _) = parse_probe(&text);
    let p = dumps
        .iter()
        .rev()
        .find(|d| d.wheel == wheel_no && d.normal_force > 0.0 && d.patch_length > 0.0)
        .expect("no dump of the wheel in contact")
        .clone();
    let w = wheel(&p);
    let cal = rbr_tyre::fit::calibration(&w).expect("calibration");
    let loose = rbr_tyre::fit::loose(&w, &cal).expect("loose calibration");
    println!(
        "step {} wheel {wheel_no}: static {:.3} kinetic {:.3} (after the temperature grip {:.3}), tread {:.1} K, {fz} N, {v} m/s",
        p.step, p.static_friction, p.kinetic_friction, p.temperature_grip, p.tread_temp
    );
    let lab = Lab { p, loose, v };

    let peaks = [true, false].map(|ngp| lab.lat_peak(ngp, fz));
    println!("\n## μ_y against slip angle (free rolling); over peak; Coutermarsh pea stone / wet gravel over peak");
    println!("| α° | NGP | loose-v1 | NGP/peak | loose/peak | published/peak |\n| --- | --- | --- | --- | --- | --- |");
    // Coutermarsh & Shoop Fig. 15 (fig ±0.004): pea stone 0.30/0.44/0.52/0.54, wet gravel 0.22/0.35/0.48/0.53 at
    // 5/10/20/30°, max 0.54 / 0.55, still rising at 38°.
    let published = |a: f64| match a as i32 {
        5 => "0.56 / 0.40",
        10 => "0.81 / 0.64",
        20 => "0.96 / 0.87",
        30 => "1.00 / 0.96",
        35..=40 => "~1, no fall to 38°",
        _ => "",
    };
    for a in [1.0, 2.5, 5.0, 7.5, 10.0, 15.0, 20.0, 25.0, 30.0, 35.0, 40.0] {
        let (n, l) = (lab.mu(true, fz, a, 0.0).0, lab.mu(false, fz, a, 0.0).0);
        println!("| {a} | {n:.3} | {l:.3} | {:.3} | {:.3} | {} |", n / peaks[0].0, l / peaks[1].0, published(a));
    }
    println!("peak μ_y: NGP {:.3} at {}°, loose-v1 {:.3} at {}°", peaks[0].0, peaks[0].1, peaks[1].0, peaks[1].1);

    let drive = [true, false].map(|ngp| lab.lon_peak(ngp, fz, 1.0));
    println!("\n## μ_x against slip ratio, driving (+) and braking (-); over the driving peak");
    println!("| κ | NGP | loose-v1 | NGP/peak | loose/peak |\n| --- | --- | --- | --- | --- |");
    for k in [0.05, 0.1, 0.2, 0.3, 0.5, 1.0, -0.05, -0.1, -0.2, -0.3, -0.5, -0.7, -0.9, -1.0] {
        let (n, l) = (lab.mu(true, fz, 0.0, k).1.abs(), lab.mu(false, fz, 0.0, k).1.abs());
        println!("| {k} | {n:.3} | {l:.3} | {:.3} | {:.3} |", n / drive[0].0, l / drive[1].0);
    }
    println!(
        "driving peak μ_x: NGP {:.3} at κ {}, loose-v1 {:.3} at κ {}; locked over the driving peak NGP {:.3}, loose-v1 {:.3} \
         (published 1.25-1.30 car; Coutermarsh locked 0.85, 1.25-1.47 times the traction, 1.5 times the lateral max)",
        drive[0].0,
        drive[0].1,
        drive[1].0,
        drive[1].1,
        lab.mu(true, fz, 0.0, -1.0).1.abs() / drive[0].0,
        lab.mu(false, fz, 0.0, -1.0).1.abs() / drive[1].0
    );

    println!("\n## Locked wheel: μ_y against slip angle, and over the free rolling lateral peak (Coutermarsh 0.72 sin α over 0.55: 1.31 sin α)");
    println!("| α° | NGP | loose-v1 | NGP/peak | loose/peak |\n| --- | --- | --- | --- | --- |");
    for a in [5.0, 10.0, 20.0, 35.0] {
        let (n, l) = (lab.mu(true, fz, a, -1.0).0, lab.mu(false, fz, a, -1.0).0);
        println!("| {a} | {n:.3} | {l:.3} | {:.3} | {:.3} |", n / peaks[0].0, l / peaks[1].0);
    }

    println!("\n## Peaks against load (μ_y, μ_x driving)");
    println!("| Fz N | NGP | loose-v1 |\n| --- | --- | --- |");
    for f in [2000.0, 4000.0, 6000.0] {
        let (n, l) = (
            (lab.lat_peak(true, f).0, lab.lon_peak(true, f, 1.0).0),
            (lab.lat_peak(false, f).0, lab.lon_peak(false, f, 1.0).0),
        );
        println!("| {f} | {:.3} / {:.3} | {:.3} / {:.3} |", n.0, n.1, l.0, l.1);
    }
}
